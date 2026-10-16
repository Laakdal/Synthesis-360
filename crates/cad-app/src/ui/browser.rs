//! Fusion-style Browser (F2): floating tree at the top-left of the canvas.
//!
//! Root (document) › Document Settings (Units) · Named Views · Origin (O, X/Y/Z, XY/XZ/YZ) ·
//! Joints · Bodies · Sketches · Construction · one node per component (its Bodies and Sketches).
//! Eye icons toggle visibility, rows select, double-click edits a sketch / joint, right-click
//! opens the item's context menu. The radio button at the right of the root and component rows
//! activates that component (new features go into the active component, as in Fusion). Fully
//! constrained sketches show Fusion's lock glyph (F19); features with errors / warnings get a
//! red / yellow badge.

use crate::state::command::{CommandId, NOT_SUPPORTED};
use crate::state::{AppState, Pick};
use crate::ui::icons::{self, Icon};
use crate::ui::theme;
use cad_core::{FeatureKind, OriginPlane};
use cad_kernel::Health;
use egui::{pos2, vec2, Align2, Color32, CornerRadius, Frame, Id, Margin, Rect, Response, Sense, Stroke, Ui};

const ROW_H: f32 = 21.0;
const INDENT: f32 = 14.0;

/// Something the user did in the browser; applied after drawing (keeps borrows simple).
#[derive(Clone, Debug, PartialEq)]
enum Act {
    Select(Pick, bool),
    EditSketch(String),
    EditFeature(String),
    DeleteFeature(String),
    SetBodyVisible(String, bool),
    SetAllBodies(bool),
    SetBodiesVisible(Vec<String>, bool),
    ToggleFeatureVisible(String),
    ToggleOrigin,
    Appearance(String),
    Command(CommandId),
    ActivateComponent(Option<String>),
    SetComponentVisible(String, bool),
    RenameComponent(String, String),
    DeleteComponent(String),
    GroundComponent(String, bool),
}

/// Plain data for one component node.
struct ComponentRow {
    id: String,
    name: String,
    visible: bool,
    active: bool,
    /// Grounded: joints never move it (shown with the lock glyph).
    grounded: bool,
}

/// Plain data for one feature row (copied out of the state before drawing).
struct FeatureRow {
    id: String,
    name: String,
    visible: bool,
    selected: bool,
    suppressed: bool,
    health: Health,
    locked: bool,
    component: Option<String>,
}

struct BodyRow {
    id: String,
    name: String,
    visible: bool,
    selected: bool,
    owner: String,
    component: Option<String>,
    /// The body's component is shown.
    shown: bool,
}

#[derive(Default)]
struct RowOut {
    row: Option<Response>,
    eye: Option<Response>,
    arrow: Option<Response>,
}

struct Row<'a> {
    depth: usize,
    /// Some(open) for expandable nodes.
    expand: Option<bool>,
    /// Some(visible) to show an eye.
    eye: Option<bool>,
    icon: Icon,
    label: &'a str,
    selected: bool,
    dim: bool,
    badge: Option<Icon>,
    lock: bool,
}

fn open_id(key: &str) -> Id {
    Id::unique(("cad360.browser.open", key))
}

fn is_open(ctx: &egui::Context, key: &str, default: bool) -> bool {
    ctx.data_mut(|d| d.get_temp::<bool>(open_id(key)).unwrap_or(default))
}

fn toggle_open(ctx: &egui::Context, key: &str, default: bool) {
    let v = !is_open(ctx, key, default);
    ctx.data_mut(|d| d.insert_temp(open_id(key), v));
}

/// Default body name like Fusion ("Body1", "Body2" ...).
pub fn body_display_name(meta_name: Option<&str>, index: usize) -> String {
    match meta_name {
        Some(n) if !n.trim().is_empty() => n.to_string(),
        _ => format!("Body{}", index + 1),
    }
}

