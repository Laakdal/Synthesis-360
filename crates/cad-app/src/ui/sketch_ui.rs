//! Sketch Palette (Fusion F10 layout), dimension labels with inline value entry, the
//! over-constrained / driven-dimension offer (F18) and constraint glyphs over the canvas.
//!
//! Labels and glyphs are painted (not widgets) so canvas clicks and drags reach the sketch
//! tools; hit-testing of labels and glyphs happens in `AppState::sketch_hit`. The value box and
//! the driven offer are the only interactive widgets drawn over the canvas.
use crate::state::command::CommandId;
use crate::state::sketch::{ConstraintTool, SketchPrompt, SketchTool, OVER_CONSTRAINED};
use crate::state::{AppState, Pick};
use egui::{Align2, Color32, CornerRadius, FontId, Key, Margin, RichText, Stroke, TextEdit, Vec2};

const PANEL_FILL: Color32 = Color32::from_rgb(0x2b, 0x2e, 0x34);
const PANEL_STROKE: Color32 = Color32::from_rgb(0x3c, 0x40, 0x48);
const TEXT: Color32 = Color32::from_rgb(0xdc, 0xde, 0xe3);
const MUTED: Color32 = Color32::from_rgb(0x9a, 0xa0, 0xaa);
const ACCENT: Color32 = Color32::from_rgb(0x4d, 0x9c, 0xff);
const OK: Color32 = Color32::from_rgb(0x8c, 0xd1, 0x7d);
const ERROR: Color32 = Color32::from_rgb(0xff, 0x70, 0x66);
const LABEL_BG: Color32 = Color32::from_rgba_premultiplied(24, 26, 30, 215);

