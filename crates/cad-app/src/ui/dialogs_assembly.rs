//! Panel of the Joint dialog (ASSEMBLE > Joint), same layout conventions as the other feature
//! dialogs: two selection inputs (the highlighted one receives canvas clicks), the joint type,
//! angle, offset and flip. Which component moves is not decided by pick order (Fusion F36): the
//! status line names it, and an over-constraining joint (F37) shows Fusion's error and blocks OK.

use super::{combo, expr_row, feature_param, row, Action, ACCENT, CONTROL, DIM, LABEL_W};
use crate::state::features::{BodyInput, JointDialog};
use crate::state::AppState;
use cad_core::JointKind;
use egui::{Color32, RichText, Ui};

/// A selection input showing what it holds; clicking makes it the active input.
fn origin_row(ui: &mut Ui, label: &str, text: &str, filled: bool, active: bool, actions: &mut Vec<Action>) -> bool {
    let mut clicked = false;
    row(ui, label, |ui| {
        let fill = if active { ACCENT } else { CONTROL };
        let r = ui
            .add(egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(fill).min_size(egui::vec2(150.0, 20.0)).truncate())
            .on_hover_text("Click to make this input active, then click a planar face or a circular edge in the canvas");
        clicked = r.clicked();
        if active && filled && ui.add(egui::Button::new("\u{00d7}").frame(false)).on_hover_text("Clear selection").clicked() {
            actions.push(Action::ClearInputs);
        }
    });
    clicked
}

/// Draws the Joint dialog body; returns whether a value changed.
pub(super) fn joint_body(ui: &mut Ui, state: &AppState, x: &mut JointDialog, actions: &mut Vec<Action>) -> bool {
    let mut changed = false;
    ui.label(RichText::new("Component 1").color(DIM).size(11.5));
    if origin_row(ui, "Snap", &state.joint_input_text(&x.a), x.a.is_some(), x.active == BodyInput::First, actions) {
        x.active = BodyInput::First;
    }
    ui.label(RichText::new("Component 2").color(DIM).size(11.5));
    if origin_row(ui, "Snap", &state.joint_input_text(&x.b), x.b.is_some(), x.active == BodyInput::Second, actions) {
        x.active = BodyInput::Second;
    }
    ui.separator();
    let kinds: Vec<(JointKind, String)> = JointKind::ALL.iter().map(|k| (*k, k.label().to_string())).collect();
    row(ui, "Type", |ui| {
        changed |= combo(ui, "joint_kind", &mut x.kind, &kinds);
    });
    let (angle_label, offset_label) = match x.kind {
        JointKind::Revolute => ("Rotation", "Offset"),
        JointKind::Slider => ("Angle", "Slide"),
        JointKind::Rigid => ("Angle", "Offset"),
    };
    changed |= expr_row(ui, angle_label, &mut x.angle, feature_param(state, &x.s, 0));
    changed |= expr_row(ui, offset_label, &mut x.offset, feature_param(state, &x.s, 1));
    row(ui, "Flip", |ui| {
        if ui.add(egui::Button::new("Flip").selected(x.flip).min_size(egui::vec2(44.0, 20.0))).on_hover_text("Reverse the joint axis of Component 2 (by default both joint Z axes point the same way)").clicked() {
            x.flip = !x.flip;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(LABEL_W + 6.0);
        ui.add(egui::Label::new(RichText::new("The joint origins coincide with their Z axes aligned. Root and grounded components stay fixed; otherwise the component created later moves (see the status line).").color(DIM).size(11.5)).wrap());
    });
    changed
}
