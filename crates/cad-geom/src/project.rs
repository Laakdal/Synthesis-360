//! Closest-point projection and parameter inversion.
//!
//! Analytic curves and surfaces use closed forms; the ellipse uses Eberly's bisection on the
//! monotone first-quadrant root function ("Distance from a Point to an Ellipse", Geometric
//! Tools), which is robust near the evolute; NURBS use sampling per knot span followed by
//! Newton iteration (P&T §6.1) from every sampled local minimum, compared with the range ends.
//!
//! Returned parameters lie in the natural ranges: `[0, 2π)` for angles, the domain for NURBS
//! (wrapped when periodic). Degenerate positions are resolved deterministically:
//! * on a circle's, cylinder's, cone's, sphere's or torus' axis: u (or t) = 0;
//! * at a sphere's centre: (u, v) = (0, 0); on a torus' core circle: v = 0;
//! * equidistant cone nappes or spindle-torus tubes: the generator/tube at angle u wins.

use crate::{Curve, NurbsCurve, NurbsSurface, Surface, P3};
use std::f64::consts::{PI, TAU};

/// A point within this fraction of the local length scale of an axis/centre counts as on it
/// (its angular coordinate is then meaningless and set to 0).
const AXIS_REL: f64 = 1e-14;
/// Newton iteration limit for NURBS inversion (it converges quadratically in a few steps from a
/// sampled start; the limit only guards against cycling).
const NEWTON_ITERS: usize = 60;
/// Newton stops when the step moves the point by less than this fraction of the model scale.
const STEP_REL: f64 = 1e-15;
/// Samples per knot span (times degree + 1) used to seed NURBS Newton iterations.
const SAMPLES_PER_DEGREE: usize = 3;
/// At most this many sampled local minima are refined by Newton.
const MAX_SEEDS: usize = 16;
/// Step halvings tried before a damped Newton step is abandoned (2^-30 of the full step).
const BACKTRACK_STEPS: usize = 30;
/// Bisection steps for Eberly's root: enough to reach floating-point resolution of the bracket
/// (it halves each step and stops early once the midpoint no longer moves).
const EBERLY_ITERS: usize = 2200;
/// Grid intervals used to find the stationary points of the ellipse distance on an arc.
const ELLIPSE_GRID: usize = 64;
/// Steepest-descent step (fraction of the domain) used only when both the Newton and the
/// Gauss–Newton systems are singular.
const GRADIENT_STEP_REL: f64 = 1e-3;

fn wrap_tau(a: f64) -> f64 {
    let w = a.rem_euclid(TAU);
    if w >= TAU { 0.0 } else { w }
}

/// Polar angle of (x, y) in [0, 2π), 0 when the point is on the axis relative to `scale`.
fn angle(x: f64, y: f64, scale: f64) -> f64 {
    if x.hypot(y) <= AXIS_REL * scale { 0.0 } else { wrap_tau(y.atan2(x)) }
}

fn best(cands: impl IntoIterator<Item = (f64, P3)>, p: &P3) -> (f64, P3, f64) {
    let mut out: Option<(f64, P3, f64)> = None;
    for (t, q) in cands {
        let d = (q - p).norm();
        if out.is_none_or(|o| d < o.2) {
            out = Some((t, q, d));
        }
    }
    out.expect("at least one candidate")
}

/// Closest point of the ellipse (a cos t, b sin t) to (x, y): returns t in [0, 2π).
pub fn ellipse_param(a: f64, b: f64, x: f64, y: f64) -> f64 {
    let (cx, cy) = if a >= b {
        eberly(a, b, x.abs(), y.abs())
    } else {
        let (qy, qx) = eberly(b, a, y.abs(), x.abs());
        (qx, qy)
    };
    let (cx, cy) = (cx.copysign(x), cy.copysign(y));
    wrap_tau((cy / b).atan2(cx / a))
}