/// Text glyph with an ASCII fallback when the UI font lacks the symbol.
fn glyph_text(ctx: &egui::Context, s: &'static str, font: &FontId) -> &'static str {
    if ctx.fonts_mut(|f| f.has_glyphs(font, s)) {
        return s;
    }
    match s {
        "⊙" => "o",
        "∥" => "//",
        "⊥" => "_|_",
        "⋯" => "--",
        "◎" => "()",
        "Δ" => "M",
        "🔒" => "F",
        "↔" => "<>",
        other => other,
    }
}

fn option_row(ui: &mut egui::Ui, label: &str, value: &mut bool, tip: &str) {
    ui.label(RichText::new(label).color(TEXT));
    ui.checkbox(value, "").on_hover_text(tip);
    ui.end_row();
}

/// Sketch Palette panel (right side of the canvas while sketching).
pub fn palette(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(overlay) = state.sketch_overlay() else { return };
    let Some(session) = state.active_sketch() else { return };
    let tool = session.tool;
    let hint = state.sketch_tool_hint();
    let fully = overlay.status == "Fully constrained";
    let failed = overlay.status.starts_with("Sketch cannot");
    let selected_curves = state.selection.iter().filter(|p| matches!(p, Pick::SketchEntity(id) if overlay.curves.iter().any(|c| &c.id == id))).count();

    egui::Frame::new()
        .fill(PANEL_FILL)
        .stroke(Stroke::new(1.0, PANEL_STROKE))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width().min(280.0));
            ui.set_max_width(ui.available_width().min(280.0));
            ui.visuals_mut().override_text_color = Some(TEXT);
            ui.horizontal(|ui| {
                ui.label(RichText::new("—").color(MUTED));
                ui.label(RichText::new("SKETCH PALETTE").size(12.5).strong().color(TEXT));
            });
            ui.add_space(2.0);

            egui::CollapsingHeader::new(RichText::new("Options").color(TEXT)).default_open(true).show(ui, |ui| {
                egui::Grid::new("sketch-palette-options").num_columns(2).spacing([16.0, 5.0]).min_col_width(110.0).show(ui, |ui| {
                    ui.label(RichText::new("Linetype").color(TEXT));
                    let lt = ui
                        .add_enabled(selected_curves > 0, egui::Button::new(RichText::new("Construction").size(11.5)).small())
                        .on_hover_text("Toggles the selected curves between normal and construction geometry (X)")
                        .on_disabled_hover_text("Select sketch curves first");
                    if lt.clicked() {
                        state.execute(CommandId::ToggleConstruction);
                    }
                    ui.end_row();

                    ui.label(RichText::new("Look At").color(TEXT));
                    if ui.add(egui::Button::new(RichText::new("Look At").size(11.5)).small()).on_hover_text("Looks straight at the sketch plane").clicked() {
                        state.execute(CommandId::LookAt);
                    }
                    ui.end_row();

                    let st = &mut state.settings;
                    option_row(ui, "Sketch Grid", &mut st.grid, "Shows the sketch grid");
                    option_row(ui, "Snap", &mut st.snap, "Snaps the cursor to the grid");
                    option_row(ui, "Profile", &mut st.show_profiles, "Shades closed profiles");
                    option_row(ui, "Points", &mut st.show_points, "Shows sketch points");
                    option_row(ui, "Dimensions", &mut st.show_dimensions, "Shows sketch dimensions");
                    option_row(ui, "Constraints", &mut st.show_constraints, "Shows constraint glyphs");
                });
            });

            if tool == SketchTool::Polygon {
                if let Some((mut sides, mut inscribed)) = state.polygon_options() {
                    egui::CollapsingHeader::new(RichText::new("Polygon").color(TEXT)).default_open(true).show(ui, |ui| {
                        egui::Grid::new("sketch-palette-polygon").num_columns(2).spacing([16.0, 5.0]).min_col_width(110.0).show(ui, |ui| {
                            ui.label(RichText::new("Sides").color(TEXT));
                            ui.add(egui::DragValue::new(&mut sides).range(3..=64)).on_hover_text("Number of sides (3-64)");
                            ui.end_row();
                            ui.label(RichText::new("Inscribed").color(TEXT));
                            ui.checkbox(&mut inscribed, "").on_hover_text("On: the click sets a vertex. Off (circumscribed): the click sets the midpoint of an edge.");
                            ui.end_row();
                        });
                    });
                    state.set_polygon_options(sides, inscribed);
                }
            }

            egui::CollapsingHeader::new(RichText::new("Constraints").color(TEXT)).default_open(true).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                    let font = FontId::proportional(14.0);
                    for ct in ConstraintTool::ALL {
                        let active = tool == SketchTool::Constraint(ct);
                        let g = glyph_text(ui.ctx(), ct.glyph(), &font);
                        let b = egui::Button::new(RichText::new(g).font(font.clone()).color(if active { ACCENT } else { TEXT })).selected(active).min_size(Vec2::new(30.0, 24.0));
                        if ui.add(b).on_hover_text(ct.label()).clicked() {
                            state.execute(ct.command());
                        }
                    }
                });
            });

            ui.add_space(6.0);
            ui.separator();
            let (status_col, status) = if failed {
                (ERROR, overlay.status.clone())
            } else if fully {
                (OK, "🔒 Fully constrained".to_string())
            } else {
                (ACCENT, overlay.status.clone())
            };
            let font = FontId::proportional(12.5);
            let status = if status.starts_with('🔒') && !ui.ctx().fonts_mut(|f| f.has_glyphs(&font, "🔒")) { overlay.status.clone() } else { status };
            ui.label(RichText::new(status).font(font).color(status_col));
            ui.label(RichText::new(format!("{}: {hint}", tool.label())).size(11.5).color(MUTED));
            ui.add_space(4.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if ui.add(egui::Button::new(RichText::new("Finish Sketch").color(TEXT)).min_size(Vec2::new(96.0, 24.0))).on_hover_text("Exits the sketch (one undo step)").clicked() {
                    state.execute(CommandId::FinishSketch);
                }
            });
        });
}

