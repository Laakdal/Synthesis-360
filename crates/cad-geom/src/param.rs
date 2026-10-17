//! Parameter-space helpers shared by mass properties, validation and tessellation: closed-form
//! inverses of the analytic surfaces, point–surface distance, Gauss–Legendre rules, and the
//! loop walk that turns a face's loops into pcurves (u, v) sampled at chosen curve parameters.
//!
//! Pcurves are derived, not stored: each sample point C(t) of an edge is mapped back to (u, v)
//! with the closed-form inverse (atan2 throughout, so no precision is lost near poles), and
//! d(u, v)/dt comes from solving [S_u S_v]·(du, dv) = C'(t) in the least-squares sense.
//! Periodic coordinates are unwrapped by continuity along the loop, except on seam coedges
//! (edges used twice by the face), whose side is fixed by the material-on-the-left rule: with
//! the natural normal, a u-seam travelled in +v lies at the high end of the u range and one
//! travelled in −v at the low end; a v-seam travelled in +u lies at the low end of v, in −u at
//! the high end; `Face.reversed` swaps the sides. Seamless loops (ASM) are walked by continuity and
//! come back with a non-zero winding, which [`crate::mass`] closes with a virtual seam. NURBS
//! geometry is not supported here.

use crate::topology::Edge;
use crate::{Brep, CoedgeId, Curve, FaceId, GResult, GeomError, LoopId, Surface, P3, V3};
use std::f64::consts::{PI, TAU};
use std::sync::OnceLock;

/// Number of Gauss–Legendre points per sub-interval used by the integrators.
pub const GL_POINTS: usize = 16;

/// Nodes and weights of the n-point Gauss–Legendre rule on [−1, 1] (Newton on P_n, Golub–Welsch
/// accuracy: nodes to ~1 ulp). Exact for polynomials of degree ≤ 2n − 1.
pub fn gauss_legendre(n: usize) -> (Vec<f64>, Vec<f64>) {
    let (mut x, mut w) = (vec![0.0; n], vec![0.0; n]);
    for i in 0..n.div_ceil(2) {
        let mut z = (PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
        let mut dp = 1.0;
        for _ in 0..100 {
            let (mut p0, mut p1) = (1.0, 0.0);
            for j in 0..n {
                let p2 = p1;
                p1 = p0;
                p0 = ((2 * j + 1) as f64 * z * p1 - j as f64 * p2) / (j + 1) as f64;
            }
            dp = n as f64 * (z * p0 - p1) / (z * z - 1.0);
            let dz = p0 / dp;
            z -= dz;
            if dz.abs() < 1e-16 {
                break;
            }
        }
        x[i] = -z;
        x[n - 1 - i] = z;
        w[i] = 2.0 / ((1.0 - z * z) * dp * dp);
        w[n - 1 - i] = w[i];
    }
    (x, w)
}

fn gl16() -> &'static (Vec<f64>, Vec<f64>) {
    static R: OnceLock<(Vec<f64>, Vec<f64>)> = OnceLock::new();
    R.get_or_init(|| gauss_legendre(GL_POINTS))
}

/// Composite Gauss–Legendre nodes `(t, weight)` over [a, b] split into `pieces` equal parts
/// (ascending in t; weights positive when a < b).
pub fn composite_nodes(a: f64, b: f64, pieces: usize) -> Vec<(f64, f64)> {
    let (x, w) = gl16();
    let pieces = pieces.max(1);
    let h = (b - a) / pieces as f64;
    let mut out = Vec::with_capacity(pieces * x.len());
    for k in 0..pieces {
        let (lo, half) = (a + h * k as f64, h / 2.0);
        out.extend(x.iter().zip(w).map(|(xi, wi)| (lo + half * (xi + 1.0), wi * half)));
    }
    out
}

/// Sub-intervals for an angular range: pieces of at most π/8, so the n = 16 rule integrates the
/// trigonometric polynomials met by the analytic primitives to rounding level (the error term
/// of a piece of half-width h for a frequency-k integrand is ~ (kh)^32 / 32!).
pub fn angular_pieces(span: f64) -> usize {
    ((span.abs() / (PI / 8.0)).ceil() as usize).max(1)
}

/// Sub-intervals for integrating along an edge curve.
pub fn edge_pieces(e: &Edge) -> GResult<usize> {
    match &e.curve {
        Some(Curve::Line { .. }) => Ok(2),
        Some(Curve::Circle { .. } | Curve::Ellipse { .. }) => Ok(angular_pieces(e.t1 - e.t0)),
        Some(Curve::Nurbs(_)) => Err(GeomError::Unsupported("NURBS edges in parameter-space integration".into())),
        None => Ok(0),
    }
}

