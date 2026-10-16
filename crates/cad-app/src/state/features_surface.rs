//! Dialogs of the SURFACE, MESH and UTILITIES tabs: Surface Extrude / Revolve, Patch, Stitch,
//! Offset, Thicken, Insert Mesh and 3D Print. Same session model as the other feature dialogs
//! (see features.rs): live preview inside a transaction, OK = one undo step, Cancel restores.
//!
//! Inputs:
//! * Surface Extrude / Revolve work on the curves of a finished sketch. In the canvas the sketch is
//!   picked through one of its profiles (the chains bounding that profile are toggled) or through
//!   the timeline / browser (every chain is selected); the dialog lists the sketch's curve chains
//!   so open chains, which have no profile, can be switched on and off.
//! * Patch takes closed profiles of one sketch and/or a closed loop of body edges.
//! * Stitch takes surface bodies; Offset / Thicken take faces (a body pick adds all its faces).
//! * 3D Print takes bodies (none = every visible body) and a refinement; OK asks the UI for the STL
//!   file name (`FileAction::ExportStl`) and the dialog stays open until the export runs
//!   ([`AppState::export_stl_requested`]).

use super::{apply_expr, param_expr, Dialog, ExprField, Route, Session};
use crate::state::command::{spec, CommandId};
use crate::state::{AppState, FileAction, Mode, Pick};
use cad_core::{ops, BodyId, EdgeRef, ExtentDirection, FaceRef, FeatureId, FeatureKind, ProfileRef, RevolveAxis};
use cad_kernel::mesh::Refinement;
use cad_kernel::surface::{sketch_chains, Chain};
use cad_kernel::{refs, BodyKind, Model};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct SurfExtrudeDialog {
    pub s: Session,
    pub sketch: Option<FeatureId>,
    /// Selected sketch curves (entity ids).
    pub curves: Vec<String>,
    pub direction: ExtentDirection,
    pub distance: ExprField,
}

#[derive(Clone, Debug)]
pub struct SurfRevolveDialog {
    pub s: Session,
    pub sketch: Option<FeatureId>,
    pub curves: Vec<String>,
    pub axis: Option<RevolveAxis>,
    pub angle: ExprField,
}

#[derive(Clone, Debug)]
pub struct PatchDialog {
    pub s: Session,
    pub sketch: Option<FeatureId>,
    pub profiles: Vec<ProfileRef>,
    pub edges: Vec<EdgeRef>,
}

#[derive(Clone, Debug)]
pub struct StitchDialog {
    pub s: Session,
    pub bodies: Vec<BodyId>,
    pub tolerance: ExprField,
}

/// Offset (surface) and Thicken: faces plus a distance.
#[derive(Clone, Debug)]
pub struct FacesDialog {
    pub s: Session,
    pub faces: Vec<FaceRef>,
    /// Offset distance / thickness.
    pub value: ExprField,
    /// Thicken: symmetric (half the thickness on each side).
    pub symmetric: bool,
}

#[derive(Clone, Debug)]
pub struct MeshDialog {
    pub s: Session,
    /// File name of the inserted mesh (empty until a file is chosen).
    pub source: String,
    /// The STL bytes (until the feature exists).
    pub stl: Vec<u8>,
    pub to_brep: bool,
    /// Number of triangles of the chosen file.
    pub triangles: usize,
    /// A file chosen in the panel, read on the next update.
    pub file: Option<std::path::PathBuf>,
}

#[derive(Clone, Debug)]
pub struct PrintDialog {
    pub s: Session,
    /// Bodies to print; empty = every visible body.
    pub bodies: Vec<BodyId>,
    pub refinement: Refinement,
    pub ascii: bool,
    /// OK was pressed: waiting for the UI to deliver the STL file name.
    pub pending: bool,
}

