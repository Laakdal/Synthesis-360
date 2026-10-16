//! Native document model.
//!
//! A document is an ordered feature list (the timeline) plus a parameter table. Everything that
//! determines geometry lives here; bodies are never stored but recomputed from the features by
//! the kernel. That is what makes the history editable: changing an extrude distance or a sketch
//! dimension and recomputing regenerates every downstream feature (verified behaviour in Fusion:
//! width 60 -> 70 mm with a downstream fillet increased the volume by exactly 10*30*20 mm³).

use crate::expr::{self, ExprError, Quantity};
use cad_sketch::Sketch;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub type FeatureId = String;
pub type BodyId = String;
/// Id of a (non-root) component; `None` designates the implicit root component.
pub type ComponentId = String;

pub const FORMAT: &str = "cad360";
pub const FORMAT_VERSION: u32 = 1;
pub const FILE_EXTENSION: &str = "c360";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub expression: String,
    pub quantity: Quantity,
    /// Owning feature for model parameters (d1, d2, ...); `None` for user parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<FeatureId>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OriginPlane {
    XY,
    XZ,
    YZ,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OriginAxis {
    X,
    Y,
    Z,
}

/// Geometric descriptor used to re-find a face after upstream edits (persistent reference).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceRef {
    pub body: BodyId,
    pub surface: SurfaceKind,
    pub normal: [f64; 3],
    /// Centroid normalised to the body's bounding box (0..1 per axis).
    pub nc: [f64; 3],
    pub area: f64,
    /// Generative (history-based) name of the face, e.g. "extrude3/side:l2#0"; preferred over the
    /// geometric descriptor when present (see cad-kernel naming). Absent in older documents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Geometric descriptor used to re-find an edge after upstream edits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeRef {
    pub body: BodyId,
    pub curve: CurveKind,
    pub dir: [f64; 3],
    /// Midpoint normalised to the body's bounding box.
    pub nm: [f64; 3],
    pub length: f64,
    /// Normals of the adjacent planar faces, sorted (disambiguates parallel edges).
    #[serde(default)]
    pub face_normals: Vec<[f64; 3]>,
    /// Generative names of the two adjacent faces (sorted); an edge is identified by them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub face_names: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SurfaceKind {
    Plane,
    Cylinder,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CurveKind {
    Line,
    Circle,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PlaneRef {
    Origin { plane: OriginPlane },
    Face { face: FaceRef },
    Construction { feature: FeatureId },
}

/// Persistent reference to a sketch profile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileRef {
    /// Sorted ids of the sketch curves bounding the region (outer loop + holes).
    pub curves: Vec<String>,
    /// A point inside the region at selection time (sketch coordinates), the fallback key.
    pub sample: [f64; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    /// "New Body"
    New,
    Join,
    Cut,
    Intersect,
}

impl Operation {
    pub const ALL: [Operation; 4] = [Operation::Join, Operation::Cut, Operation::Intersect, Operation::New];
    pub fn label(self) -> &'static str {
        match self {
            Operation::New => "New Body",
            Operation::Join => "Join",
            Operation::Cut => "Cut",
            Operation::Intersect => "Intersect",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtentDirection {
    /// "One Side"
    Normal,
    /// One side, flipped
    Reverse,
    Symmetric,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FeatureKind {
    Sketch {
        plane: PlaneRef,
        sketch: Sketch,
        #[serde(default = "yes")]
        visible: bool,
    },
    Extrude {
        sketch: FeatureId,
        profiles: Vec<ProfileRef>,
        /// Parameter holding the distance.
        distance: String,
        direction: ExtentDirection,
        /// Through All: extends through every body (Fusion extent type "All").
        #[serde(default)]
        through_all: bool,
        operation: Operation,
    },
    Revolve {
        sketch: FeatureId,
        profiles: Vec<ProfileRef>,
        axis: RevolveAxis,
        angle: String,
        operation: Operation,
    },
    Fillet { edges: Vec<EdgeRef>, radius: String },
    Chamfer { edges: Vec<EdgeRef>, distance: String },
    Hole {
        face: FaceRef,
        /// Centre in the face's sketch frame (mm).
        center: [f64; 2],
        diameter: String,
        depth: String,
        through_all: bool,
    },
    /// Offset construction plane.
    Plane {
        base: PlaneRef,
        offset: String,
        #[serde(default = "yes")]
        visible: bool,
    },
    /// Geometry imported from a STEP file (a non-parametric base feature).
    Import { source: String, step: String },
    /// SURFACE ▸ Extrude: open or closed sketch curves swept into a surface body.
    SurfaceExtrude { sketch: FeatureId, curves: Vec<String>, distance: String, direction: ExtentDirection },
    /// SURFACE ▸ Revolve: sketch curves revolved into a surface body.
    SurfaceRevolve { sketch: FeatureId, curves: Vec<String>, axis: RevolveAxis, angle: String },
    /// SURFACE ▸ Patch: fills a closed boundary (sketch profiles or a loop of body edges).
    Patch {
        #[serde(default)]
        sketch: Option<FeatureId>,
        #[serde(default)]
        profiles: Vec<ProfileRef>,
        #[serde(default)]
        edges: Vec<EdgeRef>,
    },
    /// SURFACE ▸ Stitch: sews surface bodies into one (a solid when closed).
    Stitch { bodies: Vec<BodyId>, tolerance: String },
    /// SURFACE ▸ Offset: offsets faces into a new surface body.
    OffsetSurface { faces: Vec<FaceRef>, distance: String },
    /// SURFACE/SOLID ▸ Thicken: turns faces or surface bodies into a solid.
    Thicken { faces: Vec<FaceRef>, thickness: String, symmetric: bool },
    /// INSERT ▸ Insert Mesh: an STL embedded in the document (base64), optionally converted to B-rep.
    InsertMesh { source: String, data: String, to_brep: bool },
    /// MODIFY > Shell: removes the selected faces and hollows the body to an inside thickness.
    Shell { faces: Vec<FaceRef>, thickness: String },
    /// MODIFY > Combine: joins, cuts or intersects tool bodies with a target body.
    Combine {
        target: BodyId,
        tools: Vec<BodyId>,
        operation: Operation,
        #[serde(default)]
        keep_tools: bool,
    },
    /// CREATE > Mirror (bodies): mirrored copies of bodies across a plane, as new bodies.
    MirrorBodies { bodies: Vec<BodyId>, plane: PlaneRef },
    /// CREATE > Rectangular Pattern (bodies) along one or two origin axes. Quantities count the
    /// original; distances are the spacing between instances (negative flips the direction).
    RectangularPattern {
        bodies: Vec<BodyId>,
        axis1: OriginAxis,
        quantity1: String,
        distance1: String,
        /// Second direction; `None` for a one-direction pattern.
        #[serde(default)]
        axis2: Option<OriginAxis>,
        quantity2: String,
        distance2: String,
        /// Join the instances to the original instead of creating new bodies.
        #[serde(default)]
        join: bool,
    },
    /// CREATE > Circular Pattern (bodies) about an origin axis. 360 deg spaces N instances at
    /// 360/N; a partial angle spreads them from 0 to the angle (angle/(N-1)).
    CircularPattern {
        bodies: Vec<BodyId>,
        axis: OriginAxis,
        quantity: String,
        angle: String,
        #[serde(default)]
        join: bool,
    },
    /// ASSEMBLE > Joint: constrains the components of `a` and `b` so that the two joint frames
    /// coincide with their z axes aligned in the same direction (Fusion F35), offset by `angle`
    /// about and `offset` along the joint z axis. The assembly solve decides which component
    /// moves: root / grounded components never move, otherwise the earlier-created one stays.
    Joint {
        a: JointOrigin,
        b: JointOrigin,
        kind: JointKind,
        /// Parameter holding the rotation about the joint axis (deg).
        angle: String,
        /// Parameter holding the translation along the joint axis (mm).
        offset: String,
        /// Reverses b's joint z axis (the z axes end up opposed instead of aligned).
        #[serde(default)]
        flip: bool,
    },
    /// MODIFY > Move/Copy (bodies): rotation about an origin axis, then a translation.
    MoveBodies {
        bodies: Vec<BodyId>,
        dx: String,
        dy: String,
        dz: String,
        axis: OriginAxis,
        angle: String,
        /// Copy: keep the originals and create the moved copies as new bodies.
        #[serde(default)]
        copy: bool,
    },
}

fn yes() -> bool {
    true
}

/// Joint type (the Fusion motion types CAD360 supports).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JointKind {
    /// No relative motion (angle / offset only position the component).
    Rigid,
    /// Rotation about the joint z axis.
    Revolute,
    /// Translation along the joint z axis.
    Slider,
}

impl JointKind {
    pub const ALL: [JointKind; 3] = [JointKind::Rigid, JointKind::Revolute, JointKind::Slider];
    pub fn label(self) -> &'static str {
        match self {
            JointKind::Rigid => "Rigid",
            JointKind::Revolute => "Revolute",
            JointKind::Slider => "Slider",
        }
    }
}

/// Geometry a joint frame is derived from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum JointGeometry {
    /// Planar face: origin at the face centroid, z = outward normal.
    Face { face: FaceRef },
    /// Circular edge: origin at the circle centre, z = circle axis.
    Edge { edge: EdgeRef },
}

/// One side of a joint: a persistent geometry reference plus the component its body belonged
/// to when it was picked (informative; the kernel derives the moving component from the body).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JointOrigin {
    pub geometry: JointGeometry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<ComponentId>,
}

