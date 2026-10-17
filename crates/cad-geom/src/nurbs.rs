//! Rational B-spline curves and surfaces (Piegl & Tiller, "The NURBS Book").
//! Knot vectors are full (clamped vectors repeat end knots degree+1 times); weights are per
//! control point (all 1.0 for non-rational splines).
//!
//! **Domain.** With `count` control points and degree `p` the knot vector has `count + p + 1`
//! entries and the curve is defined on `[knots[p], knots[count]]` (the whole knot range for a
//! clamped vector). A non-periodic curve evaluated outside its domain extrapolates the end span.
//!
//! **Periodic convention.** A periodic curve stores an *unclamped* knot vector whose knot
//! intervals repeat with period `count − p`, and a *wrapped* control polygon: the last `p`
//! control points (and weights) repeat the first `p`. The period is `knots[count] − knots[p]`
//! (see [`NurbsCurve::period`]) and evaluation wraps the parameter into the domain before running
//! the ordinary algorithms. [`NurbsCurve::periodic_uniform`] builds such a curve from the
//! distinct control points. Surfaces follow the same convention independently in u and v.
//!
//! Algorithm numbers (A2.1 …) refer to the book.

use crate::{Frame, GResult, GeomError, P3, V3};
use nalgebra::Vector4;
use serde::{Deserialize, Serialize};

type V4 = Vector4<f64>;