fn health_badge(h: &Health) -> Option<Icon> {
    match h {
        Health::Error(_) => Some(Icon::Error),
        Health::Warning(_) => Some(Icon::Warning),
        _ => None,
    }
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let max = ui.max_rect();
    let collapsed_id = Id::unique("cad360.browser.collapsed");
    let rect_id = Id::unique("cad360.browser.rect");
    let collapsed = ctx.data_mut(|d| d.get_temp::<bool>(collapsed_id).unwrap_or(false));

    // Swallow clicks / drags on the panel background so they don't reach the canvas.
    if let Some(r) = ctx.data_mut(|d| d.get_temp::<Rect>(rect_id)) {
        let _ = ui.interact(r, ui.scope_id().with("browser-bg"), Sense::click_and_drag());
    }

    let mut acts: Vec<Act> = Vec::new();
    let frame = Frame::new()
        .fill(theme::PANEL_BG.gamma_multiply(0.97))
        .stroke(Stroke::new(1.0, theme::SEPARATOR))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(4, 3));
    let resp = frame.show(ui, |ui| {
        ui.set_width(max.width() - 10.0);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        // header
        let (hr, hresp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
        let p = ui.painter();
        icons::paint(p, Rect::from_center_size(pos2(hr.left() + 8.0, hr.center().y), vec2(10.0, 10.0)), if collapsed { Icon::CaretRight } else { Icon::CaretDown }, true);
        p.text(pos2(hr.left() + 20.0, hr.center().y), Align2::LEFT_CENTER, "BROWSER", theme::caption_font(), theme::TEXT);
        let mr = Rect::from_center_size(pos2(hr.right() - 10.0, hr.center().y), vec2(16.0, 16.0));
        let mresp = ui.interact(mr, ui.scope_id().with("browser-min"), Sense::click());
        icons::paint_button_bg(ui.painter(), mr, &mresp, true, false);
        ui.painter().hline((mr.left() + 4.0)..=(mr.right() - 4.0), mr.center().y, Stroke::new(1.5, theme::TEXT));
        let mresp = mresp.on_hover_text(if collapsed { "Expand the browser" } else { "Collapse the browser" });
        if hresp.clicked() || mresp.clicked() {
            ctx.data_mut(|d| d.insert_temp(collapsed_id, !collapsed));
        }
        if !collapsed {
            ui.painter().hline(hr.x_range(), hr.bottom(), Stroke::new(1.0, theme::SEPARATOR));
            egui::ScrollArea::vertical().max_height((max.height() - 36.0).max(60.0)).auto_shrink([false, true]).show(ui, |ui| {
                tree(ui, state, &mut acts);
            });
        }
    });
    ctx.data_mut(|d| d.insert_temp(rect_id, resp.response.rect));
    apply(state, acts);
}

fn apply(state: &mut AppState, acts: Vec<Act>) {
    for a in acts {
        match a {
            Act::Select(p, add) => state.select(p, add),
            Act::EditSketch(id) => state.edit_sketch(&id),
            Act::EditFeature(id) => state.edit_feature(&id),
            Act::DeleteFeature(id) => state.delete_feature(&id),
            Act::SetBodyVisible(b, v) => state.set_body_visible(&b, v),
            Act::SetAllBodies(v) => {
                let ids: Vec<String> = state.model.bodies.iter().map(|b| b.id.clone()).collect();
                state.apply("Toggle Visibility", |d| {
                    for b in ids {
                        d.bodies.entry(b).or_default().visible = v;
                    }
                    Ok(())
                });
            }
            Act::SetBodiesVisible(ids, v) => {
                state.apply("Toggle Visibility", |d| {
                    for b in ids {
                        d.bodies.entry(b).or_default().visible = v;
                    }
                    Ok(())
                });
            }
            Act::ToggleFeatureVisible(id) => {
                state.apply("Toggle Visibility", |d| {
                    let f = d.feature_mut(&id).ok_or("unknown feature")?;
                    match &mut f.kind {
                        FeatureKind::Sketch { visible, .. } | FeatureKind::Plane { visible, .. } => *visible = !*visible,
                        _ => {}
                    }
                    Ok(())
                });
            }
            Act::ToggleOrigin => {
                state.apply("Toggle Visibility", |d| {
                    d.origin_visible = !d.origin_visible;
                    Ok(())
                });
            }
            Act::Appearance(body) => {
                state.select(Pick::Body { body }, false);
                state.execute(CommandId::Appearance);
            }
            Act::Command(c) => state.execute(c),
            Act::ActivateComponent(c) => state.activate_component(c.as_deref()),
            Act::SetComponentVisible(c, v) => state.set_component_visible(&c, v),
            Act::RenameComponent(c, n) => state.rename_component(&c, &n),
            Act::DeleteComponent(c) => {
                state.delete_component(&c);
            }
            Act::GroundComponent(c, g) => state.set_component_grounded(&c, g),
        }
    }
}

