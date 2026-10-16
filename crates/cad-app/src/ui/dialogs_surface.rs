//! Panels of the SURFACE / MESH / UTILITIES dialogs: Surface Extrude / Revolve, Patch, Stitch,
//! Offset, Thicken, Insert Mesh and 3D Print (same layout conventions as the other dialogs).

use super::{combo, expr_row, feature_param, row, selection_row, static_row, Action, DIM, LABEL_W};
use crate::state::features::{Session, SurfaceDialog};
use crate::state::AppState;
use cad_core::{ExtentDirection, FeatureId, RevolveAxis};
use cad_kernel::mesh::Refinement;
use egui::{RichText, Ui};

fn note(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(LABEL_W + 6.0);
        ui.add(egui::Label::new(RichText::new(text).color(DIM).size(11.5)).wrap());
    });
}

/// Sketch chooser plus the sketch's curve chains (check boxes). Returns whether anything changed.
fn curve_inputs(ui: &mut Ui, state: &AppState, s: &Session, sketch: &mut Option<FeatureId>, curves: &mut Vec<String>, salt: &str) -> (bool, bool) {
    let mut changed = false;
    let mut sketch_changed = false;
    let choices: Vec<(Option<FeatureId>, String)> = std::iter::once((None, "Select a sketch".to_string())).chain(state.surface_sketch_choices(&s.base.0).into_iter().map(|(id, n)| (Some(id), n))).collect();
    row(ui, "Sketch", |ui| {
        if combo(ui, &format!("{salt}_sketch"), sketch, &choices) {
            sketch_changed = true;
        }
    });
    if sketch_changed {
        curves.clear();
        if let Some(sk) = sketch.as_deref() {
            for c in state.sketch_curve_chains(&s.base.0, sk) {
                curves.extend(c.curves);
            }
        }
        changed = true;
    }
    let mut dummy = Vec::new();
    selection_row(ui, "Curves", curves.len(), &mut dummy);
    if let Some(sk) = sketch.as_deref() {
        let chains = state.sketch_curve_chains(&s.base.0, sk);
        egui::ScrollArea::vertical().id_salt(format!("{salt}_chains")).max_height(110.0).show(ui, |ui| {
            for (i, c) in chains.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_space(LABEL_W + 6.0);
                    let mut on = c.curves.iter().all(|id| curves.contains(id));
                    let text = format!("Chain {} \u{00b7} {} curve{} \u{00b7} {} \u{00b7} {:.1} mm", i + 1, c.curves.len(), if c.curves.len() == 1 { "" } else { "s" }, if c.closed { "closed" } else { "open" }, c.length());
                    if ui.checkbox(&mut on, RichText::new(text).size(12.0)).changed() {
                        if on {
                            for id in &c.curves {
                                if !curves.contains(id) {
                                    curves.push(id.clone());
                                }
                            }
                        } else {
                            curves.retain(|id| !c.curves.contains(id));
                        }
                        changed = true;
                    }
                });
            }
        });
    } else {
        note(ui, "Click a profile of a sketch, pick a sketch in the timeline, or choose one above.");
    }
    (changed, sketch_changed)
}