/// One dialog of the SURFACE / MESH / UTILITIES tabs (a single `Dialog::Surface` variant).
#[derive(Clone, Debug)]
pub enum SurfaceDialog {
    Extrude(SurfExtrudeDialog),
    Revolve(SurfRevolveDialog),
    Patch(PatchDialog),
    Stitch(StitchDialog),
    Offset(FacesDialog),
    Thicken(FacesDialog),
    Mesh(MeshDialog),
    Print(PrintDialog),
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

impl SurfaceDialog {
    pub fn title(&self) -> &'static str {
        match self {
            SurfaceDialog::Extrude(_) => "EXTRUDE",
            SurfaceDialog::Revolve(_) => "REVOLVE",
            SurfaceDialog::Patch(_) => "PATCH",
            SurfaceDialog::Stitch(_) => "STITCH",
            SurfaceDialog::Offset(_) => "OFFSET",
            SurfaceDialog::Thicken(_) => "THICKEN",
            SurfaceDialog::Mesh(_) => "INSERT MESH",
            SurfaceDialog::Print(_) => "3D PRINT",
        }
    }
    pub fn session(&self) -> &Session {
        match self {
            SurfaceDialog::Extrude(d) => &d.s,
            SurfaceDialog::Revolve(d) => &d.s,
            SurfaceDialog::Patch(d) => &d.s,
            SurfaceDialog::Stitch(d) => &d.s,
            SurfaceDialog::Offset(d) | SurfaceDialog::Thicken(d) => &d.s,
            SurfaceDialog::Mesh(d) => &d.s,
            SurfaceDialog::Print(d) => &d.s,
        }
    }
    pub fn session_mut(&mut self) -> &mut Session {
        match self {
            SurfaceDialog::Extrude(d) => &mut d.s,
            SurfaceDialog::Revolve(d) => &mut d.s,
            SurfaceDialog::Patch(d) => &mut d.s,
            SurfaceDialog::Stitch(d) => &mut d.s,
            SurfaceDialog::Offset(d) | SurfaceDialog::Thicken(d) => &mut d.s,
            SurfaceDialog::Mesh(d) => &mut d.s,
            SurfaceDialog::Print(d) => &mut d.s,
        }
    }
    pub fn fields(&self) -> Vec<(&'static str, &ExprField)> {
        match self {
            SurfaceDialog::Extrude(d) => vec![("Distance", &d.distance)],
            SurfaceDialog::Revolve(d) => vec![("Angle", &d.angle)],
            SurfaceDialog::Stitch(d) => vec![("Tolerance", &d.tolerance)],
            SurfaceDialog::Offset(d) => vec![("Offset Distance", &d.value)],
            SurfaceDialog::Thicken(d) => vec![("Thickness", &d.value)],
            _ => vec![],
        }
    }
    pub fn primary_field_mut(&mut self) -> Option<&mut ExprField> {
        match self {
            SurfaceDialog::Extrude(d) => Some(&mut d.distance),
            SurfaceDialog::Revolve(d) => Some(&mut d.angle),
            SurfaceDialog::Stitch(d) => Some(&mut d.tolerance),
            SurfaceDialog::Offset(d) | SurfaceDialog::Thicken(d) => Some(&mut d.value),
            _ => None,
        }
    }
    pub fn input_count(&self) -> usize {
        match self {
            SurfaceDialog::Extrude(d) => d.curves.len(),
            SurfaceDialog::Revolve(d) => d.curves.len(),
            SurfaceDialog::Patch(d) => d.profiles.len() + d.edges.len(),
            SurfaceDialog::Stitch(d) => d.bodies.len(),
            SurfaceDialog::Offset(d) | SurfaceDialog::Thicken(d) => d.faces.len(),
            SurfaceDialog::Mesh(d) => (!d.source.is_empty()) as usize,
            SurfaceDialog::Print(d) => d.bodies.len(),
        }
    }
    pub fn missing_input_text(&self) -> Option<&'static str> {
        match self {
            SurfaceDialog::Extrude(d) if d.curves.is_empty() => Some("Select a sketch (click one of its profiles, or pick it in the timeline) and its curves."),
            SurfaceDialog::Revolve(d) if d.curves.is_empty() => Some("Select a sketch and the curves to revolve."),
            SurfaceDialog::Revolve(d) if d.axis.is_none() => Some("Select an axis."),
            SurfaceDialog::Patch(d) if d.profiles.is_empty() && d.edges.is_empty() => Some("Select closed sketch profiles or a closed loop of edges."),
            SurfaceDialog::Stitch(d) if d.bodies.is_empty() => Some("Select the surface bodies to stitch."),
            SurfaceDialog::Offset(d) | SurfaceDialog::Thicken(d) if d.faces.is_empty() => Some("Select faces in the canvas."),
            SurfaceDialog::Mesh(d) if d.source.is_empty() => Some("Select an STL file."),
            _ => None,
        }
    }
    /// The sketch the feature consumes (auto-hidden on OK, like Extrude).
    pub fn sketch(&self) -> Option<FeatureId> {
        match self {
            SurfaceDialog::Extrude(d) => d.sketch.clone(),
            SurfaceDialog::Revolve(d) => d.sketch.clone(),
            SurfaceDialog::Patch(d) if !d.profiles.is_empty() => d.sketch.clone(),
            _ => None,
        }
    }
    /// The "x" of the selection row.
    pub fn clear_inputs(&mut self) {
        match self {
            SurfaceDialog::Extrude(d) => d.curves.clear(),
            SurfaceDialog::Revolve(d) => d.curves.clear(),
            SurfaceDialog::Patch(d) => {
                d.profiles.clear();
                d.edges.clear();
            }
            SurfaceDialog::Stitch(d) => d.bodies.clear(),
            SurfaceDialog::Offset(d) | SurfaceDialog::Thicken(d) => d.faces.clear(),
            SurfaceDialog::Print(d) => d.bodies.clear(),
            SurfaceDialog::Mesh(_) => {}
        }
    }
}

/// (picks, mode) with mode 0 toggle, 1 add, 2 remove, 3 nothing.
fn route_picks(route: Route) -> (Vec<Pick>, u8) {
    match route {
        Route::Toggle(p) => (vec![p], 0),
        Route::Add(v) => (v, 1),
        Route::Remove(v) => (v, 2),
        Route::Nothing => (Vec::new(), 3),
    }
}

fn is_body_pick(p: &Pick) -> bool {
    matches!(p, Pick::Body { .. } | Pick::Face { .. } | Pick::Edge { .. })
}

/// Reads an STL file: (file name, bytes, triangle count).
fn load_stl(path: &Path) -> Result<(String, Vec<u8>, usize), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let n = cad_kernel::mesh::parse_stl(&bytes).map_err(|e| format!("{}: {e}", path.display()))?.len() / 9;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok((name, bytes, n))
}

/// The commands this module opens.
pub fn handles(id: CommandId) -> bool {
    use CommandId::*;
    matches!(id, SurfaceExtrude | SurfaceRevolve | Patch | Stitch | OffsetSurface | Thicken | InsertMesh | ThreeDPrint)
}

impl AppState {
    // ------------------------------------------------------------------ commands
    /// Opens the dialog of a SURFACE / MESH / UTILITIES command. Returns false for other commands.
    pub(crate) fn surface_command(&mut self, id: CommandId) -> bool {
        if !handles(id) {
            return false;
        }
        if matches!(self.mode, Mode::Sketch(_)) || self.in_txn() {
            self.info(format!("Finish the sketch before using {}.", spec(id).label));
            return true;
        }
        match id {
            CommandId::SurfaceExtrude => self.open_surface_extrude(None),
            CommandId::SurfaceRevolve => self.open_surface_revolve(None),
            CommandId::Patch => self.open_patch(None),
            CommandId::Stitch => self.open_stitch(None),
            CommandId::OffsetSurface => self.open_faces_dialog(false, None),
            CommandId::Thicken => self.open_faces_dialog(true, None),
            CommandId::InsertMesh => self.open_mesh(None),
            CommandId::ThreeDPrint => self.open_3d_print(),
            _ => {}
        }
        true
    }

