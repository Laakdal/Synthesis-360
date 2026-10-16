//! Fusion-style ribbon, generated from the command registry.
//!
//! Layout (probe screenshots `screen-v2_06_extrude_dialog.png`, `window-03_sketch_mode.png`):
//! the workspace switcher "DESIGN ▾" on the left spans the full height; to its right a row of
//! tabs (SOLID · SURFACE · MESH · SHEET METAL · PLASTIC · UTILITIES · MANAGE, plus the contextual
//! SKETCH tab while a sketch is open) above a row of panels. Each panel shows its promoted
//! commands as large icons and a caption "CREATE ▾" that drops down every command of the panel.

use crate::state::command::{spec, CommandId, CommandSpec, Panel, COMMANDS, NOT_SUPPORTED};
use crate::state::sketch::{ConstraintTool, SketchTool};
use crate::state::{AppState, Mode, NavMode};
use crate::ui::icons::{self, Icon};
use crate::ui::theme;
use egui::{pos2, vec2, Align2, Color32, CornerRadius, Id, Popup, Rect, RichText, Sense, Stroke, StrokeKind, Ui};

/// Ribbon tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    Solid,
    Surface,
    Mesh,
    SheetMetal,
    Plastic,
    Utilities,
    Manage,
    /// Contextual tab, present only while a sketch is open.
    Sketch,
}

pub const TABS: [(Tab, &str); 7] =
    [(Tab::Solid, "SOLID"), (Tab::Surface, "SURFACE"), (Tab::Mesh, "MESH"), (Tab::SheetMetal, "SHEET METAL"), (Tab::Plastic, "PLASTIC"), (Tab::Utilities, "UTILITIES"), (Tab::Manage, "MANAGE")];

/// Fusion workspaces listed by the "DESIGN ▾" switcher (only Design exists in CAD360).
pub const WORKSPACES: [&str; 7] = ["Design", "Generative Design", "Render", "Animation", "Simulation", "Manufacture", "Drawing"];

/// Ribbon panels of a tab: (caption, registry panel).
pub fn tab_panels(tab: Tab) -> &'static [(&'static str, Panel)] {
    match tab {
        Tab::Solid => &[
            ("CREATE", Panel::Create),
            ("MODIFY", Panel::Modify),
            ("ASSEMBLE", Panel::Assemble),
            ("CONSTRUCT", Panel::Construct),
            ("INSPECT", Panel::Inspect),
            ("INSERT", Panel::Insert),
            ("SELECT", Panel::Select),
        ],
        Tab::Sketch => &[
            ("CREATE", Panel::SketchCreate),
            ("MODIFY", Panel::SketchModify),
            ("CONSTRAINTS", Panel::SketchConstraints),
            ("INSPECT", Panel::Inspect),
            ("INSERT", Panel::Insert),
            ("SELECT", Panel::Select),
        ],
        // Fusion probe (ui-v3-design-workspace.json): SurfaceTab, ParaMeshOuterTab ("Mesh"), ToolsTab
        // ("UTILITIES"); panels CAD360 has no commands for (Configure, Position, Modeling Tools,
        // Create PMI) are left out as on the SOLID tab.
        Tab::Surface => &[
            ("CREATE", Panel::SurfaceCreate),
            ("MODIFY", Panel::SurfaceModify),
            ("ASSEMBLE", Panel::Assemble),
            ("CONSTRUCT", Panel::Construct),
            ("INSPECT", Panel::Inspect),
            ("INSERT", Panel::Insert),
            ("SELECT", Panel::Select),
        ],
        Tab::Mesh => &[
            ("CREATE", Panel::MeshCreate),
            ("PREPARE", Panel::MeshPrepare),
            ("MODIFY", Panel::MeshModify),
            ("ASSEMBLE", Panel::Assemble),
            ("CONSTRUCT", Panel::Construct),
            ("INSPECT", Panel::Inspect),
            ("INSERT", Panel::Insert),
            ("SELECT", Panel::Select),
            ("EXPORT", Panel::MeshExport),
        ],
        Tab::Utilities => &[
            ("MAKE", Panel::Make),
            ("NEST", Panel::Nest),
            ("ADD-INS", Panel::AddIns),
            ("UTILITY", Panel::Utility),
            ("INSPECT", Panel::Inspect),
            ("SELECT", Panel::Select),
        ],
        _ => &[],
    }
}

/// Tabs whose panels CAD360 draws (the others show the not-supported page).
pub fn tab_supported(tab: Tab) -> bool {
    !tab_panels(tab).is_empty()
}

