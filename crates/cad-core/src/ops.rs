//! Document operations used by commands and tests. Each creates features together with their
//! model parameters (Fusion creates a model parameter for every dimension and feature value).

use crate::document::*;
use crate::expr::Quantity;
use cad_sketch::{ConstraintKind, Sketch};

pub fn new_sketch(doc: &mut Document, plane: PlaneRef) -> FeatureId {
    let f = new_feature(doc, FeatureKind::Sketch { plane, sketch: Sketch::new(), visible: true });
    let id = f.id.clone();
    doc.insert_feature(f);
    id
}

pub fn sketch_mut<'a>(doc: &'a mut Document, id: &str) -> Option<&'a mut Sketch> {
    doc.feature_mut(id)?.sketch_mut()
}

/// Adds a driving (or driven) dimension to a sketch with a new model parameter (d1, d2, ...).
pub fn add_dimension(doc: &mut Document, sketch: &str, kind: ConstraintKind, expression: &str, label: [f64; 2], driven: bool) -> Option<(String, String)> {
    let q = if kind.is_angle() { Quantity::Angle } else { Quantity::Length };
    let param = doc.add_model_parameter(expression, q, sketch);
    let s = sketch_mut(doc, sketch)?;
    let cid = s.add_dimension(kind, &param, label, driven);
    Some((cid, param))
}

pub fn delete_sketch_constraint(doc: &mut Document, sketch: &str, constraint: &str) {
    let param = doc.feature(sketch).and_then(|f| f.sketch()).and_then(|s| s.constraint(constraint)).and_then(|c| c.param.clone());
    if let Some(s) = sketch_mut(doc, sketch) {
        s.delete_constraint(constraint);
    }
    if let Some(p) = param {
        doc.parameters.retain(|q| q.name != p);
    }
}

/// Deletes sketch entities and the model parameters of dimensions removed with them.
pub fn delete_sketch_entity(doc: &mut Document, sketch: &str, entity: &str) {
    let before: Vec<String> = doc.feature(sketch).and_then(|f| f.sketch()).map(|s| s.constraints.iter().filter_map(|c| c.param.clone()).collect()).unwrap_or_default();
    if let Some(s) = sketch_mut(doc, sketch) {
        s.delete_entity(entity);
    }
    let after: Vec<String> = doc.feature(sketch).and_then(|f| f.sketch()).map(|s| s.constraints.iter().filter_map(|c| c.param.clone()).collect()).unwrap_or_default();
    doc.parameters.retain(|p| !(before.contains(&p.name) && !after.contains(&p.name)));
}

fn with_param(doc: &mut Document, kind: impl FnOnce(String) -> FeatureKind, expression: &str, q: Quantity, type_hint: &str) -> FeatureId {
    // allocate the feature first so the parameter's owner is its id
    let placeholder = new_feature(doc, kind(String::new()));
    let id = placeholder.id.clone();
    let p = doc.add_model_parameter(expression, q, &id);
    let mut f = placeholder;
    match &mut f.kind {
        FeatureKind::Extrude { distance, .. } => *distance = p,
        FeatureKind::Revolve { angle, .. } => *angle = p,
        FeatureKind::Fillet { radius, .. } => *radius = p,
        FeatureKind::Chamfer { distance, .. } => *distance = p,
        FeatureKind::Plane { offset, .. } => *offset = p,
        _ => debug_assert!(false, "{type_hint}"),
    }
    doc.insert_feature(f);
    id
}

pub fn new_extrude(doc: &mut Document, sketch: &str, profiles: Vec<ProfileRef>, distance: &str, direction: ExtentDirection, operation: Operation) -> FeatureId {
    let s = sketch.to_string();
    with_param(doc, |d| FeatureKind::Extrude { sketch: s, profiles, distance: d, direction, through_all: false, operation }, distance, Quantity::Length, "extrude")
}

pub fn new_revolve(doc: &mut Document, sketch: &str, profiles: Vec<ProfileRef>, axis: RevolveAxis, angle: &str, operation: Operation) -> FeatureId {
    let s = sketch.to_string();
    with_param(doc, |a| FeatureKind::Revolve { sketch: s, profiles, axis, angle: a, operation }, angle, Quantity::Angle, "revolve")
}

pub fn new_fillet(doc: &mut Document, edges: Vec<EdgeRef>, radius: &str) -> FeatureId {
    with_param(doc, |r| FeatureKind::Fillet { edges, radius: r }, radius, Quantity::Length, "fillet")
}

pub fn new_chamfer(doc: &mut Document, edges: Vec<EdgeRef>, distance: &str) -> FeatureId {
    with_param(doc, |d| FeatureKind::Chamfer { edges, distance: d }, distance, Quantity::Length, "chamfer")
}

pub fn new_plane(doc: &mut Document, base: PlaneRef, offset: &str) -> FeatureId {
    with_param(doc, |o| FeatureKind::Plane { base, offset: o, visible: true }, offset, Quantity::Length, "plane")
}

pub fn new_hole(doc: &mut Document, face: FaceRef, center: [f64; 2], diameter: &str, depth: &str, through_all: bool) -> FeatureId {
    let f = new_feature(doc, FeatureKind::Hole { face, center, diameter: String::new(), depth: String::new(), through_all });
    let id = f.id.clone();
    let pd = doc.add_model_parameter(diameter, Quantity::Length, &id);
    let ph = doc.add_model_parameter(depth, Quantity::Length, &id);
    let mut f = f;
    if let FeatureKind::Hole { diameter, depth, .. } = &mut f.kind {
        *diameter = pd;
        *depth = ph;
    }
    doc.insert_feature(f);
    id
}

pub fn new_import(doc: &mut Document, source: &str, step: String) -> FeatureId {
    let f = new_feature(doc, FeatureKind::Import { source: source.into(), step });
    let id = f.id.clone();
    doc.insert_feature(f);
    id
}

/// Model parameter name of a feature's primary value (distance, angle, radius ...).
pub fn primary_parameter(f: &Feature) -> Option<&str> {
    match &f.kind {
        FeatureKind::Extrude { distance, .. } | FeatureKind::Chamfer { distance, .. } => Some(distance),
        FeatureKind::Revolve { angle, .. } => Some(angle),
        FeatureKind::Fillet { radius, .. } => Some(radius),
        FeatureKind::Plane { offset, .. } => Some(offset),
        FeatureKind::Hole { diameter, .. } => Some(diameter),
        _ => None,
    }
}
