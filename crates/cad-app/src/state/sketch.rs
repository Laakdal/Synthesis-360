//! Sketch environment: creating/editing sketches, sketch tools, dimensions, constraints and the
//! render overlay for the active sketch.
//!
//! INTERFACE CONTRACT (used by app.rs / viewport): the pub types and pub fns below. Their
//! bodies may be extended freely, but signatures must stay stable.
//!
//! Layout of the implementation:
//! * this file — contract types, session lifecycle (one transaction per session), command
//!   routing, keyboard handling and the render overlay;
//! * `sketch_tools.rs` — pointer handling: snapping, creation tools, Select (hover / select /
//!   drag), Delete, Normal/Construction and the Sketch Dimension tool with its value entry;
//! * `sketch_constraints.rs` — the geometric constraint tools;
//! * `sketch_tests.rs` — headless workflow tests.

use super::{AppState, Mode, Pick, ViewRequest};
use crate::state::command::CommandId;
use cad_core::sketch::interact::{self, Axis};
use cad_core::sketch::{ConstraintKind, DimensionValues, Entity, EntityKind, Sketch, SolveStatus, ORIGIN};
use cad_core::{Document, FeatureId, PlaneRef, Quantity};
use cad_kernel::{Frame, SketchResult};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[path = "sketch_constraints.rs"]
mod constraints;
#[path = "sketch_modify.rs"]
mod modify;
#[cfg(test)]
#[path = "sketch_tests.rs"]
mod tests;
#[path = "sketch_tools.rs"]
mod tools;

/// Fusion's message when a dimension or constraint adds no independent equation (F18).
pub const OVER_CONSTRAINED: &str = "Sketch geometry is over constrained";
/// Line tool horizontal / vertical inference threshold (F15: inferred at ~1°).
pub const HV_INFERENCE_DEG: f64 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstraintTool {
    Coincident,
    Collinear,
    Concentric,
    Midpoint,
    Fix,
    Parallel,
    Perpendicular,
    HorizontalVertical,
    Tangent,
    Equal,
    Symmetry,
}

impl ConstraintTool {
    pub const ALL: [ConstraintTool; 11] = [
        ConstraintTool::Coincident,
        ConstraintTool::Collinear,
        ConstraintTool::Concentric,
        ConstraintTool::Midpoint,
        ConstraintTool::Fix,
        ConstraintTool::Parallel,
        ConstraintTool::Perpendicular,
        ConstraintTool::HorizontalVertical,
        ConstraintTool::Tangent,
        ConstraintTool::Equal,
        ConstraintTool::Symmetry,
    ];
    pub fn label(self) -> &'static str {
        match self {
            ConstraintTool::Coincident => "Coincident",
            ConstraintTool::Collinear => "Collinear",
            ConstraintTool::Concentric => "Concentric",
            ConstraintTool::Midpoint => "Midpoint",
            ConstraintTool::Fix => "Fix/UnFix",
            ConstraintTool::Parallel => "Parallel",
            ConstraintTool::Perpendicular => "Perpendicular",
            ConstraintTool::HorizontalVertical => "Horizontal/Vertical",
            ConstraintTool::Tangent => "Tangent",
            ConstraintTool::Equal => "Equal",
            ConstraintTool::Symmetry => "Symmetry",
        }
    }
    pub fn command(self) -> CommandId {
        match self {
            ConstraintTool::Coincident => CommandId::ConstraintCoincident,
            ConstraintTool::Collinear => CommandId::ConstraintCollinear,
            ConstraintTool::Concentric => CommandId::ConstraintConcentric,
            ConstraintTool::Midpoint => CommandId::ConstraintMidpoint,
            ConstraintTool::Fix => CommandId::ConstraintFix,
            ConstraintTool::Parallel => CommandId::ConstraintParallel,
            ConstraintTool::Perpendicular => CommandId::ConstraintPerpendicular,
            ConstraintTool::HorizontalVertical => CommandId::ConstraintHorizontalVertical,
            ConstraintTool::Tangent => CommandId::ConstraintTangent,
            ConstraintTool::Equal => CommandId::ConstraintEqual,
            ConstraintTool::Symmetry => CommandId::ConstraintSymmetry,
        }
    }
    /// Short text glyph used in the palette and over the canvas.
    pub fn glyph(self) -> &'static str {
        match self {
            ConstraintTool::Coincident => "⊙",
            ConstraintTool::Collinear => "⋯",
            ConstraintTool::Concentric => "◎",
            ConstraintTool::Midpoint => "Δ",
            ConstraintTool::Fix => "🔒",
            ConstraintTool::Parallel => "∥",
            ConstraintTool::Perpendicular => "⊥",
            ConstraintTool::HorizontalVertical => "H/V",
            ConstraintTool::Tangent => "T",
            ConstraintTool::Equal => "=",
            ConstraintTool::Symmetry => "↔",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SketchTool {
    Select,
    Line,
    Rectangle2Pt,
    RectangleCenter,
    Circle,
    Arc3Pt,
    Point,
    Dimension,
    Constraint(ConstraintTool),
    Rectangle3Pt,
    Circle2Pt,
    ArcCenter,
    Polygon,
    Slot,
    Fillet,
    Trim,
    Extend,
    Offset,
    Mirror,
}

impl SketchTool {
    pub fn label(self) -> &'static str {
        match self {
            SketchTool::Select => "Select",
            SketchTool::Line => "Line",
            SketchTool::Rectangle2Pt => "2-Point Rectangle",
            SketchTool::RectangleCenter => "Center Rectangle",
            SketchTool::Circle => "Center Diameter Circle",
            SketchTool::Arc3Pt => "3-Point Arc",
            SketchTool::Point => "Point",
            SketchTool::Dimension => "Sketch Dimension",
            SketchTool::Constraint(c) => c.label(),
            SketchTool::Rectangle3Pt => "3-Point Rectangle",
            SketchTool::Circle2Pt => "2-Point Circle",
            SketchTool::ArcCenter => "Center Point Arc",
            SketchTool::Polygon => "Polygon",
            SketchTool::Slot => "Slot",
            SketchTool::Fillet => "Sketch Fillet",
            SketchTool::Trim => "Trim",
            SketchTool::Extend => "Extend",
            SketchTool::Offset => "Offset",
            SketchTool::Mirror => "Mirror",
        }
    }
    /// Shape tools that place geometry from a fixed number of clicks.
    pub(crate) fn clicks_needed(self) -> Option<usize> {
        match self {
            SketchTool::Rectangle2Pt | SketchTool::RectangleCenter | SketchTool::Circle | SketchTool::Circle2Pt | SketchTool::Polygon => Some(2),
            SketchTool::Arc3Pt | SketchTool::Rectangle3Pt | SketchTool::ArcCenter | SketchTool::Slot => Some(3),
            _ => None,
        }
    }
    /// The Modify tools that pick existing geometry instead of placing points.
    pub(crate) fn is_modify(self) -> bool {
        matches!(self, SketchTool::Fillet | SketchTool::Trim | SketchTool::Extend | SketchTool::Offset | SketchTool::Mirror)
    }
}

