//! Fusion-like marking menu on canvas right-click: eight commands on a ring around the pointer
//! (Repeat, Undo, Delete, Create Sketch / Finish Sketch, Extrude, Fillet, Hole, Look At, or the
//! sketch tools while sketching) and an overflow list below (Redo, Fit, Hide, Measure, Change
//! Parameters ...). Everything runs through the command registry.

use crate::state::command::{spec, CommandId};
use crate::state::{AppState, Mode, Pick};
use crate::ui::icons::{self, Icon};
use crate::ui::{ribbon, theme};
use egui::{pos2, vec2, Align2, CornerRadius, Frame, Popup, Pos2, Rect, RectAlign, Response, Sense, Stroke, StrokeKind, Ui};

#[derive(Clone, Debug, PartialEq)]
pub enum MAction {
    Cmd(CommandId),
    HideBodies(Vec<String>),
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub label: String,
    pub icon: Icon,
    pub enabled: bool,
    pub action: MAction,
}

fn cmd(state: &AppState, id: CommandId) -> Entry {
    Entry { label: spec(id).label.to_string(), icon: Icon::Cmd(id), enabled: state.is_enabled(id), action: MAction::Cmd(id) }
}

/// Bodies touched by the selection (body, face or edge picks), deduplicated.
pub fn selected_bodies(sel: &[Pick]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for p in sel {
        if let Pick::Body { body } | Pick::Face { body, .. } | Pick::Edge { body, .. } = p {
            if !v.contains(body) {
                v.push(body.clone());
            }
        }
    }
    v
}

/// The ring (N, NE, E, SE, S, SW, W, NW) and overflow entries for the current mode.
pub fn entries(state: &AppState, last: Option<CommandId>) -> ([Entry; 8], Vec<Entry>) {
    use CommandId::*;
    let repeat = match last {
        Some(id) => Entry { label: format!("Repeat {}", spec(id).label), icon: Icon::Cmd(id), enabled: state.is_enabled(id), action: MAction::Cmd(id) },
        None => Entry { label: "Repeat".into(), icon: Icon::Cmd(ComputeAll), enabled: false, action: MAction::Cmd(ComputeAll) },
    };
    let sketching = matches!(state.mode, Mode::Sketch(_));
    if sketching {
        let ring = [repeat, cmd(state, FinishSketch), cmd(state, Line), cmd(state, Rectangle2Pt), cmd(state, CircleCenter), cmd(state, SketchDimension), cmd(state, Undo), cmd(state, Delete)];
        let more = vec![cmd(state, Redo), cmd(state, LookAt), cmd(state, Fit), cmd(state, ToggleConstruction), cmd(state, Measure), cmd(state, ChangeParameters)];
        (ring, more)
    } else {
        let ring = [repeat, cmd(state, CreateSketch), cmd(state, Extrude), cmd(state, Fillet), cmd(state, Hole), cmd(state, LookAt), cmd(state, Undo), cmd(state, Delete)];
        let mut more = vec![cmd(state, Redo), cmd(state, Fit)];
        let bodies = selected_bodies(&state.selection);
        if !bodies.is_empty() {
            more.push(Entry { label: if bodies.len() == 1 { "Hide Body".into() } else { format!("Hide {} Bodies", bodies.len()) }, icon: Icon::EyeOff, enabled: true, action: MAction::HideBodies(bodies) });
            more.push(cmd(state, Appearance));
        }
        more.push(cmd(state, Measure));
        more.push(cmd(state, ChangeParameters));
        more.push(cmd(state, Chamfer));
        more.push(cmd(state, OffsetPlane));
        (ring, more)
    }
}

const SIZE: egui::Vec2 = vec2(400.0, 500.0);
const RX: f32 = 118.0;
const RY: f32 = 86.0;
const PILL: egui::Vec2 = vec2(132.0, 26.0);

pub fn show(response: &Response, state: &mut AppState) {
    Popup::context_menu(response)
        .align(RectAlign { parent: Align2::CENTER_CENTER, child: Align2::CENTER_CENTER })
        .align_alternatives(&[])
        .frame(Frame::NONE)
        .gap(0.0)
        .show(|ui| marking_menu(ui, state));
}