/// Explicit command order of the SURFACE / MESH / UTILITIES panels, as Fusion lists them:
/// (command, promoted). Fusion repeats commands across tabs (Create Sketch, Fillet, Move/Copy,
/// Compute All, 3D Print ...); a command keeps its one registry panel and is listed here again.
pub fn panel_layout(panel: Panel) -> Option<&'static [(CommandId, bool)]> {
    use CommandId::*;
    Some(match panel {
        Panel::SurfaceCreate => &[
            (NewComponent, false),
            (CreateSketch, true),
            (CreateForm, true),
            (SurfaceExtrude, true),
            (SurfaceRevolve, true),
            (SurfaceSweep, true),
            (SurfaceLoft, false),
            (Patch, true),
            (Ruled, false),
            (OffsetSurface, false),
            (RectangularPattern, false),
            (CircularPattern, false),
            (MirrorFeature, false),
            (Thicken, false),
            (BoundaryFill, false),
        ],
        Panel::SurfaceModify => &[
            (PressPull, false),
            (Fillet, false),
            (Chamfer, false),
            (SurfaceTrim, true),
            (Untrim, false),
            (SurfaceExtend, true),
            (Stitch, true),
            (Unstitch, true),
            (ReverseNormal, false),
            (Move, false),
            (Delete, false),
            (Appearance, false),
            (ChangeParameters, false),
            (ComputeAll, false),
        ],
        Panel::MeshCreate => &[(NewComponent, true), (InsertMesh, true), (Tessellate, true), (MeshSectionSketch, true), (BaseMeshFeature, false)],
        Panel::MeshPrepare => &[(MeshRepair, true), (GenerateFaceGroups, true), (CombineFaceGroups, true)],
        Panel::MeshModify => &[
            (MeshDirectEdit, true),
            (Remesh, true),
            (MeshReduce, true),
            (MeshPlaneCut, true),
            (MeshShell, true),
            (MeshCombine, true),
            (MeshSmooth, false),
            (MeshReverseNormal, false),
            (MeshSeparate, false),
            (Move, true),
            (MeshScale, true),
            (Delete, false),
            (ConvertMesh, false),
            (Appearance, false),
            (ChangeParameters, false),
            (ComputeAll, false),
        ],
        // (Fusion lists 3D Print unpromoted here; promoted so the panel is not empty)
        Panel::MeshExport => &[(ThreeDPrint, true)],
        Panel::Make => &[(ThreeDPrint, true)],
        Panel::Nest => &[(NestPreparation, true)],
        Panel::AddIns => &[(ScriptsAddIns, true)],
        Panel::Utility => &[(ManageMaterials, true), (ComputeAll, true)],
        _ => return None,
    })
}

/// Every command of a panel (the panel drop-down): the explicit layout, else the registry
/// commands of the panel in registry order.
pub fn panel_commands(panel: Panel) -> Vec<&'static CommandSpec> {
    match panel_layout(panel) {
        Some(l) => l.iter().map(|(id, _)| spec(*id)).collect(),
        None => COMMANDS.iter().filter(|c| c.panel == panel && c.id != CommandId::FinishSketch).collect(),
    }
}

/// The promoted commands of a panel (the large ribbon icons).
pub fn promoted_commands(panel: Panel) -> Vec<&'static CommandSpec> {
    match panel_layout(panel) {
        Some(l) => l.iter().filter(|(_, p)| *p).map(|(id, _)| spec(*id)).collect(),
        None => panel_commands(panel).into_iter().filter(|c| c.promoted).collect(),
    }
}

/// The sketch tool a ribbon command activates (for the active-tool highlight).
pub fn tool_for(id: CommandId) -> Option<SketchTool> {
    use CommandId::*;
    Some(match id {
        Line => SketchTool::Line,
        Rectangle2Pt => SketchTool::Rectangle2Pt,
        RectangleCenter => SketchTool::RectangleCenter,
        CircleCenter => SketchTool::Circle,
        Arc3Pt => SketchTool::Arc3Pt,
        Point => SketchTool::Point,
        SketchDimension => SketchTool::Dimension,
        ConstraintCoincident => SketchTool::Constraint(ConstraintTool::Coincident),
        ConstraintCollinear => SketchTool::Constraint(ConstraintTool::Collinear),
        ConstraintConcentric => SketchTool::Constraint(ConstraintTool::Concentric),
        ConstraintMidpoint => SketchTool::Constraint(ConstraintTool::Midpoint),
        ConstraintFix => SketchTool::Constraint(ConstraintTool::Fix),
        ConstraintParallel => SketchTool::Constraint(ConstraintTool::Parallel),
        ConstraintPerpendicular => SketchTool::Constraint(ConstraintTool::Perpendicular),
        ConstraintHorizontalVertical => SketchTool::Constraint(ConstraintTool::HorizontalVertical),
        ConstraintTangent => SketchTool::Constraint(ConstraintTool::Tangent),
        ConstraintEqual => SketchTool::Constraint(ConstraintTool::Equal),
        ConstraintSymmetry => SketchTool::Constraint(ConstraintTool::Symmetry),
        Rectangle3Pt => SketchTool::Rectangle3Pt,
        Circle2Pt => SketchTool::Circle2Pt,
        ArcCenter => SketchTool::ArcCenter,
        Polygon => SketchTool::Polygon,
        Slot => SketchTool::Slot,
        SketchFillet => SketchTool::Fillet,
        Trim => SketchTool::Trim,
        Extend => SketchTool::Extend,
        SketchOffset => SketchTool::Offset,
        SketchMirror => SketchTool::Mirror,
        _ => return None,
    })
}