/// What the cursor snapped to.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Snap {
    #[default]
    None,
    /// An existing point (or the sketch origin, id [`ORIGIN`]).
    Point(String),
    /// A position on an existing curve.
    Curve(String),
}

/// Line tool chain in progress. The first point is only created with the first segment, so
/// ending the chain after one click leaves nothing behind.
#[derive(Clone, Debug)]
pub struct LineChain {
    pub first: ([f64; 2], Snap),
    pub start: Option<String>,
    pub last: Option<String>,
}

/// A dimension whose label follows the cursor until it is placed.
#[derive(Clone, Debug)]
pub struct PendingDimension {
    pub picks: Vec<String>,
    pub kind: ConstraintKind,
    pub label: [f64; 2],
}

/// Inline prompt at a dimension label.
#[derive(Clone, Debug)]
pub enum SketchPrompt {
    /// Value entry for a dimension's parameter (accepts expressions like `d1/2`, `30 mm`).
    Value { constraint: String, param: String, text: String, error: Option<String>, focus: bool },
    /// The dimension would over-constrain the sketch; offer to create it as driven (F18).
    Driven { kind: ConstraintKind, label: [f64; 2], measured: f64 },
}

/// Select-tool drag: the points moved (held while solving) and their positions at the press.
#[derive(Clone, Debug)]
pub struct DragState {
    pub ids: Vec<String>,
    pub start: Vec<[f64; 2]>,
    pub press: [f64; 2],
    /// Dragging a circle's rim changes its radius (centre held).
    pub radius_of: Option<String>,
}

/// State of an open sketch-edit session (one undo step: "Edit Sketch1").
pub struct SketchSession {
    pub feature: FeatureId,
    pub tool: SketchTool,
    /// Timeline marker before editing; downstream features are rolled back while editing.
    pub saved_marker: usize,
    /// Points clicked so far by the active tool (sketch coordinates).
    pub clicks: Vec<[f64; 2]>,
    /// Current (snapped) cursor position in sketch coordinates.
    pub cursor: Option<[f64; 2]>,
    /// Snap target of each click in `clicks`.
    pub click_snaps: Vec<Snap>,
    /// Snap target under the cursor.
    pub snap: Snap,
    /// Horizontal / vertical inference of the line being drawn.
    pub inferred: Option<Axis>,
    pub chain: Option<LineChain>,
    /// Entities picked so far by the Dimension / constraint tools.
    pub picks: Vec<String>,
    pub pending_dim: Option<PendingDimension>,
    pub prompt: Option<SketchPrompt>,
    pub drag: Option<DragState>,
    /// Press position and what was under it.
    pub press: Option<([f64; 2], Option<Pick>)>,
    pub dragged: bool,
    /// Select tool: dimension whose label is being dragged.
    pub label_drag: Option<String>,
    /// Last pick tolerance (mm per ~8 px); sizes glyph offsets.
    pub tolerance: f64,
    /// Multi-step Modify tools: Mirror 0 = picking objects, 1 = picking the mirror line.
    pub stage: u8,
    /// Center Point Arc: angle swept by the cursor since the start point (sign = direction).
    pub arc_turn: f64,
    pub arc_last: Option<f64>,
    /// Polygon tool options (kept across tool switches within the session).
    pub polygon_sides: usize,
    pub polygon_inscribed: bool,
    /// Parameter of the last fillet radius; the next fillet starts from its value.
    pub fillet_param: Option<String>,
    /// Offset whose distance box is open: committing a value re-places the offset curves.
    pub(crate) offset_edit: Option<modify::OffsetEdit>,
}

