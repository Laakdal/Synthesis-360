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
}

fn yes() -> bool {
    true
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
            origin_visible: true,
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
    let body_owner = |b: &str| b.split(':').next().unwrap_or(b).to_string();
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
        FeatureKind::Import { .. } => {}
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
    Feature { id: format!("{}{}", label.to_lowercase(), n), name, suppressed: false, kind }
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