/// Knots closer than this fraction of the domain length are treated as the same knot when
/// inserting or splitting, so round-off cannot create near-empty spans.
const KNOT_SNAP: f64 = 1e-12;
/// Relative agreement required between the wrapped control points/weights/knot intervals of a
/// periodic spline (they are copies, so only round-off may separate them).
const WRAP_MATCH: f64 = 1e-9;
/// An arc sweep within this many radians above a multiple of π/2 does not get an extra segment.
const ARC_SEGMENT_SLACK: f64 = 1e-9;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NurbsCurve {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub control_points: Vec<P3>,
    pub weights: Vec<f64>,
    pub periodic: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NurbsSurface {
    pub degree_u: usize,
    pub degree_v: usize,
    pub knots_u: Vec<f64>,
    pub knots_v: Vec<f64>,
    /// Row-major: `control_points[i * count_v + j]` is P(i, j), i along u.
    pub control_points: Vec<P3>,
    pub weights: Vec<f64>,
    pub count_u: usize,
    pub count_v: usize,
    pub periodic_u: bool,
    pub periodic_v: bool,
}

/// Knot span index `k` with `knots[k] ≤ u < knots[k+1]`, `p ≤ k < count` (A2.1). Parameters at
/// or beyond the domain ends map to the first/last non-empty span.
pub fn find_span(knots: &[f64], p: usize, count: usize, u: f64) -> usize {
    let n = count - 1;
    if u >= knots[n + 1] {
        let mut k = n;
        while k > p && knots[k] >= knots[n + 1] {
            k -= 1;
        }
        return k;
    }
    if u <= knots[p] {
        let mut k = p;
        while k < n && knots[k + 1] <= knots[p] {
            k += 1;
        }
        return k;
    }
    knots[..=n + 1].partition_point(|&x| x <= u) - 1
}

/// The `p + 1` non-zero basis functions `N[span−p..=span], p` at `u` (A2.2).
pub fn basis_funs(knots: &[f64], p: usize, span: usize, u: f64) -> Vec<f64> {
    let mut n = vec![0.0; p + 1];
    let (mut left, mut right) = (vec![0.0; p + 1], vec![0.0; p + 1]);
    n[0] = 1.0;
    for j in 1..=p {
        left[j] = u - knots[span + 1 - j];
        right[j] = knots[span + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            let tmp = n[r] / (right[r + 1] + left[j - r]);
            n[r] = saved + right[r + 1] * tmp;
            saved = left[j - r] * tmp;
        }
        n[j] = saved;
    }
    n
}

/// Basis functions and their derivatives up to order `nd`: `ders[k][j]` is the k-th derivative
/// of `N[span−p+j], p` at `u` (A2.3). Orders above `p` are zero.
pub fn ders_basis_funs(knots: &[f64], p: usize, span: usize, u: f64, nd: usize) -> Vec<Vec<f64>> {
    let mut ndu = vec![vec![0.0; p + 1]; p + 1];
    let (mut left, mut right) = (vec![0.0; p + 1], vec![0.0; p + 1]);
    ndu[0][0] = 1.0;
    for j in 1..=p {
        left[j] = u - knots[span + 1 - j];
        right[j] = knots[span + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            ndu[j][r] = right[r + 1] + left[j - r];
            let tmp = ndu[r][j - 1] / ndu[j][r];
            ndu[r][j] = saved + right[r + 1] * tmp;
            saved = left[j - r] * tmp;
        }
        ndu[j][j] = saved;
    }
    let mut ders = vec![vec![0.0; p + 1]; nd + 1];
    for j in 0..=p {
        ders[0][j] = ndu[j][p];
    }
    let top = nd.min(p);
    let mut a = [vec![0.0; p + 1], vec![0.0; p + 1]];
    for r in 0..=p {
        let (mut s1, mut s2) = (0, 1);
        a[0][0] = 1.0;
        for k in 1..=top {
            let mut d = 0.0;
            let rk = r as isize - k as isize;
            let pk = p - k;
            if r >= k {
                a[s2][0] = a[s1][0] / ndu[pk + 1][rk as usize];
                d = a[s2][0] * ndu[rk as usize][pk];
            }
            let j1 = if rk >= -1 { 1 } else { (-rk) as usize };
            let j2 = if r as isize - 1 <= pk as isize { k - 1 } else { p - r };
            for j in j1..=j2 {
                let c = (rk + j as isize) as usize;
                a[s2][j] = (a[s1][j] - a[s1][j - 1]) / ndu[pk + 1][c];
                d += a[s2][j] * ndu[c][pk];
            }
            if r <= pk {
                a[s2][k] = -a[s1][k - 1] / ndu[pk + 1][r];
                d += a[s2][k] * ndu[r][pk];
            }
            ders[k][r] = d;
            std::mem::swap(&mut s1, &mut s2);
        }
    }
    let mut f = p as f64;
    for (k, row) in ders.iter_mut().enumerate().take(top + 1).skip(1) {
        row.iter_mut().for_each(|x| *x *= f);
        f *= (p - k) as f64;
    }
    ders
}

/// Binomial coefficient C(n, k) as f64 (small arguments only).
pub(crate) fn binom(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

fn homog(p: &P3, w: f64) -> V4 {
    V4::new(p.x * w, p.y * w, p.z * w, w)
}

fn dehomog(h: &V4) -> P3 {
    P3::new(h.x / h.w, h.y / h.w, h.z / h.w)
}

/// Rational derivatives from homogeneous ones (A4.2): `ck[k]` = k-th derivative of the curve
/// (`ck[0]` as a position vector).
fn rational_derivs(aw: &[V4]) -> Vec<V3> {
    let mut ck: Vec<V3> = Vec::with_capacity(aw.len());
    for k in 0..aw.len() {
        let mut v = aw[k].xyz();
        for i in 1..=k {
            v -= ck[k - i] * (binom(k, i) * aw[i].w);
        }
        ck.push(v / aw[0].w);
    }
    ck
}

fn wrap_param(t: f64, a: f64, b: f64) -> f64 {
    let w = a + (t - a).rem_euclid(b - a);
    if w >= b { a } else { w }
}

fn bad(msg: impl Into<String>) -> GeomError {
    GeomError::InvalidInput(msg.into())
}

/// Shared validation for one parametric direction.
fn check_direction(p: usize, knots: &[f64], count: usize, what: &str) -> GResult<()> {
    if p == 0 {
        return Err(bad(format!("{what}: degree must be at least 1")));
    }
    if count < p + 1 {
        return Err(bad(format!("{what}: need at least degree + 1 = {} control points, got {count}", p + 1)));
    }
    if knots.len() != count + p + 1 {
        return Err(bad(format!("{what}: knot count {} != control points {count} + degree {p} + 1", knots.len())));
    }
    if knots.iter().any(|k| !k.is_finite()) || knots.windows(2).any(|w| w[1] < w[0]) {
        return Err(bad(format!("{what}: knots must be finite and non-decreasing")));
    }
    let (a, b) = (knots[p], knots[count]);
    if a >= b {
        return Err(bad(format!("{what}: empty domain [{a}, {b}]")));
    }
    let mut i = 0;
    while i < knots.len() {
        let m = knots[i..].iter().take_while(|&&k| k == knots[i]).count();
        let interior = knots[i] > a && knots[i] < b;
        if m > p + 1 || (interior && m > p) {
            return Err(bad(format!("{what}: knot {} has multiplicity {m} (max {})", knots[i], if interior { p } else { p + 1 })));
        }
        i += m;
    }
    Ok(())
}

fn check_weights(weights: &[f64], count: usize, what: &str) -> GResult<()> {
    if weights.len() != count {
        return Err(bad(format!("{what}: {} weights for {count} control points", weights.len())));
    }
    if weights.iter().any(|w| !w.is_finite() || *w <= 0.0) {
        return Err(bad(format!("{what}: weights must be finite and > 0")));
    }
    Ok(())
}

/// Checks the periodic convention for one direction: knot intervals repeat with period
/// `count − p` and `same(i, i + count − p)` holds for the first `p` control indices.
fn check_periodic(p: usize, knots: &[f64], count: usize, same: impl Fn(usize, usize) -> bool, what: &str) -> GResult<()> {
    let m = count - p;
    let len = knots[count] - knots[p];
    for i in 0..knots.len() - 1 - m {
        let (d0, d1) = (knots[i + 1] - knots[i], knots[i + m + 1] - knots[i + m]);
        if (d0 - d1).abs() > WRAP_MATCH * len {
            return Err(bad(format!("{what}: periodic knot intervals do not repeat")));
        }
    }
    if (0..p).any(|i| !same(i, i + m)) {
        return Err(bad(format!("{what}: periodic control points/weights are not wrapped (last p must repeat first p)")));
    }
    Ok(())
}

fn close(a: &P3, b: &P3, wa: f64, wb: f64, scale: f64) -> bool {
    (a - b).norm() <= WRAP_MATCH * scale && (wa - wb).abs() <= WRAP_MATCH * wa.max(wb)
}

fn point_scale(pts: &[P3]) -> f64 {
    pts.iter().map(|p| p.coords.amax()).fold(1.0, f64::max)
}

/// Inserts `u` once into a homogeneous control polygon (Boehm / A5.1 with r = 1).
fn insert_once(p: usize, knots: &mut Vec<f64>, pw: &mut Vec<V4>, u: f64) {
    let count = pw.len();
    let k = find_span(knots, p, count, u);
    let q: Vec<V4> = (0..=count)
        .map(|i| {
            if i + p <= k {
                pw[i]
            } else if i > k {
                pw[i - 1]
            } else {
                let a = (u - knots[i]) / (knots[i + p] - knots[i]);
                pw[i] * a + pw[i - 1] * (1.0 - a)
            }
        })
        .collect();
    knots.insert(k + 1, u);
    *pw = q;
}

fn multiplicity(knots: &[f64], u: f64) -> usize {
    knots.iter().filter(|&&k| k == u).count()
}

/// Moves `u` onto an existing knot when it lies within [`KNOT_SNAP`] of one.
fn snap(knots: &[f64], u: f64, len: f64) -> f64 {
    knots.iter().copied().find(|k| (k - u).abs() <= KNOT_SNAP * len).unwrap_or(u)
}

/// Clamped sub-spline on `[a, b]` (both inside the domain, already snapped): inserts each end to
/// multiplicity `p`, then keeps the control points that influence `[a, b]`.
fn extract_raw(p: usize, knots: &[f64], pw: &[V4], a: f64, b: f64) -> (Vec<f64>, Vec<V4>) {
    let (mut k, mut q) = (knots.to_vec(), pw.to_vec());
    for x in [a, b] {
        for _ in multiplicity(&k, x)..p {
            insert_once(p, &mut k, &mut q, x);
        }
    }
    let l = k.iter().rposition(|&x| x == a).expect("inserted knot");
    let f = k.iter().position(|&x| x == b).expect("inserted knot");
    let mut nk = k[l - p..=f + p].to_vec();
    nk[0] = a;
    *nk.last_mut().expect("non-empty") = b;
    (nk, q[l - p..f].to_vec())
}

impl NurbsCurve {
    /// Validated non-periodic curve: knot count = control points + degree + 1, knots
    /// non-decreasing (multiplicity ≤ degree inside the domain, ≤ degree + 1 elsewhere), weights
    /// finite and > 0. Unclamped knot vectors are allowed (domain `[knots[p], knots[count]]`).
    pub fn new(degree: usize, knots: Vec<f64>, control_points: Vec<P3>, weights: Vec<f64>) -> GResult<Self> {
        let c = Self { degree, knots, control_points, weights, periodic: false };
        c.validate()?;
        Ok(c)
    }

    /// Validated periodic curve in the wrapped convention of the module docs.
    pub fn new_periodic(degree: usize, knots: Vec<f64>, control_points: Vec<P3>, weights: Vec<f64>) -> GResult<Self> {
        let c = Self { degree, knots, control_points, weights, periodic: true };
        c.validate()?;
        Ok(c)
    }

    /// Non-rational (all weights 1) non-periodic B-spline.
    pub fn bspline(degree: usize, knots: Vec<f64>, control_points: Vec<P3>) -> GResult<Self> {
        let w = vec![1.0; control_points.len()];
        Self::new(degree, knots, control_points, w)
    }

    /// Closed periodic curve through the distinct control points `points` (≥ degree + 1), with
    /// uniform integer knots: domain `[0, points.len()]`, period `points.len()`.
    pub fn periodic_uniform(degree: usize, points: &[P3], weights: &[f64]) -> GResult<Self> {
        let m = points.len();
        if m < degree + 1 || weights.len() != m {
            return Err(bad("periodic_uniform: need ≥ degree + 1 points and one weight per point"));
        }
        let cps = (0..m + degree).map(|i| points[i % m]).collect();
        let w = (0..m + degree).map(|i| weights[i % m]).collect();
        let knots = (0..m + 2 * degree + 1).map(|i| i as f64 - degree as f64).collect();
        Self::new_periodic(degree, knots, cps, w)
    }

    /// Checks the invariants listed on [`NurbsCurve::new`] (and the wrap for periodic curves).
    pub fn validate(&self) -> GResult<()> {
        let n = self.control_points.len();
        check_direction(self.degree, &self.knots, n, "curve")?;
        check_weights(&self.weights, n, "curve")?;
        if self.control_points.iter().any(|p| !p.coords.iter().all(|c| c.is_finite())) {
            return Err(bad("curve: control points must be finite"));
        }
        if self.periodic {
            let s = point_scale(&self.control_points);
            let (cp, w) = (&self.control_points, &self.weights);
            check_periodic(self.degree, &self.knots, n, |i, j| close(&cp[i], &cp[j], w[i], w[j], s), "curve")?;
        }
        Ok(())
    }

    /// Exact circular arc (rational quadratic, A7.1) on `frame` from angle `t0` to `t1`
    /// (0 < t1 − t0 ≤ 2π), split into segments of at most 90°. The domain is `[t0, t1]` and the
    /// parameter equals the angle at the knots (not in between). A full circle is closed but
    /// clamped, so `periodic` is false.
    pub fn circle_arc(frame: &Frame, radius: f64, t0: f64, t1: f64) -> GResult<Self> {
        Self::ellipse_arc(frame, radius, radius, t0, t1)
    }

    /// Exact elliptical arc (affine image of [`NurbsCurve::circle_arc`]) with semi-axes `a`
    /// along `frame.x` and `b` along `frame.y`, between the ellipse angles `t0` and `t1`.
    pub fn ellipse_arc(frame: &Frame, a: f64, b: f64, t0: f64, t1: f64) -> GResult<Self> {
        let (knots, pts, w) = unit_arc(t0, t1)?;
        let cps = pts.iter().map(|q| frame.origin + frame.x * (a * q[0]) + frame.y * (b * q[1])).collect();
        Self::new(2, knots, cps, w)
    }

    /// Straight segment from `a` (at `t0`) to `b` (at `t1`) as a degree-1 curve.
    pub fn segment(a: P3, b: P3, t0: f64, t1: f64) -> GResult<Self> {
        Self::bspline(1, vec![t0, t0, t1, t1], vec![a, b])
    }

    pub fn count(&self) -> usize {
        self.control_points.len()
    }

    /// Parameter domain `[knots[p], knots[count]]`.
    pub fn domain(&self) -> (f64, f64) {
        (self.knots[self.degree], self.knots[self.count()])
    }

    /// Parameter wrapped into the domain for periodic curves; unchanged otherwise.
    pub fn wrap(&self, t: f64) -> f64 {
        let (a, b) = self.domain();
        if self.periodic { wrap_param(t, a, b) } else { t }
    }

    fn pw(&self) -> Vec<V4> {
        self.control_points.iter().zip(&self.weights).map(|(p, w)| homog(p, *w)).collect()
    }

    fn from_homog(p: usize, knots: Vec<f64>, pw: &[V4]) -> Self {
        Self {
            degree: p,
            knots,
            control_points: pw.iter().map(dehomog).collect(),
            weights: pw.iter().map(|h| h.w).collect(),
            periodic: false,
        }
    }

    /// Point and derivatives up to order `nd` at `t`: `[C, C', C'', …]` (A2.3 + A4.2), with
    /// `C` as a position vector.
    pub fn derivs(&self, t: f64, nd: usize) -> Vec<V3> {
        let (p, t) = (self.degree, self.wrap(t));
        let span = find_span(&self.knots, p, self.count(), t);
        let ders = ders_basis_funs(&self.knots, p, span, t, nd.min(p));
        let aw: Vec<V4> = (0..=nd)
            .map(|k| {
                if k > p {
                    return V4::zeros();
                }
                (0..=p).fold(V4::zeros(), |acc, j| {
                    let i = span - p + j;
                    acc + homog(&self.control_points[i], self.weights[i]) * ders[k][j]
                })
            })
            .collect();
        rational_derivs(&aw)
    }

    pub fn point(&self, t: f64) -> P3 {
        P3::from(self.derivs(t, 0)[0])
    }
    pub fn d1(&self, t: f64) -> V3 {
        self.derivs(t, 1)[1]
    }
    pub fn d2(&self, t: f64) -> V3 {
        self.derivs(t, 2)[2]
    }
    pub fn period(&self) -> Option<f64> {
        self.periodic.then(|| self.knots[self.knots.len() - 1 - self.degree] - self.knots[self.degree])
    }

    /// Distinct knot values inside the domain including both ends (span boundaries).
    pub fn breakpoints(&self) -> Vec<f64> {
        let (a, b) = self.domain();
        let mut v: Vec<f64> = vec![a];
        for &k in &self.knots {
            if k > *v.last().expect("non-empty") && k < b {
                v.push(k);
            }
        }
        v.push(b);
        v
    }

    fn snapped(&self, u: f64) -> f64 {
        let (a, b) = self.domain();
        snap(&self.knots, u, b - a)
    }

    /// Clamped, non-periodic sub-curve on `[a, b]` (within the domain, a < b). The geometry is
    /// unchanged; arcs crossing the seam of a periodic curve must be extracted in two parts.
    pub fn extract(&self, a: f64, b: f64) -> GResult<Self> {
        let (d0, d1) = self.domain();
        let (a, b) = (self.snapped(a), self.snapped(b));
        let slack = KNOT_SNAP * (d1 - d0);
        if !(a < b && a >= d0 - slack && b <= d1 + slack) {
            return Err(bad(format!("extract: [{a}, {b}] is not an increasing range inside [{d0}, {d1}]")));
        }
        let (a, b) = (a.max(d0), b.min(d1));
        let (k, q) = extract_raw(self.degree, &self.knots, &self.pw(), a, b);
        Ok(Self::from_homog(self.degree, k, &q))
    }

    /// The same curve with a clamped knot vector (periodic curves become non-periodic).
    pub fn clamped(&self) -> Self {
        let (a, b) = self.domain();
        self.extract(a, b).expect("domain is a valid range")
    }

    /// Inserts knot `u` (strictly inside the domain) `times` times (A5.1); the curve is unchanged.
    /// Fails when the multiplicity would exceed the degree. Periodic curves are clamped first, so
    /// the result is non-periodic.
    pub fn insert_knot(&self, u: f64, times: usize) -> GResult<Self> {
        let (a, b) = self.domain();
        let u = self.snapped(u);
        if !(u > a && u < b) {
            return Err(bad(format!("insert_knot: {u} is not inside the domain ({a}, {b})")));
        }
        let mut c = if self.periodic { self.clamped() } else { self.clone() };
        if multiplicity(&c.knots, u) + times > c.degree {
            return Err(bad(format!("insert_knot: multiplicity of {u} would exceed the degree {}", c.degree)));
        }
        let mut pw = c.pw();
        for _ in 0..times {
            insert_once(c.degree, &mut c.knots, &mut pw, u);
        }
        Ok(Self::from_homog(c.degree, c.knots, &pw))
    }

    /// Splits at `t` (strictly inside the domain) into two clamped curves on `[a, t]`, `[t, b]`.
    pub fn split(&self, t: f64) -> GResult<(Self, Self)> {
        let (a, b) = self.domain();
        let t = self.snapped(t);
        if !(t > a && t < b) {
            return Err(bad(format!("split: {t} is not inside the domain ({a}, {b})")));
        }
        Ok((self.extract(a, t)?, self.extract(t, b)?))
    }

    /// The curve as clamped Bézier segments (one per non-empty knot span).
    pub fn bezier_segments(&self) -> Vec<Self> {
        self.breakpoints().windows(2).map(|w| self.extract(w[0], w[1]).expect("span inside domain")).collect()
    }

    /// Degree elevation by one (exact, same parameterisation; P&T 5.5 via Bézier decomposition):
    /// every Bézier segment is elevated and the segments are rejoined with interior knot
    /// multiplicity `degree + 1`. Higher continuity at the joins is kept geometrically but not
    /// in the knot vector (no knot removal), so the result has more control points than the
    /// minimal representation. The result is clamped and non-periodic.
    pub fn elevate_degree(&self) -> Self {
        let p = self.degree;
        let segs = self.bezier_segments();
        let mut knots = Vec::new();
        let mut pw: Vec<V4> = Vec::new();
        for (s, seg) in segs.iter().enumerate() {
            let q = seg.pw();
            let e: Vec<V4> = (0..=p + 1)
                .map(|i| {
                    let a = i as f64 / (p + 1) as f64;
                    let lo = if i > 0 { q[i - 1] * a } else { V4::zeros() };
                    let hi = if i <= p { q[i] * (1.0 - a) } else { V4::zeros() };
                    lo + hi
                })
                .collect();
            let (a, b) = seg.domain();
            if s == 0 {
                knots.extend(std::iter::repeat_n(a, p + 2));
                pw.extend_from_slice(&e);
            } else {
                knots.extend(std::iter::repeat_n(a, p + 1));
                pw.extend_from_slice(&e[1..]);
            }
            if s + 1 == segs.len() {
                knots.extend(std::iter::repeat_n(b, p + 2));
            }
        }
        Self::from_homog(p + 1, knots, &pw)
    }
}

/// Knots, 2D control points and weights of a planar rational arc.
pub(crate) type ArcData = (Vec<f64>, Vec<[f64; 2]>, Vec<f64>);

/// Rational quadratic arc of the unit circle from angle `a0` to `a1` (A7.1): knots spanning
/// `[a0, a1]`, 2D control points, weights.
pub(crate) fn unit_arc(a0: f64, a1: f64) -> GResult<ArcData> {
    let sweep = a1 - a0;
    if !(sweep > 0.0 && sweep <= std::f64::consts::TAU + ARC_SEGMENT_SLACK) {
        return Err(bad(format!("arc: sweep {sweep} must be in (0, 2π]")));
    }
    let n = ((sweep / std::f64::consts::FRAC_PI_2 - ARC_SEGMENT_SLACK).ceil() as usize).max(1);
    let d = sweep / n as f64;
    let w1 = (d / 2.0).cos();
    let mut pts = vec![[a0.cos(), a0.sin()]];
    let mut w = vec![1.0];
    let mut knots = vec![a0; 3];
    for i in 1..=n {
        let (m, e) = (a0 + (i as f64 - 0.5) * d, if i == n { a1 } else { a0 + i as f64 * d });
        pts.push([m.cos() / w1, m.sin() / w1]);
        pts.push([e.cos(), e.sin()]);
        w.extend([w1, 1.0]);
        knots.extend(if i == n { [a1; 3].to_vec() } else { vec![e; 2] });
    }
    Ok((knots, pts, w))
}

impl NurbsSurface {
    /// Validated surface (same rules per direction as [`NurbsCurve::new`]; the periodic flags
    /// demand the wrapped convention in that direction).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        degree_u: usize,
        degree_v: usize,
        knots_u: Vec<f64>,
        knots_v: Vec<f64>,
        control_points: Vec<P3>,
        weights: Vec<f64>,
        count_u: usize,
        count_v: usize,
    ) -> GResult<Self> {
        let s = Self { degree_u, degree_v, knots_u, knots_v, control_points, weights, count_u, count_v, periodic_u: false, periodic_v: false };
        s.validate()?;
        Ok(s)
    }

    /// Checks knot vectors, counts, weights and (for periodic directions) the wrap.
    pub fn validate(&self) -> GResult<()> {
        let (nu, nv) = (self.count_u, self.count_v);
        check_direction(self.degree_u, &self.knots_u, nu, "surface u")?;
        check_direction(self.degree_v, &self.knots_v, nv, "surface v")?;
        if self.control_points.len() != nu * nv {
            return Err(bad(format!("surface: {} control points for {nu} × {nv}", self.control_points.len())));
        }
        check_weights(&self.weights, nu * nv, "surface")?;
        let s = point_scale(&self.control_points);
        let (cp, w) = (&self.control_points, &self.weights);
        if self.periodic_u {
            let same = |i: usize, k: usize| (0..nv).all(|j| close(&cp[i * nv + j], &cp[k * nv + j], w[i * nv + j], w[k * nv + j], s));
            check_periodic(self.degree_u, &self.knots_u, nu, same, "surface u")?;
        }
        if self.periodic_v {
            let same = |j: usize, k: usize| (0..nu).all(|i| close(&cp[i * nv + j], &cp[i * nv + k], w[i * nv + j], w[i * nv + k], s));
            check_periodic(self.degree_v, &self.knots_v, nv, same, "surface v")?;
        }
        Ok(())
    }

    /// Parameter domains `((u0, u1), (v0, v1))`.
    pub fn domain(&self) -> ((f64, f64), (f64, f64)) {
        (
            (self.knots_u[self.degree_u], self.knots_u[self.count_u]),
            (self.knots_v[self.degree_v], self.knots_v[self.count_v]),
        )
    }

    /// Parameters wrapped into the domain in periodic directions.
    pub fn wrap(&self, u: f64, v: f64) -> (f64, f64) {
        let ((u0, u1), (v0, v1)) = self.domain();
        (
            if self.periodic_u { wrap_param(u, u0, u1) } else { u },
            if self.periodic_v { wrap_param(v, v0, v1) } else { v },
        )
    }

    fn pw(&self) -> Vec<V4> {
        self.control_points.iter().zip(&self.weights).map(|(p, w)| homog(p, *w)).collect()
    }

    /// Derivatives up to total order `d`: `skl[k][l]` = ∂^{k+l}S/∂u^k∂v^l (A3.6 + A4.4), with
    /// `skl[0][0]` the point as a position vector. Entries with k + l > d are zero.
    pub fn derivs(&self, u: f64, v: f64, d: usize) -> Vec<Vec<V3>> {
        let (p, q) = (self.degree_u, self.degree_v);
        let (u, v) = self.wrap(u, v);
        let su = find_span(&self.knots_u, p, self.count_u, u);
        let sv = find_span(&self.knots_v, q, self.count_v, v);
        let nu = ders_basis_funs(&self.knots_u, p, su, u, d.min(p));
        let nv = ders_basis_funs(&self.knots_v, q, sv, v, d.min(q));
        let mut aw = vec![vec![V4::zeros(); d + 1]; d + 1];
        for k in 0..=d.min(p) {
            let temp: Vec<V4> = (0..=q)
                .map(|s| {
                    (0..=p).fold(V4::zeros(), |acc, r| {
                        let idx = (su - p + r) * self.count_v + sv - q + s;
                        acc + homog(&self.control_points[idx], self.weights[idx]) * nu[k][r]
                    })
                })
                .collect();
            for l in 0..=(d - k).min(q) {
                aw[k][l] = (0..=q).fold(V4::zeros(), |acc, s| acc + temp[s] * nv[l][s]);
            }
        }
        let mut skl = vec![vec![V3::zeros(); d + 1]; d + 1];
        for k in 0..=d {
            for l in 0..=d - k {
                let mut x = aw[k][l].xyz();
                for j in 1..=l {
                    x -= skl[k][l - j] * (binom(l, j) * aw[0][j].w);
                }
                for i in 1..=k {
                    x -= skl[k - i][l] * (binom(k, i) * aw[i][0].w);
                    let mut x2 = V3::zeros();
                    for j in 1..=l {
                        x2 += skl[k - i][l - j] * (binom(l, j) * aw[i][j].w);
                    }
                    x -= x2 * binom(k, i);
                }
                skl[k][l] = x / aw[0][0].w;
            }
        }
        skl
    }

    pub fn point(&self, u: f64, v: f64) -> P3 {
        P3::from(self.derivs(u, v, 0)[0][0])
    }
    pub fn d1(&self, u: f64, v: f64) -> (V3, V3) {
        let s = self.derivs(u, v, 1);
        (s[1][0], s[0][1])
    }
    /// Second partials (S_uu, S_uv, S_vv).
    pub fn d2(&self, u: f64, v: f64) -> (V3, V3, V3) {
        let s = self.derivs(u, v, 2);
        (s[2][0], s[1][1], s[0][2])
    }
    pub fn periods(&self) -> (Option<f64>, Option<f64>) {
        let span = |k: &[f64], p: usize| k[k.len() - 1 - p] - k[p];
        (
            self.periodic_u.then(|| span(&self.knots_u, self.degree_u)),
            self.periodic_v.then(|| span(&self.knots_v, self.degree_v)),
        )
    }

    /// Distinct span boundaries in u and v (including the domain ends).
    pub fn breakpoints(&self) -> (Vec<f64>, Vec<f64>) {
        let bp = |k: &[f64], p: usize, n: usize| {
            let (a, b) = (k[p], k[n]);
            let mut v = vec![a];
            for &x in k {
                if x > *v.last().expect("non-empty") && x < b {
                    v.push(x);
                }
            }
            v.push(b);
            v
        };
        (bp(&self.knots_u, self.degree_u, self.count_u), bp(&self.knots_v, self.degree_v, self.count_v))
    }

    /// Clamped, non-periodic sub-patch on `[u0, u1] × [v0, v1]` (inside the domain); the
    /// geometry is unchanged.
    pub fn extract(&self, u: (f64, f64), v: (f64, f64)) -> GResult<Self> {
        let ((a0, a1), (b0, b1)) = self.domain();
        let (su, sv) = (KNOT_SNAP * (a1 - a0), KNOT_SNAP * (b1 - b0));
        let (u0, u1) = (snap(&self.knots_u, u.0, a1 - a0), snap(&self.knots_u, u.1, a1 - a0));
        let (v0, v1) = (snap(&self.knots_v, v.0, b1 - b0), snap(&self.knots_v, v.1, b1 - b0));
        if !(u0 < u1 && v0 < v1 && u0 >= a0 - su && u1 <= a1 + su && v0 >= b0 - sv && v1 <= b1 + sv) {
            return Err(bad("surface extract: range is not increasing or not inside the domain"));
        }
        let (u0, u1, v0, v1) = (u0.max(a0), u1.min(a1), v0.max(b0), v1.min(b1));
        let (p, q) = (self.degree_u, self.degree_v);
        let pw = self.pw();
        let (nu, nv) = (self.count_u, self.count_v);
        // u direction: one column (fixed j) at a time.
        let mut ku = Vec::new();
        let mut cols: Vec<Vec<V4>> = Vec::with_capacity(nv);
        for j in 0..nv {
            let col: Vec<V4> = (0..nu).map(|i| pw[i * nv + j]).collect();
            let (k, c) = extract_raw(p, &self.knots_u, &col, u0, u1);
            ku = k;
            cols.push(c);
        }
        let nu2 = cols[0].len();
        let mut kv = Vec::new();
        let mut rows: Vec<Vec<V4>> = Vec::with_capacity(nu2);
        for i in 0..nu2 {
            let row: Vec<V4> = cols.iter().map(|c| c[i]).collect();
            let (k, r) = extract_raw(q, &self.knots_v, &row, v0, v1);
            kv = k;
            rows.push(r);
        }
        let nv2 = rows[0].len();
        let flat: Vec<V4> = rows.into_iter().flatten().collect();
        Ok(Self {
            degree_u: p,
            degree_v: q,
            knots_u: ku,
            knots_v: kv,
            control_points: flat.iter().map(dehomog).collect(),
            weights: flat.iter().map(|h| h.w).collect(),
            count_u: nu2,
            count_v: nv2,
            periodic_u: false,
            periodic_v: false,
        })
    }

    /// The surface as clamped Bézier patches (one per non-empty knot rectangle).
    pub fn bezier_patches(&self) -> Vec<Self> {
        let (bu, bv) = self.breakpoints();
        let mut out = Vec::new();
        for wu in bu.windows(2) {
            for wv in bv.windows(2) {
                out.push(self.extract((wu[0], wu[1]), (wv[0], wv[1])).expect("span inside domain"));
            }
        }
        out
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binomials() {
        assert_eq!(binom(4, 2), 6.0);
        assert_eq!(binom(5, 0), 1.0);
        assert_eq!(binom(6, 6), 1.0);
    }

    #[test]
    fn unit_arc_segments_and_weights() {
        let (k, p, w) = unit_arc(0.0, std::f64::consts::PI).unwrap();
        assert_eq!((k.len(), p.len(), w.len()), (8, 5, 5));
        assert!((w[1] - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-15);
        assert!(unit_arc(1.0, 1.0).is_err() && unit_arc(0.0, 7.0).is_err());
    }
}
