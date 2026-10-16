//! Feature commands and their dialogs (Extrude, Revolve, Fillet, Chamfer, Hole, Offset Plane,
//! Change Parameters, Measure, Appearance).
//!
//! INTERFACE CONTRACT (used by app.rs / ui): `Dialog`, `feature_command`, `edit_feature`,
//! `dialog_ok`, `dialog_cancel`, `on_selection_changed`. Dialog payload types may grow freely.
//!
//! How a feature dialog works (Fusion's command-dialog model):
//! * Opening a feature dialog starts a transaction. The feature is inserted into (or, when
//!   editing, modified in) `state.doc` and the model recomputes after every change, so the canvas
//!   shows the real result as a live preview. OK commits one undo step ("Extrude",
//!   "Edit Extrude1"); Cancel restores the document as it was.
//! * The dialog payload is the source of truth for its inputs. Inputs are stored as persistent
//!   references (`ProfileRef`, `EdgeRef`, `FaceRef`, `PlaneRef`) resolved against the *base
//!   model* — the model before the feature — because the canvas shows the preview, whose
//!   topology differs (a filleted edge no longer exists in the preview).
//! * Canvas clicks while a dialog is open toggle inputs (Fusion selection inputs do not need
//!   Ctrl). `state.selection` mirrors the inputs so the canvas highlights them.
//! * Every change goes through [`AppState::dialog_changed`], which writes the payload into the
//!   document and recomputes. The UI and the tests use the same path.

use super::{AppState, Mode, Pick};
use crate::state::command::{spec, CommandId};
use cad_core::{expr, Document, EdgeRef, ExtentDirection, FaceRef, FeatureId, FeatureKind, Operation, PlaneRef, ProfileRef, Quantity, RevolveAxis};
use cad_kernel::{Health, Model};

#[path = "features_edges.rs"]
mod edges;
#[path = "features_profiles.rs"]
mod profiles;
#[cfg(test)]
#[path = "features_inspect_tests.rs"]
mod inspect_tests;
#[path = "features_inspect.rs"]
mod inspect;
#[path = "features_tools.rs"]
mod tools;
#[path = "features_assembly.rs"]
mod assembly;
#[path = "features_more.rs"]
mod more;
#[path = "features_surface.rs"]
mod surface;
#[cfg(test)]
#[path = "features_surface_tests.rs"]
mod surface_tests;
#[path = "features_manip.rs"] mod manip;

#[cfg(test)]
#[path = "features_assembly_tests.rs"]
mod assembly_tests;

#[cfg(test)]
#[path = "features_more_tests.rs"]
mod more_tests;

#[cfg(test)]
#[path = "features_tests.rs"]
mod tests;

pub use assembly::JointDialog;
pub use profiles::axis_label;
#[allow(unused_imports)]
pub use tools::{quantity_unit, MeasureReport, Measurement, APPEARANCES};
#[allow(unused_imports)]
pub use inspect::{InspectDialog, InspectKind, InspectMarker, InterferenceRow, INTERFERENCE_TOLERANCE};
#[allow(unused_imports)]
#[allow(unused_imports)]
pub use surface::{FacesDialog, MeshDialog, PatchDialog, PrintDialog, StitchDialog, SurfExtrudeDialog, SurfRevolveDialog, SurfaceDialog};
#[allow(unused_imports)]
pub use more::{BodyInput, CircPatternDialog, CombineDialog, MirrorDialog, MoveDialog, RectPatternDialog, ShellDialog, COMBINE_OPERATIONS};

/// The active command dialog (Fusion shows these as a panel at the right of the canvas).
#[derive(Clone, Debug)]
pub enum Dialog {
    Extrude(ExtrudeDialog),
    Revolve(RevolveDialog),
    Fillet(EdgeDialog),
    Chamfer(EdgeDialog),
    Hole(HoleDialog),
    OffsetPlane(PlaneDialog),
    Parameters(ParametersDialog),
    Measure(MeasureDialog),
    /// INSPECT: Interference, Center of Mass (features_inspect.rs).
    Inspect(InspectDialog),
    Appearance(AppearanceDialog),
    Shell(ShellDialog),
    Combine(CombineDialog),
    Mirror(MirrorDialog),
    RectangularPattern(RectPatternDialog),
    CircularPattern(CircPatternDialog),
    Move(MoveDialog),
    Joint(JointDialog),
    /// SURFACE / MESH / UTILITIES dialogs (features_surface.rs).
    Surface(SurfaceDialog),
    /// A message-only panel.
    #[allow(dead_code)]
    Info(String),
}

