//! Fusion-style timeline (F3, F30): playback buttons, one icon per feature, the history marker
//! and the settings gear.
//!
//! * features after the marker are rolled back (dimmed); suppressed features are dimmed and
//!   struck through; errors are red, warnings yellow; the selection is outlined in blue;
//! * click selects, double-click edits, right-click opens Edit / Rename / Suppress / Roll History
//!   Marker Here / Delete;
//! * drag the marker to roll back / forward (committed on release, one undo step);
//! * drag a feature to reorder it (rejected with a message when it would break a dependency).

use crate::state::{AppState, Pick};
use crate::ui::icons::{self, Icon};
use crate::ui::theme;
use cad_kernel::Health;
use egui::{pos2, vec2, Color32, CornerRadius, Id, Rect, Sense, Stroke, StrokeKind, Ui};

const SLOT: f32 = 28.0;
const BOX: f32 = 24.0;
const PLAY_STEP_S: f64 = 0.45;

/// Marker gap (0..=n) nearest to screen x for features laid out from `x0` with `slot` width.
pub fn gap_at(x: f32, x0: f32, slot: f32, n: usize) -> usize {
    (((x - x0) / slot).round().max(0.0) as usize).min(n)
}

/// Final index for `reorder_feature` when the feature at `from` is dropped into gap `gap`
/// (the index is taken after removing the feature).
pub fn reorder_target(from: usize, gap: usize) -> usize {
    if gap > from {
        gap - 1
    } else {
        gap
    }
}

/// Marker after the "step back" button.
pub fn step_back(marker: usize) -> usize {
    marker.saturating_sub(1)
}

/// Marker after the "step forward" button.
pub fn step_forward(marker: usize, n: usize) -> usize {
    (marker + 1).min(n)
}

