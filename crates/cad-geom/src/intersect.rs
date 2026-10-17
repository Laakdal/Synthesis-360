//! Intersections of curves and surfaces.
//!
//! **Tolerance policy.** Every routine takes a [`Tolerance`]. Directions closer than
//! `tol.angular` are parallel (parallel planes, a line parallel to a plane or to a cylinder's
//! axis, …). Contacts are decided in *distance space*, never on a discriminant: along a curve
//! with signed distance f(t) to the surface, candidate roots (f = 0) and candidate extrema of f
//! with |f| ≤ `tol.linear` are collected, and neighbouring candidates are merged into one
//! *tangent* hit whenever |f| stays within `tol.linear` between them. So a line that misses or
//! penetrates a sphere by less than `tol.linear` gives exactly one tangent point (the extremum),
//! while a deeper penetration gives two transversal points.
//!
//! Lines are handled in closed form (the torus via its quartic, whose real roots are isolated
//! between the real roots of its derivatives and bisected, so double and quadruple roots are
//! found deterministically). Other curves against analytic surfaces are sampled (per NURBS span)
//! and refined by bisection/golden-section on the exact signed distance. Curves against NURBS
//! surfaces use recursive Bézier subdivision with control-net bounding boxes, followed by Newton
//! on C(t) = S(u, v).

use crate::{Aabb, Curve, Frame, GResult, GeomError, NurbsCurve, NurbsSurface, Surface, Tolerance, P3, V3};

/// Points sampled between two candidates when deciding whether |f| stays within tolerance.
const MERGE_SAMPLES: usize = 16;
/// Samples per knot span (times degree + 1) of the distance function along a general curve.
const CURVE_SAMPLES_PER_DEGREE: usize = 16;
/// Minimum number of distance samples along a general curve range.
const CURVE_MIN_SAMPLES: usize = 128;
/// Subdivision stops when a curve piece and a surface patch are both smaller than this fraction
/// of the whole curve / surface; Newton then converges from the piece centres.
const LEAF_REL: f64 = 1e-2;
/// Subdivision depth limit (pieces of 2^-40 of the original are far below any tolerance).
const MAX_DEPTH: usize = 40;
/// Pair budget of the subdivision; exceeding it means an overlap or a tangential contact
/// along a curve, which this point-intersection routine does not handle.
const MAX_PAIRS: usize = 200_000;
/// Newton iteration limit for curve–NURBS-surface refinement.
const NEWTON_ITERS: usize = 40;
/// Newton stops once the curve–surface gap is below this fraction of `tol.linear`.
const NEWTON_RESIDUAL_REL: f64 = 1e-3;
/// Singular values below this fraction of the largest are dropped from the Newton step. At a
/// tangent contact C' lies in the surface tangent plane and the Jacobian [C', −S_u, −S_v] is
/// singular; truncation then gives the minimum-norm step instead of an unbounded one.
const SVD_REL: f64 = 1e-10;
/// First secant step (fraction of the curve domain) when searching the stationary point of the
/// distance next to a NURBS hit.
const SECANT_START_REL: f64 = 1e-6;
/// The secant search has converged (a genuine stationary point) once its step is below this
/// fraction of the curve domain; unconverged searches give no tangent candidate.
const SECANT_TOL_REL: f64 = 1e-13;
/// Golden-section iterations (the bracket shrinks by 0.618 per step; 200 reach f64 resolution).
const GOLDEN_ITERS: usize = 200;
/// A range spanning one period up to this many ulps of round-off is treated as closed.
const CLOSED_RANGE_ULPS: f64 = 8.0;

/// Result of a surface–surface intersection.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceIntersection {
    None,
    /// An isolated contact point (tangency within tolerance).
    Point(P3),
    /// Intersection curves; `tangent` when the surfaces touch along them.
    Curves { curves: Vec<Curve>, tangent: bool },
    /// The surfaces coincide (within tolerance).
    Coincident,
}

/// One curve–surface intersection point with its parameters on both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveSurfaceHit {
    pub t: f64,
    pub u: f64,
    pub v: f64,
    pub point: P3,
    /// The curve touches the surface here (double root within tolerance) rather than crossing.
    pub tangent: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CurveSurfaceIntersection {
    None,
    /// Hits sorted by curve parameter.
    Points(Vec<CurveSurfaceHit>),
    /// The curve lies in the surface (within tolerance) over its range.
    Coincident,
}

/// One curve–curve intersection point with its parameters on both curves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveCurveHit {
    pub ta: f64,
    pub tb: f64,
    pub point: P3,
    pub tangent: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CurveCurveIntersection {
    None,
    /// Hits sorted by parameter on the first curve.
    Points(Vec<CurveCurveHit>),
    /// The curves coincide (same line, or same circle).
    Coincident,
}

impl CurveSurfaceIntersection {
    /// The hits (empty for `None` and `Coincident`).
    pub fn points(&self) -> &[CurveSurfaceHit] {
        match self {
            CurveSurfaceIntersection::Points(p) => p,
            _ => &[],
        }
    }
}