impl SketchSession {
    fn new(feature: FeatureId, saved_marker: usize) -> Self {
        SketchSession {
            feature,
            tool: SketchTool::Select,
            saved_marker,
            clicks: Vec::new(),
            cursor: None,
            click_snaps: Vec::new(),
            snap: Snap::None,
            inferred: None,
            chain: None,
            picks: Vec::new(),
            pending_dim: None,
            prompt: None,
            drag: None,
            press: None,
            dragged: false,
            label_drag: None,
            tolerance: 1.0,
            stage: 0,
            arc_turn: 0.0,
            arc_last: None,
            polygon_sides: 6,
            polygon_inscribed: true,
            fillet_param: None,
            offset_edit: None,
        }
    }
    fn reset_tool_state(&mut self) {
        self.clicks.clear();
        self.click_snaps.clear();
        self.snap = Snap::None;
        self.inferred = None;
        self.chain = None;
        self.picks.clear();
        self.pending_dim = None;
        self.drag = None;
        self.press = None;
        self.dragged = false;
        self.label_drag = None;
        self.stage = 0;
        self.arc_turn = 0.0;
        self.arc_last = None;
    }
    /// Whether a tool operation is in progress (Esc cancels it before anything else).
    pub fn busy(&self) -> bool {
        !self.clicks.is_empty() || self.chain.is_some() || !self.picks.is_empty() || self.pending_dim.is_some() || self.prompt.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Move,
    Press,
    Release,
    Click,
    DoubleClick,
    /// Dragging with the primary button held (after a Press).
    Drag,
}

/// Pointer event in sketch-plane coordinates.
#[derive(Clone, Copy, Debug)]
pub struct SketchPointer {
    pub pos: [f64; 2],
    /// Pick tolerance in sketch units (mm), ~8 px at the current zoom.
    pub tolerance: f64,
    pub kind: PointerKind,
    pub shift: bool,
    pub ctrl: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SketchKey {
    Escape,
    Enter,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveState {
    /// Under-constrained (Fusion: blue)
    Free,
    /// Fully constrained (Fusion: black)
    Fixed,
    Construction,
    Hovered,
    Selected,
    /// Rubber-band preview of the active tool
    Preview,
    /// Involved in a failed solve
    Error,
}

pub struct OverlayCurve {
    pub id: String,
    pub polyline: Vec<[f64; 2]>,
    pub state: CurveState,
    pub dashed: bool,
}

pub struct OverlayPoint {
    pub id: String,
    pub pos: [f64; 2],
    pub state: CurveState,
}

pub struct OverlayDimension {
    pub id: String,
    pub text: String,
    pub anchor: [f64; 2],
    /// Extension / dimension lines in sketch coordinates.
    pub lines: Vec<([f64; 2], [f64; 2])>,
    pub driven: bool,
    pub selected: bool,
}

pub struct OverlayGlyph {
    pub constraint: String,
    pub pos: [f64; 2],
    pub label: &'static str,
    pub selected: bool,
}

/// Everything the viewport needs to draw the active sketch (sketch coordinates + its frame).
pub struct SketchOverlay {
    pub frame: Frame,
    pub curves: Vec<OverlayCurve>,
    pub points: Vec<OverlayPoint>,
    /// Closed regions: each is an outer polygon followed by hole polygons.
    pub profiles: Vec<Vec<Vec<[f64; 2]>>>,
    pub dimensions: Vec<OverlayDimension>,
    pub glyphs: Vec<OverlayGlyph>,
    /// e.g. "Fully constrained" / "4 degrees of freedom"
    pub status: String,
}

/// Id used for the dimension being placed / the inferred-constraint preview glyph.
pub const PENDING_ID: &str = "__pending";
pub const PREVIEW_ID: &str = "__preview";

pub(crate) fn quantity_of(kind: &ConstraintKind) -> Quantity {
    if kind.is_angle() {
        Quantity::Angle
    } else {
        Quantity::Length
    }
}

/// Expression text for a measured value ("50 mm", "90 deg").
pub(crate) fn value_text(v: f64, q: Quantity) -> String {
    cad_core::expr::Display(v, q).to_string()
}

/// Canvas glyph of a constraint (None for dimensions).
pub fn constraint_glyph(kind: &ConstraintKind) -> Option<&'static str> {
    use ConstraintKind::*;
    Some(match kind {
        Coincident { .. } | PointOnCurve { .. } => "⊙",
        Horizontal { .. } => "H",
        Vertical { .. } => "V",
        Parallel { .. } => "∥",
        Perpendicular { .. } => "⊥",
        Collinear { .. } => "⋯",
        Equal { .. } => "=",
        Tangent { .. } => "T",
        Concentric { .. } => "◎",
        Midpoint { .. } => "Δ",
        Fix { .. } => "🔒",
        Symmetric { .. } => "↔",
        _ => return None,
    })
}

/// Glyph positions of all geometric constraints: (constraint id, position, glyph).
pub(crate) fn constraint_glyphs(sk: &Sketch, tol: f64) -> Vec<(String, [f64; 2], &'static str)> {
    use ConstraintKind::*;
    let off = 1.8 * tol;
    let mut count: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    for c in &sk.constraints {
        let Some(label) = constraint_glyph(&c.kind) else { continue };
        let targets: Vec<&str> = match &c.kind {
            Coincident { a, b } => vec![if a == ORIGIN { b } else { a }],
            PointOnCurve { p, curve } => vec![if p == ORIGIN { curve } else { p }],
            Midpoint { p, .. } | Fix { p, .. } => vec![p],
            Symmetric { line, .. } => vec![line],
            Concentric { a, .. } => vec![a],
            Horizontal { line } | Vertical { line } => vec![line],
            Parallel { a, b } | Perpendicular { a, b } | Collinear { a, b } | Equal { a, b } | Tangent { a, b } => vec![a, b],
            _ => Vec::new(),
        };
        for t in targets {
            let k = count.entry(t.to_string()).or_default();
            if let Some(p) = interact::glyph_anchor(sk, t, off, *k) {
                out.push((c.id.clone(), p, label));
                *k += 1;
            }
        }
    }
    out
}

/// Default label position for a dimension created without one.
pub(crate) fn default_label(sk: &Sketch, kind: &ConstraintKind) -> [f64; 2] {
    let pts: Vec<[f64; 2]> = kind
        .refs()
        .iter()
        .flat_map(|r| match sk.line_points(r) {
            Some((a, b)) => vec![a, b],
            None => sk.point(r).into_iter().collect(),
        })
        .collect();
    if pts.is_empty() {
        return [0.0, 0.0];
    }
    let n = pts.len() as f64;
    let c = pts.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
    [c[0] + 5.0, c[1] - 5.0]
}

/// Adds `kind` unless it would over-constrain the sketch (automatic / inferred constraints).
pub(crate) fn add_if_independent(sk: &mut Sketch, vals: &DimensionValues, kind: ConstraintKind) -> bool {
    if cad_core::sketch::would_overconstrain(sk, &kind, None, vals) {
        return false;
    }
    sk.add_constraint(kind);
    true
}

fn rect_polyline(a: [f64; 2], b: [f64; 2]) -> Vec<[f64; 2]> {
    vec![a, [b[0], a[1]], b, [a[0], b[1]], a]
}

impl AppState {
    /// Creates a sketch on `plane` and enters sketch mode (Fusion: Create Sketch with a
    /// preselected plane enters sketch mode directly).
    pub fn create_sketch(&mut self, plane: PlaneRef) {
        self.begin_txn("Create Sketch");
        let id = cad_core::ops::new_sketch(&mut self.doc, plane);
        self.recompute();
        self.enter_sketch(id);
    }