/// Dimension labels, value-entry boxes and constraint glyphs over the viewport.
/// `project` maps a world point to a screen position (None when off-screen).
pub fn labels(ui: &mut egui::Ui, state: &mut AppState, project: &dyn Fn([f64; 3]) -> Option<egui::Pos2>) {
    let Some(o) = state.sketch_overlay() else { return };
    let frame = o.frame;
    let to_screen = |p: [f64; 2]| project(frame.to_world(p));
    let painter = ui.painter().clone();
    let prompt = state.sketch_prompt().cloned();
    let editing = match &prompt {
        Some(SketchPrompt::Value { constraint, .. }) => Some(constraint.clone()),
        _ => None,
    };

    // dimension labels (driven ones in parentheses, muted like Fusion's reference dimensions)
    if state.settings.show_dimensions {
        let font = FontId::proportional(12.5);
        for d in &o.dimensions {
            if editing.as_ref() == Some(&d.id) {
                continue;
            }
            let Some(p) = to_screen(d.anchor) else { continue };
            let col = if d.selected { ACCENT } else if d.driven { MUTED } else { TEXT };
            let galley = painter.layout_no_wrap(d.text.clone(), font.clone(), col);
            let r = egui::Rect::from_center_size(p, galley.size() + Vec2::new(8.0, 4.0));
            painter.rect(r, CornerRadius::same(3), LABEL_BG, Stroke::new(1.0, if d.selected { ACCENT } else { PANEL_STROKE }), egui::StrokeKind::Inside);
            painter.galley(r.min + Vec2::new(4.0, 2.0), galley, col);
        }
    }

    // constraint glyphs
    if state.settings.show_constraints {
        let font = FontId::proportional(11.0);
        for g in &o.glyphs {
            let Some(p) = to_screen(g.pos) else { continue };
            let text = glyph_text(ui.ctx(), g.label, &font);
            let col = if g.selected { ACCENT } else { Color32::from_rgb(0xe8, 0xc5, 0x5a) };
            let galley = painter.layout_no_wrap(text.to_string(), font.clone(), col);
            let r = egui::Rect::from_center_size(p, galley.size() + Vec2::new(6.0, 2.0)).expand(0.5);
            painter.rect(r, CornerRadius::same(2), LABEL_BG, Stroke::new(1.0, if g.selected { ACCENT } else { Color32::from_rgb(0x6a, 0x5a, 0x2a) }), egui::StrokeKind::Inside);
            painter.galley(r.min + Vec2::new(3.0, 1.0), galley, col);
        }
    }

    // inline value entry / driven-dimension offer
    let Some(anchor) = state.sketch_prompt_anchor().and_then(|a| to_screen(a)) else { return };
    match prompt {
        Some(SketchPrompt::Value { text, error, .. }) => {
            let rect = egui::Rect::from_center_size(anchor, Vec2::new(128.0, 24.0));
            let id = ui.make_persistent_id("cad360-sketch-value-entry");
            let mut buf = text.clone();
            let resp = ui.put(
                rect,
                TextEdit::singleline(&mut buf).id(id).font(FontId::proportional(13.0)).margin(Margin::symmetric(5, 3)).desired_width(118.0).background_color(Color32::from_rgb(0x1d, 0x1f, 0x23)).text_color(TEXT),
            );
            if buf != text {
                state.sketch_set_value_text(&buf);
            }
            if state.sketch_prompt_take_focus() {
                resp.request_focus();
            }
            let (enter, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if resp.lost_focus() {
                if esc {
                    state.sketch_value_cancel();
                } else {
                    // Enter, or focus moved elsewhere: commit like Fusion
                    let _ = enter;
                    state.sketch_value_commit();
                }
            }
            if let Some(e) = error {
                painter.text(rect.center_bottom() + Vec2::new(0.0, 4.0), Align2::CENTER_TOP, e, FontId::proportional(11.5), ERROR);
            }
        }
        Some(SketchPrompt::Driven { .. }) => {
            let rect = egui::Rect::from_min_size(anchor + Vec2::new(-130.0, 14.0), Vec2::new(260.0, 70.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                egui::Frame::new().fill(PANEL_FILL).stroke(Stroke::new(1.0, ERROR)).corner_radius(CornerRadius::same(4)).inner_margin(Margin::same(6)).show(ui, |ui| {
                    ui.label(RichText::new(OVER_CONSTRAINED).color(ERROR).size(12.0));
                    ui.label(RichText::new("Create the dimension as a Driven (reference) dimension?").color(TEXT).size(11.5));
                    ui.horizontal(|ui| {
                        if ui.button("Create Driven").clicked() {
                            state.sketch_accept_driven();
                        }
                        if ui.button("Cancel").clicked() {
                            state.sketch_value_cancel();
                        }
                    });
                });
            });
        }
        None => {}
    }
}