fn tree(ui: &mut Ui, state: &AppState, acts: &mut Vec<Act>) {
    let ctx = ui.ctx().clone();
    let sel = &state.selection;
    let additive = ctx.input(|i| i.modifiers.command || i.modifiers.shift);

    // ---- gather rows
    let bodies: Vec<BodyRow> = state
        .model
        .bodies
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let meta = state.doc.bodies.get(&b.id);
            let component = state.doc.body_component(&b.id, Some(&b.created_by)).map(String::from);
            BodyRow {
                id: b.id.clone(),
                name: body_display_name(meta.and_then(|m| m.name.as_deref()), i),
                visible: meta.is_none_or(|m| m.visible),
                selected: sel.iter().any(|p| matches!(p, Pick::Body { body } if *body == b.id)),
                owner: b.created_by.clone(),
                shown: state.doc.component_visible(component.as_deref()),
                component,
            }
        })
        .collect();
    let marker = state.doc.marker;
    let joints: Vec<FeatureRow> = state
        .doc
        .features
        .iter()
        .take(marker)
        .filter(|f| matches!(f.kind, FeatureKind::Joint { .. }))
        .map(|f| FeatureRow {
            id: f.id.clone(),
            name: f.name.clone(),
            visible: true,
            selected: sel.iter().any(|p| matches!(p, Pick::Feature(x) if *x == f.id)),
            suppressed: f.suppressed,
            health: state.health(&f.id),
            locked: false,
            component: f.component.clone(),
        })
        .collect();
    let components: Vec<ComponentRow> = state
        .doc
        .components
        .iter()
        .map(|c| ComponentRow { id: c.id.clone(), name: c.name.clone(), visible: c.visible, active: state.doc.active_component.as_deref() == Some(c.id.as_str()), grounded: c.grounded })
        .collect();
    let feature_rows = |want_sketch: bool| -> Vec<FeatureRow> {
        state
            .doc
            .features
            .iter()
            .enumerate()
            .filter(|(i, _)| *i < marker)
            .filter_map(|(_, f)| {
                let visible = match (&f.kind, want_sketch) {
                    (FeatureKind::Sketch { visible, .. }, true) => *visible,
                    (FeatureKind::Plane { visible, .. }, false) => *visible,
                    _ => return None,
                };
                let locked = want_sketch && state.model.sketches.get(&f.id).is_some_and(|r| r.report.is_fully_constrained());
                Some(FeatureRow {
                    id: f.id.clone(),
                    name: f.name.clone(),
                    visible,
                    selected: sel.iter().any(|p| matches!(p, Pick::Feature(x) | Pick::ConstructionPlane(x) if *x == f.id)),
                    suppressed: f.suppressed,
                    health: state.health(&f.id),
                    locked,
                    component: f.component.clone(),
                })
            })
            .collect()
    };
    let all_sketches = feature_rows(true);
    let all_planes = feature_rows(false);
    let any_body_visible = bodies.iter().any(|b| b.visible);
    // the root component's own items (components list theirs in their nodes)
    let root_bodies: Vec<&BodyRow> = bodies.iter().filter(|b| b.component.is_none()).collect();
    let sketches: Vec<&FeatureRow> = all_sketches.iter().filter(|s| s.component.is_none()).collect();
    let planes: Vec<&FeatureRow> = all_planes.iter().filter(|s| s.component.is_none()).collect();

    // ---- root
    let root_open = is_open(&ctx, "root", true);
    let o = row(ui, "root", &Row { depth: 0, expand: Some(root_open), eye: Some(any_body_visible || bodies.is_empty()), icon: Icon::Component, label: &state.doc.name, selected: false, dim: false, badge: None, lock: false });
    if clicked(&o.arrow) || o.row.as_ref().is_some_and(|r| r.double_clicked()) {
        toggle_open(&ctx, "root", true);
    }
    if clicked(&o.eye) {
        acts.push(Act::SetAllBodies(!any_body_visible));
    }
    if !components.is_empty() {
        let root_active = state.doc.active_component.is_none();
        if radio(ui, "root", &o, root_active).is_some_and(|r| r.clicked()) && !root_active {
            acts.push(Act::ActivateComponent(None));
        }
    }
    if let Some(r) = &o.row {
        r.context_menu(|ui| {
            if icons::menu_row(ui, Some(Icon::Cmd(CommandId::NewComponent)), "New Component", None, true, None).clicked() {
                acts.push(Act::Command(CommandId::NewComponent));
                ui.close();
            }
            if state.doc.active_component.is_some() && icons::menu_row(ui, Some(Icon::Component), "Activate Component", None, true, None).clicked() {
                acts.push(Act::ActivateComponent(None));
                ui.close();
            }
        });
    }
    if !root_open {
        return;
    }

    // ---- document settings
    let ds_open = is_open(&ctx, "docsettings", false);
    let o = row(ui, "docsettings", &Row { depth: 1, expand: Some(ds_open), eye: None, icon: Icon::Gear, label: "Document Settings", selected: false, dim: false, badge: None, lock: false });
    if clicked(&o.arrow) || clicked(&o.row) {
        toggle_open(&ctx, "docsettings", false);
    }
    if ds_open {
        let units = format!("Units: {}", state.doc.units);
        let o = row(ui, "units", &Row { depth: 2, expand: None, eye: None, icon: Icon::Units, label: &units, selected: false, dim: false, badge: None, lock: false });
        if let Some(r) = o.row {
            r.on_hover_text(format!("Changing document units: {NOT_SUPPORTED}"));
        }
    }

    // ---- named views (standard views; custom named views are not supported)
    let nv_open = is_open(&ctx, "namedviews", false);
    let o = row(ui, "namedviews", &Row { depth: 1, expand: Some(nv_open), eye: None, icon: Icon::Folder, label: "Named Views", selected: false, dim: false, badge: None, lock: false });
    if clicked(&o.arrow) || clicked(&o.row) {
        toggle_open(&ctx, "namedviews", false);
    }
    if let Some(r) = o.row {
        r.on_hover_text(format!("Standard views. Saving custom named views: {NOT_SUPPORTED}"));
    }
    if nv_open {
        for (label, cmd) in [("TOP", CommandId::ViewTop), ("FRONT", CommandId::ViewFront), ("RIGHT", CommandId::ViewRight), ("HOME", CommandId::ViewHome)] {
            let o = row(ui, label, &Row { depth: 2, expand: None, eye: None, icon: Icon::Cmd(cmd), label, selected: false, dim: false, badge: None, lock: false });
            if let Some(r) = o.row {
                if r.on_hover_text("Click to restore this view").clicked() {
                    acts.push(Act::Command(cmd));
                }
            }
        }
    }

    // ---- origin
    let or_open = is_open(&ctx, "origin", false);
    let o = row(ui, "origin", &Row { depth: 1, expand: Some(or_open), eye: Some(state.doc.origin_visible), icon: Icon::Folder, label: "Origin", selected: false, dim: !state.doc.origin_visible, badge: None, lock: false });
    if clicked(&o.arrow) || clicked(&o.row) {
        toggle_open(&ctx, "origin", false);
    }
    if clicked(&o.eye) {
        acts.push(Act::ToggleOrigin);
    }
    if or_open {
        let dim = !state.doc.origin_visible;
        row(ui, "origin-o", &Row { depth: 2, expand: None, eye: None, icon: Icon::Origin, label: "O", selected: false, dim, badge: None, lock: false });
        for a in ["X", "Y", "Z"] {
            row(ui, a, &Row { depth: 2, expand: None, eye: None, icon: Icon::Axis, label: a, selected: false, dim, badge: None, lock: false });
        }
        for (label, plane) in [("XY", OriginPlane::XY), ("XZ", OriginPlane::XZ), ("YZ", OriginPlane::YZ)] {
            let pick = Pick::OriginPlane(plane);
            let selected = sel.contains(&pick);
            let o = row(ui, label, &Row { depth: 2, expand: None, eye: None, icon: Icon::Plane, label, selected, dim, badge: None, lock: false });
            if let Some(r) = o.row {
                if r.clicked() {
                    acts.push(Act::Select(pick.clone(), additive));
                }
                r.context_menu(|ui| {
                    if icons::menu_row(ui, Some(Icon::Cmd(CommandId::CreateSketch)), "Create Sketch", None, true, None).clicked() {
                        acts.push(Act::Select(pick.clone(), false));
                        acts.push(Act::Command(CommandId::CreateSketch));
                        ui.close();
                    }
                    if icons::menu_row(ui, Some(Icon::Cmd(CommandId::OffsetPlane)), "Offset Plane", None, true, None).clicked() {
                        acts.push(Act::Select(pick.clone(), false));
                        acts.push(Act::Command(CommandId::OffsetPlane));
                        ui.close();
                    }
                });
            }
        }
    }

    // ---- joints (Fusion lists them right after the Origin folder)
    if !joints.is_empty() {
        let j_open = is_open(&ctx, "joints", true);
        let o = row(ui, "joints", &Row { depth: 1, expand: Some(j_open), eye: None, icon: Icon::Folder, label: "Joints", selected: false, dim: false, badge: None, lock: false });
        if clicked(&o.arrow) || clicked(&o.row) {
            toggle_open(&ctx, "joints", true);
        }
        if j_open {
            for j in &joints {
                joint_row(ui, j, additive, acts);
            }
        }
    }

    // ---- root bodies, sketches, construction
    bodies_folder(ui, "bodies", &root_bodies, 1, additive, acts);
    feature_folder(ui, "sketches", "Sketches", &sketches, true, 1, additive, acts);
    feature_folder(ui, "construction", "Construction", &planes, false, 1, additive, acts);

    // ---- components
    for c in &components {
        let key = format!("comp-{}", c.id);
        let open = is_open(&ctx, &key, true);
        let label = if c.active { format!("{} (active)", c.name) } else { c.name.clone() };
        let o = row(ui, &key, &Row { depth: 1, expand: Some(open), eye: Some(c.visible), icon: Icon::Component, label: &label, selected: false, dim: !c.visible, badge: None, lock: c.grounded });
        if clicked(&o.arrow) || o.row.as_ref().is_some_and(|r| r.clicked()) {
            toggle_open(&ctx, &key, true);
        }
        if clicked(&o.eye) {
            acts.push(Act::SetComponentVisible(c.id.clone(), !c.visible));
        }
        if radio(ui, &key, &o, c.active).is_some_and(|r| r.clicked()) && !c.active {
            acts.push(Act::ActivateComponent(Some(c.id.clone())));
        }
        if let Some(r) = &o.row {
            if r.double_clicked() && !c.active {
                acts.push(Act::ActivateComponent(Some(c.id.clone())));
            }
            component_menu(r, c, acts);
        }
        if open {
            let cb: Vec<&BodyRow> = bodies.iter().filter(|b| b.component.as_deref() == Some(c.id.as_str())).collect();
            let cs: Vec<&FeatureRow> = all_sketches.iter().filter(|s| s.component.as_deref() == Some(c.id.as_str())).collect();
            let cp: Vec<&FeatureRow> = all_planes.iter().filter(|s| s.component.as_deref() == Some(c.id.as_str())).collect();
            bodies_folder(ui, &format!("{key}-bodies"), &cb, 2, additive, acts);
            feature_folder(ui, &format!("{key}-sketches"), "Sketches", &cs, true, 2, additive, acts);
            feature_folder(ui, &format!("{key}-construction"), "Construction", &cp, false, 2, additive, acts);
        }
    }
}