impl CurveCurveIntersection {
    pub fn points(&self) -> &[CurveCurveHit] {
        match self {
            CurveCurveIntersection::Points(p) => p,
            _ => &[],
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Shared root handling

/// Merges candidate roots and extrema of the signed distance `f` into hits `(t, tangent)`.
fn cluster(roots: &[f64], extrema: &[f64], f: &dyn Fn(f64) -> f64, tol: f64) -> Vec<(f64, bool)> {
    let mut c: Vec<(f64, bool)> = roots
        .iter()
        .map(|&t| (t, false))
        .chain(extrema.iter().map(|&t| (t, true)))
        .filter(|(t, _)| t.is_finite() && f(*t).abs() <= tol)
        .collect();
    c.sort_by(|a, b| a.0.total_cmp(&b.0));
    let within = |a: f64, b: f64| (1..MERGE_SAMPLES).all(|k| f(a + (b - a) * k as f64 / MERGE_SAMPLES as f64).abs() <= tol);
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let mut j = i;
        while j + 1 < c.len() && within(c[j].0, c[j + 1].0) {
            j += 1;
        }
        let g = &c[i..=j];
        let ext = g.iter().filter(|x| x.1).min_by(|a, b| f(a.0).abs().total_cmp(&f(b.0).abs()));
        out.push(match ext {
            Some(e) => (e.0, true),
            // Roots only: the same crossing found more than once (a genuine double contact always
            // has a stationary point of f between its roots, which every caller supplies).
            None => (g.iter().min_by(|a, b| f(a.0).abs().total_cmp(&f(b.0).abs())).expect("non-empty").0, false),
        });
        i = j + 1;
    }
    out
}

/// Root of `g` between `lo` and `hi` where g changes sign, bisected to floating-point resolution.
fn bisect_sign(g: &dyn Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> f64 {
    let neg_lo = g(lo) < 0.0;
    loop {
        let m = 0.5 * (lo + hi);
        if m <= lo || m >= hi {
            return m;
        }
        if (g(m) < 0.0) == neg_lo { lo = m } else { hi = m }
    }
}

/// Extremum of `g` on `[a, b]` by golden-section search (`maximize` selects max or min).
fn golden(g: &dyn Fn(f64) -> f64, mut a: f64, mut b: f64, maximize: bool) -> f64 {
    let s = if maximize { -1.0 } else { 1.0 };
    let r = (5f64.sqrt() - 1.0) / 2.0;
    let (mut x1, mut x2) = (b - r * (b - a), a + r * (b - a));
    let (mut f1, mut f2) = (s * g(x1), s * g(x2));
    for _ in 0..GOLDEN_ITERS {
        if b - a <= f64::EPSILON * (a.abs() + b.abs()) {
            break;
        }
        if f1 <= f2 {
            b = x2;
            (x2, f2) = (x1, f1);
            x1 = b - r * (b - a);
            f1 = s * g(x1);
        } else {
            a = x1;
            (x1, f1) = (x2, f2);
            x2 = a + r * (b - a);
            f2 = s * g(x2);
        }
    }
    0.5 * (a + b)
}

fn eval_poly(c: &[f64], x: f64) -> f64 {
    c.iter().rev().fold(0.0, |acc, &k| acc * x + k)
}

/// Real roots of Σ c[i]·x^i and the real roots of its derivative (critical points). Roots are
/// isolated between consecutive critical points (where the polynomial is monotone) and the
/// Cauchy bound, then bisected; multiple roots appear as critical points with a ~0 value.
pub fn poly_real_roots(c: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let n = match c.iter().rposition(|&x| x != 0.0) {
        Some(n) => n,
        None => return (Vec::new(), Vec::new()),
    };
    if n == 0 {
        return (Vec::new(), Vec::new());
    }
    if n == 1 {
        return (vec![-c[0] / c[1]], Vec::new());
    }
    let c = &c[..=n];
    let d: Vec<f64> = (1..=n).map(|i| c[i] * i as f64).collect();
    let (mut crit, _) = poly_real_roots(&d);
    let bound = 1.0 + c[..n].iter().map(|x| (x / c[n]).abs()).fold(0.0, f64::max);
    crit.retain(|x| x.abs() < bound);
    crit.sort_by(f64::total_cmp);
    let mut pts = vec![-bound];
    pts.extend(&crit);
    pts.push(bound);
    let p = |x: f64| eval_poly(c, x);
    let mut roots: Vec<f64> = Vec::new();
    for w in pts.windows(2) {
        let (fa, fb) = (p(w[0]), p(w[1]));
        if fa == 0.0 {
            roots.push(w[0]);
        } else if fa * fb < 0.0 {
            roots.push(bisect_sign(&p, w[0], w[1]));
        }
    }
    if p(bound) == 0.0 {
        roots.push(bound);
    }
    roots.dedup();
    (roots, crit)
}

fn hits_on_line(o: &P3, d: &V3, ts: &[(f64, bool)], s: &Surface, speed: f64) -> CurveSurfaceIntersection {
    if ts.is_empty() {
        return CurveSurfaceIntersection::None;
    }
    let hits = ts
        .iter()
        .map(|&(t, tangent)| {
            let point = o + d * t;
            let (u, v, _, _) = s.project(&point);
            CurveSurfaceHit { t: t / speed, u, v, point, tangent }
        })
        .collect();
    CurveSurfaceIntersection::Points(hits)
}

/// Line data normalised: unit direction and its original length.
fn unit_line(dir: &V3) -> (V3, f64) {
    let s = dir.norm();
    (dir / s, s)
}

// ---------------------------------------------------------------------------------------------
// Line – surface

/// Line `origin + t·dir` against a plane. Parallel within `tol.angular` → `Coincident` when the
/// line is within `tol.linear` of the plane, else `None`.
pub fn line_plane(origin: &P3, dir: &V3, plane: &Frame, tol: &Tolerance) -> CurveSurfaceIntersection {
    let (d, speed) = unit_line(dir);
    let dist = (origin - plane.origin).dot(&plane.z);
    let den = d.dot(&plane.z);
    if den.abs() <= tol.angular {
        return if dist.abs() <= tol.linear { CurveSurfaceIntersection::Coincident } else { CurveSurfaceIntersection::None };
    }
    let s = Surface::Plane { frame: *plane };
    hits_on_line(origin, &d, &[(-dist / den, false)], &s, speed)
}

/// Line against a sphere: 0, 1 (tangent within `tol.linear`) or 2 points.
pub fn line_sphere(origin: &P3, dir: &V3, sphere: &Frame, radius: f64, tol: &Tolerance) -> CurveSurfaceIntersection {
    let (d, speed) = unit_line(dir);
    let s = Surface::Sphere { frame: *sphere, radius };
    let w = origin - sphere.origin;
    let ts = -w.dot(&d);
    let h = (w + d * ts).norm();
    let roots = if h <= radius {
        let off = ((radius - h) * (radius + h)).sqrt();
        vec![ts - off, ts + off]
    } else {
        Vec::new()
    };
    let f = |t: f64| (w + d * t).norm() - radius;
    hits_on_line(origin, &d, &cluster(&roots, &[ts], &f, tol.linear), &s, speed)
}

/// Line against an (infinite) cylinder: parallel to the axis (within `tol.angular`) gives
/// `Coincident` when on the surface within `tol.linear`, else `None`; otherwise 0, 1 (tangent)
/// or 2 points.
pub fn line_cylinder(origin: &P3, dir: &V3, cyl: &Frame, radius: f64, tol: &Tolerance) -> CurveSurfaceIntersection {
    let (d, speed) = unit_line(dir);
    let s = Surface::Cylinder { frame: *cyl, radius };
    let q = cyl.to_local(*origin);
    let dl = V3::new(d.dot(&cyl.x), d.dot(&cyl.y), d.dot(&cyl.z));
    let e = dl.x.hypot(dl.y);
    if e <= tol.angular {
        let rho = q.x.hypot(q.y);
        return if (rho - radius).abs() <= tol.linear { CurveSurfaceIntersection::Coincident } else { CurveSurfaceIntersection::None };
    }
    let ts = -(q.x * dl.x + q.y * dl.y) / (e * e);
    let h = (q.x + dl.x * ts).hypot(q.y + dl.y * ts);
    let roots = if h <= radius {
        let off = ((radius - h) * (radius + h)).sqrt() / e;
        vec![ts - off, ts + off]
    } else {
        Vec::new()
    };
    let f = |t: f64| (q.x + dl.x * t).hypot(q.y + dl.y * t) - radius;
    hits_on_line(origin, &d, &cluster(&roots, &[ts], &f, tol.linear), &s, speed)
}

/// Line against the double cone of [`Surface::Cone`]. A line lying on the cone (a generator)
/// gives `Coincident`. A line parallel to a generator direction (|quadratic coefficient| ≤
/// `tol.angular`) has a single finite root; the second root at infinity is dropped.
pub fn line_cone(origin: &P3, dir: &V3, cone: &Frame, radius: f64, half_angle: f64, tol: &Tolerance) -> CurveSurfaceIntersection {
    let (d, speed) = unit_line(dir);
    let s = Surface::Cone { frame: *cone, radius, half_angle };
    let q = cone.to_local(*origin);
    let dl = V3::new(d.dot(&cone.x), d.dot(&cone.y), d.dot(&cone.z));
    let k = half_angle.tan();
    let f = |t: f64| s.signed_distance(&(origin + d * t)).expect("analytic");
    let scale = q.coords.norm() + radius.abs() + 1.0;
    if [-scale, 0.0, scale].iter().all(|&t| f(t).abs() <= tol.linear) {
        return CurveSurfaceIntersection::Coincident;
    }
    let rr = radius + k * q.z;
    let a = dl.x * dl.x + dl.y * dl.y - k * k * dl.z * dl.z;
    let b = 2.0 * (q.x * dl.x + q.y * dl.y - k * dl.z * rr);
    let c = q.x * q.x + q.y * q.y - rr * rr;
    let (roots, ext) = if a.abs() <= tol.angular {
        (if b != 0.0 { vec![-c / b] } else { Vec::new() }, Vec::new())
    } else {
        let disc = b * b - 4.0 * a * c;
        let mut r = Vec::new();
        if disc >= 0.0 {
            let qq = -0.5 * (b + b.signum() * disc.sqrt());
            r.push(qq / a);
            if qq != 0.0 {
                r.push(c / qq);
            }
        }
        (r, vec![-b / (2.0 * a)])
    };
    hits_on_line(origin, &d, &cluster(&roots, &ext, &f, tol.linear), &s, speed)
}

/// Line against a torus via the quartic (x²+y²+z²+R²−r²)² = 4R²(x²+y²) in the torus frame,
/// written about the foot of the perpendicular from the centre and scaled by R + r. Real roots
/// and critical points come from [`poly_real_roots`]; tangency is decided on the exact
/// distance to the torus, so double (tangent) and quadruple (horn-torus axis) roots resolve to
/// single tangent hits.
pub fn line_torus(origin: &P3, dir: &V3, torus: &Frame, major: f64, minor: f64, tol: &Tolerance) -> CurveSurfaceIntersection {
    let (d, speed) = unit_line(dir);
    let s = Surface::Torus { frame: *torus, major, minor };
    let q = torus.to_local(*origin);
    let dl = V3::new(d.dot(&torus.x), d.dot(&torus.y), d.dot(&torus.z));
    let tf = -q.coords.dot(&dl);
    let l = major.abs() + minor.abs();
    let q0 = (q.coords + dl * tf) / l;
    let (rb, rs) = (major / l, minor / l);
    let kk = rb * rb - rs * rs;
    let cc = q0.norm_squared() + kk;
    let (e, fx, g) = (dl.x * dl.x + dl.y * dl.y, q0.x * dl.x + q0.y * dl.y, q0.x * q0.x + q0.y * q0.y);
    let r4 = 4.0 * rb * rb;
    let coeffs = [cc * cc - r4 * g, -2.0 * r4 * fx, 2.0 * cc - r4 * e, 0.0, 1.0];
    let (roots, crit) = poly_real_roots(&coeffs);
    let to_t = |tau: f64| tf + l * tau;
    let roots: Vec<f64> = roots.into_iter().map(to_t).collect();
    let crit: Vec<f64> = crit.into_iter().map(to_t).collect();
    let f = |t: f64| s.signed_distance(&(origin + d * t)).expect("analytic");
    hits_on_line(origin, &d, &cluster(&roots, &crit, &f, tol.linear), &s, speed)
}

/// Line `origin + t·dir` (all t) against any surface. NURBS surfaces are handled by
/// [`curve_surface`] on the part of the line inside the control-net box.
pub fn line_surface(origin: &P3, dir: &V3, surface: &Surface, tol: &Tolerance) -> GResult<CurveSurfaceIntersection> {
    if !dir.norm().is_finite() || dir.norm() == 0.0 {
        return Err(GeomError::InvalidInput("line direction is zero".into()));
    }
    Ok(match surface {
        Surface::Plane { frame } => line_plane(origin, dir, frame, tol),
        Surface::Sphere { frame, radius } => line_sphere(origin, dir, frame, *radius, tol),
        Surface::Cylinder { frame, radius } => line_cylinder(origin, dir, frame, *radius, tol),
        Surface::Cone { frame, radius, half_angle } => line_cone(origin, dir, frame, *radius, *half_angle, tol),
        Surface::Torus { frame, major, minor } => line_torus(origin, dir, frame, *major, *minor, tol),
        Surface::Nurbs(n) => {
            let line = Curve::Line { origin: *origin, dir: *dir };
            match clip_line(origin, dir, &n.control_bbox().inflated(tol.linear)) {
                Some(r) => curve_nurbs_surface(&line, r, n, tol)?,
                None => CurveSurfaceIntersection::None,
            }
        }
    })
}

/// Parameter range of `origin + t·dir` inside the box (slab method).
fn clip_line(o: &P3, d: &V3, b: &Aabb) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for i in 0..3 {
        if d[i] == 0.0 {
            if o[i] < b.min[i] || o[i] > b.max[i] {
                return None;
            }
        } else {
            let (a, c) = ((b.min[i] - o[i]) / d[i], (b.max[i] - o[i]) / d[i]);
            lo = lo.max(a.min(c));
            hi = hi.min(a.max(c));
        }
    }
    (lo < hi).then_some((lo, hi))
}

// ---------------------------------------------------------------------------------------------
// Curve – surface

/// Intersections of `curve` restricted to `[t0, t1]` with `surface`.
/// * Line × analytic: closed forms ([`line_surface`]), filtered to the range (infinite bounds
///   allowed).
/// * Other curves × analytic: the exact signed distance is sampled (≥ 128 samples, 16·(p+1)
///   per NURBS span), sign changes are bisected and sampled extrema refined by golden section,
///   then merged per the module's tolerance policy. Features narrower than the sampling step
///   (two roots within one step) can be missed.
/// * Any curve × NURBS: Bézier subdivision + Newton on C(t) = S(u, v); the hits and the nearby
///   stationary points of the (locally signed) distance are then merged by the same
///   distance-space rule as the analytic paths, so tangency is decided identically. Returns
///   `GeomError::Numerical` when the curve overlaps the surface (pair budget exhausted).
pub fn curve_surface(curve: &Curve, range: (f64, f64), surface: &Surface, tol: &Tolerance) -> GResult<CurveSurfaceIntersection> {
    let (t0, t1) = range;
    if t0.is_nan() || t1.is_nan() || t0 > t1 {
        return Err(GeomError::InvalidInput(format!("curve_surface: invalid range [{t0}, {t1}]")));
    }
    match (curve, surface) {
        (Curve::Line { origin, dir }, s) => {
            let mut r = if let Surface::Nurbs(n) = s {
                let Some((a, b)) = clip_line(origin, dir, &n.control_bbox().inflated(tol.linear)) else {
                    return Ok(CurveSurfaceIntersection::None);
                };
                let (a, b) = (a.max(t0), b.min(t1));
                if a >= b {
                    return Ok(CurveSurfaceIntersection::None);
                }
                curve_nurbs_surface(curve, (a, b), n, tol)?
            } else {
                line_surface(origin, dir, s, tol)?
            };
            if let CurveSurfaceIntersection::Points(h) = &mut r {
                let slack = tol.linear / dir.norm();
                h.retain(|x| x.t >= t0 - slack && x.t <= t1 + slack);
                if h.is_empty() {
                    r = CurveSurfaceIntersection::None;
                }
            }
            Ok(r)
        }
        (_, Surface::Nurbs(n)) => curve_nurbs_surface(curve, range, n, tol),
        (_, s) => {
            if !(t0.is_finite() && t1.is_finite() && t0 < t1) {
                return Err(GeomError::InvalidInput("curve_surface: finite increasing range required".into()));
            }
            Ok(curve_implicit(curve, t0, t1, s, tol))
        }
    }
}

fn sample_params(curve: &Curve, t0: f64, t1: f64) -> Vec<f64> {
    let mut cuts = vec![t0];
    let mut per = CURVE_MIN_SAMPLES;
    if let Curve::Nurbs(n) = curve {
        let p = n.degree;
        let bp = n.breakpoints();
        let spans = bp.len() - 1;
        per = (CURVE_SAMPLES_PER_DEGREE * (p + 1) * spans).max(CURVE_MIN_SAMPLES);
        let (d0, d1) = n.domain();
        let shift = n.period().unwrap_or(0.0);
        let reps = if shift > 0.0 { ((t0 - d0) / shift).floor() as i64..=((t1 - d0) / shift).ceil() as i64 } else { 0..=0 };
        for r in reps {
            cuts.extend(bp.iter().map(|k| k + r as f64 * (d1 - d0)).filter(|k| *k > t0 && *k < t1));
        }
    }
    cuts.push(t1);
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let pieces = cuts.len() - 1;
    let m = per.div_ceil(pieces).max(4);
    let mut ts = vec![t0];
    for w in cuts.windows(2) {
        ts.extend((1..=m).map(|i| w[0] + (w[1] - w[0]) * i as f64 / m as f64));
    }
    ts
}

fn curve_implicit(curve: &Curve, t0: f64, t1: f64, s: &Surface, tol: &Tolerance) -> CurveSurfaceIntersection {
    let f = |t: f64| s.signed_distance(&curve.point(t)).expect("analytic surface");
    let ts = sample_params(curve, t0, t1);
    let fs: Vec<f64> = ts.iter().map(|&t| f(t)).collect();
    let mids_on = ts.windows(2).all(|w| f(0.5 * (w[0] + w[1])).abs() <= tol.linear);
    if fs.iter().all(|x| x.abs() <= tol.linear) && mids_on {
        return CurveSurfaceIntersection::Coincident;
    }
    let mut roots = Vec::new();
    let mut ext = Vec::new();
    for i in 0..ts.len() {
        if fs[i] == 0.0 {
            roots.push(ts[i]);
        }
        if i + 1 < ts.len() && fs[i] * fs[i + 1] < 0.0 {
            roots.push(bisect_sign(&f, ts[i], ts[i + 1]));
        }
        if i > 0 && i + 1 < ts.len() {
            let (dl, dr) = (fs[i] - fs[i - 1], fs[i + 1] - fs[i]);
            if dl * dr <= 0.0 && !(dl == 0.0 && dr == 0.0) {
                ext.push(golden(&f, ts[i - 1], ts[i + 1], dl > 0.0));
            }
        }
    }
    // A closed range (one full period) has no ends: look for an extremum across the seam too.
    let n = ts.len();
    if let Some(p) = curve.period().filter(|p| (t1 - t0 - p).abs() <= CLOSED_RANGE_ULPS * f64::EPSILON * p.max(t0.abs())) {
        let (dl, dr) = (fs[0] - fs[n - 2], fs[1] - fs[0]);
        if dl * dr <= 0.0 && !(dl == 0.0 && dr == 0.0) {
            let t = golden(&f, ts[n - 2] - p, ts[1], dl > 0.0);
            ext.push(if t < t0 { t + p } else { t });
        }
    }
    roots.extend([t0, t1].into_iter().filter(|&t| f(t).abs() <= tol.linear));
    let hits = cluster(&roots, &ext, &f, tol.linear);
    let mut out: Vec<CurveSurfaceHit> = Vec::new();
    for (t, tangent) in hits {
        let point = curve.point(t);
        if out.iter().any(|h| (h.point - point).norm() <= tol.linear) {
            continue;
        }
        let (u, v, _, _) = s.project(&point);
        out.push(CurveSurfaceHit { t, u, v, point, tangent });
    }
    if out.is_empty() { CurveSurfaceIntersection::None } else { CurveSurfaceIntersection::Points(out) }
}

fn corner_extent(p: &NurbsSurface, along_u: bool) -> f64 {
    let (nu, nv) = (p.count_u, p.count_v);
    let at = |i: usize, j: usize| p.control_points[i * nv + j];
    if along_u {
        (0..nv).map(|j| (at(nu - 1, j) - at(0, j)).norm()).fold(0.0, f64::max)
    } else {
        (0..nu).map(|i| (at(i, nv - 1) - at(i, 0)).norm()).fold(0.0, f64::max)
    }
}

fn curve_nurbs_surface(curve: &Curve, range: (f64, f64), s: &NurbsSurface, tol: &Tolerance) -> GResult<CurveSurfaceIntersection> {
    let c = curve.to_nurbs(range.0, range.1)?;
    let (cd, sd) = (c.control_bbox().diagonal(), s.control_bbox().diagonal());
    let mut stack: Vec<(NurbsCurve, NurbsSurface, usize)> = Vec::new();
    for seg in c.bezier_segments() {
        for patch in s.bezier_patches() {
            stack.push((seg.clone(), patch, 0));
        }
    }
    let mut leaves: Vec<(f64, f64, f64)> = Vec::new();
    let mut pairs = 0;
    while let Some((cs, ps, depth)) = stack.pop() {
        pairs += 1;
        if pairs > MAX_PAIRS {
            return Err(GeomError::Numerical("curve–NURBS surface: subdivision budget exhausted (overlap or tangential contact along a curve)".into()));
        }
        let (cb, pb) = (cs.control_bbox(), ps.control_bbox());
        if !cb.inflated(tol.linear).overlaps(&pb) {
            continue;
        }
        let (small_c, small_s) = (cb.diagonal() <= LEAF_REL * cd, pb.diagonal() <= LEAF_REL * sd);
        let (a, b) = cs.domain();
        let ((u0, u1), (v0, v1)) = ps.domain();
        if (small_c && small_s) || depth >= MAX_DEPTH {
            leaves.push((0.5 * (a + b), 0.5 * (u0 + u1), 0.5 * (v0 + v1)));
            continue;
        }
        if !small_c && (small_s || cb.diagonal() >= pb.diagonal()) {
            let (l, r) = cs.split(0.5 * (a + b))?;
            stack.push((l, ps.clone(), depth + 1));
            stack.push((r, ps, depth + 1));
        } else if corner_extent(&ps, true) >= corner_extent(&ps, false) {
            let m = 0.5 * (u0 + u1);
            stack.push((cs.clone(), ps.extract((u0, m), (v0, v1))?, depth + 1));
            stack.push((cs, ps.extract((m, u1), (v0, v1))?, depth + 1));
        } else {
            let m = 0.5 * (v0 + v1);
            stack.push((cs.clone(), ps.extract((u0, u1), (v0, m))?, depth + 1));
            stack.push((cs, ps.extract((u0, u1), (m, v1))?, depth + 1));
        }
    }
    let ((su0, su1), (sv0, sv1)) = s.domain();
    let (ca, cb) = c.domain();
    let mut raw: Vec<(f64, f64, f64)> = Vec::new();
    for (t, u, v) in leaves {
        let (mut t, mut u, mut v) = (t, u, v);
        for _ in 0..NEWTON_ITERS {
            let cd = c.derivs(t, 1);
            let sk = s.derivs(u, v, 1);
            let r = cd[0] - sk[0][0];
            if r.norm() <= tol.linear * NEWTON_RESIDUAL_REL {
                break;
            }
            let svd = nalgebra::Matrix3::from_columns(&[cd[1], -sk[1][0], -sk[0][1]]).svd(true, true);
            let cut = svd.singular_values.max() * SVD_REL;
            let Ok(step) = svd.solve(&(-r), cut) else { break };
            let (tn, un, vn) = ((t + step.x).clamp(ca, cb), u + step.y, v + step.z);
            let (un, vn) = s.wrap(un, vn);
            let (un, vn) = (if s.periodic_u { un } else { un.clamp(su0, su1) }, if s.periodic_v { vn } else { vn.clamp(sv0, sv1) });
            let done = (tn - t).abs() + (un - u).abs() + (vn - v).abs() == 0.0;
            (t, u, v) = (tn, un, vn);
            if done {
                break;
            }
        }
        let cp = c.point(t);
        if (cp - s.point(u, v)).norm() <= tol.linear && !raw.iter().any(|h| (c.point(h.0) - cp).norm() <= tol.linear) {
            raw.push((t, u, v));
        }
    }
    if raw.is_empty() {
        return Ok(CurveSurfaceIntersection::None);
    }
    raw.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Local signed distance along the curve: Newton foot point seeded from the nearest hit.
    let foot = |t: f64| {
        let h = raw.iter().min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs())).expect("non-empty");
        s.project_from(&c.point(t), h.1, h.2)
    };
    let unit_normal = |u: f64, v: f64| {
        let (du, dv) = s.d1(u, v);
        let n = du.cross(&dv);
        let len = n.norm();
        (len > 0.0).then(|| n / len)
    };
    let f = |t: f64| {
        let (u, v, q, d) = foot(t);
        unit_normal(u, v).map_or(d, |n| (c.point(t) - q).dot(&n))
    };
    let g = |t: f64| {
        let (u, v, _, _) = foot(t);
        unit_normal(u, v).map_or(0.0, |n| c.d1(t).dot(&n))
    };
    // Stationary points of the distance near each hit (secant on C'·n = 0): tangent candidates.
    let h = SECANT_START_REL * (cb - ca);
    let ext: Vec<f64> = raw
        .iter()
        .filter_map(|r| {
            let (mut a, mut b) = (r.0, if r.0 + h <= cb { r.0 + h } else { r.0 - h });
            let (mut ga, mut gb) = (g(a), g(b));
            for _ in 0..NEWTON_ITERS {
                if gb == 0.0 || (b - a).abs() <= SECANT_TOL_REL * (cb - ca) {
                    return Some(b);
                }
                if gb == ga {
                    return None;
                }
                let x = (b - gb * (b - a) / (gb - ga)).clamp(ca, cb);
                (a, ga) = (b, gb);
                b = x;
                gb = g(b);
            }
            None
        })
        .collect();
    let ts: Vec<f64> = raw.iter().map(|r| r.0).collect();
    let mut hits: Vec<CurveSurfaceHit> = Vec::new();
    for (t, tangent) in cluster(&ts, &ext, &f, tol.linear) {
        let point = c.point(t);
        let (u, v, _, _) = foot(t);
        let t = match curve {
            Curve::Circle { .. } | Curve::Ellipse { .. } => curve.project_in(&point, range.0, range.1).0,
            _ => t,
        };
        hits.push(CurveSurfaceHit { t, u, v, point, tangent });
    }
    hits.sort_by(|a, b| a.t.total_cmp(&b.t));
    Ok(CurveSurfaceIntersection::Points(hits))
}