#[derive(Clone, Debug, Default)]
struct Rename {
    id: String,
    buf: String,
    focused: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Act {
    Select(String, bool),
    Edit(String),
    StartRename(String, String),
    Suppress(String),
    Marker(usize),
    Delete(String),
    Reorder(String, usize),
}

fn mem<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, key: &str) -> Option<T> {
    ctx.data_mut(|d| d.get_temp::<T>(Id::unique(("cad360.timeline", key))))
}
fn set_mem<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, key: &str, v: Option<T>) {
    let id = Id::unique(("cad360.timeline", key));
    ctx.data_mut(|d| match v {
        Some(v) => {
            d.insert_temp(id, v);
        }
        None => {
            d.remove::<T>(id);
        }
    });
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let full = ui.max_rect().expand2(vec2(8.0, 2.0));
    let painter = ui.painter().clone();
    painter.rect_filled(full, CornerRadius::ZERO, theme::TIMELINE_BG);
    painter.hline(full.x_range(), full.top() + 0.5, Stroke::new(1.0, Color32::from_rgb(0x1f, 0x21, 0x25)));
    let cy = full.center().y;
    let n = state.doc.features.len();
    let marker = state.doc.marker.min(n);
    let locked = state.in_txn();
    let now = ctx.input(|i| i.time);

    playback(ui, state, full, cy, n, marker, locked, now);

    // settings gear (right)
    let gr = Rect::from_center_size(pos2(full.right() - 22.0, cy), vec2(24.0, 24.0));
    let gresp = ui.interact(gr, ui.scope_id().with("tl-gear"), Sense::click());
    icons::paint_button_bg(&painter, gr, &gresp, true, false);
    icons::paint(&painter, gr.shrink(4.0), Icon::Gear, true);
    if gresp.on_hover_text("Timeline settings").clicked() {
        state.info("Timeline settings (turning design history capture off) are not yet supported in CAD360.");
    }

    // ---- feature strip
    let x0 = full.left() + 160.0;
    let avail = (gr.left() - 16.0 - x0).max(SLOT);
    let slot = if n == 0 { SLOT } else { (avail / n as f32).min(SLOT).max(10.0) };
    let additive = ctx.input(|i| i.modifiers.command || i.modifiers.shift);
    let pointer = ctx.pointer_interact_pos();
    let marker_preview: Option<usize> = mem(&ctx, "marker-drag");
    let shown_marker = marker_preview.unwrap_or(marker);
    let dragging: Option<String> = mem(&ctx, "feature-drag");
    let mut acts: Vec<Act> = Vec::new();

    for (i, f) in state.doc.features.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(x0 + i as f32 * slot, cy - BOX * 0.5), vec2(slot, BOX));
        let b = Rect::from_center_size(cell.center(), vec2((slot - 4.0).min(BOX), BOX));
        let resp = ui.interact(b, ui.scope_id().with(("tl-feature", &f.id)), Sense::click_and_drag());
        let health = state.health(&f.id);
        let rolled = i >= shown_marker;
        let selected = state.selection.iter().any(|p| matches!(p, Pick::Feature(x) if *x == f.id));
        let r = CornerRadius::same(3);
        match &health {
            Health::Error(_) => {
                painter.rect_filled(b, r, theme::ERROR.gamma_multiply(if rolled { 0.35 } else { 0.85 }));
            }
            Health::Warning(_) => {
                painter.rect_filled(b, r, theme::WARNING.gamma_multiply(if rolled { 0.35 } else { 0.85 }));
            }
            _ => {
                if resp.hovered() {
                    painter.rect_filled(b, r, theme::HOVER_BG);
                }
            }
        }
        let being_dragged = dragging.as_deref() == Some(f.id.as_str());
        let icon_r = Rect::from_center_size(b.center(), vec2(18.0, 18.0).min(b.size() - vec2(2.0, 4.0)));
        if !being_dragged {
            icons::paint(&painter, icon_r, icons::feature_icon(&f.kind), !(rolled || f.suppressed));
        }
        if rolled {
            painter.rect_filled(b, r, theme::TIMELINE_BG.gamma_multiply(0.45));
        }
        if f.suppressed {
            painter.line_segment([b.left_bottom() + vec2(3.0, -3.0), b.right_top() + vec2(-3.0, 3.0)], Stroke::new(1.6, theme::ERROR.gamma_multiply(0.9)));
        }
        if selected {
            painter.rect_stroke(b, r, Stroke::new(2.0, theme::ACCENT), StrokeKind::Outside);
        }

        // interactions
        let tip = feature_tooltip(&f.name, f.type_label(), &health, rolled, f.suppressed);
        if resp.drag_started() && !locked {
            set_mem(&ctx, "feature-drag", Some(f.id.clone()));
        }
        if resp.drag_stopped() {
            if let (Some(p), true) = (pointer, dragging.as_deref() == Some(f.id.as_str())) {
                let gap = gap_at(p.x, x0, slot, n);
                let to = reorder_target(i, gap);
                if to != i {
                    acts.push(Act::Reorder(f.id.clone(), to));
                }
            }
            set_mem::<String>(&ctx, "feature-drag", None);
        }
        if resp.double_clicked() {
            acts.push(Act::Edit(f.id.clone()));
        } else if resp.clicked() {
            acts.push(Act::Select(f.id.clone(), additive));
        }
        if resp.secondary_clicked() && !selected {
            acts.push(Act::Select(f.id.clone(), false));
        }
        let resp = if dragging.is_none() { resp.on_hover_text(tip) } else { resp };
        let fid = f.id.clone();
        let fname = f.name.clone();
        let suppressed = f.suppressed;
        resp.context_menu(|ui| {
            if icons::menu_row(ui, Some(icons::feature_icon(&f.kind)), "Edit Feature", None, !locked, None).clicked() && !locked {
                acts.push(Act::Edit(fid.clone()));
                ui.close();
            }
            if icons::menu_row(ui, None, "Rename...", None, !locked, None).clicked() && !locked {
                acts.push(Act::StartRename(fid.clone(), fname.clone()));
                ui.close();
            }
            let l = if suppressed { "Unsuppress Features" } else { "Suppress Features" };
            if icons::menu_row(ui, None, l, None, !locked, None).clicked() && !locked {
                acts.push(Act::Suppress(fid.clone()));
                ui.close();
            }
            if icons::menu_row(ui, Some(Icon::StepBack), "Roll History Marker Here", None, !locked, None).clicked() && !locked {
                acts.push(Act::Marker(i));
                ui.close();
            }
            icons::menu_separator(ui);
            if icons::menu_row(ui, Some(Icon::Cmd(crate::state::command::CommandId::Delete)), "Delete", Some("Delete"), !locked, None).clicked() && !locked {
                acts.push(Act::Delete(fid.clone()));
                ui.close();
            }
        });
    }

    // drag ghost + insertion indicator for feature reordering
    if let (Some(id), Some(p)) = (dragging.as_ref(), pointer) {
        if let Some(f) = state.doc.feature(id) {
            let gap = gap_at(p.x, x0, slot, n);
            let gx = x0 + gap as f32 * slot;
            painter.vline(gx, (cy - BOX * 0.6)..=(cy + BOX * 0.6), Stroke::new(2.0, theme::ACCENT));
            let g = Rect::from_center_size(pos2(p.x, cy), vec2(BOX, BOX));
            painter.rect_filled(g, CornerRadius::same(3), theme::ACCENT_DIM);
            icons::paint(&painter, g.shrink(3.0), icons::feature_icon(&f.kind), true);
            ctx.request_repaint();
        }
        if !ctx.input(|i| i.pointer.any_down()) {
            set_mem::<String>(&ctx, "feature-drag", None);
        }
    }

    // ---- history marker
    let mx = x0 + shown_marker as f32 * slot;
    let hit = Rect::from_center_size(pos2(mx, cy), vec2(10.0, full.height() - 4.0));
    let mresp = ui.interact(hit, ui.scope_id().with("tl-marker"), Sense::drag());
    let hot = mresp.hovered() || mresp.dragged();
    let col = if hot { theme::TEXT_STRONG } else { Color32::from_rgb(0xc4, 0xc8, 0xce) };
    painter.vline(mx, (cy - BOX * 0.55)..=(cy + BOX * 0.6), Stroke::new(2.5, col));
    let knob = Rect::from_center_size(pos2(mx + 2.5, cy - BOX * 0.55), vec2(9.0, 7.0));
    painter.rect_filled(knob, CornerRadius::same(1), col);
    if !locked {
        if mresp.dragged() {
            if let Some(p) = pointer {
                set_mem(&ctx, "marker-drag", Some(gap_at(p.x, x0, slot, n)));
            }
        }
        if mresp.drag_stopped() {
            if let Some(g) = marker_preview {
                acts.push(Act::Marker(g));
            }
            set_mem::<usize>(&ctx, "marker-drag", None);
        }
        mresp.on_hover_text("History marker: drag to roll the design back or forward").on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    } else if marker_preview.is_some() {
        set_mem::<usize>(&ctx, "marker-drag", None);
    }

    rename_editor(&ctx, state, x0, slot, cy);
    apply(&ctx, state, acts);
}

