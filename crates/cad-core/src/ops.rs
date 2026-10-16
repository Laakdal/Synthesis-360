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

/// Creates a feature whose kind holds several model parameters: `kind` is built with empty
/// parameter names, then `assign` receives the new parameters (in `values` order).
fn with_params(doc: &mut Document, kind: FeatureKind, values: &[(&str, Quantity)], assign: impl FnOnce(&mut FeatureKind, Vec<String>)) -> FeatureId {
    let mut f = new_feature(doc, kind);
    let id = f.id.clone();
    let names: Vec<String> = values.iter().map(|(e, q)| doc.add_model_parameter(e, *q, &id)).collect();
    assign(&mut f.kind, names);
    doc.insert_feature(f);
    id
}

/// Shell: faces to remove and the inside thickness.
pub fn new_shell(doc: &mut Document, faces: Vec<FaceRef>, thickness: &str) -> FeatureId {
    with_params(doc, FeatureKind::Shell { faces, thickness: String::new() }, &[(thickness, Quantity::Length)], |k, p| {
        if let FeatureKind::Shell { thickness, .. } = k {
            *thickness = p[0].clone();
        }
    })
}

/// Combine (no model parameters).
pub fn new_combine(doc: &mut Document, target: BodyId, tools: Vec<BodyId>, operation: Operation, keep_tools: bool) -> FeatureId {
    let f = new_feature(doc, FeatureKind::Combine { target, tools, operation, keep_tools });
    let id = f.id.clone();
    doc.insert_feature(f);
    id
}

/// Mirror bodies across a plane (no model parameters).
pub fn new_mirror(doc: &mut Document, bodies: Vec<BodyId>, plane: PlaneRef) -> FeatureId {
    let f = new_feature(doc, FeatureKind::MirrorBodies { bodies, plane });
    let id = f.id.clone();
    doc.insert_feature(f);
    id
}

/// Rectangular pattern of bodies. `second` = (axis, quantity, distance) of the optional second
/// direction; its parameters are always created ("1" / "10 mm" when unused) so the dialog can
/// switch it on later.
pub fn new_rectangular_pattern(
    doc: &mut Document,
    bodies: Vec<BodyId>,
    axis1: OriginAxis,
    quantity1: &str,
    distance1: &str,
    second: Option<(OriginAxis, &str, &str)>,
    join: bool,
) -> FeatureId {
    let (axis2, q2, d2) = match second {
        Some((a, q, d)) => (Some(a), q, d),
        None => (None, "1", "10 mm"),
    };
    let kind = FeatureKind::RectangularPattern { bodies, axis1, quantity1: String::new(), distance1: String::new(), axis2, quantity2: String::new(), distance2: String::new(), join };
    with_params(doc, kind, &[(quantity1, Quantity::Unitless), (distance1, Quantity::Length), (q2, Quantity::Unitless), (d2, Quantity::Length)], |k, p| {
        if let FeatureKind::RectangularPattern { quantity1, distance1, quantity2, distance2, .. } = k {
            *quantity1 = p[0].clone();
            *distance1 = p[1].clone();
            *quantity2 = p[2].clone();
            *distance2 = p[3].clone();
        }
    })
}

/// Circular pattern of bodies about an origin axis.
pub fn new_circular_pattern(doc: &mut Document, bodies: Vec<BodyId>, axis: OriginAxis, quantity: &str, angle: &str, join: bool) -> FeatureId {
    let kind = FeatureKind::CircularPattern { bodies, axis, quantity: String::new(), angle: String::new(), join };
    with_params(doc, kind, &[(quantity, Quantity::Unitless), (angle, Quantity::Angle)], |k, p| {
        if let FeatureKind::CircularPattern { quantity, angle, .. } = k {
            *quantity = p[0].clone();
            *angle = p[1].clone();
        }
    })
}