    /// Opens an existing sketch for editing (timeline double-click / browser "Edit Sketch").
    pub fn edit_sketch(&mut self, id: &str) {
        if self.doc.feature(id).and_then(|f| f.sketch()).is_none() {
            return;
        }
        let name = self.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
        self.begin_txn(&format!("Edit {name}"));
        self.enter_sketch(id.to_string());
    }

    fn enter_sketch(&mut self, id: FeatureId) {
        let saved = self.doc.marker;
        if let Some(i) = self.doc.feature_index(&id) {
            self.doc.marker = i + 1;
        }
        self.recompute();
        if let Some(r) = self.model.sketches.get(&id) {
            let f = r.frame;
            self.view_requests.push(ViewRequest::LookAt(f.origin, f.n, f.y));
        }
        self.selection.clear();
        self.hover = None;
        self.mode = Mode::Sketch(SketchSession::new(id, saved));
    }

    pub fn finish_sketch(&mut self) {
        if let Mode::Sketch(s) = std::mem::replace(&mut self.mode, Mode::Model) {
            self.doc.marker = s.saved_marker.max(self.doc.feature_index(&s.feature).map(|i| i + 1).unwrap_or(0)).min(self.doc.features.len());
            self.commit_txn();
            self.selection.clear();
            self.hover = None;
        }
    }

    pub fn cancel_sketch(&mut self) {
        if matches!(self.mode, Mode::Sketch(_)) {
            self.mode = Mode::Model;
            // the kernel's sketch cache does not key on constraints: drop it so constraint-only
            // edits of the session are not written back over the restored document
            self.kernel.clear();
            self.cancel_txn();
            self.selection.clear();
            self.hover = None;
        }
    }

    pub fn active_sketch(&self) -> Option<&SketchSession> {
        match &self.mode {
            Mode::Sketch(s) => Some(s),
            Mode::Model => None,
        }
    }

    pub(crate) fn session_mut(&mut self) -> Option<&mut SketchSession> {
        match &mut self.mode {
            Mode::Sketch(s) => Some(s),
            Mode::Model => None,
        }
    }

    pub(crate) fn sketch_id(&self) -> Option<FeatureId> {
        self.active_sketch().map(|s| s.feature.clone())
    }

    /// Solved state of the active sketch (as drawn).
    pub fn sketch_result(&self) -> Option<Arc<SketchResult>> {
        let id = &self.active_sketch()?.feature;
        self.model.sketches.get(id).cloned()
    }

    /// The active sketch as stored in the document.
    pub(crate) fn doc_sketch(&self) -> Option<&Sketch> {
        self.doc.feature(&self.active_sketch()?.feature)?.sketch()
    }