#[allow(clippy::too_many_arguments)]
fn playback(ui: &mut Ui, state: &mut AppState, full: Rect, cy: f32, n: usize, marker: usize, locked: bool, now: f64) {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let playing: Option<f64> = mem(&ctx, "play");
    let buttons = [
        (Icon::ToStart, "Move to beginning", "start"),
        (Icon::StepBack, "Step back one feature", "back"),
        (Icon::Play, if playing.is_some() { "Stop" } else { "Play the design history" }, "play"),
        (Icon::StepForward, "Step forward one feature", "fwd"),
        (Icon::ToEnd, "Move to end", "end"),
    ];
    for (k, (icon, tip, key)) in buttons.iter().enumerate() {
        let r = Rect::from_center_size(pos2(full.left() + 20.0 + k as f32 * 25.0, cy), vec2(23.0, 23.0));
        let resp = ui.interact(r, ui.scope_id().with(("tl-play", *key)), Sense::click());
        let enabled = !locked && n > 0;
        icons::paint_button_bg(&painter, r, &resp, enabled, *key == "play" && playing.is_some());
        icons::paint(&painter, r.shrink(4.0), *icon, enabled);
        if resp.on_hover_text(*tip).clicked() && enabled {
            set_mem::<f64>(&ctx, "play", None);
            match *key {
                "start" => state.set_marker(0),
                "back" => state.set_marker(step_back(marker)),
                "fwd" => state.set_marker(step_forward(marker, n)),
                "end" => state.set_marker(n),
                _ => {
                    if playing.is_none() {
                        if marker >= n {
                            state.set_marker(0);
                        }
                        set_mem(&ctx, "play", Some(now + PLAY_STEP_S));
                    }
                }
            }
        }
    }
    // playback animation: one feature per step. Steps move the marker directly (no undo entry
    // per step); only a replay started from the end records one ("Move Timeline Marker" to 0).
    if let Some(next) = mem::<f64>(&ctx, "play") {
        if locked {
            set_mem::<f64>(&ctx, "play", None);
        } else if now >= next {
            let m = state.doc.marker;
            if m < state.doc.features.len() {
                state.doc.set_marker(m + 1);
                state.dirty = true;
                state.recompute();
                set_mem(&ctx, "play", Some(now + PLAY_STEP_S));
            } else {
                set_mem::<f64>(&ctx, "play", None);
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

fn feature_tooltip(name: &str, type_label: &str, health: &Health, rolled: bool, suppressed: bool) -> String {
    let msg = match health {
        Health::Error(m) | Health::Warning(m) => m.clone(),
        _ if suppressed => "Suppressed".to_string(),
        _ if rolled => "Rolled back (after the history marker)".to_string(),
        _ => type_label.to_string(),
    };
    format!("{name} — {msg}")
}

fn rename_editor(ctx: &egui::Context, state: &mut AppState, x0: f32, slot: f32, cy: f32) {
    let Some(mut rn) = mem::<Rename>(ctx, "rename") else { return };
    let Some(idx) = state.doc.feature_index(&rn.id) else {
        set_mem::<Rename>(ctx, "rename", None);
        return;
    };
    let pos = pos2(x0 + idx as f32 * slot - 4.0, cy - BOX * 0.5 - 34.0);
    let mut done: Option<bool> = None; // Some(true) commit, Some(false) cancel
    egui::Area::new(Id::unique("cad360.timeline.rename-area")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            let te = ui.add(egui::TextEdit::singleline(&mut rn.buf).desired_width(170.0));
            if !rn.focused {
                te.request_focus();
                rn.focused = true;
            }
            let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
            if esc {
                done = Some(false);
            } else if enter || te.lost_focus() {
                done = Some(true);
            }
        });
    });
    match done {
        Some(true) => {
            state.rename_feature(&rn.id, &rn.buf);
            set_mem::<Rename>(ctx, "rename", None);
        }
        Some(false) => set_mem::<Rename>(ctx, "rename", None),
        None => set_mem(ctx, "rename", Some(rn)),
    }
}

fn apply(ctx: &egui::Context, state: &mut AppState, acts: Vec<Act>) {
    for a in acts {
        match a {
            Act::Select(id, add) => state.select(Pick::Feature(id), add),
            Act::Edit(id) => state.edit_feature(&id),
            Act::StartRename(id, name) => set_mem(ctx, "rename", Some(Rename { id, buf: name, focused: false })),
            Act::Suppress(id) => state.toggle_suppress(&id),
            Act::Marker(m) => state.set_marker(m),
            Act::Delete(id) => state.delete_feature(&id),
            Act::Reorder(id, to) => reorder(state, &id, to),
        }
    }
}

/// Moves a feature in the timeline as one undo step; an invalid move is rejected with a message.
pub fn reorder(state: &mut AppState, id: &str, to: usize) {
    let name = state.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    let fid = id.to_string();
    state.apply(&format!("Reorder {name}"), |d| {
        if d.reorder_feature(&fid, to) {
            Ok(())
        } else {
            Err(format!("Cannot move {name} there: a feature must stay after the features it depends on and before the features that use it."))
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_gap_from_pointer() {
        // features at x0=100, 28 px each, 4 features
        assert_eq!(gap_at(90.0, 100.0, 28.0, 4), 0);
        assert_eq!(gap_at(100.0, 100.0, 28.0, 4), 0);
        assert_eq!(gap_at(113.0, 100.0, 28.0, 4), 0);
        assert_eq!(gap_at(115.0, 100.0, 28.0, 4), 1);
        assert_eq!(gap_at(156.0, 100.0, 28.0, 4), 2);
        assert_eq!(gap_at(500.0, 100.0, 28.0, 4), 4);
    }

    #[test]
    fn reorder_target_accounts_for_removal() {
        assert_eq!(reorder_target(0, 0), 0);
        assert_eq!(reorder_target(0, 1), 0, "dropping right next to itself is a no-op");
        assert_eq!(reorder_target(0, 3), 2);
        assert_eq!(reorder_target(3, 1), 1);
        assert_eq!(reorder_target(3, 4), 3);
    }

    #[test]
    fn step_buttons_clamp() {
        assert_eq!(step_back(0), 0);
        assert_eq!(step_back(3), 2);
        assert_eq!(step_forward(3, 3), 3);
        assert_eq!(step_forward(1, 3), 2);
    }

    #[test]
    fn tooltip_shows_health_message() {
        assert_eq!(feature_tooltip("Fillet1", "Fillet", &Health::Error("radius too large".into()), false, false), "Fillet1 — radius too large");
        assert_eq!(feature_tooltip("Sketch1", "Sketch", &Health::Healthy, false, false), "Sketch1 — Sketch");
        assert!(feature_tooltip("E", "Extrude", &Health::Healthy, true, false).contains("Rolled back"));
    }

    #[test]
    fn invalid_reorder_is_rejected_with_message() {
        let mut s = AppState::new();
        // two independent offset planes can swap; a missing feature is rejected
        let before = s.doc.clone();
        reorder(&mut s, "does-not-exist", 0);
        assert_eq!(s.doc, before);
        assert!(s.messages.iter().any(|m| m.text.starts_with("Cannot move")));
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use crate::state::command::CommandId;
    use crate::ui::icons::testkit::Kit;
    use egui::pos2;

    fn two_sketches() -> AppState {
        let mut s = AppState::new();
        for _ in 0..2 {
            s.select(Pick::OriginPlane(cad_core::OriginPlane::XY), false);
            s.execute(CommandId::CreateSketch);
            s.execute(CommandId::FinishSketch);
        }
        s
    }

    fn run(kit: &mut Kit, s: &mut AppState, act: impl FnOnce(&mut Kit, &mut dyn FnMut(&mut Ui))) {
        let mut f = |ui: &mut Ui| {
            egui::Panel::bottom("timeline").exact_size(40.0).show(ui, |ui| show(ui, s));
        };
        kit.idle(2, &mut f);
        act(kit, &mut f);
    }

    #[test]
    fn playback_buttons_and_marker_drag() {
        let mut s = two_sketches();
        assert_eq!(s.doc.features.len(), 2);
        assert_eq!(s.doc.marker, 2);
        let mut kit = Kit::new();
        let cy = 940.0;
        run(&mut kit, &mut s, |k, f| k.click(pos2(20.0, cy), f));
        assert_eq!(s.doc.marker, 0, "to start");
        run(&mut kit, &mut s, |k, f| k.click(pos2(95.0, cy), f));
        assert_eq!(s.doc.marker, 1, "step forward");
        run(&mut kit, &mut s, |k, f| k.click(pos2(120.0, cy), f));
        assert_eq!(s.doc.marker, 2, "to end");
        // drag the marker (at x0 + 2 * slot) back to the start: committed on release
        run(&mut kit, &mut s, |k, f| k.drag(pos2(160.0 + 2.0 * SLOT, cy), pos2(163.0, cy), f));
        assert_eq!(s.doc.marker, 0, "marker drag");
        assert_eq!(s.history.undo_label(), Some("Move Timeline Marker"));
    }

    #[test]
    fn click_selects_and_drag_reorders() {
        let mut s = two_sketches();
        let first = s.doc.features[0].id.clone();
        let mut kit = Kit::new();
        let cy = 940.0;
        run(&mut kit, &mut s, |k, f| k.click(pos2(160.0 + SLOT * 0.5, cy), f));
        assert_eq!(s.selection, vec![Pick::Feature(first.clone())]);
        // drag the first feature past the second (independent sketches may swap)
        run(&mut kit, &mut s, |k, f| k.drag(pos2(160.0 + SLOT * 0.5, cy), pos2(160.0 + SLOT * 2.0 + 4.0, cy), f));
        assert_eq!(s.doc.features[1].id, first, "reordered");
        assert_eq!(s.doc.marker, 2);
    }
}

#[cfg(test)]
mod rename_tests {
    use super::*;
    use crate::state::command::CommandId;
    use crate::state::Mode;
    use crate::ui::icons::testkit::Kit;
    use egui::{Event, Key, Modifiers};

    fn key(k: Key) -> Event {
        Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::default() }
    }

    fn one_sketch() -> AppState {
        let mut s = AppState::new();
        s.select(Pick::OriginPlane(cad_core::OriginPlane::XY), false);
        s.execute(CommandId::CreateSketch);
        s.execute(CommandId::FinishSketch);
        s
    }

    fn rename_with(final_key: Key) -> AppState {
        let mut s = one_sketch();
        let id = s.doc.features[0].id.clone();
        let name = s.doc.features[0].name.clone();
        let mut kit = Kit::new();
        apply(&kit.ctx.clone(), &mut s, vec![Act::StartRename(id, name)]);
        {
            let mut f = |ui: &mut Ui| {
                egui::Panel::bottom("timeline").exact_size(40.0).show(ui, |ui| show(ui, &mut s));
            };
            kit.idle(3, &mut f);
            kit.frame(vec![Event::Text("X".into())], &mut f);
            kit.frame(vec![key(final_key)], &mut f);
            kit.idle(2, &mut f);
        }
        s
    }

    #[test]
    fn inline_rename_commits_on_enter() {
        let s = rename_with(Key::Enter);
        assert_eq!(s.doc.features[0].name, "Sketch1X");
    }

    #[test]
    fn inline_rename_cancels_on_escape() {
        let s = rename_with(Key::Escape);
        assert_eq!(s.doc.features[0].name, "Sketch1");
    }

    #[test]
    fn double_click_edits_the_sketch() {
        let mut s = one_sketch();
        let mut kit = Kit::new();
        let p = egui::pos2(160.0 + SLOT * 0.5, 940.0);
        {
            let mut f = |ui: &mut Ui| {
                egui::Panel::bottom("timeline").exact_size(40.0).show(ui, |ui| show(ui, &mut s));
            };
            kit.idle(2, &mut f);
            kit.frame(vec![Event::PointerMoved(p)], &mut f);
            for _ in 0..2 {
                kit.frame(vec![Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::default() }], &mut f);
                kit.frame(vec![Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::default() }], &mut f);
            }
            kit.idle(1, &mut f);
        }
        assert!(matches!(s.mode, Mode::Sketch(_)), "double-click opens the sketch for editing");
    }
}