impl JointOrigin {
    pub fn body(&self) -> &str {
        match &self.geometry {
            JointGeometry::Face { face } => &face.body,
            JointGeometry::Edge { edge } => &edge.body,
        }
    }
}

/// A component (Fusion: a design container with its own bodies, sketches and features). The
/// root component is implicit (`None`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub id: ComponentId,
    pub name: String,
    #[serde(default = "yes")]
    pub visible: bool,
    /// Grounded (Fusion "Ground"): joints never move this component (the other side moves).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub grounded: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RevolveAxis {
    SketchLine { entity: String },
    Origin { axis: OriginAxis },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub id: FeatureId,
    pub name: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
    /// Component that was active when the feature was created (`None` = root). Bodies belong to
    /// the component of the feature that created them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<ComponentId>,
    #[serde(flatten)]
    pub kind: FeatureKind,
}

impl Feature {
    pub fn type_label(&self) -> &'static str {
        type_label(&self.kind)
    }
    pub fn sketch(&self) -> Option<&Sketch> {
        match &self.kind {
            FeatureKind::Sketch { sketch, .. } => Some(sketch),
            _ => None,
        }
    }
    pub fn sketch_mut(&mut self) -> Option<&mut Sketch> {
        match &mut self.kind {
            FeatureKind::Sketch { sketch, .. } => Some(sketch),
            _ => None,
        }
    }
    /// Parameter names this feature's own values live in.
    pub fn parameter_names(&self) -> Vec<&str> {
        match &self.kind {
            FeatureKind::Sketch { sketch, .. } => sketch.constraints.iter().filter_map(|c| c.param.as_deref()).collect(),
            FeatureKind::Extrude { distance, .. } => vec![distance],
            FeatureKind::Revolve { angle, .. } => vec![angle],
            FeatureKind::Fillet { radius, .. } => vec![radius],
            FeatureKind::Chamfer { distance, .. } => vec![distance],
            FeatureKind::Hole { diameter, depth, .. } => vec![diameter, depth],
            FeatureKind::Plane { offset, .. } => vec![offset],
            FeatureKind::Import { .. } => vec![],
            FeatureKind::Shell { thickness, .. } => vec![thickness],
            FeatureKind::Combine { .. } | FeatureKind::MirrorBodies { .. } => vec![],
            FeatureKind::RectangularPattern { quantity1, distance1, quantity2, distance2, .. } => vec![quantity1, distance1, quantity2, distance2],
            FeatureKind::CircularPattern { quantity, angle, .. } => vec![quantity, angle],
            FeatureKind::MoveBodies { dx, dy, dz, angle, .. } => vec![dx, dy, dz, angle],
            FeatureKind::Joint { angle, offset, .. } => vec![angle, offset],
            FeatureKind::SurfaceExtrude { distance, .. } => vec![distance],
            FeatureKind::SurfaceRevolve { angle, .. } => vec![angle],
            FeatureKind::Stitch { tolerance, .. } => vec![tolerance],
            FeatureKind::OffsetSurface { distance, .. } => vec![distance],
            FeatureKind::Thicken { thickness, .. } => vec![thickness],
            FeatureKind::Patch { .. } | FeatureKind::InsertMesh { .. } => vec![],
        }
    }

    /// Ids of the bodies this feature takes as inputs.
    pub fn input_bodies(&self) -> Vec<&str> {
        match &self.kind {
            FeatureKind::Combine { target, tools, .. } => std::iter::once(target.as_str()).chain(tools.iter().map(String::as_str)).collect(),
            FeatureKind::MirrorBodies { bodies, .. }
            | FeatureKind::RectangularPattern { bodies, .. }
            | FeatureKind::CircularPattern { bodies, .. }
            | FeatureKind::MoveBodies { bodies, .. } => bodies.iter().map(String::as_str).collect(),
            FeatureKind::Shell { faces, .. } => faces.iter().map(|f| f.body.as_str()).collect(),
            FeatureKind::Joint { a, b, .. } => vec![a.body(), b.body()],
            FeatureKind::Stitch { bodies, .. } => bodies.iter().map(String::as_str).collect(),
            FeatureKind::OffsetSurface { faces, .. } | FeatureKind::Thicken { faces, .. } => faces.iter().map(|f| f.body.as_str()).collect(),
            FeatureKind::Patch { edges, .. } => edges.iter().map(|e| e.body.as_str()).collect(),
            _ => vec![],
        }
    }
}

