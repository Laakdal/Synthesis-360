//! Messages at the bottom-right of the canvas: the three most recent messages, coloured by
//! level, fading out after a few seconds, plus a message-log toggle that lists every message.

use crate::state::{AppState, Level};
use crate::ui::icons::{self, Icon};
use crate::ui::theme;
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Id, Rect, RichText, Sense, Stroke, StrokeKind, Ui};
use std::hash::{Hash, Hasher};

/// Seconds a message stays fully visible.
pub const VISIBLE_S: f64 = 5.0;
/// Seconds of the fade-out after that.
pub const FADE_S: f64 = 1.0;
const MAX_SHOWN: usize = 3;

/// Arrival time of each message, kept parallel to `AppState::messages` (index-keyed).
#[derive(Clone, Default, Debug)]
pub struct Clock {
    pub times: Vec<f64>,
    pub last_hash: u64,
}

impl Clock {
    /// Brings the clock in line with the message list: new messages get `now`; when the list is
    /// capped (oldest dropped, same length, different last message) times shift by one; when it
    /// shrinks (cleared) old messages are no longer shown.
    pub fn sync(&mut self, len: usize, last_hash: u64, now: f64) {
        if len < self.times.len() {
            self.times = vec![f64::NEG_INFINITY; len];
        } else if len > self.times.len() {
            let add = len - self.times.len();
            self.times.extend(std::iter::repeat_n(now, add));
        } else if len > 0 && last_hash != self.last_hash {
            self.times.remove(0);
            self.times.push(now);
        }
        self.last_hash = last_hash;
    }
}

/// Opacity of a message of the given age (1 -> fully visible, 0 -> gone).
pub fn alpha(age: f64) -> f32 {
    if age <= VISIBLE_S {
        1.0
    } else {
        (1.0 - (age - VISIBLE_S) / FADE_S).clamp(0.0, 1.0) as f32
    }
}

fn level_style(l: Level) -> (Icon, Color32) {
    match l {
        Level::Info => (Icon::Info, theme::TEXT),
        Level::Warning => (Icon::Warning, theme::WARNING),
        Level::Error => (Icon::Error, Color32::from_rgb(0xff, 0x8a, 0x8d)),
    }
}

fn hash_last(state: &AppState) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if let Some(m) = state.messages.last() {
        m.text.hash(&mut h);
        (m.level as u8).hash(&mut h);
    }
    state.messages.len().hash(&mut h);
    h.finish()
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let rect = ui.max_rect();
    let now = ctx.input(|i| i.time);
    let clock_id = Id::unique("cad360.statusbar.clock");
    let log_id = Id::unique("cad360.statusbar.log_open");
    let mut clock: Clock = ctx.data_mut(|d| d.get_temp(clock_id).unwrap_or_default());
    clock.sync(state.messages.len(), hash_last(state), now);

    let painter = ui.painter().clone();

    // message-log toggle (bottom-right corner)
    let br = Rect::from_min_size(rect.right_bottom() - vec2(28.0, 24.0), vec2(26.0, 22.0));
    let bresp = ui.interact(br, ui.scope_id().with("status-log"), Sense::click());
    let log_open = ctx.data_mut(|d| d.get_temp::<bool>(log_id).unwrap_or(false));
    painter.rect_filled(br, CornerRadius::same(4), theme::PANEL_BG.gamma_multiply(0.95));
    icons::paint_button_bg(&painter, br, &bresp, true, log_open);
    icons::paint(&painter, Rect::from_center_size(br.center(), vec2(14.0, 14.0)), Icon::Log, true);
    let errors = state.messages.iter().filter(|m| m.level == Level::Error).count();
    if errors > 0 {
        painter.circle_filled(br.right_top() + vec2(-3.0, 3.0), 3.5, theme::ERROR);
    }
    if bresp.on_hover_text(format!("Message log ({} messages)", state.messages.len())).clicked() {
        ctx.data_mut(|d| d.insert_temp(log_id, !log_open));
    }

    // recent messages, newest at the bottom
    let mut y = rect.bottom() - 2.0;
    let mut soonest: Option<f64> = None;
    let n = state.messages.len();
    let mut dismiss: Option<usize> = None;
    for idx in (n.saturating_sub(MAX_SHOWN)..n).rev() {
        let age = now - clock.times.get(idx).copied().unwrap_or(f64::NEG_INFINITY);
        let a = alpha(age);
        if a <= 0.0 {
            continue;
        }
        let wait = if age < VISIBLE_S { VISIBLE_S - age } else { 0.03 };
        soonest = Some(soonest.map_or(wait, |s: f64| s.min(wait)));
        let m = &state.messages[idx];
        let (icon, col) = level_style(m.level);
        let mut job = egui::text::LayoutJob::simple_singleline(m.text.clone(), FontId::proportional(12.0), col.gamma_multiply(a));
        job.wrap = egui::text::TextWrapping { max_width: (rect.width() - 80.0).max(60.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
        let galley = painter.layout_job(job);
        let w = galley.size().x + 32.0;
        let pill = Rect::from_min_max(pos2(br.left() - 6.0 - w, y - 21.0), pos2(br.left() - 6.0, y));
        let fill = theme::PANEL_BG.gamma_multiply(0.94 * a);
        painter.rect_filled(pill, CornerRadius::same(4), fill);
        let edge = match m.level {
            Level::Info => theme::SEPARATOR,
            Level::Warning => theme::WARNING,
            Level::Error => theme::ERROR,
        };
        painter.rect_stroke(pill, CornerRadius::same(4), Stroke::new(1.0, edge.gamma_multiply(a)), StrokeKind::Inside);
        let mut ip = painter.clone();
        ip.set_opacity(a);
        icons::paint(&ip, Rect::from_center_size(pos2(pill.left() + 13.0, pill.center().y), vec2(13.0, 13.0)), icon, true);
        painter.galley(pos2(pill.left() + 24.0, pill.center().y - galley.size().y * 0.5), galley, col);
        let presp = ui.interact(pill, ui.scope_id().with(("status-msg", idx)), Sense::click());
        if presp.on_hover_text(format!("{}\n(click to dismiss)", m.text)).clicked() {
            dismiss = Some(idx);
        }
        y -= 24.0;
        if y < rect.top() + 20.0 {
            break;
        }
    }
    if let Some(i) = dismiss {
        if let Some(t) = clock.times.get_mut(i) {
            *t = f64::NEG_INFINITY;
        }
    }
    if let Some(s) = soonest {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(s.max(0.016)));
    }
    ctx.data_mut(|d| d.insert_temp(clock_id, clock));

    if log_open {
        log_window(&ctx, state, log_id);
    }
}

