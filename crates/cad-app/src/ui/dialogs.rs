//! Feature command dialogs, drawn as a Fusion-style command panel at the right of the canvas:
//! a header bar with the command name in capitals, label / value rows, the status of the
//! selection and of the previewed feature, and OK / Cancel at the bottom.
//!
//! The panel edits a detached copy of the dialog payload and then hands every change to
//! `AppState::dialog_changed`, the same path the headless tests use.

#[path = "dialogs_inspect.rs"]
mod inspect;
#[path = "dialogs_tools.rs"]
mod tools;
#[path = "dialogs_assembly.rs"]
mod assembly;
#[path = "dialogs_more.rs"]
mod more;
#[path = "dialogs_surface.rs"]
mod surface;

use crate::state::features::{axis_label, Dialog, ExprField, Session};
use crate::state::AppState;
use cad_core::{ExtentDirection, Operation, OriginPlane, PlaneRef};
use cad_kernel::Health;
use egui::{Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, RichText, Sense, Stroke, Ui};

pub(crate) const PANEL: Color32 = Color32::from_rgb(0x2b, 0x2e, 0x34);
pub(crate) const HEADER: Color32 = Color32::from_rgb(0x22, 0x25, 0x2a);
pub(crate) const BORDER: Color32 = Color32::from_rgb(0x44, 0x48, 0x50);
pub(crate) const TEXT: Color32 = Color32::from_rgb(0xdc, 0xdf, 0xe4);
pub(crate) const LABEL: Color32 = Color32::from_rgb(0xb4, 0xb9, 0xc1);
pub(crate) const DIM: Color32 = Color32::from_rgb(0x8c, 0x92, 0x9b);
pub(crate) const FIELD: Color32 = Color32::from_rgb(0x1d, 0x1f, 0x23);
pub(crate) const CONTROL: Color32 = Color32::from_rgb(0x3a, 0x3e, 0x46);
pub(crate) const CONTROL_HOVER: Color32 = Color32::from_rgb(0x48, 0x4d, 0x56);
pub(crate) const ACCENT: Color32 = Color32::from_rgb(0x1f, 0x6f, 0xc4);
pub(crate) const ERROR: Color32 = Color32::from_rgb(0xff, 0x6e, 0x6e);
pub(crate) const WARN: Color32 = Color32::from_rgb(0xf0, 0xb6, 0x3a);

const LABEL_W: f32 = 104.0;
const ROW_H: f32 = 24.0;

/// Something the panel asks the state to do after drawing.
pub(crate) enum Action {
    Changed,
    Ok,
    Cancel,
    ClearInputs,
    RemoveEdge(usize),
    ParamSet(String, String),
    ParamAdd,
    ParamDelete(String),
    Appearance(Option<usize>),
}

/// Pre-computed state information the panel shows (the dialog itself is detached while drawn).
pub(crate) struct Info {
    pub status: Option<String>,
    pub blocker: Option<String>,
    /// Live health of the previewed feature.
    pub health: Option<Health>,
}

/// Draws the active command dialog inside `rect` (right side of the canvas, like Fusion).
pub fn show(ui: &mut Ui, state: &mut AppState) {
    let health = state.dialog.as_ref().and_then(|d| d.session()).and_then(|s| s.feature.as_deref()).map(|f| state.model.health_of(f));
    let info = Info { status: state.dialog_status(), blocker: state.dialog_blocker(), health };
    let Some(mut d) = state.dialog.take() else { return };
    let mut actions: Vec<Action> = Vec::new();
    if let Dialog::Parameters(p) = &mut d {
        tools::parameters_window(ui, state, p, &mut actions);
    } else {
        panel(ui, state, &mut d, &info, &mut actions);
    }
    state.dialog = Some(d);
    run(state, actions);
}

fn run(state: &mut AppState, actions: Vec<Action>) {
    for a in actions {
        if state.dialog.is_none() {
            return;
        }
        match a {
            Action::Changed => state.dialog_changed(),
            Action::Ok => state.dialog_ok(),
            Action::Cancel => state.dialog_cancel(),
            Action::ClearInputs => state.dialog_edit(|d| match d {
                Dialog::Extrude(x) => x.profiles.clear(),
                Dialog::Revolve(x) => x.profiles.clear(),
                Dialog::Fillet(x) | Dialog::Chamfer(x) => x.edges.clear(),
                Dialog::Measure(x) => x.picks.clear(),
                Dialog::Inspect(x) => x.clear_inputs(),
                Dialog::Joint(x) => match x.active {
                    crate::state::features::BodyInput::First => x.a = None,
                    crate::state::features::BodyInput::Second => x.b = None,
                },
                Dialog::Surface(x) => x.clear_inputs(),
                other => AppState::clear_more_inputs(other),
            }),
            Action::RemoveEdge(i) => state.dialog_remove_edge(i),
            Action::ParamSet(n, e) => {
                state.parameters_set(&n, &e);
            }
            Action::ParamAdd => {
                state.parameters_add_user();
            }
            Action::ParamDelete(n) => {
                state.parameters_delete(&n);
            }
            Action::Appearance(i) => {
                state.apply_appearance(i);
            }
        }
    }
}