// ---------------------------------------------------------------------------------------------
// Surface – surface

/// Two planes: the line of intersection (direction `a.z × b.z` normalised, origin the point of
/// the line closest to `a.origin`), `Coincident` when parallel and within `tol.linear`, else
/// `None` when parallel.
pub fn plane_plane(a: &Frame, b: &Frame, tol: &Tolerance) -> SurfaceIntersection {
    let u = a.z.cross(&b.z);
    let s = u.norm();
    let h2 = (b.origin - a.origin).dot(&b.z);
    if s <= tol.angular {
        return if h2.abs() <= tol.linear { SurfaceIntersection::Coincident } else { SurfaceIntersection::None };
    }
    let c = a.z.dot(&b.z);
    let det = s * s;
    let (alpha, beta) = (-h2 * c / det, h2 / det);
    let origin = a.origin + a.z * alpha + b.z * beta;
    SurfaceIntersection::Curves { curves: vec![Curve::Line { origin, dir: u / s }], tangent: false }
}

/// Plane and sphere: a circle (frame axes from the plane, centred on the foot of the sphere
/// centre), a tangent `Point` when the distance is within `tol.linear` of the radius, or `None`.
pub fn plane_sphere(plane: &Frame, sphere: &Frame, radius: f64, tol: &Tolerance) -> SurfaceIntersection {
    let d = (sphere.origin - plane.origin).dot(&plane.z);
    let foot = sphere.origin - plane.z * d;
    if (d.abs() - radius).abs() <= tol.linear {
        return SurfaceIntersection::Point(foot);
    }
    if d.abs() > radius {
        return SurfaceIntersection::None;
    }
    let r = ((radius - d.abs()) * (radius + d.abs())).sqrt();
    let frame = Frame { origin: foot, ..*plane };
    SurfaceIntersection::Curves { curves: vec![Curve::Circle { frame, radius: r }], tangent: false }
}

