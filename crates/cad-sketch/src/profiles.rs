//! Profile (closed region) detection.
//!
//! Non-construction curves are split at every intersection, dangling edges are pruned, and the
//! remaining planar graph is traced into faces. Every bounded face becomes a profile; faces of
//! other connected components nested inside it become holes. This reproduces the profile
//! behaviour observed in Fusion: a circle drawn inside a rectangle on a face sketch produced two
//! profiles (the ring, 1965.4 mm², and the disc, 113.1 mm²); an open chain of three lines
//! produced none.

use crate::model::{EntityKind, Sketch};
use std::f64::consts::{PI, TAU};

const EPS: f64 = 1e-7;

#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    Line { a: [f64; 2], b: [f64; 2] },
    /// Arc around `c` from angle `a0`, sweeping `sweep` radians (positive = CCW).
    Arc { c: [f64; 2], r: f64, a0: f64, sweep: f64 },
}

impl Segment {
    pub fn start(&self) -> [f64; 2] {
        match *self {
            Segment::Line { a, .. } => a,
            Segment::Arc { c, r, a0, .. } => [c[0] + r * a0.cos(), c[1] + r * a0.sin()],
        }
    }
    pub fn end(&self) -> [f64; 2] {
        match *self {
            Segment::Line { b, .. } => b,
            Segment::Arc { c, r, a0, sweep } => [c[0] + r * (a0 + sweep).cos(), c[1] + r * (a0 + sweep).sin()],
        }
    }
    pub fn mid(&self) -> [f64; 2] {
        match *self {
            Segment::Line { a, b } => [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0],
            Segment::Arc { c, r, a0, sweep } => [c[0] + r * (a0 + sweep / 2.0).cos(), c[1] + r * (a0 + sweep / 2.0).sin()],
        }
    }
    fn reversed(&self) -> Segment {
        match *self {
            Segment::Line { a, b } => Segment::Line { a: b, b: a },
            Segment::Arc { c, r, a0, sweep } => Segment::Arc { c, r, a0: a0 + sweep, sweep: -sweep },
        }
    }
    /// Unit tangent at the start, pointing along the segment.
    fn start_tangent(&self) -> [f64; 2] {
        match *self {
            Segment::Line { a, b } => norm([b[0] - a[0], b[1] - a[1]]),
            Segment::Arc { a0, sweep, .. } => {
                let s = sweep.signum();
                [-a0.sin() * s, a0.cos() * s]
            }
        }
    }
    /// Signed curvature at the start (for ordering edges with equal tangents).
    fn start_curvature(&self) -> f64 {
        match *self {
            Segment::Line { .. } => 0.0,
            Segment::Arc { r, sweep, .. } => sweep.signum() / r,
        }
    }
    /// Contribution to the shoelace area integral (∮ x dy − y dx) / 2.
    fn area_term(&self) -> f64 {
        match *self {
            Segment::Line { a, b } => 0.5 * (a[0] * b[1] - b[0] * a[1]),
            Segment::Arc { c, r, a0, sweep } => {
                let a1 = a0 + sweep;
                0.5 * (r * r * sweep + c[0] * r * (a1.sin() - a0.sin()) - c[1] * r * (a1.cos() - a0.cos()))
            }
        }
    }
    /// Polyline approximation (including start, excluding end).
    pub fn tessellate(&self, out: &mut Vec<[f64; 2]>) {
        match *self {
            Segment::Line { a, .. } => out.push(a),
            Segment::Arc { c, r, a0, sweep } => {
                let n = ((sweep.abs() / (PI / 36.0)).ceil() as usize).max(2);
                for i in 0..n {
                    let t = a0 + sweep * i as f64 / n as f64;
                    out.push([c[0] + r * t.cos(), c[1] + r * t.sin()]);
                }
            }
        }
    }
}

fn norm(v: [f64; 2]) -> [f64; 2] {
    let l = (v[0] * v[0] + v[1] * v[1]).sqrt().max(1e-15);
    [v[0] / l, v[1] / l]
}
fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// A closed loop of segments, each tagged with the sketch curve it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Loop {
    pub segments: Vec<(Segment, String)>,
}

impl Loop {
    pub fn signed_area(&self) -> f64 {
        self.segments.iter().map(|(s, _)| s.area_term()).sum()
    }
    pub fn polygon(&self) -> Vec<[f64; 2]> {
        let mut p = Vec::new();
        for (s, _) in &self.segments {
            s.tessellate(&mut p);
        }
        p
    }
    fn contains(&self, p: [f64; 2]) -> bool {
        point_in_polygon(&self.polygon(), p)
    }
}