/// Move/Copy bodies: rotation `angle` about origin axis `axis`, then translation (dx, dy, dz).
#[allow(clippy::too_many_arguments)]
pub fn new_move(doc: &mut Document, bodies: Vec<BodyId>, dx: &str, dy: &str, dz: &str, axis: OriginAxis, angle: &str, copy: bool) -> FeatureId {
    let kind = FeatureKind::MoveBodies { bodies, dx: String::new(), dy: String::new(), dz: String::new(), axis, angle: String::new(), copy };
    with_params(doc, kind, &[(dx, Quantity::Length), (dy, Quantity::Length), (dz, Quantity::Length), (angle, Quantity::Angle)], |k, p| {
        if let FeatureKind::MoveBodies { dx, dy, dz, angle, .. } = k {
            *dx = p[0].clone();
            *dy = p[1].clone();
            *dz = p[2].clone();
            *angle = p[3].clone();
        }
    })
}

/// Joint between the components of `a` and `b` (the assembly solve picks the moving one); `angle` and `offset` become
/// model parameters (editable in Change Parameters, which drives the motion parametrically).
pub fn new_joint(doc: &mut Document, a: JointOrigin, b: JointOrigin, kind: JointKind, angle: &str, offset: &str, flip: bool) -> FeatureId {
    let k = FeatureKind::Joint { a, b, kind, angle: String::new(), offset: String::new(), flip };
    with_params(doc, k, &[(angle, Quantity::Angle), (offset, Quantity::Length)], |k, p| {
        if let FeatureKind::Joint { angle, offset, .. } = k {
            *angle = p[0].clone();
            *offset = p[1].clone();
        }
    })
}

/// ASSEMBLE > New Component: creates "ComponentN", moves the given bodies into it and activates
/// it. Bodies move by reassigning the component of the feature that created them, so every body
/// created by that feature (`{fid}`, `{fid}~k`) moves along.
pub fn new_component(doc: &mut Document, bodies: &[BodyId]) -> ComponentId {
    let id = doc.add_component();
    for b in bodies {
        let owner = b.split([':', '~']).next().unwrap_or(b).to_string();
        if let Some(f) = doc.feature_mut(&owner) {
            f.component = Some(id.clone());
        }
    }
    doc.active_component = Some(id.clone());
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
        FeatureKind::Shell { thickness, .. } => Some(thickness),
        FeatureKind::RectangularPattern { quantity1, .. } => Some(quantity1),
        FeatureKind::CircularPattern { quantity, .. } => Some(quantity),
        FeatureKind::MoveBodies { dx, .. } => Some(dx),
        FeatureKind::Joint { kind: JointKind::Slider, offset, .. } => Some(offset),
        FeatureKind::Joint { angle, .. } => Some(angle),
        FeatureKind::SurfaceExtrude { distance, .. } | FeatureKind::OffsetSurface { distance, .. } => Some(distance),
        FeatureKind::SurfaceRevolve { angle, .. } => Some(angle),
        FeatureKind::Thicken { thickness, .. } => Some(thickness),
        FeatureKind::Stitch { tolerance, .. } => Some(tolerance),
        _ => None,
    }
}

// ===== surface =====

/// SURFACE ▸ Extrude: sketch curves (entity ids) extruded into a surface body.
pub fn new_surface_extrude(doc: &mut Document, sketch: &str, curves: Vec<String>, distance: &str, direction: ExtentDirection) -> FeatureId {
    let kind = FeatureKind::SurfaceExtrude { sketch: sketch.into(), curves, distance: String::new(), direction };
    with_params(doc, kind, &[(distance, Quantity::Length)], |k, p| {
        if let FeatureKind::SurfaceExtrude { distance, .. } = k {
            *distance = p[0].clone();
        }
    })
}

/// SURFACE ▸ Revolve: sketch curves revolved into a surface body.
pub fn new_surface_revolve(doc: &mut Document, sketch: &str, curves: Vec<String>, axis: RevolveAxis, angle: &str) -> FeatureId {
    let kind = FeatureKind::SurfaceRevolve { sketch: sketch.into(), curves, axis, angle: String::new() };
    with_params(doc, kind, &[(angle, Quantity::Angle)], |k, p| {
        if let FeatureKind::SurfaceRevolve { angle, .. } = k {
            *angle = p[0].clone();
        }
    })
}

