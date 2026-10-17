//! 3D curves. Parameterisations (fixed; topology, mass properties and tests rely on them):
//! * `Line`:    C(t) = origin + t·dir, `dir` unit, so t is arc length.
//! * `Circle`:  C(t) = o + r(cos t·x + sin t·y), period 2π.
//! * `Ellipse`: C(t) = o + a cos t·x + b sin t·y (a = major along x), period 2π.
//! * `Nurbs`:   see `nurbs.rs`.

use crate::{Frame, GResult, GeomError, NurbsCurve, P3, V3};
use serde::{Deserialize, Serialize};

/// Requested relative accuracy of adaptive Gauss–Legendre arc length (close to f64 round-off
/// for the smooth per-span integrands it is applied to).
const LENGTH_REL_TOL: f64 = 1e-13;
/// Bisection depth limit of the adaptive quadrature (2^30 sub-intervals per span at most).
const LENGTH_MAX_DEPTH: u32 = 30;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Curve {
    Line { origin: P3, dir: V3 },
    Circle { frame: Frame, radius: f64 },
    Ellipse { frame: Frame, major: f64, minor: f64 },
    Nurbs(NurbsCurve),
}

impl Curve {
    pub fn line_through(a: P3, b: P3) -> Self {
        Curve::Line { origin: a, dir: (b - a).normalize() }
    }

    /// Short type name used in reports.
    pub fn type_name(&self) -> &'static str {
        match self {
            Curve::Line { .. } => "line",
            Curve::Circle { .. } => "circle",
            Curve::Ellipse { .. } => "ellipse",
            Curve::Nurbs(_) => "nurbs",
        }
    }

    pub fn point(&self, t: f64) -> P3 {
        match self {
            Curve::Line { origin, dir } => origin + dir * t,
            Curve::Circle { frame, radius } => frame.origin + (frame.x * t.cos() + frame.y * t.sin()) * *radius,
            Curve::Ellipse { frame, major, minor } => frame.origin + frame.x * (major * t.cos()) + frame.y * (minor * t.sin()),
            Curve::Nurbs(n) => n.point(t),
        }
    }

    /// First derivative dC/dt.
    pub fn d1(&self, t: f64) -> V3 {
        match self {
            Curve::Line { dir, .. } => *dir,
            Curve::Circle { frame, radius } => (frame.y * t.cos() - frame.x * t.sin()) * *radius,
            Curve::Ellipse { frame, major, minor } => frame.y * (minor * t.cos()) - frame.x * (major * t.sin()),
            Curve::Nurbs(n) => n.d1(t),
        }
    }

    /// Second derivative d²C/dt².
    pub fn d2(&self, t: f64) -> V3 {
        match self {
            Curve::Line { .. } => V3::zeros(),
            Curve::Circle { frame, radius } => -(frame.x * t.cos() + frame.y * t.sin()) * *radius,
            Curve::Ellipse { frame, major, minor } => -(frame.x * (major * t.cos()) + frame.y * (minor * t.sin())),
            Curve::Nurbs(n) => n.d2(t),
        }
    }

    /// Period for periodic curves (circle, ellipse, periodic NURBS).
    pub fn period(&self) -> Option<f64> {
        match self {
            Curve::Line { .. } => None,
            Curve::Circle { .. } | Curve::Ellipse { .. } => Some(std::f64::consts::TAU),
            Curve::Nurbs(n) => n.period(),
        }
    }

    /// Natural parameter range: (−∞, ∞) for a line, [0, 2π] for circle/ellipse, the knot domain
    /// for NURBS.
    pub fn domain(&self) -> (f64, f64) {
        match self {
            Curve::Line { .. } => (f64::NEG_INFINITY, f64::INFINITY),
            Curve::Circle { .. } | Curve::Ellipse { .. } => (0.0, std::f64::consts::TAU),
            Curve::Nurbs(n) => n.domain(),
        }
    }

    /// Parameter wrapped into `[start, start + period)` for periodic curves; unchanged otherwise.
    pub fn wrap(&self, t: f64) -> f64 {
        match (self.period(), self) {
            (Some(_), Curve::Nurbs(n)) => n.wrap(t),
            (Some(p), _) => {
                let w = t.rem_euclid(p);
                if w >= p { 0.0 } else { w }
            }
            (None, _) => t,
        }
    }

    /// Arc length between parameters `t0` and `t1` (order-independent, ≥ 0). Exact for lines
    /// and circles; adaptive 5-point Gauss–Legendre otherwise, applied per NURBS knot span so
    /// the integrand is smooth on each piece (relative accuracy ≈ 1e-13).
    pub fn length(&self, t0: f64, t1: f64) -> f64 {
        let (a, b) = if t0 <= t1 { (t0, t1) } else { (t1, t0) };
        match self {
            Curve::Line { dir, .. } => dir.norm() * (b - a),
            Curve::Circle { radius, .. } => radius.abs() * (b - a),
            Curve::Ellipse { .. } => integrate_speed(self, &[a, b]),
            Curve::Nurbs(n) => {
                let bp = n.breakpoints();
                let mut cuts = vec![a];
                let (d0, d1) = n.domain();
                let shifts = match n.period() {
                    Some(p) => ((a - d0) / p).floor() as i64..=((b - d0) / p).ceil() as i64,
                    None => 0..=0,
                };
                let per = d1 - d0;
                for s in shifts {
                    for k in &bp {
                        let x = k + s as f64 * per;
                        if x > a && x < b {
                            cuts.push(x);
                        }
                    }
                }
                cuts.sort_by(f64::total_cmp);
                cuts.dedup();
                cuts.push(b);
                integrate_speed(self, &cuts)
            }
        }
    }

    /// Exact NURBS form of the curve restricted to `[t0, t1]` (finite, t0 < t1): a degree-1
    /// segment for a line, rational quadratic arcs for circle and ellipse (whose NURBS
    /// parameter matches the angle only at the knots), the extracted sub-curve for NURBS.
    pub fn to_nurbs(&self, t0: f64, t1: f64) -> GResult<NurbsCurve> {
        if !(t0.is_finite() && t1.is_finite() && t0 < t1) {
            return Err(GeomError::InvalidInput(format!("to_nurbs: invalid range [{t0}, {t1}]")));
        }
        match self {
            Curve::Line { .. } => NurbsCurve::segment(self.point(t0), self.point(t1), t0, t1),
            Curve::Circle { frame, radius } => NurbsCurve::circle_arc(frame, *radius, t0, t1),
            Curve::Ellipse { frame, major, minor } => NurbsCurve::ellipse_arc(frame, *major, *minor, t0, t1),
            Curve::Nurbs(n) => n.extract(t0, t1),
        }
    }
}