/// Whether a command is worth offering as "Repeat <command>" in the marking menu.
pub fn is_repeatable(id: CommandId) -> bool {
    let s = spec(id);
    s.supported && !matches!(s.panel, Panel::File | Panel::View | Panel::Select) && !matches!(id, CommandId::Undo | CommandId::Redo | CommandId::Delete | CommandId::FinishSketch | CommandId::ComputeAll)
}

fn last_command_id() -> Id {
    Id::unique("cad360.last_command")
}

/// Runs a command from the chrome and remembers it for "Repeat" (marking menu).
pub fn run_command(ctx: &egui::Context, state: &mut AppState, id: CommandId) {
    state.execute(id);
    if is_repeatable(id) {
        ctx.data_mut(|d| d.insert_temp(last_command_id(), id));
    }
}

/// The last repeatable command run from the chrome.
pub fn last_command(ctx: &egui::Context) -> Option<CommandId> {
    ctx.data_mut(|d| d.get_temp::<CommandId>(last_command_id()))
}

/// Tooltip body for a command: label, shortcut, description, availability.
pub fn command_tooltip(ui: &mut Ui, s: &CommandSpec, enabled: bool) {
    ui.set_max_width(290.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(s.label).strong().color(theme::TEXT_STRONG));
        if let Some(k) = s.shortcut {
            ui.label(RichText::new(format!("({k})")).color(theme::TEXT_WEAK));
        }
    });
    ui.label(RichText::new(s.tooltip).color(theme::TEXT));
    if !s.supported {
        ui.label(RichText::new(NOT_SUPPORTED).color(theme::WARNING));
    } else if !enabled {
        ui.label(RichText::new("Not available right now.").color(theme::TEXT_WEAK));
    }
}

/// One command row in a drop-down menu (icon · label · shortcut). Executes on click.
pub fn command_menu_row(ui: &mut Ui, state: &mut AppState, id: CommandId) {
    let s = spec(id);
    let enabled = state.is_enabled(id);
    let resp = icons::menu_row(ui, Some(Icon::Cmd(id)), s.label, s.shortcut, enabled, None);
    let clicked = resp.clicked();
    resp.on_hover_ui(|ui| command_tooltip(ui, s, enabled));
    if clicked && enabled {
        let ctx = ui.ctx().clone();
        run_command(&ctx, state, id);
        ui.close();
    }
}

fn tab_mem() -> Id {
    Id::unique("cad360.ribbon.tab")
}

/// The active ribbon tab, following the sketch mode like Fusion: entering a sketch activates the
/// contextual SKETCH tab, finishing it returns to SOLID.
fn current_tab(ctx: &egui::Context, sketching: bool) -> Tab {
    let was_id = Id::unique("cad360.ribbon.was_sketching");
    let (tab, was) = ctx.data_mut(|d| (d.get_temp::<Tab>(tab_mem()).unwrap_or(Tab::Solid), d.get_temp::<bool>(was_id).unwrap_or(false)));
    let tab = if sketching && !was {
        Tab::Sketch
    } else if !sketching && tab == Tab::Sketch {
        Tab::Solid
    } else {
        tab
    };
    ctx.data_mut(|d| {
        d.insert_temp(tab_mem(), tab);
        d.insert_temp(was_id, sketching);
    });
    tab
}

