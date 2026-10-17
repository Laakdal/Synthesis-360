//! Regularised Boolean operations (union, cut, intersect). This module is the polyhedral pipeline
//! (every face a plane, every edge a line); operands with cylinder or sphere faces or circle
//! edges are dispatched to [`crate::boolean_quadric`] (same structure, analytic curves, see its
//! docs). Other surfaces (cone, torus, NURBS) give `GeomError::Unsupported` there.
//!
//! Method: boundary evaluation by face splitting and classification (Requicha & Voelcker,
//! "Boolean operations in solid modeling: boundary evaluation and merging algorithms", Proc.
//! IEEE 73(1), 1985; Mäntylä, *An Introduction to Solid Modeling*, 1988, ch. 15):
//! 1. **Extraction.** Each operand becomes a list of planar faces (outer loop counter-clockwise
//!    about the outward normal, holes clockwise). All points go through one pool that welds
//!    points closer than `tol.linear`; all face planes go through one registry that maps planes
//!    within `tol.linear` of each other (every vertex of the face within the band, by
//!    [`crate::predicates::orient_plane`]) to one canonical plane. Coplanarity is therefore a
//!    single decision shared by every later step.
//! 2. **Face–face intersection.** For each pair (A face, B face) on different planes, the
//!    intersection line of the two planes is clipped to each face (vertices classified Above/On/
//!    Below the line with the tolerance band, crossings sorted, the midpoint of every span tested
//!    in-or-on the polygon), and the overlap of the two interval sets becomes a cut segment
//!    inserted into both faces. A pair on the same plane inserts each face's boundary into the
//!    other, so coplanar overlaps are delimited exactly.
//! 3. **Splitting.** Each face's boundary plus its cut segments form a planar arrangement:
//!    segments are split at their crossings, touches and collinear overlaps; dangling edges are
//!    pruned; faces of the arrangement are traced by the "next edge clockwise" rule; clockwise
//!    cycles become holes of the smallest counter-clockwise cycle that strictly contains them.
//!    Pieces whose sample point (midpoint of the widest scan-line span) lies outside the original
//!    face are dropped.
//! 4. **Classification** of each piece's sample point against the other solid: *on* when it lies
//!    inside a coplanar face of the other solid (same or opposite orientation), else *in* or
//!    *out* by the generalised winding number (sum of signed solid angles of the other solid's
//!    faces, Van Oosterom & Strackee 1983), which has no ray/edge degeneracies; faces whose plane
//!    passes within `tol` of the point contribute 0 (the point lies in their plane, outside them).
//! 5. **Selection** (regularised set operations): union keeps A-out, B-out and A-on-same; intersect
//!    keeps A-in, B-in and A-on-same; cut A − B keeps A-out, A-on-opposite and B-in reversed.
//! 6. **Sewing and merging** (ASM's observable behaviour, `research/REFERENCE_BEHAVIOR.md`):
//!    vertices lying on an edge are inserted into it (T-junctions); adjacent faces on the same
//!    canonical plane with the same orientation are merged (shared edges cancel, loops are
//!    re-traced); faces are grouped into shells through their edges (an edge with four or more
//!    uses is paired radially, so solids touching along an edge or at a vertex become separate
//!    shells); per shell, vertices of degree 2 between collinear edges are removed; every vertex is
//!    moved to the least-squares intersection of its faces' canonical planes (so near-coincident
//!    input within tolerance yields exact geometry); shells of negative volume become voids of the
//!    smallest enclosing outer shell.
//!
//! Conventions and edge cases:
//! * **Empty result** (e.g. `cut(a, a)`, disjoint `intersect`): `Ok` with an empty [`Brep`] (no
//!   solids). An empty operand is accepted and behaves as the empty set.
//! * **Contact without overlap** (touching at a vertex or along an edge, within `tol`): regularised
//!   out. A union returns both solids as separate lumps (one solid each, no shared vertices or
//!   edges); a cut returns the target unchanged; an intersect returns the empty result. ASM's
//!   recorded line contact (`bool.union.tangent_cylinder`, box ∪ cylinder) is one non-manifold
//!   lump with a 4-coedge edge; the quadric pipeline reproduces that (flagged in
//!   [`Brep::nonmanifold_edges`]). For two *polyhedra* touching along an edge, where no ASM
//!   evidence exists, this pipeline keeps its separate-lumps convention.
//! * **Near-coincidence:** faces whose planes are within `tol.linear` are coplanar; points within
//!   `tol.linear` are one point. Beyond `tol.linear` they are distinct.
//! * The merge rule applies to every pair of adjacent coplanar same-orientation faces of the
//!   result, including any such pair already present in an operand.
//!
//! Complexity is quadratic in the number of faces (all pairs are tested, with bounding-box
//! rejection) and quadratic in the segments per face; adequate for models of hundreds of faces.

use crate::intersect::{plane_plane, SurfaceIntersection};
use crate::predicates::{orient_line_2d, orient_plane, Side};
use crate::{Brep, Curve, EdgeId, Frame, GResult, GeomError, Surface, Tolerance, VertexId, P3, V3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::f64::consts::TAU;

/// The tolerance booleans use by default: ASM's measured point and angle tolerances
/// ([`Tolerance::asm`]), the parity target. [`boolean_default`] applies it.
pub const BOOLEAN_TOLERANCE: Tolerance = Tolerance::asm();

/// Boolean operation; `Cut` is `a − b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BoolOp {
    Union,
    Cut,
    Intersect,
}

/// [`boolean`] with [`BOOLEAN_TOLERANCE`] (ASM's 1e-5 mm / 1e-10 rad).
pub fn boolean_default(a: &Brep, b: &Brep, op: BoolOp) -> GResult<Brep> {
    boolean(a, b, op, &BOOLEAN_TOLERANCE)
}

