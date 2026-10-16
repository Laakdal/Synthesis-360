//! Application shell: Fusion-style layout, viewport interaction, shortcuts and file dialogs.
//!
//! Layout (from the Fusion 2705.1.30 probe screenshots): application bar · ribbon · canvas with
//! floating Browser (top-left), ViewCube (top-right), command dialog / Sketch Palette (right),
//! navigation bar (bottom-centre) · timeline (bottom).

use crate::state::command::{CommandId, COMMANDS};
use crate::state::sketch::{PointerKind, SketchKey, SketchPointer};
use crate::state::{AppState, FileAction, Mode, NavMode, Pick, ViewRequest};
use crate::ui;
use crate::viewport::camera::{Camera, Projection};
use crate::viewport::manipulator::{Manipulator, PointerInput, View};
use crate::viewport::picking::{self, PickContext};
use crate::viewport::renderer::Renderer;
use crate::viewport::scene;
use egui_wgpu::wgpu;
use glam::{Vec2, Vec3};
use std::hash::{Hash, Hasher};

pub struct CadApp {
    pub state: AppState,
    pub cam: Camera,
    renderer: Option<Renderer>,
    mesh_gen: u64,
    mesh_bodies: Vec<String>,
    highlight_key: u64,
    themed: bool,
    pub viewport_rect: egui::Rect,
    /// On-canvas drag handle of the open feature dialog (Extrude arrow, ...).
    pub manip: Manipulator,
    /// Scripted scenario driver (screenshots / automated UI runs).
    pub scenario: Option<crate::scenario::Scenario>,
}

const CANVAS_ID: &str = "cad360-canvas";

impl CadApp {
    pub fn new(cc: &eframe::CreationContext, state: AppState) -> Self {
        let renderer = cc.wgpu_render_state.as_ref().map(|rs| {
            let info = rs.adapter.get_info();
            eprintln!("CAD360: GPU adapter {} ({:?}, {:?})", info.name, info.backend, info.device_type);
            Renderer::new(&rs.device)
        });
        CadApp { state, cam: Camera::default(), renderer, mesh_gen: u64::MAX, mesh_bodies: Vec::new(), highlight_key: 0, themed: false, viewport_rect: egui::Rect::NOTHING, manip: Manipulator::default(), scenario: None }
    }

    /// Camera + viewport for the manipulator (screen <-> world).
    pub fn view(&self) -> View {
        View { cam: self.cam, rect: self.viewport_rect }
    }

    fn aspect(&self) -> f32 {
        (self.viewport_rect.width() / self.viewport_rect.height().max(1.0)).max(0.1)
    }

    /// Bounding sphere of everything visible (bodies and sketches).
    fn scene_bounds(&self) -> (Vec3, f32) {
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        let mut any = false;
        for b in self.state.visible_bodies() {
            let bb = b.topology().bbox;
            lo = lo.min(Vec3::new(bb[0] as f32, bb[1] as f32, bb[2] as f32));
            hi = hi.max(Vec3::new(bb[3] as f32, bb[4] as f32, bb[5] as f32));
            any = true;
        }
        for (fid, r) in &self.state.model.sketches {
            if !self.state.doc.is_active(fid) {
                continue;
            }
            for e in &r.solved.entities {
                if let cad_core::sketch::EntityKind::Point { x, y } = e.kind {
                    let w = r.frame.to_world([x, y]);
                    let p = Vec3::new(w[0] as f32, w[1] as f32, w[2] as f32);
                    lo = lo.min(p);
                    hi = hi.max(p);
                    any = true;
                }
            }
        }
        if !any {
            return (Vec3::ZERO, 60.0);
        }
        ((lo + hi) * 0.5, ((hi - lo).length() * 0.5).max(5.0))
    }

