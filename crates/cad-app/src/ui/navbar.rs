//! Fusion navigation bar (F4): a floating pill at the bottom centre of the canvas with Orbit,
//! Look At, Pan, Zoom (left-drag modes, the active one highlighted), Fit, Display Settings
//! (orthographic / perspective, grid) and the Grid toggle. Viewports is shown disabled.

use crate::state::command::{spec, CommandId, NOT_SUPPORTED};
use crate::state::{AppState, NavMode};
use crate::ui::icons::{self, Icon};
use crate::ui::{ribbon, theme};
use egui::{pos2, vec2, CornerRadius, Id, Popup, Rect, Sense, Stroke, StrokeKind, Ui};

const B: f32 = 28.0;

/// Request written by the ViewCube menu (which has no access to the state): Some(perspective).
pub fn projection_request_id() -> Id {
    Id::unique("cad360.viewcube.projection_request")
}

#[derive(Clone, Copy)]
enum Item {
    Cmd(CommandId),
    Display,
    Viewports,
    Sep,
}

const ITEMS: [Item; 10] = [
    Item::Cmd(CommandId::OrbitMode),
    Item::Cmd(CommandId::LookAt),
    Item::Cmd(CommandId::PanMode),
    Item::Cmd(CommandId::ZoomMode),
    Item::Cmd(CommandId::Fit),
    Item::Sep,
    Item::Display,
    Item::Cmd(CommandId::ToggleGrid),
    Item::Sep,
    Item::Viewports,
];

fn item_width(i: Item) -> f32 {
    match i {
        Item::Sep => 9.0,
        Item::Display => B + 10.0,
        _ => B,
    }
}

/// Whether a nav-bar command is currently "on" (highlighted).
pub fn is_active(state: &AppState, id: CommandId) -> bool {
    match id {
        CommandId::OrbitMode => state.settings.nav == NavMode::Orbit,
        CommandId::PanMode => state.settings.nav == NavMode::Pan,
        CommandId::ZoomMode => state.settings.nav == NavMode::Zoom,
        CommandId::ToggleGrid => state.settings.grid,
        _ => false,
    }
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();

    // projection change requested by the ViewCube menu
    if let Some(want_persp) = ctx.data_mut(|d| d.remove_temp::<bool>(projection_request_id())) {
        if want_persp != state.settings.perspective {
            state.execute(CommandId::ToggleProjection);
        }
    }

    let area = ui.max_rect();
    let total: f32 = ITEMS.iter().map(|i| item_width(*i)).sum::<f32>() + 12.0;
    let pill = Rect::from_center_size(area.center(), vec2(total, (area.height() - 2.0).min(32.0)));
    let painter = ui.painter().clone();
    // swallow clicks on the pill background
    let _ = ui.interact(pill, ui.scope_id().with("navbar-bg"), Sense::click_and_drag());
    painter.rect_filled(pill.translate(vec2(0.0, 2.0)), CornerRadius::same(8), egui::Color32::from_black_alpha(60));
    painter.rect_filled(pill, CornerRadius::same(8), theme::PANEL_BG.gamma_multiply(0.97));
    painter.rect_stroke(pill, CornerRadius::same(8), Stroke::new(1.0, theme::SEPARATOR), StrokeKind::Inside);

    let mut x = pill.left() + 6.0;
    let cy = pill.center().y;
    for item in ITEMS {
        let w = item_width(item);
        let r = Rect::from_center_size(pos2(x + w * 0.5, cy), vec2(w, B - 2.0));
        x += w;
        match item {
            Item::Sep => {
                painter.vline(r.center().x, (r.top() + 4.0)..=(r.bottom() - 4.0), Stroke::new(1.0, theme::SEPARATOR));
            }
            Item::Cmd(id) => {
                let enabled = state.is_enabled(id);
                let active = is_active(state, id);
                let resp = ui.interact(r, ui.scope_id().with(("navbar", id)), Sense::click());
                icons::paint_button_bg(&painter, r, &resp, enabled, active && id != CommandId::ToggleGrid);
                icons::paint(&painter, Rect::from_center_size(r.center(), vec2(18.0, 18.0)), Icon::Cmd(id), enabled);
                let clicked = resp.clicked();
                resp.on_hover_ui(|ui| ribbon::command_tooltip(ui, spec(id), enabled));
                if clicked && enabled {
                    ribbon::run_command(&ctx, state, id);
                }
            }
            Item::Display => {
                let resp = ui.interact(r, ui.scope_id().with("navbar-display"), Sense::click());
                icons::paint_button_bg(&painter, r, &resp, true, false);
                icons::paint(&painter, Rect::from_center_size(r.center() - vec2(5.0, 0.0), vec2(18.0, 18.0)), Icon::DisplaySettings, true);
                icons::caret(&painter, pos2(r.right() - 6.0, r.center().y + 1.0), 5.0, theme::TEXT);
                let resp = resp.on_hover_text("Display Settings");
                Popup::menu(&resp).show(|ui| display_menu(ui, state));
            }
            Item::Viewports => {
                let resp = ui.interact(r, ui.scope_id().with("navbar-viewports"), Sense::click());
                icons::paint_button_bg(&painter, r, &resp, false, false);
                icons::paint(&painter, Rect::from_center_size(r.center(), vec2(18.0, 18.0)), Icon::Viewports, false);
                resp.on_hover_text(format!("Multiple Views: {NOT_SUPPORTED}"));
            }
        }
    }
}

fn display_menu(ui: &mut Ui, state: &mut AppState) {
    icons::menu_caption(ui, "CAMERA");
    let persp = state.settings.perspective;
    if icons::menu_row(ui, Some(Icon::Cmd(CommandId::ToggleProjection)), "Orthographic", None, true, Some(!persp)).clicked() {
        if persp {
            state.execute(CommandId::ToggleProjection);
        }
        ui.close();
    }
    if icons::menu_row(ui, Some(Icon::Cmd(CommandId::ToggleProjection)), "Perspective", None, true, Some(persp)).clicked() {
        if !persp {
            state.execute(CommandId::ToggleProjection);
        }
        ui.close();
    }
    icons::menu_separator(ui);
    icons::menu_caption(ui, "DISPLAY");
    if icons::menu_row(ui, Some(Icon::Cmd(CommandId::ToggleGrid)), "Layout Grid", None, true, Some(state.settings.grid)).clicked() {
        state.execute(CommandId::ToggleGrid);
        ui.close();
    }
    for l in ["Visual Style", "Environment", "Effects", "Object Visibility"] {
        icons::menu_row(ui, None, l, None, false, None).on_hover_text(format!("{l}: {NOT_SUPPORTED}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nav_modes_highlight_and_toggle() {
        let mut s = AppState::new();
        assert!(!is_active(&s, CommandId::OrbitMode));
        s.execute(CommandId::OrbitMode);
        assert!(is_active(&s, CommandId::OrbitMode));
        s.execute(CommandId::PanMode);
        assert!(is_active(&s, CommandId::PanMode) && !is_active(&s, CommandId::OrbitMode));
        s.execute(CommandId::PanMode);
        assert_eq!(s.settings.nav, NavMode::Select);
    }
}
