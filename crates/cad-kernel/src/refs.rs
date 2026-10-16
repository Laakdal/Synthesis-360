//! Persistent face/edge references.
//!
//! Fillets, chamfers, holes and face sketches must find "the same" face or edge again after an
//! upstream edit regenerates the body. References carry two descriptors:
//!
//! * a generative (history-based) name — `FaceRef::name`, `EdgeRef::face_names` (the names of the
//!   edge's two faces), see [`crate::naming`]. Names do not depend on where a face is, so they
//!   survive edits that grow the bounding box, add faces next to the referenced one or reorder
//!   candidates;
//! * a geometric descriptor — surface/curve type, direction/normal, adjacent face normals and the
//!   position normalised to the body's bounding box.
//!
//! Resolution restricts the candidates to faces/edges with the same name and uses the geometric
//! score only to choose among them (a face split into several pieces keeps its name). References
//! without names (documents saved before naming) resolve by geometry alone, as before. A named
//! reference whose name no longer exists falls back to geometry and is reported as
//! [`Via::GeometryFallback`] so the feature can warn. If no candidate matches well, or two
//! candidates match equally well, resolution fails and the feature reports a reference error, as
//! Fusion does for lost references ("1 Reference Failures").

use crate::body::{Body, EdgeInfo, FaceInfo, Topology};
use crate::frame::{dot, len, sub, V3};
use cad_core::{CurveKind, EdgeRef, FaceRef};

fn norm_pos(p: V3, bbox: [f64; 6]) -> V3 {
    let mut out = [0.0; 3];
    for k in 0..3 {
        let ext = bbox[k + 3] - bbox[k];
        out[k] = if ext.abs() < 1e-9 { 0.5 } else { (p[k] - bbox[k]) / ext };
    }
    out
}

fn sorted_normals(mut v: Vec<V3>) -> Vec<V3> {
    for n in v.iter_mut() {
        for c in n.iter_mut() {
            *c = (*c * 1e6).round() / 1e6 + 0.0;
        }
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v
}

fn adjacent_plane_normals(e: &EdgeInfo, faces: &[FaceInfo]) -> Vec<V3> {
    e.faces
        .iter()
        .filter(|&&i| i >= 0)
        .filter_map(|&i| faces.get(i as usize))
        .filter(|f| f.surface == cad_core::SurfaceKind::Plane)
        .map(|f| f.normal)
        .collect()
}

/// Sorted names of the faces adjacent to an edge (its generative name).
fn edge_face_names(body: &Body, e: &EdgeInfo) -> Vec<String> {
    let mut n: Vec<String> = e.faces.iter().filter(|&&f| f >= 0).filter_map(|&f| body.face_name(f as usize).map(str::to_string)).collect();
    n.sort();
    n
}

pub fn face_ref(body: &Body, index: usize) -> Option<FaceRef> {
    let t = body.topology();
    let f = t.face_info.get(index)?;
    Some(FaceRef { body: body.id.clone(), surface: f.surface, normal: f.normal, nc: norm_pos(f.centroid, t.bbox), area: f.area, name: body.face_name(index).map(str::to_string) })
}

pub fn edge_ref(body: &Body, index: usize) -> Option<EdgeRef> {
    let t = body.topology();
    let e = t.edge_info.get(index)?;
    Some(EdgeRef {
        body: body.id.clone(),
        curve: e.curve,
        dir: e.dir,
        nm: norm_pos(e.mid, t.bbox),
        length: e.length,
        face_normals: sorted_normals(adjacent_plane_normals(e, &t.face_info)),
        face_names: edge_face_names(body, e),
    })
}

fn normals_mismatch(a: &[V3], b: &[V3]) -> f64 {
    if a.len() != b.len() {
        return 0.5;
    }
    a.iter().zip(b).map(|(x, y)| len(sub(*x, *y))).sum::<f64>() / a.len().max(1) as f64
}

const ACCEPT: f64 = 0.35;

fn sorted(scores: impl Iterator<Item = (usize, f64)>) -> Vec<(usize, f64)> {
    let mut v: Vec<(usize, f64)> = scores.collect();
    v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    v
}

/// Pure geometric match: the best candidate must be close enough and unambiguous.
fn best(scores: impl Iterator<Item = (usize, f64)>) -> Result<usize, String> {
    match sorted(scores).as_slice() {
        [] => Err("no candidate geometry".into()),
        [(i, s), ..] if *s > ACCEPT => Err(format!("closest candidate #{i} differs too much (score {s:.2})")),
        [(i, s), (_, s2), ..] if (s2 - s).abs() < 1e-6 => Err(format!("ambiguous: several candidates match equally (score {s:.3}), e.g. #{i}")),
        [(i, _), ..] => Ok(*i),
    }
}

/// Choice among candidates that carry the referenced name (pieces of a split face): the identity
/// is already established, so there is no acceptance threshold — only a tie is an error.
fn best_named(scores: impl Iterator<Item = (usize, f64)>) -> Result<usize, String> {
    match sorted(scores).as_slice() {
        [] => Err("no candidate geometry".into()),
        [(i, s), (_, s2), ..] if (s2 - s).abs() < 1e-6 => Err(format!("ambiguous: several pieces of the named face match equally (score {s:.3}), e.g. #{i}")),
        [(i, _), ..] => Ok(*i),
    }
}

/// How a reference was resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// By generative name (geometry only chose among same-name pieces).
    Name,
    /// By geometry: the reference has no name (older document).
    Geometry,
    /// By geometry although the reference has a name: no face/edge with that name exists any more.
    GeometryFallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub index: usize,
    pub via: Via,
}

