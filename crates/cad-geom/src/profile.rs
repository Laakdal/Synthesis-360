//! Planar profiles: the closed 2D regions that extrude and revolve sweep.
//!
//! A [`Profile`] is a plane (a [`Frame`]; normal = `frame.z`) and closed loops of [`Seg`]ments in
//! plane coordinates (x along `frame.x`, y along `frame.y`). The input format matches
//! `research/feature-cases.json` (which uses degrees; [`Seg`] uses radians). Loop orientation in
//! the input is arbitrary. [`Profile::region`] checks the loops and returns a [`Region`]: one
//! outer loop, counter-clockwise about the normal, and its holes, clockwise, so the material is
//! always to the left of a segment's direction.
//!
//! Checks: segments finite with positive length / radius; each loop closed (consecutive
//! endpoints within the linear tolerance; they are then snapped together); a full circle forms a
//! loop on its own; no two segments cross or touch except consecutive ones at their shared
//! endpoint; exactly one loop contains every other loop, and holes are not nested.
//!
//! Normalisation (a design choice, not ASM evidence; see `docs/FEATURE_MATRIX.md`): consecutive
//! collinear lines and consecutive arcs of the same circle are merged, so a straight side or a
//! circular side always becomes one face.

use crate::{Frame, GResult, GeomError, Tolerance, P3};
use std::f64::consts::{PI, TAU};

pub type P2 = nalgebra::Point2<f64>;
pub type V2 = nalgebra::Vector2<f64>;

/// One profile segment in plane coordinates (mm, radians).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    /// Straight segment from `a` to `b`.
    Line { a: P2, b: P2 },
    /// Circular arc about `c` of radius `r`, from angle `a0` (measured from the plane x axis
    /// towards y) through the signed `sweep` (+ = counter-clockwise about the normal). A sweep
    /// of ±2π is a full circle and must be its loop's only segment; its vertex is at angle `a0`.
    Arc { c: P2, r: f64, a0: f64, sweep: f64 },
}

impl Seg {
    /// Full circle starting (and ending) at angle 0.
    pub fn circle(c: P2, r: f64) -> Self {
        Seg::Arc { c, r, a0: 0.0, sweep: TAU }
    }

    pub fn is_full_circle(&self) -> bool {
        matches!(self, Seg::Arc { sweep, .. } if (sweep.abs() - TAU).abs() <= 1e-12)
    }

    pub fn start(&self) -> P2 {
        self.point(0.0)
    }

    pub fn end(&self) -> P2 {
        self.point(1.0)
    }

    /// Point at fraction `s` ∈ [0, 1] of the segment.
    pub fn point(&self, s: f64) -> P2 {
        match *self {
            Seg::Line { a, b } => a + (b - a) * s,
            Seg::Arc { c, r, a0, sweep } => {
                let t = a0 + sweep * s;
                c + V2::new(t.cos(), t.sin()) * r
            }
        }
    }

    /// Unit tangent at fraction `s`, in the direction of travel.
    pub fn tangent(&self, s: f64) -> V2 {
        match *self {
            Seg::Line { a, b } => (b - a).normalize(),
            Seg::Arc { a0, sweep, .. } => {
                let t = a0 + sweep * s;
                V2::new(-t.sin(), t.cos()) * sweep.signum()
            }
        }
    }

    pub fn reversed(&self) -> Self {
        match *self {
            Seg::Line { a, b } => Seg::Line { a: b, b: a },
            Seg::Arc { c, r, a0, sweep } => Seg::Arc { c, r, a0: a0 + sweep, sweep: -sweep },
        }
    }

    pub fn length(&self) -> f64 {
        match *self {
            Seg::Line { a, b } => (b - a).norm(),
            Seg::Arc { r, sweep, .. } => r * sweep.abs(),
        }
    }

    /// ½∮(x dy − y dx) along the segment: summed over a closed loop this is its signed area.
    pub fn area_term(&self) -> f64 {
        match *self {
            Seg::Line { a, b } => 0.5 * (a.x * b.y - b.x * a.y),
            Seg::Arc { c, r, sweep, .. } => {
                let (p0, p1) = (self.start(), self.end());
                0.5 * (c.x * (p1.y - p0.y) - c.y * (p1.x - p0.x) + r * r * sweep)
            }
        }
    }