/// Dark command-panel styling for the widgets inside the panel.
pub(crate) fn style(ui: &mut Ui) {
    let v = ui.visuals_mut();
    v.override_text_color = Some(TEXT);
    v.extreme_bg_color = FIELD;
    v.text_edit_bg_color = Some(FIELD);
    for (w, fill) in [(&mut v.widgets.inactive, CONTROL), (&mut v.widgets.hovered, CONTROL_HOVER), (&mut v.widgets.active, CONTROL_HOVER), (&mut v.widgets.open, CONTROL_HOVER)] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.fg_stroke = Stroke::new(1.0, TEXT);
        w.corner_radius = CornerRadius::same(2);
    }
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    ui.spacing_mut().item_spacing = egui::vec2(6.0, 5.0);
}

/// The panel frame: header bar, body, footer with OK / Cancel.
fn panel(ui: &mut Ui, state: &AppState, d: &mut Dialog, info: &Info, actions: &mut Vec<Action>) {
    let title = d.title();
    let width = ui.available_width().min(306.0);
    egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, BORDER)).corner_radius(CornerRadius::same(3)).show(ui, |ui| {
        ui.set_width(width);
        style(ui);
        header(ui, &title);
        egui::Frame::new().inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
            ui.set_width(width - 20.0);
            body(ui, state, d, actions);
            footer(ui, d, info, actions);
        });
    });
}

pub(crate) fn header(ui: &mut Ui, title: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, HEADER);
    ui.painter().text(rect.left_center() + egui::vec2(10.0, 0.0), Align2::LEFT_CENTER, format!("\u{2212}  {title}"), FontId::proportional(12.5), TEXT);
    ui.painter().text(rect.right_center() - egui::vec2(10.0, 0.0), Align2::RIGHT_CENTER, "\u{00bb}", FontId::proportional(13.0), DIM);
}

/// A label / value row.
pub(crate) fn row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(LABEL_W, ROW_H), Sense::hover());
        ui.painter().text(r.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(13.0), LABEL);
        add(ui)
    })
    .inner
}

fn value_width(ui: &Ui) -> f32 {
    ui.available_width().max(60.0)
}

/// Expression text box. Red text and an error line while invalid; resynchronises from the
/// document parameter (`doc_expr`) when changed elsewhere and not being edited.
pub(crate) fn expr_row(ui: &mut Ui, label: &str, f: &mut ExprField, doc_expr: Option<String>) -> bool {
    let changed = row(ui, label, |ui| {
        let color = if f.error.is_some() { ERROR } else { TEXT };
        let resp = ui.add(egui::TextEdit::singleline(&mut f.text).desired_width(value_width(ui)).text_color(color));
        if resp.changed() {
            return true;
        }
        if !resp.has_focus() && f.error.is_none() {
            if let Some(e) = doc_expr {
                if f.text.trim() != e {
                    f.text = e;
                }
            }
        }
        false
    });
    if let Some(e) = &f.error {
        ui.horizontal(|ui| {
            ui.add_space(LABEL_W + 6.0);
            ui.add(egui::Label::new(RichText::new(e).color(ERROR).size(11.5)).wrap());
        });
    }
    changed
}

/// Drop-down with fixed options; returns whether the value changed.
pub(crate) fn combo<T: PartialEq + Clone>(ui: &mut Ui, id: &str, value: &mut T, options: &[(T, String)]) -> bool {
    let before = value.clone();
    let text = options.iter().find(|o| o.0 == *value).map(|o| o.1.clone()).unwrap_or_default();
    let w = value_width(ui) - 8.0;
    egui::ComboBox::from_id_salt(id).selected_text(text).width(w).show_ui(ui, |ui| {
        for (v, l) in options {
            ui.selectable_value(value, v.clone(), l.as_str());
        }
    });
    *value != before
}