/// Eberly's closest point on the ellipse with semi-axes e0 ≥ e1 to (y0, y1) in the first quadrant.
fn eberly(e0: f64, e1: f64, y0: f64, y1: f64) -> (f64, f64) {
    if y1 > 0.0 {
        if y0 > 0.0 {
            let (z0, z1) = (y0 / e0, y1 / e1);
            let g = z0 * z0 + z1 * z1 - 1.0;
            if g == 0.0 {
                return (y0, y1);
            }
            let r0 = (e0 / e1) * (e0 / e1);
            let n0 = r0 * z0;
            let (mut s0, mut s1) = (z1 - 1.0, if g < 0.0 { 0.0 } else { n0.hypot(z1) - 1.0 });
            let mut s = 0.0;
            for _ in 0..EBERLY_ITERS {
                s = 0.5 * (s0 + s1);
                if s == s0 || s == s1 {
                    break;
                }
                let (q0, q1) = (n0 / (s + r0), z1 / (s + 1.0));
                let g = q0 * q0 + q1 * q1 - 1.0;
                if g > 0.0 {
                    s0 = s;
                } else if g < 0.0 {
                    s1 = s;
                } else {
                    break;
                }
            }
            (r0 * y0 / (s + r0), y1 / (s + 1.0))
        } else {
            (0.0, e1)
        }
    } else {
        let (num, den) = (e0 * y0, e0 * e0 - e1 * e1);
        if num < den {
            let x = num / den;
            (e0 * x, e1 * (1.0 - x * x).max(0.0).sqrt())
        } else {
            (e0, 0.0)
        }
    }
}

impl Curve {
    /// Closest point on the whole curve: `(t, point, distance)`. Closed form for line, circle and
    /// ellipse; sampled Newton for NURBS (global over the domain, ends included).
    pub fn project(&self, p: &P3) -> (f64, P3, f64) {
        let t = match self {
            Curve::Line { origin, dir } => (p - origin).dot(dir) / dir.norm_squared(),
            Curve::Circle { frame, radius } => {
                let l = frame.to_local(*p);
                angle(l.x, l.y, l.coords.norm() + radius.abs())
            }
            Curve::Ellipse { frame, major, minor } => {
                let l = frame.to_local(*p);
                ellipse_param(*major, *minor, l.x, l.y)
            }
            Curve::Nurbs(n) => return n.project(p),
        };
        let q = self.point(t);
        (t, q, (q - p).norm())
    }

    /// Closest point on the arc `t ∈ [t0, t1]` (t0 ≤ t1, finite): interior stationary points
    /// compared with both ends. For the ellipse, interior candidates are the global minimum plus
    /// sign changes of the distance derivative on a 64-interval grid (two stationary points
    /// closer than one grid step can be missed; the ends are always considered).
    pub fn project_in(&self, p: &P3, t0: f64, t1: f64) -> (f64, P3, f64) {
        let ends = [(t0, self.point(t0)), (t1, self.point(t1))];
        let shift_in = |t: f64| {
            let s = t0 + (t - t0).rem_euclid(TAU);
            (s <= t1).then_some(s)
        };
        let mut cands: Vec<(f64, P3)> = ends.to_vec();
        match self {
            Curve::Line { .. } => {
                let t = self.project(p).0.clamp(t0, t1);
                cands.push((t, self.point(t)));
            }
            Curve::Circle { .. } => {
                if let Some(s) = shift_in(self.project(p).0) {
                    cands.push((s, self.point(s)));
                }
            }
            Curve::Ellipse { frame, major, minor } => {
                if let Some(s) = shift_in(self.project(p).0) {
                    cands.push((s, self.point(s)));
                }
                let l = frame.to_local(*p);
                let (a, b) = (*major, *minor);
                let g = |t: f64| (b * b - a * a) * t.sin() * t.cos() + a * l.x * t.sin() - b * l.y * t.cos();
                let n = ELLIPSE_GRID;
                for i in 0..n {
                    let (lo, hi) = (t0 + (t1 - t0) * i as f64 / n as f64, t0 + (t1 - t0) * (i + 1) as f64 / n as f64);
                    if g(lo) < 0.0 && g(hi) >= 0.0 {
                        let t = bisect(&g, lo, hi);
                        cands.push((t, self.point(t)));
                    }
                }
            }
            Curve::Nurbs(n) => return n.project_in(p, t0, t1),
        }
        let r = best(cands, p);
        (r.0, r.1, r.2)
    }
}

