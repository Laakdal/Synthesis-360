//! Surface and mesh features (SURFACE tab: Extrude/Revolve as surfaces, Patch, Stitch, Offset,
//! Thicken; INSERT ▸ Insert Mesh). Bodies produced here use `BodyKind::Surface` / `BodyKind::Mesh`
//! unless they are closed solids.
//!
//! Child module of `recompute` (it works on the private evaluation `State`).
//!
//! Body identity follows `recompute_more`: bodies a feature creates get `{feature_id}`,
//! `{feature_id}~1`, ... with `created_by = feature_id`. Stitch keeps the id of its first input
//! body (the stitched result replaces it at its place in the body list; the other inputs are
//! consumed). Thicken consumes a surface body whose every face it thickens; faces of solids are
//! thickened into new bodies and the solid stays.

use super::{profile_face, resolve_profiles, sketch_of, Health, State};
use crate::body::{new_body_with, Body, BodyKind};
use crate::frame::{normalize, sub, V3};
use crate::occ::{Shape, FACE, SOLID};
use crate::{mesh, refs, surface};
use cad_core::{BodyId, EdgeRef, ExtentDirection, FaceRef, Feature, FeatureKind, OriginAxis, RevolveAxis};

pub(super) fn evaluate(f: &Feature, state: &mut State, vals: &dyn Fn(&str) -> f64) -> Result<Health, String> {
    match &f.kind {
        FeatureKind::SurfaceExtrude { sketch, curves, distance, direction } => surface_extrude(f, state, sketch, curves, vals(distance), *direction),
        FeatureKind::SurfaceRevolve { sketch, curves, axis, angle } => surface_revolve(f, state, sketch, curves, axis, vals(angle)),
        FeatureKind::Patch { sketch, profiles, edges } => patch(f, state, sketch.as_deref(), profiles, edges),
        FeatureKind::Stitch { bodies, tolerance } => stitch(f, state, bodies, vals(tolerance)),
        FeatureKind::OffsetSurface { faces, distance } => offset(f, state, faces, vals(distance)),
        FeatureKind::Thicken { faces, thickness, symmetric } => thicken(f, state, faces, vals(thickness), *symmetric),
        FeatureKind::InsertMesh { data, to_brep, .. } => insert_mesh(f, state, data, *to_brep),
        _ => Ok(Health::Healthy),
    }
}

fn new_id(fid: &str, k: usize) -> BodyId {
    if k == 0 { fid.to_string() } else { format!("{fid}~{k}") }
}

/// Bodies for result pieces: solids become Solid bodies, anything else Surface bodies.
fn bodies_of(fid: &str, pieces: Vec<Shape>, start: usize) -> Vec<Body> {
    pieces
        .into_iter()
        .filter(|p| !p.subshapes(FACE).is_empty())
        .enumerate()
        .map(|(k, p)| {
            let kind = if p.shape_type() == SOLID { BodyKind::Solid } else { BodyKind::Surface };
            new_body_with(new_id(fid, start + k), fid.into(), p, kind, Vec::new())
        })
        .collect()
}

fn surface_body(fid: &str, shape: Shape) -> Body {
    new_body_with(fid.into(), fid.into(), shape, BodyKind::Surface, Vec::new())
}

/// Sketch chains of the selected curves (construction curves and lost ids are skipped).
fn selected_chains(sr: &super::SketchResult, curves: &[String], skip: Option<&str>) -> Result<(Vec<surface::Chain>, usize), String> {
    if curves.is_empty() {
        return Err("No curves selected".into());
    }
    let ids: Vec<String> = curves.iter().filter(|c| Some(c.as_str()) != skip && sr.solved.entity(c).is_some_and(|e| e.is_curve())).cloned().collect();
    let lost = curves.len() - ids.len() - curves.iter().filter(|c| Some(c.as_str()) == skip).count();
    if ids.is_empty() {
        return Err("The selected curves no longer exist in the sketch. Edit this feature and select new curves.".into());
    }
    Ok((surface::chains_of(&sr.solved, &ids), lost))
}

fn lost_warning(lost: usize) -> Health {
    if lost > 0 { Health::Warning(format!("{lost} reference failure(s): some selected curves no longer exist.")) } else { Health::Healthy }
}