/// 5-point Gauss–Legendre nodes and weights on [−1, 1] (closed forms).
fn gauss5() -> ([f64; 5], [f64; 5]) {
    let s = (10.0f64 / 7.0).sqrt();
    let (x1, x2) = ((5.0 - 2.0 * s).sqrt() / 3.0, (5.0 + 2.0 * s).sqrt() / 3.0);
    let r = 70.0f64.sqrt();
    let (w1, w2) = ((322.0 + 13.0 * r) / 900.0, (322.0 - 13.0 * r) / 900.0);
    ([-x2, -x1, 0.0, x1, x2], [w2, w1, 128.0 / 225.0, w1, w2])
}

/// ∫|C'(t)| dt over consecutive pieces of `cuts`.
fn integrate_speed(c: &Curve, cuts: &[f64]) -> f64 {
    let (x, w) = gauss5();
    let gl = |a: f64, b: f64| {
        let (m, h) = ((a + b) / 2.0, (b - a) / 2.0);
        h * (0..5).map(|i| w[i] * c.d1(m + h * x[i]).norm()).sum::<f64>()
    };
    fn adapt(gl: &dyn Fn(f64, f64) -> f64, a: f64, b: f64, whole: f64, tol: f64, depth: u32) -> f64 {
        let m = (a + b) / 2.0;
        let (l, r) = (gl(a, m), gl(m, b));
        if depth == 0 || (l + r - whole).abs() <= tol {
            l + r
        } else {
            adapt(gl, a, m, l, tol / 2.0, depth - 1) + adapt(gl, m, b, r, tol / 2.0, depth - 1)
        }
    }
    cuts.windows(2)
        .filter(|s| s[1] > s[0])
        .map(|s| {
            let whole = gl(s[0], s[1]);
            let tol = LENGTH_REL_TOL * whole.abs().max(f64::MIN_POSITIVE);
            adapt(&gl, s[0], s[1], whole, tol, LENGTH_MAX_DEPTH)
        })
        .sum()
}