/// Fusion selection input: "↖ 1 selected" (highlighted when it holds items) and a clear "×".
pub(crate) fn selection_row(ui: &mut Ui, label: &str, count: usize, actions: &mut Vec<Action>) {
    row(ui, label, |ui| {
        let text = if count == 0 { "Select".to_string() } else { format!("{count} selected") };
        let fill = if count == 0 { CONTROL } else { ACCENT };
        ui.add(egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(fill).min_size(egui::vec2(110.0, 20.0)))
            .on_hover_text("Click items in the canvas to add or remove them");
        if count > 0 && ui.add(egui::Button::new("\u{00d7}").frame(false)).on_hover_text("Clear selection").clicked() {
            actions.push(Action::ClearInputs);
        }
    });
}

pub(crate) fn static_row(ui: &mut Ui, label: &str, value: &str) {
    row(ui, label, |ui| {
        ui.label(RichText::new(value).color(TEXT));
    });
}

/// Current expression of the n-th parameter of the dialog's feature.
fn feature_param(state: &AppState, s: &Session, n: usize) -> Option<String> {
    let f = state.doc.feature(s.feature.as_deref()?)?;
    let name = f.parameter_names().get(n)?.to_string();
    state.doc.parameter(&name).map(|p| p.expression.clone())
}

fn operations() -> Vec<(Operation, String)> {
    Operation::ALL.iter().map(|o| (*o, o.label().to_string())).collect()
}