const CELL: f32 = 34.0;
const ICON: f32 = 26.0;

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let full = ui.max_rect().expand2(vec2(8.0, 2.0));
    let painter = ui.painter().clone();
    painter.rect_filled(full, CornerRadius::ZERO, theme::RIBBON_BG);
    painter.hline(full.x_range(), full.bottom() - 0.5, Stroke::new(1.0, Color32::from_rgb(0x22, 0x24, 0x28)));

    let sketching = matches!(state.mode, Mode::Sketch(_));
    let mut tab = current_tab(&ctx, sketching);

    // workspace switcher
    let design = Rect::from_min_size(full.min + vec2(10.0, 8.0), vec2(96.0, full.height() - 16.0));
    workspace_switcher(ui, design);

    // tab row
    let x0 = design.right() + 18.0;
    let mut x = x0;
    let tab_y = full.top() + 3.0;
    let mut tabs: Vec<(Tab, &str)> = TABS.to_vec();
    if sketching {
        tabs.push((Tab::Sketch, "SKETCH"));
    }
    for (t, label) in tabs {
        let galley = painter.layout_no_wrap(label.to_string(), theme::caption_font(), theme::TEXT);
        let w = galley.size().x + 26.0;
        let r = Rect::from_min_size(pos2(x, tab_y), vec2(w, 20.0));
        let resp = ui.interact(r, ui.scope_id().with(("ribbon-tab", t)), Sense::click());
        let active = t == tab;
        let col = if active { theme::TEXT_STRONG } else if resp.hovered() { theme::TEXT } else { Color32::from_rgb(0xb4, 0xb8, 0xbf) };
        painter.galley_with_override_text_color(r.center() - galley.size() * 0.5, galley, col);
        if active {
            painter.hline((r.left() + 6.0)..=(r.right() - 6.0), r.bottom(), Stroke::new(2.0, if t == Tab::Sketch { theme::ACCENT } else { theme::TEXT_STRONG }));
        }
        let supported = tab_supported(t);
        let resp = if supported { resp } else { resp.on_hover_text(format!("{label}: {NOT_SUPPORTED}")) };
        if resp.clicked() {
            tab = t;
            ctx.data_mut(|d| d.insert_temp(tab_mem(), t));
        }
        x += w;
    }

    // panel row
    let body = Rect::from_min_max(pos2(x0 - 6.0, full.top() + 27.0), pos2(full.right() - 8.0, full.bottom() - 3.0));
    let panels = tab_panels(tab);
    if panels.is_empty() {
        let name = TABS.iter().find(|(t, _)| *t == tab).map(|(_, l)| *l).unwrap_or("");
        painter.text(
            pos2(body.left() + 12.0, body.center().y),
            Align2::LEFT_CENTER,
            format!("{name} tools are not yet supported in CAD360 — switch to the SOLID tab to model."),
            theme::body_font(),
            theme::TEXT_WEAK,
        );
        return;
    }
    let mut x = body.left();
    for (caption, panel) in panels {
        x = draw_panel(ui, state, body, x, caption, *panel);
    }
    if tab == Tab::Sketch && sketching {
        finish_sketch_panel(ui, state, body, x);
    }
}

fn workspace_switcher(ui: &mut Ui, rect: Rect) {
    let resp = ui.interact(rect, ui.scope_id().with("workspace-switcher"), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(4), if resp.hovered() { Color32::from_rgb(0x3d, 0x41, 0x48) } else { Color32::from_rgb(0x2c, 0x2f, 0x35) });
    p.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, theme::SEPARATOR), StrokeKind::Inside);
    p.text(rect.center() - vec2(6.0, 0.0), Align2::CENTER_CENTER, "DESIGN", egui::FontId::proportional(12.0), theme::TEXT_STRONG);
    icons::caret(p, pos2(rect.center().x + 30.0, rect.center().y), 6.0, theme::TEXT);
    let resp = resp.on_hover_text("Switch workspace");
    Popup::menu(&resp).show(|ui| {
        icons::menu_caption(ui, "WORKSPACES");
        for (i, w) in WORKSPACES.iter().enumerate() {
            let enabled = i == 0;
            let r = icons::menu_row(ui, if i == 0 { Some(Icon::Workspace) } else { None }, w, None, enabled, Some(i == 0));
            let r = if enabled { r } else { r.on_hover_text(format!("{w} workspace: {NOT_SUPPORTED}")) };
            if r.clicked() && enabled {
                ui.close();
            }
        }
    });
}