/// SURFACE ▸ Patch of closed sketch profiles and/or a closed loop of body edges.
pub fn new_patch(doc: &mut Document, sketch: Option<FeatureId>, profiles: Vec<ProfileRef>, edges: Vec<EdgeRef>) -> FeatureId {
    let f = new_feature(doc, FeatureKind::Patch { sketch, profiles, edges });
    let id = f.id.clone();
    doc.insert_feature(f);
    id
}

/// SURFACE ▸ Stitch of surface bodies with a sewing tolerance.
pub fn new_stitch(doc: &mut Document, bodies: Vec<BodyId>, tolerance: &str) -> FeatureId {
    with_params(doc, FeatureKind::Stitch { bodies, tolerance: String::new() }, &[(tolerance, Quantity::Length)], |k, p| {
        if let FeatureKind::Stitch { tolerance, .. } = k {
            *tolerance = p[0].clone();
        }
    })
}

/// SURFACE ▸ Offset: faces offset into a new surface body.
pub fn new_offset_surface(doc: &mut Document, faces: Vec<FaceRef>, distance: &str) -> FeatureId {
    with_params(doc, FeatureKind::OffsetSurface { faces, distance: String::new() }, &[(distance, Quantity::Length)], |k, p| {
        if let FeatureKind::OffsetSurface { distance, .. } = k {
            *distance = p[0].clone();
        }
    })
}

/// SURFACE ▸ Thicken: faces (of surface or solid bodies) thickened into a solid.
pub fn new_thicken(doc: &mut Document, faces: Vec<FaceRef>, thickness: &str, symmetric: bool) -> FeatureId {
    with_params(doc, FeatureKind::Thicken { faces, thickness: String::new(), symmetric }, &[(thickness, Quantity::Length)], |k, p| {
        if let FeatureKind::Thicken { thickness, .. } = k {
            *thickness = p[0].clone();
        }
    })
}

/// INSERT ▸ Insert Mesh: STL bytes embedded (base64) in the document.
pub fn new_insert_mesh(doc: &mut Document, source: &str, stl: &[u8], to_brep: bool) -> FeatureId {
    let f = new_feature(doc, FeatureKind::InsertMesh { source: source.into(), data: encode_base64(stl), to_brep });
    let id = f.id.clone();
    doc.insert_feature(f);
    id
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 (RFC 4648, with padding) of `bytes`.
pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Decodes standard base64 (whitespace ignored).
pub fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    let mut val = [255u8; 256];
    for (i, c) in B64.iter().enumerate() {
        val[*c as usize] = i as u8;
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c == b'=' {
            break;
        }
        let v = val[c as usize];
        if v == 255 {
            return Err(format!("invalid base64 character {:?}", c as char));
        }
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod surface_tests {
    use super::*;

    #[test]
    fn base64_round_trip() {
        for s in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0u8, 255, 128, 7, 3][..]] {
            let e = encode_base64(s);
            assert_eq!(decode_base64(&e).unwrap(), s, "{e}");
        }
        assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert!(decode_base64("Zm9v!").is_err());
    }

    #[test]
    fn surface_constructors_create_parameters() {
        let mut d = Document::new("t");
        let sk = new_sketch(&mut d, PlaneRef::Origin { plane: OriginPlane::XY });
        let e = new_surface_extrude(&mut d, &sk, vec!["l1".into()], "15 mm", ExtentDirection::Normal);
        let f = d.feature(&e).unwrap();
        assert_eq!(f.type_label(), "SurfaceExtrude");
        assert_eq!(d.parameter_values().values[primary_parameter(f).unwrap()], 15.0);
        let t = new_thicken(&mut d, vec![], "2 mm", true);
        assert_eq!(d.parameter_values().values[primary_parameter(d.feature(&t).unwrap()).unwrap()], 2.0);
        let m = new_insert_mesh(&mut d, "a.stl", b"solid x", false);
        match &d.feature(&m).unwrap().kind {
            FeatureKind::InsertMesh { data, .. } => assert_eq!(decode_base64(data).unwrap(), b"solid x"),
            _ => unreachable!(),
        }
    }
}