/// Model snapshot (the model before the feature); bodies are shared `Arc`s, so cloning is cheap.
#[derive(Clone, Default)]
pub struct BaseModel(pub Model);

impl std::fmt::Debug for BaseModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BaseModel({} bodies)", self.0.bodies.len())
    }
}

/// An expression text box bound to a parameter. Invalid text stays in the box with an error;
/// the parameter keeps its last valid expression (Fusion: "Expression is invalid.").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExprField {
    pub text: String,
    pub error: Option<String>,
}

impl ExprField {
    pub fn new(text: &str) -> Self {
        ExprField { text: text.into(), error: None }
    }
}

/// State shared by every feature dialog (a transaction-owning, previewing command).
#[derive(Clone, Debug)]
pub struct Session {
    /// Feature being edited (timeline double-click); `None` while creating.
    pub editing: Option<FeatureId>,
    /// Feature currently in the document for the preview (created lazily once inputs exist).
    pub feature: Option<FeatureId>,
    /// Model before the feature: inputs are resolved against it.
    pub base: BaseModel,
    /// Timeline marker to restore when an edit is committed.
    pub saved_marker: usize,
    /// The selection as this dialog last mirrored it (to tell clicks apart).
    pub synced: Vec<Pick>,
    /// Hint shown in the dialog ("Select a planar face", ...).
    pub notice: Option<String>,
    /// Health of the previewed feature after the last recompute.
    pub health: Health,
    /// Sketch shown during an edit, with its visibility before the edit.
    pub shown_sketch: Option<(FeatureId, bool)>,
}

#[derive(Clone, Debug)]
pub struct ExtrudeDialog {
    pub s: Session,
    pub sketch: Option<FeatureId>,
    pub profiles: Vec<ProfileRef>,
    pub direction: ExtentDirection,
    /// Extent type "All" (Through All).
    pub through_all: bool,
    pub distance: ExprField,
    pub operation: Operation,
}

#[derive(Clone, Debug)]
pub struct RevolveDialog {
    pub s: Session,
    pub sketch: Option<FeatureId>,
    pub profiles: Vec<ProfileRef>,
    pub axis: Option<RevolveAxis>,
    pub angle: ExprField,
    pub operation: Operation,
}

/// Fillet or Chamfer.
#[derive(Clone, Debug)]
pub struct EdgeDialog {
    pub s: Session,
    pub edges: Vec<EdgeRef>,
    /// Radius (fillet) or distance (chamfer).
    pub value: ExprField,
}

#[derive(Clone, Debug)]
pub struct HoleDialog {
    pub s: Session,
    pub face: Option<FaceRef>,
    /// Face centroid in the face frame (the default hole centre).
    pub centroid: [f64; 2],
    pub offset_x: ExprField,
    pub offset_y: ExprField,
    pub diameter: ExprField,
    pub depth: ExprField,
    pub through_all: bool,
}

#[derive(Clone, Debug)]
pub struct PlaneDialog {
    pub s: Session,
    pub base: Option<PlaneRef>,
    pub offset: ExprField,
}

#[derive(Clone, Debug)]
pub struct ParametersDialog {
    /// Expression text being edited, per parameter name.
    pub edits: std::collections::BTreeMap<String, String>,
    /// Error of the last rejected edit, per parameter name.
    pub errors: std::collections::BTreeMap<String, String>,
    pub new_name: String,
    pub new_expression: String,
    pub new_quantity: Quantity,
    pub new_comment: String,
    pub new_error: Option<String>,
}