/// Draws the body of one of this module's dialogs; returns whether a value changed.
pub(super) fn body(ui: &mut Ui, state: &AppState, d: &mut SurfaceDialog, actions: &mut Vec<Action>) -> bool {
    let mut changed = false;
    match d {
        SurfaceDialog::Extrude(x) => {
            static_row(ui, "Type", "Surface");
            let (c, _) = curve_inputs(ui, state, &x.s, &mut x.sketch, &mut x.curves, "sext");
            changed |= c;
            static_row(ui, "Start", "Profile Plane");
            #[derive(Clone, PartialEq)]
            enum Side {
                One,
                Sym,
            }
            let mut side = if x.direction == ExtentDirection::Symmetric { Side::Sym } else { Side::One };
            row(ui, "Direction", |ui| {
                let w = ui.available_width().max(60.0);
                ui.scope(|ui| {
                    ui.set_max_width(if side == Side::One { w - 52.0 } else { w });
                    if combo(ui, "sext_dir", &mut side, &[(Side::One, "One Side".into()), (Side::Sym, "Symmetric".into())]) {
                        x.direction = if side == Side::Sym { ExtentDirection::Symmetric } else { ExtentDirection::Normal };
                        changed = true;
                    }
                });
                if side == Side::One {
                    let flipped = x.direction == ExtentDirection::Reverse;
                    if ui.add(egui::Button::new("Flip").selected(flipped).min_size(egui::vec2(44.0, 20.0))).on_hover_text("Flip the extrude direction").clicked() {
                        x.direction = if flipped { ExtentDirection::Normal } else { ExtentDirection::Reverse };
                        changed = true;
                    }
                }
            });
            changed |= expr_row(ui, "Distance", &mut x.distance, feature_param(state, &x.s, 0));
        }
        SurfaceDialog::Revolve(x) => {
            static_row(ui, "Type", "Surface");
            let (c, sketch_changed) = curve_inputs(ui, state, &x.s, &mut x.sketch, &mut x.curves, "srev");
            changed |= c;
            if sketch_changed {
                x.axis = state.default_axis(x.sketch.as_deref(), &x.s.base.0);
            }
            let choices: Vec<(Option<RevolveAxis>, String)> =
                state.revolve_axis_choices(x.sketch.as_deref(), &x.s.base.0).into_iter().map(|a| (Some(a.clone()), crate::state::features::axis_label(&a))).collect();
            row(ui, "Axis", |ui| {
                changed |= combo(ui, "srev_axis", &mut x.axis, &choices);
            });
            static_row(ui, "Type", "Angle");
            changed |= expr_row(ui, "Angle", &mut x.angle, feature_param(state, &x.s, 0));
        }
        SurfaceDialog::Patch(x) => {
            selection_row(ui, "Boundary", x.profiles.len() + x.edges.len(), actions);
            note(ui, "Closed sketch profiles, or edges forming one closed loop (planar or not).");
            static_row(ui, "Continuity", "Connected (G0)");
        }
        SurfaceDialog::Stitch(x) => {
            selection_row(ui, "Stitch Surfaces", x.bodies.len(), actions);
            changed |= expr_row(ui, "Tolerance", &mut x.tolerance, feature_param(state, &x.s, 0));
            note(ui, "A closed result becomes a solid body.");
        }
        SurfaceDialog::Offset(x) => {
            selection_row(ui, "Faces", x.faces.len(), actions);
            changed |= expr_row(ui, "Offset Distance", &mut x.value, feature_param(state, &x.s, 0));
            static_row(ui, "Operation", "New Body");
        }
        SurfaceDialog::Thicken(x) => {
            selection_row(ui, "Faces", x.faces.len(), actions);
            changed |= expr_row(ui, "Thickness", &mut x.value, feature_param(state, &x.s, 0));
            row(ui, "Direction", |ui| {
                changed |= combo(ui, "thk_dir", &mut x.symmetric, &[(false, "One Side".into()), (true, "Symmetric".into())]);
            });
            static_row(ui, "Operation", "New Body");
        }
        SurfaceDialog::Mesh(x) => {
            row(ui, "Mesh File", |ui| {
                let text = if x.source.is_empty() { "Select File...".to_string() } else { x.source.clone() };
                let editable = x.s.feature.is_none();
                let r = ui.add_enabled(editable, egui::Button::new(text).min_size(egui::vec2(140.0, 20.0)));
                if r.on_hover_text("Choose an STL file (binary or ASCII)").clicked() {
                    if let Some(p) = rfd::FileDialog::new().set_title("Insert Mesh").add_filter("STL mesh", &["stl"]).pick_file() {
                        x.file = Some(p);
                        changed = true;
                    }
                }
            });
            if x.triangles > 0 {
                static_row(ui, "Triangles", &x.triangles.to_string());
            }
            static_row(ui, "Units", "Millimeter");
            row(ui, "Convert to B-rep", |ui| {
                changed |= ui.checkbox(&mut x.to_brep, "").on_hover_text("Sew the triangles into a solid so solid features (fillet, combine ...) work on it").changed();
            });
            note(ui, if x.to_brep { "Coplanar triangles are merged into planar faces." } else { "Displayed as a mesh body." });
        }
        SurfaceDialog::Print(x) => {
            let mut dummy = Vec::new();
            selection_row(ui, "Bodies", x.bodies.len(), if x.bodies.is_empty() { &mut dummy } else { actions });
            if x.bodies.is_empty() {
                note(ui, "None selected: every visible body is exported.");
            }
            let options: Vec<(Refinement, String)> = Refinement::ALL.iter().map(|r| (*r, r.label().to_string())).collect();
            row(ui, "Refinement", |ui| {
                changed |= combo(ui, "print_ref", &mut x.refinement, &options);
            });
            row(ui, "Format", |ui| {
                changed |= combo(ui, "print_fmt", &mut x.ascii, &[(false, "Binary".into()), (true, "ASCII".into())]);
            });
            static_row(ui, "Output", "Save to STL file");
        }
    }
    changed
}