/// Root of `g` in `[lo, hi]` with `g(lo) < 0 ≤ g(hi)`, to floating-point resolution.
pub(crate) fn bisect(g: &dyn Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> f64 {
    loop {
        let m = 0.5 * (lo + hi);
        if m <= lo || m >= hi {
            return m;
        }
        if g(m) < 0.0 { lo = m } else { hi = m }
    }
}

impl NurbsCurve {
    /// Closest point on the whole curve: `(t, point, distance)`.
    pub fn project(&self, p: &P3) -> (f64, P3, f64) {
        let (a, b) = self.domain();
        self.project_in(p, a, b)
    }

    /// Closest point for `t ∈ [t0, t1]` (inside the domain; periodic curves may use any
    /// sub-range of one period). Seeds: samples per knot span; every sampled local minimum is
    /// refined by Newton on (C − P)·C' = 0 and compared with the range ends.
    pub fn project_in(&self, p: &P3, t0: f64, t1: f64) -> (f64, P3, f64) {
        let scale = crate::bbox::Aabb::from_points(&self.control_points).map_or(1.0, |b| b.diagonal()).max(1.0) + p.coords.amax();
        let mut ts = vec![t0];
        let cuts: Vec<f64> = self.breakpoints().into_iter().filter(|k| *k > t0 && *k < t1).collect();
        let m = SAMPLES_PER_DEGREE * (self.degree + 1);
        let mut prev = t0;
        for k in cuts.into_iter().chain([t1]) {
            for i in 1..=m {
                ts.push(prev + (k - prev) * i as f64 / m as f64);
            }
            prev = k;
        }
        let d: Vec<f64> = ts.iter().map(|&t| (self.point(t) - p).norm()).collect();
        let mut seeds: Vec<usize> = (0..ts.len())
            .filter(|&i| (i == 0 || d[i] <= d[i - 1]) && (i + 1 == ts.len() || d[i] <= d[i + 1]))
            .collect();
        seeds.sort_by(|&i, &j| d[i].total_cmp(&d[j]));
        seeds.truncate(MAX_SEEDS);
        let (whole, periodic) = (self.domain() == (t0, t1), self.periodic);
        let mut cands = vec![(t0, self.point(t0)), (t1, self.point(t1))];
        for i in seeds {
            let mut t = ts[i];
            for _ in 0..NEWTON_ITERS {
                let c = self.derivs(t, 2);
                let r = c[0] - p.coords;
                let f = c[1].dot(&r);
                let gn = c[1].norm_squared();
                let fp = gn + c[2].dot(&r);
                let fp = if fp > 0.0 { fp } else { gn };
                if fp == 0.0 {
                    break;
                }
                let mut tn = t - f / fp;
                tn = if whole && periodic { self.wrap(tn) } else { tn.clamp(t0, t1) };
                let moved = (P3::from(c[0]) - self.point(tn)).norm();
                t = tn;
                if moved <= STEP_REL * scale {
                    break;
                }
            }
            cands.push((t, self.point(t)));
        }
        best(cands, p)
    }
}

impl Surface {
    /// Closest point on the (untrimmed) surface: `(u, v, point, distance)`. Closed forms for
    /// the analytic surfaces (with the degenerate-position rules of the module docs); sampled
    /// Newton for NURBS.
    pub fn project(&self, p: &P3) -> (f64, f64, P3, f64) {
        let (u, v) = match self {
            Surface::Plane { frame } => {
                let l = frame.to_local(*p);
                (l.x, l.y)
            }
            Surface::Cylinder { frame, radius } => {
                let l = frame.to_local(*p);
                (angle(l.x, l.y, l.x.hypot(l.y) + radius.abs()), l.z)
            }
            Surface::Cone { frame, radius, half_angle } => {
                let l = frame.to_local(*p);
                let (rho, k, r) = (l.x.hypot(l.y), half_angle.tan(), *radius);
                let u = angle(l.x, l.y, rho + r.abs() + l.z.abs());
                let v1 = ((rho - r) * k + l.z) / (1.0 + k * k);
                let v2 = (l.z - (rho + r) * k) / (1.0 + k * k);
                let d1 = (rho - r - v1 * k).hypot(l.z - v1);
                let d2 = (rho + r + v2 * k).hypot(l.z - v2);
                if d2 < d1 { (wrap_tau(u + PI), v2) } else { (u, v1) }
            }
            Surface::Sphere { frame, radius } => {
                let l = frame.to_local(*p);
                let rho = l.x.hypot(l.y);
                let scale = l.coords.norm() + radius.abs();
                if l.coords.norm() <= AXIS_REL * scale {
                    (0.0, 0.0)
                } else {
                    (angle(l.x, l.y, scale), l.z.atan2(rho))
                }
            }
            Surface::Torus { frame, major, minor } => {
                let l = frame.to_local(*p);
                let (rho, big) = (l.x.hypot(l.y), *major);
                let scale = l.coords.norm() + big.abs() + minor.abs();
                let u = angle(l.x, l.y, scale);
                let (d1, d2) = ((rho - big).hypot(l.z), (rho + big).hypot(l.z));
                if (d2 - minor).abs() < (d1 - minor).abs() {
                    (wrap_tau(u + PI), wrap_tau(l.z.atan2(-(rho + big))))
                } else if d1 <= AXIS_REL * scale {
                    (u, 0.0)
                } else {
                    (u, wrap_tau(l.z.atan2(rho - big)))
                }
            }
            Surface::Nurbs(n) => return n.project(p),
        };
        let q = self.point(u, v);
        (u, v, q, (q - p).norm())
    }

    /// Signed distance to an analytic surface (positive on the side of the natural normal:
    /// above the plane, outside cylinder, double cone, sphere and torus); `None` for NURBS.
    /// Its zero set is exactly the surface, which the intersection routines rely on.
    pub fn signed_distance(&self, p: &P3) -> Option<f64> {
        let l = self.frame()?.to_local(*p);
        let rho = l.x.hypot(l.y);
        Some(match self {
            Surface::Plane { .. } => l.z,
            Surface::Cylinder { radius, .. } => rho - radius,
            Surface::Sphere { radius, .. } => l.coords.norm() - radius,
            Surface::Torus { major, minor, .. } => (rho - major).hypot(l.z).min((rho + major).hypot(l.z)) - minor,
            Surface::Cone { radius, half_angle, .. } => {
                let k = half_angle.tan();
                let c = 1.0 + k * k;
                let (v1, v2) = (((rho - radius) * k + l.z) / c, (l.z - (rho + radius) * k) / c);
                let d = (rho - radius - v1 * k).hypot(l.z - v1).min((rho + radius + v2 * k).hypot(l.z - v2));
                if rho < (radius + k * l.z).abs() { -d } else { d }
            }
            Surface::Nurbs(_) => unreachable!("frame() is None for NURBS"),
        })
    }
}

impl NurbsSurface {
    /// Closest point on the surface: `(u, v, point, distance)`. Seeds: a grid per knot span;
    /// sampled local minima are refined by damped Newton on ((S − P)·S_u, (S − P)·S_v) = 0,
    /// falling back to Gauss–Newton steps where the Hessian is indefinite or singular (poles).
    pub fn project(&self, p: &P3) -> (f64, f64, P3, f64) {
        let (bu, bv) = self.breakpoints();
        let grid = |b: &[f64], deg: usize| {
            let m = SAMPLES_PER_DEGREE * (deg + 1);
            let mut g = vec![b[0]];
            for w in b.windows(2) {
                g.extend((1..=m).map(|i| w[0] + (w[1] - w[0]) * i as f64 / m as f64));
            }
            g
        };
        let (gu, gv) = (grid(&bu, self.degree_u), grid(&bv, self.degree_v));
        let (nu, nv) = (gu.len(), gv.len());
        let d: Vec<f64> = gu.iter().flat_map(|&u| gv.iter().map(move |&v| (u, v))).map(|(u, v)| (self.point(u, v) - p).norm()).collect();
        let at = |i: usize, j: usize| d[i * nv + j];
        let mut seeds: Vec<(usize, usize)> = Vec::new();
        for i in 0..nu {
            for j in 0..nv {
                let x = at(i, j);
                let lower = (i > 0 && at(i - 1, j) < x) || (i + 1 < nu && at(i + 1, j) < x) || (j > 0 && at(i, j - 1) < x) || (j + 1 < nv && at(i, j + 1) < x);
                if !lower {
                    seeds.push((i, j));
                }
            }
        }
        seeds.sort_by(|a, b| at(a.0, a.1).total_cmp(&at(b.0, b.1)));
        seeds.truncate(MAX_SEEDS);
        let mut out: Option<(f64, f64, P3, f64)> = None;
        for (i, j) in seeds {
            let r = self.project_from(p, gu[i], gv[j]);
            if out.as_ref().is_none_or(|o| r.3 < o.3) {
                out = Some(r);
            }
        }
        out.expect("grid has at least one local minimum")
    }

    /// Local inversion: damped Newton from the start `(u, v)` (no global search), returning
    /// `(u, v, point, distance)` of the stationary point reached. The distance never increases
    /// along the iteration, so the result is at least as close as the start.
    pub fn project_from(&self, p: &P3, u: f64, v: f64) -> (f64, f64, P3, f64) {
        let ((u0, u1), (v0, v1)) = self.domain();
        let scale = self.control_bbox().diagonal().max(1.0) + p.coords.amax();
        let clampw = |u: f64, v: f64| {
            let (u, v) = self.wrap(u, v);
            (if self.periodic_u { u } else { u.clamp(u0, u1) }, if self.periodic_v { v } else { v.clamp(v0, v1) })
        };
        let (mut u, mut v) = clampw(u, v);
        let mut dist = (self.point(u, v) - p).norm();
        for _ in 0..NEWTON_ITERS {
            let s = self.derivs(u, v, 2);
            let r = s[0][0] - p.coords;
            let (su, sv) = (s[1][0], s[0][1]);
            let f = [r.dot(&su), r.dot(&sv)];
            let (a, b, c) = (su.norm_squared(), su.dot(&sv), sv.norm_squared());
            let (h11, h12, h22) = (a + r.dot(&s[2][0]), b + r.dot(&s[1][1]), c + r.dot(&s[0][2]));
            let solve = |m11: f64, m12: f64, m22: f64| {
                let det = m11 * m22 - m12 * m12;
                (det > 0.0 && m11 > 0.0).then(|| ((-f[0] * m22 + f[1] * m12) / det, (f[0] * m12 - f[1] * m11) / det))
            };
            let step = solve(h11, h12, h22).or_else(|| solve(a, b, c)).unwrap_or_else(|| {
                let g = (f[0] * f[0] + f[1] * f[1]).sqrt().max(f64::MIN_POSITIVE);
                (-f[0] / g * GRADIENT_STEP_REL * (u1 - u0), -f[1] / g * GRADIENT_STEP_REL * (v1 - v0))
            });
            let mut lambda = 1.0;
            let mut accepted = None;
            for _ in 0..BACKTRACK_STEPS {
                let (un, vn) = clampw(u + step.0 * lambda, v + step.1 * lambda);
                let dn = (self.point(un, vn) - p).norm();
                if dn <= dist {
                    accepted = Some((un, vn, dn));
                    break;
                }
                lambda *= 0.5;
            }
            let Some((un, vn, dn)) = accepted else { break };
            let moved = (self.point(un, vn) - P3::from(s[0][0])).norm();
            (u, v, dist) = (un, vn, dn);
            if moved <= STEP_REL * scale {
                break;
            }
        }
        (u, v, self.point(u, v), dist)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eberly_reduces_to_polar_angle_on_a_circle() {
        for (x, y) in [(3.0, 4.0), (-1.0, 0.5), (0.0, -2.0), (5.0, 0.0)] {
            let t = ellipse_param(2.0, 2.0, x, y);
            assert!((t - wrap_tau(f64::atan2(y, x))).abs() < 1e-12, "({x}, {y}) → {t}");
        }
    }

    #[test]
    fn eberly_on_axis_inside_evolute() {
        // Inside the evolute on the major axis the nearest point leaves the axis.
        let t = ellipse_param(4.0, 1.0, 1.0, 0.0);
        let (cx, cy) = (4.0 * t.cos(), t.sin());
        assert!(cy.abs() > 0.5, "nearest point off the axis");
        assert!(((cx - 1.0).hypot(cy) - (1.0 - 1.0 / 15.0f64).sqrt()).abs() < 1e-12);
    }
}
