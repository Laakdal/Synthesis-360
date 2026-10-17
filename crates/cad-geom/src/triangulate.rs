//! Triangulation of a polygon with holes, for the tessellator.
//!
//! 1. **Hole bridging** (D. Eberly, "Triangulation by Ear Clipping", Geometric Tools, 2002–2015):
//!    holes are processed by decreasing maximum x; from a hole's rightmost vertex M a ray in +x
//!    finds the nearest boundary edge, the edge's right endpoint P (or, when reflex vertices lie
//!    in the triangle M–I–P, the one seen at the smallest angle from the ray) is visible from M,
//!    and the hole is spliced in along the doubled bridge P → M … M → P. A hole that shares a
//!    vertex with the boundary (a hole touching the outer loop) is spliced at that vertex instead,
//!    with no bridge.
//! 2. **Ear clipping** of the resulting weakly simple polygon. An ear must be strictly convex and
//!    its closed triangle must contain no other polygon vertex; at vertices that occur more than
//!    once (bridge ends, pinch points) the edges leaving the other occurrence must not enter the
//!    triangle. Collinear runs therefore never produce zero-area triangles.
//! 3. **Lawson flips** (C. L. Lawson, 1977) of every unconstrained edge that fails the in-circle
//!    test give the constrained Delaunay triangulation (L. P. Chew, "Constrained Delaunay
//!    triangulations", Algorithmica 4, 1989) of the polygon's edges.
//! 4. **Steiner points** are inserted incrementally (walking point location, split of the
//!    containing triangle or edge, flip legalisation as in Guibas & Stolfi 1985); points on or
//!    next to a constrained edge are refused, so the boundary stays exactly as given.
//!
//! Orientation signs are exact: Shewchuk's floating-point filter ("Adaptive precision
//! floating-point arithmetic and fast robust geometric predicates", 1997) with an exact
//! expansion (TwoSum / FMA TwoProduct) fallback.

use crate::{GResult, GeomError};
use std::collections::{HashMap, HashSet};

pub(crate) type P2 = [f64; 2];

fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    (p, a.mul_add(b, -p))
}

/// Adds `b` to the non-overlapping expansion `e` (Shewchuk's GROW-EXPANSION, zeros eliminated).
fn grow(e: &mut Vec<f64>, b: f64) {
    let mut q = b;
    let mut k = 0;
    for i in 0..e.len() {
        let (s, h) = two_sum(q, e[i]);
        if h != 0.0 {
            e[k] = h;
            k += 1;
        }
        q = s;
    }
    e.truncate(k);
    if q != 0.0 {
        e.push(q);
    }
}

/// Twice the signed area of (a, b, c): > 0 counter-clockwise. The sign is exact.
pub(crate) fn orient(a: P2, b: P2, c: P2) -> f64 {
    let l = (a[0] - c[0]) * (b[1] - c[1]);
    let r = (a[1] - c[1]) * (b[0] - c[0]);
    let det = l - r;
    if det.abs() >= 3.330_669_073_875_472e-16 * (l.abs() + r.abs()) {
        return det;
    }
    // exact: every difference is split into hi + lo by TwoSum, every product by TwoProduct
    let d = |x: f64, y: f64| two_sum(x, -y);
    let (acx, acy, bcx, bcy) = (d(a[0], c[0]), d(a[1], c[1]), d(b[0], c[0]), d(b[1], c[1]));
    let mut e = Vec::with_capacity(32);
    for (p, q, s) in [(acx, bcy, 1.0), (acy, bcx, -1.0)] {
        for x in [p.0, p.1] {
            for y in [q.0, q.1] {
                let (h, l) = two_prod(x, y);
                grow(&mut e, s * h);
                grow(&mut e, s * l);
            }
        }
    }
    e.last().copied().unwrap_or(0.0)
}