/// Fusion's activation radio button at the right end of a component row.
fn radio(ui: &mut Ui, key: &str, o: &RowOut, active: bool) -> Option<Response> {
    let r = o.row.as_ref()?.rect;
    let rr = Rect::from_center_size(pos2(r.right() - 10.0, r.center().y), vec2(16.0, 16.0));
    let resp = ui.interact(rr, ui.scope_id().with(("browser-radio", key)), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rr, CornerRadius::same(2), theme::PRESSED_BG);
    }
    p.circle_stroke(rr.center(), 5.0, Stroke::new(1.2, if active { theme::ACCENT } else { theme::TEXT_WEAK }));
    if active {
        p.circle_filled(rr.center(), 2.6, theme::ACCENT);
    }
    Some(resp.on_hover_text(if active { "Active component: new features are created here" } else { "Activate this component" }))
}

/// Context menu of a component row: activate, rename, show/hide, delete.
fn component_menu(r: &Response, c: &ComponentRow, acts: &mut Vec<Act>) {
    r.context_menu(|ui| {
        if !c.active && icons::menu_row(ui, Some(Icon::Component), "Activate Component", None, true, None).clicked() {
            acts.push(Act::ActivateComponent(Some(c.id.clone())));
            ui.close();
        }
        // rename: edit the name and press Enter
        let key = Id::unique(("cad360.browser.rename", c.id.as_str()));
        let mut buf = ui.ctx().data_mut(|d| d.get_temp::<String>(key)).unwrap_or_else(|| c.name.clone());
        let mut commit = false;
        ui.horizontal(|ui| {
            ui.label("Name");
            let resp = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(120.0));
            commit = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Rename").clicked() {
                commit = true;
            }
        });
        if commit {
            acts.push(Act::RenameComponent(c.id.clone(), buf.clone()));
            ui.ctx().data_mut(|d| d.remove::<String>(key));
            ui.close();
        } else {
            ui.ctx().data_mut(|d| d.insert_temp(key, buf));
        }
        let (l, ic) = if c.visible { ("Hide", Icon::EyeOff) } else { ("Show", Icon::Eye) };
        if icons::menu_row(ui, Some(ic), l, None, true, None).clicked() {
            acts.push(Act::SetComponentVisible(c.id.clone(), !c.visible));
            ui.close();
        }
        // Fusion "Ground": a grounded component never moves when jointed
        if icons::menu_row(ui, Some(Icon::Component), if c.grounded { "Unground" } else { "Ground" }, None, true, None).on_hover_text("Grounded components stay in place; joints move the other component").clicked() {
            acts.push(Act::GroundComponent(c.id.clone(), !c.grounded));
            ui.close();
        }
        icons::menu_separator(ui);
        if icons::menu_row(ui, Some(Icon::Cmd(CommandId::Delete)), "Delete", None, true, None).on_hover_text("Deletes the component (only when it contains no features)").clicked() {
            acts.push(Act::DeleteComponent(c.id.clone()));
            ui.close();
        }
    });
}