/// A closed region usable by Extrude/Revolve: an outer loop (CCW) and holes (CW).
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub outer: Loop,
    pub holes: Vec<Loop>,
    pub area: f64,
    /// Sorted, de-duplicated ids of the sketch curves bounding the region (persistent key).
    pub curves: Vec<String>,
}

impl Profile {
    pub fn contains(&self, p: [f64; 2]) -> bool {
        self.outer.contains(p) && !self.holes.iter().any(|h| h.contains(p))
    }
    /// A point strictly inside the region.
    pub fn interior_point(&self) -> [f64; 2] {
        let poly = self.outer.polygon();
        let (mut minx, mut maxx, mut miny, mut maxy) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for p in &poly {
            minx = minx.min(p[0]);
            maxx = maxx.max(p[0]);
            miny = miny.min(p[1]);
            maxy = maxy.max(p[1]);
        }
        // scan horizontal lines; take the midpoint of the widest inside span
        let mut best = ([0.0, 0.0], -1.0);
        for k in 1..24 {
            let y = miny + (maxy - miny) * k as f64 / 24.0;
            let mut xs: Vec<f64> = Vec::new();
            let all: Vec<Vec<[f64; 2]>> = std::iter::once(poly.clone()).chain(self.holes.iter().map(|h| h.polygon())).collect();
            for pg in &all {
                for i in 0..pg.len() {
                    let (a, b) = (pg[i], pg[(i + 1) % pg.len()]);
                    if (a[1] > y) != (b[1] > y) {
                        xs.push(a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                    }
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for w in xs.chunks(2) {
                if w.len() == 2 && w[1] - w[0] > best.1 {
                    best = ([(w[0] + w[1]) / 2.0, y], w[1] - w[0]);
                }
            }
        }
        if best.1 < 0.0 { [(minx + maxx) / 2.0, (miny + maxy) / 2.0] } else { best.0 }
    }
}

pub fn point_in_polygon(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
    }
    inside
}

/// A source curve as an analytic primitive.
#[derive(Clone, Debug)]
enum Prim {
    Line { a: [f64; 2], b: [f64; 2] },
    /// Full circle (`sweep == TAU`) or CCW arc.
    Arc { c: [f64; 2], r: f64, a0: f64, sweep: f64 },
}

impl Prim {
    fn point_at(&self, t: f64) -> [f64; 2] {
        match *self {
            Prim::Line { a, b } => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
            Prim::Arc { c, r, a0, sweep } => {
                let ang = a0 + sweep * t;
                [c[0] + r * ang.cos(), c[1] + r * ang.sin()]
            }
        }
    }
    /// Parameter (0..1) of a point known to lie on the primitive, if within range.
    fn param_of(&self, p: [f64; 2]) -> Option<f64> {
        match *self {
            Prim::Line { a, b } => {
                let d = [b[0] - a[0], b[1] - a[1]];
                let l2 = d[0] * d[0] + d[1] * d[1];
                let t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2;
                (t > -EPS && t < 1.0 + EPS).then_some(t.clamp(0.0, 1.0))
            }
            Prim::Arc { c, a0, sweep, .. } => {
                let rel = ((p[1] - c[1]).atan2(p[0] - c[0]) - a0).rem_euclid(TAU);
                if rel <= sweep + EPS {
                    Some((rel / sweep).min(1.0))
                } else if TAU - rel < EPS {
                    Some(0.0) // just before the start angle: the start point itself
                } else {
                    None
                }
            }
        }
    }
    fn sub(&self, t0: f64, t1: f64) -> Segment {
        match *self {
            Prim::Line { .. } => Segment::Line { a: self.point_at(t0), b: self.point_at(t1) },
            Prim::Arc { c, r, a0, sweep } => Segment::Arc { c, r, a0: a0 + sweep * t0, sweep: sweep * (t1 - t0) },
        }
    }
}

fn intersections(p: &Prim, q: &Prim) -> Vec<[f64; 2]> {
    match (p, q) {
        (Prim::Line { a, b }, Prim::Line { a: c, b: d }) => {
            let r = [b[0] - a[0], b[1] - a[1]];
            let s = [d[0] - c[0], d[1] - c[1]];
            let den = r[0] * s[1] - r[1] * s[0];
            if den.abs() < 1e-12 {
                // collinear overlap: endpoints lying on the other segment
                let mut out = Vec::new();
                for (pt, other) in [(*c, p), (*d, p), (*a, q), (*b, q)] {
                    if let Some(t) = other.param_of(pt) {
                        if dist(other.point_at(t), pt) < 1e-6 {
                            out.push(pt);
                        }
                    }
                }
                return out;
            }
            let t = ((c[0] - a[0]) * s[1] - (c[1] - a[1]) * s[0]) / den;
            let u = ((c[0] - a[0]) * r[1] - (c[1] - a[1]) * r[0]) / den;
            if (-EPS..=1.0 + EPS).contains(&t) && (-EPS..=1.0 + EPS).contains(&u) {
                vec![p.point_at(t.clamp(0.0, 1.0))]
            } else {
                vec![]
            }
        }
        (Prim::Line { a, b }, Prim::Arc { c, r, .. }) | (Prim::Arc { c, r, .. }, Prim::Line { a, b }) => {
            let d = [b[0] - a[0], b[1] - a[1]];
            let f = [a[0] - c[0], a[1] - c[1]];
            let aa = d[0] * d[0] + d[1] * d[1];
            let bb = 2.0 * (f[0] * d[0] + f[1] * d[1]);
            let cc = f[0] * f[0] + f[1] * f[1] - r * r;
            let disc = bb * bb - 4.0 * aa * cc;
            let mut out = Vec::new();
            if disc < -1e-9 * aa.max(1.0) {
                return out;
            }
            let sq = disc.max(0.0).sqrt();
            let arc = if matches!(p, Prim::Arc { .. }) { p } else { q };
            for t in [(-bb - sq) / (2.0 * aa), (-bb + sq) / (2.0 * aa)] {
                if (-EPS..=1.0 + EPS).contains(&t) {
                    let pt = [a[0] + d[0] * t, a[1] + d[1] * t];
                    if arc.param_of(pt).is_some() && !out.iter().any(|o| dist(*o, pt) < 1e-6) {
                        out.push(pt);
                    }
                }
            }
            out
        }
        (Prim::Arc { c: c1, r: r1, .. }, Prim::Arc { c: c2, r: r2, .. }) => {
            let d = dist(*c1, *c2);
            if d < 1e-12 || d > r1 + r2 + 1e-9 || d < (r1 - r2).abs() - 1e-9 {
                return vec![];
            }
            let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
            let h = (r1 * r1 - a * a).max(0.0).sqrt();
            let m = [c1[0] + a * (c2[0] - c1[0]) / d, c1[1] + a * (c2[1] - c1[1]) / d];
            let mut out = Vec::new();
            for s in [-1.0, 1.0] {
                let pt = [m[0] + s * h * (c2[1] - c1[1]) / d, m[1] - s * h * (c2[0] - c1[0]) / d];
                if p.param_of(pt).is_some() && q.param_of(pt).is_some() && !out.iter().any(|o| dist(*o, pt) < 1e-6) {
                    out.push(pt);
                }
            }
            out
        }
    }
}

struct Graph {
    verts: Vec<[f64; 2]>,
    /// half-edges: (from, to, segment, source curve id)
    half: Vec<(usize, usize, Segment, String)>,
}

impl Graph {
    fn vertex(&mut self, p: [f64; 2]) -> usize {
        if let Some(i) = self.verts.iter().position(|v| dist(*v, p) < 1e-6) {
            return i;
        }
        self.verts.push(p);
        self.verts.len() - 1
    }
}

/// Computes all profiles of a sketch from its solved geometry.
pub fn find_profiles(sketch: &Sketch) -> Vec<Profile> {
    let mut prims: Vec<(Prim, String)> = Vec::new();
    for e in sketch.entities.iter().filter(|e| e.is_curve() && !e.construction) {
        match &e.kind {
            EntityKind::Line { p1, p2 } => {
                let (a, b) = (sketch.point(p1).unwrap_or_default(), sketch.point(p2).unwrap_or_default());
                if dist(a, b) > 1e-9 {
                    prims.push((Prim::Line { a, b }, e.id.clone()));
                }
            }
            EntityKind::Circle { c, r } => {
                if *r > 1e-9 {
                    prims.push((Prim::Arc { c: sketch.point(c).unwrap_or_default(), r: *r, a0: 0.0, sweep: TAU }, e.id.clone()));
                }
            }
            EntityKind::Arc { c, start, end, r } => {
                let cc = sketch.point(c).unwrap_or_default();
                let (s, t) = (sketch.point(start).unwrap_or_default(), sketch.point(end).unwrap_or_default());
                let a0 = (s[1] - cc[1]).atan2(s[0] - cc[0]);
                let a1 = (t[1] - cc[1]).atan2(t[0] - cc[0]);
                let mut sweep = (a1 - a0).rem_euclid(TAU);
                if sweep < 1e-9 {
                    sweep = TAU;
                }
                prims.push((Prim::Arc { c: cc, r: *r, a0, sweep }, e.id.clone()));
            }
            EntityKind::Point { .. } => {}
        }
    }

    // split parameters per primitive
    let mut splits: Vec<Vec<f64>> = prims.iter().map(|(p, _)| match p {
        Prim::Arc { sweep, .. } if (*sweep - TAU).abs() < 1e-12 => vec![0.0, 0.5, 1.0],
        _ => vec![0.0, 1.0],
    }).collect();
    for i in 0..prims.len() {
        for j in (i + 1)..prims.len() {
            for pt in intersections(&prims[i].0, &prims[j].0) {
                if let Some(t) = prims[i].0.param_of(pt) {
                    splits[i].push(t);
                }
                if let Some(t) = prims[j].0.param_of(pt) {
                    splits[j].push(t);
                }
            }
        }
    }

    let mut g = Graph { verts: Vec::new(), half: Vec::new() };
    let mut edges: Vec<(usize, usize, Segment, String)> = Vec::new();
    for (k, (prim, id)) in prims.iter().enumerate() {
        let mut ts = splits[k].clone();
        ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        for w in ts.windows(2) {
            let seg = prim.sub(w[0], w[1]);
            if dist(seg.start(), seg.end()) < 1e-9 && matches!(seg, Segment::Line { .. }) {
                continue;
            }
            let (u, v) = (g.vertex(seg.start()), g.vertex(seg.end()));
            if u == v {
                continue;
            }
            edges.push((u, v, seg, id.clone()));
        }
    }

    // prune dangling edges (vertices of degree 1) until stable
    loop {
        let mut deg = vec![0usize; g.verts.len()];
        for (u, v, _, _) in &edges {
            deg[*u] += 1;
            deg[*v] += 1;
        }
        let before = edges.len();
        edges.retain(|(u, v, _, _)| deg[*u] > 1 && deg[*v] > 1);
        if edges.len() == before {
            break;
        }
    }

    for (u, v, s, id) in &edges {
        g.half.push((*u, *v, s.clone(), id.clone()));
        g.half.push((*v, *u, s.reversed(), id.clone()));
    }

    // outgoing half-edges per vertex, sorted counter-clockwise by tangent angle
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); g.verts.len()];
    for (h, (u, _, _, _)) in g.half.iter().enumerate() {
        out[*u].push(h);
    }
    let key = |h: usize| {
        let s = &g.half[h].2;
        let t = s.start_tangent();
        (t[1].atan2(t[0]), s.start_curvature())
    };
    for list in out.iter_mut() {
        list.sort_by(|&a, &b| {
            let (ka, kb) = (key(a), key(b));
            ka.0.partial_cmp(&kb.0).unwrap().then(ka.1.partial_cmp(&kb.1).unwrap())
        });
    }
    let twin = |h: usize| h ^ 1;