    /// Values of the active sketch's driving dimensions, by constraint id.
    pub(crate) fn dimension_values(&self) -> DimensionValues {
        let mut out = DimensionValues::new();
        let Some(sk) = self.doc_sketch() else { return out };
        let pv = self.doc.parameter_values();
        for c in &sk.constraints {
            if let (Some(p), false) = (&c.param, c.driven) {
                if let Some(v) = pv.values.get(p) {
                    out.insert(c.id.clone(), *v);
                }
            }
        }
        out
    }

    fn sketch_solved(&self) -> bool {
        self.sketch_result().is_none_or(|r| r.report.status == SolveStatus::Solved)
    }

    /// Applies an edit to the document inside the sketch session (no history entry: the
    /// whole session is one transaction). `Err` rejects the edit and restores the document; an
    /// edit that turns a solvable sketch into an unsolvable one is rejected too.
    pub(crate) fn edit_doc<R>(&mut self, f: impl FnOnce(&mut Document, &str, &DimensionValues) -> Result<R, String>) -> Result<R, String> {
        let fid = self.sketch_id().ok_or("No sketch is being edited")?;
        let vals = self.dimension_values();
        let was_ok = self.sketch_solved();
        let before = self.doc.clone();
        match f(&mut self.doc, &fid, &vals) {
            Ok(r) => {
                self.dirty = true;
                self.recompute_sketch_edit();
                if was_ok && !self.sketch_solved() {
                    self.doc = before;
                    self.recompute();
                    return Err("The sketch cannot be solved with this change".into());
                }
                Ok(r)
            }
            Err(e) => {
                self.doc = before;
                Err(e)
            }
        }
    }

    /// Recompute after a sketch edit. The kernel's sketch cache key covers entity geometry and
    /// dimension parameters but not the constraint list, so a constraint-only edit that moves
    /// nothing (deleting a constraint, a Fix, ...) would be served from the cache and the stale
    /// solved copy written back over the document. Detect that and recompute without cache.
    pub(crate) fn recompute_sketch_edit(&mut self) {
        let intended = self.doc_sketch().cloned();
        self.recompute();
        let (Some(intended), Some(fid)) = (intended, self.sketch_id()) else { return };
        if self.sketch_result().is_some_and(|r| r.solved.constraints != intended.constraints) {
            if let Some(sk) = cad_core::ops::sketch_mut(&mut self.doc, &fid) {
                *sk = intended;
            }
            self.kernel.clear();
            self.recompute();
        }
    }

    /// [`Self::edit_doc`] on the active sketch itself.
    pub(crate) fn edit_active_sketch<R>(&mut self, f: impl FnOnce(&mut Sketch, &DimensionValues) -> Result<R, String>) -> Result<R, String> {
        self.edit_doc(|d, fid, vals| {
            let sk = cad_core::ops::sketch_mut(d, fid).ok_or("The sketch no longer exists")?;
            f(sk, vals)
        })
    }

    pub fn set_sketch_tool(&mut self, tool: SketchTool) {
        self.close_prompt();
        if let Mode::Sketch(s) = &mut self.mode {
            s.tool = tool;
            s.reset_tool_state();
        }
        self.hover = None;
    }

    pub(crate) fn sketch_command(&mut self, id: CommandId) {
        use CommandId::*;
        match id {
            CreateSketch => {
                let plane = self.selection.iter().find_map(|p| match p {
                    Pick::OriginPlane(o) => Some(PlaneRef::Origin { plane: *o }),
                    Pick::ConstructionPlane(f) => Some(PlaneRef::Construction { feature: f.clone() }),
                    Pick::Face { body, face } => self.model.body(body).and_then(|b| cad_kernel::refs::face_ref(b, *face)).map(|face| PlaneRef::Face { face }),
                    _ => None,
                });
                match plane {
                    Some(p) => {
                        self.plane_prompt = false;
                        self.create_sketch(p)
                    }
                    None => {
                        // Fusion: Create Sketch without a selection shows the origin planes and waits
                        self.plane_prompt = true;
                        self.info("Select a plane or planar face to create a sketch on.");
                    }
                }
            }
            FinishSketch => self.finish_sketch(),
            Line => self.set_sketch_tool(SketchTool::Line),
            Rectangle2Pt => self.set_sketch_tool(SketchTool::Rectangle2Pt),
            RectangleCenter => self.set_sketch_tool(SketchTool::RectangleCenter),
            CircleCenter => self.set_sketch_tool(SketchTool::Circle),
            Arc3Pt => self.set_sketch_tool(SketchTool::Arc3Pt),
            Point => self.set_sketch_tool(SketchTool::Point),
            SketchDimension => self.start_dimension_tool(),
            ToggleConstruction => self.toggle_construction(),
            ConstraintCoincident => self.start_constraint(ConstraintTool::Coincident),
            ConstraintCollinear => self.start_constraint(ConstraintTool::Collinear),
            ConstraintConcentric => self.start_constraint(ConstraintTool::Concentric),
            ConstraintMidpoint => self.start_constraint(ConstraintTool::Midpoint),
            ConstraintFix => self.start_constraint(ConstraintTool::Fix),
            ConstraintParallel => self.start_constraint(ConstraintTool::Parallel),
            ConstraintPerpendicular => self.start_constraint(ConstraintTool::Perpendicular),
            ConstraintHorizontalVertical => self.start_constraint(ConstraintTool::HorizontalVertical),
            ConstraintTangent => self.start_constraint(ConstraintTool::Tangent),
            ConstraintEqual => self.start_constraint(ConstraintTool::Equal),
            ConstraintSymmetry => self.start_constraint(ConstraintTool::Symmetry),
            Rectangle3Pt => self.set_sketch_tool(SketchTool::Rectangle3Pt),
            Circle2Pt => self.set_sketch_tool(SketchTool::Circle2Pt),
            ArcCenter => self.set_sketch_tool(SketchTool::ArcCenter),
            Polygon => self.set_sketch_tool(SketchTool::Polygon),
            Slot => self.set_sketch_tool(SketchTool::Slot),
            Trim => self.set_sketch_tool(SketchTool::Trim),
            Extend => self.set_sketch_tool(SketchTool::Extend),
            SketchFillet => self.start_modify_tool(SketchTool::Fillet),
            SketchOffset => self.start_modify_tool(SketchTool::Offset),
            SketchMirror => self.start_modify_tool(SketchTool::Mirror),
            _ => self.info("This sketch command is not available."),
        }
    }