/// A "Bodies" folder with one row per body.
fn bodies_folder(ui: &mut Ui, key: &str, bodies: &[&BodyRow], depth: usize, additive: bool, acts: &mut Vec<Act>) {
    if bodies.is_empty() {
        return;
    }
    let ctx = ui.ctx().clone();
    let open = is_open(&ctx, key, true);
    let any = bodies.iter().any(|b| b.visible);
    let o = row(ui, key, &Row { depth, expand: Some(open), eye: Some(any), icon: Icon::Folder, label: "Bodies", selected: false, dim: false, badge: None, lock: false });
    if clicked(&o.arrow) || clicked(&o.row) {
        toggle_open(&ctx, key, true);
    }
    if clicked(&o.eye) {
        acts.push(Act::SetBodiesVisible(bodies.iter().map(|b| b.id.clone()).collect(), !any));
    }
    if !open {
        return;
    }
    for b in bodies {
        let o = row(ui, &format!("body-{}", b.id), &Row { depth: depth + 1, expand: None, eye: Some(b.visible), icon: Icon::Body, label: &b.name, selected: b.selected, dim: !b.visible || !b.shown, badge: None, lock: false });
        if clicked(&o.eye) {
            acts.push(Act::SetBodyVisible(b.id.clone(), !b.visible));
        }
        if let Some(r) = o.row {
            if r.clicked() {
                acts.push(Act::Select(Pick::Body { body: b.id.clone() }, additive));
            }
            if r.secondary_clicked() && !b.selected {
                acts.push(Act::Select(Pick::Body { body: b.id.clone() }, false));
            }
            r.context_menu(|ui| {
                let (l, ic) = if b.visible { ("Hide", Icon::EyeOff) } else { ("Show", Icon::Eye) };
                if icons::menu_row(ui, Some(ic), l, None, true, None).clicked() {
                    acts.push(Act::SetBodyVisible(b.id.clone(), !b.visible));
                    ui.close();
                }
                if icons::menu_row(ui, Some(Icon::Cmd(CommandId::Appearance)), "Appearance", Some("A"), true, None).clicked() {
                    acts.push(Act::Appearance(b.id.clone()));
                    ui.close();
                }
                if icons::menu_row(ui, Some(Icon::Cmd(CommandId::NewComponent)), "Create Component from Body", None, true, None)
                    .on_hover_text("Moves this body (and the other bodies of its feature) into a new component")
                    .clicked()
                {
                    acts.push(Act::Select(Pick::Body { body: b.id.clone() }, false));
                    acts.push(Act::Command(CommandId::NewComponent));
                    ui.close();
                }
                icons::menu_separator(ui);
                if icons::menu_row(ui, Some(Icon::Cmd(CommandId::Delete)), "Delete", None, true, None).on_hover_text("Deletes the feature that created this body").clicked() {
                    acts.push(Act::DeleteFeature(b.owner.clone()));
                    ui.close();
                }
            });
        }
    }
}