    // face tracing: next(h) = the edge leaving h's end vertex that is the next clockwise turn
    // after the reverse of h, i.e. the one just before twin(h) in CCW order.
    let mut used = vec![false; g.half.len()];
    let mut loops: Vec<Loop> = Vec::new();
    for start in 0..g.half.len() {
        if used[start] {
            continue;
        }
        let mut segs = Vec::new();
        let mut h = start;
        let mut guard = 0;
        loop {
            used[h] = true;
            segs.push((g.half[h].2.clone(), g.half[h].3.clone()));
            let v = g.half[h].1;
            let list = &out[v];
            let pos = list.iter().position(|&x| x == twin(h)).unwrap();
            h = list[(pos + list.len() - 1) % list.len()];
            guard += 1;
            if h == start || guard > 100_000 {
                break;
            }
        }
        loops.push(Loop { segments: segs });
    }

    let (faces, outers): (Vec<Loop>, Vec<Loop>) = loops.into_iter().partition(|l| l.signed_area() > 1e-9);
    let mut profiles: Vec<Profile> = faces
        .into_iter()
        .map(|l| {
            let area = l.signed_area();
            Profile { outer: l, holes: Vec::new(), area, curves: Vec::new() }
        })
        .collect();

    // A negative loop is the outer boundary of a connected component; it is a hole of the
    // smallest face (from another component) that contains it.
    for hole in outers {
        let probe = hole.segments[0].0.mid();
        let mut best: Option<usize> = None;
        for (i, f) in profiles.iter().enumerate() {
            let same_component = f.outer.segments.iter().any(|(_, id)| hole.segments.iter().any(|(_, hid)| hid == id));
            if same_component {
                continue;
            }
            if f.outer.contains(probe) && best.map(|b| f.area < profiles[b].area).unwrap_or(true) {
                best = Some(i);
            }
        }
        if let Some(b) = best {
            let a = hole.signed_area();
            profiles[b].area += a;
            profiles[b].holes.push(hole);
        }
    }

