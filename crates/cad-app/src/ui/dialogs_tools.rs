//! Panels of the tool dialogs: Change Parameters (a separate window, as in Fusion), Measure and
//! Appearance.

use super::{combo, header, row, style, Action, ACCENT, BORDER, DIM, ERROR, LABEL, PANEL, TEXT};
use crate::state::features::{quantity_unit, AppearanceDialog, MeasureDialog, ParametersDialog, APPEARANCES};
use crate::state::AppState;
use cad_core::expr::Display;
use cad_core::{Parameter, Quantity};
use egui::{Color32, CornerRadius, Margin, RichText, Sense, Stroke, Ui};

fn fmt(v: f64, unit: &str) -> String {
    format!("{v:.3} {unit}")
}

// ---------------------------------------------------------------------- parameters
pub(crate) fn parameters_window(ui: &mut Ui, state: &AppState, p: &mut ParametersDialog, actions: &mut Vec<Action>) {
    let mut open = true;
    let ctx = ui.ctx().clone();
    let frame = egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, BORDER)).corner_radius(CornerRadius::same(3)).inner_margin(Margin::same(0));
    egui::Window::new("PARAMETERS")
        .title_bar(false)
        .frame(frame)
        .collapsible(false)
        .resizable(true)
        .default_size([780.0, 460.0])
        .default_pos(ctx.content_rect().center() - egui::vec2(390.0, 230.0))
        .open(&mut open)
        .show(&ctx, |ui| {
            style(ui);
            header(ui, "PARAMETERS");
            egui::Frame::new().inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| parameters_body(ui, state, p, actions));
        });
    if !open {
        actions.push(Action::Ok);
    }
}

fn parameters_body(ui: &mut Ui, state: &AppState, p: &mut ParametersDialog, actions: &mut Vec<Action>) {
    let values = state.doc.parameter_values();
    let users: Vec<&Parameter> = state.doc.parameters.iter().filter(|q| q.owner.is_none()).collect();
    let max_h = (ui.ctx().content_rect().height() * 0.6).max(200.0);
    egui::ScrollArea::vertical().id_salt("params_scroll").max_height(max_h).show(ui, |ui| {
        egui::Grid::new("params_grid").num_columns(7).spacing([12.0, 5.0]).striped(true).show(ui, |ui| {
            for h in ["Parameter", "Name", "Unit", "Expression", "Value", "Comments", ""] {
                ui.label(RichText::new(h).color(LABEL).strong());
            }
            ui.end_row();

            ui.label(RichText::new("User Parameters").color(TEXT).strong());
            ui.end_row();
            for q in &users {
                param_row(ui, q, "", &values, p, actions, true);
            }
            // add a user parameter
            ui.label(RichText::new("+ New").color(DIM));
            ui.add_sized([90.0, 20.0], egui::TextEdit::singleline(&mut p.new_name).hint_text("name"));
            let units = [(Quantity::Length, "mm".to_string()), (Quantity::Angle, "deg".to_string()), (Quantity::Unitless, "No Units".to_string())];
            ui.scope(|ui| {
                ui.set_max_width(96.0);
                combo(ui, "new_param_unit", &mut p.new_quantity, &units);
            });
            ui.add_sized([150.0, 20.0], egui::TextEdit::singleline(&mut p.new_expression).hint_text("expression"));
            ui.label("");
            ui.add_sized([140.0, 20.0], egui::TextEdit::singleline(&mut p.new_comment).hint_text("comment"));
            let can_add = !p.new_name.trim().is_empty() && !p.new_expression.trim().is_empty();
            if ui.add_enabled(can_add, egui::Button::new(RichText::new("Add").color(Color32::WHITE)).fill(ACCENT)).clicked() {
                actions.push(Action::ParamAdd);
            }
            ui.end_row();
            if let Some(e) = &p.new_error {
                ui.label("");
                ui.label(RichText::new(e).color(ERROR).size(12.0));
                ui.end_row();
            }

            ui.label(RichText::new("Model Parameters").color(TEXT).strong());
            ui.end_row();
            for f in &state.doc.features {
                let own: Vec<&Parameter> = state.doc.parameters.iter().filter(|q| q.owner.as_deref() == Some(f.id.as_str())).collect();
                if own.is_empty() {
                    continue;
                }
                for (k, q) in own.iter().enumerate() {
                    let owner = if k == 0 { f.name.as_str() } else { "" };
                    param_row(ui, q, owner, &values, p, actions, false);
                }
            }
        });
    });
    ui.add_space(6.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Edit an expression and press Enter to apply it (each change is one undo step).").color(DIM).size(12.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new("Close").min_size(egui::vec2(64.0, 22.0))).clicked() {
                actions.push(Action::Ok);
            }
        });
    });
}

fn param_row(ui: &mut Ui, q: &Parameter, owner: &str, values: &cad_core::ParameterValues, p: &mut ParametersDialog, actions: &mut Vec<Action>, user: bool) {
    ui.label(RichText::new(owner).color(TEXT));
    ui.label(RichText::new(&q.name).color(TEXT));
    ui.label(RichText::new(quantity_unit(q.quantity)).color(DIM));
    let err = p.errors.get(&q.name).cloned();
    let buf = p.edits.entry(q.name.clone()).or_insert_with(|| q.expression.clone());
    let resp = ui.add_sized([150.0, 20.0], egui::TextEdit::singleline(buf).text_color(if err.is_some() { ERROR } else { TEXT }));
    let commit = resp.lost_focus() && buf.trim() != q.expression;
    if commit {
        actions.push(Action::ParamSet(q.name.clone(), buf.clone()));
    } else if !resp.has_focus() && err.is_none() && buf.trim() != q.expression {
        *buf = q.expression.clone();
    }
    let value = match (values.values.get(&q.name), values.errors.get(&q.name)) {
        (Some(v), _) => Display(*v, q.quantity).to_string(),
        (None, Some(e)) => e.clone(),
        _ => String::new(),
    };
    ui.label(RichText::new(value).color(TEXT));
    match &err {
        Some(e) => ui.label(RichText::new(e).color(ERROR).size(12.0)),
        None => ui.label(RichText::new(&q.comment).color(DIM)),
    };
    if user {
        if ui.add(egui::Button::new("\u{00d7}").frame(false)).on_hover_text("Delete this user parameter").clicked() {
            actions.push(Action::ParamDelete(q.name.clone()));
        }
    } else {
        ui.label("");
    }
    ui.end_row();
}