/// Closed-form inverse of an analytic surface: `(u, v)` of the point (assumed on or near the
/// surface). `u` is `None` where it is undefined (sphere pole, cone apex, cylinder axis);
/// angles come from atan2 in (−π, π]. `None` for NURBS.
pub fn inverse(s: &Surface, p: &P3) -> Option<(Option<f64>, f64)> {
    let l = s.frame()?.to_local(*p);
    let rho = l.x.hypot(l.y);
    let scale = l.coords.norm().max(1e-300);
    let u = (rho > 1e-13 * scale).then(|| l.y.atan2(l.x));
    Some(match s {
        Surface::Plane { .. } => (Some(l.x), l.y),
        Surface::Cylinder { .. } | Surface::Cone { .. } => (u, l.z),
        Surface::Sphere { .. } => (u, l.z.atan2(rho)),
        Surface::Torus { major, .. } => (u, l.z.atan2(rho - major)),
        Surface::Nurbs(_) => return None,
    })
}

/// Distance from `p` to an analytic surface (unbounded, full surface); `None` for NURBS.
pub fn surface_distance(s: &Surface, p: &P3) -> Option<f64> {
    let l = s.frame()?.to_local(*p);
    let rho = l.x.hypot(l.y);
    Some(match s {
        Surface::Plane { .. } => l.z.abs(),
        Surface::Cylinder { radius, .. } => (rho - radius).abs(),
        Surface::Cone { radius, half_angle, .. } => {
            // generator through (r, 0) with direction (sin α, cos α) in the (ρ, z) half-plane
            ((rho - radius) * half_angle.cos() - l.z * half_angle.sin()).abs()
        }
        Surface::Sphere { radius, .. } => (l.coords.norm() - radius).abs(),
        Surface::Torus { major, minor, .. } => ((rho - major).hypot(l.z) - minor).abs(),
        Surface::Nurbs(_) => return None,
    })
}

/// Tangent `d` expressed as (du, dv) on the surface at (u, v) (least squares; at a singular
/// point where S_u vanishes, du = 0).
pub fn uv_rates(s: &Surface, u: f64, v: f64, d: &V3) -> [f64; 2] {
    let (su, sv) = s.d1(u, v);
    let (a, b, c) = (su.norm_squared(), su.dot(&sv), sv.norm_squared());
    let (ru, rv) = (su.dot(d), sv.dot(d));
    let det = a * c - b * b;
    if a > 0.0 && det > 1e-20 * a * c {
        [(c * ru - b * rv) / det, (a * rv - b * ru) / det]
    } else if c > 0.0 {
        [0.0, rv / c]
    } else {
        [0.0, 0.0]
    }
}

/// Produces `(t, weight)` sample pairs, ascending in t, for an edge.
pub type EdgeSampler<'a> = dyn Fn(&Edge) -> GResult<Vec<(f64, f64)>> + 'a;

/// One pcurve sample: curve parameter, quadrature weight (for dt), (u, v) and d(u, v)/dt.
#[derive(Debug, Clone, Copy)]
pub struct UvSample {
    pub t: f64,
    pub w: f64,
    pub uv: [f64; 2],
    pub duv: [f64; 2],
}

/// The pcurve of one coedge: samples in travel order; `sign` is −1 for a reversed use (the
/// samples then run from t1 down to t0, so ∫ along the coedge = sign · Σ w · …).
#[derive(Debug, Clone)]
pub struct CoedgeTrack {
    pub coedge: CoedgeId,
    pub sign: f64,
    pub degenerate: bool,
    pub samples: Vec<UvSample>,
}

fn unwrap_near(x: f64, reference: f64, period: f64) -> f64 {
    x + period * ((reference - x) / period).round()
}

/// One walked loop: coedge tracks in loop order (starting from the first non-degenerate
/// coedge), the loop's winding in each periodic direction (Δ of the continuous lift / period;
/// 0 for a loop that closes in the parameter plane, ±1 for a seamless loop going once round a
/// cylinder), and (u, v) of the lift's start point (the first non-degenerate coedge's start
/// vertex). A loop made only of degenerate edges (ASM's apex/pole "point loop") has no samples,
/// winding 0 and no start: it bounds nothing in the parameter plane and is accounted for by the
/// face-level winding balance (see [`crate::mass`]).
#[derive(Debug, Clone)]
pub struct LoopWalk {
    pub tracks: Vec<CoedgeTrack>,
    pub winding: [i64; 2],
    pub start: Option<[f64; 2]>,
}

impl LoopWalk {
    /// True for a loop of degenerate edges only.
    pub fn is_point_loop(&self) -> bool {
        self.start.is_none()
    }
}