    /// Axis-aligned bounds (exact for arcs: includes the quadrant extremes they pass).
    fn bounds(&self) -> (P2, P2) {
        let (p, q) = (self.start(), self.end());
        let mut lo = P2::new(p.x.min(q.x), p.y.min(q.y));
        let mut hi = P2::new(p.x.max(q.x), p.y.max(q.y));
        if let Seg::Arc { c, r, .. } = *self {
            for k in 0..4 {
                let ang = k as f64 * PI / 2.0;
                if self.arc_contains_angle(ang) {
                    let e = c + V2::new(ang.cos(), ang.sin()) * r;
                    lo = P2::new(lo.x.min(e.x), lo.y.min(e.y));
                    hi = P2::new(hi.x.max(e.x), hi.y.max(e.y));
                }
            }
        }
        (lo, hi)
    }

    /// Fraction s ∈ [0, 1] at which an arc passes angle `ang`, if it does.
    fn arc_fraction(&self, ang: f64) -> Option<f64> {
        let Seg::Arc { a0, sweep, .. } = *self else { return None };
        let d = if sweep >= 0.0 { (ang - a0).rem_euclid(TAU) } else { (a0 - ang).rem_euclid(TAU) };
        let s = d / sweep.abs();
        if s <= 1.0 + 1e-15 { Some(s.min(1.0)) } else if (TAU - d) <= 1e-15 { Some(0.0) } else { None }
    }

    fn arc_contains_angle(&self, ang: f64) -> bool {
        self.arc_fraction(ang).is_some()
    }

    /// Fraction of the point of the segment nearest to `p` (approximate for points off it).
    fn fraction_of(&self, p: P2) -> f64 {
        match *self {
            Seg::Line { a, b } => ((p - a).dot(&(b - a)) / (b - a).norm_squared()).clamp(0.0, 1.0),
            Seg::Arc { c, .. } => {
                let ang = (p.y - c.y).atan2(p.x - c.x);
                self.arc_fraction(ang).unwrap_or_else(|| {
                    // outside the arc: the nearer end
                    if (p - self.start()).norm() <= (p - self.end()).norm() { 0.0 } else { 1.0 }
                })
            }
        }
    }
}

/// A planar profile as supplied (loops in any orientation).
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// The profile plane; segments are in its (x, y) coordinates and its z is the normal.
    pub plane: Frame,
    pub loops: Vec<Vec<Seg>>,
}

/// A checked, oriented profile region: `outer` counter-clockwise about `plane.z`, each hole
/// clockwise (material to the left of every segment). Consecutive segments share endpoints
/// exactly; loops of more than one segment contain no full circle.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub plane: Frame,
    pub outer: Vec<Seg>,
    pub holes: Vec<Vec<Seg>>,
}

impl Region {
    /// The outer loop followed by the holes.
    pub fn loops(&self) -> impl Iterator<Item = &Vec<Seg>> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }

    /// Net area (outer minus holes), > 0.
    pub fn area(&self) -> f64 {
        self.loops().map(|l| loop_area(l)).sum()
    }

    /// World point of plane coordinates (x, y) at offset `z` along the normal.
    pub fn world(&self, p: P2, z: f64) -> P3 {
        self.plane.to_world(P3::new(p.x, p.y, z))
    }
}

/// Signed area of a closed loop (+ = counter-clockwise).
pub fn loop_area(l: &[Seg]) -> f64 {
    l.iter().map(Seg::area_term).sum()
}

fn invalid<T>(m: String) -> GResult<T> {
    Err(GeomError::InvalidInput(m))
}