pub fn type_label(kind: &FeatureKind) -> &'static str {
    match kind {
        FeatureKind::Sketch { .. } => "Sketch",
        FeatureKind::Extrude { .. } => "Extrude",
        FeatureKind::Revolve { .. } => "Revolve",
        FeatureKind::Fillet { .. } => "Fillet",
        FeatureKind::Chamfer { .. } => "Chamfer",
        FeatureKind::Hole { .. } => "Hole",
        FeatureKind::Plane { .. } => "Plane",
        FeatureKind::Import { .. } => "Import",
        FeatureKind::Shell { .. } => "Shell",
        FeatureKind::Combine { .. } => "Combine",
        FeatureKind::MirrorBodies { .. } => "Mirror",
        FeatureKind::RectangularPattern { .. } => "RectangularPattern",
        FeatureKind::CircularPattern { .. } => "CircularPattern",
        FeatureKind::MoveBodies { .. } => "Move",
        FeatureKind::Joint { .. } => "Joint",
        FeatureKind::SurfaceExtrude { .. } => "SurfaceExtrude",
        FeatureKind::SurfaceRevolve { .. } => "SurfaceRevolve",
        FeatureKind::Patch { .. } => "Patch",
        FeatureKind::Stitch { .. } => "Stitch",
        FeatureKind::OffsetSurface { .. } => "OffsetSurface",
        FeatureKind::Thicken { .. } => "Thicken",
        FeatureKind::InsertMesh { .. } => "Mesh",
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BodyMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub units: String,
    pub parameters: Vec<Parameter>,
    pub features: Vec<Feature>,
    /// Timeline marker: features with index >= marker are rolled back (not computed).
    pub marker: usize,
    pub counters: BTreeMap<String, u64>,
    #[serde(default)]
    pub bodies: BTreeMap<BodyId, BodyMeta>,
    #[serde(default = "yes")]
    pub origin_visible: bool,
    /// Components of the design (the root component is implicit and not listed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<Component>,
    /// Component new features are created in (`None` = root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_component: Option<ComponentId>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new("Untitled")
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DocError {
    #[error("{0}")]
    Expression(#[from] ExprError),
    #[error("Unknown parameter \"{0}\"")]
    UnknownParameter(String),
    #[error("Unknown feature \"{0}\"")]
    UnknownFeature(String),
    #[error("Invalid parameter name \"{0}\"")]
    InvalidName(String),
    #[error("A parameter named \"{0}\" already exists")]
    DuplicateName(String),
    #[error("Parameter \"{0}\" is used by {1} and cannot be deleted")]
    ParameterInUse(String, String),
    #[error("Unknown component \"{0}\"")]
    UnknownComponent(String),
    #[error("{0} still contains {1} feature(s) and cannot be deleted. Delete its features first.")]
    ComponentInUse(String, usize),
    #[error("{0}")]
    Format(String),
}

impl Document {
    pub fn new(name: &str) -> Self {
        Document {
            format: FORMAT.into(),
            version: FORMAT_VERSION,
            name: name.into(),
            units: "mm".into(),
            parameters: Vec::new(),
            features: Vec::new(),
            marker: 0,
            counters: BTreeMap::new(),
            bodies: BTreeMap::new(),
            // Fusion: the Origin folder is hidden by default (probe browser screenshots)
            origin_visible: false,
            components: Vec::new(),
            active_component: None,
        }
    }

    /// Allocates the next sequential name: Sketch1, Extrude2, d7 ... (Fusion naming, observed in
    /// the probe: d1..d11 created by sketches and features in creation order).
    pub fn next_name(&mut self, key: &str) -> String {
        let n = self.counters.entry(key.to_string()).or_insert(0);
        *n += 1;
        format!("{key}{n}")
    }

    pub fn feature(&self, id: &str) -> Option<&Feature> {
        self.features.iter().find(|f| f.id == id)
    }
    pub fn feature_mut(&mut self, id: &str) -> Option<&mut Feature> {
        self.features.iter_mut().find(|f| f.id == id)
    }
    pub fn feature_index(&self, id: &str) -> Option<usize> {
        self.features.iter().position(|f| f.id == id)
    }
    pub fn parameter(&self, name: &str) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.name == name)
    }

    /// Adds a model parameter owned by `owner` and returns its name.
    pub fn add_model_parameter(&mut self, expression: &str, quantity: Quantity, owner: &str) -> String {
        let name = self.next_name("d");
        self.parameters.push(Parameter { name: name.clone(), expression: expression.into(), quantity, owner: Some(owner.into()), comment: String::new() });
        name
    }

    pub fn add_user_parameter(&mut self, name: &str, expression: &str, quantity: Quantity, comment: &str) -> Result<(), DocError> {
        if !is_valid_parameter_name(name) {
            return Err(DocError::InvalidName(name.into()));
        }
        if self.parameter(name).is_some() {
            return Err(DocError::DuplicateName(name.into()));
        }
        let trial = Parameter { name: name.into(), expression: expression.into(), quantity, owner: None, comment: comment.into() };
        let mut params = self.parameters.clone();
        params.push(trial.clone());
        if let Some(e) = evaluate_parameters(&params).errors.get(name) {
            return Err(DocError::Expression(ExprError(e.clone())));
        }
        self.parameters.push(trial);
        Ok(())
    }

    pub fn delete_user_parameter(&mut self, name: &str) -> Result<(), DocError> {
        let users = self.parameter_users(name);
        if !users.is_empty() {
            return Err(DocError::ParameterInUse(name.into(), users.join(", ")));
        }
        self.parameters.retain(|p| p.name != name || p.owner.is_some());
        Ok(())
    }

    /// Parameters whose expressions reference `name`.
    pub fn parameter_users(&self, name: &str) -> Vec<String> {
        self.parameters.iter().filter(|p| expr::referenced_names(&p.expression).iter().any(|r| r == name)).map(|p| p.name.clone()).collect()
    }

    /// Validates and sets a parameter expression. Mirrors Fusion: an invalid expression is
    /// rejected and the previous value kept ("Expression is invalid.").
    pub fn set_expression(&mut self, name: &str, expression: &str) -> Result<f64, DocError> {
        let idx = self.parameters.iter().position(|p| p.name == name).ok_or_else(|| DocError::UnknownParameter(name.into()))?;
        let mut trial = self.parameters.clone();
        trial[idx].expression = expression.trim().into();
        let r = evaluate_parameters(&trial);
        if let Some(e) = r.errors.get(name) {
            return Err(DocError::Expression(ExprError(e.clone())));
        }
        self.parameters = trial;
        Ok(r.values[name])
    }

    pub fn parameter_values(&self) -> ParameterValues {
        evaluate_parameters(&self.parameters)
    }

    /// Whether a feature is computed (inside the marker and not suppressed).
    pub fn is_active(&self, id: &str) -> bool {
        match self.feature_index(id) {
            Some(i) => i < self.marker && !self.features[i].suppressed,
            None => false,
        }
    }

    /// Appends a feature at the marker position (Fusion inserts new features at the marker).
    pub fn insert_feature(&mut self, f: Feature) -> usize {
        let at = self.marker.min(self.features.len());
        self.features.insert(at, f);
        self.marker = at + 1;
        at
    }

    /// Deletes a feature and its model parameters. Dependents are kept and will report a
    /// reference error on recompute (observed in Fusion: deleting a sketch used by a cut left the
    /// cut with "Profile 1 missing ... The profile reference is lost").
    pub fn delete_feature(&mut self, id: &str) -> Result<(), DocError> {
        let idx = self.feature_index(id).ok_or_else(|| DocError::UnknownFeature(id.into()))?;
        let f = self.features.remove(idx);
        if idx < self.marker {
            self.marker -= 1;
        }
        let owned: HashSet<String> = self.parameters.iter().filter(|p| p.owner.as_deref() == Some(&f.id)).map(|p| p.name.clone()).collect();
        // freeze references to deleted parameters into their current values
        let values = self.parameter_values().values;
        for p in self.parameters.iter_mut() {
            if !owned.contains(&p.name) && expr::referenced_names(&p.expression).iter().any(|r| owned.contains(r)) {
                if let Some(v) = values.get(&p.name) {
                    p.expression = expr::Display(*v, p.quantity).to_string();
                }
            }
        }
        self.parameters.retain(|p| !owned.contains(&p.name));
        Ok(())
    }

    pub fn set_marker(&mut self, marker: usize) {
        self.marker = marker.min(self.features.len());
    }

    /// Moves a feature to a new timeline index if it stays after everything it depends on and
    /// before everything that depends on it.
    pub fn reorder_feature(&mut self, id: &str, to: usize) -> bool {
        let Some(from) = self.feature_index(id) else { return false };
        let to = to.min(self.features.len() - 1);
        if from == to {
            return true;
        }
        let mut trial = self.clone();
        let f = trial.features.remove(from);
        trial.features.insert(to, f);
        for (i, f) in trial.features.iter().enumerate() {
            if feature_dependencies(&trial, f).iter().any(|d| trial.feature_index(d).is_some_and(|j| j > i)) {
                return false;
            }
        }
        if from < self.marker && to >= self.marker {
            self.marker = to + 1;
        } else if from >= self.marker && to < self.marker {
            self.marker += 1;
        }
        self.features = trial.features;
        true
    }

    // ------------------------------------------------------------------ components
    pub fn component(&self, id: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id)
    }
    pub fn component_mut(&mut self, id: &str) -> Option<&mut Component> {
        self.components.iter_mut().find(|c| c.id == id)
    }

    /// Creates a component named "Component1", "Component2" ... and returns its id (it is not
    /// activated).
    pub fn add_component(&mut self) -> ComponentId {
        let name = self.next_name("Component");
        let n = self.counters.get("__component").copied().unwrap_or(0) + 1;
        self.counters.insert("__component".into(), n);
        let id = format!("component{n}");
        self.components.push(Component { id: id.clone(), name, visible: true, grounded: false });
        id
    }

    /// Display name of a component (`None` = the root component, named after the document).
    pub fn component_name(&self, id: Option<&str>) -> String {
        match id {
            None => self.name.clone(),
            Some(c) => self.component(c).map(|c| c.name.clone()).unwrap_or_else(|| c.to_string()),
        }
    }

    /// Component of a feature (`None` = root, also for unknown features).
    pub fn feature_component(&self, feature: &str) -> Option<&str> {
        self.feature(feature).and_then(|f| f.component.as_deref())
    }

    /// Component a body belongs to: the component of the feature that created it. `created_by`
    /// is the kernel's creating feature; without it the feature id encoded in the body id
    /// (`{feature}` / `{feature}~k`) is used.
    pub fn body_component(&self, body: &str, created_by: Option<&str>) -> Option<&str> {
        let owner = created_by.unwrap_or_else(|| body.split([':', '~']).next().unwrap_or(body));
        self.feature_component(owner)
    }

    /// Whether a component is shown (the root component and unknown ids count as visible).
    pub fn component_visible(&self, id: Option<&str>) -> bool {
        id.and_then(|c| self.component(c)).is_none_or(|c| c.visible)
    }

    /// Features owned by a component (timeline order).
    pub fn component_features(&self, id: &str) -> Vec<FeatureId> {
        self.features.iter().filter(|f| f.component.as_deref() == Some(id)).map(|f| f.id.clone()).collect()
    }

    /// Deletes an empty component (refused while it still owns features).
    pub fn delete_component(&mut self, id: &str) -> Result<(), DocError> {
        let c = self.component(id).ok_or_else(|| DocError::UnknownComponent(id.into()))?;
        let n = self.component_features(id).len();
        if n > 0 {
            return Err(DocError::ComponentInUse(c.name.clone(), n));
        }
        self.components.retain(|c| c.id != id);
        if self.active_component.as_deref() == Some(id) {
            self.active_component = None;
        }
        Ok(())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("document serialises")
    }

    pub fn from_json(text: &str) -> Result<Document, DocError> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| DocError::Format(format!("The file is not a valid CAD360 document ({e}).")))?;
        if v.get("format").and_then(|f| f.as_str()) != Some(FORMAT) {
            return Err(DocError::Format("The file is not a CAD360 document.".into()));
        }
        let version = v.get("version").and_then(|f| f.as_u64()).unwrap_or(0);
        if version == 0 || version > FORMAT_VERSION as u64 {
            return Err(DocError::Format(format!("Unsupported CAD360 document version {version}.")));
        }
        let mut doc: Document = serde_json::from_value(v).map_err(|e| DocError::Format(format!("The document is damaged: {e}")))?;
        doc.marker = doc.marker.min(doc.features.len());
        if doc.active_component.as_ref().is_some_and(|c| doc.component(c).is_none()) {
            doc.active_component = None;
        }
        Ok(doc)
    }
}

