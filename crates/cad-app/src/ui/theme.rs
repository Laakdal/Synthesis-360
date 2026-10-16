//! Fusion-like dark theme: charcoal panels, slightly lighter ribbon, light-grey text, blue
//! accent, thin separators, compact spacing and small corner radii.
//!
//! The colour constants are shared by every chrome module (and the icon painter) so the whole
//! UI stays consistent.

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Theme, Vec2};

/// Application bar (top strip with File / Save / Undo and the document tab).
pub const APPBAR_BG: Color32 = Color32::from_rgb(0x26, 0x29, 0x2e);
/// Floating panels (Browser, dialogs, nav bar, menus).
pub const PANEL_BG: Color32 = Color32::from_rgb(0x2f, 0x32, 0x38);
/// Ribbon / toolbar background (also the active document tab).
pub const RIBBON_BG: Color32 = Color32::from_rgb(0x33, 0x36, 0x3c);
/// Timeline strip.
pub const TIMELINE_BG: Color32 = Color32::from_rgb(0x2a, 0x2d, 0x32);
pub const TEXT: Color32 = Color32::from_rgb(0xd8, 0xda, 0xdf);
pub const TEXT_STRONG: Color32 = Color32::from_rgb(0xf2, 0xf3, 0xf5);
pub const TEXT_WEAK: Color32 = Color32::from_rgb(0x8d, 0x92, 0x9a);
pub const TEXT_DISABLED: Color32 = Color32::from_rgb(0x6a, 0x6e, 0x75);
pub const ACCENT: Color32 = Color32::from_rgb(0x3b, 0x8c, 0xff);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x2a, 0x4f, 0x86);
pub const SEPARATOR: Color32 = Color32::from_rgb(0x45, 0x49, 0x50);
pub const HOVER_BG: Color32 = Color32::from_rgb(0x41, 0x45, 0x4d);
pub const PRESSED_BG: Color32 = Color32::from_rgb(0x4b, 0x50, 0x59);
pub const ERROR: Color32 = Color32::from_rgb(0xe5, 0x48, 0x4d);
pub const WARNING: Color32 = Color32::from_rgb(0xf0, 0xb4, 0x29);

/// Small corner radius used by buttons and floating panels.
pub const RADIUS: u8 = 3;

/// Installs the theme. Called once at startup by `app.rs`.
pub fn apply(ctx: &egui::Context) {
    // eframe follows the OS theme by default; force dark so the styled theme is the active one.
    ctx.set_theme(Theme::Dark);
    ctx.style_mut_of(Theme::Dark, configure);
}

/// Mutates a style into the CAD360 look (exposed for tests and for kittest harnesses).
pub fn configure(style: &mut egui::Style) {
    let mut v = egui::Visuals::dark();
    v.dark_mode = true;
    v.override_text_color = None;
    v.panel_fill = RIBBON_BG;
    v.window_fill = PANEL_BG;
    v.faint_bg_color = Color32::from_rgb(0x35, 0x38, 0x3f);
    v.extreme_bg_color = Color32::from_rgb(0x1f, 0x22, 0x26);
    v.code_bg_color = Color32::from_rgb(0x24, 0x27, 0x2c);
    v.text_edit_bg_color = Some(Color32::from_rgb(0x22, 0x25, 0x2a));
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = WARNING;
    v.error_fg_color = ERROR;
    v.window_stroke = Stroke::new(1.0, SEPARATOR);
    v.window_corner_radius = CornerRadius::same(4);
    v.menu_corner_radius = CornerRadius::same(4);
    v.window_shadow = Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = Shadow { offset: [0, 3], blur: 10, spread: 0, color: Color32::from_black_alpha(110) };
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.indent_has_left_vline = false;
    v.striped = false;
    v.collapsing_header_frame = false;

    let r = CornerRadius::same(RADIUS);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL_BG;
    w.noninteractive.weak_bg_fill = PANEL_BG;
    w.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.noninteractive.corner_radius = r;
    w.inactive.bg_fill = Color32::from_rgb(0x3a, 0x3e, 0x45);
    w.inactive.weak_bg_fill = Color32::from_rgb(0x3a, 0x3e, 0x45);
    w.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x4a, 0x4e, 0x56));
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.corner_radius = r;
    w.hovered.bg_fill = HOVER_BG;
    w.hovered.weak_bg_fill = HOVER_BG;
    w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x5a, 0x5f, 0x68));
    w.hovered.fg_stroke = Stroke::new(1.5, TEXT_STRONG);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = PRESSED_BG;
    w.active.weak_bg_fill = PRESSED_BG;
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.5, TEXT_STRONG);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;
    w.open.bg_fill = PRESSED_BG;
    w.open.weak_bg_fill = PRESSED_BG;
    w.open.bg_stroke = Stroke::new(1.0, SEPARATOR);
    w.open.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    w.open.corner_radius = r;
    style.visuals = v;

    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(6.0, 3.0);
    s.button_padding = Vec2::new(6.0, 2.0);
    s.interact_size = Vec2::new(36.0, 20.0);
    s.window_margin = Margin::same(8);
    s.menu_margin = Margin::same(4);
    s.indent = 14.0;
    s.icon_width = 13.0;
    s.icon_width_inner = 7.0;
    s.icon_spacing = 5.0;
    s.menu_width = 220.0;
    s.tooltip_width = 300.0;
    s.combo_height = 240.0;

    style.text_styles = [
        (TextStyle::Small, FontId::new(10.5, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(12.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(12.5, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(15.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
    ]
    .into();
}

/// Font used for ribbon tab and panel captions ("SOLID", "CREATE ▾").
pub fn caption_font() -> FontId {
    FontId::proportional(11.0)
}

/// Font for normal chrome text (menus, browser rows).
pub fn body_font() -> FontId {
    FontId::proportional(12.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_applies_dark_fusion_palette() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let st = ctx.style_of(Theme::Dark);
        assert_eq!(st.visuals.panel_fill, RIBBON_BG);
        assert_eq!(st.visuals.window_fill, PANEL_BG);
        assert!(st.visuals.dark_mode);
        assert!(st.spacing.item_spacing.y <= 4.0, "compact spacing");
    }
}