/// A "Sketches" or "Construction" folder.
#[allow(clippy::too_many_arguments)]
fn feature_folder(ui: &mut Ui, key: &str, label: &str, rows: &[&FeatureRow], sketch: bool, depth: usize, additive: bool, acts: &mut Vec<Act>) {
    if rows.is_empty() {
        return;
    }
    let ctx = ui.ctx().clone();
    let open = is_open(&ctx, key, true);
    let any = rows.iter().any(|s| s.visible);
    let o = row(ui, key, &Row { depth, expand: Some(open), eye: Some(any), icon: Icon::Folder, label, selected: false, dim: false, badge: None, lock: false });
    if clicked(&o.arrow) || clicked(&o.row) {
        toggle_open(&ctx, key, true);
    }
    if clicked(&o.eye) {
        for s in rows.iter().filter(|s| s.visible == any) {
            acts.push(Act::ToggleFeatureVisible(s.id.clone()));
        }
    }
    if open {
        let icon = if sketch { Icon::Cmd(CommandId::CreateSketch) } else { Icon::Plane };
        for s in rows {
            feature_row(ui, s, icon, sketch, depth + 1, additive, acts);
        }
    }
}

/// A joint in the Joints folder: click selects, double-click edits.
fn joint_row(ui: &mut Ui, j: &FeatureRow, additive: bool, acts: &mut Vec<Act>) {
    let o = row(ui, &format!("joint-{}", j.id), &Row { depth: 2, expand: None, eye: None, icon: Icon::Cmd(CommandId::Joint), label: &j.name, selected: j.selected, dim: j.suppressed, badge: health_badge(&j.health), lock: false });
    let Some(r) = o.row else { return };
    let pick = Pick::Feature(j.id.clone());
    if r.double_clicked() {
        acts.push(Act::EditFeature(j.id.clone()));
    } else if r.clicked() {
        acts.push(Act::Select(pick.clone(), additive));
    }
    let r = match j.health.message() {
        Some(m) => r.on_hover_text(format!("{} — {m}", j.name)),
        None => r,
    };
    r.context_menu(|ui| {
        if icons::menu_row(ui, Some(Icon::Cmd(CommandId::Joint)), "Edit Joint", None, true, None).clicked() {
            acts.push(Act::EditFeature(j.id.clone()));
            ui.close();
        }
        icons::menu_separator(ui);
        if icons::menu_row(ui, Some(Icon::Cmd(CommandId::Delete)), "Delete", Some("Delete"), true, None).clicked() {
            acts.push(Act::DeleteFeature(j.id.clone()));
            ui.close();
        }
    });
}

