//! Numerical geometric constraint solver.
//!
//! Unknowns are point coordinates and circle/arc radii; every constraint contributes one or
//! more residual equations. The system is solved with Gauss–Newton steps using the SVD
//! pseudo-inverse (minimum-norm steps keep under-constrained geometry close to where the user
//! drew it) with a Levenberg–Marquardt fallback when a step does not reduce the residual.
//!
//! Constraint-state analysis uses the Jacobian at the solution, the same approach as FreeCAD's
//! PlaneGCS / SolveSpace:
//! * degrees of freedom = unknowns − rank(J);
//! * an unknown is *determined* when its components in the nullspace of J vanish, which gives
//!   per-entity "fully constrained" state (Fusion draws such curves black, others blue);
//! * a new constraint is *redundant/over-constraining* when it does not increase rank(J).
//!   Observed in Fusion: adding such a dimension fails with "Sketch geometry is over
//!   constrained", while the same dimension is accepted as a driven (reference) dimension.

use crate::model::{ConstraintKind, DistanceOrientation, EntityKind, Sketch, ORIGIN};
use nalgebra::{DMatrix, DVector};
use std::collections::HashMap;

/// Values of dimension constraints, by constraint id (mm or degrees).
pub type DimensionValues = HashMap<String, f64>;

#[derive(Clone, Copy, Debug)]
enum Var {
    X(usize),
    Y(usize),
    R(usize),
}

struct System<'a> {
    sketch: &'a Sketch,
    /// entity index -> first unknown index for its point coords (x, y) or radius
    point_var: HashMap<&'a str, usize>,
    radius_var: HashMap<&'a str, usize>,
    vars: Vec<Var>,
    /// unknowns whose value is held fixed (dragged points)
    held: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SolveStatus {
    Solved,
    /// The system could not be satisfied; the listed constraints have non-zero residuals.
    Failed { unsatisfied: Vec<String> },
}

#[derive(Clone, Debug)]
pub struct SolveReport {
    pub status: SolveStatus,
    pub iterations: usize,
    pub max_residual: f64,
    pub dof: usize,
    /// Entities (points and curves) that are fully determined.
    pub fully_constrained: Vec<String>,
    /// Measured values of driven dimensions, by constraint id.
    pub driven_values: HashMap<String, f64>,
}

impl SolveReport {
    pub fn is_fully_constrained(&self) -> bool {
        self.dof == 0 && self.status == SolveStatus::Solved
    }
}

const TOL: f64 = 1e-9;

impl<'a> System<'a> {
    fn new(sketch: &'a Sketch, hold: &[&str]) -> Self {
        let mut point_var = HashMap::new();
        let mut radius_var = HashMap::new();
        let mut vars = Vec::new();
        let mut held = Vec::new();
        for e in &sketch.entities {
            match &e.kind {
                EntityKind::Point { .. } => {
                    point_var.insert(e.id.as_str(), vars.len());
                    let h = hold.contains(&e.id.as_str());
                    vars.push(Var::X(0));
                    vars.push(Var::Y(0));
                    held.push(h);
                    held.push(h);
                }
                EntityKind::Circle { .. } | EntityKind::Arc { .. } => {
                    radius_var.insert(e.id.as_str(), vars.len());
                    vars.push(Var::R(0));
                    held.push(false);
                }
                EntityKind::Line { .. } => {}
            }
        }
        // store owning entity indices in Var for readback
        for (i, e) in sketch.entities.iter().enumerate() {
            if let Some(&v) = point_var.get(e.id.as_str()) {
                vars[v] = Var::X(i);
                vars[v + 1] = Var::Y(i);
            }
            if let Some(&v) = radius_var.get(e.id.as_str()) {
                vars[v] = Var::R(i);
            }
        }
        System { sketch, point_var, radius_var, vars, held }
    }

    fn initial(&self) -> DVector<f64> {
        DVector::from_iterator(
            self.vars.len(),
            self.vars.iter().map(|v| match *v {
                Var::X(i) => match self.sketch.entities[i].kind {
                    EntityKind::Point { x, .. } => x,
                    _ => 0.0,
                },
                Var::Y(i) => match self.sketch.entities[i].kind {
                    EntityKind::Point { y, .. } => y,
                    _ => 0.0,
                },
                Var::R(i) => match self.sketch.entities[i].kind {
                    EntityKind::Circle { r, .. } | EntityKind::Arc { r, .. } => r,
                    _ => 0.0,
                },
            }),
        )
    }