impl Resolved {
    /// The reference has a name that no longer exists and was re-resolved geometrically.
    pub fn fell_back(&self) -> bool {
        self.via == Via::GeometryFallback
    }
}

/// Health text for references that had to be re-resolved by geometry.
pub fn fallback_warning(n: usize) -> String {
    format!("{n} reference(s) re-resolved geometrically: the referenced face or edge no longer exists by name. Check the result and reselect if needed.")
}

fn edge_score(t: &Topology, e: &EdgeInfo, r: &EdgeRef) -> f64 {
    let nm = norm_pos(e.mid, t.bbox);
    let pos = len(sub(nm, r.nm));
    let dir = 1.0 - dot(e.dir, r.dir).abs();
    let fn_ = normals_mismatch(&sorted_normals(adjacent_plane_normals(e, &t.face_info)), &r.face_normals);
    let lenr = ((e.length / r.length.max(1e-9)).ln()).abs();
    pos + 0.6 * dir + 0.3 * fn_ + 0.05 * lenr
}

fn face_score(t: &Topology, f: &FaceInfo, r: &FaceRef) -> f64 {
    let pos = len(sub(norm_pos(f.centroid, t.bbox), r.nc));
    let n = 1.0 - dot(f.normal, r.normal);
    let a = ((f.area / r.area.max(1e-9)).ln()).abs();
    pos + 1.0 * n + 0.05 * a
}

/// Finds the edge in `body` that `r` refers to, and how.
pub fn resolve_edge_ex(body: &Body, r: &EdgeRef) -> Result<Resolved, String> {
    let t = body.topology();
    if !r.face_names.is_empty() {
        let cands: Vec<usize> = t.edge_info.iter().enumerate().filter(|(_, e)| e.length > 1e-9 && edge_face_names(body, e) == r.face_names).map(|(i, _)| i).collect();
        if cands.len() == 1 {
            return Ok(Resolved { index: cands[0], via: Via::Name });
        }
        if !cands.is_empty() {
            let kind = |e: &EdgeInfo| if e.curve == r.curve { 0.0 } else { 1.0 };
            let i = best_named(cands.iter().map(|&i| (i, edge_score(&t, &t.edge_info[i], r) + kind(&t.edge_info[i]))))?;
            return Ok(Resolved { index: i, via: Via::Name });
        }
    }
    let i = best(t.edge_info.iter().enumerate().filter(|(_, e)| e.curve == r.curve && e.length > 1e-9).map(|(i, e)| (i, edge_score(&t, e, r))))?;
    Ok(Resolved { index: i, via: if r.face_names.is_empty() { Via::Geometry } else { Via::GeometryFallback } })
}

/// Finds the face in `body` that `r` refers to, and how.
pub fn resolve_face_ex(body: &Body, r: &FaceRef) -> Result<Resolved, String> {
    let t = body.topology();
    if let Some(name) = r.name.as_deref() {
        let cands: Vec<usize> = (0..t.face_info.len()).filter(|&i| body.face_name(i) == Some(name)).collect();
        if cands.len() == 1 {
            return Ok(Resolved { index: cands[0], via: Via::Name });
        }
        if !cands.is_empty() {
            let kind = |f: &FaceInfo| if f.surface == r.surface { 0.0 } else { 1.0 };
            let i = best_named(cands.iter().map(|&i| (i, face_score(&t, &t.face_info[i], r) + kind(&t.face_info[i]))))?;
            return Ok(Resolved { index: i, via: Via::Name });
        }
    }
    let i = best(t.face_info.iter().enumerate().filter(|(_, f)| f.surface == r.surface).map(|(i, f)| (i, face_score(&t, f, r))))?;
    Ok(Resolved { index: i, via: if r.name.is_none() { Via::Geometry } else { Via::GeometryFallback } })
}

/// Finds the edge in `body` that best matches `r`.
pub fn resolve_edge(body: &Body, r: &EdgeRef) -> Result<usize, String> {
    resolve_edge_ex(body, r).map(|x| x.index)
}

/// Finds the face in `body` that best matches `r`.
pub fn resolve_face(body: &Body, r: &FaceRef) -> Result<usize, String> {
    resolve_face_ex(body, r).map(|x| x.index)
}

pub fn edge_is_line(r: &EdgeRef) -> bool {
    r.curve == CurveKind::Line
}