/// Regularised `a op b` (see the module docs for the method, the empty-result convention and the
/// contact rules). Operands must be closed, consistently oriented solids. Polyhedra (planes and
/// lines) use this module's pipeline; planes, cylinders and spheres bounded by lines and circles use
/// [`crate::boolean_quadric`]; other geometry gives `GeomError::Unsupported`.
pub fn boolean(a: &Brep, b: &Brep, op: BoolOp, tol: &Tolerance) -> GResult<Brep> {
    if !(tol.linear.is_finite() && tol.linear > 0.0 && tol.angular.is_finite() && tol.angular >= 0.0) {
        return Err(GeomError::InvalidInput(format!("boolean: invalid tolerance {tol:?}")));
    }
    // operands with cylinder or sphere faces, or circle edges, go through the quadric pipeline
    if !(is_polyhedral(a) && is_polyhedral(b)) {
        return crate::boolean_quadric::boolean_quadric(a, b, op, tol);
    }
    let mut cx = Ctx { pool: Pool::new(tol.linear), planes: Vec::new(), tol: tol.linear, angular: tol.angular };
    let fa = cx.extract(a, "first operand")?;
    let fb = cx.extract(b, "second operand")?;
    let (mut sa, mut sb) = (vec![Vec::new(); fa.len()], vec![Vec::new(); fb.len()]);
    let (ba, bb): (Vec<_>, Vec<_>) = (fa.iter().map(|f| cx.bounds(f)).collect(), fb.iter().map(|f| cx.bounds(f)).collect());
    for i in 0..fa.len() {
        for j in 0..fb.len() {
            if boxes_overlap(&ba[i], &bb[j], cx.tol) {
                cx.intersect_pair(&fa[i], &fb[j], &mut sa[i], &mut sb[j]);
            }
        }
    }
    let mut kept = Vec::new();
    for (from_a, faces, segs, other) in [(true, &fa, &sa, &fb), (false, &fb, &sb, &fa)] {
        for (f, s) in faces.iter().zip(segs) {
            for (loops, q) in cx.split(f, s)? {
                if let Some(flip) = select(op, from_a, cx.classify(&q, f, other)) {
                    let mut g = PFace { plane: f.plane, sign: f.sign, loops };
                    if flip {
                        g.sign = -g.sign;
                        g.loops.iter_mut().for_each(|l| l.reverse());
                    }
                    kept.push(g);
                }
            }
        }
    }
    cx.sew(kept)
}

/// True when every face is a plane and every edge a line (the polyhedral pipeline of this module).
fn is_polyhedral(b: &Brep) -> bool {
    b.faces.iter().all(|f| matches!(f.surface, Surface::Plane { .. })) && b.edges.iter().all(|e| matches!(e.curve, Some(Curve::Line { .. })))
}

/// Classification of a face piece against the other solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    In,
    Out,
    OnSame,
    OnOpp,
}

/// Requicha–Voelcker selection: `Some(flip)` when the piece is kept.
pub(crate) fn select(op: BoolOp, from_a: bool, c: Class) -> Option<bool> {
    use {BoolOp::*, Class::*};
    match (op, from_a, c) {
        (Union, _, Out) | (Union, true, OnSame) => Some(false),
        (Intersect, _, In) | (Intersect, true, OnSame) => Some(false),
        (Cut, true, Out) | (Cut, true, OnOpp) => Some(false),
        (Cut, false, In) => Some(true),
        _ => None,
    }
}

/// A planar face: canonical plane, orientation of the outward normal relative to the plane's
/// frame z (±1), and loops of pool point ids (outer first, counter-clockwise about the outward
/// normal; holes clockwise).
#[derive(Debug, Clone)]
struct PFace {
    plane: usize,
    sign: f64,
    loops: Vec<Vec<u32>>,
}

impl PFace {
    fn edges(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.loops.iter().flat_map(|l| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()])))
    }
}

/// Point pool welding points closer than `tol` (hash grid of cell `2·tol`).
pub(crate) struct Pool {
    pub(crate) pts: Vec<P3>,
    tol: f64,
    grid: HashMap<[i64; 3], Vec<u32>>,
}

impl Pool {
    pub(crate) fn new(tol: f64) -> Self {
        Self { pts: Vec::new(), tol, grid: HashMap::new() }
    }

    fn cell(&self, p: &P3) -> [i64; 3] {
        let c = 2.0 * self.tol;
        [(p.x / c).floor() as i64, (p.y / c).floor() as i64, (p.z / c).floor() as i64]
    }

    /// Id of the nearest pooled point within `tol`, or a new id.
    pub(crate) fn id(&mut self, p: P3) -> u32 {
        let k = self.cell(&p);
        let mut best: Option<(f64, u32)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for &i in self.grid.get(&[k[0] + dx, k[1] + dy, k[2] + dz]).into_iter().flatten() {
                        let d = (self.pts[i as usize] - p).norm();
                        if d <= self.tol && best.is_none_or(|b| d < b.0) {
                            best = Some((d, i));
                        }
                    }
                }
            }
        }
        if let Some((_, i)) = best {
            return i;
        }
        let i = self.pts.len() as u32;
        self.pts.push(p);
        self.grid.entry(k).or_default().push(i);
        i
    }
}

fn boxes_overlap(a: &(P3, P3), b: &(P3, P3), tol: f64) -> bool {
    (0..3).all(|k| a.0[k] <= b.1[k] + tol && b.0[k] <= a.1[k] + tol)
}

type P2 = [f64; 2];
/// An outer loop and its holes (point ids).
type Piece = (Vec<u32>, Vec<Vec<u32>>);
/// Uses of each undirected edge: (face, loop, position in loop, runs from smaller to larger id).
type EdgeUses = BTreeMap<(u32, u32), Vec<(usize, usize, usize, bool)>>;