fn surface_extrude(f: &Feature, state: &mut State, sketch: &str, curves: &[String], d: f64, dir: ExtentDirection) -> Result<Health, String> {
    let sr = sketch_of(state, sketch)?.clone();
    if d.abs() < 1e-9 {
        return Err("Distance must not be zero.".into());
    }
    let (chains, lost) = selected_chains(&sr, curves, None)?;
    let (start, length) = match dir {
        ExtentDirection::Normal => (0.0, d),
        ExtentDirection::Reverse => (0.0, -d),
        ExtentDirection::Symmetric => (-d.abs() / 2.0, d.abs()),
    };
    let frame = sr.frame.offset(start);
    let wires = surface::chain_wires(&frame, &chains).map_err(|e| e.0)?;
    let n = frame.n;
    let s = wires.prism([n[0] * length, n[1] * length, n[2] * length]).map_err(|e| e.0)?;
    state.bodies.push(surface_body(&f.id, s));
    Ok(lost_warning(lost))
}

fn axis_of(sr: &super::SketchResult, axis: &RevolveAxis) -> Result<(V3, V3), String> {
    Ok(match axis {
        RevolveAxis::SketchLine { entity } => {
            let (p, q) = sr.solved.line_points(entity).ok_or("The revolve axis line no longer exists in the sketch")?;
            let (p3, q3) = (sr.frame.to_world(p), sr.frame.to_world(q));
            (p3, normalize(sub(q3, p3)))
        }
        RevolveAxis::Origin { axis } => (
            [0.0; 3],
            match axis {
                OriginAxis::X => [1.0, 0.0, 0.0],
                OriginAxis::Y => [0.0, 1.0, 0.0],
                OriginAxis::Z => [0.0, 0.0, 1.0],
            },
        ),
    })
}

fn surface_revolve(f: &Feature, state: &mut State, sketch: &str, curves: &[String], axis: &RevolveAxis, angle_deg: f64) -> Result<Health, String> {
    let sr = sketch_of(state, sketch)?.clone();
    if angle_deg.abs() < 1e-9 {
        return Err("Angle must not be zero.".into());
    }
    let skip = match axis {
        RevolveAxis::SketchLine { entity } => Some(entity.as_str()),
        RevolveAxis::Origin { .. } => None,
    };
    let (chains, lost) = selected_chains(&sr, curves, skip)?;
    if chains.is_empty() {
        return Err("No curves selected (the axis line is not revolved).".into());
    }
    let (o, a) = axis_of(&sr, axis)?;
    let wires = surface::chain_wires(&sr.frame, &chains).map_err(|e| e.0)?;
    let s = wires.revolve(o, a, angle_deg.to_radians().clamp(-std::f64::consts::TAU, std::f64::consts::TAU)).map_err(|e| e.0)?;
    if s.subshapes(FACE).is_empty() {
        return Err("The curves lie on the revolve axis; nothing to revolve.".into());
    }
    state.bodies.push(surface_body(&f.id, s));
    Ok(lost_warning(lost))
}

fn patch(f: &Feature, state: &mut State, sketch: Option<&str>, profiles: &[cad_core::ProfileRef], edges: &[EdgeRef]) -> Result<Health, String> {
    if profiles.is_empty() && edges.is_empty() {
        return Err("Select closed sketch profiles or a closed loop of edges.".into());
    }
    let mut faces: Vec<Shape> = Vec::new();
    if !profiles.is_empty() {
        let sk = sketch.ok_or("The patch profiles have no sketch")?;
        let sr = sketch_of(state, sk)?.clone();
        for p in resolve_profiles(&sr, profiles)? {
            faces.push(profile_face(&sr.frame, p).map_err(|e| e.0)?);
        }
    }
    let mut warn = None;
    if !edges.is_empty() {
        let mut sel: Vec<Shape> = Vec::new();
        let mut lost = 0;
        for r in edges {
            let Some(b) = state.bodies.iter().find(|b| b.id == r.body) else {
                lost += 1;
                continue;
            };
            match refs::resolve_edge(b, r) {
                Ok(i) => sel.push(b.topology().edges[i].clone()),
                Err(_) => lost += 1,
            }
        }
        if sel.is_empty() {
            return Err(format!("{lost} reference failure(s): the boundary edges no longer exist. Edit this feature and reselect the edges."));
        }
        if lost > 0 {
            warn = Some(format!("{lost} reference failure(s): some boundary edges no longer exist."));
        }
        let refs: Vec<&Shape> = sel.iter().collect();
        faces.push(surface::fill(&refs).map_err(|e| e.0)?);
    }
    for (k, face) in faces.into_iter().enumerate() {
        state.bodies.push(new_body_with(new_id(&f.id, k), f.id.clone(), face, BodyKind::Surface, Vec::new()));
    }
    Ok(warn.map(Health::Warning).unwrap_or(Health::Healthy))
}