pub fn is_valid_parameter_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !["mm", "cm", "m", "in", "ft", "deg", "rad", "PI", "pi"].contains(&name)
}

#[derive(Clone, Debug, Default)]
pub struct ParameterValues {
    pub values: HashMap<String, f64>,
    pub errors: HashMap<String, String>,
}

/// Evaluates the parameter table in dependency order with cycle detection.
pub fn evaluate_parameters(params: &[Parameter]) -> ParameterValues {
    let by_name: HashMap<&str, &Parameter> = params.iter().map(|p| (p.name.as_str(), p)).collect();
    let mut out = ParameterValues::default();
    let mut visiting: HashSet<String> = HashSet::new();

    fn visit(name: &str, by_name: &HashMap<&str, &Parameter>, out: &mut ParameterValues, visiting: &mut HashSet<String>) -> Option<f64> {
        if let Some(v) = out.values.get(name) {
            return Some(*v);
        }
        if out.errors.contains_key(name) {
            return None;
        }
        let p = by_name.get(name)?;
        if !visiting.insert(name.to_string()) {
            out.errors.insert(name.into(), "Circular reference".into());
            return None;
        }
        let res = expr::evaluate(&p.expression, p.quantity, &mut |r: &str| -> Result<Option<(f64, Quantity)>, ExprError> {
            match by_name.get(r) {
                None => Ok(None),
                Some(q) => match visit(r, by_name, out, visiting) {
                    Some(v) => Ok(Some((v, q.quantity))),
                    None => Err(ExprError(format!("Parameter \"{r}\" has an error"))),
                },
            }
        });
        visiting.remove(name);
        match res {
            Ok(e) => {
                out.values.insert(name.into(), e.value);
                Some(e.value)
            }
            Err(e) => {
                if !out.errors.contains_key(name) {
                    out.errors.insert(name.into(), e.0);
                }
                None
            }
        }
    }

    for p in params {
        visit(&p.name, &by_name, &mut out, &mut visiting);
    }
    out
}