/// Plane and infinite cylinder: axis-perpendicular plane → circle (cylinder frame axes, so
/// the circle parameter equals the cylinder u); plane parallel to the axis → two lines, one
/// tangent line (within `tol.linear`) or none; oblique → ellipse (major r/|cos θ| along the
/// axis direction projected into the plane, minor r).
pub fn plane_cylinder(plane: &Frame, cyl: &Frame, radius: f64, tol: &Tolerance) -> SurfaceIntersection {
    let (n, z) = (plane.z, cyl.z);
    let cos = n.dot(&z);
    let sin = n.cross(&z).norm();
    if sin <= tol.angular {
        let h = (plane.origin - cyl.origin).dot(&n) / cos;
        let frame = Frame { origin: cyl.origin + z * h, ..*cyl };
        return SurfaceIntersection::Curves { curves: vec![Curve::Circle { frame, radius }], tangent: false };
    }
    if cos.abs() <= tol.angular {
        let d = (cyl.origin - plane.origin).dot(&n);
        let foot = cyl.origin - n * d;
        if (d.abs() - radius).abs() <= tol.linear {
            return SurfaceIntersection::Curves { curves: vec![Curve::Line { origin: foot, dir: z }], tangent: true };
        }
        if d.abs() > radius {
            return SurfaceIntersection::None;
        }
        let m = n.cross(&z).normalize();
        let w = ((radius - d.abs()) * (radius + d.abs())).sqrt();
        let curves = vec![Curve::Line { origin: foot - m * w, dir: z }, Curve::Line { origin: foot + m * w, dir: z }];
        return SurfaceIntersection::Curves { curves, tangent: false };
    }
    let h = (plane.origin - cyl.origin).dot(&n) / cos;
    let center = cyl.origin + z * h;
    let minor_dir = z.cross(&n).normalize();
    let major_dir = n.cross(&minor_dir);
    let frame = Frame { origin: center, x: major_dir, y: n.cross(&major_dir), z: n };
    SurfaceIntersection::Curves { curves: vec![Curve::Ellipse { frame, major: radius / cos.abs(), minor: radius }], tangent: false }
}