    fn pt(&self, x: &DVector<f64>, id: &str) -> [f64; 2] {
        if id == ORIGIN {
            return [0.0, 0.0];
        }
        match self.point_var.get(id) {
            Some(&v) => [x[v], x[v + 1]],
            None => [0.0, 0.0],
        }
    }
    fn rad(&self, x: &DVector<f64>, id: &str) -> f64 {
        self.radius_var.get(id).map(|&v| x[v]).unwrap_or(0.0)
    }
    fn line(&self, x: &DVector<f64>, id: &str) -> Option<([f64; 2], [f64; 2])> {
        match &self.sketch.entity(id)?.kind {
            EntityKind::Line { p1, p2 } => Some((self.pt(x, p1), self.pt(x, p2))),
            _ => None,
        }
    }
    /// (centre, radius) of a circle or arc.
    fn circ(&self, x: &DVector<f64>, id: &str) -> Option<([f64; 2], f64)> {
        match &self.sketch.entity(id)?.kind {
            EntityKind::Circle { c, .. } | EntityKind::Arc { c, .. } => Some((self.pt(x, c), self.rad(x, id))),
            _ => None,
        }
    }
    fn is_line(&self, id: &str) -> bool {
        matches!(self.sketch.entity(id).map(|e| &e.kind), Some(EntityKind::Line { .. }))
    }