    pub fn sketch_pointer(&mut self, ev: SketchPointer) {
        let Some(s) = self.session_mut() else { return };
        if ev.tolerance > 0.0 && ev.tolerance.is_finite() {
            s.tolerance = ev.tolerance;
        }
        let tool = s.tool;
        // an open value box / driven offer: a click elsewhere commits (or declines) it
        if let Some(prompt) = &s.prompt {
            let driven = matches!(prompt, SketchPrompt::Driven { .. });
            match ev.kind {
                PointerKind::Click => {
                    if driven {
                        self.sketch_value_cancel();
                    } else {
                        self.sketch_value_commit();
                    }
                    return;
                }
                PointerKind::Press | PointerKind::Drag | PointerKind::Release => return,
                PointerKind::Move | PointerKind::DoubleClick => {}
            }
            if ev.kind == PointerKind::Move {
                if let Some(s) = self.session_mut() {
                    s.cursor = Some(ev.pos);
                }
                return;
            }
        }
        match tool {
            SketchTool::Select => self.select_pointer(ev),
            SketchTool::Dimension => self.dimension_pointer(ev),
            SketchTool::Constraint(c) => self.constraint_pointer(c, ev),
            t if t.is_modify() => self.modify_pointer(t, ev),
            t => self.create_pointer(t, ev),
        }
    }

    pub fn sketch_key(&mut self, key: SketchKey) {
        let Some(s) = self.active_sketch() else { return };
        match key {
            SketchKey::Escape => {
                if s.prompt.is_some() {
                    self.sketch_value_cancel();
                    return;
                }
                if s.chain.is_some() {
                    // first Esc ends the line chain, the tool stays active (Fusion)
                    if let Some(s) = self.session_mut() {
                        s.chain = None;
                        s.inferred = None;
                    }
                    return;
                }
                if s.busy() || s.tool != SketchTool::Select {
                    self.set_sketch_tool(SketchTool::Select);
                    return;
                }
                if self.selection.iter().any(|p| matches!(p, Pick::SketchEntity(_) | Pick::SketchConstraint(_))) {
                    self.clear_selection();
                    return;
                }
                self.finish_sketch();
            }
            SketchKey::Enter => {
                if let Some(p) = &s.prompt {
                    if matches!(p, SketchPrompt::Driven { .. }) {
                        self.sketch_accept_driven();
                    } else {
                        self.sketch_value_commit();
                    }
                } else if s.chain.is_some() {
                    if let Some(s) = self.session_mut() {
                        s.chain = None;
                        s.inferred = None;
                    }
                } else if s.tool == SketchTool::Mirror && s.stage == 0 && !s.picks.is_empty() {
                    // Mirror: Enter ends the object selection; the mirror line comes next
                    if let Some(s) = self.session_mut() {
                        s.stage = 1;
                    }
                }
            }
            SketchKey::Delete => self.sketch_delete_selection(),
        }
    }