fn feature_row(ui: &mut Ui, f: &FeatureRow, icon: Icon, sketch: bool, depth: usize, additive: bool, acts: &mut Vec<Act>) {
    let badge = health_badge(&f.health);
    let o = row(ui, &format!("feat-{}", f.id), &Row { depth, expand: None, eye: Some(f.visible), icon, label: &f.name, selected: f.selected, dim: !f.visible || f.suppressed, badge, lock: f.locked });
    if clicked(&o.eye) {
        acts.push(Act::ToggleFeatureVisible(f.id.clone()));
    }
    let Some(r) = o.row else { return };
    let pick = if sketch { Pick::Feature(f.id.clone()) } else { Pick::ConstructionPlane(f.id.clone()) };
    if r.double_clicked() {
        acts.push(if sketch { Act::EditSketch(f.id.clone()) } else { Act::EditFeature(f.id.clone()) });
    } else if r.clicked() {
        acts.push(Act::Select(pick.clone(), additive));
    }
    if r.secondary_clicked() && !f.selected {
        acts.push(Act::Select(pick.clone(), false));
    }
    let tip = match (&f.health, f.locked) {
        (Health::Error(m), _) | (Health::Warning(m), _) => Some(m.clone()),
        (_, true) => Some("Fully constrained".to_string()),
        _ => None,
    };
    let r = match tip {
        Some(t) => r.on_hover_text(format!("{} — {t}", f.name)),
        None => r,
    };
    r.context_menu(|ui| {
        let edit = if sketch { "Edit Sketch" } else { "Edit Feature" };
        let edit_icon = if sketch { Icon::Cmd(CommandId::CreateSketch) } else { Icon::Cmd(CommandId::OffsetPlane) };
        if icons::menu_row(ui, Some(edit_icon), edit, None, true, None).clicked() {
            acts.push(if sketch { Act::EditSketch(f.id.clone()) } else { Act::EditFeature(f.id.clone()) });
            ui.close();
        }
        if sketch && icons::menu_row(ui, Some(Icon::Cmd(CommandId::Extrude)), "Extrude", Some("E"), true, None).clicked() {
            acts.push(Act::Select(pick.clone(), false));
            acts.push(Act::Command(CommandId::Extrude));
            ui.close();
        }
        if !sketch && icons::menu_row(ui, Some(Icon::Cmd(CommandId::CreateSketch)), "Create Sketch", None, true, None).clicked() {
            acts.push(Act::Select(pick.clone(), false));
            acts.push(Act::Command(CommandId::CreateSketch));
            ui.close();
        }
        let (l, ic) = if f.visible { ("Hide", Icon::EyeOff) } else { ("Show", Icon::Eye) };
        if icons::menu_row(ui, Some(ic), l, None, true, None).clicked() {
            acts.push(Act::ToggleFeatureVisible(f.id.clone()));
            ui.close();
        }
        icons::menu_separator(ui);
        if icons::menu_row(ui, Some(Icon::Cmd(CommandId::Delete)), "Delete", Some("Delete"), true, None).clicked() {
            acts.push(Act::DeleteFeature(f.id.clone()));
            ui.close();
        }
    });
}

fn clicked(r: &Option<Response>) -> bool {
    r.as_ref().is_some_and(|r| r.clicked())
}

