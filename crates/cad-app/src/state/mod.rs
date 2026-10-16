//! Headless application state. Everything the UI shows comes from here and every user action
//! is a method call here, so complete workflows can be tested with plain `cargo test` (no
//! window, no GPU). The egui layer only translates input into these calls and draws the state.

pub mod command;
pub mod features;
pub mod sketch;
#[cfg(test)]
mod workflow_tests;

use cad_core::history::History;
use cad_core::{Document, FeatureId, OriginPlane};
use cad_kernel::{Health, Kernel, Model};
use command::{spec, CommandId, NOT_SUPPORTED};
use std::path::{Path, PathBuf};

pub use features::Dialog;
pub use sketch::{SketchSession, SketchTool};

/// Something the user can hover or select.
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    Face { body: String, face: usize },
    Edge { body: String, edge: usize },
    Body { body: String },
    /// A closed region of a (finished, visible) sketch, selectable in model mode for Extrude.
    Profile { sketch: FeatureId, index: usize },
    OriginPlane(OriginPlane),
    ConstructionPlane(FeatureId),
    /// A timeline / browser feature.
    Feature(FeatureId),
    /// Inside an active sketch.
    SketchEntity(String),
    SketchConstraint(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub level: Level,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavMode {
    Select,
    Orbit,
    Pan,
    Zoom,
}

/// Section analysis plane (Fusion INSPECT › Section Analysis): an origin-aligned cutting plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    pub plane: OriginPlane,
    pub offset: f64,
    pub flip: bool,
}

impl Section {
    /// (normal, offset) of the half-space that stays visible: dot(n, p) <= offset.
    pub fn clip(&self) -> ([f32; 3], f32) {
        let n = cad_kernel::Frame::origin_plane(self.plane).n;
        let s = if self.flip { -1.0 } else { 1.0 };
        ([(n[0] * s) as f32, (n[1] * s) as f32, (n[2] * s) as f32], (self.offset * s) as f32)
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub grid: bool,
    pub snap: bool,
    pub show_profiles: bool,
    pub show_dimensions: bool,
    pub show_constraints: bool,
    pub show_points: bool,
    pub perspective: bool,
    pub nav: NavMode,
    pub section: Option<Section>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { grid: true, snap: true, show_profiles: true, show_dimensions: true, show_constraints: true, show_points: true, perspective: false, nav: NavMode::Select, section: None }
    }
}

/// Camera requests for the UI layer (the camera itself is UI state).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewRequest {
    Fit,
    Standard(crate::viewport::camera::StandardView),
    /// Look straight at a plane: (origin, normal, up/y axis).
    LookAt([f64; 3], [f64; 3], [f64; 3]),
}

/// File operations that need a native file dialog (completed by the UI layer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileAction {
    Open,
    Save,
    SaveAs,
    ImportStep,
    ExportStep,
    ExportStl,
    /// INSERT ▸ Insert Mesh: pick an STL to embed as a mesh body.
    InsertMesh,
}

pub enum Mode {
    Model,
    Sketch(SketchSession),
}

struct Txn {
    label: String,
    before: Document,
}