    /// Residual equations of one constraint. `x0` is the configuration before solving, used to
    /// pick the branch of sign-ambiguous constraints (so solutions keep their orientation).
    fn residuals(&self, kind: &ConstraintKind, value: Option<f64>, x: &DVector<f64>, x0: &DVector<f64>, out: &mut Vec<f64>) {
        use ConstraintKind::*;
        let sub = |a: [f64; 2], b: [f64; 2]| [a[0] - b[0], a[1] - b[1]];
        let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
        let dot = |a: [f64; 2], b: [f64; 2]| a[0] * b[0] + a[1] * b[1];
        let norm = |a: [f64; 2]| (a[0] * a[0] + a[1] * a[1]).sqrt().max(1e-12);
        let sign = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
        match kind {
            Coincident { a, b } => {
                let (p, q) = (self.pt(x, a), self.pt(x, b));
                out.push(p[0] - q[0]);
                out.push(p[1] - q[1]);
            }
            PointOnCurve { p, curve } => {
                let pp = self.pt(x, p);
                if let Some((a, b)) = self.line(x, curve) {
                    let d = sub(b, a);
                    out.push(cross(sub(pp, a), d) / norm(d));
                } else if let Some((c, r)) = self.circ(x, curve) {
                    out.push(norm(sub(pp, c)) - r);
                }
            }
            Horizontal { line } => {
                if let Some((a, b)) = self.line(x, line) {
                    out.push(b[1] - a[1]);
                }
            }
            Vertical { line } => {
                if let Some((a, b)) = self.line(x, line) {
                    out.push(b[0] - a[0]);
                }
            }
            Parallel { a, b } | Perpendicular { a, b } | Collinear { a, b } => {
                if let (Some((a1, a2)), Some((b1, b2))) = (self.line(x, a), self.line(x, b)) {
                    let (da, db) = (sub(a2, a1), sub(b2, b1));
                    match kind {
                        Perpendicular { .. } => out.push(dot(da, db) / (norm(da) * norm(db))),
                        _ => out.push(cross(da, db) / (norm(da) * norm(db))),
                    }
                    if matches!(kind, Collinear { .. }) {
                        out.push(cross(sub(b1, a1), da) / norm(da));
                    }
                }
            }
            Equal { a, b } => {
                if let (Some((a1, a2)), Some((b1, b2))) = (self.line(x, a), self.line(x, b)) {
                    out.push(norm(sub(a2, a1)) - norm(sub(b2, b1)));
                } else if let (Some((_, ra)), Some((_, rb))) = (self.circ(x, a), self.circ(x, b)) {
                    out.push(ra - rb);
                }
            }
            Tangent { a, b } => {
                let (l, c) = if self.is_line(a) { (a, b) } else { (b, a) };
                if self.is_line(l) {
                    if let (Some((p1, p2)), Some((cc, r))) = (self.line(x, l), self.circ(x, c)) {
                        let d = sub(p2, p1);
                        // signed distance keeps the circle on the side it started on
                        let s0 = self.line(x0, l).zip(self.circ(x0, c)).map(|((q1, q2), (c0, _))| sign(cross(sub(c0, q1), sub(q2, q1)))).unwrap_or(1.0);
                        out.push(cross(sub(cc, p1), d) / norm(d) - s0 * r);
                    }
                } else if let (Some((c1, r1)), Some((c2, r2))) = (self.circ(x, a), self.circ(x, b)) {
                    let internal = self.circ(x0, a).zip(self.circ(x0, b)).map(|((c1, r1), (c2, r2))| norm(sub(c1, c2)) < r1.max(r2)).unwrap_or(false);
                    let dc = norm(sub(c1, c2));
                    out.push(if internal { dc - (r1 - r2).abs() } else { dc - (r1 + r2) });
                }
            }
            Concentric { a, b } => {
                if let (Some((c1, _)), Some((c2, _))) = (self.circ(x, a), self.circ(x, b)) {
                    out.push(c1[0] - c2[0]);
                    out.push(c1[1] - c2[1]);
                }
            }
            Midpoint { p, line } => {
                if let Some((a, b)) = self.line(x, line) {
                    let pp = self.pt(x, p);
                    out.push(pp[0] - 0.5 * (a[0] + b[0]));
                    out.push(pp[1] - 0.5 * (a[1] + b[1]));
                }
            }
            Fix { p, x: fx, y: fy } => {
                let pp = self.pt(x, p);
                out.push(pp[0] - fx);
                out.push(pp[1] - fy);
            }
            Symmetric { p1, p2, line } => {
                if let Some((a, b)) = self.line(x, line) {
                    let (q1, q2) = (self.pt(x, p1), self.pt(x, p2));
                    let d = sub(b, a);
                    let m = [0.5 * (q1[0] + q2[0]), 0.5 * (q1[1] + q2[1])];
                    out.push(cross(sub(m, a), d) / norm(d));
                    out.push(dot(sub(q2, q1), d) / norm(d));
                }
            }
            Distance { a, b, orientation } => {
                let v = value.unwrap_or(0.0);
                let (pa, pb) = self.dim_points(x, a, b);
                let (qa, qb) = self.dim_points(x0, a, b);
                match orientation {
                    // the sign of the current offset is preserved, so a negative value flips the
                    // geometry (observed in Fusion: width "-10 mm" produced bbox x in [-10, 0])
                    DistanceOrientation::Horizontal => out.push((pb[0] - pa[0]) - sign(qb[0] - qa[0]) * v),
                    DistanceOrientation::Vertical => out.push((pb[1] - pa[1]) - sign(qb[1] - qa[1]) * v),
                    DistanceOrientation::Aligned => out.push(norm(sub(pb, pa)) - v.abs()),
                }
            }
            PointLineDistance { p, line } => {
                if let (Some((a, b)), Some((a0, b0))) = (self.line(x, line), self.line(x0, line)) {
                    let pp = self.pt(x, p);
                    let s0 = sign(cross(sub(self.pt(x0, p), a0), sub(b0, a0)));
                    let d = sub(b, a);
                    out.push(cross(sub(pp, a), d) / norm(d) - s0 * value.unwrap_or(0.0));
                }
            }
            Radius { curve } => out.push(self.rad(x, curve) - value.unwrap_or(0.0)),
            Diameter { curve } => out.push(2.0 * self.rad(x, curve) - value.unwrap_or(0.0)),
            Angle { a, b } => {
                if let (Some((a1, a2)), Some((b1, b2)), Some((c1, c2)), Some((e1, e2))) = (self.line(x, a), self.line(x, b), self.line(x0, a), self.line(x0, b)) {
                    let (da, db) = (sub(a2, a1), sub(b2, b1));
                    let phi = cross(da, db).atan2(dot(da, db));
                    let phi0 = cross(sub(c2, c1), sub(e2, e1)).atan2(dot(sub(c2, c1), sub(e2, e1)));
                    out.push(phi - sign(phi0) * value.unwrap_or(0.0).to_radians());
                }
            }
        }
    }

    /// Points measured by a distance dimension; a line id stands for its two endpoints.
    fn dim_points(&self, x: &DVector<f64>, a: &str, b: &str) -> ([f64; 2], [f64; 2]) {
        if a == b {
            if let Some((p, q)) = self.line(x, a) {
                return (p, q);
            }
        }
        let pa = self.line(x, a).map(|(p, _)| p).unwrap_or_else(|| self.pt(x, a));
        let pb = self.line(x, b).map(|(_, q)| q).unwrap_or_else(|| self.pt(x, b));
        (pa, pb)
    }