    fn apply_view_requests(&mut self) {
        for r in std::mem::take(&mut self.state.view_requests) {
            match r {
                ViewRequest::Fit => {
                    let (c, rad) = self.scene_bounds();
                    let a = self.aspect();
                    self.cam.fit(c, rad, a);
                }
                ViewRequest::Standard(v) => {
                    self.cam.set_view(v);
                    let (c, rad) = self.scene_bounds();
                    let a = self.aspect();
                    self.cam.fit(c, rad, a);
                }
                ViewRequest::LookAt(o, n, y) => {
                    let n = Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32);
                    let y = Vec3::new(y[0] as f32, y[1] as f32, y[2] as f32);
                    self.cam.set_axes(n, y);
                    let o = Vec3::new(o[0] as f32, o[1] as f32, o[2] as f32);
                    // keep the target on the plane
                    self.cam.target -= n * (self.cam.target - o).dot(n);
                }
            }
        }
        self.cam.projection = if self.state.settings.perspective { Projection::Perspective } else { Projection::Orthographic };
    }

    fn handle_file_request(&mut self) {
        let Some(action) = self.state.file_request.take() else { return };
        let native = |title: &str| rfd::FileDialog::new().set_title(title);
        let name = self.state.doc.name.clone();
        match action {
            FileAction::Open => {
                if let Some(p) = native("Open design").add_filter("CAD360 design", &["c360"]).pick_file() {
                    self.state.open(&p);
                }
            }
            FileAction::Save | FileAction::SaveAs => {
                if let Some(p) = native("Save design").add_filter("CAD360 design", &["c360"]).set_file_name(format!("{name}.c360")).save_file() {
                    self.state.save_to(&p);
                }
            }
            FileAction::ImportStep => {
                if let Some(p) = native("Import STEP").add_filter("STEP", &["step", "stp"]).pick_file() {
                    self.state.import_step(&p);
                }
            }
            FileAction::ExportStep => {
                if let Some(p) = native("Export STEP").add_filter("STEP", &["step", "stp"]).set_file_name(format!("{name}.step")).save_file() {
                    self.state.export_step(&p);
                }
            }
            FileAction::ExportStl => {
                // routes through the 3D Print dialog (refinement, body selection) when it requested the export
                match native("Export STL").add_filter("STL", &["stl"]).set_file_name(format!("{name}.stl")).save_file() {
                    Some(p) => {
                        self.state.export_stl_requested(&p);
                    }
                    None => self.state.export_stl_cancelled(),
                }
            }
            FileAction::InsertMesh => {
                if let Some(p) = native("Insert Mesh").add_filter("Mesh (STL)", &["stl"]).pick_file() {
                    self.state.insert_mesh_file(&p);
                }
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (keys, mods): (Vec<egui::Key>, egui::Modifiers) = ctx.input(|i| {
            (
                i.events.iter().filter_map(|e| if let egui::Event::Key { key, pressed: true, repeat: false, .. } = e { Some(*key) } else { None }).collect(),
                i.modifiers,
            )
        });
        for key in keys {
            let sketching = matches!(self.state.mode, Mode::Sketch(_));
            match key {
                egui::Key::Escape => {
                    if sketching {
                        self.state.sketch_key(SketchKey::Escape);
                    } else if self.state.dialog.is_some() {
                        self.state.dialog_cancel();
                    } else if self.state.plane_prompt {
                        self.state.plane_prompt = false;
                    } else {
                        self.state.clear_selection();
                    }
                    continue;
                }
                egui::Key::Enter => {
                    if sketching {
                        self.state.sketch_key(SketchKey::Enter);
                    } else if self.state.dialog.is_some() {
                        self.state.dialog_ok();
                    }
                    continue;
                }
                egui::Key::Delete | egui::Key::Backspace => {
                    self.state.execute(CommandId::Delete);
                    continue;
                }
                egui::Key::F6 => {
                    self.state.execute(CommandId::Fit);
                    continue;
                }
                _ => {}
            }
            let name = key.name();
            let wanted = if mods.command && mods.shift {
                format!("Ctrl+Shift+{name}")
            } else if mods.command {
                format!("Ctrl+{name}")
            } else {
                name.to_string()
            };
            if let Some(c) = COMMANDS.iter().find(|c| c.shortcut == Some(wanted.as_str()) && self.state.is_enabled(c.id)) {
                self.state.execute(c.id);
            }
        }
    }

    fn sync_gpu(&mut self, rs: &egui_wgpu::RenderState) {
        let Some(r) = self.renderer.as_mut() else { return };
        let visible: Vec<String> = self.state.visible_bodies().iter().map(|b| b.id.clone()).collect();
        if self.mesh_gen != self.state.generation || visible != self.mesh_bodies {
            let (meshes, ids) = scene::build_meshes(&self.state);
            r.set_meshes(&rs.device, &meshes);
            self.mesh_bodies = ids;
            self.mesh_gen = self.state.generation;
            self.highlight_key = 0;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}{:?}", self.state.hover, self.state.selection).hash(&mut h);
        self.state.doc.bodies.iter().for_each(|(k, v)| (k, v.color).hash(&mut h));
        let key = h.finish() | 1;
        if key != self.highlight_key {
            for (i, id) in self.mesh_bodies.iter().enumerate() {
                if let Some(b) = self.state.model.body(id) {
                    let m = b.mesh();
                    r.update_colors(&rs.queue, i, &scene::face_colors(&self.state, id, &m));
                }
            }
            self.highlight_key = key;
        }
        let overlay = self.state.sketch_overlay();
        let ctx = scene::LineContext { cam: &self.cam, plane_prompt: self.state.plane_prompt, overlay: overlay.as_ref() };
        let (lines, fills) = scene::build_lines(&self.state, &ctx);
        r.set_lines(&rs.device, &rs.queue, &lines);
        r.set_fills(&rs.device, &fills);
    }

    fn ndc(&self, p: egui::Pos2) -> Vec2 {
        let r = self.viewport_rect;
        Vec2::new((p.x - r.left()) / r.width() * 2.0 - 1.0, 1.0 - (p.y - r.top()) / r.height() * 2.0)
    }

    pub fn project(&self, p: [f64; 3]) -> Option<egui::Pos2> {
        let n = self.cam.project(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32), self.aspect())?;
        let r = self.viewport_rect;
        Some(egui::pos2(r.left() + (n.x + 1.0) * 0.5 * r.width(), r.top() + (1.0 - n.y) * 0.5 * r.height()))
    }

    /// Hover pick in model mode.
    fn model_pick(&self, pos: egui::Pos2) -> Option<Pick> {
        let ndc = self.ndc(pos);
        let aspect = self.aspect();
        let (o, d) = self.cam.ray(ndc, aspect);
        let face = picking::pick_face(&self.state, o, d);
        let face_t = face.as_ref().map(|f| f.2).unwrap_or(f32::MAX);
        if self.state.plane_prompt {
            let plane = picking::pick_origin_plane(o, d, scene::origin_plane_half(&self.cam));
            return match (plane, face) {
                (Some((p, t)), _) if t <= face_t => Some(Pick::OriginPlane(p)),
                (_, Some((body, face, _))) => Some(Pick::Face { body, face }),
                (Some((p, _)), None) => Some(Pick::OriginPlane(p)),
                _ => None,
            };
        }
        let ctx = PickContext { cam: &self.cam, aspect, size: Vec2::new(self.viewport_rect.width(), self.viewport_rect.height()), ndc };
        if let Some(p) = picking::pick_profile(&self.state, o, d, face_t + 0.05) {
            return Some(p);
        }
        if let Some((body, edge, _)) = picking::pick_edge(&self.state, &ctx, 6.0) {
            return Some(Pick::Edge { body, edge });
        }
        face.map(|(body, face, _)| Pick::Face { body, face })
    }

    fn canvas_input(&mut self, ctx: &egui::Context, resp: &egui::Response) {
        let rect = self.viewport_rect;
        let aspect = self.aspect();
        let (shift, ctrl, scroll) = ctx.input(|i| (i.modifiers.shift, i.modifiers.command, i.smooth_scroll_delta.y));
        let hover_pos = resp.hover_pos();

        // zoom
        if resp.hovered() && scroll.abs() > 0.0 {
            if let Some(p) = hover_pos {
                let n = self.ndc(p);
                self.cam.zoom((-scroll * 0.0018).exp(), n, aspect);
            }
        }
        // navigation drags (Fusion: middle = pan, Shift+middle = orbit; nav-bar modes use left)
        let d = resp.drag_delta();
        let dv = Vec2::new(d.x, d.y);
        if resp.dragged_by(egui::PointerButton::Middle) {
            if shift {
                self.cam.orbit(dv);
            } else {
                self.cam.pan(dv, rect.height());
            }
        }
        let nav = self.state.settings.nav;
        if resp.dragged_by(egui::PointerButton::Primary) && nav != NavMode::Select {
            match nav {
                NavMode::Orbit => self.cam.orbit(dv),
                NavMode::Pan => self.cam.pan(dv, rect.height()),
                NavMode::Zoom => {
                    let c = self.ndc(rect.center());
                    self.cam.zoom((dv.y * 0.01).exp(), c, aspect)
                }
                NavMode::Select => {}
            }
            return;
        }

        if matches!(self.state.mode, Mode::Sketch(_)) {
            self.sketch_input(ctx, resp);
            return;
        }

        // on-canvas manipulators take priority over picking while hovered or dragged
        let (pressed, released, moved, time, latest) = ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.delta() != egui::Vec2::ZERO, i.time, i.pointer.latest_pos()));
        let pos = if self.manip.dragging() { latest.or(hover_pos) } else { hover_pos };
        let input = PointerInput { pos, pressed: pressed && resp.hovered(), released, moved, shift, time };
        let view = self.view();
        if self.manip.input(&mut self.state, &view, &input) {
            self.state.hover = None;
            return;
        }

        // model-mode hover and selection
        if let Some(p) = hover_pos {
            if !resp.dragged() {
                self.state.hover = self.model_pick(p);
            }
        } else {
            self.state.hover = None;
        }
        if resp.clicked() {
            match self.state.hover.clone() {
                Some(p) => {
                    if self.state.plane_prompt {
                        self.state.selection = vec![p];
                        self.state.execute(CommandId::CreateSketch);
                    } else {
                        self.state.select(p, shift || ctrl);
                    }
                }
                None => {
                    if !(shift || ctrl) {
                        self.state.clear_selection();
                    }
                }
            }
        }
    }

    fn sketch_input(&mut self, ctx: &egui::Context, resp: &egui::Response) {
        let Some(frame) = self.state.active_sketch().and_then(|s| self.state.model.sketches.get(&s.feature)).map(|r| r.frame) else { return };
        let Some(p) = resp.hover_pos().or(resp.interact_pointer_pos()) else { return };
        let (o, d) = self.cam.ray(self.ndc(p), self.aspect());
        let o3 = [o.x as f64, o.y as f64, o.z as f64];
        let d3 = [d.x as f64, d.y as f64, d.z as f64];
        let Some(pos) = frame.intersect_ray(o3, d3) else { return };
        let mm_per_px = 2.0 * self.cam.half_height() as f64 / self.viewport_rect.height().max(1.0) as f64;
        let (shift, ctrl, pressed, released, moved) = ctx.input(|i| (i.modifiers.shift, i.modifiers.command, i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.is_moving()));
        let mk = |kind| SketchPointer { pos, tolerance: 8.0 * mm_per_px, kind, shift, ctrl };
        if resp.hovered() && pressed {
            self.state.sketch_pointer(mk(PointerKind::Press));
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            self.state.sketch_pointer(mk(PointerKind::Drag));
        } else if moved || resp.hovered() {
            self.state.sketch_pointer(mk(PointerKind::Move));
        }
        if released {
            self.state.sketch_pointer(mk(PointerKind::Release));
        }
        if resp.double_clicked() {
            self.state.sketch_pointer(mk(PointerKind::DoubleClick));
        } else if resp.clicked() {
            self.state.sketch_pointer(mk(PointerKind::Click));
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let rect = ui.available_rect_before_wrap();
        self.viewport_rect = rect;
        let ctx = ui.ctx().clone();
        let resp = ui.interact(rect, egui::Id::new(CANVAS_ID), egui::Sense::click_and_drag());
        self.canvas_input(&ctx, &resp);
        ui::context_menu::show(&resp, &mut self.state);
        self.apply_view_requests();

        if let Some(rs) = frame.wgpu_render_state().cloned() {
            let ppp = ctx.pixels_per_point();
            let size = [(rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32];
            if let Some(r) = self.renderer.as_mut() {
                if r.resize(&rs.device, size) {
                    let view = r.resolve_view().expect("targets exist after resize");
                    let mut er = rs.renderer.write();
                    match r.texture_id {
                        Some(id) => er.update_egui_texture_from_wgpu_texture(&rs.device, view, wgpu::FilterMode::Linear, id),
                        None => r.texture_id = Some(er.register_native_texture(&rs.device, view, wgpu::FilterMode::Linear)),
                    }
                }
            }
            self.sync_gpu(&rs);
            if let Some(r) = self.renderer.as_mut() {
                r.section = self.state.settings.section.map(|s| s.clip());
            }
            if let Some(r) = self.renderer.as_ref() {
                r.render(&rs.device, &rs.queue, &self.cam);
                if let Some(id) = r.texture_id {
                    ui.painter().image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                }
                self.manip.paint(&ui.painter_at(rect), &self.state, &self.view());
            }
        } else {
            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(30, 32, 36));
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "No GPU renderer available", egui::FontId::proportional(16.0), egui::Color32::GRAY);
        }

        // ---- overlays on the canvas
        let cam_snapshot = self.cam;
        let project = move |p: [f64; 3], rect: egui::Rect, aspect: f32| -> Option<egui::Pos2> {
            let n = cam_snapshot.project(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32), aspect)?;
            Some(egui::pos2(rect.left() + (n.x + 1.0) * 0.5 * rect.width(), rect.top() + (1.0 - n.y) * 0.5 * rect.height()))
        };
        let aspect = self.aspect();
        let proj = |p: [f64; 3]| project(p, rect, aspect);
        let sketching = matches!(self.state.mode, Mode::Sketch(_));
        if sketching {
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| ui::sketch_ui::labels(ui, &mut self.state, &proj));
        }
        // Browser (top-left)
        let browser_rect = egui::Rect::from_min_size(rect.min + egui::vec2(8.0, 8.0), egui::vec2(270.0, (rect.height() * 0.6).min(520.0)));
        ui.scope_builder(egui::UiBuilder::new().max_rect(browser_rect), |ui| ui::browser::show(ui, &mut self.state));
        // ViewCube (top-right)
        let cube = egui::Rect::from_min_size(egui::pos2(rect.right() - 128.0, rect.top() + 12.0), egui::vec2(116.0, 116.0));
        let mut cam = self.cam;
        if ui::viewcube::show(ui, cube, &mut cam) {
            self.cam = cam;
        }
        // command dialog or Sketch Palette (right)
        let side = egui::Rect::from_min_size(egui::pos2(rect.right() - 318.0, rect.top() + 140.0), egui::vec2(306.0, rect.height() - 200.0));
        if self.state.dialog.is_some() {
            ui.scope_builder(egui::UiBuilder::new().max_rect(side), |ui| ui::dialogs::show(ui, &mut self.state));
        } else if sketching {
            ui.scope_builder(egui::UiBuilder::new().max_rect(side), |ui| ui::sketch_ui::palette(ui, &mut self.state));
        } else if self.state.settings.section.is_some() {
            let bounds = self.scene_bounds();
            ui.scope_builder(egui::UiBuilder::new().max_rect(side), |ui| section_panel(ui, &mut self.state, bounds));
        }
        // navigation bar (bottom-centre) and messages (bottom-right)
        let nav = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.bottom() - 22.0), egui::vec2(420.0, 34.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(nav), |ui| ui::navbar::show(ui, &mut self.state));
        let status = egui::Rect::from_min_max(egui::pos2(rect.right() - 520.0, rect.bottom() - 60.0), egui::pos2(rect.right() - 8.0, rect.bottom() - 6.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(status), |ui| ui::statusbar::show(ui, &mut self.state));
    }
}

