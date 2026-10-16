//! Persistent face/edge references.
//!
//! Fillets, chamfers, holes and face sketches must find "the same" face or edge again after an
//! upstream edit regenerates the body. Full topological naming (as in Fusion's ASM kernel) is not
//! available through OCCT's plain modelling API, so CAD360 stores a geometric descriptor —
//! surface/curve type, direction/normal, adjacent face normals and the position normalised to the
//! body's bounding box — and re-resolves it by best match. Normalised positions are stable under
//! the common edits (a corner edge stays at (1, 1, ·) when the width grows from 60 to 70 mm). If
//! no candidate matches well, or two candidates match equally well, resolution fails and the
//! feature reports a reference error, as Fusion does for lost references ("1 Reference Failures").

use crate::body::{Body, EdgeInfo, FaceInfo};
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

pub fn face_ref(body: &Body, index: usize) -> Option<FaceRef> {
    let t = body.topology();
    let f = t.face_info.get(index)?;
    Some(FaceRef { body: body.id.clone(), surface: f.surface, normal: f.normal, nc: norm_pos(f.centroid, t.bbox), area: f.area })
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
    })
}

fn normals_mismatch(a: &[V3], b: &[V3]) -> f64 {
    if a.len() != b.len() {
        return 0.5;
    }
    a.iter().zip(b).map(|(x, y)| len(sub(*x, *y))).sum::<f64>() / a.len().max(1) as f64
}

const ACCEPT: f64 = 0.35;

fn best(scores: impl Iterator<Item = (usize, f64)>) -> Result<usize, String> {
    let mut v: Vec<(usize, f64)> = scores.collect();
    v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    match v.as_slice() {
        [] => Err("no candidate geometry".into()),
        [(i, s), ..] if *s > ACCEPT => Err(format!("closest candidate #{i} differs too much (score {s:.2})")),
        [(i, s), (_, s2), ..] if (s2 - s).abs() < 1e-6 => Err(format!("ambiguous: several candidates match equally (score {s:.3}), e.g. #{i}")),
        [(i, _), ..] => Ok(*i),
    }
}

/// Finds the edge in `body` that best matches `r`.
pub fn resolve_edge(body: &Body, r: &EdgeRef) -> Result<usize, String> {
    let t = body.topology();
    best(t.edge_info.iter().enumerate().filter(|(_, e)| e.curve == r.curve && e.length > 1e-9).map(|(i, e)| {
        let nm = norm_pos(e.mid, t.bbox);
        let pos = len(sub(nm, r.nm));
        let dir = 1.0 - dot(e.dir, r.dir).abs();
        let fn_ = normals_mismatch(&sorted_normals(adjacent_plane_normals(e, &t.face_info)), &r.face_normals);
        let lenr = ((e.length / r.length.max(1e-9)).ln()).abs();
        (i, pos + 0.6 * dir + 0.3 * fn_ + 0.05 * lenr)
    }))
}

/// Finds the face in `body` that best matches `r`.
pub fn resolve_face(body: &Body, r: &FaceRef) -> Result<usize, String> {
    let t = body.topology();
    best(t.face_info.iter().enumerate().filter(|(_, f)| f.surface == r.surface).map(|(i, f)| {
        let pos = len(sub(norm_pos(f.centroid, t.bbox), r.nc));
        let n = 1.0 - dot(f.normal, r.normal);
        let a = ((f.area / r.area.max(1e-9)).ln()).abs();
        (i, pos + 1.0 * n + 0.05 * a)
    }))
}

pub fn edge_is_line(r: &EdgeRef) -> bool {
    r.curve == CurveKind::Line
}