/// Ids of features this feature directly depends on.
pub fn feature_dependencies(doc: &Document, f: &Feature) -> Vec<FeatureId> {
    let mut deps: Vec<FeatureId> = Vec::new();
    let mut add = |id: &str| {
        if id != f.id && !deps.iter().any(|d| d == id) && doc.feature(id).is_some() {
            deps.push(id.to_string());
        }
    };
    // body ids are "{feature}" or "{feature}~k" (further solids / copies made by one feature)
    let body_owner = |b: &str| b.split([':', '~']).next().unwrap_or(b).to_string();
    let plane_dep = |p: &PlaneRef| match p {
        PlaneRef::Face { face } => Some(body_owner(&face.body)),
        PlaneRef::Construction { feature } => Some(feature.clone()),
        PlaneRef::Origin { .. } => None,
    };
    match &f.kind {
        FeatureKind::Sketch { plane, .. } => {
            if let Some(d) = plane_dep(plane) {
                add(&d);
            }
        }
        FeatureKind::Extrude { sketch, .. } | FeatureKind::Revolve { sketch, .. } => add(sketch),
        FeatureKind::Fillet { edges, .. } | FeatureKind::Chamfer { edges, .. } => {
            for e in edges {
                add(&body_owner(&e.body));
            }
        }
        FeatureKind::Hole { face, .. } => add(&body_owner(&face.body)),
        FeatureKind::Plane { base, .. } => {
            if let Some(d) = plane_dep(base) {
                add(&d);
            }
        }
        FeatureKind::Import { .. } | FeatureKind::InsertMesh { .. } => {}
        FeatureKind::SurfaceExtrude { sketch, .. } | FeatureKind::SurfaceRevolve { sketch, .. } => add(sketch),
        FeatureKind::Patch { sketch, .. } => {
            if let Some(sk) = sketch {
                add(sk);
            }
            for b in f.input_bodies() {
                add(&body_owner(b));
            }
        }
        FeatureKind::Stitch { .. } | FeatureKind::OffsetSurface { .. } | FeatureKind::Thicken { .. } => {
            for b in f.input_bodies() {
                add(&body_owner(b));
            }
        }
        FeatureKind::MirrorBodies { plane, .. } => {
            if let Some(d) = plane_dep(plane) {
                add(&d);
            }
            for b in f.input_bodies() {
                add(&body_owner(b));
            }
        }
        FeatureKind::Shell { .. }
        | FeatureKind::Combine { .. }
        | FeatureKind::RectangularPattern { .. }
        | FeatureKind::CircularPattern { .. }
        | FeatureKind::MoveBodies { .. }
        | FeatureKind::Joint { .. } => {
            for b in f.input_bodies() {
                add(&body_owner(b));
            }
        }
    }
    deps
}