/// True when `d` lies inside the circumcircle of the counter-clockwise triangle (a, b, c), with
/// a relative dead band so that near-cocircular quadruples never flip back and forth.
fn in_circle(a: P2, b: P2, c: P2, d: P2) -> bool {
    let (adx, ady, bdx, bdy, cdx, cdy) = (a[0] - d[0], a[1] - d[1], b[0] - d[0], b[1] - d[1], c[0] - d[0], c[1] - d[1]);
    let (al, bl, cl) = (adx * adx + ady * ady, bdx * bdx + bdy * bdy, cdx * cdx + cdy * cdy);
    let (x, y, z) = (bdx * cdy - bdy * cdx, cdx * ady - cdy * adx, adx * bdy - ady * bdx);
    let det = al * x + bl * y + cl * z;
    let perm = al * ((bdx * cdy).abs() + (bdy * cdx).abs())
        + bl * ((cdx * ady).abs() + (cdy * adx).abs())
        + cl * ((adx * bdy).abs() + (ady * bdx).abs());
    det > 1e-10 * perm
}

/// Is `q` strictly inside the interior angle of a counter-clockwise boundary at `cur`?
fn in_wedge(prev: P2, cur: P2, next: P2, q: P2) -> bool {
    let (l, r) = (orient(prev, cur, q) > 0.0, orient(cur, next, q) > 0.0);
    if orient(prev, cur, next) > 0.0 { l && r } else { l || r }
}