    /// Timeline double-click on a surface / mesh feature.
    pub(crate) fn edit_surface_feature(&mut self, id: &str, kind: &FeatureKind) {
        let e = Some(id);
        match kind {
            FeatureKind::SurfaceExtrude { .. } => self.open_surface_extrude(e),
            FeatureKind::SurfaceRevolve { .. } => self.open_surface_revolve(e),
            FeatureKind::Patch { .. } => self.open_patch(e),
            FeatureKind::Stitch { .. } => self.open_stitch(e),
            FeatureKind::OffsetSurface { .. } => self.open_faces_dialog(false, e),
            FeatureKind::Thicken { .. } => self.open_faces_dialog(true, e),
            FeatureKind::InsertMesh { .. } => self.open_mesh(e),
            _ => {}
        }
    }

    fn show_sketch(&mut self, s: &mut Session, sk: &str) {
        if let Some(FeatureKind::Sketch { visible, .. }) = self.doc.feature_mut(sk).map(|f| &mut f.kind) {
            if !*visible {
                if s.shown_sketch.is_none() {
                    s.shown_sketch = Some((sk.to_string(), false));
                }
                *visible = true;
            }
        }
    }

    fn field_of_param(&self, fid: &str) -> ExprField {
        let name = self.doc.feature(fid).and_then(|f| f.parameter_names().first().map(|s| s.to_string())).unwrap_or_default();
        ExprField::new(&param_expr(&self.doc, &name))
    }

    fn set_param(&mut self, fid: &str, field: &mut ExprField) {
        let Some(name) = self.doc.feature(fid).and_then(|f| f.parameter_names().first().map(|s| s.to_string())) else { return };
        apply_expr(&mut self.doc, &name, field);
    }

    /// Curve chains of a sketch (base model first, then the displayed model).
    pub fn sketch_curve_chains(&self, base: &Model, sketch: &str) -> Vec<Chain> {
        base.sketches.get(sketch).or_else(|| self.model.sketches.get(sketch)).map(|sr| sketch_chains(&sr.solved)).unwrap_or_default()
    }

    /// Finished sketches that can feed Surface Extrude / Revolve: (id, name).
    pub fn surface_sketch_choices(&self, base: &Model) -> Vec<(FeatureId, String)> {
        self.doc
            .features
            .iter()
            .filter(|f| f.sketch().is_some() && base.sketches.contains_key(&f.id))
            .filter(|f| base.sketches[&f.id].solved.entities.iter().any(|e| e.is_curve() && !e.construction))
            .map(|f| (f.id.clone(), f.name.clone()))
            .collect()
    }

    /// The sketch picks of the current selection: (sketch, its profile indices).
    fn initial_sketch_pick(&self) -> Option<(FeatureId, Vec<usize>)> {
        for p in &self.selection {
            match p {
                Pick::Profile { sketch, .. } => {
                    let idx = self.selection.iter().filter_map(|q| if let Pick::Profile { sketch: s2, index } = q { (s2 == sketch).then_some(*index) } else { None }).collect();
                    return Some((sketch.clone(), idx));
                }
                Pick::Feature(f) if self.doc.feature(f).is_some_and(|f| f.sketch().is_some()) => return Some((f.clone(), Vec::new())),
                _ => {}
            }
        }
        None
    }