fn stitch(f: &Feature, state: &mut State, bodies: &[BodyId], tol: f64) -> Result<Health, String> {
    if bodies.is_empty() {
        return Err("Select the surface bodies to stitch.".into());
    }
    if tol <= 0.0 {
        return Err("Tolerance must be greater than zero.".into());
    }
    let inputs: Vec<Body> = bodies.iter().filter_map(|id| state.bodies.iter().find(|b| b.id == *id).cloned()).collect();
    if inputs.is_empty() {
        return Err("The bodies to stitch no longer exist. Edit this feature and select the bodies again.".into());
    }
    if let Some(m) = inputs.iter().find(|b| b.kind == BodyKind::Mesh) {
        return Err(format!("{} is a mesh body; convert it to B-rep before stitching.", m.id));
    }
    let shapes: Vec<&Shape> = inputs.iter().map(|b| &b.shape).collect();
    let all = Shape::compound(&shapes).map_err(|e| e.0)?;
    let pieces = surface::sew(&all, tol, true).map_err(|e| e.0)?;
    if pieces.is_empty() {
        return Err("Stitch produced no result.".into());
    }
    let first = inputs[0].clone();
    let mut result: Vec<Body> = Vec::new();
    for (k, p) in pieces.into_iter().enumerate() {
        let kind = if p.shape_type() == SOLID { BodyKind::Solid } else { BodyKind::Surface };
        result.push(if k == 0 { new_body_with(first.id.clone(), first.created_by.clone(), p, kind, Vec::new()) } else { new_body_with(new_id(&f.id, k - 1), f.id.clone(), p, kind, Vec::new()) });
    }
    let consumed: Vec<&str> = inputs.iter().map(|b| b.id.as_str()).collect();
    let mut next: Vec<Body> = Vec::new();
    for b in &state.bodies {
        if b.id == first.id {
            next.extend(result.iter().cloned());
        } else if !consumed.contains(&b.id.as_str()) {
            next.push(b.clone());
        }
    }
    state.bodies = next;
    let lost = bodies.len() - inputs.len();
    let solid = result.iter().any(|b| b.kind == BodyKind::Solid);
    Ok(if lost > 0 {
        Health::Warning(format!("{lost} body reference(s) lost: those bodies no longer exist."))
    } else if inputs.len() > 1 && result.len() == inputs.len() && !solid {
        Health::Warning("No edges were stitched within the tolerance.".into())
    } else {
        Health::Healthy
    })
}

/// Resolves face references, grouped by body (body order of the model). Returns the groups and
/// the number of lost references.
fn faces_by_body(state: &State, faces: &[FaceRef]) -> (Vec<(Body, Vec<usize>)>, usize) {
    let mut groups: Vec<(Body, Vec<usize>)> = Vec::new();
    let mut lost = 0;
    for r in faces {
        let Some(b) = state.bodies.iter().find(|b| b.id == r.body) else {
            lost += 1;
            continue;
        };
        match refs::resolve_face(b, r) {
            Ok(i) => match groups.iter_mut().find(|(g, _)| g.id == b.id) {
                Some((_, v)) => {
                    if !v.contains(&i) {
                        v.push(i)
                    }
                }
                None => groups.push((b.clone(), vec![i])),
            },
            Err(_) => lost += 1,
        }
    }
    (groups, lost)
}

/// The selected faces of one body as one shape: the face itself, or the faces sewn into a shell.
fn faces_shape(b: &Body, idx: &[usize]) -> Result<Shape, String> {
    let topo = b.topology();
    if idx.len() == 1 {
        return Ok(topo.faces[idx[0]].clone());
    }
    let sel: Vec<&Shape> = idx.iter().map(|&i| &topo.faces[i]).collect();
    let c = Shape::compound(&sel).map_err(|e| e.0)?;
    let pieces = surface::sew(&c, 1e-4, false).map_err(|e| e.0)?;
    if pieces.len() == 1 {
        Ok(pieces.into_iter().next().unwrap())
    } else {
        let refs: Vec<&Shape> = pieces.iter().collect();
        Shape::compound(&refs).map_err(|e| e.0)
    }
}

fn lost_faces(lost: usize, total: usize) -> Result<Option<String>, String> {
    if lost == total {
        return Err(format!("{lost} reference failure(s): the selected faces no longer exist. Edit this feature and reselect the faces."));
    }
    Ok((lost > 0).then(|| format!("{lost} reference failure(s): some selected faces no longer exist.")))
}