    for p in profiles.iter_mut() {
        let mut ids: Vec<String> = p.outer.segments.iter().chain(p.holes.iter().flat_map(|h| h.segments.iter())).map(|(_, id)| id.clone()).collect();
        ids.sort();
        ids.dedup();
        p.curves = ids;
    }
    profiles.sort_by(|a, b| b.area.partial_cmp(&a.area).unwrap());
    profiles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_is_one_profile() {
        let mut s = Sketch::new();
        s.add_rectangle([0.0, 0.0], [50.0, 30.0]);
        let p = find_profiles(&s);
        assert_eq!(p.len(), 1);
        assert!((p[0].area - 1500.0).abs() < 1e-6);
        assert_eq!(p[0].curves.len(), 4);
    }

    #[test]
    fn circle_inside_rectangle_gives_ring_and_disc() {
        // Mirrors the Fusion probe: 70x30 face with a 12 mm hole circle -> two profiles
        let mut s = Sketch::new();
        s.add_rectangle([0.0, 0.0], [70.0, 30.0]);
        s.add_circle([35.0, 15.0], 6.0);
        let p = find_profiles(&s);
        assert_eq!(p.len(), 2);
        let disc = std::f64::consts::PI * 36.0;
        assert!((p[0].area - (2100.0 - disc)).abs() < 0.5, "ring area {}", p[0].area);
        assert_eq!(p[0].holes.len(), 1);
        assert!((p[1].area - disc).abs() < 1e-6);
        assert!(p[0].contains([5.0, 5.0]) && !p[0].contains([35.0, 15.0]));
        assert!(p[1].contains([35.0, 15.0]));
    }