fn body(ui: &mut Ui, state: &AppState, d: &mut Dialog, actions: &mut Vec<Action>) {
    let mut changed = false;
    let chamfer = matches!(d, Dialog::Chamfer(_));
    match d {
        Dialog::Extrude(x) => {
            static_row(ui, "Type", "Solid");
            selection_row(ui, "Profiles", x.profiles.len(), actions);
            static_row(ui, "Start", "Profile Plane");
            #[derive(Clone, PartialEq)]
            enum Side {
                One,
                Sym,
            }
            let mut side = if x.direction == ExtentDirection::Symmetric { Side::Sym } else { Side::One };
            row(ui, "Direction", |ui| {
                let w = value_width(ui);
                ui.scope(|ui| {
                    ui.set_max_width(if side == Side::One { w - 52.0 } else { w });
                    if combo(ui, "ext_dir", &mut side, &[(Side::One, "One Side".into()), (Side::Sym, "Symmetric".into())]) {
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
            row(ui, "Extent Type", |ui| {
                if combo(ui, "ext_extent", &mut x.through_all, &[(false, "Distance".into()), (true, "All".into())]) {
                    changed = true;
                }
            });
            if !x.through_all {
                changed |= expr_row(ui, "Distance", &mut x.distance, feature_param(state, &x.s, 0));
            }
            ui.separator();
            row(ui, "Operation", |ui| {
                changed |= combo(ui, "ext_op", &mut x.operation, &operations());
            });
        }
        Dialog::Revolve(x) => {
            selection_row(ui, "Profile", x.profiles.len(), actions);
            let choices: Vec<(Option<cad_core::RevolveAxis>, String)> =
                state.revolve_axis_choices(x.sketch.as_deref(), &x.s.base.0).into_iter().map(|a| (Some(a.clone()), axis_label(&a))).collect();
            row(ui, "Axis", |ui| {
                changed |= combo(ui, "rev_axis", &mut x.axis, &choices);
            });
            static_row(ui, "Type", "Angle");
            changed |= expr_row(ui, "Angle", &mut x.angle, feature_param(state, &x.s, 0));
            ui.separator();
            row(ui, "Operation", |ui| {
                changed |= combo(ui, "rev_op", &mut x.operation, &operations());
            });
        }
        Dialog::Fillet(x) | Dialog::Chamfer(x) => {
            selection_row(ui, "Edges", x.edges.len(), actions);
            if !x.edges.is_empty() {
                egui::ScrollArea::vertical().id_salt("edge_list").max_height(110.0).show(ui, |ui| {
                    for (i, e) in x.edges.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add_space(LABEL_W + 6.0);
                            let kind = match e.curve {
                                cad_core::CurveKind::Line => "Line",
                                cad_core::CurveKind::Circle => "Arc",
                                cad_core::CurveKind::Other => "Curve",
                            };
                            ui.label(RichText::new(format!("Edge {} \u{00b7} {kind} {:.2} mm", i + 1, e.length)).color(DIM).size(12.0));
                            if ui.add(egui::Button::new("\u{00d7}").frame(false)).on_hover_text("Remove this edge").clicked() {
                                actions.push(Action::RemoveEdge(i));
                            }
                        });
                    }
                });
            }
            static_row(ui, "Type", if chamfer { "Equal distance" } else { "Constant radius" });
            changed |= expr_row(ui, if chamfer { "Distance" } else { "Radius" }, &mut x.value, feature_param(state, &x.s, 0));
        }
        Dialog::Hole(x) => {
            selection_row(ui, "Face", x.face.is_some() as usize, actions);
            static_row(ui, "Placement", "Face centre + offset");
            changed |= expr_row(ui, "X Offset", &mut x.offset_x, None);
            changed |= expr_row(ui, "Y Offset", &mut x.offset_y, None);
            static_row(ui, "Hole Type", "Simple");
            row(ui, "Extents", |ui| {
                changed |= combo(ui, "hole_extent", &mut x.through_all, &[(false, "Distance".into()), (true, "All".into())]);
            });
            if !x.through_all {
                changed |= expr_row(ui, "Depth", &mut x.depth, feature_param(state, &x.s, 1));
            }
            changed |= expr_row(ui, "Diameter", &mut x.diameter, feature_param(state, &x.s, 0));
        }
        Dialog::OffsetPlane(x) => {
            let what = match &x.base {
                None => "Select a plane or face".to_string(),
                Some(PlaneRef::Origin { plane }) => format!("{plane:?} Plane"),
                Some(PlaneRef::Construction { feature }) => state.doc.feature(feature).map(|f| f.name.clone()).unwrap_or_default(),
                Some(PlaneRef::Face { .. }) => "Planar face".to_string(),
            };
            selection_row(ui, "Plane", x.base.is_some() as usize, actions);
            static_row(ui, "", &what);
            row(ui, "Origin Plane", |ui| {
                for p in [OriginPlane::XY, OriginPlane::XZ, OriginPlane::YZ] {
                    let sel = x.base == Some(PlaneRef::Origin { plane: p });
                    if ui.add(egui::Button::new(format!("{p:?}")).selected(sel).min_size(egui::vec2(40.0, 20.0))).clicked() && !sel {
                        x.base = Some(PlaneRef::Origin { plane: p });
                        changed = true;
                    }
                }
            });
            changed |= expr_row(ui, "Distance", &mut x.offset, feature_param(state, &x.s, 0));
        }
        Dialog::Measure(x) => tools::measure_body(ui, state, x, actions),
        Dialog::Inspect(x) => inspect::body(ui, state, x, actions),
        Dialog::Appearance(x) => tools::appearance_body(ui, state, x, actions),
        Dialog::Info(t) => {
            ui.label(t.as_str());
        }
        Dialog::Parameters(_) => {}
        Dialog::Joint(x) => changed |= assembly::joint_body(ui, state, x, actions),
        Dialog::Surface(x) => changed |= surface::body(ui, state, x, actions),
        other => changed |= more::body(ui, state, other, actions),
    }
    if changed {
        actions.push(Action::Changed);
    }
}

fn footer(ui: &mut Ui, d: &Dialog, info: &Info, actions: &mut Vec<Action>) {
    if let Some(s) = d.session() {
        ui.add_space(4.0);
        if let Some(n) = &s.notice {
            ui.add(egui::Label::new(RichText::new(n).color(DIM).size(12.0)).wrap());
        }
        match info.health.as_ref().unwrap_or(&s.health) {
            Health::Error(e) => {
                ui.add(egui::Label::new(RichText::new(e.as_str()).color(ERROR).size(12.0)).wrap());
            }
            Health::Warning(w) => {
                ui.add(egui::Label::new(RichText::new(w.as_str()).color(WARN).size(12.0)).wrap());
            }
            _ => {}
        }
        if let Some(st) = &info.status {
            ui.label(RichText::new(st).color(DIM).size(12.0));
        }
    }
    ui.add_space(4.0);
    ui.separator();
    let owns = d.owns_txn();
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 26.0), Layout::right_to_left(Align::Center), |ui| {
        if owns {
            if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(64.0, 22.0))).clicked() {
                actions.push(Action::Cancel);
            }
            let enabled = info.blocker.is_none();
            let ok = egui::Button::new(RichText::new("OK").color(if enabled { Color32::WHITE } else { DIM })).fill(if enabled { ACCENT } else { CONTROL }).min_size(egui::vec2(64.0, 22.0));
            let r = ui.add_enabled(enabled, ok);
            let r = match &info.blocker {
                Some(b) => r.on_disabled_hover_text(b.as_str()),
                None => r,
            };
            if r.clicked() {
                actions.push(Action::Ok);
            }
        } else if ui.add(egui::Button::new("Close").min_size(egui::vec2(64.0, 22.0))).clicked() {
            actions.push(Action::Ok);
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            let (r, resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
            ui.painter().circle_stroke(r.center(), 6.5, Stroke::new(1.0, DIM));
            ui.painter().text(r.center(), Align2::CENTER_CENTER, "i", FontId::proportional(10.5), DIM);
            resp.on_hover_text("Click items in the canvas to add them to the selection input; click again to remove them.");
        });
    });
}