impl Profile {
    /// Checks the loops and orients them (see the module docs). `tol.linear` is the gap allowed
    /// between consecutive endpoints and the closest approach allowed between segments.
    pub fn region(&self, tol: &Tolerance) -> GResult<Region> {
        let pl = &self.plane;
        let ortho = (pl.x.norm() - 1.0).abs() < 1e-9 && (pl.y.norm() - 1.0).abs() < 1e-9 && pl.x.dot(&pl.y).abs() < 1e-9;
        if !ortho || (pl.z - pl.x.cross(&pl.y)).norm() > 1e-9 {
            return invalid("profile plane must be a right-handed orthonormal frame".into());
        }
        if self.loops.is_empty() {
            return invalid("profile has no loops".into());
        }
        let mut loops = Vec::with_capacity(self.loops.len());
        for (li, l) in self.loops.iter().enumerate() {
            loops.push(close_loop(li, l, tol)?);
        }
        // non-crossing, then containment
        let segs: Vec<(usize, usize, Seg)> = loops.iter().enumerate().flat_map(|(li, l)| l.iter().enumerate().map(move |(si, s)| (li, si, *s))).collect();
        for i in 0..segs.len() {
            for j in i + 1..segs.len() {
                let (li, si, a) = segs[i];
                let (lj, sj, b) = segs[j];
                let n = loops[li].len();
                let adjacent = li == lj && (sj == si + 1 || (si == 0 && sj == n - 1));
                if let Some(p) = touch(&a, &b, tol.linear, adjacent) {
                    return invalid(format!("profile segments {li}.{si} and {lj}.{sj} intersect near ({:.6}, {:.6})", p.x, p.y));
                }
            }
        }
        let areas: Vec<f64> = loops.iter().map(|l| loop_area(l)).collect();
        let outer = (0..loops.len()).max_by(|&i, &j| areas[i].abs().total_cmp(&areas[j].abs())).unwrap();
        for (i, l) in loops.iter().enumerate() {
            if i == outer {
                continue;
            }
            let p = l[0].point(0.5);
            if winding(&loops[outer], p) == 0 {
                return invalid(format!("profile loop {i} lies outside the outer loop {outer} (several regions are not one profile)"));
            }
            for (k, h) in loops.iter().enumerate() {
                if k != outer && k != i && winding(h, p) != 0 {
                    return invalid(format!("profile loop {i} lies inside hole {k} (nested islands are separate profiles)"));
                }
            }
        }
        let orient = |l: Vec<Seg>, ccw: bool, area: f64| if (area > 0.0) == ccw { l } else { l.iter().rev().map(Seg::reversed).collect() };
        let mut holes = Vec::new();
        let mut out = Vec::new();
        for (i, l) in loops.into_iter().enumerate() {
            if i == outer {
                out = orient(l, true, areas[i]);
            } else {
                holes.push(orient(l, false, areas[i]));
            }
        }
        Ok(Region { plane: *pl, outer: out, holes })
    }
}