// ---------------------------------------------------------------------- measure
pub(crate) fn measure_body(ui: &mut Ui, state: &AppState, x: &mut MeasureDialog, actions: &mut Vec<Action>) {
    super::selection_row(ui, "Selection", x.picks.len(), actions);
    let report = state.measure_report_for(&x.picks);
    if report.items.is_empty() {
        ui.label(RichText::new("Select faces, edges, bodies or profiles in the canvas (up to two).").color(DIM).size(12.0));
    }
    for (i, m) in report.items.iter().enumerate() {
        ui.separator();
        ui.label(RichText::new(format!("{} {} \u{00b7} {}", if i == 0 { "Selection 1:" } else { "Selection 2:" }, m.kind, m.name)).color(TEXT).strong());
        if let Some(v) = m.area {
            super::static_row(ui, "Area", &fmt(v, "mm^2"));
        }
        if let Some(v) = m.length {
            super::static_row(ui, "Length", &fmt(v, "mm"));
        }
        if let Some(v) = m.radius {
            super::static_row(ui, "Radius", &fmt(v, "mm"));
        }
        if let Some(v) = m.volume {
            super::static_row(ui, "Volume", &fmt(v, "mm^3"));
        }
        if let Some(v) = m.surface_area {
            super::static_row(ui, "Surface Area", &fmt(v, "mm^2"));
        }
        if let Some(s) = m.size {
            super::static_row(ui, "Size", &format!("{:.2} \u{00d7} {:.2} \u{00d7} {:.2} mm", s[0], s[1], s[2]));
        }
    }
    if let (Some(d), Some(dl)) = (report.distance, report.delta) {
        ui.separator();
        ui.label(RichText::new("Results").color(TEXT).strong());
        super::static_row(ui, if report.min.is_some() { "Minimum Distance" } else { "Distance" }, &fmt(d, "mm"));
        super::static_row(ui, "\u{0394}X / \u{0394}Y / \u{0394}Z", &format!("{:.3} / {:.3} / {:.3} mm", dl[0], dl[1], dl[2]));
        if let Some(a) = report.angle {
            super::static_row(ui, "Angle", &format!("{a:.3} deg"));
        }
        match report.min {
            Some(m) => {
                let p = |v: [f64; 3]| format!("{:.3}, {:.3}, {:.3}", v[0], v[1], v[2]);
                super::static_row(ui, "Point 1", &p(m.a));
                super::static_row(ui, "Point 2", &p(m.b));
                if let Some(c) = report.center_distance {
                    super::static_row(ui, "Center Distance", &fmt(c, "mm"));
                }
                if m.inside {
                    ui.label(RichText::new("One body lies inside the other.").color(DIM).size(11.5));
                }
            }
            None => {
                ui.label(RichText::new("Distance between centroids / midpoints.").color(DIM).size(11.5));
            }
        }
    }
}

// ---------------------------------------------------------------------- appearance
pub(crate) fn appearance_body(ui: &mut Ui, state: &AppState, x: &mut AppearanceDialog, actions: &mut Vec<Action>) {
    let bodies = state.selected_bodies();
    row(ui, "Apply To", |ui| {
        ui.label(RichText::new(if bodies.is_empty() { "Select bodies or faces".to_string() } else { format!("{} bod{}", bodies.len(), if bodies.len() == 1 { "y" } else { "ies" }) }).color(TEXT));
    });
    let current = bodies.first().and_then(|b| state.doc.bodies.get(b)).and_then(|m| m.color);
    ui.label(RichText::new("Library").color(LABEL));
    egui::Grid::new("appearance_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        for (i, (name, rgb)) in APPEARANCES.iter().enumerate() {
            let c = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(128.0, 22.0), Sense::click());
            let hovered = resp.hovered();
            let sel = current == Some(*rgb);
            ui.painter().rect_filled(rect, CornerRadius::same(2), if hovered || sel { super::CONTROL_HOVER } else { super::CONTROL });
            let sw = egui::Rect::from_min_size(rect.min + egui::vec2(3.0, 3.0), egui::vec2(16.0, 16.0));
            ui.painter().rect_filled(sw, CornerRadius::same(2), c);
            ui.painter().rect_stroke(sw, CornerRadius::same(2), Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
            if sel {
                ui.painter().rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
            }
            ui.painter().text(sw.right_center() + egui::vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, *name, egui::FontId::proportional(12.5), TEXT);
            if resp.on_hover_text(format!("Apply {name} to the selected bodies")).clicked() {
                actions.push(Action::Appearance(Some(i)));
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
    ui.add_space(2.0);
    if ui.button("Default appearance").clicked() {
        actions.push(Action::Appearance(None));
    }
    if let Some(s) = &x.status {
        ui.label(RichText::new(s).color(DIM).size(12.0));
    }
}