impl Default for ParametersDialog {
    fn default() -> Self {
        ParametersDialog {
            edits: Default::default(),
            errors: Default::default(),
            new_name: String::new(),
            new_expression: String::new(),
            new_quantity: Quantity::Length,
            new_comment: String::new(),
            new_error: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MeasureDialog {
    /// Up to two picked items (faces, edges, bodies, profiles).
    pub picks: Vec<Pick>,
    pub synced: Vec<Pick>,
}

#[derive(Clone, Debug, Default)]
pub struct AppearanceDialog {
    pub status: Option<String>,
}

impl Dialog {
    /// Header text of the dialog panel (Fusion: command name in capitals; "EDIT FEATURE" when
    /// a timeline feature is edited).
    pub fn title(&self) -> String {
        if self.session().is_some_and(|s| s.editing.is_some()) {
            return "EDIT FEATURE".into();
        }
        match self {
            Dialog::Extrude(_) => "EXTRUDE",
            Dialog::Revolve(_) => "REVOLVE",
            Dialog::Fillet(_) => "FILLET",
            Dialog::Chamfer(_) => "CHAMFER",
            Dialog::Hole(_) => "HOLE",
            Dialog::OffsetPlane(_) => "OFFSET PLANE",
            Dialog::Parameters(_) => "PARAMETERS",
            Dialog::Measure(_) => "MEASURE",
            Dialog::Inspect(x) => x.title(),
            Dialog::Appearance(_) => "APPEARANCE",
            Dialog::Shell(_) => "SHELL",
            Dialog::Combine(_) => "COMBINE",
            Dialog::Mirror(_) => "MIRROR",
            Dialog::RectangularPattern(_) => "RECTANGULAR PATTERN",
            Dialog::CircularPattern(_) => "CIRCULAR PATTERN",
            Dialog::Move(_) => "MOVE/COPY",
            Dialog::Joint(_) => "JOINT",
            Dialog::Surface(x) => x.title(),
            Dialog::Info(_) => "INFO",
        }
        .into()
    }

    pub fn session(&self) -> Option<&Session> {
        match self {
            Dialog::Extrude(d) => Some(&d.s),
            Dialog::Revolve(d) => Some(&d.s),
            Dialog::Fillet(d) | Dialog::Chamfer(d) => Some(&d.s),
            Dialog::Hole(d) => Some(&d.s),
            Dialog::OffsetPlane(d) => Some(&d.s),
            Dialog::Shell(d) => Some(&d.s),
            Dialog::Combine(d) => Some(&d.s),
            Dialog::Mirror(d) => Some(&d.s),
            Dialog::RectangularPattern(d) => Some(&d.s),
            Dialog::CircularPattern(d) => Some(&d.s),
            Dialog::Move(d) => Some(&d.s),
            Dialog::Joint(d) => Some(&d.s),
            Dialog::Surface(d) => Some(d.session()),
            _ => None,
        }
    }
    pub fn session_mut(&mut self) -> Option<&mut Session> {
        match self {
            Dialog::Extrude(d) => Some(&mut d.s),
            Dialog::Revolve(d) => Some(&mut d.s),
            Dialog::Fillet(d) | Dialog::Chamfer(d) => Some(&mut d.s),
            Dialog::Hole(d) => Some(&mut d.s),
            Dialog::OffsetPlane(d) => Some(&mut d.s),
            Dialog::Shell(d) => Some(&mut d.s),
            Dialog::Combine(d) => Some(&mut d.s),
            Dialog::Mirror(d) => Some(&mut d.s),
            Dialog::RectangularPattern(d) => Some(&mut d.s),
            Dialog::CircularPattern(d) => Some(&mut d.s),
            Dialog::Move(d) => Some(&mut d.s),
            Dialog::Joint(d) => Some(&mut d.s),
            Dialog::Surface(d) => Some(d.session_mut()),
            _ => None,
        }
    }

    /// Feature dialogs own a transaction; tool dialogs (Parameters, Measure, Appearance) do not.
    pub fn owns_txn(&self) -> bool {
        self.session().is_some()
    }

    /// Expression fields of the dialog (for validation).
    fn fields(&self) -> Vec<(&'static str, &ExprField)> {
        match self {
            Dialog::Extrude(d) if !d.through_all => vec![("Distance", &d.distance)],
            Dialog::Revolve(d) => vec![("Angle", &d.angle)],
            Dialog::Fillet(d) => vec![("Radius", &d.value)],
            Dialog::Chamfer(d) => vec![("Distance", &d.value)],
            Dialog::Hole(d) => {
                let mut v = vec![("X Offset", &d.offset_x), ("Y Offset", &d.offset_y), ("Diameter", &d.diameter)];
                if !d.through_all {
                    v.push(("Depth", &d.depth));
                }
                v
            }
            Dialog::OffsetPlane(d) => vec![("Distance", &d.offset)],
            Dialog::Shell(d) => vec![("Inside Thickness", &d.thickness)],
            Dialog::RectangularPattern(d) => {
                let mut v = vec![("Quantity", &d.quantity1), ("Distance", &d.distance1)];
                if d.axis2.is_some() {
                    v.extend([("Quantity (Direction 2)", &d.quantity2), ("Distance (Direction 2)", &d.distance2)]);
                }
                v
            }
            Dialog::CircularPattern(d) => vec![("Quantity", &d.quantity), ("Total Angle", &d.angle)],
            Dialog::Move(d) => vec![("X Distance", &d.dx), ("Y Distance", &d.dy), ("Z Distance", &d.dz), ("Angle", &d.angle)],
            Dialog::Joint(d) => vec![("Angle", &d.angle), ("Offset", &d.offset)],
            Dialog::Surface(d) => d.fields(),
            _ => vec![],
        }
    }

    /// The primary value field (distance, angle, radius, diameter, offset).
    #[allow(dead_code)]
    pub fn primary_field_mut(&mut self) -> Option<&mut ExprField> {
        match self {
            Dialog::Extrude(d) => Some(&mut d.distance),
            Dialog::Revolve(d) => Some(&mut d.angle),
            Dialog::Fillet(d) | Dialog::Chamfer(d) => Some(&mut d.value),
            Dialog::Hole(d) => Some(&mut d.diameter),
            Dialog::OffsetPlane(d) => Some(&mut d.offset),
            Dialog::Shell(d) => Some(&mut d.thickness),
            Dialog::RectangularPattern(d) => Some(&mut d.quantity1),
            Dialog::CircularPattern(d) => Some(&mut d.quantity),
            Dialog::Move(d) => Some(&mut d.dx),
            Dialog::Joint(d) => Some(if d.kind == cad_core::JointKind::Slider { &mut d.offset } else { &mut d.angle }),
            Dialog::Surface(d) => d.primary_field_mut(),
            _ => None,
        }
    }

    /// Number of selected inputs and their noun ("Profile", "Edge", ...).
    #[allow(dead_code)]
    pub fn input_count(&self) -> usize {
        match self {
            Dialog::Extrude(d) => d.profiles.len(),
            Dialog::Revolve(d) => d.profiles.len(),
            Dialog::Fillet(d) | Dialog::Chamfer(d) => d.edges.len(),
            Dialog::Hole(d) => d.face.is_some() as usize,
            Dialog::OffsetPlane(d) => d.base.is_some() as usize,
            Dialog::Measure(d) => d.picks.len(),
            Dialog::Shell(d) => d.faces.len(),
            Dialog::Combine(d) => d.target.is_some() as usize + d.tools.len(),
            Dialog::Mirror(d) => d.bodies.len(),
            Dialog::RectangularPattern(d) => d.bodies.len(),
            Dialog::CircularPattern(d) => d.bodies.len(),
            Dialog::Move(d) => d.bodies.len(),
            Dialog::Joint(d) => d.a.is_some() as usize + d.b.is_some() as usize,
            Dialog::Surface(d) => d.input_count(),
            _ => 0,
        }
    }

    fn missing_input_text(&self) -> Option<&'static str> {
        match self {
            Dialog::Extrude(d) if d.profiles.is_empty() => Some("Select profiles in the canvas."),
            Dialog::Revolve(d) if d.profiles.is_empty() => Some("Select profiles in the canvas."),
            Dialog::Revolve(d) if d.axis.is_none() => Some("Select an axis."),
            Dialog::Fillet(d) | Dialog::Chamfer(d) if d.edges.is_empty() => Some("Select edges in the canvas."),
            Dialog::Hole(d) if d.face.is_none() => Some("Select a planar face in the canvas."),
            Dialog::OffsetPlane(d) if d.base.is_none() => Some("Select a plane or planar face."),
            Dialog::Shell(d) if d.faces.is_empty() => Some("Select the faces to remove in the canvas."),
            Dialog::Combine(d) if d.target.is_none() => Some("Select a target body."),
            Dialog::Combine(d) if d.tools.is_empty() => Some("Select tool bodies."),
            Dialog::Mirror(d) if d.bodies.is_empty() => Some("Select the bodies to mirror."),
            Dialog::Mirror(d) if d.plane.is_none() => Some("Select a mirror plane."),
            Dialog::RectangularPattern(d) if d.bodies.is_empty() => Some("Select the bodies to pattern."),
            Dialog::CircularPattern(d) if d.bodies.is_empty() => Some("Select the bodies to pattern."),
            Dialog::Move(d) if d.bodies.is_empty() => Some("Select the bodies to move."),
            Dialog::Joint(d) if d.a.is_none() => Some("Select a planar face or circular edge on Component 1."),
            Dialog::Joint(d) if d.b.is_none() => Some("Select a planar face or circular edge on Component 2."),
            Dialog::Surface(d) => d.missing_input_text(),
            _ => None,
        }
    }
}

/// Validates `text` as the expression of parameter `param` and stores it (keeps the previous
/// expression and records the error otherwise).
pub(crate) fn apply_expr(doc: &mut Document, param: &str, field: &mut ExprField) {
    match doc.set_expression(param, &field.text) {
        Ok(_) => field.error = None,
        Err(e) => field.error = Some(e.to_string()),
    }
}

/// Evaluates a free expression (not stored in a parameter) against the document parameters.
pub(crate) fn eval_expr(doc: &Document, text: &str, q: Quantity) -> Result<f64, String> {
    let pv = doc.parameter_values();
    let mut resolve = |name: &str| -> Result<Option<(f64, Quantity)>, expr::ExprError> {
        Ok(doc.parameter(name).and_then(|p| pv.values.get(name).map(|v| (*v, p.quantity))))
    };
    expr::evaluate(text, q, &mut resolve).map(|e| e.value).map_err(|e| e.0)
}

/// Current expression of a parameter.
pub(crate) fn param_expr(doc: &Document, name: &str) -> String {
    doc.parameter(name).map(|p| p.expression.clone()).unwrap_or_default()
}

/// How a selection change should modify a dialog's inputs.
pub(crate) enum Route {
    /// A canvas click on this item: toggle it.
    Toggle(Pick),
    Add(Vec<Pick>),
    Remove(Vec<Pick>),
    Nothing,
}

impl AppState {
    // ------------------------------------------------------------------ opening
    pub(crate) fn feature_command(&mut self, id: CommandId) {
        // Fusion: starting a command ends the one that is running
        if self.dialog.is_some() {
            self.dialog_cancel();
        }
        if self.surface_command(id) {
            return;
        }
        let sketching = matches!(self.mode, Mode::Sketch(_));
        let owns = matches!(
            id,
            CommandId::Extrude
                | CommandId::Revolve
                | CommandId::Fillet
                | CommandId::Chamfer
                | CommandId::Hole
                | CommandId::OffsetPlane
                | CommandId::Shell
                | CommandId::Combine
                | CommandId::MirrorFeature
                | CommandId::RectangularPattern
                | CommandId::CircularPattern
                | CommandId::Move
                | CommandId::Joint
        );
        if owns && (sketching || self.in_txn()) {
            self.info(format!("Finish the sketch before using {}.", spec(id).label));
            return;
        }
        match id {
            CommandId::Extrude => self.open_extrude(None),
            CommandId::Revolve => self.open_revolve(None),
            CommandId::Fillet => self.open_edge_dialog(false, None),
            CommandId::Chamfer => self.open_edge_dialog(true, None),
            CommandId::Hole => self.open_hole(None),
            CommandId::OffsetPlane => self.open_plane(None),
            CommandId::ChangeParameters => self.open_parameters(),
            CommandId::Measure => self.open_measure(),
            CommandId::Interference | CommandId::CenterOfMass => self.open_inspect(id),
            CommandId::Appearance => self.open_appearance(),
            CommandId::Shell => self.open_shell(None),
            CommandId::Combine => self.open_combine(None),
            CommandId::MirrorFeature => self.open_mirror(None),
            CommandId::RectangularPattern => self.open_rect_pattern(None),
            CommandId::CircularPattern => self.open_circ_pattern(None),
            CommandId::Move => self.open_move(None),
            CommandId::Joint => self.open_joint(None),
            CommandId::NewComponent => {
                if sketching || self.in_txn() {
                    self.info("Finish the sketch before creating a component.");
                } else {
                    self.new_component_command();
                }
            }
            _ => {
                let s = spec(id);
                self.warn(format!("{}: {}", s.label, crate::state::command::NOT_SUPPORTED));
            }
        }
    }

    /// Timeline double-click: sketches open in sketch mode; other features reopen their dialog.
    pub fn edit_feature(&mut self, id: &str) {
        if self.doc.feature(id).and_then(|f| f.sketch()).is_some() {
            if self.dialog.is_some() {
                self.dialog_cancel();
            }
            self.edit_sketch(id);
            return;
        }
        let Some(f) = self.doc.feature(id) else { return };
        let kind = f.kind.clone();
        if self.dialog.is_some() {
            self.dialog_cancel();
        }
        if matches!(self.mode, Mode::Sketch(_)) || self.in_txn() {
            self.info("Finish the sketch before editing a feature.");
            return;
        }
        let id = Some(id);
        match kind {
            FeatureKind::Extrude { .. } => self.open_extrude(id),
            FeatureKind::Revolve { .. } => self.open_revolve(id),
            FeatureKind::Fillet { .. } => self.open_edge_dialog(false, id),
            FeatureKind::Chamfer { .. } => self.open_edge_dialog(true, id),
            FeatureKind::Hole { .. } => self.open_hole(id),
            FeatureKind::Plane { .. } => self.open_plane(id),
            FeatureKind::Import { .. } => self.info("Imported STEP geometry is a base feature and has no editable parameters."),
            FeatureKind::Sketch { .. } => {}
            FeatureKind::Shell { .. } => self.open_shell(id),
            FeatureKind::Combine { .. } => self.open_combine(id),
            FeatureKind::MirrorBodies { .. } => self.open_mirror(id),
            FeatureKind::RectangularPattern { .. } => self.open_rect_pattern(id),
            FeatureKind::CircularPattern { .. } => self.open_circ_pattern(id),
            FeatureKind::MoveBodies { .. } => self.open_move(id),
            FeatureKind::Joint { .. } => self.open_joint(id),
            k @ (FeatureKind::SurfaceExtrude { .. }
            | FeatureKind::SurfaceRevolve { .. }
            | FeatureKind::Patch { .. }
            | FeatureKind::Stitch { .. }
            | FeatureKind::OffsetSurface { .. }
            | FeatureKind::Thicken { .. }
            | FeatureKind::InsertMesh { .. }) => self.edit_surface_feature(id.unwrap_or_default(), &k),
        }
    }

    /// Starts a previewing session: opens the transaction and captures the base model. When
    /// editing, the timeline is rolled to just after the feature (downstream features are
    /// rolled back while the dialog is open, as in Fusion).
    pub(crate) fn open_session(&mut self, type_label: &str, editing: Option<&str>) -> Session {
        let saved_marker = self.doc.marker;
        let (label, base, feature, shown_sketch) = match editing.and_then(|id| self.doc.feature_index(id).map(|i| (id, i))) {
            Some((id, i)) => {
                let name = self.doc.features[i].name.clone();
                self.begin_txn(&format!("Edit {name}"));
                self.doc.marker = i;
                self.recompute();
                let base = self.model.clone();
                self.doc.marker = i + 1;
                // show the profiles' sketch while its feature is edited
                let mut shown = None;
                let sk = match &self.doc.features[i].kind {
                    FeatureKind::Extrude { sketch, .. } | FeatureKind::Revolve { sketch, .. } => Some(sketch.clone()),
                    _ => None,
                };
                if let Some(sk) = sk {
                    if let Some(FeatureKind::Sketch { visible, .. }) = self.doc.feature_mut(&sk).map(|f| &mut f.kind) {
                        shown = Some((sk.clone(), *visible));
                        *visible = true;
                    }
                }
                self.recompute();
                (name, base, Some(id.to_string()), shown)
            }
            None => {
                self.begin_txn(type_label);
                (type_label.to_string(), self.model.clone(), None, None)
            }
        };
        let _ = label;
        Session {
            editing: feature.clone(),
            feature,
            base: BaseModel(base),
            saved_marker,
            synced: Vec::new(),
            notice: None,
            health: Health::Healthy,
            shown_sketch,
        }
    }

    // ------------------------------------------------------------------ changes
    /// Writes the dialog payload into the document and recomputes the preview. Call after
    /// changing any payload field (the UI and tests do this).
    pub fn dialog_changed(&mut self) {
        let Some(mut d) = self.dialog.take() else { return };
        match &mut d {
            Dialog::Extrude(x) => self.update_extrude(x),
            Dialog::Revolve(x) => self.update_revolve(x),
            Dialog::Fillet(x) => self.update_edges(x, false),
            Dialog::Chamfer(x) => self.update_edges(x, true),
            Dialog::Hole(x) => self.update_hole(x),
            Dialog::OffsetPlane(x) => self.update_plane(x),
            Dialog::Joint(x) => self.update_joint(x),
            Dialog::Surface(x) => self.update_surface(x),
            other => self.update_more(other),
        }
        if let Some(s) = d.session_mut() {
            s.health = s.feature.as_ref().map(|f| self.model.health_of(f)).unwrap_or(Health::Healthy);
        }
        self.dialog = Some(d);
        self.sync_selection();
    }

    /// Sets the dialog's primary value (distance / angle / radius / diameter / offset) and
    /// updates the preview. Returns false when the expression was rejected.
    #[allow(dead_code)]
    pub fn dialog_set_primary(&mut self, expression: &str) -> bool {
        let Some(f) = self.dialog.as_mut().and_then(|d| d.primary_field_mut()) else { return false };
        f.text = expression.to_string();
        self.dialog_changed();
        self.dialog.as_mut().and_then(|d| d.primary_field_mut()).is_some_and(|f| f.error.is_none())
    }

    /// Mutates the dialog payload and updates the preview.
    pub fn dialog_edit(&mut self, f: impl FnOnce(&mut Dialog)) {
        if let Some(d) = self.dialog.as_mut() {
            f(d);
            self.dialog_changed();
        }
    }

    /// Why OK is not possible right now (`None` when it is).
    pub fn dialog_blocker(&self) -> Option<String> {
        let d = self.dialog.as_ref()?;
        let s = d.session()?;
        for (name, f) in d.fields() {
            if let Some(e) = &f.error {
                return Some(format!("{name}: {e}"));
            }
        }
        if let Some(t) = d.missing_input_text() {
            return Some(t.to_string());
        }
        if let Some(Health::Error(e)) = s.feature.as_ref().map(|f| self.model.health_of(f)) {
            return Some(e);
        }
        None
    }

    /// Status line in Fusion's style ("1 Profile | Area : 1500.00 mm^2", "4 Edges").
    pub fn dialog_status(&self) -> Option<String> {
        let d = self.dialog.as_ref()?;
        match d {
            Dialog::Extrude(x) => self.profile_status(&x.s, x.sketch.as_deref(), &x.profiles),
            Dialog::Revolve(x) => self.profile_status(&x.s, x.sketch.as_deref(), &x.profiles),
            Dialog::Fillet(x) | Dialog::Chamfer(x) if !x.edges.is_empty() => Some(format!("{} Edge{}", x.edges.len(), if x.edges.len() == 1 { "" } else { "s" })),
            Dialog::Joint(x) => self.joint_status(x),
            Dialog::Surface(x) => self.surface_status(x),
            other => self.more_status(other),
        }
    }

    // ------------------------------------------------------------------ closing
    pub fn dialog_ok(&mut self) {
        if matches!(self.dialog, Some(Dialog::Surface(_))) && self.surface_dialog_ok() {
            return;
        }
        let Some(d) = self.dialog.as_ref() else { return };
        if !d.owns_txn() {
            self.dialog = None;
            return;
        }
        if let Some(why) = self.dialog_blocker() {
            self.error(why);
            return;
        }
        let Some(d) = self.dialog.take() else { return };
        let s = d.session().cloned().expect("feature dialog");
        let name = s.feature.as_ref().and_then(|f| self.doc.feature(f)).map(|f| f.name.clone()).unwrap_or_default();
        if s.editing.is_some() {
            self.doc.marker = s.saved_marker.min(self.doc.features.len());
            if let Some((sk, vis)) = &s.shown_sketch {
                if let Some(FeatureKind::Sketch { visible, .. }) = self.doc.feature_mut(sk).map(|f| &mut f.kind) {
                    *visible = *vis;
                }
            }
        } else {
            // Fusion's default "auto-hide sketch on feature creation"
            let sk = match &d {
                Dialog::Extrude(x) => x.sketch.clone(),
                Dialog::Revolve(x) => x.sketch.clone(),
                Dialog::Surface(x) => x.sketch(),
                _ => None,
            };
            if let Some(sk) = sk {
                if let Some(FeatureKind::Sketch { visible, .. }) = self.doc.feature_mut(&sk).map(|f| &mut f.kind) {
                    *visible = false;
                }
            }
        }
        self.commit_txn();
        self.selection.clear();
        if let Health::Warning(w) = self.model.health_of(s.feature.as_deref().unwrap_or("")) {
            self.warn(format!("{name}: {w}"));
        } else {
            self.info(if s.editing.is_some() { format!("Edited {name}") } else { format!("Created {name}") });
        }
    }

    pub fn dialog_cancel(&mut self) {
        let Some(d) = self.dialog.take() else { return };
        if d.owns_txn() {
            self.cancel_txn();
            self.selection.clear();
        }
    }

    // ------------------------------------------------------------------ selection
    /// Routes a selection change into the active dialog's selection inputs.
    pub(crate) fn on_selection_changed(&mut self) {
        // Sketch selections belong to the sketch session (only tool dialogs can be open then).
        if matches!(self.mode, Mode::Sketch(_)) {
            return;
        }
        let Some(mut d) = self.dialog.take() else { return };
        let changed = match &mut d {
            Dialog::Extrude(x) => {
                let r = self.route(&x.s.synced, |p| matches!(p, Pick::Profile { .. }));
                self.route_profiles(&mut x.s, &mut x.sketch, &mut x.profiles, r)
            }
            Dialog::Revolve(x) => {
                let r = self.route(&x.s.synced, |p| matches!(p, Pick::Profile { .. }));
                let before = x.sketch.clone();
                let ch = self.route_profiles(&mut x.s, &mut x.sketch, &mut x.profiles, r);
                if x.sketch != before {
                    x.axis = self.default_axis(x.sketch.as_deref(), &x.s.base.0);
                }
                ch
            }
            Dialog::Fillet(x) | Dialog::Chamfer(x) => {
                let r = self.route(&x.s.synced, |p| matches!(p, Pick::Edge { .. } | Pick::Face { .. }));
                self.route_edges(x, r)
            }
            Dialog::Hole(x) => {
                let r = self.route(&x.s.synced, |p| matches!(p, Pick::Face { .. }));
                self.route_hole(x, r)
            }
            Dialog::OffsetPlane(x) => {
                let r = self.route(&x.s.synced, |p| matches!(p, Pick::Face { .. } | Pick::OriginPlane(_) | Pick::ConstructionPlane(_)));
                self.route_plane(x, r)
            }
            Dialog::Measure(x) => {
                let r = self.route(&x.synced, |p| matches!(p, Pick::Face { .. } | Pick::Edge { .. } | Pick::Body { .. } | Pick::Profile { .. }));
                tools::route_measure(x, r);
                false
            }
            Dialog::Inspect(x) => {
                let r = self.route(&x.synced, inspect::is_inspect_pick);
                self.route_inspect(x, r);
                false
            }
            Dialog::Joint(x) => {
                let r = self.route(&x.s.synced, assembly::is_joint_pick);
                self.route_joint(x, r)
            }
            Dialog::Surface(x) => self.route_surface(x),
            other => self.route_more(other).unwrap_or(false),
        };
        self.dialog = Some(d);
        if changed {
            self.dialog_changed();
        } else {
            self.sync_selection();
        }
    }

    /// Tells a canvas click (the hovered item changed selection state) from other changes.
    pub(crate) fn route(&self, synced: &[Pick], relevant: impl Fn(&Pick) -> bool) -> Route {
        let cur: Vec<&Pick> = self.selection.iter().filter(|p| relevant(p)).collect();
        if let Some(h) = self.hover.as_ref().filter(|h| relevant(h)) {
            let in_cur = cur.contains(&h);
            let in_prev = synced.contains(h);
            if in_cur != in_prev || (in_cur && in_prev && cur.len() == 1) {
                return Route::Toggle(h.clone());
            }
        }
        let added: Vec<Pick> = cur.iter().filter(|p| !synced.contains(p)).map(|p| (*p).clone()).collect();
        if !added.is_empty() {
            return Route::Add(added);
        }
        if !cur.is_empty() {
            let removed: Vec<Pick> = synced.iter().filter(|p| relevant(p) && !cur.contains(p)).cloned().collect();
            if !removed.is_empty() {
                return Route::Remove(removed);
            }
        }
        Route::Nothing
    }

    /// Mirrors the dialog inputs into `state.selection` (canvas highlight).
    pub(crate) fn sync_selection(&mut self) {
        if matches!(self.mode, Mode::Sketch(_)) {
            return;
        }
        let Some(d) = self.dialog.as_ref() else { return };
        let picks: Option<Vec<Pick>> = match d {
            Dialog::Extrude(x) => Some(self.profile_picks(x.sketch.as_deref(), &x.profiles)),
            Dialog::Revolve(x) => Some(self.profile_picks(x.sketch.as_deref(), &x.profiles)),
            Dialog::Fillet(x) | Dialog::Chamfer(x) => Some(x.edges.iter().filter_map(|r| self.edge_pick(&x.s.base.0, r)).collect()),
            Dialog::Hole(x) => Some(x.face.iter().filter_map(|r| self.face_pick(&x.s.base.0, r)).collect()),
            Dialog::OffsetPlane(x) => Some(x.base.iter().filter_map(|b| self.plane_pick(&x.s.base.0, b)).collect()),
            Dialog::Measure(x) => Some(x.picks.clone()),
            Dialog::Inspect(x) => Some(x.picks()),
            Dialog::Joint(x) => Some(self.joint_picks(x)),
            Dialog::Surface(x) => Some(self.surface_picks(x)),
            other => self.more_picks(other),
        };
        if let Some(p) = picks {
            self.selection = p.clone();
            match self.dialog.as_mut() {
                Some(Dialog::Measure(m)) => m.synced = p,
                Some(Dialog::Inspect(m)) => m.synced = p,
                Some(d) => {
                    if let Some(s) = d.session_mut() {
                        s.synced = p;
                    }
                }
                None => {}
            }
        }
    }
}