/// Plane and double cone, for a plane perpendicular to the axis (within `tol.angular`): a
/// circle in the cone frame's axes, or the apex `Point` when the plane passes within
/// `tol.linear` of it. Oblique planes (general conics) return `Unsupported`.
pub fn plane_cone(plane: &Frame, cone: &Frame, radius: f64, half_angle: f64, tol: &Tolerance) -> GResult<SurfaceIntersection> {
    let (n, z) = (plane.z, cone.z);
    if n.cross(&z).norm() > tol.angular {
        return Err(GeomError::Unsupported("plane–cone: only planes perpendicular to the cone axis".into()));
    }
    let h = (plane.origin - cone.origin).dot(&n) / n.dot(&z);
    let rho = radius + h * half_angle.tan();
    let origin = cone.origin + z * h;
    if rho.abs() <= tol.linear {
        return Ok(SurfaceIntersection::Point(origin));
    }
    let frame = Frame { origin, ..*cone };
    Ok(SurfaceIntersection::Curves { curves: vec![Curve::Circle { frame, radius: rho.abs() }], tangent: false })
}

/// Dispatcher for the supported surface pairs (plane with plane, sphere, cylinder, cone), in
/// either order; other pairs return `Unsupported`.
pub fn surface_surface(a: &Surface, b: &Surface, tol: &Tolerance) -> GResult<SurfaceIntersection> {
    match (a, b) {
        (Surface::Plane { frame: p }, Surface::Plane { frame: q }) => Ok(plane_plane(p, q, tol)),
        (Surface::Plane { frame: p }, other) | (other, Surface::Plane { frame: p }) => match other {
            Surface::Sphere { frame, radius } => Ok(plane_sphere(p, frame, *radius, tol)),
            Surface::Cylinder { frame, radius } => Ok(plane_cylinder(p, frame, *radius, tol)),
            Surface::Cone { frame, radius, half_angle } => plane_cone(p, frame, *radius, *half_angle, tol),
            _ => Err(GeomError::Unsupported(format!("surface intersection plane–{}", other.type_name()))),
        },
        _ => Err(GeomError::Unsupported(format!("surface intersection {}–{}", a.type_name(), b.type_name()))),
    }
}