/// Checks one loop's segments and closure, snaps shared endpoints and merges collinear lines and
/// co-circular arcs.
fn close_loop(li: usize, l: &[Seg], tol: &Tolerance) -> GResult<Vec<Seg>> {
    if l.is_empty() {
        return invalid(format!("profile loop {li} is empty"));
    }
    for (si, s) in l.iter().enumerate() {
        let ok = match *s {
            Seg::Line { a, b } => a.iter().chain(b.iter()).all(|x| x.is_finite()) && (b - a).norm() > tol.linear,
            Seg::Arc { c, r, a0, sweep } => {
                c.iter().all(|x| x.is_finite()) && r.is_finite() && r > tol.linear && a0.is_finite() && sweep.is_finite() && sweep != 0.0 && sweep.abs() <= TAU + 1e-12 && r * sweep.abs() > tol.linear
            }
        };
        if !ok {
            return invalid(format!("profile segment {li}.{si} is degenerate or not finite: {s:?}"));
        }
        if s.is_full_circle() && l.len() > 1 {
            return invalid(format!("profile loop {li}: a full circle must be a loop on its own"));
        }
    }
    if l.len() == 1 && l[0].is_full_circle() {
        let Seg::Arc { c, r, a0, sweep } = l[0] else { unreachable!() };
        return Ok(vec![Seg::Arc { c, r, a0, sweep: TAU * sweep.signum() }]);
    }
    let n = l.len();
    for i in 0..n {
        let gap = (l[i].end() - l[(i + 1) % n].start()).norm();
        if gap > tol.linear {
            return invalid(format!("profile loop {li} is open: segment {i} ends {gap:.3e} mm from the start of segment {}", (i + 1) % n));
        }
    }
    let mut segs: Vec<Seg> = l.to_vec();
    // merge collinear lines / co-circular arcs (cyclically)
    let same_line = |p: &Seg, q: &Seg| match (*p, *q) {
        (Seg::Line { a, b }, Seg::Line { b: d, .. }) => {
            let u = (b - a).normalize();
            let w = d - b;
            // the next line continues forwards and its end stays on this line
            u.dot(&w) > 0.0 && (u.x * w.y - u.y * w.x).abs() <= tol.linear
        }
        (Seg::Arc { c, r, sweep: s1, .. }, Seg::Arc { c: c2, r: r2, sweep: s2, .. }) => {
            (c - c2).norm() <= tol.linear && (r - r2).abs() <= tol.linear && s1.signum() == s2.signum() && (s1 + s2).abs() <= TAU + 1e-9
        }
        _ => false,
    };
    let merge = |p: &Seg, q: &Seg| match (*p, *q) {
        (Seg::Line { a, .. }, Seg::Line { b, .. }) => Seg::Line { a, b },
        (Seg::Arc { c, r, a0, sweep }, Seg::Arc { sweep: s2, .. }) => Seg::Arc { c, r, a0, sweep: sweep + s2 },
        _ => unreachable!(),
    };
    let mut changed = true;
    while changed && segs.len() > 1 {
        changed = false;
        for i in 0..segs.len() {
            let j = (i + 1) % segs.len();
            if i != j && same_line(&segs[i], &segs[j]) {
                let m = merge(&segs[i], &segs[j]);
                segs[i] = m;
                segs.remove(j);
                changed = true;
                break;
            }
        }
    }
    if segs.len() == 1 {
        // two co-circular arcs closing a circle merge to a full circle
        if let Seg::Arc { c, r, a0, sweep } = segs[0]
            && (sweep.abs() - TAU).abs() < 1e-9
        {
            return Ok(vec![Seg::Arc { c, r, a0, sweep: TAU * sweep.signum() }]);
        }
        return invalid(format!("profile loop {li} does not enclose an area"));
    }
    if segs.len() == 2 && segs.iter().all(|s| matches!(s, Seg::Line { .. })) {
        return invalid(format!("profile loop {li} does not enclose an area"));
    }
    // snap: each segment starts exactly where the previous one ends
    let n = segs.len();
    for i in 0..n {
        let p = segs[(i + n - 1) % n].end();
        segs[i] = with_start(segs[i], p);
    }
    Ok(segs)
}

/// The segment with its start moved to `p`: a line takes `p` as its start exactly; an arc is left
/// as it is (its circle defines it; its start is within the linear tolerance of `p`).
fn with_start(s: Seg, p: P2) -> Seg {
    match s {
        Seg::Line { b, .. } => Seg::Line { a: p, b },
        arc => arc,
    }
}

/// Winding number of a closed loop around `p` (crossings of the +x ray from `p`).
pub fn winding(l: &[Seg], p: P2) -> i32 {
    let mut w = 0;
    for s in l {
        // flatten finely; exactness is not needed: callers test points well inside/outside
        let n = match s {
            Seg::Line { .. } => 1,
            Seg::Arc { sweep, .. } => (sweep.abs() / (PI / 64.0)).ceil() as usize,
        };
        for k in 0..n {
            let (a, b) = (s.point(k as f64 / n as f64), s.point((k + 1) as f64 / n as f64));
            if (a.y <= p.y) != (b.y <= p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if x > p.x {
                    w += if b.y > a.y { 1 } else { -1 };
                }
            }
        }
    }
    w
}