fn cross2(a: P2, b: P2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn sub2(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}

fn signed_area(pts: &[P2]) -> f64 {
    let o = pts[0];
    (1..pts.len().saturating_sub(1)).map(|i| cross2(sub2(pts[i], o), sub2(pts[i + 1], o))).sum::<f64>() / 2.0
}

/// Distance from `q` to segment `ab`.
fn seg_dist(q: P2, a: P2, b: P2) -> f64 {
    let (d, w) = (sub2(b, a), sub2(q, a));
    let l2 = d[0] * d[0] + d[1] * d[1];
    let t = if l2 > 0.0 { ((w[0] * d[0] + w[1] * d[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (w[0] - t * d[0]).hypot(w[1] - t * d[1])
}

/// Even–odd crossing test (no boundary handling).
fn crossing_inside(loops: &[Vec<P2>], q: P2) -> bool {
    let mut inside = false;
    for l in loops {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < a[0] + (q[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]) {
                inside = !inside;
            }
        }
    }
    inside
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Loc {
    Inside,
    Boundary,
    Outside,
}

/// Location of `q` in a polygon with holes; within `tol` of an edge is `Boundary`.
fn locate(loops: &[Vec<P2>], q: P2, tol: f64) -> Loc {
    if loops.iter().any(|l| (0..l.len()).any(|i| seg_dist(q, l[i], l[(i + 1) % l.len()]) <= tol)) {
        Loc::Boundary
    } else if crossing_inside(loops, q) {
        Loc::Inside
    } else {
        Loc::Outside
    }
}

/// A point well inside a polygon with holes: the midpoint of the widest span of the scan lines
/// halfway between consecutive vertex ordinates (score = min(span width, slab height)).
fn sample_2d(loops: &[Vec<P2>]) -> Option<P2> {
    let mut ys: Vec<f64> = loops.iter().flatten().map(|p| p[1]).collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    let mut best: Option<(f64, P2)> = None;
    for w in ys.windows(2) {
        let (h, y) = (w[1] - w[0], 0.5 * (w[0] + w[1]));
        let mut xs: Vec<f64> = Vec::new();
        for l in loops {
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                if (a[1] > y) != (b[1] > y) {
                    xs.push(a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        for c in xs.chunks_exact(2) {
            let score = (c[1] - c[0]).min(h);
            if best.is_none_or(|b| score > b.0) {
                best = Some((score, [0.5 * (c[0] + c[1]), y]));
            }
        }
    }
    best.filter(|b| b.0 > 0.0).map(|b| b.1)
}

/// Signed solid angle of triangle (a, b, c) seen from the origin (Van Oosterom & Strackee):
/// positive when the triangle runs counter-clockwise seen from the side away from the origin.
fn solid_angle(a: V3, b: V3, c: V3) -> f64 {
    let (la, lb, lc) = (a.norm(), b.norm(), c.norm());
    let num = a.dot(&b.cross(&c));
    let den = la * lb * lc + a.dot(&b) * lc + a.dot(&c) * lb + b.dot(&c) * la;
    2.0 * num.atan2(den)
}

/// Minimal union–find.
pub(crate) struct Uf(Vec<usize>);

impl Uf {
    pub(crate) fn new(n: usize) -> Self {
        Self((0..n).collect())
    }
    pub(crate) fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }
    pub(crate) fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra.max(rb)] = ra.min(rb);
        }
    }
    /// Groups in order of their smallest member.
    pub(crate) fn groups(&mut self, n: usize) -> Vec<Vec<usize>> {
        let mut m: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for i in 0..n {
            let r = self.find(i);
            m.entry(r).or_default().push(i);
        }
        m.into_values().collect()
    }
}

struct Ctx {
    pool: Pool,
    /// Canonical planes (frame z = plane normal).
    planes: Vec<Frame>,
    tol: f64,
    angular: f64,
}

impl Ctx {
    fn pt(&self, id: u32) -> P3 {
        self.pool.pts[id as usize]
    }

    /// 2D coordinates in the plane's frame, y mirrored when `sign` < 0 (so loops that run
    /// counter-clockwise about the outward normal are counter-clockwise in 2D).
    fn to2d(&self, p: &P3, plane: usize, sign: f64) -> P2 {
        let f = &self.planes[plane];
        let d = p - f.origin;
        [d.dot(&f.x), sign * d.dot(&f.y)]
    }

    fn from2d(&self, q: P2, plane: usize, sign: f64) -> P3 {
        let f = &self.planes[plane];
        f.origin + f.x * q[0] + f.y * (sign * q[1])
    }

    fn loops2d(&self, f: &PFace, sign: f64) -> Vec<Vec<P2>> {
        f.loops.iter().map(|l| l.iter().map(|&i| self.to2d(&self.pt(i), f.plane, sign)).collect()).collect()
    }

    fn bounds(&self, f: &PFace) -> (P3, P3) {
        let mut it = f.loops.iter().flatten().map(|&i| self.pt(i));
        let first = it.next().unwrap_or_else(P3::origin);
        it.fold((first, first), |(lo, hi), p| (lo.inf(&p), hi.sup(&p)))
    }

    /// Canonical plane of a face whose points are `pts` and whose surface frame is `frame`:
    /// `(index, sign of frame.z against the canonical z)`.
    fn plane_of(&mut self, frame: &Frame, pts: &[P3]) -> (usize, f64) {
        for (k, c) in self.planes.iter().enumerate() {
            let d = c.z.dot(&frame.z);
            if d.abs() > 0.5 && pts.iter().all(|p| orient_plane(p, &c.origin, &c.z, self.tol) == Side::On) {
                return (k, d.signum());
            }
        }
        self.planes.push(*frame);
        (self.planes.len() - 1, 1.0)
    }

    fn extract(&mut self, b: &Brep, what: &str) -> GResult<Vec<PFace>> {
        let in_solid: usize = b.solids.iter().map(|s| s.shells.iter().map(|&sh| b.shell(sh).faces.len()).sum::<usize>()).sum();
        if in_solid != b.faces.len() {
            return Err(GeomError::InvalidInput(format!("boolean: {what} has faces outside any solid (open shell)")));
        }
        let mut out = Vec::new();
        for s in b.solid_ids() {
            for f in b.solid_faces(s) {
                let face = b.face(f);
                let Surface::Plane { frame } = &face.surface else {
                    return Err(GeomError::Unsupported(format!(
                        "boolean: {what} face F{} is a {} (only planar polyhedra are supported)",
                        f.0,
                        face.surface.type_name()
                    )));
                };
                if face.loops.is_empty() {
                    return Err(GeomError::InvalidInput(format!("boolean: {what} face F{} has no loop", f.0)));
                }
                let mut loops = Vec::new();
                let mut pts = Vec::new();
                for &l in &face.loops {
                    let mut ids: Vec<u32> = Vec::new();
                    for &c in &b.lp(l).coedges {
                        let e = b.edge(b.coedge(c).edge);
                        if !matches!(e.curve, Some(Curve::Line { .. })) {
                            let t = e.curve.as_ref().map_or("degenerate", |c| c.type_name());
                            return Err(GeomError::Unsupported(format!(
                                "boolean: {what} face F{} has a {t} edge (only line edges are supported)",
                                f.0
                            )));
                        }
                        let p = b.vertex(b.coedge_start(c)).point;
                        pts.push(p);
                        let id = self.pool.id(p);
                        if ids.last() != Some(&id) {
                            ids.push(id);
                        }
                    }
                    while ids.len() > 1 && ids.first() == ids.last() {
                        ids.pop();
                    }
                    if ids.len() < 3 {
                        return Err(GeomError::InvalidInput(format!(
                            "boolean: {what} face F{} has a loop with fewer than 3 distinct vertices at tolerance {}",
                            f.0, self.tol
                        )));
                    }
                    loops.push(ids);
                }
                let (plane, s) = self.plane_of(frame, &pts);
                out.push(PFace { plane, sign: if face.reversed { -s } else { s }, loops });
            }
        }
        Ok(out)
    }

    /// Parameter intervals (along `o + t·d`, `d` unit and in the face plane) where the line is
    /// inside or on the face.
    fn intervals(&self, f: &PFace, o: &P3, d: &V3) -> Vec<(f64, f64)> {
        let m = self.planes[f.plane].z.cross(d).normalize();
        let tol = self.tol;
        let mut ts = Vec::new();
        for l in &f.loops {
            for i in 0..l.len() {
                let (p, q) = (self.pt(l[i]), self.pt(l[(i + 1) % l.len()]));
                let (sp, sq) = (orient_plane(&p, o, &m, tol), orient_plane(&q, o, &m, tol));
                if sp == Side::On {
                    ts.push((p - o).dot(d));
                } else if sq != Side::On && sp != sq {
                    let (dp, dq) = ((p - o).dot(&m), (q - o).dot(&m));
                    ts.push((p + (q - p) * (dp / (dp - dq)) - o).dot(d));
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|b, a| *b - *a <= tol);
        let poly = self.loops2d(f, 1.0);
        let mut out: Vec<(f64, f64)> = Vec::new();
        for w in ts.windows(2) {
            let mid = o + d * (0.5 * (w[0] + w[1]));
            if locate(&poly, self.to2d(&mid, f.plane, 1.0), tol) != Loc::Outside {
                match out.last_mut() {
                    Some(last) if w[0] - last.1 <= tol => last.1 = w[1],
                    _ => out.push((w[0], w[1])),
                }
            }
        }
        out
    }

    /// Cut segments between two faces, appended to both faces' lists.
    fn intersect_pair(&mut self, fa: &PFace, fb: &PFace, sa: &mut Vec<(u32, u32)>, sb: &mut Vec<(u32, u32)>) {
        if fa.plane == fb.plane {
            sa.extend(fb.edges());
            sb.extend(fa.edges());
            return;
        }
        let t = Tolerance { linear: self.tol, angular: self.angular };
        let SurfaceIntersection::Curves { curves, .. } = plane_plane(&self.planes[fa.plane], &self.planes[fb.plane], &t) else {
            return;
        };
        let Some(Curve::Line { origin, dir }) = curves.first() else { return };
        let ia = self.intervals(fa, origin, dir);
        if ia.is_empty() {
            return;
        }
        let ib = self.intervals(fb, origin, dir);
        for &(a0, a1) in &ia {
            for &(b0, b1) in &ib {
                let (t0, t1) = (a0.max(b0), a1.min(b1));
                if t1 - t0 > self.tol {
                    let (i0, i1) = (self.pool.id(origin + dir * t0), self.pool.id(origin + dir * t1));
                    if i0 != i1 {
                        sa.push((i0, i1));
                        sb.push((i0, i1));
                    }
                }
            }
        }
    }

    /// Splits a face by its cut segments: pieces as (loops of ids oriented like the face,
    /// sample point inside the piece).
    fn split(&mut self, f: &PFace, extra: &[(u32, u32)]) -> GResult<Vec<(Vec<Vec<u32>>, P3)>> {
        let key = |(a, b): (u32, u32)| (a.min(b), a.max(b));
        let own: BTreeSet<(u32, u32)> = f.edges().map(key).collect();
        let mut segs: BTreeSet<(u32, u32)> = own.clone();
        segs.extend(extra.iter().copied().filter(|e| e.0 != e.1).map(key));
        let poly = self.loops2d(f, f.sign);
        if segs.len() == own.len() {
            // nothing new: the face is one piece
            let q = sample_2d(&poly).ok_or_else(|| GeomError::Numerical("boolean: degenerate face (no interior point)".into()))?;
            return Ok(vec![(f.loops.clone(), self.from2d(q, f.plane, f.sign))]);
        }
        let segs: Vec<(u32, u32)> = segs.into_iter().collect();
        let tol = self.tol;
        let mut c2: HashMap<u32, P2> = HashMap::new();
        for &(a, b) in &segs {
            for i in [a, b] {
                c2.entry(i).or_insert_with(|| self.to2d(&self.pt(i), f.plane, f.sign));
            }
        }
        let mut splits: Vec<Vec<(f64, u32)>> = segs.iter().map(|&(a, b)| vec![(0.0, a), (1.0, b)]).collect();
        // parameter of `p` on segment (a, b) when it lies on its interior within tol
        let on_seg = |p: P2, a: P2, b: P2| -> Option<f64> {
            let d = sub2(b, a);
            let len = d[0].hypot(d[1]);
            let s = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len;
            (s > tol && s < len - tol && orient_line_2d(p, a, [d[0] / len, d[1] / len], tol) == Side::On).then_some(s / len)
        };
        for i in 0..segs.len() {
            for j in i + 1..segs.len() {
                let (ai, bi, aj, bj) = (segs[i].0, segs[i].1, segs[j].0, segs[j].1);
                let (pai, pbi, paj, pbj) = (c2[&ai], c2[&bi], c2[&aj], c2[&bj]);
                if (0..2).any(|k| pai[k].min(pbi[k]) > paj[k].max(pbj[k]) + tol || paj[k].min(pbj[k]) > pai[k].max(pbi[k]) + tol) {
                    continue;
                }
                for (v, pv, s, (pa, pb), (ea, eb)) in
                    [(aj, paj, i, (pai, pbi), (ai, bi)), (bj, pbj, i, (pai, pbi), (ai, bi)), (ai, pai, j, (paj, pbj), (aj, bj)), (bi, pbi, j, (paj, pbj), (aj, bj))]
                {
                    if v != ea && v != eb && let Some(t) = on_seg(pv, pa, pb) {
                        splits[s].push((t, v));
                    }
                }
                let (di, dj) = (sub2(pbi, pai), sub2(pbj, paj));
                let (li, lj) = (di[0].hypot(di[1]), dj[0].hypot(dj[1]));
                let den = cross2(di, dj);
                if den.abs() <= 1e-12 * li * lj || [ai, bi].contains(&aj) || [ai, bi].contains(&bj) {
                    continue;
                }
                let w = sub2(paj, pai);
                let (s, r) = (cross2(w, dj) / den, cross2(w, di) / den);
                if s * li > tol && (1.0 - s) * li > tol && r * lj > tol && (1.0 - r) * lj > tol {
                    let (p0, p1) = (self.pt(ai), self.pt(bi));
                    let id = self.pool.id(p0 + (p1 - p0) * s);
                    c2.entry(id).or_insert_with(|| self.to2d(&self.pool.pts[id as usize], f.plane, f.sign));
                    splits[i].push((s, id));
                    splits[j].push((r, id));
                }
            }
        }
        let mut edges: BTreeSet<(u32, u32)> = BTreeSet::new();
        for mut sp in splits {
            sp.sort_by(|a, b| a.0.total_cmp(&b.0));
            sp.dedup_by_key(|x| x.1);
            for w in sp.windows(2) {
                if w[0].1 != w[1].1 {
                    edges.insert(key((w[0].1, w[1].1)));
                }
            }
        }
        // prune dangling edges
        loop {
            let mut deg: HashMap<u32, usize> = HashMap::new();
            for &(a, b) in &edges {
                *deg.entry(a).or_default() += 1;
                *deg.entry(b).or_default() += 1;
            }
            let n = edges.len();
            edges.retain(|&(a, b)| deg[&a] > 1 && deg[&b] > 1);
            if edges.len() == n {
                break;
            }
        }
        let directed: Vec<(u32, u32)> = edges.iter().flat_map(|&(a, b)| [(a, b), (b, a)]).collect();
        let cycles = trace_cycles(&directed, &c2)?;
        let mut out = Vec::new();
        for (outer, holes) in assemble(&cycles, &c2, true)? {
            let loops: Vec<Vec<u32>> = std::iter::once(outer).chain(holes).collect();
            let l2: Vec<Vec<P2>> = loops.iter().map(|l| l.iter().map(|i| c2[i]).collect()).collect();
            let Some(q) = sample_2d(&l2) else { continue };
            // the face boundary is part of the arrangement, so every piece is strictly inside or
            // outside it: no tolerance here (a sliver 2·tol wide has its sample at tol from an edge)
            if crossing_inside(&poly, q) {
                out.push((loops, self.from2d(q, f.plane, f.sign)));
            }
        }
        Ok(out)
    }

    fn classify(&self, q: &P3, f: &PFace, other: &[PFace]) -> Class {
        let q2 = self.to2d(q, f.plane, 1.0);
        // coplanar faces of the other solid were inserted into the arrangement, so the piece is
        // strictly inside or outside each of them (no tolerance, as in `split`)
        for g in other.iter().filter(|g| g.plane == f.plane) {
            if crossing_inside(&self.loops2d(g, 1.0), q2) {
                return if g.sign == f.sign { Class::OnSame } else { Class::OnOpp };
            }
        }
        if self.winding(q, other.iter()) > 0.5 { Class::In } else { Class::Out }
    }

    /// Generalised winding number of `q` with respect to a closed set of faces (1 inside, 0
    /// outside). Faces whose plane passes within `tol` of `q` contribute 0: callers only ask about
    /// points that are not on those faces.
    fn winding<'a>(&self, q: &P3, faces: impl Iterator<Item = &'a PFace>) -> f64 {
        let mut total = 0.0;
        for g in faces {
            let pl = &self.planes[g.plane];
            if orient_plane(q, &pl.origin, &pl.z, self.tol) == Side::On {
                continue;
            }
            for l in &g.loops {
                let p0 = self.pt(l[0]) - q;
                for i in 1..l.len() - 1 {
                    total += solid_angle(p0, self.pt(l[i]) - q, self.pt(l[i + 1]) - q);
                }
            }
        }
        total / (2.0 * TAU)
    }

    fn sew(&mut self, faces: Vec<PFace>) -> GResult<Brep> {
        if faces.is_empty() {
            return Ok(Brep::new());
        }
        let faces = self.fix_t_junctions(faces);
        let faces = self.merge_coplanar(faces)?;
        let mut shells: Vec<Vec<PFace>> = Vec::new();
        for g in self.split_shells(&faces)? {
            let mut sf: Vec<PFace> = g.iter().map(|&i| faces[i].clone()).collect();
            self.remove_collinear(&mut sf);
            shells.push(sf);
        }
        let snapped: Vec<HashMap<u32, P3>> = shells.iter().map(|s| self.snap(s)).collect();
        // outer shells (positive volume) and voids
        let vols: Vec<f64> = shells.iter().zip(&snapped).map(|(s, p)| shell_volume(s, p)).collect();
        let outers: Vec<usize> = (0..shells.len()).filter(|&i| vols[i] > 0.0).collect();
        let mut solids: Vec<(usize, Vec<usize>)> = outers.iter().map(|&i| (i, Vec::new())).collect();
        for v in (0..shells.len()).filter(|&i| vols[i] <= 0.0) {
            let f = &shells[v][0];
            let q = sample_2d(&self.loops2d(f, f.sign)).ok_or_else(|| GeomError::Numerical("boolean: degenerate void face".into()))?;
            let q = self.from2d(q, f.plane, f.sign);
            let host = (0..solids.len())
                .filter(|&k| self.winding(&q, shells[solids[k].0].iter()) > 0.5)
                .min_by(|&x, &y| vols[solids[x].0].total_cmp(&vols[solids[y].0]))
                .ok_or_else(|| GeomError::Numerical("boolean: an inward-facing shell lies in no outer shell".into()))?;
            solids[host].1.push(v);
        }
        let mut out = Brep::new();
        for (outer, voids) in solids {
            let s = out.add_solid();
            for sh in std::iter::once(outer).chain(voids) {
                self.build_shell(&mut out, s, &shells[sh], &snapped[sh])?;
            }
        }
        Ok(out)
    }

    /// Inserts every result vertex that lies on the interior of a loop edge into that edge.
    fn fix_t_junctions(&self, mut faces: Vec<PFace>) -> Vec<PFace> {
        let used: BTreeSet<u32> = faces.iter().flat_map(|f| f.loops.iter().flatten().copied()).collect();
        let pts: Vec<(u32, P3)> = used.iter().map(|&i| (i, self.pt(i))).collect();
        for f in &mut faces {
            for l in &mut f.loops {
                let mut nl = Vec::with_capacity(l.len());
                for i in 0..l.len() {
                    let (a, b) = (l[i], l[(i + 1) % l.len()]);
                    nl.push(a);
                    let (pa, pb) = (self.pt(a), self.pt(b));
                    let len = (pb - pa).norm();
                    let d = (pb - pa) / len;
                    let mut ins: Vec<(f64, u32)> = pts
                        .iter()
                        .filter(|(v, _)| *v != a && *v != b)
                        .filter_map(|&(v, p)| {
                            let s = (p - pa).dot(&d);
                            (s > self.tol && s < len - self.tol && (p - pa - d * s).norm() <= self.tol).then_some((s, v))
                        })
                        .collect();
                    ins.sort_by(|x, y| x.0.total_cmp(&y.0));
                    nl.extend(ins.into_iter().map(|x| x.1));
                }
                *l = nl;
            }
        }
        faces
    }

    /// Merges adjacent faces on the same canonical plane with the same orientation.
    fn merge_coplanar(&self, faces: Vec<PFace>) -> GResult<Vec<PFace>> {
        let mut by_edge: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for (i, f) in faces.iter().enumerate() {
            for e in f.edges() {
                by_edge.entry(e).or_default().push(i);
            }
        }
        let mut uf = Uf::new(faces.len());
        for (i, f) in faces.iter().enumerate() {
            for (a, b) in f.edges() {
                for &j in by_edge.get(&(b, a)).into_iter().flatten() {
                    if j != i && faces[j].plane == f.plane && faces[j].sign == f.sign {
                        uf.union(i, j);
                    }
                }
            }
        }
        let mut out = Vec::new();
        for g in uf.groups(faces.len()) {
            if g.len() == 1 {
                out.push(faces[g[0]].clone());
                continue;
            }
            let (plane, sign) = (faces[g[0]].plane, faces[g[0]].sign);
            let mut count: BTreeMap<(u32, u32), i64> = BTreeMap::new();
            for &i in &g {
                for e in faces[i].edges() {
                    *count.entry(e).or_default() += 1;
                }
            }
            let keys: Vec<(u32, u32)> = count.keys().copied().collect();
            for (a, b) in keys {
                let m = count[&(a, b)].min(count.get(&(b, a)).copied().unwrap_or(0));
                if m > 0 {
                    *count.get_mut(&(a, b)).unwrap() -= m;
                    *count.get_mut(&(b, a)).unwrap() -= m;
                }
            }
            let mut directed = Vec::new();
            for (e, n) in count {
                if n > 1 {
                    return Err(GeomError::Numerical(format!("boolean: edge V{}–V{} used {n} times in one merged face", e.0, e.1)));
                }
                if n == 1 {
                    directed.push(e);
                }
            }
            let c2: HashMap<u32, P2> = directed.iter().flat_map(|&(a, b)| [a, b]).map(|i| (i, self.to2d(&self.pt(i), plane, sign))).collect();
            let cycles = trace_cycles(&directed, &c2)?;
            for (outer, holes) in assemble(&cycles, &c2, false)? {
                out.push(PFace { plane, sign, loops: std::iter::once(outer).chain(holes).collect() });
            }
        }
        Ok(out)
    }

    /// Pairs the uses of one edge radially (for edges with four or more uses): each use is
    /// `(face, runs from the smaller to the larger id)`; returns pairs of indices into `uses`
    /// bounding the same material wedge.
    fn radial_pairs(&self, key: (u32, u32), uses: &[(usize, bool)], faces: &[PFace]) -> GResult<Vec<(usize, usize)>> {
        let bad = |m: &str| GeomError::Numerical(format!("boolean: edge V{}–V{} with {} uses: {m}", key.0, key.1, uses.len()));
        if uses.len() == 2 {
            return if uses[0].1 != uses[1].1 { Ok(vec![(0, 1)]) } else { Err(bad("both uses run the same way")) };
        }
        if !uses.len().is_multiple_of(2) {
            return Err(bad("odd number of uses (result not closed)"));
        }
        let d = (self.pt(key.1) - self.pt(key.0)).normalize();
        let r = Frame::from_origin_z(P3::origin(), d).x;
        let s = d.cross(&r);
        let mut rad: Vec<(f64, usize, bool)> = uses
            .iter()
            .enumerate()
            .map(|(k, &(fi, fwd))| {
                let n = self.planes[faces[fi].plane].z * faces[fi].sign;
                let w = n.cross(&if fwd { d } else { -d });
                // material lies on the +θ side of the face when n·(d × w) < 0
                (w.dot(&s).atan2(w.dot(&r)), k, n.dot(&d.cross(&w)) < 0.0)
            })
            .collect();
        rad.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut pairs = Vec::new();
        for i in 0..rad.len() {
            if rad[i].2 {
                let j = (i + 1) % rad.len();
                if rad[j].2 || uses[rad[i].1].1 == uses[rad[j].1].1 {
                    return Err(bad("inconsistent material wedges"));
                }
                pairs.push((rad[i].1, rad[j].1));
            }
        }
        if pairs.len() * 2 != uses.len() {
            return Err(bad("inconsistent material wedges"));
        }
        Ok(pairs)
    }

    /// Uses of every undirected edge: key → [(face, loop, position, forward)].
    fn edge_uses(faces: &[PFace]) -> EdgeUses {
        let mut m = EdgeUses::new();
        for (fi, f) in faces.iter().enumerate() {
            for (li, l) in f.loops.iter().enumerate() {
                for k in 0..l.len() {
                    let (a, b) = (l[k], l[(k + 1) % l.len()]);
                    m.entry((a.min(b), a.max(b))).or_default().push((fi, li, k, a < b));
                }
            }
        }
        m
    }

    /// Groups faces into shells through radially paired edge uses.
    fn split_shells(&self, faces: &[PFace]) -> GResult<Vec<Vec<usize>>> {
        let mut uf = Uf::new(faces.len());
        for (key, us) in Self::edge_uses(faces) {
            let u2: Vec<(usize, bool)> = us.iter().map(|x| (x.0, x.3)).collect();
            for (x, y) in self.radial_pairs(key, &u2, faces)? {
                uf.union(u2[x].0, u2[y].0);
            }
        }
        Ok(uf.groups(faces.len()))
    }

    /// Removes vertices with exactly two neighbours lying between them on a straight line.
    fn remove_collinear(&self, faces: &mut [PFace]) {
        loop {
            let mut nbrs: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
            for f in faces.iter() {
                for (a, b) in f.edges() {
                    nbrs.entry(a).or_default().insert(b);
                    nbrs.entry(b).or_default().insert(a);
                }
            }
            let victim = nbrs.iter().find_map(|(&v, ns)| {
                let [a, b] = <[u32; 2]>::try_from(ns.iter().copied().collect::<Vec<_>>()).ok()?;
                let (pa, pb, pv) = (self.pt(a), self.pt(b), self.pt(v));
                let len = (pb - pa).norm();
                let d = (pb - pa) / len;
                let s = (pv - pa).dot(&d);
                (s > 0.0 && s < len && (pv - pa - d * s).norm() <= self.tol).then_some(v)
            });
            let Some(v) = victim else { return };
            for f in faces.iter_mut() {
                for l in &mut f.loops {
                    if l.len() > 3 {
                        l.retain(|&x| x != v);
                    }
                }
            }
        }
    }

    /// Vertex positions moved to the least-squares intersection of their faces' canonical planes
    /// (minimum-norm correction; near-parallel planes, singular value < 1e-9, count as one).
    fn snap(&self, faces: &[PFace]) -> HashMap<u32, P3> {
        let mut planes: BTreeMap<u32, BTreeSet<usize>> = BTreeMap::new();
        for f in faces {
            for &v in f.loops.iter().flatten() {
                planes.entry(v).or_default().insert(f.plane);
            }
        }
        planes
            .into_iter()
            .map(|(v, ps)| {
                let p = self.pt(v);
                let ps: Vec<&Frame> = ps.iter().map(|&k| &self.planes[k]).collect();
                let m = nalgebra::DMatrix::from_fn(ps.len(), 3, |i, j| ps[i].z[j]);
                let r = nalgebra::DVector::from_fn(ps.len(), |i, _| ps[i].z.dot(&(ps[i].origin - p)));
                let moved = m.svd(true, true).solve(&r, 1e-9).ok().map(|dx| V3::new(dx[0], dx[1], dx[2]));
                // never move further than the welding allowed
                let q = moved.filter(|dx| dx.norm() <= 4.0 * self.tol).map_or(p, |dx| p + dx);
                (v, q)
            })
            .collect()
    }

    fn build_shell(&self, out: &mut Brep, solid: crate::SolidId, faces: &[PFace], pos: &HashMap<u32, P3>) -> GResult<()> {
        let sh = out.add_shell(Some(solid));
        let mut slot: HashMap<(usize, usize, usize), usize> = HashMap::new();
        let mut npairs = 0;
        for (key, us) in Self::edge_uses(faces) {
            let u2: Vec<(usize, bool)> = us.iter().map(|x| (x.0, x.3)).collect();
            for (x, y) in self.radial_pairs(key, &u2, faces)? {
                for z in [x, y] {
                    slot.insert((us[z].0, us[z].1, us[z].2), npairs);
                }
                npairs += 1;
            }
        }
        let mut verts: HashMap<u32, VertexId> = HashMap::new();
        let mut edges: Vec<Option<(EdgeId, u32)>> = vec![None; npairs];
        for (fi, f) in faces.iter().enumerate() {
            let fr = self.planes[f.plane];
            let fid = out.add_face(sh, Surface::Plane { frame: fr }, f.sign < 0.0);
            for (li, l) in f.loops.iter().enumerate() {
                let mut uses = Vec::with_capacity(l.len());
                for k in 0..l.len() {
                    let (a, b) = (l[k], l[(k + 1) % l.len()]);
                    let mut vid = |i: u32| *verts.entry(i).or_insert_with(|| out.add_vertex(pos[&i]));
                    let (va, vb) = (vid(a), vid(b));
                    let e = &mut edges[slot[&(fi, li, k)]];
                    let (eid, start) = *e.get_or_insert_with(|| (out.add_line_edge(va, vb), a));
                    uses.push((eid, start != a));
                }
                out.add_loop(fid, &uses);
            }
        }
        Ok(())
    }
}

/// Six times the signed volume enclosed by a shell (fan triangles about the bounds centre).
fn shell_volume(faces: &[PFace], pos: &HashMap<u32, P3>) -> f64 {
    let mut it = pos.values();
    let first = *it.next().unwrap_or(&P3::origin());
    let (lo, hi) = it.fold((first, first), |(lo, hi), p| (lo.inf(p), hi.sup(p)));
    let c = nalgebra::center(&lo, &hi);
    let mut v = 0.0;
    for f in faces {
        for l in &f.loops {
            let p0 = pos[&l[0]] - c;
            for i in 1..l.len() - 1 {
                v += p0.dot(&(pos[&l[i]] - c).cross(&(pos[&l[i + 1]] - c)));
            }
        }
    }
    v
}

/// Traces the cycles of a set of directed edges: after (u → v) comes the edge leaving v that is
/// first clockwise from the direction v → u (faces on the left, minimal cycles).
fn trace_cycles(directed: &[(u32, u32)], c2: &HashMap<u32, P2>) -> GResult<Vec<Vec<u32>>> {
    let mut outgoing: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(a, b) in directed {
        outgoing.entry(a).or_default().push(b);
    }
    let ang = |from: u32, to: u32| {
        let d = sub2(c2[&to], c2[&from]);
        d[1].atan2(d[0])
    };
    let next = |u: u32, v: u32| -> Option<u32> {
        let back = ang(v, u);
        outgoing.get(&v)?.iter().copied().min_by(|&x, &y| {
            // clockwise turn from `back`, in (0, 2π]; going straight back is the last resort
            let cw = |w: u32| if w == u { TAU } else { (back - ang(v, w)).rem_euclid(TAU).max(f64::MIN_POSITIVE) };
            cw(x).total_cmp(&cw(y))
        })
    };
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    let mut cycles = Vec::new();
    for &start in directed {
        if seen.contains(&start) {
            continue;
        }
        let mut cyc = Vec::new();
        let (mut u, mut v) = start;
        loop {
            if !seen.insert((u, v)) {
                return Err(GeomError::Numerical("boolean: edge cycles do not close (inconsistent arrangement)".into()));
            }
            cyc.push(u);
            let w = next(u, v).ok_or_else(|| GeomError::Numerical("boolean: open edge chain".into()))?;
            (u, v) = (v, w);
            if (u, v) == start {
                break;
            }
        }
        cycles.push(cyc);
    }
    Ok(cycles)
}

/// Groups traced cycles into faces: counter-clockwise cycles are outer loops; each clockwise cycle
/// becomes a hole of the smallest outer loop strictly containing it. Unassigned clockwise cycles
/// (outer boundaries of arrangement components) are dropped when `drop_free_holes`, else an error.
fn assemble(cycles: &[Vec<u32>], c2: &HashMap<u32, P2>, drop_free_holes: bool) -> GResult<Vec<Piece>> {
    let pts = |c: &Vec<u32>| c.iter().map(|i| c2[i]).collect::<Vec<P2>>();
    let area: Vec<f64> = cycles.iter().map(|c| signed_area(&pts(c))).collect();
    let outers: Vec<usize> = (0..cycles.len()).filter(|&i| area[i] > 0.0).collect();
    let mut holes: Vec<Vec<Vec<u32>>> = vec![Vec::new(); outers.len()];
    for h in (0..cycles.len()).filter(|&i| area[i] < 0.0) {
        let host = (0..outers.len())
            .filter(|&k| strictly_inside(&cycles[h], &cycles[outers[k]], c2))
            .min_by(|&x, &y| area[outers[x]].total_cmp(&area[outers[y]]));
        match host {
            Some(k) => holes[k].push(cycles[h].clone()),
            None if drop_free_holes => {}
            None => return Err(GeomError::Numerical("boolean: a merged face has a hole outside its outer loop".into())),
        }
    }
    Ok(outers.iter().zip(holes).map(|(&o, h)| (cycles[o].clone(), h)).collect())
}

/// True when cycle `inner` lies strictly inside cycle `outer` (tested at a vertex of `inner` that
/// is not a vertex of `outer`, else at the midpoint of an edge that `outer` does not use).
fn strictly_inside(inner: &[u32], outer: &[u32], c2: &HashMap<u32, P2>) -> bool {
    let poly = vec![outer.iter().map(|i| c2[i]).collect::<Vec<P2>>()];
    if let Some(v) = inner.iter().find(|v| !outer.contains(v)) {
        return crossing_inside(&poly, c2[v]);
    }
    let oe: HashSet<(u32, u32)> = (0..outer.len()).map(|i| (outer[i], outer[(i + 1) % outer.len()])).collect();
    for i in 0..inner.len() {
        let (a, b) = (inner[i], inner[(i + 1) % inner.len()]);
        if !oe.contains(&(a, b)) && !oe.contains(&(b, a)) {
            let (pa, pb) = (c2[&a], c2[&b]);
            return crossing_inside(&poly, [0.5 * (pa[0] + pb[0]), 0.5 * (pa[1] + pb[1])]);
        }
    }
    false
}