    #[test]
    fn open_chain_has_no_profile() {
        // Fusion probe: three connected lines -> profiles.count == 0
        let mut s = Sketch::new();
        let a = s.add_point(100.0, 0.0, None);
        let b = s.add_point(140.0, 0.0, None);
        let c = s.add_point(140.0, 30.0, None);
        let d = s.add_point(100.0, 30.0, None);
        s.add_line_between(&a, &b);
        s.add_line_between(&b, &c);
        s.add_line_between(&c, &d);
        assert!(find_profiles(&s).is_empty());
    }

    #[test]
    fn overlapping_rectangles_split_into_three_regions() {
        let mut s = Sketch::new();
        s.add_rectangle([0.0, 0.0], [20.0, 20.0]);
        s.add_rectangle([10.0, 10.0], [30.0, 30.0]);
        let p = find_profiles(&s);
        assert_eq!(p.len(), 3);
        let total: f64 = p.iter().map(|x| x.area).sum();
        assert!((total - 700.0).abs() < 1e-6);
    }

    #[test]
    fn line_across_circle_splits_it() {
        let mut s = Sketch::new();
        s.add_circle([0.0, 0.0], 10.0);
        s.add_line([-20.0, 0.0], [20.0, 0.0]); // dangling ends are pruned
        let p = find_profiles(&s);
        assert_eq!(p.len(), 2);
        assert!((p[0].area - 50.0 * std::f64::consts::PI).abs() < 1e-6);
    }

    #[test]
    fn construction_geometry_is_ignored() {
        let mut s = Sketch::new();
        s.add_rectangle([0.0, 0.0], [10.0, 10.0]);
        let c = s.add_circle([5.0, 5.0], 2.0);
        s.entity_mut(&c).unwrap().construction = true;
        assert_eq!(find_profiles(&s).len(), 1);
    }

    #[test]
    fn slot_with_arcs() {
        let mut s = Sketch::new();
        let a = s.add_arc_3pt([0.0, -5.0], [-5.0, 0.0], [0.0, 5.0]).unwrap();
        let b = s.add_arc_3pt([20.0, 5.0], [25.0, 0.0], [20.0, -5.0]).unwrap();
        let _ = (a, b);
        s.add_line([0.0, 5.0], [20.0, 5.0]);
        s.add_line([20.0, -5.0], [0.0, -5.0]);
        let p = find_profiles(&s);
        assert_eq!(p.len(), 1);
        let expect = 200.0 + std::f64::consts::PI * 25.0;
        assert!((p[0].area - expect).abs() < 1e-6, "{}", p[0].area);
        let ip = p[0].interior_point();
        assert!(p[0].contains(ip));
    }
}