fn marking_menu(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let (rect, _) = ui.allocate_exact_size(SIZE, Sense::hover());
    let c = rect.center();
    let painter = ui.painter().clone();
    let (ring, more) = entries(state, ribbon::last_command(&ctx));
    let mut chosen: Option<MAction> = None;

    // centre hub
    painter.circle_filled(c, 11.0, theme::PANEL_BG.gamma_multiply(0.95));
    painter.circle_stroke(c, 11.0, Stroke::new(1.5, theme::SEPARATOR));
    painter.circle_filled(c, 3.0, theme::TEXT_WEAK);

    for (k, e) in ring.iter().enumerate() {
        let a = (k as f32 * 45.0).to_radians();
        let (s, co) = (a.sin(), a.cos());
        let anchor = pos2(c.x + RX * s, c.y - RY * co);
        let r = if s > 0.3 {
            Rect::from_min_size(pos2(anchor.x - 18.0, anchor.y - PILL.y * 0.5), PILL)
        } else if s < -0.3 {
            Rect::from_min_size(pos2(anchor.x + 18.0 - PILL.x, anchor.y - PILL.y * 0.5), PILL)
        } else {
            Rect::from_center_size(anchor, PILL)
        };
        let resp = ui.interact(r, ui.scope_id().with(("marking-ring", k)), Sense::click());
        if resp.hovered() {
            painter.line_segment([c + (closest(r, c) - c).normalized() * 11.0, closest(r, c)], Stroke::new(2.0, theme::ACCENT));
        }
        pill(&painter, r, e, resp.hovered());
        if let Some(a) = handle(resp, e) {
            chosen = Some(a);
        }
    }

    // overflow list
    let top = c.y + RY + 34.0;
    let w = 190.0;
    let list = Rect::from_min_size(pos2(c.x - w * 0.5, top), vec2(w, more.len() as f32 * 24.0 + 6.0));
    painter.rect_filled(list.translate(vec2(0.0, 2.0)), CornerRadius::same(4), egui::Color32::from_black_alpha(70));
    painter.rect_filled(list, CornerRadius::same(4), theme::PANEL_BG);
    painter.rect_stroke(list, CornerRadius::same(4), Stroke::new(1.0, theme::SEPARATOR), StrokeKind::Inside);
    for (k, e) in more.iter().enumerate() {
        let r = Rect::from_min_size(pos2(list.left() + 3.0, list.top() + 3.0 + k as f32 * 24.0), vec2(w - 6.0, 24.0));
        let resp = ui.interact(r, ui.scope_id().with(("marking-more", k)), Sense::click());
        if resp.hovered() {
            painter.rect_filled(r, CornerRadius::same(3), if e.enabled { theme::ACCENT_DIM } else { theme::HOVER_BG });
        }
        icons::paint(&painter, Rect::from_center_size(pos2(r.left() + 13.0, r.center().y), vec2(16.0, 16.0)), e.icon, e.enabled);
        painter.text(pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, &e.label, theme::body_font(), if e.enabled { theme::TEXT } else { theme::TEXT_DISABLED });
        if let MAction::Cmd(id) = e.action {
            if let Some(k) = spec(id).shortcut {
                painter.text(pos2(r.right() - 6.0, r.center().y), Align2::RIGHT_CENTER, k, egui::FontId::proportional(11.0), theme::TEXT_WEAK);
            }
        }
        if let Some(a) = handle(resp, e) {
            chosen = Some(a);
        }
    }

    if let Some(a) = chosen {
        match a {
            MAction::Cmd(id) => ribbon::run_command(&ctx, state, id),
            MAction::HideBodies(bodies) => {
                state.apply("Hide Bodies", |d| {
                    for b in bodies {
                        d.bodies.entry(b).or_default().visible = false;
                    }
                    Ok(())
                });
                state.selection.retain(|p| !matches!(p, Pick::Body { .. } | Pick::Face { .. } | Pick::Edge { .. }));
            }
        }
        ui.close();
    }
}