// ---------------------------------------------------------------------------------------------
// Curve – curve

/// In-plane line (point `o`, unit `d`) against the circle (centre `c`, radius `r`): parameters
/// along the line with tangent flags, per the module tolerance policy.
fn line_circle_params(o: &P3, d: &V3, c: &P3, r: f64, tol: f64) -> Vec<(f64, bool)> {
    let w = o - c;
    let ts = -w.dot(d);
    let h = (w + d * ts).norm();
    let roots = if h <= r {
        let off = ((r - h) * (r + h)).sqrt();
        vec![ts - off, ts + off]
    } else {
        Vec::new()
    };
    let f = |t: f64| (w + d * t).norm() - r;
    cluster(&roots, &[ts], &f, tol)
}

/// Intersections of lines and circles in 3D (line–line, line–circle, circle–circle, either
/// order). Same line or same circle → `Coincident`. Ellipses and NURBS return `Unsupported`.
pub fn curve_curve(a: &Curve, b: &Curve, tol: &Tolerance) -> GResult<CurveCurveIntersection> {
    let hits = |pts: Vec<(P3, bool)>| {
        let mut h: Vec<CurveCurveHit> = Vec::new();
        for (point, tangent) in pts {
            if h.iter().any(|x| (x.point - point).norm() <= tol.linear) {
                continue;
            }
            h.push(CurveCurveHit { ta: a.project(&point).0, tb: b.project(&point).0, point, tangent });
        }
        h.sort_by(|x, y| x.ta.total_cmp(&y.ta));
        if h.is_empty() { CurveCurveIntersection::None } else { CurveCurveIntersection::Points(h) }
    };
    match (a, b) {
        (Curve::Line { origin: o1, dir: d1 }, Curve::Line { origin: o2, dir: d2 }) => {
            let (d1, d2) = (d1.normalize(), d2.normalize());
            let cr = d1.cross(&d2);
            let w = o1 - o2;
            if cr.norm() <= tol.angular {
                let dist = w.cross(&d2).norm();
                return Ok(if dist <= tol.linear { CurveCurveIntersection::Coincident } else { CurveCurveIntersection::None });
            }
            let (bb, dd, ee) = (d1.dot(&d2), d1.dot(&w), d2.dot(&w));
            let den = 1.0 - bb * bb;
            let (s, t) = ((bb * ee - dd) / den, (ee - bb * dd) / den);
            let (p1, p2) = (o1 + d1 * s, o2 + d2 * t);
            Ok(if (p1 - p2).norm() <= tol.linear { hits(vec![(nalgebra::center(&p1, &p2), false)]) } else { CurveCurveIntersection::None })
        }
        (Curve::Line { origin, dir }, Curve::Circle { frame, radius }) | (Curve::Circle { frame, radius }, Curve::Line { origin, dir }) => {
            let d = dir.normalize();
            let den = d.dot(&frame.z);
            let off = (origin - frame.origin).dot(&frame.z);
            if den.abs() <= tol.angular {
                if off.abs() > tol.linear {
                    return Ok(CurveCurveIntersection::None);
                }
                let o = origin - frame.z * off;
                let pts = line_circle_params(&o, &d, &frame.origin, *radius, tol.linear).into_iter().map(|(t, tg)| (o + d * t, tg)).collect();
                return Ok(hits(pts));
            }
            let p = origin - d * (off / den);
            Ok(if ((p - frame.origin).norm() - radius).abs() <= tol.linear { hits(vec![(p, false)]) } else { CurveCurveIntersection::None })
        }
        (Curve::Circle { frame: f1, radius: r1 }, Curve::Circle { frame: f2, radius: r2 }) => {
            let (r1, r2) = (*r1, *r2);
            let cr = f1.z.cross(&f2.z);
            if cr.norm() <= tol.angular {
                let off = (f2.origin - f1.origin).dot(&f1.z);
                if off.abs() > tol.linear {
                    return Ok(CurveCurveIntersection::None);
                }
                let c2 = f2.origin - f1.z * off;
                let dv = c2 - f1.origin;
                let dist = dv.norm();
                if dist <= tol.linear {
                    return Ok(if (r1 - r2).abs() <= tol.linear { CurveCurveIntersection::Coincident } else { CurveCurveIntersection::None });
                }
                let e = dv / dist;
                let a_ = (dist * dist + r1 * r1 - r2 * r2) / (2.0 * dist);
                if (dist - (r1 + r2)).abs() <= tol.linear || (dist - (r1 - r2).abs()).abs() <= tol.linear {
                    return Ok(hits(vec![(f1.origin + e * (r1 * a_.signum()), true)]));
                }
                if dist > r1 + r2 || dist < (r1 - r2).abs() {
                    return Ok(CurveCurveIntersection::None);
                }
                let h = (r1 * r1 - a_ * a_).max(0.0).sqrt();
                let m = f1.z.cross(&e);
                let base = f1.origin + e * a_;
                return Ok(hits(vec![(base - m * h, false), (base + m * h, false)]));
            }
            let SurfaceIntersection::Curves { curves, .. } = plane_plane(f1, f2, tol) else { unreachable!("planes are not parallel") };
            let Curve::Line { origin, dir } = &curves[0] else { unreachable!("plane–plane gives a line") };
            let pts = line_circle_params(origin, dir, &f1.origin, r1, tol.linear)
                .into_iter()
                .map(|(t, tg)| (origin + dir * t, tg))
                .filter(|(p, _)| ((p - f2.origin).norm() - r2).abs() <= tol.linear)
                .collect();
            Ok(hits(pts))
        }
        _ => Err(GeomError::Unsupported(format!("curve intersection {}–{}", a.type_name(), b.type_name()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_merges_roots_within_tolerance() {
        // f(t) = t² − 1e-9: two roots ±3.2e-5 with the extremum −1e-9 between them.
        let f = |t: f64| t * t - 1e-9;
        let r = (1e-9f64).sqrt();
        assert_eq!(cluster(&[-r, r], &[0.0], &f, 1e-7), vec![(0.0, true)]);
        assert_eq!(cluster(&[-r, r], &[0.0], &f, 1e-12).len(), 2);
        assert_eq!(cluster(&[0.5], &[], &|t| t - 0.5, 1e-7), vec![(0.5, false)]);
    }

    #[test]
    fn golden_finds_extrema() {
        assert!((golden(&|t| (t - 0.3).powi(2), -1.0, 2.0, false) - 0.3).abs() < 1e-7);
        assert!((golden(&|t| -(t + 0.2).powi(2), -1.0, 2.0, true) + 0.2).abs() < 1e-7);
    }
}
