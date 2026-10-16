//! Panels of the INSPECT tool dialogs: Interference and Center of Mass.

use super::{Action, ACCENT, DIM, LABEL, TEXT};
use crate::state::features::{InspectDialog, InspectKind};
use crate::state::AppState;
use egui::{Color32, RichText, Ui};

pub(crate) fn body(ui: &mut Ui, state: &AppState, x: &mut InspectDialog, actions: &mut Vec<Action>) {
    // after Clear, a recompute or an undo: evaluate again
    state.inspect_refresh_if_stale(x);
    super::selection_row(ui, "Bodies", x.bodies.len(), actions);
    if x.bodies.is_empty() {
        ui.label(RichText::new("No selection: all visible bodies are used.").color(DIM).size(12.0));
    }
    match x.kind {
        InspectKind::Interference => interference(ui, state, x),
        InspectKind::CenterOfMass => center_of_mass(ui, x),
    }
    if let Some(s) = &x.status {
        ui.add(egui::Label::new(RichText::new(s).color(DIM).size(12.0)).wrap());
    }
}

fn interference(ui: &mut Ui, state: &AppState, x: &mut InspectDialog) {
    ui.horizontal(|ui| {
        ui.add_space(super::LABEL_W + 6.0);
        if ui.add(egui::Button::new(RichText::new("Compute").color(Color32::WHITE)).fill(ACCENT).min_size(egui::vec2(90.0, 22.0))).on_hover_text("Check every pair of bodies for a shared volume").clicked() {
            state.inspect_compute(x);
        }
    });
    let Some(rows) = &x.interference else { return };
    ui.separator();
    if rows.is_empty() {
        return;
    }
    egui::Grid::new("interference_grid").num_columns(3).spacing([12.0, 4.0]).striped(true).show(ui, |ui| {
        for h in ["Body 1", "Body 2", "Volume"] {
            ui.label(RichText::new(h).color(LABEL).strong());
        }
        ui.end_row();
        for r in rows {
            ui.label(RichText::new(&r.a_name).color(TEXT));
            ui.label(RichText::new(&r.b_name).color(TEXT));
            ui.label(RichText::new(format!("{:.3} mm^3", r.volume)).color(TEXT));
            ui.end_row();
        }
    });
}

fn center_of_mass(ui: &mut Ui, x: &InspectDialog) {
    let Some(m) = x.mass else { return };
    ui.separator();
    ui.label(RichText::new("Results").color(TEXT).strong());
    let c = m.centroid;
    super::static_row(ui, "Center of Mass", &format!("{:.3}, {:.3}, {:.3} mm", c[0], c[1], c[2]));
    super::static_row(ui, "Volume", &format!("{:.3} mm^3", m.volume));
    super::static_row(ui, "Surface Area", &format!("{:.3} mm^2", m.area));
    ui.label(RichText::new("Uniform density; surface and mesh bodies are not included.").color(DIM).size(11.5));
}