/// A point where two segments come within `tol` of each other, other than the shared endpoint
/// of `adjacent` segments (end of the first = start of the second, or the reverse).
fn touch(a: &Seg, b: &Seg, tol: f64, adjacent: bool) -> Option<P2> {
    let (alo, ahi) = a.bounds();
    let (blo, bhi) = b.bounds();
    if alo.x > bhi.x + tol || blo.x > ahi.x + tol || alo.y > bhi.y + tol || blo.y > ahi.y + tol {
        return None;
    }
    let shared: Vec<P2> = if adjacent { [a.start(), a.end()].into_iter().filter(|p| (b.start() - p).norm() <= tol || (b.end() - p).norm() <= tol).collect() } else { Vec::new() };
    let near_shared = |p: P2| shared.iter().any(|q| (p - q).norm() <= 10.0 * tol);
    // candidate points: exact intersections plus each endpoint's closest point on the other
    let mut cands = intersections(a, b, tol);
    for (s, o) in [(a, b), (b, a)] {
        for p in [s.start(), s.end()] {
            let q = o.point(o.fraction_of(p));
            if (p - q).norm() <= tol {
                cands.push(p);
            }
        }
    }
    cands.into_iter().find(|p| !near_shared(*p))
}

/// Intersection points of the underlying lines/circles that lie on both segments.
fn intersections(a: &Seg, b: &Seg, tol: f64) -> Vec<P2> {
    let on = |s: &Seg, p: P2| (s.point(s.fraction_of(p)) - p).norm() <= tol;
    let mut pts = Vec::new();
    match (*a, *b) {
        (Seg::Line { a: p, b: q }, Seg::Line { a: r, b: s }) => {
            let (d, e) = (q - p, s - r);
            let den = d.x * e.y - d.y * e.x;
            if den.abs() > 1e-14 * d.norm() * e.norm() {
                let t = ((r.x - p.x) * e.y - (r.y - p.y) * e.x) / den;
                pts.push(p + d * t);
            } else {
                // parallel: overlapping collinear lines are caught by the endpoint test
            }
        }
        (Seg::Line { a: p, b: q }, Seg::Arc { c, r, .. }) | (Seg::Arc { c, r, .. }, Seg::Line { a: p, b: q }) => {
            let d = (q - p).normalize();
            let f = p - c;
            let bq = f.dot(&d);
            let disc = bq * bq - (f.norm_squared() - r * r);
            if disc >= -2.0 * r * tol {
                let s = disc.max(0.0).sqrt();
                pts.push(p + d * (-bq - s));
                pts.push(p + d * (-bq + s));
            }
        }
        (Seg::Arc { c: c1, r: r1, .. }, Seg::Arc { c: c2, r: r2, .. }) => {
            let dv = c2 - c1;
            let dist = dv.norm();
            if dist <= tol && (r1 - r2).abs() <= tol {
                // same circle: overlapping arcs are caught by the endpoint test, except two arcs
                // sharing no endpoint yet overlapping in their interiors: test their midpoints
                pts.push(a.point(0.5));
                pts.push(b.point(0.5));
            } else if dist > 0.0 && dist <= r1 + r2 + tol && dist >= (r1 - r2).abs() - tol {
                let x = (dist * dist + r1 * r1 - r2 * r2) / (2.0 * dist);
                let h = (r1 * r1 - x * x).max(0.0).sqrt();
                let u = dv / dist;
                let m = c1 + u * x;
                let nrm = V2::new(-u.y, u.x);
                pts.push(m + nrm * h);
                pts.push(m - nrm * h);
            }
        }
    }
    pts.retain(|p| on(a, *p) && on(b, *p));
    pts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Seg> {
        let p = [P2::new(x0, y0), P2::new(x1, y0), P2::new(x1, y1), P2::new(x0, y1)];
        (0..4).map(|i| Seg::Line { a: p[i], b: p[(i + 1) % 4] }).collect()
    }

    fn region(loops: Vec<Vec<Seg>>) -> GResult<Region> {
        Profile { plane: Frame::world(), loops }.region(&Tolerance::asm())
    }

    #[test]
    fn orients_outer_ccw_and_holes_cw() {
        let cw_outer: Vec<Seg> = rect(0.0, 0.0, 50.0, 30.0).iter().rev().map(Seg::reversed).collect();
        let r = region(vec![vec![Seg::circle(P2::new(25.0, 15.0), 6.0)], cw_outer]).unwrap();
        assert!(loop_area(&r.outer) > 0.0 && loop_area(&r.holes[0]) < 0.0);
        assert!((r.area() - (1500.0 - PI * 36.0)).abs() < 1e-9);
    }

    #[test]
    fn merges_collinear_and_cocircular_segments() {
        let mut l = rect(0.0, 0.0, 10.0, 10.0);
        let Seg::Line { a, b } = l[0] else { unreachable!() };
        let m = P2::new(4.0, 0.0);
        l.splice(0..1, [Seg::Line { a, b: m }, Seg::Line { a: m, b }]);
        assert_eq!(region(vec![l]).unwrap().outer.len(), 4);
        let c = P2::new(0.0, 0.0);
        let halves = vec![Seg::Arc { c, r: 3.0, a0: 0.0, sweep: PI }, Seg::Arc { c, r: 3.0, a0: PI, sweep: PI }];
        let r = region(vec![halves]).unwrap();
        assert!(r.outer.len() == 1 && r.outer[0].is_full_circle());
    }

    #[test]
    fn rejects_bad_loops() {
        let mut open = rect(0.0, 0.0, 10.0, 10.0);
        open.pop();
        assert!(region(vec![open]).is_err());
        // bow tie
        let p = [P2::new(0.0, 0.0), P2::new(10.0, 10.0), P2::new(10.0, 0.0), P2::new(0.0, 10.0)];
        let bow: Vec<Seg> = (0..4).map(|i| Seg::Line { a: p[i], b: p[(i + 1) % 4] }).collect();
        assert!(region(vec![bow]).is_err());
        // hole crossing the boundary, hole outside, disjoint regions, nested islands
        assert!(region(vec![rect(0.0, 0.0, 10.0, 10.0), vec![Seg::circle(P2::new(10.0, 5.0), 2.0)]]).is_err());
        assert!(region(vec![rect(0.0, 0.0, 10.0, 10.0), rect(20.0, 0.0, 30.0, 10.0)]).is_err());
        assert!(region(vec![rect(0.0, 0.0, 10.0, 10.0), rect(2.0, 2.0, 8.0, 8.0), rect(4.0, 4.0, 6.0, 6.0)]).is_err());
        // a hole touching the outer loop at one point
        assert!(region(vec![rect(0.0, 0.0, 10.0, 10.0), vec![Seg::circle(P2::new(5.0, 2.0), 2.0)]]).is_err());
        // full circle sharing a loop
        let mut l = rect(0.0, 0.0, 10.0, 10.0);
        l.push(Seg::circle(P2::new(5.0, 5.0), 1.0));
        assert!(region(vec![l]).is_err());
    }

    #[test]
    fn arc_area_terms() {
        // slot: 30×10 with semicircular ends, area 300 + 25π
        let l = vec![
            Seg::Line { a: P2::new(0.0, 0.0), b: P2::new(30.0, 0.0) },
            Seg::Arc { c: P2::new(30.0, 5.0), r: 5.0, a0: -PI / 2.0, sweep: PI },
            Seg::Line { a: P2::new(30.0, 10.0), b: P2::new(0.0, 10.0) },
            Seg::Arc { c: P2::new(0.0, 5.0), r: 5.0, a0: PI / 2.0, sweep: PI },
        ];
        assert!((loop_area(&l) - (300.0 + 25.0 * PI)).abs() < 1e-9);
        // notch: concave arc
        let n = vec![
            Seg::Line { a: P2::new(0.0, 0.0), b: P2::new(40.0, 0.0) },
            Seg::Line { a: P2::new(40.0, 0.0), b: P2::new(40.0, 20.0) },
            Seg::Line { a: P2::new(40.0, 20.0), b: P2::new(30.0, 20.0) },
            Seg::Arc { c: P2::new(20.0, 20.0), r: 10.0, a0: 0.0, sweep: -PI },
            Seg::Line { a: P2::new(10.0, 20.0), b: P2::new(0.0, 20.0) },
            Seg::Line { a: P2::new(0.0, 20.0), b: P2::new(0.0, 0.0) },
        ];
        let r = region(vec![n]).unwrap();
        assert!((r.area() - (800.0 - 50.0 * PI)).abs() < 1e-9);
    }
}