pub struct AppState {
    pub doc: Document,
    pub kernel: Kernel,
    pub model: Model,
    pub history: History,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub selection: Vec<Pick>,
    pub hover: Option<Pick>,
    pub mode: Mode,
    pub dialog: Option<Dialog>,
    pub messages: Vec<Message>,
    pub settings: Settings,
    pub view_requests: Vec<ViewRequest>,
    /// Incremented on every recompute (the viewport rebuilds its meshes when it changes).
    pub generation: u64,
    /// Create Sketch is waiting for a plane / planar face to be picked.
    pub plane_prompt: bool,
    /// File action waiting for the UI to show a file dialog.
    pub file_request: Option<FileAction>,
    txn: Option<Txn>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        let mut s = AppState {
            doc: Document::new("Untitled"),
            kernel: Kernel::new(),
            model: Model::default(),
            history: History::new(),
            path: None,
            dirty: false,
            selection: Vec::new(),
            hover: None,
            mode: Mode::Model,
            dialog: None,
            messages: Vec::new(),
            settings: Settings::default(),
            view_requests: Vec::new(),
            generation: 0,
            plane_prompt: false,
            file_request: None,
            txn: None,
        };
        s.recompute();
        s
    }

    // ------------------------------------------------------------------ messages
    pub fn info(&mut self, t: impl Into<String>) {
        self.push(Level::Info, t)
    }
    pub fn warn(&mut self, t: impl Into<String>) {
        self.push(Level::Warning, t)
    }
    pub fn error(&mut self, t: impl Into<String>) {
        self.push(Level::Error, t)
    }
    fn push(&mut self, level: Level, t: impl Into<String>) {
        self.messages.push(Message { level, text: t.into() });
        if self.messages.len() > 200 {
            self.messages.remove(0);
        }
    }
    pub fn last_message(&self) -> Option<&Message> {
        self.messages.last()
    }

    // ------------------------------------------------------------------ model
    /// Re-evaluates the design (incremental) and stores solved sketch geometry.
    pub fn recompute(&mut self) {
        self.model = self.kernel.recompute(&self.doc);
        self.generation += 1;
        cad_kernel::store_solved_sketches(&mut self.doc, &self.model);
        // drop selections that no longer exist
        let model = &self.model;
        self.selection.retain(|p| match p {
            Pick::Face { body, .. } | Pick::Edge { body, .. } | Pick::Body { body } => model.body(body).is_some(),
            Pick::Profile { sketch, .. } => model.sketches.contains_key(sketch),
            _ => true,
        });
    }

    pub fn health(&self, id: &str) -> Health {
        self.model.health_of(id)
    }

    /// Applies one undoable operation. On error the document is restored and the error shown.
    pub fn apply(&mut self, label: &str, f: impl FnOnce(&mut Document) -> Result<(), String>) -> bool {
        let before = self.doc.clone();
        match f(&mut self.doc) {
            Ok(()) => {
                if self.txn.is_none() {
                    self.history.record(label, &before);
                }
                self.dirty = true;
                self.recompute();
                true
            }
            Err(e) => {
                self.doc = before;
                self.error(e);
                false
            }
        }
    }

    /// Starts a transaction (a command dialog or sketch session): later edits preview live and
    /// become one undo step on commit, or are discarded on cancel.
    pub fn begin_txn(&mut self, label: &str) {
        if self.txn.is_none() {
            self.txn = Some(Txn { label: label.into(), before: self.doc.clone() });
        }
    }
    pub fn in_txn(&self) -> bool {
        self.txn.is_some()
    }
    pub fn commit_txn(&mut self) {
        if let Some(t) = self.txn.take() {
            if t.before != self.doc {
                self.history.record(&t.label, &t.before);
                self.dirty = true;
            }
        }
        self.recompute();
    }
    pub fn cancel_txn(&mut self) {
        if let Some(t) = self.txn.take() {
            self.doc = t.before;
        }
        self.recompute();
    }

    pub fn undo(&mut self) {
        self.abort_interactions();
        match self.history.undo(&self.doc) {
            Some(d) => {
                let label = self.history.redo_label().unwrap_or("").to_string();
                self.doc = d;
                self.dirty = true;
                self.recompute();
                self.info(format!("Undo {label}"));
            }
            None => self.info("Nothing to undo"),
        }
    }
    pub fn redo(&mut self) {
        self.abort_interactions();
        match self.history.redo(&self.doc) {
            Some(d) => {
                let label = self.history.undo_label().unwrap_or("").to_string();
                self.doc = d;
                self.dirty = true;
                self.recompute();
                self.info(format!("Redo {label}"));
            }
            None => self.info("Nothing to redo"),
        }
    }

    /// Cancels an open dialog or sketch session (restoring the state before it).
    pub fn abort_interactions(&mut self) {
        if self.dialog.is_some() {
            self.dialog_cancel();
        }
        if matches!(self.mode, Mode::Sketch(_)) {
            self.cancel_sketch();
        }
    }

    // ------------------------------------------------------------------ selection
    pub fn select(&mut self, p: Pick, additive: bool) {
        if !additive {
            self.selection.clear();
        }
        if let Some(i) = self.selection.iter().position(|q| *q == p) {
            if additive {
                self.selection.remove(i);
            }
        } else {
            self.selection.push(p);
        }
        self.on_selection_changed();
    }
    pub fn clear_selection(&mut self) {
        self.selection.clear();
        self.on_selection_changed();
    }

    // ------------------------------------------------------------------ files
    pub fn new_design(&mut self) {
        self.abort_interactions();
        self.doc = Document::new("Untitled");
        self.kernel.clear();
        self.history.clear();
        self.path = None;
        self.dirty = false;
        self.selection.clear();
        self.recompute();
        self.view_requests.push(ViewRequest::Standard(crate::viewport::camera::StandardView::Home));
        self.info("New design");
    }

    pub fn open(&mut self, path: &Path) -> bool {
        match std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| Document::from_json(&t).map_err(|e| e.to_string())) {
            Ok(doc) => {
                self.abort_interactions();
                self.doc = doc;
                self.kernel.clear();
                self.history.clear();
                self.path = Some(path.to_path_buf());
                self.dirty = false;
                self.selection.clear();
                self.recompute();
                self.view_requests.push(ViewRequest::Fit);
                self.info(format!("Opened {}", path.display()));
                true
            }
            Err(e) => {
                self.error(format!("Could not open {}: {e}", path.display()));
                false
            }
        }
    }

    pub fn save_to(&mut self, path: &Path) -> bool {
        if matches!(self.mode, Mode::Sketch(_)) || self.dialog.is_some() {
            self.warn("Finish or cancel the active command before saving.");
            return false;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            self.doc.name = stem.to_string();
        }
        match std::fs::write(path, self.doc.to_json()) {
            Ok(()) => {
                self.path = Some(path.to_path_buf());
                self.dirty = false;
                self.info(format!("Saved {}", path.display()));
                true
            }
            Err(e) => {
                self.error(format!("Could not save {}: {e}", path.display()));
                false
            }
        }
    }

    pub fn visible_bodies(&self) -> Vec<&cad_kernel::Body> {
        // a body is shown when it and its component are visible
        self.model
            .bodies
            .iter()
            .filter(|b| self.doc.bodies.get(&b.id).is_none_or(|m| m.visible) && self.doc.component_visible(self.doc.body_component(&b.id, Some(&b.created_by))))
            .collect()
    }

    pub fn export_step(&mut self, path: &Path) -> bool {
        // STEP carries exact B-rep; tessellated mesh bodies have no surfaces to export
        let all: Vec<cad_kernel::Body> = self.visible_bodies().into_iter().cloned().collect();
        let skipped = all.iter().filter(|b| b.kind == cad_kernel::BodyKind::Mesh).count();
        let bodies: Vec<&cad_kernel::Body> = all.iter().filter(|b| b.kind != cad_kernel::BodyKind::Mesh).collect();
        if skipped > 0 {
            self.warn(format!("{skipped} mesh bod{} skipped: STEP needs B-rep geometry (convert the mesh to B-rep first).", if skipped == 1 { "y" } else { "ies" }));
        }
        match cad_kernel::exchange::export_step(&bodies, &path.to_string_lossy()) {
            Ok(()) => {
                self.info(format!("Exported STEP {} (geometry only, no history)", path.display()));
                true
            }
            Err(e) => {
                self.error(e.0);
                false
            }
        }
    }

    pub fn export_stl(&mut self, path: &Path) -> bool {
        let bodies = self.visible_bodies();
        match cad_kernel::exchange::export_stl(&bodies, &path.to_string_lossy(), false) {
            Ok(()) => {
                self.info(format!("Exported STL {} (mesh only, no history)", path.display()));
                true
            }
            Err(e) => {
                self.error(e.0);
                false
            }
        }
    }

    pub fn import_step(&mut self, path: &Path) -> bool {
        match cad_kernel::exchange::read_step_text(&path.to_string_lossy()) {
            Ok(text) => {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let ok = self.apply("Import STEP", |d| {
                    cad_core::ops::new_import(d, &name, text);
                    Ok(())
                });
                if ok {
                    self.view_requests.push(ViewRequest::Fit);
                }
                ok
            }
            Err(e) => {
                self.error(e.0);
                false
            }
        }
    }

    // ------------------------------------------------------------------ timeline
    pub fn set_marker(&mut self, marker: usize) {
        if self.doc.marker != marker {
            self.apply("Move Timeline Marker", |d| {
                d.set_marker(marker);
                Ok(())
            });
        }
    }

    pub fn toggle_suppress(&mut self, id: &str) {
        let id = id.to_string();
        self.apply("Suppress Feature", |d| {
            let f = d.feature_mut(&id).ok_or("unknown feature")?;
            f.suppressed = !f.suppressed;
            Ok(())
        });
    }

    pub fn delete_feature(&mut self, id: &str) {
        let id = id.to_string();
        let deps = cad_core::dependent_features(&self.doc, &id);
        let name = self.doc.feature(&id).map(|f| f.name.clone()).unwrap_or_default();
        if self.apply(&format!("Delete {name}"), |d| d.delete_feature(&id).map_err(|e| e.to_string())) && !deps.is_empty() {
            let names: Vec<String> = deps.iter().filter_map(|d| self.doc.feature(d).map(|f| f.name.clone())).collect();
            self.warn(format!("Deleted {name}; dependent features may fail: {}", names.join(", ")));
        }
    }

    pub fn rename_feature(&mut self, id: &str, name: &str) {
        let (id, name) = (id.to_string(), name.trim().to_string());
        if name.is_empty() {
            return;
        }
        self.apply("Rename", |d| {
            d.feature_mut(&id).ok_or("unknown feature")?.name = name;
            Ok(())
        });
    }

    pub fn set_body_visible(&mut self, body: &str, visible: bool) {
        let b = body.to_string();
        self.apply("Toggle Visibility", |d| {
            d.bodies.entry(b).or_default().visible = visible;
            Ok(())
        });
    }

    pub fn set_parameter(&mut self, name: &str, expression: &str) -> bool {
        let (n, e) = (name.to_string(), expression.to_string());
        self.apply(&format!("Change Parameter {name}"), |d| d.set_expression(&n, &e).map(|_| ()).map_err(|err| format!("{n}: {err}")))
    }

    // ------------------------------------------------------------------ commands
    pub fn is_enabled(&self, id: CommandId) -> bool {
        let s = spec(id);
        if !s.supported {
            return false;
        }
        let sketching = matches!(self.mode, Mode::Sketch(_));
        use CommandId::*;
        match id {
            Undo => self.history.can_undo() || self.in_txn(),
            Redo => self.history.can_redo(),
            FinishSketch | Line | Rectangle2Pt | RectangleCenter | CircleCenter | Arc3Pt | Point | SketchDimension | ToggleConstruction | ConstraintCoincident | ConstraintCollinear
            | ConstraintConcentric | ConstraintMidpoint | ConstraintFix | ConstraintParallel | ConstraintPerpendicular | ConstraintHorizontalVertical | ConstraintTangent
            | ConstraintEqual | ConstraintSymmetry | Rectangle3Pt | Circle2Pt | ArcCenter | Polygon | Slot | SketchFillet | Trim | Extend | SketchOffset | SketchMirror => sketching,
            Extrude | Revolve | Hole | Fillet | Chamfer | OffsetPlane | CreateSketch | ImportStep | Shell | Combine | MirrorFeature | RectangularPattern | CircularPattern | Move | NewComponent | Joint
            | SurfaceExtrude | SurfaceRevolve | Patch | Stitch | OffsetSurface | Thicken | InsertMesh | ThreeDPrint => !sketching,
            _ => true,
        }
    }

    /// Runs a command. File commands that need a path are completed by the UI (dialogs) and
    /// then call `open` / `save_to` / `export_*` / `import_step` directly.
    pub fn execute(&mut self, id: CommandId) {
        let s = spec(id);
        if !s.supported {
            self.warn(format!("{}: {NOT_SUPPORTED}", s.label));
            return;
        }
        if !self.is_enabled(id) {
            self.info(format!("{} is not available right now", s.label));
            return;
        }
        use crate::viewport::camera::StandardView as V;
        use CommandId::*;
        match id {
            NewDesign => self.new_design(),
            Undo => {
                if self.dialog.is_some() || matches!(self.mode, Mode::Sketch(_)) {
                    self.abort_interactions()
                } else {
                    self.undo()
                }
            }
            Redo => self.redo(),
            ComputeAll => {
                self.kernel.clear();
                self.recompute();
                self.info("Recomputed all features");
            }
            Delete => self.delete_selection(),
            Fit => self.view_requests.push(ViewRequest::Fit),
            ViewHome => self.view_requests.push(ViewRequest::Standard(V::Home)),
            ViewTop => self.view_requests.push(ViewRequest::Standard(V::Top)),
            ViewFront => self.view_requests.push(ViewRequest::Standard(V::Front)),
            ViewRight => self.view_requests.push(ViewRequest::Standard(V::Right)),
            ViewBack => self.view_requests.push(ViewRequest::Standard(V::Back)),
            ViewLeft => self.view_requests.push(ViewRequest::Standard(V::Left)),
            ViewBottom => self.view_requests.push(ViewRequest::Standard(V::Bottom)),
            ToggleProjection => self.settings.perspective = !self.settings.perspective,
            ToggleGrid => self.settings.grid = !self.settings.grid,
            SectionAnalysis => {
                self.settings.section = match self.settings.section {
                    Some(_) => None,
                    // start through the middle of the visible bodies (Fusion places the section on the model)
                    None => Some(Section { plane: OriginPlane::XZ, offset: self.section_center(OriginPlane::XZ), flip: false }),
                };
            }
            OrbitMode => self.settings.nav = if self.settings.nav == NavMode::Orbit { NavMode::Select } else { NavMode::Orbit },
            PanMode => self.settings.nav = if self.settings.nav == NavMode::Pan { NavMode::Select } else { NavMode::Pan },
            ZoomMode => self.settings.nav = if self.settings.nav == NavMode::Zoom { NavMode::Select } else { NavMode::Zoom },
            LookAt => self.look_at_selection(),
            Open => self.file_request = Some(FileAction::Open),
            Save => match self.path.clone() {
                Some(p) => {
                    self.save_to(&p);
                }
                None => self.file_request = Some(FileAction::SaveAs),
            },
            SaveAs => self.file_request = Some(FileAction::SaveAs),
            ImportStep => self.file_request = Some(FileAction::ImportStep),
            ExportStep => self.file_request = Some(FileAction::ExportStep),
            ExportStl => self.file_request = Some(FileAction::ExportStl),
            CreateSketch | FinishSketch | Line | Rectangle2Pt | RectangleCenter | CircleCenter | Arc3Pt | Point | SketchDimension | ToggleConstruction | ConstraintCoincident
            | ConstraintCollinear | ConstraintConcentric | ConstraintMidpoint | ConstraintFix | ConstraintParallel | ConstraintPerpendicular | ConstraintHorizontalVertical
            | ConstraintTangent | ConstraintEqual | ConstraintSymmetry | Rectangle3Pt | Circle2Pt | ArcCenter | Polygon | Slot | SketchFillet | Trim | Extend | SketchOffset
            | SketchMirror => self.sketch_command(id),
            _ => self.feature_command(id),
        }
    }

    pub fn delete_selection(&mut self) {
        if matches!(self.mode, Mode::Sketch(_)) {
            self.sketch_delete_selection();
            return;
        }
        let feats: Vec<String> = self.selection.iter().filter_map(|p| if let Pick::Feature(f) = p { Some(f.clone()) } else { None }).collect();
        if feats.is_empty() {
            self.info("Select a feature in the timeline or browser to delete it.");
        }
        for f in feats {
            self.delete_feature(&f);
        }
        self.selection.clear();
    }

    fn look_at_selection(&mut self) {
        if let Mode::Sketch(s) = &self.mode {
            if let Some(r) = self.model.sketches.get(&s.feature) {
                let f = r.frame;
                self.view_requests.push(ViewRequest::LookAt(f.origin, f.n, f.y));
                return;
            }
        }
        for p in &self.selection {
            if let Pick::Face { body, face } = p {
                if let Some(b) = self.model.body(body) {
                    let fi = &b.topology().face_info[*face];
                    let fr = cad_kernel::Frame::on_plane(fi.centroid, fi.normal);
                    self.view_requests.push(ViewRequest::LookAt(fi.centroid, fi.normal, fr.y));
                    return;
                }
            }
        }
        self.info("Select a planar face (or edit a sketch) to look at it.");
    }

    /// Centre of the visible bodies along the normal of an origin plane.
    pub fn section_center(&self, plane: OriginPlane) -> f64 {
        let k = match plane {
            OriginPlane::XY => 2,
            OriginPlane::XZ => 1,
            OriginPlane::YZ => 0,
        };
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for b in self.visible_bodies() {
            let bb = b.topology().bbox;
            lo = lo.min(bb[k]);
            hi = hi.max(bb[k + 3]);
        }
        if lo <= hi { 0.5 * (lo + hi) } else { 0.0 }
    }

    /// Title for the window / document tab.
    pub fn title(&self) -> String {
        format!("{}{}", self.doc.name, if self.dirty { "*" } else { "" })
    }
}