/// Draws one ribbon panel starting at `x`; returns the x after it.
fn draw_panel(ui: &mut Ui, state: &mut AppState, body: Rect, x: f32, caption: &str, panel: Panel) -> f32 {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let active_tool = state.active_sketch().map(|s| s.tool);
    // SELECT has no registry commands: it is the selection tool (returns the nav bar to Select).
    let items: Vec<Option<&'static CommandSpec>> = if panel == Panel::Select { vec![None] } else { promoted_commands(panel).into_iter().map(Some).collect() };
    let n = items.len().max(1) as f32;
    let cap_galley = painter.layout_no_wrap(caption.to_string(), theme::caption_font(), theme::TEXT);
    let cap_w = cap_galley.size().x + 16.0;
    let w = (n * CELL).max(cap_w) + 10.0;
    let ix0 = x + (w - n * CELL) * 0.5;
    let top = body.top() + 1.0;
    for (i, item) in items.iter().enumerate() {
        let r = Rect::from_min_size(pos2(ix0 + i as f32 * CELL, top), vec2(CELL - 2.0, 36.0));
        match item {
            Some(s) => {
                let enabled = state.is_enabled(s.id);
                let active = active_tool.is_some() && tool_for(s.id) == active_tool;
                let resp = ui.interact(r, ui.scope_id().with(("ribbon-cmd", s.id, caption)), Sense::click());
                icons::paint_button_bg(&painter, r, &resp, enabled, active);
                icons::paint(&painter, Rect::from_center_size(r.center(), vec2(ICON, ICON)), Icon::Cmd(s.id), enabled);
                let clicked = resp.clicked();
                resp.on_hover_ui(|ui| command_tooltip(ui, s, enabled));
                if clicked && enabled {
                    run_command(&ctx, state, s.id);
                }
            }
            None => {
                let active = state.settings.nav == NavMode::Select;
                let resp = ui.interact(r, ui.scope_id().with(("ribbon-select", caption)), Sense::click());
                icons::paint_button_bg(&painter, r, &resp, true, active);
                icons::paint(&painter, Rect::from_center_size(r.center(), vec2(ICON, ICON)), Icon::Select, true);
                let clicked = resp.clicked();
                resp.on_hover_ui(|ui| {
                    ui.label(RichText::new("Select").strong().color(theme::TEXT_STRONG));
                    ui.label("Selection tool: click faces, edges, profiles and bodies; Ctrl/Shift adds to the selection.");
                });
                if clicked {
                    return_to_select(state);
                }
            }
        }
    }
    // caption with drop-down
    let cap_rect = Rect::from_center_size(pos2(x + w * 0.5, body.bottom() - 8.0), vec2(cap_w, 15.0));
    let resp = ui.interact(cap_rect, ui.scope_id().with(("ribbon-caption", caption, panel_key(panel))), Sense::click());
    if resp.hovered() {
        painter.rect_filled(cap_rect.expand2(vec2(2.0, 1.0)), CornerRadius::same(2), theme::HOVER_BG);
    }
    let col = if resp.hovered() { theme::TEXT_STRONG } else { theme::TEXT };
    painter.galley_with_override_text_color(pos2(cap_rect.left() + 3.0, cap_rect.center().y - cap_galley.size().y * 0.5), cap_galley, col);
    icons::caret(&painter, pos2(cap_rect.right() - 6.0, cap_rect.center().y), 5.0, col);
    Popup::menu(&resp).show(|ui| {
        icons::menu_caption(ui, caption);
        if panel == Panel::Select {
            let r = icons::menu_row(ui, Some(Icon::Select), "Select", None, true, Some(state.settings.nav == NavMode::Select));
            if r.clicked() {
                return_to_select(state);
                ui.close();
            }
            let has = !state.selection.is_empty();
            let r = icons::menu_row(ui, Some(Icon::Close), "Clear Selection", Some("Esc"), has, None);
            if r.clicked() && has {
                state.clear_selection();
                ui.close();
            }
            for l in ["Window Selection", "Freeform Selection", "Paint Selection", "Selection Filters"] {
                icons::menu_row(ui, None, l, None, false, None).on_hover_text(format!("{l}: {NOT_SUPPORTED}"));
            }
        } else {
            for s in panel_commands(panel) {
                command_menu_row(ui, state, s.id);
            }
        }
    });
    // separator
    let sx = x + w + 3.0;
    painter.vline(sx, (body.top() + 4.0)..=(body.bottom() - 2.0), Stroke::new(1.0, theme::SEPARATOR));
    sx + 4.0
}

fn panel_key(p: Panel) -> u8 {
    p as u8
}

/// Leaves Orbit / Pan / Zoom navigation (the nav-bar modes toggle back to Select).
fn return_to_select(state: &mut AppState) {
    match state.settings.nav {
        NavMode::Orbit => state.execute(CommandId::OrbitMode),
        NavMode::Pan => state.execute(CommandId::PanMode),
        NavMode::Zoom => state.execute(CommandId::ZoomMode),
        NavMode::Select => {}
    }
}