fn ukey(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

/// A triangulation of a polygonal domain whose boundary edges are constrained.
pub(crate) struct Cdt {
    pub pts: Vec<P2>,
    pub tris: Vec<[usize; 3]>,
    /// directed edge (a → b) → triangle that has it (counter-clockwise)
    edge: HashMap<(usize, usize), usize>,
    fixed: HashSet<(usize, usize)>,
    hint: usize,
}

impl Cdt {
    /// Triangulates the domain inside `outer` (counter-clockwise) and outside every hole
    /// (clockwise); indices refer to `pts`. Rings may visit a point twice (pinch) and may share
    /// points with each other (touching), but must not cross. Ring edges are constrained.
    pub fn new(pts: Vec<P2>, outer: &[usize], holes: &[Vec<usize>]) -> GResult<Cdt> {
        let mut fixed = HashSet::new();
        for r in std::iter::once(outer).chain(holes.iter().map(|h| h.as_slice())) {
            for i in 0..r.len() {
                let (a, b) = (r[i], r[(i + 1) % r.len()]);
                if a != b {
                    fixed.insert(ukey(a, b));
                }
            }
        }
        let poly = bridge(&pts, outer, holes);
        let tris = ear_clip(&pts, &poly);
        let mut cdt = Cdt { pts, tris: Vec::new(), edge: HashMap::new(), fixed, hint: 0 };
        for t in tris {
            if t[0] == t[1] || t[1] == t[2] || t[2] == t[0] {
                continue;
            }
            let id = cdt.tris.len();
            for k in 0..3 {
                if cdt.edge.insert((t[k], t[(k + 1) % 3]), id).is_some() {
                    return Err(GeomError::Numerical("triangulation: polygon boundary overlaps itself".into()));
                }
            }
            cdt.tris.push(t);
        }
        cdt.make_delaunay();
        Ok(cdt)
    }

    pub fn is_fixed(&self, a: usize, b: usize) -> bool {
        self.fixed.contains(&ukey(a, b))
    }

    fn third(&self, t: usize, a: usize, b: usize) -> usize {
        let tr = self.tris[t];
        let k = (0..3).find(|&k| tr[k] == a && tr[(k + 1) % 3] == b).expect("edge in triangle");
        tr[(k + 2) % 3]
    }

    fn set_tri(&mut self, id: usize, t: [usize; 3]) {
        if id == self.tris.len() {
            self.tris.push(t);
        } else {
            let old = self.tris[id];
            for k in 0..3 {
                let e = (old[k], old[(k + 1) % 3]);
                if self.edge.get(&e) == Some(&id) {
                    self.edge.remove(&e);
                }
            }
            self.tris[id] = t;
        }
        for k in 0..3 {
            self.edge.insert((t[k], t[(k + 1) % 3]), id);
        }
    }

    /// Flips edge (a, b) when it is unconstrained, interior and not locally Delaunay; returns the
    /// four edges of the flipped quadrilateral.
    fn try_flip(&mut self, a: usize, b: usize) -> Option<[(usize, usize); 4]> {
        if self.is_fixed(a, b) {
            return None;
        }
        let (t1, t2) = (*self.edge.get(&(a, b))?, *self.edge.get(&(b, a))?);
        let (c, d) = (self.third(t1, a, b), self.third(t2, b, a));
        if c == d {
            return None;
        }
        let p = |i: usize| self.pts[i];
        if !in_circle(p(a), p(b), p(c), p(d)) || orient(p(a), p(d), p(c)) <= 0.0 || orient(p(d), p(b), p(c)) <= 0.0 {
            return None;
        }
        self.set_tri(t1, [a, d, c]);
        self.set_tri(t2, [d, b, c]);
        Some([(a, d), (d, b), (b, c), (c, a)])
    }

    fn legalize(&mut self, mut stack: Vec<(usize, usize)>) {
        let mut guard = 0usize;
        let limit = 64 * (self.tris.len() + 16);
        while let Some((a, b)) = stack.pop() {
            guard += 1;
            if guard > limit {
                break;
            }
            if let Some(q) = self.try_flip(a, b) {
                stack.extend_from_slice(&q);
            }
        }
    }

    /// Lawson's algorithm: flip until every unconstrained edge is locally Delaunay.
    fn make_delaunay(&mut self) {
        let mut stack: Vec<(usize, usize)> = self.edge.keys().filter(|(a, b)| a < b).copied().collect();
        stack.sort_unstable();
        self.legalize(stack);
    }

    /// Triangle containing `p` (closed), by a visibility walk with a linear-scan fallback.
    fn locate(&mut self, p: P2) -> Option<usize> {
        if self.tris.is_empty() {
            return None;
        }
        let mut t = self.hint.min(self.tris.len() - 1);
        'walk: for _ in 0..self.tris.len().min(4096) {
            let tr = self.tris[t];
            for k in 0..3 {
                let (u, v) = (tr[k], tr[(k + 1) % 3]);
                if orient(self.pts[u], self.pts[v], p) < 0.0 {
                    match self.edge.get(&(v, u)) {
                        Some(&n) => {
                            t = n;
                            continue 'walk;
                        }
                        None => break 'walk,
                    }
                }
            }
            self.hint = t;
            return Some(t);
        }
        let found = (0..self.tris.len()).find(|&t| {
            let tr = self.tris[t];
            (0..3).all(|k| orient(self.pts[tr[k]], self.pts[tr[(k + 1) % 3]], p) >= 0.0)
        });
        if let Some(t) = found {
            self.hint = t;
        }
        found
    }

    /// Inserts a Steiner point; `None` when it lies outside the domain, within `min_dist` of an
    /// existing vertex, or on / within `min_dist` of a constrained edge.
    pub fn insert(&mut self, p: P2, min_dist: f64) -> Option<usize> {
        let t = self.locate(p)?;
        let tr = self.tris[t];
        let d2 = |a: P2, b: P2| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
        if tr.iter().any(|&v| d2(self.pts[v], p) <= min_dist * min_dist) {
            return None;
        }
        // distance to each edge of the containing triangle
        let mut near: Option<(usize, f64)> = None;
        for k in 0..3 {
            let (a, b) = (self.pts[tr[k]], self.pts[tr[(k + 1) % 3]]);
            let len = d2(a, b).sqrt();
            let dist = if len > 0.0 { orient(a, b, p) / len } else { 0.0 };
            if dist <= min_dist && near.is_none_or(|(_, d)| dist < d) {
                near = Some((k, dist));
            }
        }
        let id = self.pts.len();
        if let Some((k, _)) = near {
            let (a, b, c) = (tr[k], tr[(k + 1) % 3], tr[(k + 2) % 3]);
            if self.is_fixed(a, b) {
                return None;
            }
            let t2 = *self.edge.get(&(b, a))?;
            let d = self.third(t2, b, a);
            self.pts.push(p);
            self.edge.remove(&(a, b));
            self.edge.remove(&(b, a));
            self.set_tri(t, [a, id, c]);
            self.set_tri(t2, [b, id, d]);
            let n = self.tris.len();
            self.set_tri(n, [id, b, c]);
            self.set_tri(n + 1, [id, a, d]);
            self.legalize(vec![(b, c), (c, a), (a, d), (d, b)]);
        } else {
            let [a, b, c] = tr;
            self.pts.push(p);
            self.set_tri(t, [a, b, id]);
            let n = self.tris.len();
            self.set_tri(n, [b, c, id]);
            self.set_tri(n + 1, [c, a, id]);
            self.legalize(vec![(a, b), (b, c), (c, a)]);
        }
        Some(id)
    }
}