    /// Short instruction for the active tool (palette / canvas hint).
    pub fn sketch_tool_hint(&self) -> String {
        let Some(s) = self.active_sketch() else { return String::new() };
        if let Some(p) = &s.prompt {
            return match p {
                SketchPrompt::Value { .. } => "Enter a value or expression, then press Enter.".into(),
                SketchPrompt::Driven { .. } => format!("{OVER_CONSTRAINED}. Create a driven dimension instead?"),
            };
        }
        match s.tool {
            SketchTool::Select => "Click to select, drag points or curves to move them. Double-click a dimension to edit it.".into(),
            SketchTool::Line => {
                if s.chain.is_some() {
                    "Click the next point. Double-click, Enter or Esc ends the line.".into()
                } else {
                    "Click the start point of the line.".into()
                }
            }
            SketchTool::Rectangle2Pt => if s.clicks.is_empty() { "Click the first corner." } else { "Click the opposite corner." }.into(),
            SketchTool::RectangleCenter => if s.clicks.is_empty() { "Click the centre of the rectangle." } else { "Click a corner." }.into(),
            SketchTool::Circle => if s.clicks.is_empty() { "Click the centre of the circle." } else { "Click to set the diameter." }.into(),
            SketchTool::Arc3Pt => match s.clicks.len() {
                0 => "Click the start point of the arc.",
                1 => "Click the end point of the arc.",
                _ => "Click a point on the arc.",
            }
            .into(),
            SketchTool::Point => "Click to place a point.".into(),
            SketchTool::Dimension => {
                if s.pending_dim.is_some() {
                    "Click to place the dimension (or pick another entity to change its type).".into()
                } else if !s.picks.is_empty() {
                    "Select a second point or a line.".into()
                } else {
                    "Select a line, circle, arc, or two points / lines to dimension.".into()
                }
            }
            SketchTool::Constraint(c) => constraints::hint(c, s.picks.len()).into(),
            SketchTool::Rectangle3Pt => match s.clicks.len() {
                0 => "Click the first corner.",
                1 => "Click the end of the first edge (sets width and direction).",
                _ => "Click to set the height.",
            }
            .into(),
            SketchTool::Circle2Pt => if s.clicks.is_empty() { "Click the first end of the diameter." } else { "Click the other end of the diameter." }.into(),
            SketchTool::ArcCenter => match s.clicks.len() {
                0 => "Click the centre of the arc.",
                1 => "Click the start point (sets the radius).",
                _ => "Move in the direction of the arc and click its end.",
            }
            .into(),
            SketchTool::Polygon => {
                let kind = if s.polygon_inscribed { "a vertex" } else { "the midpoint of an edge" };
                if s.clicks.is_empty() { format!("Click the centre of the {}-sided polygon.", s.polygon_sides) } else { format!("Click {kind}.") }
            }
            SketchTool::Slot => match s.clicks.len() {
                0 => "Click the centre of the first slot end.",
                1 => "Click the centre of the second slot end.",
                _ => "Click to set the slot width.",
            }
            .into(),
            SketchTool::Fillet => if s.picks.is_empty() { "Click a corner, or the first of two lines or arcs." } else { "Click the second line or arc." }.into(),
            SketchTool::Trim => "Click the curve segment to remove (up to the nearest intersections).".into(),
            SketchTool::Extend => "Click near the end of a line or arc to extend it to the next curve.".into(),
            SketchTool::Offset => if s.picks.is_empty() { "Select a curve; its connected chain is offset." } else { "Click on the side to offset to (the value box sets the distance)." }.into(),
            SketchTool::Mirror => {
                if s.stage == 0 {
                    if s.picks.is_empty() { "Select the curves to mirror." } else { "Click the mirror line (Shift+click adds more lines; Enter also moves on to the mirror line)." }.into()
                } else {
                    "Select the mirror line.".into()
                }
            }
        }
    }

