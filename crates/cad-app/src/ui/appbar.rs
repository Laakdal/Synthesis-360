//! Fusion application bar (F1): data panel / home buttons, File menu, Save, Undo / Redo, the
//! centred document tab ("Name*" while unsaved changes exist) and Help on the right, which
//! opens an About window with every keyboard shortcut from the command registry.

use crate::state::command::{spec, CommandId, COMMANDS};
use crate::state::AppState;
use crate::ui::icons::{self, Icon};
use crate::ui::ribbon;
use crate::ui::theme;
use egui::{pos2, vec2, Align2, Color32, CornerRadius, Id, Popup, Rect, RichText, Sense, Stroke, Ui};

const BTN: f32 = 26.0;

fn about_id() -> Id {
    Id::unique("cad360.about_open")
}

/// Mouse / special-key bindings handled by the canvas (not registry commands).
pub const MOUSE_BINDINGS: [(&str, &str); 7] = [
    ("Pan", "Middle mouse drag"),
    ("Orbit", "Shift + middle mouse drag"),
    ("Zoom", "Mouse wheel"),
    ("Fit", "F6"),
    ("Cancel command / clear selection", "Esc"),
    ("OK / finish chain", "Enter"),
    ("Marking menu", "Right click"),
];

/// Registry commands that have a keyboard shortcut, in registry order.
pub fn shortcut_list() -> Vec<(&'static str, &'static str)> {
    COMMANDS.iter().filter_map(|c| c.shortcut.map(|k| (c.label, k))).collect()
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let full = ui.max_rect().expand2(vec2(8.0, 2.0));
    let painter = ui.painter().clone();
    painter.rect_filled(full, CornerRadius::ZERO, theme::APPBAR_BG);

    let y = full.center().y;
    let mut x = full.left() + 8.0;
    let mut slot = |w: f32| {
        let r = Rect::from_center_size(pos2(x + w * 0.5, y), vec2(w, BTN - 2.0));
        x += w + 2.0;
        r
    };

    // data panel + home (cloud features are not part of CAD360)
    let r = slot(BTN);
    let resp = button(ui, r, "appbar-data", Icon::DataPanel, true).on_hover_text("Show Data Panel");
    if resp.clicked() {
        state.info("The cloud Data Panel is not part of CAD360: designs are local .c360 files (File > Open / Save).");
    }
    let r = slot(BTN);
    let resp = button(ui, r, "appbar-home", Icon::Home, true).on_hover_text("Home");
    if resp.clicked() {
        state.info("The Fusion home / project page is not part of CAD360. Use File > New Design or Open.");
    }

    // File menu (icon + caret)
    let r = slot(BTN + 10.0);
    let resp = ui.interact(r, ui.scope_id().with("appbar-file"), Sense::click());
    icons::paint_button_bg(&painter, r, &resp, true, false);
    icons::paint(&painter, Rect::from_center_size(r.center() - vec2(5.0, 0.0), vec2(16.0, 16.0)), Icon::FileMenu, true);
    icons::caret(&painter, pos2(r.right() - 7.0, r.center().y + 1.0), 5.0, theme::TEXT);
    let resp = resp.on_hover_text("File");
    Popup::menu(&resp).show(|ui| {
        icons::menu_caption(ui, "FILE");
        for id in [CommandId::NewDesign, CommandId::Open, CommandId::Save, CommandId::SaveAs] {
            ribbon::command_menu_row(ui, state, id);
        }
        icons::menu_separator(ui);
        for id in [CommandId::ImportStep, CommandId::ExportStep, CommandId::ExportStl] {
            ribbon::command_menu_row(ui, state, id);
        }
    });

    // Save
    let r = slot(BTN);
    let enabled = state.is_enabled(CommandId::Save);
    let resp = button(ui, r, "appbar-save", Icon::Cmd(CommandId::Save), enabled);
    let clicked = resp.clicked();
    resp.on_hover_ui(|ui| ribbon::command_tooltip(ui, spec(CommandId::Save), enabled));
    if clicked {
        ribbon::run_command(&ctx, state, CommandId::Save);
    }

    // Undo / Redo
    let r = slot(BTN);
    let undo_on = state.is_enabled(CommandId::Undo);
    let undo_tip = match state.history.undo_label() {
        Some(l) if state.dialog.is_none() && state.active_sketch().is_none() => format!("Undo {l}  (Ctrl+Z)"),
        _ if state.in_txn() => "Cancel the active command  (Ctrl+Z)".to_string(),
        _ => "Nothing to undo".to_string(),
    };
    let resp = button(ui, r, "appbar-undo", Icon::Cmd(CommandId::Undo), undo_on).on_hover_text(undo_tip);
    if resp.clicked() && undo_on {
        ribbon::run_command(&ctx, state, CommandId::Undo);
    }
    let r = slot(BTN);
    let redo_on = state.is_enabled(CommandId::Redo);
    let redo_tip = match state.history.redo_label() {
        Some(l) => format!("Redo {l}  (Ctrl+Y)"),
        None => "Nothing to redo".to_string(),
    };
    let resp = button(ui, r, "appbar-redo", Icon::Cmd(CommandId::Redo), redo_on).on_hover_text(redo_tip);
    if resp.clicked() && redo_on {
        ribbon::run_command(&ctx, state, CommandId::Redo);
    }
    let left_end = x;

    // right side: help
    let help = Rect::from_center_size(pos2(full.right() - 8.0 - BTN * 0.5, y), vec2(BTN, BTN - 2.0));
    let resp = button(ui, help, "appbar-help", Icon::Help, true).on_hover_text("Help: About CAD360 and keyboard shortcuts");
    if resp.clicked() {
        ctx.data_mut(|d| {
            let v = d.get_temp_mut_or_default::<bool>(about_id());
            *v = !*v;
        });
    }

    // document tab, centred between the left buttons and the right icons (Fusion style)
    let tab = Rect::from_min_max(pos2(left_end + 18.0, full.top() + 4.0), pos2(help.left() - 18.0, full.bottom()));
    if tab.width() > 80.0 {
        painter.rect_filled(tab, CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, theme::RIBBON_BG);
        let title = state.title();
        let galley = painter.layout_no_wrap(title, egui::FontId::proportional(12.5), theme::TEXT_STRONG);
        let gw = galley.size().x;
        let c = tab.center();
        let icon_r = Rect::from_center_size(pos2(c.x - gw * 0.5 - 12.0, c.y + 1.0), vec2(14.0, 14.0));
        doc_icon(&painter, icon_r);
        painter.galley(pos2(c.x - gw * 0.5 + 2.0, c.y + 1.0 - galley.size().y * 0.5), galley, theme::TEXT_STRONG);
        let resp = ui.interact(tab, ui.scope_id().with("appbar-doc-tab"), Sense::hover());
        let path = state.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "Not saved yet".into());
        resp.on_hover_text(format!("{}{}\n{path}", state.doc.name, if state.dirty { " (unsaved changes)" } else { "" }));
    }

    about_window(&ctx);
}