/// Splices the holes into the outer ring (Eberly's bridges, or at a shared vertex).
fn bridge(pts: &[P2], outer: &[usize], holes: &[Vec<usize>]) -> Vec<usize> {
    let mut poly = outer.to_vec();
    let maxx = |h: &[usize]| h.iter().map(|&i| pts[i][0]).fold(f64::NEG_INFINITY, f64::max);
    let mut hs: Vec<&Vec<usize>> = holes.iter().filter(|h| h.len() >= 3).collect();
    hs.sort_by(|a, b| maxx(b).total_cmp(&maxx(a)));
    for h in hs {
        let n = poly.len();
        let at = |poly: &[usize], i: usize| pts[poly[(i + n) % n]];
        let wedge_ok = |poly: &[usize], i: usize, q: P2| in_wedge(at(poly, i + n - 1), at(poly, i), at(poly, i + 1), q);
        // a vertex shared with the boundary: splice there, choosing the occurrence whose wedge
        // contains the hole's edges
        let mut shared: Option<(usize, usize, usize)> = None; // (score, hole index, poly position)
        for (hi, &hv) in h.iter().enumerate() {
            let (hp, hn) = (pts[h[(hi + h.len() - 1) % h.len()]], pts[h[(hi + 1) % h.len()]]);
            for pos in (0..n).filter(|&pos| poly[pos] == hv) {
                let score = usize::from(wedge_ok(&poly, pos, hp)) + usize::from(wedge_ok(&poly, pos, hn)) + 1;
                if shared.is_none_or(|s| score > s.0) {
                    shared = Some((score, hi, pos));
                }
            }
        }
        if let Some((_, hi, pos)) = shared {
            let mut out = poly[..=pos].to_vec();
            out.extend((1..=h.len()).map(|k| h[(hi + k) % h.len()]));
            out.extend_from_slice(&poly[pos + 1..]);
            poly = out;
            continue;
        }
        let mi = (0..h.len()).max_by(|&a, &b| pts[h[a]][0].total_cmp(&pts[h[b]][0]).then(pts[h[b]][1].total_cmp(&pts[h[a]][1]))).unwrap_or(0);
        let m = pts[h[mi]];
        let pos = visible_vertex(pts, &poly, m);
        let mut out = poly[..=pos].to_vec();
        out.extend((0..=h.len()).map(|k| h[(mi + k) % h.len()]));
        out.extend_from_slice(&poly[pos..]);
        poly = out;
    }
    poly
}