    /// Render data for the active sketch.
    pub fn sketch_overlay(&self) -> Option<SketchOverlay> {
        let s = self.active_sketch()?;
        let r = self.model.sketches.get(&s.feature)?;
        let sk = &r.solved;
        let tol = s.tolerance;
        let picked = |id: &str| s.picks.iter().any(|p| p == id) || s.pending_dim.as_ref().is_some_and(|d| d.picks.iter().any(|p| p == id));
        let selected = |id: &str| picked(id) || self.selection.iter().any(|p| matches!(p, Pick::SketchEntity(e) | Pick::SketchConstraint(e) if e == id));
        let hovered = |id: &str| matches!(&self.hover, Some(Pick::SketchEntity(e)) | Some(Pick::SketchConstraint(e)) if e == id);
        let fully: HashSet<&str> = r.report.fully_constrained.iter().map(String::as_str).collect();
        let mut errors: HashSet<String> = HashSet::new();
        if let SolveStatus::Failed { unsatisfied } = &r.report.status {
            for cid in unsatisfied {
                if let Some(c) = sk.constraint(cid) {
                    for e in c.kind.refs() {
                        errors.insert(e.to_string());
                        if let Some(o) = sk.entity(e).and_then(|en| en.owner.clone()) {
                            errors.insert(o);
                        }
                    }
                }
            }
        }
        let state_of = |e: &Entity| {
            if selected(&e.id) {
                CurveState::Selected
            } else if hovered(&e.id) {
                CurveState::Hovered
            } else if errors.contains(&e.id) {
                CurveState::Error
            } else if e.construction {
                CurveState::Construction
            } else if fully.contains(e.id.as_str()) {
                CurveState::Fixed
            } else {
                CurveState::Free
            }
        };

        let mut curves = Vec::new();
        let mut points = Vec::new();
        for e in &sk.entities {
            match e.kind {
                EntityKind::Point { x, y } => points.push(OverlayPoint { id: e.id.clone(), pos: [x, y], state: state_of(e) }),
                _ => {
                    let poly = cad_core::sketch::profiles::entity_polyline(sk, e);
                    if poly.len() >= 2 {
                        curves.push(OverlayCurve { id: e.id.clone(), polyline: poly, state: state_of(e), dashed: e.construction });
                    }
                }
            }
        }
        let origin_state = if selected(ORIGIN) {
            CurveState::Selected
        } else if hovered(ORIGIN) {
            CurveState::Hovered
        } else {
            CurveState::Fixed
        };
        points.push(OverlayPoint { id: ORIGIN.into(), pos: [0.0, 0.0], state: origin_state });

        // dimensions
        let pv = self.doc.parameter_values();
        let mut dimensions = Vec::new();
        for c in sk.constraints.iter().filter(|c| c.kind.is_dimension()) {
            let label = c.label.unwrap_or_else(|| default_label(sk, &c.kind));
            let v = if c.driven { r.report.driven_values.get(&c.id).copied() } else { c.param.as_ref().and_then(|p| pv.values.get(p).copied()) };
            let text = dimension_text(&c.kind, v, c.driven);
            dimensions.push(OverlayDimension {
                id: c.id.clone(),
                text,
                anchor: label,
                lines: interact::dimension_graphics(sk, &c.kind, label),
                driven: c.driven,
                selected: selected(&c.id) || hovered(&c.id),
            });
        }
        if let Some(pd) = &s.pending_dim {
            let v = cad_core::sketch::measure(sk, &pd.kind);
            dimensions.push(OverlayDimension {
                id: PENDING_ID.into(),
                text: dimension_text(&pd.kind, v, false),
                anchor: pd.label,
                lines: interact::dimension_graphics(sk, &pd.kind, pd.label),
                driven: false,
                selected: true,
            });
        }
        if let Some(SketchPrompt::Driven { kind, label, measured }) = &s.prompt {
            dimensions.push(OverlayDimension {
                id: PENDING_ID.into(),
                text: dimension_text(kind, Some(*measured), true),
                anchor: *label,
                lines: interact::dimension_graphics(sk, kind, *label),
                driven: true,
                selected: true,
            });
        }

        // constraint glyphs
        let mut glyphs: Vec<OverlayGlyph> = constraint_glyphs(sk, tol)
            .into_iter()
            .map(|(id, pos, label)| {
                let sel = selected(&id) || hovered(&id);
                OverlayGlyph { constraint: id, pos, label, selected: sel }
            })
            .collect();

        // tool preview
        if let Some(cur) = s.cursor {
            let mut preview = |poly: Vec<[f64; 2]>, dashed: bool| curves.push(OverlayCurve { id: PREVIEW_ID.into(), polyline: poly, state: CurveState::Preview, dashed });
            match s.tool {
                SketchTool::Line => {
                    if let Some(ch) = &s.chain {
                        let from = ch.last.as_deref().and_then(|id| sk.point(id)).unwrap_or(ch.first.0);
                        preview(vec![from, cur], false);
                        if let Some(ax) = s.inferred {
                            let m = interact::lerp(from, cur, 0.5);
                            let g = if ax == Axis::Horizontal { "H" } else { "V" };
                            let off = if ax == Axis::Horizontal { [0.0, 1.8 * tol] } else { [1.8 * tol, 0.0] };
                            glyphs.push(OverlayGlyph { constraint: PREVIEW_ID.into(), pos: interact::add(m, off), label: g, selected: true });
                        }
                    }
                }
                SketchTool::Rectangle2Pt if s.clicks.len() == 1 => preview(rect_polyline(s.clicks[0], cur), false),
                SketchTool::RectangleCenter if s.clicks.len() == 1 => {
                    let c = s.clicks[0];
                    let a = [2.0 * c[0] - cur[0], 2.0 * c[1] - cur[1]];
                    preview(rect_polyline(a, cur), false);
                    preview(vec![a, cur], true);
                }
                SketchTool::Circle if s.clicks.len() == 1 => preview(interact::circle_polyline(s.clicks[0], interact::dist(s.clicks[0], cur)), false),
                SketchTool::Arc3Pt => match s.clicks.len() {
                    1 => preview(vec![s.clicks[0], cur], false),
                    2 => preview(interact::arc_3pt_polyline(s.clicks[0], cur, s.clicks[1]), false),
                    _ => {}
                },
                _ => {}
            }
            curves.extend(self.tool_preview(s, sk, cur));
            let creating = s.tool == SketchTool::Line || s.tool == SketchTool::Point || s.tool.clicks_needed().is_some();
            if creating {
                let st = if s.snap == Snap::None { CurveState::Preview } else { CurveState::Hovered };
                points.push(OverlayPoint { id: PREVIEW_ID.into(), pos: cur, state: st });
            }
        }

        let profiles = r.profiles.iter().map(|p| std::iter::once(p.outer.polygon()).chain(p.holes.iter().map(|h| h.polygon())).collect()).collect();
        let status = match &r.report.status {
            SolveStatus::Failed { unsatisfied } => format!("Sketch cannot be solved: {} constraint(s) conflict", unsatisfied.len()),
            SolveStatus::Solved if sk.entities.is_empty() => "Empty sketch".to_string(),
            SolveStatus::Solved if r.report.is_fully_constrained() => "Fully constrained".to_string(),
            SolveStatus::Solved if r.report.dof == 1 => "1 degree of freedom".to_string(),
            SolveStatus::Solved => format!("{} degrees of freedom", r.report.dof),
        };
        Some(SketchOverlay { frame: r.frame, curves, points, profiles, dimensions, glyphs, status })
    }
}

/// Display text of a dimension value; driven dimensions in parentheses like Fusion.
pub(crate) fn dimension_text(kind: &ConstraintKind, v: Option<f64>, driven: bool) -> String {
    let body = match v {
        Some(v) => value_text(v, quantity_of(kind)),
        None => "?".into(),
    };
    let body = match kind {
        ConstraintKind::Diameter { .. } => format!("Ø{body}"),
        ConstraintKind::Radius { .. } => format!("R{body}"),
        _ => body,
    };
    if driven {
        format!("({body})")
    } else {
        body
    }
}