fn button(ui: &mut Ui, r: Rect, key: &str, icon: Icon, enabled: bool) -> egui::Response {
    let resp = ui.interact(r, ui.scope_id().with(key), Sense::click());
    icons::paint_button_bg(ui.painter(), r, &resp, enabled, false);
    icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(17.0, 17.0)), icon, enabled);
    resp
}

/// Small orange design glyph shown in the document tab.
fn doc_icon(p: &egui::Painter, r: Rect) {
    let c = r.center();
    let s = r.width() * 0.5;
    let h = 0.866 * s;
    let top = [pos2(c.x, c.y - s), pos2(c.x + h, c.y - s * 0.5), c, pos2(c.x - h, c.y - s * 0.5)];
    let left = [pos2(c.x - h, c.y - s * 0.5), c, pos2(c.x, c.y + s), pos2(c.x - h, c.y + s * 0.5)];
    let right = [c, pos2(c.x + h, c.y - s * 0.5), pos2(c.x + h, c.y + s * 0.5), pos2(c.x, c.y + s)];
    p.add(egui::Shape::convex_polygon(top.to_vec(), Color32::from_rgb(0xff, 0xb2, 0x4d), Stroke::NONE));
    p.add(egui::Shape::convex_polygon(left.to_vec(), Color32::from_rgb(0xe0, 0x7a, 0x1f), Stroke::NONE));
    p.add(egui::Shape::convex_polygon(right.to_vec(), Color32::from_rgb(0xf0, 0x92, 0x2e), Stroke::NONE));
}

fn about_window(ctx: &egui::Context) {
    let mut open = ctx.data_mut(|d| d.get_temp::<bool>(about_id()).unwrap_or(false));
    if !open {
        return;
    }
    egui::Window::new("About CAD360")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(430.0)
        .anchor(Align2::RIGHT_TOP, vec2(-12.0, 34.0))
        .show(ctx, |ui| {
            ui.label(RichText::new("CAD360").heading().color(theme::TEXT_STRONG));
            ui.label("Native parametric CAD in Rust (egui + wgpu, OpenCascade kernel), modelled on the Fusion Design workspace.");
            ui.add_space(6.0);
            ui.label(RichText::new("KEYBOARD SHORTCUTS").font(theme::caption_font()).color(theme::TEXT_WEAK));
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                egui::Grid::new("about-shortcuts").num_columns(2).spacing(vec2(24.0, 4.0)).striped(true).show(ui, |ui| {
                    for (label, key) in shortcut_list() {
                        ui.label(label);
                        ui.label(RichText::new(key).color(theme::ACCENT));
                        ui.end_row();
                    }
                    for (label, key) in MOUSE_BINDINGS {
                        ui.label(label);
                        ui.label(RichText::new(key).color(theme::TEXT_WEAK));
                        ui.end_row();
                    }
                });
            });
        });
    ctx.data_mut(|d| d.insert_temp(about_id(), open));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_come_from_the_registry() {
        let list = shortcut_list();
        assert!(list.contains(&("Extrude", "E")));
        assert!(list.contains(&("Line", "L")));
        assert!(list.contains(&("Undo", "Ctrl+Z")));
        assert_eq!(list.len(), COMMANDS.iter().filter(|c| c.shortcut.is_some()).count());
    }
}