fn closest(r: Rect, p: Pos2) -> Pos2 {
    pos2(p.x.clamp(r.left(), r.right()), p.y.clamp(r.top(), r.bottom()))
}

fn pill(painter: &egui::Painter, r: Rect, e: &Entry, hovered: bool) {
    let fill = if hovered && e.enabled { theme::ACCENT_DIM } else if hovered { theme::HOVER_BG } else { theme::PANEL_BG.gamma_multiply(0.97) };
    painter.rect_filled(r.translate(vec2(0.0, 2.0)), CornerRadius::same(13), egui::Color32::from_black_alpha(60));
    painter.rect_filled(r, CornerRadius::same(13), fill);
    painter.rect_stroke(r, CornerRadius::same(13), Stroke::new(1.0, if hovered { theme::ACCENT } else { theme::SEPARATOR }), StrokeKind::Inside);
    icons::paint(painter, Rect::from_center_size(pos2(r.left() + 15.0, r.center().y), vec2(17.0, 17.0)), e.icon, e.enabled);
    painter.text(pos2(r.left() + 29.0, r.center().y), Align2::LEFT_CENTER, &e.label, egui::FontId::proportional(12.0), if e.enabled { theme::TEXT } else { theme::TEXT_DISABLED });
}

/// Tooltip + click handling for an entry; returns the action when it should run.
fn handle(resp: Response, e: &Entry) -> Option<MAction> {
    let clicked = resp.clicked();
    if let MAction::Cmd(id) = e.action {
        if e.label != "Repeat" {
            resp.on_hover_ui(|ui| ribbon::command_tooltip(ui, spec(id), e.enabled));
        } else {
            resp.on_hover_text("Repeats the last command run from the ribbon or this menu");
        }
    }
    (clicked && e.enabled).then(|| e.action.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ring_has_fusion_commands() {
        let s = AppState::new();
        let (ring, more) = entries(&s, Some(CommandId::Extrude));
        let labels: Vec<&str> = ring.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels[0], "Repeat Extrude");
        for want in ["Create Sketch", "Extrude", "Fillet", "Hole", "Look At", "Undo", "Delete"] {
            assert!(labels.contains(&want), "{want} missing: {labels:?}");
        }
        let more: Vec<&str> = more.iter().map(|e| e.label.as_str()).collect();
        for want in ["Redo", "Fit", "Measure", "Change Parameters"] {
            assert!(more.contains(&want), "{want} missing: {more:?}");
        }
        assert!(!more.iter().any(|l| l.starts_with("Hide")), "no Hide without selected bodies");
    }

    #[test]
    fn repeat_is_disabled_without_history() {
        let s = AppState::new();
        let (ring, _) = entries(&s, None);
        assert!(!ring[0].enabled);
    }

    #[test]
    fn hide_offered_for_selected_bodies() {
        let sel = vec![Pick::Face { body: "b1".into(), face: 0 }, Pick::Edge { body: "b1".into(), edge: 2 }, Pick::Body { body: "b2".into() }, Pick::OriginPlane(cad_core::OriginPlane::XY)];
        assert_eq!(selected_bodies(&sel), vec!["b1".to_string(), "b2".to_string()]);
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use crate::state::ViewRequest;
    use crate::ui::icons::testkit::Kit;
    use egui::PointerButton;

    #[test]
    fn right_click_opens_marking_menu_and_fit_runs() {
        let mut s = AppState::new();
        let mut kit = Kit::new();
        let at = pos2(800.0, 300.0);
        {
            let mut f = |ui: &mut Ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let rect = ui.max_rect();
                    let resp = ui.interact(rect, egui::Id::unique("test-canvas"), Sense::click_and_drag());
                    show(&resp, &mut s);
                });
            };
            kit.idle(2, &mut f);
            kit.click_with(at, PointerButton::Secondary, &mut f);
            kit.idle(2, &mut f);
            // overflow list: Redo, Fit, ... below the ring
            let fit = pos2(at.x, at.y + RY + 34.0 + 3.0 + 24.0 + 12.0);
            kit.click(fit, &mut f);
        }
        assert!(s.view_requests.contains(&ViewRequest::Fit), "{:?}", s.view_requests);
    }
}