/// Features downstream that (transitively) depend on `id`, in timeline order.
pub fn dependent_features(doc: &Document, id: &str) -> Vec<FeatureId> {
    let mut set: HashSet<FeatureId> = HashSet::from([id.to_string()]);
    let mut out = Vec::new();
    for f in &doc.features {
        if f.id != id && feature_dependencies(doc, f).iter().any(|d| set.contains(d)) {
            set.insert(f.id.clone());
            out.push(f.id.clone());
        }
    }
    out
}

/// Creates a feature identity (stable id + Fusion-style display name).
pub fn new_feature(doc: &mut Document, kind: FeatureKind) -> Feature {
    let label = type_label(&kind);
    let name = doc.next_name(label);
    let n = doc.counters.get("__uid").copied().unwrap_or(0) + 1;
    doc.counters.insert("__uid".into(), n);
    Feature { id: format!("{}{}", label.to_lowercase(), n), name, suppressed: false, component: doc.active_component.clone(), kind }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_params() -> Document {
        let mut d = Document::new("t");
        d.add_model_parameter("50 mm", Quantity::Length, "sketch1");
        d.add_model_parameter("30 mm", Quantity::Length, "sketch1");
        d.add_model_parameter("d2 / 2", Quantity::Length, "extrude1");
        d
    }

    #[test]
    fn sequential_parameter_names_like_fusion() {
        let d = doc_with_params();
        let names: Vec<_> = d.parameters.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["d1", "d2", "d3"]);
        assert_eq!(d.parameter_values().values["d3"], 15.0);
    }

    #[test]
    fn set_expression_validates_and_propagates() {
        let mut d = doc_with_params();
        assert_eq!(d.set_expression("d2", "40 mm").unwrap(), 40.0);
        assert_eq!(d.parameter_values().values["d3"], 20.0);
        assert!(d.set_expression("d2", "foo * 2").is_err());
        assert!(d.set_expression("d2", "15 deg").is_err());
        assert_eq!(d.parameter("d2").unwrap().expression, "40 mm", "rejected edits keep the old value");
    }

    #[test]
    fn cycles_are_detected() {
        let mut d = doc_with_params();
        assert!(d.set_expression("d2", "d3 * 2").is_err(), "d3 = d2 / 2, so d2 = d3 * 2 is circular");
        let mut params = d.parameters.clone();
        params[1].expression = "d3 * 2".into();
        let r = evaluate_parameters(&params);
        assert!(r.errors.contains_key("d2") && r.errors.contains_key("d3"));
    }

    #[test]
    fn user_parameters() {
        let mut d = doc_with_params();
        d.add_user_parameter("width", "75 mm", Quantity::Length, "overall width").unwrap();
        assert!(d.add_user_parameter("width", "1 mm", Quantity::Length, "").is_err());
        assert!(d.add_user_parameter("2bad", "1 mm", Quantity::Length, "").is_err());
        d.set_expression("d1", "width - 5 mm").unwrap();
        assert_eq!(d.parameter_values().values["d1"], 70.0);
        assert!(matches!(d.delete_user_parameter("width"), Err(DocError::ParameterInUse(..))));
    }

    #[test]
    fn json_round_trip_and_format_errors() {
        let mut d = doc_with_params();
        let f = new_feature(&mut d, FeatureKind::Sketch { plane: PlaneRef::Origin { plane: OriginPlane::XY }, sketch: Sketch::new(), visible: true });
        d.insert_feature(f);
        let text = d.to_json();
        let back = Document::from_json(&text).unwrap();
        assert_eq!(back, d);
        assert!(Document::from_json("{}").is_err());
        assert!(Document::from_json("not json").is_err());
        assert!(Document::from_json(&text.replace("\"version\": 1", "\"version\": 99")).is_err());
    }

    #[test]
    fn insert_at_marker_and_delete() {
        let mut d = Document::new("t");
        let a = new_feature(&mut d, FeatureKind::Sketch { plane: PlaneRef::Origin { plane: OriginPlane::XY }, sketch: Sketch::new(), visible: true });
        let aid = a.id.clone();
        d.insert_feature(a);
        let p = d.add_model_parameter("10 mm", Quantity::Length, "extrude2");
        let b = new_feature(&mut d, FeatureKind::Extrude { sketch: aid.clone(), profiles: vec![], distance: p, direction: ExtentDirection::Normal, through_all: false, operation: Operation::New });
        let bid = b.id.clone();
        d.insert_feature(b);
        assert_eq!(dependent_features(&d, &aid), vec![bid.clone()]);
        d.set_marker(1);
        let c = new_feature(&mut d, FeatureKind::Plane { base: PlaneRef::Origin { plane: OriginPlane::XY }, offset: "d1".into(), visible: true });
        let at = d.insert_feature(c);
        assert_eq!(at, 1, "new features go at the marker");
        assert_eq!(d.marker, 2);
        d.delete_feature(&aid).unwrap();
        assert_eq!(d.features.len(), 2);
        assert_eq!(d.marker, 1);
    }
}