fn log_window(ctx: &egui::Context, state: &mut AppState, log_id: Id) {
    let mut open = true;
    let mut clear = false;
    egui::Window::new("Messages")
        .open(&mut open)
        .collapsible(false)
        .default_size(vec2(480.0, 260.0))
        .anchor(Align2::RIGHT_BOTTOM, vec2(-14.0, -110.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} messages", state.messages.len())).color(theme::TEXT_WEAK));
                if ui.button("Clear").clicked() {
                    clear = true;
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                if state.messages.is_empty() {
                    ui.label(RichText::new("No messages").color(theme::TEXT_WEAK));
                }
                for m in &state.messages {
                    let (icon, col) = level_style(m.level);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                        icons::paint(ui.painter(), r, icon, true);
                        ui.add(egui::Label::new(RichText::new(&m.text).color(col)).wrap());
                    });
                }
            });
        });
    if clear {
        state.messages.clear();
    }
    if !open {
        ctx.data_mut(|d| d.insert_temp(log_id, false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_curve() {
        assert_eq!(alpha(0.0), 1.0);
        assert_eq!(alpha(VISIBLE_S), 1.0);
        assert!((alpha(VISIBLE_S + FADE_S * 0.5) - 0.5).abs() < 1e-6);
        assert_eq!(alpha(VISIBLE_S + FADE_S + 0.1), 0.0);
    }

    #[test]
    fn clock_tracks_new_capped_and_cleared_messages() {
        let mut c = Clock::default();
        c.sync(2, 11, 1.0);
        assert_eq!(c.times, vec![1.0, 1.0]);
        c.sync(3, 12, 2.0);
        assert_eq!(c.times, vec![1.0, 1.0, 2.0]);
        // nothing new
        c.sync(3, 12, 3.0);
        assert_eq!(c.times, vec![1.0, 1.0, 2.0]);
        // capped list: same length, new last message -> shift
        c.sync(3, 13, 4.0);
        assert_eq!(c.times, vec![1.0, 2.0, 4.0]);
        // cleared
        c.sync(0, 0, 5.0);
        assert!(c.times.is_empty());
        c.sync(1, 14, 6.0);
        assert_eq!(c.times, vec![6.0]);
    }

    #[test]
    fn last_message_hash_changes_with_text() {
        let mut s = AppState::new();
        let a = hash_last(&s);
        s.info("hello");
        let b = hash_last(&s);
        assert_ne!(a, b);
    }
}