    /// Implicit equations every arc carries: both endpoints lie on its circle.
    fn arc_rules(&self, x: &DVector<f64>, out: &mut Vec<f64>) {
        for e in &self.sketch.entities {
            if let EntityKind::Arc { c, start, end, .. } = &e.kind {
                let (cc, r) = (self.pt(x, c), self.rad(x, &e.id));
                for p in [start, end] {
                    let q = self.pt(x, p);
                    out.push(((q[0] - cc[0]).powi(2) + (q[1] - cc[1]).powi(2)).sqrt() - r);
                }
            }
        }
    }

    /// All residuals; also returns the row range of each active constraint.
    fn eval(&self, x: &DVector<f64>, x0: &DVector<f64>, values: &DimensionValues, extra: Option<&crate::model::Constraint>) -> (Vec<f64>, Vec<(String, std::ops::Range<usize>)>) {
        let mut out = Vec::new();
        let mut rows = Vec::new();
        self.arc_rules(x, &mut out);
        for c in self.sketch.constraints.iter().chain(extra) {
            if c.driven {
                continue;
            }
            let start = out.len();
            let value = if c.kind.is_dimension() { values.get(&c.id).copied().or(Some(0.0)) } else { None };
            self.residuals(&c.kind, value, x, x0, &mut out);
            rows.push((c.id.clone(), start..out.len()));
        }
        (out, rows)
    }

    fn jacobian(&self, x: &DVector<f64>, x0: &DVector<f64>, values: &DimensionValues, extra: Option<&crate::model::Constraint>, m: usize) -> DMatrix<f64> {
        let n = x.len();
        let mut j = DMatrix::zeros(m, n);
        let mut xp = x.clone();
        for k in 0..n {
            if self.held[k] {
                continue;
            }
            let h = 1e-7 * x[k].abs().max(1.0);
            xp[k] = x[k] + h;
            let (rp, _) = self.eval(&xp, x0, values, extra);
            xp[k] = x[k] - h;
            let (rm, _) = self.eval(&xp, x0, values, extra);
            xp[k] = x[k];
            for i in 0..m {
                j[(i, k)] = (rp[i] - rm[i]) / (2.0 * h);
            }
        }
        j
    }
}

fn max_abs(v: &[f64]) -> f64 {
    v.iter().fold(0.0f64, |m, r| m.max(r.abs()))
}

fn rank(j: &DMatrix<f64>) -> usize {
    if j.nrows() == 0 || j.ncols() == 0 {
        return 0;
    }
    let svd = j.clone().svd(false, false);
    let smax = svd.singular_values.iter().cloned().fold(0.0, f64::max);
    svd.singular_values.iter().filter(|&&s| s > 1e-8 * smax.max(1.0)).count()
}

