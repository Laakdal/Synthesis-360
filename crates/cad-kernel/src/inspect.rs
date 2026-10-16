//! INSPECT panel geometry: minimum distance and angles (Measure), interference between bodies
//! and centre of mass. Exact B-rep evaluations through Open CASCADE (BRepExtrema, BRepGProp,
//! boolean common).

use crate::body::{Body, BodyKind};
use crate::frame::{cross, dot, len, normalize, V3};
use crate::occ::{KResult, KernelError, Shape};
use opencascade_sys::cad360 as ffi;

/// Closest points between two shapes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinDistance {
    pub distance: f64,
    /// Closest point on the first shape.
    pub a: V3,
    /// Closest point on the second shape.
    pub b: V3,
    /// One solid lies inside the other (the distance is 0).
    pub inside: bool,
}

/// True minimum distance between two shapes (faces, edges, vertices, solids).
pub fn min_distance(a: &Shape, b: &Shape) -> KResult<MinDistance> {
    let mut o = [0.0; 8];
    if !ffi::cad_min_distance(a.inner(), b.inner(), &mut o) {
        let e = ffi::cad_last_error();
        return Err(KernelError(if e.is_empty() { "The minimum distance could not be computed".into() } else { e }));
    }
    Ok(MinDistance { distance: o[0], a: [o[1], o[2], o[3]], b: [o[4], o[5], o[6]], inside: o[7] > 0.5 })
}

/// Mass properties at unit density.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassProps {
    pub volume: f64,
    pub area: f64,
    /// Centre of mass of the volume (of the surface for a shape without volume).
    pub centroid: V3,
}

pub fn mass_props(s: &Shape) -> Option<MassProps> {
    let mut o = [0.0; 5];
    ffi::cad_mass_props(s.inner(), &mut o).then_some(MassProps { volume: o[0], area: o[1], centroid: [o[2], o[3], o[4]] })
}

/// Bodies that have a volume (solids and closed mesh-converted solids); surfaces and
/// triangulation-only meshes have no meaningful mass or interference volume.
pub fn is_solid(b: &Body) -> bool {
    b.kind == BodyKind::Solid
}

/// Combined centre of mass of solid bodies (volume-weighted, uniform density). `None` when no
/// body has a volume.
pub fn center_of_mass(bodies: &[&Body]) -> Option<MassProps> {
    let (mut vol, mut area, mut m) = (0.0, 0.0, [0.0; 3]);
    for b in bodies.iter().filter(|b| is_solid(b)) {
        let Some(p) = mass_props(&b.shape) else { continue };
        vol += p.volume;
        area += p.area;
        for k in 0..3 {
            m[k] += p.centroid[k] * p.volume;
        }
    }
    (vol > 1e-12).then(|| MassProps { volume: vol, area, centroid: [m[0] / vol, m[1] / vol, m[2] / vol] })
}

/// One interfering pair (indices into the checked list).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interference {
    pub a: usize,
    pub b: usize,
    /// Volume shared by the two bodies.
    pub volume: f64,
}

fn boxes_overlap(p: &[f64; 6], q: &[f64; 6], tol: f64) -> bool {
    (0..3).all(|k| p[k] <= q[k + 3] + tol && q[k] <= p[k + 3] + tol)
}

/// Every pair of solid bodies whose shared volume exceeds `min_volume` (pairs whose bounding
/// boxes are apart are skipped without a boolean). Pairs are in (a, b) order with a < b.
pub fn interference(bodies: &[&Body], min_volume: f64) -> Vec<Interference> {
    interference_check(bodies, min_volume).pairs
}

/// Result of an interference check.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InterferenceCheck {
    pub pairs: Vec<Interference>,
    /// Pairs whose shared volume could not be computed (the boolean failed).
    pub failed: Vec<(usize, usize)>,
}

/// [`interference`] that also reports the pairs it could not evaluate.
pub fn interference_check(bodies: &[&Body], min_volume: f64) -> InterferenceCheck {
    let boxes: Vec<[f64; 6]> = bodies.iter().map(|b| b.topology().bbox).collect();
    let mut out = InterferenceCheck::default();
    for i in 0..bodies.len() {
        for j in i + 1..bodies.len() {
            if !is_solid(bodies[i]) || !is_solid(bodies[j]) || !boxes_overlap(&boxes[i], &boxes[j], 1e-7) {
                continue;
            }
            let Ok(c) = bodies[i].shape.common(&bodies[j].shape) else {
                out.failed.push((i, j));
                continue;
            };
            let v = c.volume().abs();
            if v > min_volume {
                out.pairs.push(Interference { a: i, b: j, volume: v });
            }
        }
    }
    out
}

/// Angle in degrees between two undirected lines (0..=90).
pub fn line_line_angle(u: V3, v: V3) -> f64 {
    let c = (dot(normalize(u), normalize(v))).abs().min(1.0);
    c.acos().to_degrees()
}

/// Angle in degrees between two planes given by their normals (0..=90).
pub fn plane_plane_angle(n1: V3, n2: V3) -> f64 {
    line_line_angle(n1, n2)
}

/// Angle in degrees between a line and a plane (0 = line in the plane, 90 = along the normal).
pub fn line_plane_angle(dir: V3, normal: V3) -> f64 {
    let s = len(cross(normalize(dir), normalize(normal))).min(1.0);
    90.0 - s.asin().to_degrees()
}