/// Draws one tree row. Interaction order: the row first, then the arrow and eye on top of it.
fn row(ui: &mut Ui, key: &str, r: &Row) -> RowOut {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::hover());
    let id = ui.scope_id().with(("browser-row", key));
    let resp = ui.interact(rect, id, Sense::click());
    let p = ui.painter().clone();
    if r.selected {
        p.rect_filled(rect, CornerRadius::same(2), theme::ACCENT_DIM);
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(2), theme::HOVER_BG);
    }
    let mut x = rect.left() + 2.0 + r.depth as f32 * INDENT;
    let cy = rect.center().y;
    let mut out = RowOut::default();
    // arrow
    if let Some(open) = r.expand {
        let ar = Rect::from_center_size(pos2(x + 6.0, cy), vec2(14.0, 14.0));
        let aresp = ui.interact(ar, id.with("arrow"), Sense::click());
        let col_rect = Rect::from_center_size(ar.center(), vec2(8.0, 8.0));
        icons::paint(&p, col_rect, if open { Icon::CaretDown } else { Icon::CaretRight }, true);
        out.arrow = Some(aresp);
    }
    x += 14.0;
    // eye
    if let Some(vis) = r.eye {
        let er = Rect::from_center_size(pos2(x + 8.0, cy), vec2(16.0, 16.0));
        let eresp = ui.interact(er, id.with("eye"), Sense::click());
        if eresp.hovered() {
            p.rect_filled(er, CornerRadius::same(2), theme::PRESSED_BG);
        }
        icons::paint(&p, er.shrink(1.0), if vis { Icon::Eye } else { Icon::EyeOff }, true);
        out.eye = Some(eresp.on_hover_text(if vis { "Hide" } else { "Show" }));
    }
    x += 18.0;
    // icon + label
    icons::paint(&p, Rect::from_center_size(pos2(x + 8.0, cy), vec2(15.0, 15.0)), r.icon, !r.dim);
    x += 21.0;
    let col = if r.dim { theme::TEXT_DISABLED } else { theme::TEXT };
    let lr = p.text(pos2(x, cy), Align2::LEFT_CENTER, r.label, theme::body_font(), col);
    let mut x = lr.right() + 5.0;
    if r.lock {
        icons::paint(&p, Rect::from_center_size(pos2(x + 6.0, cy), vec2(12.0, 12.0)), Icon::Lock, true);
        x += 15.0;
    }
    if let Some(b) = r.badge {
        icons::paint(&p, Rect::from_center_size(pos2(x + 6.0, cy), vec2(13.0, 13.0)), b, true);
    }
    let _ = Color32::TRANSPARENT;
    out.row = Some(resp);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_get_fusion_default_names() {
        assert_eq!(body_display_name(None, 0), "Body1");
        assert_eq!(body_display_name(Some(""), 2), "Body3");
        assert_eq!(body_display_name(Some("Bracket"), 0), "Bracket");
    }

    #[test]
    fn health_badges() {
        assert_eq!(health_badge(&Health::Error("x".into())), Some(Icon::Error));
        assert_eq!(health_badge(&Health::Warning("x".into())), Some(Icon::Warning));
        assert_eq!(health_badge(&Health::Healthy), None);
        assert_eq!(health_badge(&Health::RolledBack), None);
    }

    #[test]
    fn origin_eye_toggles_document_flag() {
        let mut s = AppState::new();
        let before = s.doc.origin_visible;
        apply(&mut s, vec![Act::ToggleOrigin]);
        assert_eq!(s.doc.origin_visible, !before);
        assert!(s.history.can_undo());
    }

    #[test]
    fn component_actions() {
        let mut s = AppState::new();
        apply(&mut s, vec![Act::Command(CommandId::NewComponent)]);
        let c = s.doc.active_component.clone().expect("created and activated");
        apply(&mut s, vec![Act::ActivateComponent(None)]);
        assert_eq!(s.doc.active_component, None);
        apply(&mut s, vec![Act::ActivateComponent(Some(c.clone())), Act::RenameComponent(c.clone(), "Lid".into()), Act::SetComponentVisible(c.clone(), false)]);
        let comp = s.doc.component(&c).unwrap();
        assert_eq!((comp.name.as_str(), comp.visible), ("Lid", false));
        assert_eq!(s.doc.active_component.as_deref(), Some(c.as_str()));
        apply(&mut s, vec![Act::DeleteComponent(c.clone())]);
        assert!(s.doc.components.is_empty(), "an empty component can be deleted");
    }

    #[test]
    fn plane_rows_select_origin_planes() {
        let mut s = AppState::new();
        apply(&mut s, vec![Act::Select(Pick::OriginPlane(OriginPlane::XY), false)]);
        assert_eq!(s.selection, vec![Pick::OriginPlane(OriginPlane::XY)]);
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use crate::ui::icons::testkit::Kit;

    fn frame_fn(s: &mut AppState) -> impl FnMut(&mut Ui) + '_ {
        move |ui: &mut Ui| {
            egui::CentralPanel::default().frame(Frame::NONE).show(ui, |ui| {
                let b = Rect::from_min_size(pos2(8.0, 8.0), vec2(270.0, 400.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(b), |ui| show(ui, s));
            });
        }
    }

    #[test]
    fn origin_eye_and_plane_rows() {
        let mut s = AppState::new();
        let before = s.doc.origin_visible;
        let mut kit = Kit::new();
        // rows: header 22 px, then 21 px rows: root, Document Settings, Named Views, Origin
        let origin_y = 8.0 + 3.0 + 22.0 + 3.0 * ROW_H + ROW_H * 0.5;
        let eye_x = 8.0 + 4.0 + 2.0 + INDENT + 14.0 + 8.0;
        {
            let mut f = frame_fn(&mut s);
            kit.idle(2, &mut f);
            kit.click(pos2(eye_x, origin_y), &mut f);
        }
        assert_eq!(s.doc.origin_visible, !before, "eye toggles origin visibility");
        {
            let mut f = frame_fn(&mut s);
            // expand Origin (click the row), then pick the XY plane (O, X, Y, Z, XY)
            kit.click(pos2(150.0, origin_y), &mut f);
            kit.click(pos2(150.0, origin_y + 5.0 * ROW_H), &mut f);
        }
        assert_eq!(s.selection, vec![Pick::OriginPlane(OriginPlane::XY)]);
    }
}