/// Solves the sketch in place. `values` gives dimension values by constraint id.
/// `hold` lists point ids whose positions must not move (interactive dragging).
pub fn solve(sketch: &mut Sketch, values: &DimensionValues, hold: &[&str]) -> SolveReport {
    let snapshot = sketch.clone();
    let sys = System::new(&snapshot, hold);
    let x0 = sys.initial();
    let mut x = x0.clone();
    let (mut r, _) = sys.eval(&x, &x0, values, None);
    let mut iterations = 0;
    let mut lambda: f64 = 1e-3;
    let free: Vec<usize> = (0..x.len()).filter(|&k| !sys.held[k]).collect();

    while iterations < 200 && max_abs(&r) > TOL && !free.is_empty() {
        iterations += 1;
        let m = r.len();
        let jfull = sys.jacobian(&x, &x0, values, None, m);
        let j = jfull.select_columns(&free);
        let rv = DVector::from_vec(r.clone());
        // Gauss-Newton step with minimum norm
        let step = j.clone().svd(true, true).solve(&(-&rv), 1e-12).unwrap_or_else(|_| DVector::zeros(free.len()));
        let try_step = |s: &DVector<f64>, x: &DVector<f64>| {
            let mut xn = x.clone();
            for (i, &k) in free.iter().enumerate() {
                xn[k] += s[i];
            }
            let (rn, _) = sys.eval(&xn, &x0, values, None);
            (xn, rn)
        };
        let (xn, rn) = try_step(&step, &x);
        let norm2 = |v: &[f64]| v.iter().map(|a| a * a).sum::<f64>();
        if norm2(&rn) < norm2(&r) {
            x = xn;
            r = rn;
            lambda = (lambda * 0.3).max(1e-12);
            continue;
        }
        // Levenberg-Marquardt fallback with increasing damping
        let jt = j.transpose();
        let mut improved = false;
        for _ in 0..12 {
            let mut a = &jt * &j;
            for d in 0..a.nrows() {
                a[(d, d)] += lambda * (1.0 + a[(d, d)]);
            }
            if let Some(s) = a.lu().solve(&(-(&jt * &rv))) {
                let (xn, rn) = try_step(&s, &x);
                if norm2(&rn) < norm2(&r) {
                    x = xn;
                    r = rn;
                    improved = true;
                    break;
                }
            }
            lambda *= 10.0;
        }
        if !improved {
            break;
        }
    }

    let (rfinal, rows) = sys.eval(&x, &x0, values, None);
    let max_residual = max_abs(&rfinal);
    let status = if max_residual <= 1e-6 {
        SolveStatus::Solved
    } else {
        let unsatisfied = rows.iter().filter(|(_, rg)| rfinal[rg.clone()].iter().any(|v| v.abs() > 1e-6)).map(|(id, _)| id.clone()).collect();
        SolveStatus::Failed { unsatisfied }
    };

    // write back only a successful solution, so a failing edit leaves the last good geometry
    if status == SolveStatus::Solved {
        apply(sketch, &sys, &x);
    }

    // DOF analysis at the (attempted) solution, over all unknowns (held points are free here)
    let analysis_sys = System::new(&snapshot, &[]);
    let m = rfinal.len();
    let j = analysis_sys.jacobian(&x, &x0, values, None, m);
    let n = x.len();
    let rk = rank(&j);
    let dof = n - rk.min(n);
    let determined = determined_vars(&j, n);
    let fully_constrained = snapshot
        .entities
        .iter()
        .filter(|e| {
            let mut vars = Vec::new();
            for p in e.points() {
                if p == ORIGIN {
                    continue;
                }
                if let Some(&v) = analysis_sys.point_var.get(p) {
                    vars.push(v);
                    vars.push(v + 1);
                }
            }
            if let Some(&v) = analysis_sys.radius_var.get(e.id.as_str()) {
                vars.push(v);
            }
            vars.iter().all(|&v| determined[v])
        })
        .map(|e| e.id.clone())
        .collect();

    let driven_values = sketch
        .constraints
        .iter()
        .filter(|c| c.driven)
        .map(|c| (c.id.clone(), measure(sketch, &c.kind).unwrap_or(0.0)))
        .collect();

    SolveReport { status, iterations, max_residual, dof, fully_constrained, driven_values }
}

/// Unknowns whose components in the nullspace of J are zero.
fn determined_vars(j: &DMatrix<f64>, n: usize) -> Vec<bool> {
    if n == 0 {
        return Vec::new();
    }
    if j.nrows() == 0 {
        return vec![false; n];
    }
    // pad to a square-or-taller matrix so the full V (n x n) is available
    let mut a = DMatrix::zeros(j.nrows().max(n), n);
    a.view_mut((0, 0), (j.nrows(), n)).copy_from(j);
    let svd = a.svd(false, true);
    let vt = svd.v_t.expect("v_t requested");
    let smax = svd.singular_values.iter().cloned().fold(0.0, f64::max).max(1.0);
    let mut det = vec![true; n];
    for (row, &s) in svd.singular_values.iter().enumerate() {
        if s <= 1e-8 * smax {
            for k in 0..n {
                if vt[(row, k)].abs() > 1e-6 {
                    det[k] = false;
                }
            }
        }
    }
    det
}

fn apply(sketch: &mut Sketch, sys: &System, x: &DVector<f64>) {
    for (k, v) in sys.vars.iter().enumerate() {
        match *v {
            Var::X(i) => {
                if let EntityKind::Point { x: px, .. } = &mut sketch.entities[i].kind {
                    *px = x[k];
                }
            }
            Var::Y(i) => {
                if let EntityKind::Point { y: py, .. } = &mut sketch.entities[i].kind {
                    *py = x[k];
                }
            }
            Var::R(i) => match &mut sketch.entities[i].kind {
                EntityKind::Circle { r, .. } | EntityKind::Arc { r, .. } => *r = x[k],
                _ => {}
            },
        }
    }
}