fn finish_sketch_panel(ui: &mut Ui, state: &mut AppState, body: Rect, x: f32) {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let caption = "FINISH SKETCH";
    let cap_galley = painter.layout_no_wrap(caption.to_string(), theme::caption_font(), theme::TEXT);
    let w = cap_galley.size().x + 26.0;
    let r = Rect::from_center_size(pos2(x + w * 0.5, body.top() + 19.0), vec2(CELL, 36.0));
    let id = CommandId::FinishSketch;
    let s = spec(id);
    let enabled = state.is_enabled(id);
    let resp = ui.interact(r, ui.scope_id().with("ribbon-finish-sketch"), Sense::click());
    icons::paint_button_bg(&painter, r, &resp, enabled, false);
    icons::paint(&painter, Rect::from_center_size(r.center(), vec2(ICON + 2.0, ICON + 2.0)), Icon::Cmd(id), enabled);
    let clicked = resp.clicked();
    resp.on_hover_ui(|ui| command_tooltip(ui, s, enabled));
    if clicked && enabled {
        run_command(&ctx, state, id);
        return;
    }
    let cap_rect = Rect::from_center_size(pos2(x + w * 0.5, body.bottom() - 8.0), vec2(w - 6.0, 15.0));
    let resp = ui.interact(cap_rect, ui.scope_id().with("ribbon-finish-caption"), Sense::click());
    if resp.hovered() {
        painter.rect_filled(cap_rect.expand2(vec2(2.0, 1.0)), CornerRadius::same(2), theme::HOVER_BG);
    }
    painter.galley_with_override_text_color(pos2(cap_rect.left() + 3.0, cap_rect.center().y - cap_galley.size().y * 0.5), cap_galley, theme::TEXT);
    icons::caret(&painter, pos2(cap_rect.right() - 6.0, cap_rect.center().y), 5.0, theme::TEXT);
    Popup::menu(&resp).show(|ui| {
        command_menu_row(ui, state, CommandId::FinishSketch);
        command_menu_row(ui, state, CommandId::LookAt);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_panels_follow_fusion_order() {
        let caps: Vec<&str> = tab_panels(Tab::Solid).iter().map(|(c, _)| *c).collect();
        assert_eq!(caps, ["CREATE", "MODIFY", "ASSEMBLE", "CONSTRUCT", "INSPECT", "INSERT", "SELECT"]);
        let caps: Vec<&str> = tab_panels(Tab::Sketch).iter().map(|(c, _)| *c).collect();
        assert_eq!(&caps[..3], ["CREATE", "MODIFY", "CONSTRAINTS"]);
        assert!(tab_panels(Tab::SheetMetal).is_empty(), "other tabs show the not-supported page");
        assert!(!tab_supported(Tab::Plastic) && !tab_supported(Tab::Manage));
    }

    #[test]
    fn panel_dropdowns_list_every_ribbon_command_once() {
        // every command whose panel is on a ribbon tab appears in exactly one drop-down
        let ribbon: Vec<Panel> = [Tab::Solid, Tab::Sketch, Tab::Surface, Tab::Mesh, Tab::Utilities].iter().flat_map(|t| tab_panels(*t)).map(|(_, p)| *p).collect();
        for c in COMMANDS {
            if c.id == CommandId::FinishSketch || matches!(c.panel, Panel::File | Panel::View) {
                continue;
            }
            assert!(ribbon.contains(&c.panel), "{:?} is in no ribbon panel", c.id);
            assert_eq!(panel_commands(c.panel).iter().filter(|s| s.id == c.id).count(), 1);
        }
    }

    #[test]
    fn promoted_commands_come_from_the_registry() {
        let create: Vec<CommandId> = promoted_commands(Panel::Create).iter().map(|c| c.id).collect();
        assert_eq!(create[0], CommandId::CreateSketch);
        assert!(create.contains(&CommandId::Extrude) && create.contains(&CommandId::Hole));
        assert!(!create.contains(&CommandId::Sweep), "non-promoted commands are drop-down only");
        let cons = promoted_commands(Panel::SketchConstraints);
        assert!(cons.len() >= 11);
        assert!(promoted_commands(Panel::SketchCreate).iter().all(|c| c.id != CommandId::FinishSketch));
    }

    #[test]
    fn sketch_tools_map_to_ribbon_commands() {
        assert_eq!(tool_for(CommandId::Line), Some(SketchTool::Line));
        assert_eq!(tool_for(CommandId::CircleCenter), Some(SketchTool::Circle));
        assert_eq!(tool_for(CommandId::ConstraintTangent), Some(SketchTool::Constraint(ConstraintTool::Tangent)));
        assert_eq!(tool_for(CommandId::Extrude), None);
    }

    #[test]
    fn repeatable_commands_exclude_edit_and_view() {
        assert!(is_repeatable(CommandId::Extrude));
        assert!(is_repeatable(CommandId::Fillet));
        assert!(!is_repeatable(CommandId::Undo));
        assert!(!is_repeatable(CommandId::Fit));
        assert!(!is_repeatable(CommandId::Save));
        assert!(!is_repeatable(CommandId::Sweep), "unsupported");
    }

    /// Runs every chrome `show` function for a few frames (model mode, then sketch mode, then
    /// with a finished sketch in the timeline / browser) to catch panics and id clashes.
    #[test]
    fn chrome_smoke_frames() {
        use crate::state::Pick;
        use crate::ui;
        use crate::viewport::camera::Camera;
        let ctx = egui::Context::default();
        ui::theme::apply(&ctx);
        let mut state = AppState::new();
        let mut cam = Camera::default();
        let frame = |state: &mut AppState, cam: &mut Camera| {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::Panel::top("appbar").exact_size(30.0).show(ui, |ui| ui::appbar::show(ui, state));
                egui::Panel::top("ribbon").exact_size(92.0).show(ui, |ui| ui::ribbon::show(ui, state));
                egui::Panel::bottom("timeline").exact_size(40.0).show(ui, |ui| ui::timeline::show(ui, state));
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
                    let rect = ui.available_rect_before_wrap();
                    let resp = ui.interact(rect, egui::Id::unique("smoke-canvas"), egui::Sense::click_and_drag());
                    ui::context_menu::show(&resp, state);
                    let b = egui::Rect::from_min_size(rect.min + egui::vec2(8.0, 8.0), egui::vec2(270.0, 400.0));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(b), |ui| ui::browser::show(ui, state));
                    let cube = egui::Rect::from_min_size(egui::pos2(rect.right() - 128.0, rect.top() + 12.0), egui::vec2(116.0, 116.0));
                    ui::viewcube::show(ui, cube, cam);
                    let nav = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.bottom() - 22.0), egui::vec2(420.0, 34.0));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(nav), |ui| ui::navbar::show(ui, state));
                    let st = egui::Rect::from_min_max(egui::pos2(rect.right() - 520.0, rect.bottom() - 60.0), egui::pos2(rect.right() - 8.0, rect.bottom() - 6.0));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(st), |ui| ui::statusbar::show(ui, state));
                });
            });
            out.textures_delta.clear();
        };
        for _ in 0..3 {
            frame(&mut state, &mut cam);
        }
        state.select(Pick::OriginPlane(cad_core::OriginPlane::XY), false);
        state.execute(CommandId::CreateSketch);
        for _ in 0..3 {
            frame(&mut state, &mut cam);
        }
        assert_eq!(current_tab(&ctx, matches!(state.mode, Mode::Sketch(_))), if matches!(state.mode, Mode::Sketch(_)) { Tab::Sketch } else { Tab::Solid });
        state.execute(CommandId::FinishSketch);
        state.warn("a warning");
        state.error("an error");
        for _ in 0..3 {
            frame(&mut state, &mut cam);
        }
        assert!(cam.rotation.is_finite());
    }

    #[test]
    fn surface_mesh_and_utilities_panels_follow_fusion() {
        let caps = |t: Tab| tab_panels(t).iter().map(|(c, _)| *c).collect::<Vec<_>>();
        assert_eq!(caps(Tab::Surface), ["CREATE", "MODIFY", "ASSEMBLE", "CONSTRUCT", "INSPECT", "INSERT", "SELECT"]);
        assert_eq!(caps(Tab::Mesh), ["CREATE", "PREPARE", "MODIFY", "ASSEMBLE", "CONSTRUCT", "INSPECT", "INSERT", "SELECT", "EXPORT"]);
        assert_eq!(caps(Tab::Utilities), ["MAKE", "NEST", "ADD-INS", "UTILITY", "INSPECT", "SELECT"]);
        let ids = |p: Panel| promoted_commands(p).iter().map(|c| c.id).collect::<Vec<_>>();
        use CommandId::*;
        assert_eq!(ids(Panel::SurfaceCreate), [CreateSketch, CreateForm, SurfaceExtrude, SurfaceRevolve, SurfaceSweep, Patch]);
        assert_eq!(ids(Panel::SurfaceModify), [SurfaceTrim, SurfaceExtend, Stitch, Unstitch]);
        assert_eq!(ids(Panel::MeshCreate), [NewComponent, InsertMesh, Tessellate, MeshSectionSketch]);
        assert_eq!(ids(Panel::Make), [ThreeDPrint]);
        assert_eq!(ids(Panel::Utility), [ManageMaterials, ComputeAll]);
        // drop-down only, as in Fusion
        let all = |p: Panel| panel_commands(p).iter().map(|c| c.id).collect::<Vec<_>>();
        assert!(all(Panel::SurfaceCreate).contains(&OffsetSurface) && all(Panel::SurfaceCreate).contains(&Thicken));
        assert!(all(Panel::MeshModify).contains(&ConvertMesh));
        // shared commands appear in the tabs Fusion repeats them in
        assert!(all(Panel::SurfaceModify).contains(&Fillet) && all(Panel::MeshModify).contains(&Move));
        assert!(all(Panel::MeshExport).contains(&ThreeDPrint));
    }

    #[test]
    fn panel_layouts_list_every_command_of_their_panel() {
        for (_, p) in [Tab::Surface, Tab::Mesh, Tab::Utilities].iter().flat_map(|t| tab_panels(*t)) {
            let Some(l) = panel_layout(*p) else { continue };
            for c in COMMANDS.iter().filter(|c| c.panel == *p) {
                assert!(l.iter().any(|(id, _)| *id == c.id), "{:?} missing from the {:?} layout", c.id, p);
            }
            for (i, (a, _)) in l.iter().enumerate() {
                assert!(l[i + 1..].iter().all(|(b, _)| b != a), "{a:?} twice in {p:?}");
            }
        }
    }

    #[test]
    fn surface_tab_enables_implemented_commands_only() {
        let s = AppState::new();
        use CommandId::*;
        for id in [SurfaceExtrude, SurfaceRevolve, Patch, Stitch, OffsetSurface, Thicken, InsertMesh, ThreeDPrint, ComputeAll, Measure] {
            assert!(s.is_enabled(id), "{id:?} should be enabled");
        }
        for id in [CreateForm, SurfaceSweep, SurfaceLoft, Ruled, BoundaryFill, SurfaceTrim, Untrim, SurfaceExtend, Unstitch, ReverseNormal, Tessellate, MeshRepair, Remesh, MeshPlaneCut, ConvertMesh, NestPreparation, ScriptsAddIns, ManageMaterials] {
            assert!(!s.is_enabled(id), "{id:?} is a placeholder");
            assert!(!spec(id).supported);
        }
        for t in [Tab::Surface, Tab::Mesh, Tab::Utilities] {
            assert!(tab_supported(t));
            let enabled = tab_panels(t).iter().flat_map(|(_, p)| panel_commands(*p)).filter(|c| s.is_enabled(c.id)).count();
            assert!(enabled > 0, "{t:?} has working commands");
        }
    }

    #[test]
    fn tab_follows_sketch_mode() {
        let ctx = egui::Context::default();
        assert_eq!(current_tab(&ctx, false), Tab::Solid);
        assert_eq!(current_tab(&ctx, true), Tab::Sketch);
        // user switches to SOLID while sketching: respected
        ctx.data_mut(|d| d.insert_temp(tab_mem(), Tab::Solid));
        assert_eq!(current_tab(&ctx, true), Tab::Solid);
        ctx.data_mut(|d| d.insert_temp(tab_mem(), Tab::Sketch));
        assert_eq!(current_tab(&ctx, false), Tab::Solid);
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use crate::ui::icons::testkit::Kit;
    use egui::pos2;

    #[test]
    fn clicking_create_sketch_icon_runs_the_command() {
        let mut s = AppState::new();
        let mut kit = Kit::new();
        {
            let mut f = |ui: &mut Ui| {
                egui::Panel::top("ribbon").exact_size(92.0).show(ui, |ui| show(ui, &mut s));
            };
            kit.idle(2, &mut f);
            // first promoted icon of CREATE: body.left = 10 + 96 + 18 - 6, centred in a 180 px panel
            kit.click(pos2(118.0 + 5.0 + 16.0, 46.0), &mut f);
        }
        assert!(s.plane_prompt, "Create Sketch waits for a plane");
        assert_eq!(last_command(&kit.ctx), Some(CommandId::CreateSketch));
    }

    #[test]
    fn unsupported_tab_shows_placeholder_and_keeps_state() {
        let mut s = AppState::new();
        let mut kit = Kit::new();
        {
            let mut f = |ui: &mut Ui| {
                egui::Panel::top("ribbon").exact_size(92.0).show(ui, |ui| show(ui, &mut s));
            };
            kit.idle(2, &mut f);
            // SURFACE is the second tab (SOLID is ~62 px wide starting at x = 124)
            kit.click(pos2(124.0 + 62.0 + 30.0, 13.0), &mut f);
        }
        assert_eq!(kit.ctx.data_mut(|d| d.get_temp::<Tab>(tab_mem())), Some(Tab::Surface));
    }

    #[test]
    fn surface_tab_extrude_icon_opens_the_surface_extrude_dialog() {
        let mut s = AppState::new();
        let mut kit = Kit::new();
        kit.ctx.data_mut(|d| d.insert_temp(tab_mem(), Tab::Surface));
        {
            let mut f = |ui: &mut Ui| {
                egui::Panel::top("ribbon").exact_size(92.0).show(ui, |ui| show(ui, &mut s));
            };
            kit.idle(2, &mut f);
            // CREATE has 6 promoted icons (214 px panel, icons start 5 px in); Extrude is the third
            kit.click(pos2(118.0 + 5.0 + 2.0 * 34.0 + 16.0, 46.0), &mut f);
        }
        assert_eq!(last_command(&kit.ctx), Some(CommandId::SurfaceExtrude));
        assert!(matches!(&s.dialog, Some(crate::state::Dialog::Surface(crate::state::features::SurfaceDialog::Extrude(_)))), "{:?}", s.dialog.as_ref().map(|d| d.title()));
        // every new tab draws without panics
        for t in [Tab::Mesh, Tab::Utilities, Tab::Surface] {
            kit.ctx.data_mut(|d| d.insert_temp(tab_mem(), t));
            let mut f = |ui: &mut Ui| {
                egui::Panel::top("ribbon").exact_size(92.0).show(ui, |ui| show(ui, &mut s));
            };
            kit.idle(2, &mut f);
        }
    }
}