/// Section Analysis panel (display-only cutting plane).
fn section_panel(ui: &mut egui::Ui, state: &mut AppState, bounds: (Vec3, f32)) {
    let Some(mut sec) = state.settings.section else { return };
    egui::Frame::new().fill(egui::Color32::from_rgb(43, 46, 52)).corner_radius(4.0).inner_margin(10.0).show(ui, |ui| {
        ui.set_width(286.0);
        ui.label(egui::RichText::new("SECTION ANALYSIS").strong());
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Plane");
            for p in [cad_core::OriginPlane::XY, cad_core::OriginPlane::XZ, cad_core::OriginPlane::YZ] {
                if ui.selectable_value(&mut sec.plane, p, format!("{p:?}")).clicked() {
                    sec.offset = state.section_center(p);
                }
            }
        });
        let r = (bounds.1 * 1.2) as f64 + 1.0;
        let c = bounds.0;
        let center = match sec.plane {
            cad_core::OriginPlane::XY => c.z,
            cad_core::OriginPlane::XZ => c.y,
            cad_core::OriginPlane::YZ => c.x,
        } as f64;
        ui.horizontal(|ui| {
            ui.label("Offset");
            ui.add(egui::Slider::new(&mut sec.offset, (center - r)..=(center + r)).suffix(" mm"));
        });
        ui.checkbox(&mut sec.flip, "Flip");
        ui.horizontal(|ui| {
            if ui.button("Close").clicked() {
                state.settings.section = None;
            }
        });
    });
    if state.settings.section.is_some() {
        state.settings.section = Some(sec);
    }
}

impl eframe::App for CadApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.themed {
            ui::theme::apply(&ctx);
            self.themed = true;
            self.state.view_requests.push(ViewRequest::Standard(crate::viewport::camera::StandardView::Home));
        }
        if let Some(mut sc) = self.scenario.take() {
            sc.step(self, &ctx);
            self.scenario = Some(sc);
        }
        self.handle_file_request();
        self.shortcuts(&ctx);

        egui::Panel::top("appbar").exact_size(30.0).show(ui, |ui| ui::appbar::show(ui, &mut self.state));
        egui::Panel::top("ribbon").exact_size(92.0).show(ui, |ui| ui::ribbon::show(ui, &mut self.state));
        egui::Panel::bottom("timeline").exact_size(40.0).show(ui, |ui| ui::timeline::show(ui, &mut self.state));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.canvas(ui, frame));

        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} - CAD360", self.state.title())));
        if self.state.active_sketch().is_some() || self.scenario.is_some() || self.state.hover.is_some() || self.manip.hovered() || self.manip.dragging() {
            ctx.request_repaint();
        }
    }
}