    // ------------------------------------------------------------------ opening
    pub(crate) fn open_surface_extrude(&mut self, editing: Option<&str>) {
        let initial = self.initial_sketch_pick();
        let mut s = self.open_session("Extrude", editing);
        let mut d = SurfExtrudeDialog { s: s.clone(), sketch: None, curves: Vec::new(), direction: ExtentDirection::Normal, distance: ExprField::new("10 mm") };
        match editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            Some(FeatureKind::SurfaceExtrude { sketch, curves, direction, .. }) => {
                self.show_sketch(&mut s, &sketch);
                d = SurfExtrudeDialog { distance: self.field_of_param(editing.unwrap_or_default()), s, sketch: Some(sketch), curves, direction };
            }
            _ => {
                if let Some((sk, profiles)) = initial {
                    let base = d.s.base.0.clone();
                    d.curves = self.curves_for(&base, &sk, &profiles);
                    d.sketch = Some(sk);
                }
            }
        }
        self.dialog = Some(Dialog::Surface(SurfaceDialog::Extrude(d)));
        self.dialog_changed();
    }

    pub(crate) fn open_surface_revolve(&mut self, editing: Option<&str>) {
        let initial = self.initial_sketch_pick();
        let mut s = self.open_session("Revolve", editing);
        let mut d = SurfRevolveDialog { s: s.clone(), sketch: None, curves: Vec::new(), axis: None, angle: ExprField::new("360 deg") };
        match editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            Some(FeatureKind::SurfaceRevolve { sketch, curves, axis, .. }) => {
                self.show_sketch(&mut s, &sketch);
                d = SurfRevolveDialog { angle: self.field_of_param(editing.unwrap_or_default()), s, sketch: Some(sketch), curves, axis: Some(axis) };
            }
            _ => {
                if let Some((sk, profiles)) = initial {
                    let base = d.s.base.0.clone();
                    d.curves = self.curves_for(&base, &sk, &profiles);
                    d.axis = self.default_axis(Some(&sk), &base);
                    d.sketch = Some(sk);
                }
            }
        }
        self.dialog = Some(Dialog::Surface(SurfaceDialog::Revolve(d)));
        self.dialog_changed();
    }

    pub(crate) fn open_patch(&mut self, editing: Option<&str>) {
        let initial: Vec<Pick> = self.selection.iter().filter(|p| matches!(p, Pick::Profile { .. } | Pick::Edge { .. })).cloned().collect();
        let mut s = self.open_session("Patch", editing);
        let mut d = PatchDialog { s: s.clone(), sketch: None, profiles: Vec::new(), edges: Vec::new() };
        match editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            Some(FeatureKind::Patch { sketch, profiles, edges }) => {
                if let Some(sk) = &sketch {
                    self.show_sketch(&mut s, sk);
                }
                d = PatchDialog { s, sketch, profiles, edges };
            }
            _ => {
                self.route_patch(&mut d, Route::Add(initial));
            }
        }
        self.dialog = Some(Dialog::Surface(SurfaceDialog::Patch(d)));
        self.dialog_changed();
    }

    pub(crate) fn open_stitch(&mut self, editing: Option<&str>) {
        let initial: Vec<Pick> = self.selection.iter().filter(|p| is_body_pick(p)).cloned().collect();
        let s = self.open_session("Stitch", editing);
        let mut d = StitchDialog { s, bodies: Vec::new(), tolerance: ExprField::new("0.01 mm") };
        match editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            Some(FeatureKind::Stitch { bodies, .. }) => {
                d.bodies = bodies;
                d.tolerance = self.field_of_param(editing.unwrap_or_default());
            }
            _ => {
                Self::route_stitch(&mut d, &initial, 1);
            }
        }
        self.dialog = Some(Dialog::Surface(SurfaceDialog::Stitch(d)));
        self.dialog_changed();
    }

    pub(crate) fn open_faces_dialog(&mut self, thicken: bool, editing: Option<&str>) {
        let initial: Vec<Pick> = self.selection.iter().filter(|p| matches!(p, Pick::Face { .. } | Pick::Body { .. })).cloned().collect();
        let s = self.open_session(if thicken { "Thicken" } else { "Offset" }, editing);
        let mut d = FacesDialog { s, faces: Vec::new(), value: ExprField::new(if thicken { "2 mm" } else { "5 mm" }), symmetric: false };
        match editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            Some(FeatureKind::Thicken { faces, symmetric, .. }) => {
                d.faces = faces;
                d.symmetric = symmetric;
                d.value = self.field_of_param(editing.unwrap_or_default());
            }
            Some(FeatureKind::OffsetSurface { faces, .. }) => {
                d.faces = faces;
                d.value = self.field_of_param(editing.unwrap_or_default());
            }
            _ => {
                self.route_faces(&mut d, Route::Add(initial));
            }
        }
        self.dialog = Some(Dialog::Surface(if thicken { SurfaceDialog::Thicken(d) } else { SurfaceDialog::Offset(d) }));
        self.dialog_changed();
    }

    pub(crate) fn open_mesh(&mut self, editing: Option<&str>) {
        let s = self.open_session("Insert Mesh", editing);
        let mut d = MeshDialog { s, source: String::new(), stl: Vec::new(), to_brep: false, triangles: 0, file: None };
        if let Some(FeatureKind::InsertMesh { source, data, to_brep }) = editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            d.source = source;
            d.to_brep = to_brep;
            d.triangles = ops::decode_base64(&data).map(|b| cad_kernel::mesh::stl_triangle_count(&b)).unwrap_or(0);
        }
        self.dialog = Some(Dialog::Surface(SurfaceDialog::Mesh(d)));
        self.dialog_changed();
    }

    /// INSERT ▸ Insert Mesh with a chosen file: reads the STL and opens (or fills) the Insert Mesh
    /// dialog, which previews the mesh body. Returns false when the file is not a readable STL.
    pub fn insert_mesh_file(&mut self, path: &Path) -> bool {
        let (name, bytes, triangles) = match load_stl(path) {
            Ok(v) => v,
            Err(e) => {
                self.error(e);
                return false;
            }
        };
        let fresh = !matches!(&self.dialog, Some(Dialog::Surface(SurfaceDialog::Mesh(m))) if m.s.feature.is_none());
        if fresh {
            if matches!(self.mode, Mode::Sketch(_)) {
                self.info("Finish the sketch before inserting a mesh.");
                return false;
            }
            if self.dialog.is_some() {
                self.dialog_cancel();
            }
            self.open_mesh(None);
        }
        self.dialog_edit(|d| {
            if let Dialog::Surface(SurfaceDialog::Mesh(m)) = d {
                m.source = name;
                m.stl = bytes;
                m.triangles = triangles;
            }
        });
        self.view_requests.push(crate::state::ViewRequest::Fit);
        true
    }

    pub(crate) fn open_3d_print(&mut self) {
        let initial: Vec<Pick> = self.selection.iter().filter(|p| is_body_pick(p)).cloned().collect();
        let s = self.open_session("3D Print", None);
        let mut d = PrintDialog { s, bodies: Vec::new(), refinement: Refinement::Medium, ascii: false, pending: false };
        Self::route_print(&mut d, &initial, 1);
        self.dialog = Some(Dialog::Surface(SurfaceDialog::Print(d)));
        self.dialog_changed();
    }

    // ------------------------------------------------------------------ routing
    /// Curves selected for a sketch pick: the chains bounding the given profiles, or every chain.
    fn curves_for(&self, base: &Model, sketch: &str, profiles: &[usize]) -> Vec<String> {
        let chains = self.sketch_curve_chains(base, sketch);
        let Some(sr) = base.sketches.get(sketch).or_else(|| self.model.sketches.get(sketch)) else { return Vec::new() };
        let mut out: Vec<String> = Vec::new();
        for c in &chains {
            let wanted = profiles.is_empty() || profiles.iter().filter_map(|i| sr.profiles.get(*i)).any(|p| p.curves.iter().any(|id| c.curves.contains(id)));
            if wanted {
                out.extend(c.curves.iter().cloned());
            }
        }
        out
    }

    /// Applies a sketch pick (profile or timeline sketch) to a curve input.
    fn route_curves(&self, s: &mut Session, sketch: &mut Option<FeatureId>, curves: &mut Vec<String>, route: Route) -> bool {
        let (picks, mode) = route_picks(route);
        let base = s.base.0.clone();
        let mut changed = false;
        for p in picks {
            let (sk, profile) = match &p {
                Pick::Profile { sketch, index } => (sketch.clone(), Some(*index)),
                Pick::Feature(f) if base.sketches.contains_key(f) => (f.clone(), None),
                _ => continue,
            };
            if sketch.as_deref() != Some(sk.as_str()) {
                if mode == 2 {
                    continue;
                }
                *sketch = Some(sk.clone());
                curves.clear();
                changed = true;
            }
            let wanted = self.curves_for(&base, &sk, &profile.into_iter().collect::<Vec<_>>());
            if wanted.is_empty() {
                continue;
            }
            let all_in = wanted.iter().all(|c| curves.contains(c));
            if all_in && (mode == 0 || mode == 2) && profile.is_some() {
                curves.retain(|c| !wanted.contains(c));
                changed = true;
            } else if !all_in && mode != 2 {
                for c in wanted {
                    if !curves.contains(&c) {
                        curves.push(c);
                    }
                }
                changed = true;
            }
        }
        if changed {
            s.notice = None;
        }
        changed
    }

    /// Toggles a curve chain from the dialog's chain list.
    pub fn surface_toggle_chain(&mut self, chain: usize) {
        let Some(Dialog::Surface(d)) = self.dialog.as_ref() else { return };
        let (base, sketch) = match d {
            SurfaceDialog::Extrude(x) => (x.s.base.0.clone(), x.sketch.clone()),
            SurfaceDialog::Revolve(x) => (x.s.base.0.clone(), x.sketch.clone()),
            _ => return,
        };
        let Some(sk) = sketch else { return };
        let Some(c) = self.sketch_curve_chains(&base, &sk).into_iter().nth(chain) else { return };
        self.dialog_edit(|d| {
            let curves = match d {
                Dialog::Surface(SurfaceDialog::Extrude(x)) => &mut x.curves,
                Dialog::Surface(SurfaceDialog::Revolve(x)) => &mut x.curves,
                _ => return,
            };
            if c.curves.iter().all(|id| curves.contains(id)) {
                curves.retain(|id| !c.curves.contains(id));
            } else {
                for id in &c.curves {
                    if !curves.contains(id) {
                        curves.push(id.clone());
                    }
                }
            }
        });
    }

    /// Chooses the sketch of a Surface Extrude / Revolve dialog (every chain selected).
    pub fn surface_set_sketch(&mut self, sketch: &str) {
        let Some(Dialog::Surface(d)) = self.dialog.as_ref() else { return };
        let base = d.session().base.0.clone();
        let curves = self.curves_for(&base, sketch, &[]);
        let axis = self.default_axis(Some(sketch), &base);
        let sk = sketch.to_string();
        self.dialog_edit(|d| match d {
            Dialog::Surface(SurfaceDialog::Extrude(x)) => {
                x.sketch = Some(sk);
                x.curves = curves;
            }
            Dialog::Surface(SurfaceDialog::Revolve(x)) => {
                x.sketch = Some(sk);
                x.curves = curves;
                x.axis = axis;
            }
            _ => {}
        });
    }

    fn route_patch(&self, x: &mut PatchDialog, route: Route) -> bool {
        let (picks, mode) = route_picks(route);
        let profile_picks: Vec<Pick> = picks.iter().filter(|p| matches!(p, Pick::Profile { .. })).cloned().collect();
        let sub = match mode {
            0 => profile_picks.first().cloned().map(Route::Toggle).unwrap_or(Route::Nothing),
            1 => Route::Add(profile_picks),
            2 => Route::Remove(profile_picks),
            _ => Route::Nothing,
        };
        let mut changed = self.route_profiles(&mut x.s, &mut x.sketch, &mut x.profiles, sub);
        let base = x.s.base.0.clone();
        for p in picks {
            let Pick::Edge { body, edge } = &p else { continue };
            let Some((b, j)) = self.base_edge(&base, body, *edge) else {
                if mode != 2 {
                    x.s.notice = Some("Select edges of an existing body (not of the preview).".into());
                }
                continue;
            };
            let at = x.edges.iter().position(|r| r.body == b.id && refs::resolve_edge(&b, r).ok() == Some(j));
            match (at, mode) {
                (Some(i), 0 | 2) => {
                    x.edges.remove(i);
                    changed = true;
                }
                (None, 0 | 1) => {
                    if let Some(r) = refs::edge_ref(&b, j) {
                        x.edges.push(r);
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        changed
    }

    fn route_stitch(x: &mut StitchDialog, picks: &[Pick], mode: u8) -> bool {
        let mut changed = false;
        for p in picks.iter().filter(|p| is_body_pick(p)) {
            let id = match p {
                Pick::Body { body } | Pick::Face { body, .. } | Pick::Edge { body, .. } => body.clone(),
                _ => continue,
            };
            let Some(b) = x.s.base.0.body(&id) else {
                if mode != 2 {
                    x.s.notice = Some("Select an existing surface body (not the preview).".into());
                }
                continue;
            };
            if b.kind != BodyKind::Surface && mode != 2 {
                x.s.notice = Some("Stitch joins surface bodies; this body is a solid or a mesh.".into());
                continue;
            }
            let at = x.bodies.iter().position(|q| *q == id);
            match (at, mode) {
                (Some(i), 0 | 2) => {
                    x.bodies.remove(i);
                    changed = true;
                }
                (None, 0 | 1) => {
                    x.bodies.push(id);
                    changed = true;
                }
                _ => {}
            }
        }
        if changed {
            x.s.notice = None;
        }
        changed
    }

    fn route_faces(&self, x: &mut FacesDialog, route: Route) -> bool {
        let (picks, mode) = route_picks(route);
        let base = x.s.base.0.clone();
        let mut changed = false;
        for p in picks {
            let targets: Vec<(cad_kernel::Body, usize)> = match &p {
                Pick::Face { body, face } => self.base_face(&base, body, *face).into_iter().collect(),
                Pick::Body { body } => base.body(body).map(|b| (0..b.topology().faces.len()).map(|i| (b.clone(), i)).collect()).unwrap_or_default(),
                _ => continue,
            };
            if targets.is_empty() {
                if mode != 2 {
                    x.s.notice = Some("Select faces of an existing body (not of the preview).".into());
                }
                continue;
            }
            if targets[0].0.kind == BodyKind::Mesh {
                if mode != 2 {
                    x.s.notice = Some("Mesh bodies have no B-rep faces; convert the mesh to B-rep first.".into());
                }
                continue;
            }
            // a body pick toggles all its faces together
            let present: Vec<Option<usize>> = targets.iter().map(|(b, j)| x.faces.iter().position(|r| r.body == b.id && refs::resolve_face(b, r).ok() == Some(*j))).collect();
            let all = present.iter().all(|p| p.is_some());
            if (mode == 0 && all) || mode == 2 {
                let mut idx: Vec<usize> = present.into_iter().flatten().collect();
                idx.sort_unstable();
                for i in idx.into_iter().rev() {
                    x.faces.remove(i);
                    changed = true;
                }
            } else if mode != 3 {
                for ((b, j), at) in targets.iter().zip(present) {
                    if at.is_none() {
                        if let Some(r) = refs::face_ref(b, *j) {
                            x.faces.push(r);
                            changed = true;
                        }
                    }
                }
            }
        }
        if changed {
            x.s.notice = None;
        }
        changed
    }

    fn route_print(x: &mut PrintDialog, picks: &[Pick], mode: u8) -> bool {
        let mut changed = false;
        for p in picks.iter().filter(|p| is_body_pick(p)) {
            let (Pick::Body { body } | Pick::Face { body, .. } | Pick::Edge { body, .. }) = p else { continue };
            if x.s.base.0.body(body).is_none() {
                continue;
            }
            let at = x.bodies.iter().position(|q| q == body);
            match (at, mode) {
                (Some(i), 0 | 2) => {
                    x.bodies.remove(i);
                    changed = true;
                }
                (None, 0 | 1) => {
                    x.bodies.push(body.clone());
                    changed = true;
                }
                _ => {}
            }
        }
        changed
    }

    /// Selection-change routing for this module's dialogs.
    pub(crate) fn route_surface(&self, d: &mut SurfaceDialog) -> bool {
        match d {
            SurfaceDialog::Extrude(x) => {
                let r = self.route(&x.s.synced, |p| matches!(p, Pick::Profile { .. } | Pick::Feature(_)));
                self.route_curves(&mut x.s, &mut x.sketch, &mut x.curves, r)
            }
            SurfaceDialog::Revolve(x) => {
                let r = self.route(&x.s.synced, |p| matches!(p, Pick::Profile { .. } | Pick::Feature(_)));
                let before = x.sketch.clone();
                let ch = self.route_curves(&mut x.s, &mut x.sketch, &mut x.curves, r);
                if x.sketch != before {
                    x.axis = self.default_axis(x.sketch.as_deref(), &x.s.base.0);
                }
                ch
            }
            SurfaceDialog::Patch(x) => {
                let r = self.route(&x.s.synced, |p| matches!(p, Pick::Profile { .. } | Pick::Edge { .. }));
                self.route_patch(x, r)
            }
            SurfaceDialog::Stitch(x) => {
                let (picks, mode) = route_picks(self.route(&x.s.synced, is_body_pick));
                Self::route_stitch(x, &picks, mode)
            }
            SurfaceDialog::Offset(x) | SurfaceDialog::Thicken(x) => {
                let r = self.route(&x.s.synced, |p| matches!(p, Pick::Face { .. } | Pick::Body { .. }));
                self.route_faces(x, r)
            }
            SurfaceDialog::Print(x) => {
                let (picks, mode) = route_picks(self.route(&x.s.synced, is_body_pick));
                Self::route_print(x, &picks, mode)
            }
            SurfaceDialog::Mesh(_) => false,
        }
    }

    /// Canvas highlight of this module's dialog inputs.
    pub(crate) fn surface_picks(&self, d: &SurfaceDialog) -> Vec<Pick> {
        let body_picks = |ids: &[BodyId]| -> Vec<Pick> { ids.iter().filter(|b| self.model.body(b).is_some()).map(|b| Pick::Body { body: b.clone() }).collect() };
        match d {
            SurfaceDialog::Extrude(x) => self.curve_picks(x.sketch.as_deref(), &x.curves),
            SurfaceDialog::Revolve(x) => self.curve_picks(x.sketch.as_deref(), &x.curves),
            SurfaceDialog::Patch(x) => {
                let mut v = self.profile_picks(x.sketch.as_deref(), &x.profiles);
                v.extend(x.edges.iter().filter_map(|r| self.edge_pick(&x.s.base.0, r)));
                v
            }
            SurfaceDialog::Stitch(x) => body_picks(&x.bodies),
            SurfaceDialog::Offset(x) | SurfaceDialog::Thicken(x) => x.faces.iter().filter_map(|r| self.face_pick(&x.s.base.0, r)).collect(),
            SurfaceDialog::Print(x) => body_picks(&x.bodies),
            SurfaceDialog::Mesh(_) => Vec::new(),
        }
    }

    /// Profiles of the sketch whose bounding curves are all selected (shown highlighted).
    fn curve_picks(&self, sketch: Option<&str>, curves: &[String]) -> Vec<Pick> {
        let Some(sk) = sketch else { return Vec::new() };
        let Some(sr) = self.model.sketches.get(sk) else { return Vec::new() };
        sr.profiles
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.curves.is_empty() && p.curves.iter().all(|c| curves.contains(c)))
            .map(|(i, _)| Pick::Profile { sketch: sk.to_string(), index: i })
            .collect()
    }

    // ------------------------------------------------------------------ updates
    /// Writes this module's dialog payload into the document and recomputes.
    pub(crate) fn update_surface(&mut self, d: &mut SurfaceDialog) {
        match d {
            SurfaceDialog::Extrude(x) => {
                if x.s.feature.is_none() {
                    let Some(sk) = x.sketch.clone() else { return };
                    if x.curves.is_empty() {
                        return;
                    }
                    x.s.feature = Some(ops::new_surface_extrude(&mut self.doc, &sk, x.curves.clone(), "10 mm", x.direction));
                }
                let fid = x.s.feature.clone().unwrap_or_default();
                self.set_param(&fid, &mut x.distance);
                if let Some(FeatureKind::SurfaceExtrude { sketch, curves, direction, .. }) = self.doc.feature_mut(&fid).map(|f| &mut f.kind) {
                    if let Some(sk) = &x.sketch {
                        *sketch = sk.clone();
                    }
                    *curves = x.curves.clone();
                    *direction = x.direction;
                }
                if let Some(sk) = x.sketch.clone() {
                    self.show_sketch(&mut x.s, &sk);
                }
            }
            SurfaceDialog::Revolve(x) => {
                if x.s.feature.is_none() {
                    let (Some(sk), Some(axis)) = (x.sketch.clone(), x.axis.clone()) else { return };
                    if x.curves.is_empty() {
                        return;
                    }
                    x.s.feature = Some(ops::new_surface_revolve(&mut self.doc, &sk, x.curves.clone(), axis, "360 deg"));
                }
                let fid = x.s.feature.clone().unwrap_or_default();
                self.set_param(&fid, &mut x.angle);
                if let Some(FeatureKind::SurfaceRevolve { sketch, curves, axis, .. }) = self.doc.feature_mut(&fid).map(|f| &mut f.kind) {
                    if let Some(sk) = &x.sketch {
                        *sketch = sk.clone();
                    }
                    *curves = x.curves.clone();
                    if let Some(a) = &x.axis {
                        *axis = a.clone();
                    }
                }
                if let Some(sk) = x.sketch.clone() {
                    self.show_sketch(&mut x.s, &sk);
                }
            }
            SurfaceDialog::Patch(x) => {
                if x.s.feature.is_none() {
                    if x.profiles.is_empty() && x.edges.is_empty() {
                        return;
                    }
                    x.s.feature = Some(ops::new_patch(&mut self.doc, x.sketch.clone(), x.profiles.clone(), x.edges.clone()));
                }
                let fid = x.s.feature.clone().unwrap_or_default();
                if let Some(FeatureKind::Patch { sketch, profiles, edges }) = self.doc.feature_mut(&fid).map(|f| &mut f.kind) {
                    *sketch = if x.profiles.is_empty() { None } else { x.sketch.clone() };
                    *profiles = x.profiles.clone();
                    *edges = x.edges.clone();
                }
            }
            SurfaceDialog::Stitch(x) => {
                if x.s.feature.is_none() {
                    if x.bodies.is_empty() {
                        return;
                    }
                    x.s.feature = Some(ops::new_stitch(&mut self.doc, x.bodies.clone(), "0.01 mm"));
                }
                let fid = x.s.feature.clone().unwrap_or_default();
                self.set_param(&fid, &mut x.tolerance);
                if let Some(FeatureKind::Stitch { bodies, .. }) = self.doc.feature_mut(&fid).map(|f| &mut f.kind) {
                    *bodies = x.bodies.clone();
                }
            }
            SurfaceDialog::Offset(x) => {
                if x.s.feature.is_none() {
                    if x.faces.is_empty() {
                        return;
                    }
                    x.s.feature = Some(ops::new_offset_surface(&mut self.doc, x.faces.clone(), "5 mm"));
                }
                let fid = x.s.feature.clone().unwrap_or_default();
                self.set_param(&fid, &mut x.value);
                if let Some(FeatureKind::OffsetSurface { faces, .. }) = self.doc.feature_mut(&fid).map(|f| &mut f.kind) {
                    *faces = x.faces.clone();
                }
            }
            SurfaceDialog::Thicken(x) => {
                if x.s.feature.is_none() {
                    if x.faces.is_empty() {
                        return;
                    }
                    x.s.feature = Some(ops::new_thicken(&mut self.doc, x.faces.clone(), "2 mm", x.symmetric));
                }
                let fid = x.s.feature.clone().unwrap_or_default();
                self.set_param(&fid, &mut x.value);
                if let Some(FeatureKind::Thicken { faces, symmetric, .. }) = self.doc.feature_mut(&fid).map(|f| &mut f.kind) {
                    *faces = x.faces.clone();
                    *symmetric = x.symmetric;
                }
            }
            SurfaceDialog::Mesh(x) => {
                if let Some(p) = x.file.take() {
                    match load_stl(&p) {
                        Ok((name, bytes, n)) => {
                            x.source = name;
                            x.stl = bytes;
                            x.triangles = n;
                            x.s.notice = None;
                        }
                        Err(e) => x.s.notice = Some(e),
                    }
                }
                if x.s.feature.is_none() {
                    if x.stl.is_empty() {
                        return;
                    }
                    x.s.feature = Some(ops::new_insert_mesh(&mut self.doc, &x.source, &x.stl, x.to_brep));
                    x.stl.clear();
                }
                let fid = x.s.feature.clone().unwrap_or_default();
                if let Some(FeatureKind::InsertMesh { to_brep, .. }) = self.doc.feature_mut(&fid).map(|f| &mut f.kind) {
                    *to_brep = x.to_brep;
                }
            }
            SurfaceDialog::Print(_) => return,
        }
        self.recompute();
    }

    // ------------------------------------------------------------------ status / OK
    pub(crate) fn surface_status(&self, d: &SurfaceDialog) -> Option<String> {
        match d {
            SurfaceDialog::Extrude(SurfExtrudeDialog { sketch: Some(sk), curves, s, .. }) | SurfaceDialog::Revolve(SurfRevolveDialog { sketch: Some(sk), curves, s, .. }) if !curves.is_empty() => {
                let chains = self.sketch_curve_chains(&s.base.0, sk);
                let sel: Vec<&Chain> = chains.iter().filter(|c| c.curves.iter().any(|id| curves.contains(id))).collect();
                let len: f64 = sel.iter().map(|c| c.length()).sum();
                Some(format!("{} | {} | Length : {len:.2} mm", plural(curves.len(), "Curve"), plural(sel.len(), "Chain")))
            }
            SurfaceDialog::Patch(x) if !x.profiles.is_empty() || !x.edges.is_empty() => {
                let mut parts = Vec::new();
                if !x.profiles.is_empty() {
                    parts.push(plural(x.profiles.len(), "Profile"));
                }
                if !x.edges.is_empty() {
                    parts.push(plural(x.edges.len(), "Edge"));
                }
                Some(parts.join(" | "))
            }
            SurfaceDialog::Stitch(x) if !x.bodies.is_empty() => Some(format!("{} {}", x.bodies.len(), if x.bodies.len() == 1 { "Body" } else { "Bodies" })),
            SurfaceDialog::Offset(x) | SurfaceDialog::Thicken(x) if !x.faces.is_empty() => Some(plural(x.faces.len(), "Face")),
            SurfaceDialog::Mesh(x) if !x.source.is_empty() => Some(format!("{} | {}", x.source, plural(x.triangles, "Triangle"))),
            SurfaceDialog::Print(x) => {
                let n = self.print_bodies(x).len();
                Some(if x.pending { "Choose where to save the STL file...".to_string() } else { format!("{} to export", if n == 1 { "1 Body".into() } else { format!("{n} Bodies") }) })
            }
            _ => None,
        }
    }

    fn print_bodies(&self, x: &PrintDialog) -> Vec<&cad_kernel::Body> {
        if x.bodies.is_empty() {
            self.visible_bodies()
        } else {
            x.bodies.iter().filter_map(|b| self.model.body(b)).collect()
        }
    }

    /// OK of this module's dialogs that do more than commit the feature (3D Print). Returns true
    /// when handled.
    pub(crate) fn surface_dialog_ok(&mut self) -> bool {
        let Some(Dialog::Surface(SurfaceDialog::Print(x))) = self.dialog.as_ref() else { return false };
        if self.print_bodies(x).is_empty() {
            self.error("There are no bodies to print.");
            return true;
        }
        if let Some(Dialog::Surface(SurfaceDialog::Print(x))) = self.dialog.as_mut() {
            x.pending = true;
        }
        self.file_request = Some(FileAction::ExportStl);
        true
    }

    /// Writes the 3D Print dialog's bodies to `path` with its refinement, then closes the dialog.
    pub fn export_3d_print(&mut self, path: &Path) -> bool {
        let Some(Dialog::Surface(SurfaceDialog::Print(x))) = self.dialog.as_ref() else { return self.export_stl(path) };
        let (refinement, ascii) = (x.refinement, x.ascii);
        let bodies = self.print_bodies(x);
        let n = bodies.len();
        let r = cad_kernel::mesh::export_stl_refined(&bodies, &path.to_string_lossy(), ascii, refinement);
        match r {
            Ok(()) => {
                self.dialog_cancel();
                self.info(format!("3D Print: exported {n} {} to {} ({} refinement)", if n == 1 { "body" } else { "bodies" }, path.display(), refinement.label()));
                true
            }
            Err(e) => {
                self.error(e.0);
                false
            }
        }
    }

    /// Completes a `FileAction::ExportStl` request: the 3D Print export when its dialog is
    /// waiting for the file name, the plain STL export otherwise.
    pub fn export_stl_requested(&mut self, path: &Path) -> bool {
        if matches!(&self.dialog, Some(Dialog::Surface(SurfaceDialog::Print(x))) if x.pending) {
            self.export_3d_print(path)
        } else {
            self.export_stl(path)
        }
    }

    /// The STL file dialog was cancelled: a waiting 3D Print dialog becomes editable again.
    pub fn export_stl_cancelled(&mut self) {
        if let Some(Dialog::Surface(SurfaceDialog::Print(x))) = self.dialog.as_mut() {
            x.pending = false;
        }
    }
}