/// Position in `poly` of a vertex visible from the interior point `m` (Eberly's ray method).
fn visible_vertex(pts: &[P2], poly: &[usize], m: P2) -> usize {
    let n = poly.len();
    let at = |i: usize| pts[poly[i % n]];
    let mut hit: Option<(f64, usize)> = None;
    for i in 0..n {
        let (a, b) = (at(i), at(i + 1));
        if !(a[1] <= m[1] && m[1] <= b[1] && a[1] < b[1]) || orient(a, b, m) <= 0.0 {
            continue;
        }
        let x = a[0] + (m[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
        if hit.is_none_or(|(hx, _)| x < hx) {
            hit = Some((x, i));
        }
    }
    let wedge_ok = |i: usize, q: P2| in_wedge(at(i + n - 1), at(i), at(i + 1), q);
    // the chosen vertex index may occur several times; take an occurrence whose wedge sees m
    let settle = |pos: usize| -> usize {
        if wedge_ok(pos, m) {
            return pos;
        }
        (0..n).find(|&j| poly[j] == poly[pos] && wedge_ok(j, m)).unwrap_or(pos)
    };
    let Some((x, i)) = hit else {
        // no edge to the right (degenerate input): nearest vertex whose wedge sees m
        let d = |j: usize| (at(j)[0] - m[0]).powi(2) + (at(j)[1] - m[1]).powi(2);
        let best = (0..n).filter(|&j| wedge_ok(j, m)).min_by(|&a, &b| d(a).total_cmp(&d(b)));
        return best.unwrap_or_else(|| (0..n).min_by(|&a, &b| d(a).total_cmp(&d(b))).unwrap_or(0));
    };
    let ip = [x, m[1]];
    let (a, b) = (at(i), at(i + 1));
    if b == ip {
        return settle((i + 1) % n);
    }
    if a == ip {
        return settle(i);
    }
    let p_pos = if a[0] > b[0] { i } else { (i + 1) % n };
    let p = at(p_pos);
    // reflex (or flat) vertices inside the closed triangle m–I–P block the bridge to P
    let s = orient(m, ip, p).signum();
    let mut best: Option<(f64, f64, usize)> = None; // (tan of angle to the ray, distance², pos)
    for j in 0..n {
        if poly[j] == poly[p_pos] {
            continue;
        }
        let r = at(j);
        if orient(at(j + n - 1), r, at(j + 1)) > 0.0 {
            continue;
        }
        let inside = orient(m, ip, r) * s >= 0.0 && orient(ip, p, r) * s >= 0.0 && orient(p, m, r) * s >= 0.0;
        if !inside || r[0] <= m[0] {
            continue;
        }
        let (dx, dy) = (r[0] - m[0], r[1] - m[1]);
        let key = (dy.abs() / dx, dx * dx + dy * dy, j);
        if best.is_none_or(|b| key.0 < b.0 || (key.0 == b.0 && key.1 < b.1)) {
            best = Some(key);
        }
    }
    settle(best.map_or(p_pos, |b| b.2))
}

/// Ear clipping of a weakly simple counter-clockwise polygon given as indices into `pts`.
fn ear_clip(pts: &[P2], poly: &[usize]) -> Vec<[usize; 3]> {
    let n = poly.len();
    let mut out = Vec::with_capacity(n.saturating_sub(2));
    if n < 3 {
        return out;
    }
    let mut prev: Vec<usize> = (0..n).map(|i| (i + n - 1) % n).collect();
    let mut next: Vec<usize> = (0..n).map(|i| (i + 1) % n).collect();
    let p = |i: usize| pts[poly[i]];
    let is_ear = |i: usize, prev: &[usize], next: &[usize]| -> bool {
        let (ia, ic) = (prev[i], next[i]);
        let (a, b, c) = (p(ia), p(i), p(ic));
        if orient(a, b, c) <= 0.0 {
            return false;
        }
        let lo = [a[0].min(b[0]).min(c[0]), a[1].min(b[1]).min(c[1])];
        let hi = [a[0].max(b[0]).max(c[0]), a[1].max(b[1]).max(c[1])];
        let mut j = next[ic];
        while j != ia {
            let q = p(j);
            if q[0] >= lo[0] && q[0] <= hi[0] && q[1] >= lo[1] && q[1] <= hi[1] {
                let corner = [(ia, a, b, c), (i, b, c, a), (ic, c, a, b)].into_iter().find(|&(k, x, _, _)| poly[k] == poly[j] || x == q);
                if let Some((_, x, y, z)) = corner {
                    // another occurrence of a corner: its edges must not enter the triangle
                    // (wedge at x spanned by x→y and x→z, triangle counter-clockwise x, y, z)
                    for nb in [p(prev[j]), p(next[j])] {
                        if orient(x, y, nb) > 0.0 && orient(x, z, nb) < 0.0 {
                            return false;
                        }
                    }
                } else if orient(a, b, q) >= 0.0 && orient(b, c, q) >= 0.0 && orient(c, a, q) >= 0.0 {
                    return false;
                }
            }
            j = next[j];
        }
        true
    };
    let mut count = n;
    let mut i = 0;
    let mut miss = 0;
    while count > 3 {
        let ear = if is_ear(i, &prev, &next) {
            Some(i)
        } else {
            miss += 1;
            if miss < count {
                i = next[i];
                continue;
            }
            // no ear (numerically degenerate input): clip the most convex vertex
            let mut best = i;
            let mut k = next[i];
            while k != i {
                if orient(p(prev[k]), p(k), p(next[k])) > orient(p(prev[best]), p(best), p(next[best])) {
                    best = k;
                }
                k = next[k];
            }
            Some(best)
        };
        let e = ear.expect("ear");
        out.push([poly[prev[e]], poly[e], poly[next[e]]]);
        let (pa, nx) = (prev[e], next[e]);
        next[pa] = nx;
        prev[nx] = pa;
        count -= 1;
        miss = 0;
        i = pa;
    }
    out.push([poly[prev[i]], poly[i], poly[next[i]]]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orient_exact_sign() {
        // nearly collinear points where the naive determinant is unreliable
        let a = [0.5, 0.5];
        let b = [12.0, 12.0];
        let c = [24.0, 24.0];
        assert_eq!(orient(a, b, c), 0.0);
        let c2 = [24.0, 24.0 + 4.0 * f64::EPSILON * 24.0];
        assert!(orient(a, b, c2) > 0.0);
    }

    #[test]
    fn square_with_hole() {
        let pts = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [1.0, 1.0], [1.0, 3.0], [3.0, 3.0], [3.0, 1.0]];
        let cdt = Cdt::new(pts, &[0, 1, 2, 3], &[vec![4, 5, 6, 7]]).unwrap();
        assert_eq!(cdt.tris.len(), 8);
        let area: f64 = cdt.tris.iter().map(|t| orient(cdt.pts[t[0]], cdt.pts[t[1]], cdt.pts[t[2]]) / 2.0).sum();
        assert!((area - 12.0).abs() < 1e-12);
        assert!(cdt.tris.iter().all(|t| orient(cdt.pts[t[0]], cdt.pts[t[1]], cdt.pts[t[2]]) > 0.0));
    }

    #[test]
    fn collinear_runs_and_steiner() {
        // square with extra points on every side
        let mut pts = Vec::new();
        for k in 0..4 {
            pts.push([k as f64, 0.0]);
        }
        for k in 0..4 {
            pts.push([4.0, k as f64]);
        }
        for k in 0..4 {
            pts.push([4.0 - k as f64, 4.0]);
        }
        for k in 0..4 {
            pts.push([0.0, 4.0 - k as f64]);
        }
        let ring: Vec<usize> = (0..16).collect();
        let mut cdt = Cdt::new(pts, &ring, &[]).unwrap();
        assert_eq!(cdt.tris.len(), 14);
        assert!(cdt.tris.iter().all(|t| orient(cdt.pts[t[0]], cdt.pts[t[1]], cdt.pts[t[2]]) > 0.0));
        assert!(cdt.insert([2.0, 2.0], 1e-9).is_some());
        assert!(cdt.insert([2.0, 0.0], 1e-9).is_none(), "on a constrained edge");
        assert!(cdt.insert([5.0, 2.0], 1e-9).is_none(), "outside");
        assert_eq!(cdt.tris.len(), 16);
        let area: f64 = cdt.tris.iter().map(|t| orient(cdt.pts[t[0]], cdt.pts[t[1]], cdt.pts[t[2]]) / 2.0).sum();
        assert!((area - 16.0).abs() < 1e-12);
    }
}
