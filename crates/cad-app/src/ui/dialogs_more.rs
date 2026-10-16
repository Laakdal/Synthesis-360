//! Panels of the body-level modelling dialogs: Shell, Combine, Mirror, Rectangular / Circular
//! Pattern and Move/Copy (same layout conventions as the other feature dialogs).

use super::{combo, expr_row, feature_param, row, selection_row, static_row, Action, ACCENT, CONTROL, LABEL_W};
use crate::state::features::{BodyInput, Dialog, COMBINE_OPERATIONS};
use crate::state::AppState;
use cad_core::{OriginAxis, OriginPlane, PlaneRef};
use egui::{Color32, RichText, Ui};

fn axes() -> Vec<(OriginAxis, String)> {
    vec![(OriginAxis::X, "X Axis".into()), (OriginAxis::Y, "Y Axis".into()), (OriginAxis::Z, "Z Axis".into())]
}

/// A selection input that can be made active (Fusion: the highlighted input receives clicks).
fn input_row(ui: &mut Ui, label: &str, count: usize, active: bool, actions: &mut Vec<Action>) -> bool {
    let mut clicked = false;
    row(ui, label, |ui| {
        let text = if count == 0 { "Select".to_string() } else { format!("{count} selected") };
        let fill = if active { ACCENT } else { CONTROL };
        let r = ui
            .add(egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(fill).min_size(egui::vec2(110.0, 20.0)))
            .on_hover_text("Click to make this input active, then click items in the canvas");
        clicked = r.clicked();
        if active && count > 0 && ui.add(egui::Button::new("\u{00d7}").frame(false)).on_hover_text("Clear selection").clicked() {
            actions.push(Action::ClearInputs);
        }
    });
    clicked
}

fn checkbox_row(ui: &mut Ui, label: &str, value: &mut bool) -> bool {
    row(ui, label, |ui| ui.checkbox(value, "").changed())
}

fn plane_text(state: &AppState, p: &Option<PlaneRef>) -> String {
    match p {
        None => "Select a plane or planar face".to_string(),
        Some(PlaneRef::Origin { plane }) => format!("{plane:?} Plane"),
        Some(PlaneRef::Construction { feature }) => state.doc.feature(feature).map(|f| f.name.clone()).unwrap_or_default(),
        Some(PlaneRef::Face { .. }) => "Planar face".to_string(),
    }
}

/// Draws the body of one of this module's dialogs; returns whether a value changed.
pub(super) fn body(ui: &mut Ui, state: &AppState, d: &mut Dialog, actions: &mut Vec<Action>) -> bool {
    let mut changed = false;
    match d {
        Dialog::Shell(x) => {
            selection_row(ui, "Faces", x.faces.len(), actions);
            static_row(ui, "Direction", "Inside");
            changed |= expr_row(ui, "Inside Thickness", &mut x.thickness, feature_param(state, &x.s, 0));
        }
        Dialog::Combine(x) => {
            if input_row(ui, "Target Body", x.target.is_some() as usize, x.active == BodyInput::First, actions) {
                x.active = BodyInput::First;
            }
            if input_row(ui, "Tool Bodies", x.tools.len(), x.active == BodyInput::Second, actions) {
                x.active = BodyInput::Second;
            }
            let ops: Vec<_> = COMBINE_OPERATIONS.iter().map(|o| (*o, o.label().to_string())).collect();
            row(ui, "Operation", |ui| {
                changed |= combo(ui, "combine_op", &mut x.operation, &ops);
            });
            changed |= checkbox_row(ui, "Keep Tools", &mut x.keep_tools);
        }
        Dialog::Mirror(x) => {
            static_row(ui, "Type", "Bodies");
            if input_row(ui, "Objects", x.bodies.len(), x.active == BodyInput::First, actions) {
                x.active = BodyInput::First;
            }
            if input_row(ui, "Mirror Plane", x.plane.is_some() as usize, x.active == BodyInput::Second, actions) {
                x.active = BodyInput::Second;
            }
            static_row(ui, "", &plane_text(state, &x.plane));
            row(ui, "Origin Plane", |ui| {
                for p in [OriginPlane::XY, OriginPlane::XZ, OriginPlane::YZ] {
                    let sel = x.plane == Some(PlaneRef::Origin { plane: p });
                    if ui.add(egui::Button::new(format!("{p:?}")).selected(sel).min_size(egui::vec2(40.0, 20.0))).clicked() && !sel {
                        x.plane = Some(PlaneRef::Origin { plane: p });
                        changed = true;
                    }
                }
            });
        }
        Dialog::RectangularPattern(x) => {
            static_row(ui, "Pattern Type", "Bodies");
            selection_row(ui, "Objects", x.bodies.len(), actions);
            row(ui, "Direction 1", |ui| {
                changed |= combo(ui, "rp_axis1", &mut x.axis1, &axes());
            });
            changed |= expr_row(ui, "Quantity", &mut x.quantity1, feature_param(state, &x.s, 0));
            changed |= expr_row(ui, "Distance", &mut x.distance1, feature_param(state, &x.s, 1));
            ui.separator();
            let mut dir2: Vec<(Option<OriginAxis>, String)> = vec![(None, "None".into())];
            dir2.extend(axes().into_iter().filter(|(a, _)| *a != x.axis1).map(|(a, l)| (Some(a), l)));
            row(ui, "Direction 2", |ui| {
                changed |= combo(ui, "rp_axis2", &mut x.axis2, &dir2);
            });
            if x.axis2.is_some() {
                changed |= expr_row(ui, "Quantity", &mut x.quantity2, feature_param(state, &x.s, 2));
                changed |= expr_row(ui, "Distance", &mut x.distance2, feature_param(state, &x.s, 3));
            }
            ui.separator();
            row(ui, "Operation", |ui| {
                changed |= combo(ui, "rp_join", &mut x.join, &[(false, "New Body".into()), (true, "Join".into())]);
            });
        }
        Dialog::CircularPattern(x) => {
            static_row(ui, "Pattern Type", "Bodies");
            selection_row(ui, "Objects", x.bodies.len(), actions);
            row(ui, "Axis", |ui| {
                changed |= combo(ui, "cp_axis", &mut x.axis, &axes());
            });
            changed |= expr_row(ui, "Quantity", &mut x.quantity, feature_param(state, &x.s, 0));
            changed |= expr_row(ui, "Total Angle", &mut x.angle, feature_param(state, &x.s, 1));
            row(ui, "Operation", |ui| {
                changed |= combo(ui, "cp_join", &mut x.join, &[(false, "New Body".into()), (true, "Join".into())]);
            });
        }
        Dialog::Move(x) => {
            static_row(ui, "Move Object", "Bodies");
            selection_row(ui, "Bodies", x.bodies.len(), actions);
            changed |= expr_row(ui, "X Distance", &mut x.dx, feature_param(state, &x.s, 0));
            changed |= expr_row(ui, "Y Distance", &mut x.dy, feature_param(state, &x.s, 1));
            changed |= expr_row(ui, "Z Distance", &mut x.dz, feature_param(state, &x.s, 2));
            row(ui, "Rotation Axis", |ui| {
                changed |= combo(ui, "mv_axis", &mut x.axis, &axes());
            });
            changed |= expr_row(ui, "Angle", &mut x.angle, feature_param(state, &x.s, 3));
            changed |= checkbox_row(ui, "Create Copy", &mut x.copy);
            ui.horizontal(|ui| {
                ui.add_space(LABEL_W + 6.0);
                ui.label(RichText::new("Rotates about the origin axis, then translates.").color(super::DIM).size(11.5));
            });
        }
        _ => {}
    }
    changed
}