/// Whether adding `kind` would over-constrain the sketch (it adds no independent equation, or
/// cannot be satisfied together with the existing constraints).
pub fn would_overconstrain(sketch: &Sketch, kind: &ConstraintKind, value: Option<f64>, values: &DimensionValues) -> bool {
    let sys = System::new(sketch, &[]);
    let x = sys.initial();
    let probe = crate::model::Constraint { id: "__probe".into(), kind: kind.clone(), param: None, driven: false, label: None };
    let mut vals = values.clone();
    if let Some(v) = value {
        vals.insert("__probe".into(), v);
    }
    let (r0, _) = sys.eval(&x, &x, values, None);
    let (r1, rows) = sys.eval(&x, &x, &vals, Some(&probe));
    let new_rows = rows.last().map(|(_, rg)| rg.len()).unwrap_or(0);
    if new_rows == 0 {
        return false;
    }
    let j0 = sys.jacobian(&x, &x, values, None, r0.len());
    let j1 = sys.jacobian(&x, &x, &vals, Some(&probe), r1.len());
    rank(&j1) < rank(&j0) + new_rows
}

/// Current measured value of a dimension (mm or degrees), used for driven dimensions and to
/// seed the value of a newly placed dimension.
pub fn measure(sketch: &Sketch, kind: &ConstraintKind) -> Option<f64> {
    let sys = System::new(sketch, &[]);
    let x = sys.initial();
    let mut out = Vec::new();
    let zero = match kind {
        ConstraintKind::Distance { .. } | ConstraintKind::PointLineDistance { .. } | ConstraintKind::Radius { .. } | ConstraintKind::Diameter { .. } | ConstraintKind::Angle { .. } => Some(0.0),
        _ => return None,
    };
    sys.residuals(kind, zero, &x, &x, &mut out);
    let v = *out.first()?;
    Some(match kind {
        ConstraintKind::Angle { .. } => v.to_degrees().abs(),
        _ => v.abs(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConstraintKind as K, DistanceOrientation as O};

    /// Builds the Fusion probe sketch: rectangle with H/V constraints, corner at the origin,
    /// width and height dimensions.
    fn rect_sketch(w: f64, h: f64) -> (Sketch, DimensionValues, [String; 4]) {
        let mut s = Sketch::new();
        let lines = s.add_rectangle([1.0, 1.0], [42.0, 23.0]);
        let corner = match &s.entity(&lines[0]).unwrap().kind {
            EntityKind::Line { p1, .. } => p1.clone(),
            _ => unreachable!(),
        };
        s.add_constraint(K::Coincident { a: corner, b: ORIGIN.into() });
        let dw = s.add_dimension(K::Distance { a: lines[0].clone(), b: lines[0].clone(), orientation: O::Horizontal }, "d1", [0.0, -5.0], false);
        let dh = s.add_dimension(K::Distance { a: lines[1].clone(), b: lines[1].clone(), orientation: O::Vertical }, "d2", [50.0, 0.0], false);
        let mut v = DimensionValues::new();
        v.insert(dw, w);
        v.insert(dh, h);
        (s, v, lines)
    }

    fn bbox(s: &Sketch) -> [f64; 4] {
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for e in &s.entities {
            if let EntityKind::Point { x, y } = e.kind {
                b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
            }
        }
        b
    }

    #[test]
    fn dimensioned_rectangle_is_fully_constrained() {
        // Fusion probe: H/V + origin coincident + d1=50, d2=30 -> isFullyConstrained = True
        let (mut s, v, _) = rect_sketch(50.0, 30.0);
        let rep = solve(&mut s, &v, &[]);
        assert_eq!(rep.status, SolveStatus::Solved);
        let b = bbox(&s);
        assert!((b[0]).abs() < 1e-7 && (b[1]).abs() < 1e-7 && (b[2] - 50.0).abs() < 1e-7 && (b[3] - 30.0).abs() < 1e-7, "{b:?}");
        assert_eq!(rep.dof, 0);
        assert!(rep.is_fully_constrained());
        assert!(s.entities.iter().all(|e| rep.fully_constrained.contains(&e.id)));
    }

    #[test]
    fn partially_constrained_reports_dof_and_free_entities() {
        // Fusion probe: with only one dimension the sketch is not fully constrained
        let (mut s, mut v, lines) = rect_sketch(50.0, 30.0);
        let dh = s.constraints.iter().find(|c| c.param.as_deref() == Some("d2")).unwrap().id.clone();
        s.delete_constraint(&dh);
        v.remove(&dh);
        let rep = solve(&mut s, &v, &[]);
        assert_eq!(rep.status, SolveStatus::Solved);
        assert_eq!(rep.dof, 1);
        assert!(!rep.is_fully_constrained());
        assert!(rep.fully_constrained.contains(&lines[0])); // bottom edge is fixed
        assert!(!rep.fully_constrained.contains(&lines[2])); // top edge can still move
    }

    #[test]
    fn third_dimension_is_over_constraining_but_driven_is_allowed() {
        // Fusion probe: third driving dimension on the opposite side ->
        // "VCS_SKETCH_OVER_CONSTRAINTS - Sketch geometry is over constrained"; driven accepted.
        let (mut s, v, lines) = rect_sketch(50.0, 30.0);
        solve(&mut s, &v, &[]);
        let k = K::Distance { a: lines[2].clone(), b: lines[2].clone(), orientation: O::Horizontal };
        assert!(would_overconstrain(&s, &k, Some(50.0), &v));
        assert!(would_overconstrain(&s, &K::Parallel { a: lines[0].clone(), b: lines[2].clone() }, None, &v));
        s.add_dimension(k, "d3", [0.0, 40.0], true);
        let rep = solve(&mut s, &v, &[]);
        assert_eq!(rep.status, SolveStatus::Solved);
        let driven = rep.driven_values.values().next().copied().unwrap();
        assert!((driven - 50.0).abs() < 1e-6);
    }

    #[test]
    fn editing_dimensions_recomputes_geometry_and_negative_flips() {
        let (mut s, mut v, _) = rect_sketch(50.0, 30.0);
        solve(&mut s, &v, &[]);
        let dw = s.constraints.iter().find(|c| c.param.as_deref() == Some("d1")).unwrap().id.clone();
        v.insert(dw.clone(), 60.0);
        assert_eq!(solve(&mut s, &v, &[]).status, SolveStatus::Solved);
        assert!((bbox(&s)[2] - 60.0).abs() < 1e-7);
        // Fusion probe: width "-10 mm" flipped the profile to x in [-10, 0]
        v.insert(dw, -10.0);
        assert_eq!(solve(&mut s, &v, &[]).status, SolveStatus::Solved);
        let b = bbox(&s);
        assert!((b[0] + 10.0).abs() < 1e-7 && b[2].abs() < 1e-7, "{b:?}");
    }

    #[test]
    fn unconstrained_rectangle_stays_put() {
        let mut s = Sketch::new();
        s.add_rectangle([3.0, 4.0], [10.0, 9.0]);
        let before = bbox(&s);
        let rep = solve(&mut s, &DimensionValues::new(), &[]);
        assert_eq!(rep.status, SolveStatus::Solved);
        assert_eq!(bbox(&s), before);
        assert_eq!(rep.dof, 4); // x, y, width, height
    }

    #[test]
    fn circle_tangent_and_radius() {
        let mut s = Sketch::new();
        let l = s.add_line([0.0, 0.0], [100.0, 0.0]);
        s.add_constraint(K::Horizontal { line: l.clone() });
        let c = s.add_circle([50.0, 12.0], 7.0);
        let rid = s.add_dimension(K::Radius { curve: c.clone() }, "d1", [0.0, 0.0], false);
        s.add_constraint(K::Tangent { a: l.clone(), b: c.clone() });
        let mut v = DimensionValues::new();
        v.insert(rid, 10.0);
        let rep = solve(&mut s, &v, &[]);
        assert_eq!(rep.status, SolveStatus::Solved);
        let (cc, r) = match &s.entity(&c).unwrap().kind {
            EntityKind::Circle { c, r } => (s.point(c).unwrap(), *r),
            _ => unreachable!(),
        };
        let (a, _) = s.line_points(&l).unwrap();
        assert!((r - 10.0).abs() < 1e-7);
        assert!((cc[1] - a[1] - 10.0).abs() < 1e-6, "circle stays above the line");
    }

    #[test]
    fn conflicting_values_fail_and_keep_geometry() {
        let mut s = Sketch::new();
        let l = s.add_line([0.0, 0.0], [10.0, 0.0]);
        let (p1, p2) = match &s.entity(&l).unwrap().kind {
            EntityKind::Line { p1, p2 } => (p1.clone(), p2.clone()),
            _ => unreachable!(),
        };
        s.add_constraint(K::Coincident { a: p1.clone(), b: ORIGIN.into() });
        s.add_constraint(K::Fix { p: p2.clone(), x: 10.0, y: 0.0 });
        let d = s.add_dimension(K::Distance { a: p1, b: p2, orientation: O::Aligned }, "d1", [0.0, 0.0], false);
        let mut v = DimensionValues::new();
        v.insert(d.clone(), 25.0);
        let before = s.clone();
        let rep = solve(&mut s, &v, &[]);
        assert!(matches!(rep.status, SolveStatus::Failed { .. }));
        assert_eq!(s, before, "a failed solve must not move geometry");
    }

    #[test]
    fn perpendicular_parallel_equal_midpoint_symmetric() {
        let mut s = Sketch::new();
        let a = s.add_line([0.0, 0.0], [10.0, 1.0]);
        let b = s.add_line([0.0, 5.0], [1.0, 15.0]);
        let c = s.add_line([20.0, 0.0], [31.0, 3.0]);
        s.add_constraint(K::Perpendicular { a: a.clone(), b: b.clone() });
        s.add_constraint(K::Parallel { a: a.clone(), b: c.clone() });
        s.add_constraint(K::Equal { a: a.clone(), b: c.clone() });
        let rep = solve(&mut s, &DimensionValues::new(), &[]);
        assert_eq!(rep.status, SolveStatus::Solved);
        let d = |id: &str| {
            let (p, q) = s.line_points(id).unwrap();
            [q[0] - p[0], q[1] - p[1]]
        };
        let (da, db, dc) = (d(&a), d(&b), d(&c));
        assert!((da[0] * db[0] + da[1] * db[1]).abs() < 1e-6);
        assert!((da[0] * dc[1] - da[1] * dc[0]).abs() < 1e-6);
        assert!(((da[0].hypot(da[1])) - dc[0].hypot(dc[1])).abs() < 1e-6);
    }

    #[test]
    fn arc_endpoints_stay_on_circle_when_radius_changes() {
        let mut s = Sketch::new();
        let arc = s.add_arc_3pt([10.0, 0.0], [0.0, 10.0], [-10.0, 0.0]).unwrap();
        let rid = s.add_dimension(K::Radius { curve: arc.clone() }, "d1", [0.0, 0.0], false);
        let mut v = DimensionValues::new();
        v.insert(rid, 15.0);
        assert_eq!(solve(&mut s, &v, &[]).status, SolveStatus::Solved);
        if let EntityKind::Arc { c, start, end, r } = &s.entity(&arc).unwrap().kind {
            let (cc, ps, pe) = (s.point(c).unwrap(), s.point(start).unwrap(), s.point(end).unwrap());
            assert!((r - 15.0).abs() < 1e-7);
            assert!(((ps[0] - cc[0]).hypot(ps[1] - cc[1]) - 15.0).abs() < 1e-6);
            assert!(((pe[0] - cc[0]).hypot(pe[1] - cc[1]) - 15.0).abs() < 1e-6);
        }
    }

    #[test]
    fn dragging_a_held_point() {
        let mut s = Sketch::new();
        let lines = s.add_rectangle([0.0, 0.0], [10.0, 10.0]);
        let corner = match &s.entity(&lines[1]).unwrap().kind {
            EntityKind::Line { p2, .. } => p2.clone(),
            _ => unreachable!(),
        };
        s.set_point(&corner, [20.0, 15.0]);
        let rep = solve(&mut s, &DimensionValues::new(), &[&corner]);
        assert_eq!(rep.status, SolveStatus::Solved);
        assert_eq!(s.point(&corner).unwrap(), [20.0, 15.0]);
        let b = bbox(&s);
        assert!((b[2] - 20.0).abs() < 1e-7 && (b[3] - 15.0).abs() < 1e-7);
    }

    #[test]
    fn measure_values() {
        let mut s = Sketch::new();
        let l = s.add_line([0.0, 0.0], [30.0, 40.0]);
        assert!((measure(&s, &K::Distance { a: l.clone(), b: l.clone(), orientation: O::Aligned }).unwrap() - 50.0).abs() < 1e-9);
        assert!((measure(&s, &K::Distance { a: l.clone(), b: l.clone(), orientation: O::Horizontal }).unwrap() - 30.0).abs() < 1e-9);
        let c = s.add_circle([0.0, 0.0], 6.0);
        assert!((measure(&s, &K::Diameter { curve: c }).unwrap() - 12.0).abs() < 1e-9);
    }
}