fn offset(f: &Feature, state: &mut State, faces: &[FaceRef], d: f64) -> Result<Health, String> {
    if faces.is_empty() {
        return Err("No faces selected.".into());
    }
    if d.abs() < 1e-9 {
        return Err("Distance must not be zero.".into());
    }
    let (groups, lost) = faces_by_body(state, faces);
    let warn = lost_faces(lost, faces.len())?;
    if groups.iter().any(|(b, _)| b.kind == BodyKind::Mesh) {
        return Err("Mesh bodies cannot be offset; convert them to B-rep first.".into());
    }
    let mut out = Vec::new();
    for (b, idx) in &groups {
        let s = faces_shape(b, idx)?;
        out.push(surface::offset_surface(&s, d).map_err(|e| e.0)?);
    }
    let n = state.bodies.len();
    for (k, s) in out.into_iter().enumerate() {
        state.bodies.push(new_body_with(new_id(&f.id, k), f.id.clone(), s, BodyKind::Surface, Vec::new()));
    }
    if state.bodies.len() == n {
        return Err("The offset produced no surface.".into());
    }
    Ok(warn.map(Health::Warning).unwrap_or(Health::Healthy))
}

fn thicken(f: &Feature, state: &mut State, faces: &[FaceRef], t: f64, symmetric: bool) -> Result<Health, String> {
    if faces.is_empty() {
        return Err("No faces selected.".into());
    }
    if t.abs() < 1e-9 {
        return Err("Thickness must not be zero.".into());
    }
    let (groups, lost) = faces_by_body(state, faces);
    let warn = lost_faces(lost, faces.len())?;
    if groups.iter().any(|(b, _)| b.kind == BodyKind::Mesh) {
        return Err("Mesh bodies cannot be thickened; convert them to B-rep first.".into());
    }
    let mut created: Vec<Body> = Vec::new();
    let mut consumed: Vec<BodyId> = Vec::new();
    for (b, idx) in &groups {
        let mut s = faces_shape(b, idx)?;
        if symmetric {
            s = surface::offset_surface(&s, -t / 2.0).map_err(|e| format!("Symmetric thicken: {}", e.0))?;
        }
        let solid = surface::thicken(&s, t).map_err(|e| e.0)?;
        let start = created.len();
        let pieces: Vec<Shape> = match solid.subshapes(SOLID) {
            v if v.is_empty() => vec![solid],
            v => v,
        };
        created.extend(bodies_of(&f.id, pieces, start));
        if b.kind == BodyKind::Surface && idx.len() == b.topology().faces.len() {
            consumed.push(b.id.clone());
        }
    }
    if created.is_empty() {
        return Err("Thicken produced no body.".into());
    }
    if created.iter().any(|b| b.kind != BodyKind::Solid) {
        return Err("Thicken did not produce a closed solid with this thickness.".into());
    }
    state.bodies.retain(|b| !consumed.contains(&b.id));
    state.bodies.extend(created);
    Ok(warn.map(Health::Warning).unwrap_or(Health::Healthy))
}

fn insert_mesh(f: &Feature, state: &mut State, data: &str, to_brep: bool) -> Result<Health, String> {
    let bytes = cad_core::ops::decode_base64(data).map_err(|e| format!("The embedded mesh data is corrupt ({e})."))?;
    let tris = mesh::parse_stl(&bytes)?;
    if !to_brep {
        let face = surface::mesh_face(&tris).map_err(|e| e.0)?;
        state.bodies.push(new_body_with(f.id.clone(), f.id.clone(), face, BodyKind::Mesh, Vec::new()));
        return Ok(Health::Healthy);
    }
    // sewing tolerance relative to the mesh size (STL stores single-precision coordinates)
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in tris.chunks_exact(3) {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let diag = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt();
    let tol = (diag * 1e-6).clamp(1e-6, 1e-2);
    let pieces = surface::mesh_to_brep(&tris, tol).map_err(|e| e.0)?;
    let bodies = bodies_of(&f.id, pieces, 0);
    if bodies.is_empty() {
        return Err("The mesh could not be converted to B-rep.".into());
    }
    let open = bodies.iter().any(|b| b.kind != BodyKind::Solid);
    state.bodies.extend(bodies);
    Ok(if open { Health::Warning("The mesh is not closed: (part of) it was converted to a surface body, not a solid.".into()) } else { Health::Healthy })
}