/// Walks a loop of `face`, sampling each edge at the `(t, weight)` pairs produced by `sampler`
/// (ascending t). Works for both seam conventions: seamed loops close in the parameter plane
/// (winding 0); seamless loops (a closed circle bounding a cylinder band) come back with a
/// non-zero winding. Fails only for NURBS geometry.
pub fn walk_loop(brep: &Brep, face: FaceId, lp: LoopId, sampler: &EdgeSampler) -> GResult<LoopWalk> {
    let f = brep.face(face);
    let surf = &f.surface;
    if matches!(surf, Surface::Nurbs(_)) {
        return Err(GeomError::Unsupported("NURBS surfaces in parameter-space integration".into()));
    }
    let (pu, pv) = surf.periods();
    let coedges = &brep.lp(lp).coedges;
    let n = coedges.len();
    let first_real = (0..n).find(|&i| !brep.is_degenerate(brep.coedge(coedges[i]).edge));
    let start = first_real.unwrap_or(0);
    let face_uses = |e| brep.face_coedges(face).filter(|&c| brep.coedge(c).edge == e).count();
    let mut cur: Option<[f64; 2]> = None;
    let mut tracks = Vec::with_capacity(n);
    for k in 0..n {
        let cid = coedges[(start + k) % n];
        let ce = brep.coedge(cid);
        let edge = brep.edge(ce.edge);
        let sign = if ce.reversed { -1.0 } else { 1.0 };
        let Some(curve) = &edge.curve else {
            // Degenerate: pcurve u = t at constant v (dv = 0, no contribution); keep continuity.
            if let Some(c) = cur.as_mut() {
                c[0] += sign * (edge.t1 - edge.t0);
            }
            tracks.push(CoedgeTrack { coedge: cid, sign, degenerate: true, samples: Vec::new() });
            continue;
        };
        let mut nodes = sampler(edge)?;
        if ce.reversed {
            nodes.reverse();
        }
        let before = cur;
        let mut samples = Vec::with_capacity(nodes.len());
        for (t, w) in nodes {
            let p = curve.point(t);
            let (uo, mut v) = inverse(surf, &p).ok_or_else(|| GeomError::Unsupported("surface inverse".into()))?;
            let mut u = uo.or(cur.map(|c| c[0])).unwrap_or(0.0);
            if let (Some(per), Some(c)) = (pu, cur) {
                u = unwrap_near(u, c[0], per);
            }
            if let (Some(per), Some(c)) = (pv, cur) {
                v = unwrap_near(v, c[1], per);
            }
            let duv = uv_rates(surf, u, v, &curve.d1(t));
            cur = Some([u, v]);
            samples.push(UvSample { t, w, uv: [u, v], duv });
        }
        if face_uses(ce.edge) >= 2 && !samples.is_empty() {
            place_seam(surf, f.reversed, edge, sign, before, &mut samples);
            cur = samples.last().map(|s| s.uv);
        }
        tracks.push(CoedgeTrack { coedge: cid, sign, degenerate: false, samples });
    }
    let Some(i0) = first_real else {
        return Ok(LoopWalk { tracks, winding: [0, 0], start: None });
    };
    // start point of the lift: the first real coedge's start vertex, in the samples' lift
    let first = tracks.iter().find_map(|t| t.samples.first().map(|s| s.uv)).unwrap_or([0.0, 0.0]);
    let p0 = brep.vertex(brep.coedge_start(coedges[i0])).point;
    let (u0, mut v0) = inverse(surf, &p0).unwrap_or((None, first[1]));
    let mut u0 = u0.unwrap_or(first[0]);
    if let Some(per) = pu {
        u0 = unwrap_near(u0, first[0], per);
    }
    if let Some(per) = pv {
        v0 = unwrap_near(v0, first[1], per);
    }
    let end = cur.unwrap_or(first);
    let wind = |i: usize, per: Option<f64>| per.map_or(0, |per| ((end[i] - first[i]) / per).round() as i64);
    Ok(LoopWalk { tracks, winding: [wind(0, pu), wind(1, pv)], start: Some([u0, v0]) })
}

/// Moves the samples of a seam coedge onto the side of the period chosen by the
/// material-on-the-left rule (see module docs).
fn place_seam(surf: &Surface, reversed: bool, edge: &Edge, sign: f64, before: Option<[f64; 2]>, s: &mut [UvSample]) {
    let (pu, pv) = surf.periods();
    let mid = s[s.len() / 2];
    // which coordinate is constant along the seam
    let axis = match (pu, pv) {
        (Some(_), Some(_)) => usize::from(mid.duv[0].abs() > mid.duv[1].abs()),
        (Some(_), None) => 0,
        (None, Some(_)) => 1,
        (None, None) => return,
    };
    let per = if axis == 0 { pu } else { pv }.unwrap_or(TAU);
    let travel = sign * mid.duv[1 - axis];
    // u-seam: high side when travelling +v; v-seam: high side when travelling −u
    let high = if axis == 0 { travel > 0.0 } else { travel < 0.0 } != reversed;
    // seam coordinate from the edge midpoint, identical for both uses of the edge
    let pm = edge.curve.as_ref().map(|c| c.point(0.5 * (edge.t0 + edge.t1)));
    let raw = pm.and_then(|p| inverse(surf, &p)).map(|(u, v)| if axis == 0 { u.unwrap_or(0.0) } else { v });
    let raw = raw.unwrap_or(mid.uv[axis]);
    let eps = 1e-9 * per;
    let seam = match before {
        // high: the smallest seam copy ≥ reference; low: the largest ≤ reference
        Some(r) => {
            let r = r[axis];
            if high {
                raw + per * ((r - eps - raw) / per).ceil()
            } else {
                raw + per * ((r + eps - raw) / per).floor()
            }
        }
        None => {
            let base = raw.rem_euclid(per);
            let base = if base > per - eps { base - per } else { base };
            if high { base + per } else { base }
        }
    };
    for x in s.iter_mut() {
        x.uv[axis] = unwrap_near(x.uv[axis], seam, per);
    }
}
