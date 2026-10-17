//! Face tessellation for display and as a cross-check of mass properties.
//!
//! * Plane faces: every loop is sampled by chord; the outer loop (the one with positive area
//!   about the outward normal) and any number of holes are triangulated together by hole
//!   bridging, ear clipping and Lawson flips to the constrained Delaunay triangulation (see
//!   [`crate::triangulate`]). Holes may touch the outer loop (or each other) at a vertex, and
//!   collinear runs never give zero-area triangles. Planar meshes are exact: no Steiner points.
//! * Cylinder, cone, sphere and torus faces whose loops trace the boundary of their parameter
//!   rectangle with closed or seam edges (the primitives, in both seam conventions, and loopless
//!   faces): a uniform grid over the rectangle, two triangles per cell, with the pole/apex rows'
//!   zero-area triangles dropped.
//! * Any other cylinder, cone, sphere or torus face (trimmed by lines, arcs, ellipses …): the
//!   loops are mapped to (u, v) by [`walk_loop`], with u mirrored on reversed faces so that the
//!   outer boundary is counter-clockwise and scaled by the surface's radii so that distances are
//!   roughly metric. Pole and apex points are expanded into pole-line segments (the boundary
//!   runs along v = pole in the direction that keeps the face on its left). Loops that wind
//!   round the axis (seamless circles, ellipses) are joined into one polygon by a cut between
//!   the lower and the upper winding component (a loop or a pole line), chosen as the shortest
//!   segment that crosses no boundary and subdivided to the chord; the cut's two copies, one
//!   period apart, share mesh vertices. Loops of winding 0 are the outer loop or holes. The
//!   polygon with holes is triangulated as above, interior points are inserted on the
//!   curvature grid of the face (the same counts as the grid path), and triangles whose centroid
//!   or unconstrained edge midpoints deviate from the surface by more than `chord` get their
//!   centroid inserted, until none does. Supported: one outer loop with holes; one band between
//!   two winding loops, or between a winding loop and a pole/apex, with holes; a sphere minus
//!   holes. Several winding loops on one side, loops winding more than once, and loops winding
//!   in v on a torus give `GeomError::Unsupported`.
//!
//! Edge sampling is shared: an edge is cut into the same number of equal parameter steps by
//! every face that uses it (its own chord count raised to the grid count of the non-planar faces
//! around it), the end samples are the vertex positions and the interior ones `curve.point(t)`
//! with one formula, so neighbouring faces meet at identical points and a closed B-rep gives a
//! closed mesh.
//!
//! Sizing. A grid triangle with legs a (along u) and b (along v) deviates from the surface most
//! at its hypotenuse midpoint, by ≈ a²κ_u/8 + b²κ_v/8. The chord budget is therefore split
//! equally between the curved directions (all of it to u for cylinders and cones, half each for
//! spheres and tori), and each direction gets n = ⌈Δθ / (2 acos(1 − s/ρ))⌉ segments for its
//! largest radius ρ and share s. Edges are sampled with the same count as the grid line they
//! bound (the maximum over the faces around the edge), so a cap and its lateral face share
//! boundary vertices and a primitive's mesh is watertight. Vertices lie on the surface, so a
//! closed mesh is inscribed: its volume is low by O(chord/size) and converges linearly as the
//! chord shrinks. For a sphere of radius r the relative volume error is ≈ (5/3)·chord/r (mean
//! triangle depth ≈ 5/9 of the chord over the latitude–longitude grid; measured 1.64e-3 at
//! chord 0.01 on r = 10, 8.3e-4 at chord 0.005); for a cylinder ≈ (4/3)·chord/r. Seamless
//! faces (closed circles bounding a periodic face, ASM) and seamed faces (OCCT) both work: the
//! parameter rectangle spans one full period from the first wrapping loop's vertex.
//! Normals are the exact outward surface normals at the vertices (at a pole or apex vertex
//! shared by a fan, the mean of the normals of its parameter copies).

use crate::param::{inverse, walk_loop};
use crate::topology::Edge;
use crate::triangulate::{orient, Cdt, P2};
use crate::{Brep, Curve, EdgeId, FaceId, GResult, GeomError, SolidId, Surface, P3, V3};
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, TAU};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f64; 3]>,
    pub normals: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

impl Mesh {
    /// Appends another mesh (indices shifted).
    pub fn append(&mut self, other: &Mesh) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.triangles.extend(other.triangles.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }

    /// Signed enclosed volume Σ p0·(p1 × p2)/6 (divergence theorem over the triangles); positive
    /// for a closed mesh wound counter-clockwise about outward normals.
    pub fn signed_volume(&self) -> f64 {
        let p = |i: u32| V3::from(self.positions[i as usize]);
        self.triangles.iter().map(|t| p(t[0]).dot(&p(t[1]).cross(&p(t[2])))).sum::<f64>() / 6.0
    }

    /// Total triangle area.
    pub fn area(&self) -> f64 {
        let p = |i: u32| V3::from(self.positions[i as usize]);
        self.triangles.iter().map(|t| (p(t[1]) - p(t[0])).cross(&(p(t[2]) - p(t[0]))).norm() / 2.0).sum()
    }
}

/// Segments needed for an arc of radius `rho` spanning `span` radians with sagitta ≤ `s`.
fn arc_segments(rho: f64, span: f64, s: f64) -> usize {
    let span = span.abs();
    let by_chord = if s < rho { (span / (2.0 * (1.0 - s / rho).acos())).ceil() } else { 1.0 };
    (by_chord as usize).max((span / FRAC_PI_2).ceil() as usize).max(1)
}

/// (largest u-radius, v-radius or ∞, curved directions) of an analytic non-planar surface over
/// v ∈ [v0, v1].
fn radii(s: &Surface, v0: f64, v1: f64) -> GResult<(f64, f64, usize)> {
    Ok(match s {
        Surface::Cylinder { radius, .. } => (*radius, f64::INFINITY, 1),
        Surface::Cone { radius, half_angle, .. } => {
            let t = half_angle.tan();
            ((radius + v0 * t).abs().max((radius + v1 * t).abs()), f64::INFINITY, 1)
        }
        Surface::Sphere { radius, .. } => (*radius, *radius, 2),
        Surface::Torus { major, minor, .. } => (major + minor, *minor, 2),
        Surface::Plane { .. } => (f64::INFINITY, f64::INFINITY, 0),
        Surface::Nurbs(_) => return Err(GeomError::Unsupported("tessellation of NURBS faces".into())),
    })
}

/// Parameter rectangle [u0, u1] × [v0, v1] of a face (from its loops' pcurves), and whether the
/// face is exactly that rectangle with grid-aligned edges: no loops, or every non-degenerate edge
/// closed or a seam, starting at a rectangle corner and running along the rectangle's boundary.
fn param_rect_info(brep: &Brep, face: FaceId) -> GResult<([f64; 4], bool)> {
    let f = brep.face(face);
    if f.loops.is_empty() {
        return match f.surface {
            Surface::Sphere { .. } => Ok(([0.0, TAU, -FRAC_PI_2, FRAC_PI_2], true)),
            Surface::Torus { .. } => Ok(([0.0, TAU, 0.0, TAU], true)),
            _ => Err(GeomError::InvalidInput(format!("face F{} has no loops but its surface is not closed", face.0))),
        };
    }
    let sampler = |e: &Edge| Ok((0..=32).map(|k| (e.t0 + (e.t1 - e.t0) * k as f64 / 32.0, 0.0)).collect());
    let walks = f.loops.iter().map(|&lp| walk_loop(brep, face, lp, &sampler)).collect::<GResult<Vec<_>>>()?;
    let terms = crate::mass::wrap_terms_indexed(face, f.reversed, &f.surface, &walks)?;
    let (pu, pv) = f.surface.periods();
    let mut r = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
    let mut take = |u: f64, v: f64| r = [r[0].min(u), r[1].max(u), r[2].min(v), r[3].max(v)];
    // per non-degenerate track: (closed or seam, samples lifted like the rectangle)
    let mut tracks: Vec<(bool, Vec<[f64; 2]>)> = Vec::new();
    let uses = |e: EdgeId| brep.face_coedges(face).filter(|&c| brep.coedge(c).edge == e).count();
    for (i, w) in walks.iter().enumerate() {
        // v shift given to this loop by the virtual-seam lift (torus bands)
        let dv = terms.iter().find(|t| t.2 == Some(i)).zip(w.start).map_or(0.0, |(t, p)| t.1 - p[1]);
        for tr in &w.tracks {
            let eid = brep.coedge(tr.coedge).edge;
            let e = brep.edge(eid);
            let mut pts = Vec::with_capacity(tr.samples.len());
            for s in &tr.samples {
                take(s.uv[0], s.uv[1] + dv);
                pts.push([s.uv[0], s.uv[1] + dv]);
            }
            if !tr.degenerate {
                tracks.push((e.start == e.end || uses(eid) >= 2, pts));
            }
        }
    }
    for t in &terms {
        // pole / apex / lifted band edge
        r[2] = r[2].min(t.1);
        r[3] = r[3].max(t.1);
    }
    // seamless loops: the face spans one full period, starting at the first such loop's start
    if let (Some(per), Some(i)) = (pu, terms.iter().find_map(|t| t.2)) {
        let u0 = walks[i].start.map_or(0.0, |p| p[0]);
        r[0] = u0;
        r[1] = u0 + per;
    }
    if let (Some(per), Some(w)) = (pv, walks.iter().find(|w| w.winding[1] != 0)) {
        let v0 = w.start.map_or(0.0, |p| p[1]);
        r[2] = v0;
        r[3] = v0 + per;
    }
    if !r.iter().all(|x| x.is_finite()) {
        return Err(GeomError::InvalidInput(format!("face F{}: loops give no parameter range", face.0)));
    }
    let near = |x: f64, y: f64, per: Option<f64>| {
        let d = x - y;
        let d = per.map_or(d, |p| d - p * (d / p).round());
        d.abs() <= 1e-9 * (1.0 + x.abs().max(y.abs()))
    };
    let on_u = |u: f64| near(u, r[0], pu) || near(u, r[1], pu);
    let on_v = |v: f64| near(v, r[2], pv) || near(v, r[3], pv);
    let aligned = tracks.iter().all(|(closed_or_seam, pts)| {
        *closed_or_seam
            && pts.first().is_none_or(|p| on_u(p[0]) && on_v(p[1]))
            && pts.iter().all(|p| on_u(p[0]) || on_v(p[1]))
    });
    Ok((r, aligned))
}

/// Parameter rectangle [u0, u1] × [v0, v1] of a face (from its loops' pcurves).
fn param_rect(brep: &Brep, face: FaceId) -> GResult<[f64; 4]> {
    Ok(param_rect_info(brep, face)?.0)
}

/// Grid counts (nu, nv) from curvature alone.
fn curvature_counts(s: &Surface, rect: &[f64; 4], chord: f64) -> GResult<(usize, usize)> {
    let (ru, rv, curved) = radii(s, rect[2], rect[3])?;
    let share = chord / curved.max(1) as f64;
    let nu = if ru.is_finite() { arc_segments(ru, rect[1] - rect[0], share) } else { 1 };
    let nv = if rv.is_finite() { arc_segments(rv, rect[3] - rect[2], share) } else { 1 };
    Ok((nu, nv))
}

/// Does the edge run along u (true) or v (false) on this surface?
fn runs_along_u(s: &Surface, e: &Edge) -> bool {
    let Some(c) = &e.curve else { return true };
    let t = 0.5 * (e.t0 + e.t1);
    match inverse(s, &c.point(t)) {
        Some((u, v)) => {
            let d = crate::param::uv_rates(s, u.unwrap_or(0.0), v, &c.d1(t));
            d[0].abs() >= d[1].abs()
        }
        None => true,
    }
}

/// Segments used to sample an edge: its own chord count, raised to the grid count of every
/// non-planar face it bounds (so shared boundaries match).
fn edge_segments(brep: &Brep, eid: EdgeId, chord: f64) -> GResult<usize> {
    let e = brep.edge(eid);
    let own = match &e.curve {
        None | Some(Curve::Line { .. }) => 1,
        Some(Curve::Circle { radius, .. }) => arc_segments(*radius, e.t1 - e.t0, chord),
        Some(Curve::Ellipse { major, minor, .. }) => arc_segments(minor * minor / major, e.t1 - e.t0, chord),
        Some(Curve::Nurbs(_)) => return Err(GeomError::Unsupported("tessellation of NURBS edges".into())),
    };
    let mut n = own;
    for f in brep.edge_faces(eid) {
        let s = &brep.face(f).surface;
        if matches!(s, Surface::Plane { .. }) {
            continue;
        }
        // a face whose rectangle cannot be derived contributes nothing (both neighbours of the
        // edge still agree, since this count depends on the edge alone)
        let Ok(rect) = param_rect(brep, f) else { continue };
        let (nu, nv) = curvature_counts(s, &rect, chord)?;
        n = n.max(if runs_along_u(s, e) { nu } else { nv });
    }
    Ok(n)
}

/// Mesh-vertex identity: B-rep vertex, interior edge sample, pole/apex, or face-interior point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum VKey {
    Vertex(u32),
    Sample(u32, usize),
    Pole(i8),
    Free(usize),
}

#[derive(Default)]
struct Verts {
    map: HashMap<VKey, usize>,
    pos: Vec<P3>,
}

impl Verts {
    fn get(&mut self, k: VKey, p: P3) -> usize {
        let pos = &mut self.pos;
        *self.map.entry(k).or_insert_with(|| {
            pos.push(p);
            pos.len() - 1
        })
    }

    fn fresh(&mut self, p: P3) -> usize {
        let k = VKey::Free(self.pos.len());
        self.get(k, p)
    }
}

/// Sample `k` of `n` along an edge: the vertex points at the ends, `curve.point(t_k)` with
/// t_k = t0 + (t1 − t0)·k/n inside. Every face sampling the edge gets bit-identical points.
fn edge_sample(brep: &Brep, eid: EdgeId, k: usize, n: usize) -> (VKey, P3) {
    let e = brep.edge(eid);
    if k == 0 {
        return (VKey::Vertex(e.start.0), brep.vertex(e.start).point);
    }
    if k == n {
        return (VKey::Vertex(e.end.0), brep.vertex(e.end).point);
    }
    let t = e.t0 + (e.t1 - e.t0) * k as f64 / n as f64;
    let p = e.curve.as_ref().map_or(brep.vertex(e.start).point, |c| c.point(t));
    (VKey::Sample(eid.0, k), p)
}

/// Triangulates one face so that no triangle deviates from the surface by more than `chord`.
/// Triangles are wound counter-clockwise about the outward normal.
pub fn tessellate_face(brep: &Brep, face: FaceId, chord: f64) -> GResult<Mesh> {
    if !(chord.is_finite() && chord > 0.0) {
        return Err(GeomError::InvalidInput(format!("chord must be > 0, got {chord}")));
    }
    let f = brep.face(face);
    let sign = if f.reversed { -1.0 } else { 1.0 };
    match &f.surface {
        Surface::Plane { frame } => plane_face(brep, face, frame.z * sign, chord),
        Surface::Nurbs(_) => Err(GeomError::Unsupported("tessellation of NURBS faces".into())),
        s => {
            if param_rect_info(brep, face)?.1 {
                grid_face(brep, face, s, sign, chord)
            } else {
                trimmed_face(brep, face, s, chord)
            }
        }
    }
}

/// Tessellates every face of a solid into one mesh.
pub fn tessellate_solid(brep: &Brep, solid: SolidId, chord: f64) -> GResult<Mesh> {
    let mut m = Mesh::default();
    for f in brep.solid_faces(solid) {
        m.append(&tessellate_face(brep, f, chord)?);
    }
    Ok(m)
}

fn grid_face(brep: &Brep, face: FaceId, s: &Surface, sign: f64, chord: f64) -> GResult<Mesh> {
    let rect = param_rect(brep, face)?;
    let (mut nu, mut nv) = curvature_counts(s, &rect, chord)?;
    for e in brep.face_edges(face) {
        let n = edge_segments(brep, e, chord)?;
        if runs_along_u(s, brep.edge(e)) {
            nu = nu.max(n);
        } else {
            nv = nv.max(n);
        }
    }
    let (du, dv) = ((rect[1] - rect[0]) / nu as f64, (rect[3] - rect[2]) / nv as f64);
    let mut m = Mesh::default();
    let vmid = 0.5 * (rect[2] + rect[3]);
    for j in 0..=nv {
        let v = if j == nv { rect[3] } else { rect[2] + dv * j as f64 };
        for i in 0..=nu {
            let u = if i == nu { rect[1] } else { rect[0] + du * i as f64 };
            let n = s.normal(u, v).or_else(|| s.normal(u, v + (vmid - v) * 1e-9)).unwrap_or_else(V3::zeros) * sign;
            m.positions.push(s.point(u, v).into());
            m.normals.push(n.into());
        }
    }
    let id = |i: usize, j: usize| (j * (nu + 1) + i) as u32;
    for j in 0..nv {
        for i in 0..nu {
            let (a, b, c, d) = (id(i, j), id(i + 1, j), id(i + 1, j + 1), id(i, j + 1));
            for t in [[a, b, c], [a, c, d]] {
                push_tri(&mut m, t, sign < 0.0);
            }
        }
    }
    Ok(m)
}

/// Adds a triangle unless it has (relatively) zero area; `flip` reverses the winding.
fn push_tri(m: &mut Mesh, t: [u32; 3], flip: bool) {
    let p = |i: u32| V3::from(m.positions[i as usize]);
    let (e1, e2) = (p(t[1]) - p(t[0]), p(t[2]) - p(t[0]));
    let scale = e1.norm_squared().max(e2.norm_squared());
    if e1.cross(&e2).norm() <= 1e-12 * scale {
        return;
    }
    m.triangles.push(if flip { [t[0], t[2], t[1]] } else { t });
}

/// Twice the signed area of a closed 2D ring.
fn ring_area2(p: &[P2]) -> f64 {
    (0..p.len()).map(|i| {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        a[0] * b[1] - b[0] * a[1]
    }).sum()
}

/// Drops consecutive repeats (cyclically) from a ring of indices.
fn dedup_ring(r: &mut Vec<usize>) {
    r.dedup();
    while r.len() > 1 && r.first() == r.last() {
        r.pop();
    }
}

fn plane_face(brep: &Brep, face: FaceId, normal: V3, chord: f64) -> GResult<Mesh> {
    let f = brep.face(face);
    let Surface::Plane { frame } = &f.surface else { unreachable!() };
    let mut vb = Verts::default();
    let mut rings: Vec<Vec<usize>> = Vec::new();
    for &lp in &f.loops {
        let mut ring = Vec::new();
        for &c in &brep.lp(lp).coedges {
            let ce = brep.coedge(c);
            if brep.is_degenerate(ce.edge) {
                continue;
            }
            let n = edge_segments(brep, ce.edge, chord)?;
            for j in 0..n {
                let k = if ce.reversed { n - j } else { j };
                let (key, p) = edge_sample(brep, ce.edge, k, n);
                ring.push(vb.get(key, p));
            }
        }
        dedup_ring(&mut ring);
        if ring.len() >= 3 {
            rings.push(ring);
        }
    }
    // 2D in a frame whose normal is the outward normal, so the outer loop is counter-clockwise
    let x = frame.x;
    let y = normal.cross(&x);
    let pts: Vec<P2> = vb.pos.iter().map(|p| [(p - frame.origin).dot(&x), (p - frame.origin).dot(&y)]).collect();
    let areas: Vec<f64> = rings.iter().map(|r| ring_area2(&r.iter().map(|&i| pts[i]).collect::<Vec<_>>())).collect();
    let mut m = Mesh { positions: vb.pos.iter().map(|p| (*p).into()).collect(), ..Default::default() };
    m.normals = vec![normal.into(); vb.pos.len()];
    let Some(oi) = (0..rings.len()).filter(|&i| areas[i] > 0.0).max_by(|&a, &b| areas[a].total_cmp(&areas[b])) else {
        return if rings.is_empty() {
            Ok(m)
        } else {
            Err(GeomError::InvalidInput(format!("plane face F{}: no loop is counter-clockwise about the outward normal", face.0)))
        };
    };
    if (0..rings.len()).any(|i| i != oi && areas[i] > 0.0) {
        return Err(GeomError::Unsupported(format!("plane face F{} with more than one outer loop", face.0)));
    }
    let holes: Vec<Vec<usize>> = (0..rings.len()).filter(|&i| i != oi).map(|i| rings[i].clone()).collect();
    let cdt = Cdt::new(pts, &rings[oi], &holes)?;
    for t in &cdt.tris {
        push_tri(&mut m, [t[0] as u32, t[1] as u32, t[2] as u32], false);
    }
    Ok(m)
}

// ---------------------------------------------------------------------------------------------
// Trimmed curved faces
// ---------------------------------------------------------------------------------------------

/// A boundary point in parameter space: (ũ, v) with ũ = ±u (mirrored on reversed faces), or (x, y)
/// once scaled; its mesh vertex; whether it is a pole/apex point.
#[derive(Debug, Clone, Copy)]
struct BPt {
    p: P2,
    mesh: usize,
    sing: bool,
}

/// A walked loop as a lifted polyline (closing to its first point shifted by `wind` periods).
struct Ring {
    pts: Vec<BPt>,
    wind: i64,
}

/// A cut across a band: (length key, lower end, its index on a lower loop, upper end, its index
/// on an upper loop, period shift of the upper loop's copy).
type CutCandidate = (f64, P2, Option<usize>, P2, Option<usize>, f64);

/// A side of a band: a loop winding once round the axis, or a pole/apex line.
#[derive(Clone, Copy)]
enum Side<'a> {
    Ring(&'a Ring),
    Pole { y: f64, mesh: usize },
}

fn unwrap_near(x: f64, reference: f64, period: f64) -> f64 {
    x + period * ((reference - x) / period).round()
}

/// Do the closed segments (a, c) and (p, q) meet anywhere but at a shared endpoint?
fn segments_meet(a: P2, c: P2, p: P2, q: P2) -> bool {
    let within = |x: P2, s: P2, t: P2| {
        x[0] >= s[0].min(t[0]) && x[0] <= s[0].max(t[0]) && x[1] >= s[1].min(t[1]) && x[1] <= s[1].max(t[1])
    };
    let shared = [p, q].iter().any(|e| *e == a || *e == c);
    let (o1, o2, o3, o4) = (orient(a, c, p), orient(a, c, q), orient(p, q, a), orient(p, q, c));
    if shared {
        // only a collinear overlap counts
        let other = if p == a || p == c { q } else { p };
        return o1 == 0.0 && o2 == 0.0 && other != a && other != c && within(other, a, c);
    }
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }
    (o1 == 0.0 && within(p, a, c)) || (o2 == 0.0 && within(q, a, c)) || (o3 == 0.0 && within(a, p, q)) || (o4 == 0.0 && within(c, p, q))
}

fn point_in_polygon(poly: &[P2], q: P2) -> bool {
    let mut inside = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < a[0] + (q[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]) {
            inside = !inside;
        }
    }
    inside
}

/// The face-specific data of the trimmed-face tessellator.
struct Trim<'a> {
    s: &'a Surface,
    dir: f64,
    su: f64,
    sv: f64,
    chord: f64,
}

impl Trim<'_> {
    fn uv(&self, p: P2) -> (f64, f64) {
        (self.dir * p[0] / self.su, p[1] / self.sv)
    }
    fn point(&self, p: P2) -> P3 {
        let (u, v) = self.uv(p);
        self.s.point(u, v)
    }
    /// Pieces for the straight parameter segment a → b so that each piece's chord stays within
    /// the chord tolerance of the surface curve it stands for.
    fn pieces(&self, a: P2, b: P2) -> usize {
        let lerp = |t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let mut n = 1;
        while n < 4096 {
            let ok = (0..n).all(|k| {
                let (t0, t1) = (k as f64 / n as f64, (k + 1) as f64 / n as f64);
                let mid = self.point(lerp(0.5 * (t0 + t1)));
                (mid.coords - (self.point(lerp(t0)).coords + self.point(lerp(t1)).coords) / 2.0).norm() <= 0.5 * self.chord
            });
            if ok {
                break;
            }
            n *= 2;
        }
        n
    }
}

fn trimmed_face(brep: &Brep, face: FaceId, s: &Surface, chord: f64) -> GResult<Mesh> {
    let f = brep.face(face);
    let sign = if f.reversed { -1.0 } else { 1.0 };
    let dir = sign;
    let unsupported = |what: &str| GeomError::Unsupported(format!("tessellation of {} face F{}: {what}", s.type_name(), face.0));
    let (Some(pu), pv) = s.periods() else { return Err(unsupported("surface not periodic in u")) };
    // shared edge sample counts
    let mut counts: Vec<(EdgeId, usize)> = Vec::new();
    for e in brep.face_edges(face) {
        if !brep.is_degenerate(e) {
            counts.push((e, edge_segments(brep, e, chord)?));
        }
    }
    let sampler = |e: &Edge| -> GResult<Vec<(f64, f64)>> {
        let n = counts.iter().find(|(id, _)| std::ptr::eq(brep.edge(*id), e)).map_or(1, |c| c.1);
        Ok((0..=n).map(|k| (e.t0 + (e.t1 - e.t0) * k as f64 / n as f64, 0.0)).collect())
    };
    let mut vb = Verts::default();
    let mut rings: Vec<Ring> = Vec::new();
    for &lp in &f.loops {
        let w = walk_loop(brep, face, lp, &sampler)?;
        let mut raw: Vec<BPt> = Vec::new();
        for tr in &w.tracks {
            if tr.degenerate || tr.samples.is_empty() {
                continue;
            }
            let ce = brep.coedge(tr.coedge);
            let n = tr.samples.len() - 1;
            for (j, smp) in tr.samples.iter().enumerate() {
                let k = if ce.reversed { n - j } else { j };
                let (key, p) = edge_sample(brep, ce.edge, k, n);
                let mesh = vb.get(key, p);
                let sing = inverse(s, &p).is_some_and(|(u, _)| u.is_none());
                raw.push(BPt { p: [dir * smp.uv[0], smp.uv[1]], mesh, sing });
            }
        }
        if let Some(r) = lift_ring(&raw, pv)? {
            rings.push(r);
        }
    }
    if rings.iter().any(|r| r.pts.len() < 2) {
        return Err(unsupported("degenerate loop"));
    }
    // scale factors from the v range
    let mut vr = (f64::INFINITY, f64::NEG_INFINITY);
    for p in rings.iter().flat_map(|r| &r.pts) {
        vr = (vr.0.min(p.p[1]), vr.1.max(p.p[1]));
    }
    let vr_rings = vr;
    let total: i64 = rings.iter().map(|r| r.wind).sum();
    let poles: Vec<f64> = match s {
        Surface::Sphere { .. } => vec![-FRAC_PI_2, FRAC_PI_2],
        Surface::Cone { radius, half_angle, .. } if half_angle.tan() != 0.0 => vec![-radius / half_angle.tan()],
        _ => vec![],
    };
    let needs_poles = total != 0 || (rings.iter().all(|r| r.wind == 0) && !rings.iter().any(|r| ring_area_lifted(r) > 0.0));
    if needs_poles {
        for &pv0 in &poles {
            vr = (vr.0.min(pv0), vr.1.max(pv0));
        }
    }
    if !(vr.0.is_finite() && vr.1.is_finite()) {
        return Err(unsupported("loops give no parameter range"));
    }
    let (su, sv) = match s {
        Surface::Cylinder { radius, .. } => (*radius, 1.0),
        Surface::Cone { radius, half_angle, .. } => {
            let t = half_angle.tan();
            ((radius + vr.0 * t).abs().max((radius + vr.1 * t).abs()).max(1e-12), (1.0 + t * t).sqrt())
        }
        Surface::Sphere { radius, .. } => (*radius, *radius),
        Surface::Torus { major, minor, .. } => (major + minor, *minor),
        _ => return Err(unsupported("surface type")),
    };
    let tr = Trim { s, dir, su, sv, chord };
    let (px, py) = (su * pu, pv.map(|p| sv * p));
    // to scaled coordinates, each ring's first point in [0, px)
    for r in &mut rings {
        let shift = -px * (su * r.pts[0].p[0] / px).floor();
        for p in &mut r.pts {
            p.p = [su * p.p[0] + shift, sv * p.p[1]];
        }
    }
    if rings.iter().any(|r| r.wind.abs() > 1) {
        return Err(unsupported("a loop winds more than once round the axis"));
    }
    // grid spacing (also used to subdivide pole lines)
    let winding = rings.iter().any(|r| r.wind != 0) || needs_poles;
    let (xr, yr) = {
        let mut xr = (f64::INFINITY, f64::NEG_INFINITY);
        for p in rings.iter().flat_map(|r| &r.pts) {
            xr = (xr.0.min(p.p[0]), xr.1.max(p.p[0]));
        }
        if winding || !xr.0.is_finite() {
            xr = (0.0, px);
        }
        (xr, (sv * vr.0, sv * vr.1))
    };
    let (nu, nv) = curvature_counts(s, &[0.0, (xr.1 - xr.0) / su, vr.0, vr.1], chord)?;
    let (hx, hy) = ((xr.1 - xr.0) / nu as f64, (yr.1 - yr.0) / nv as f64);
    // expand pole points into pole lines
    for r in &mut rings {
        let mut out: Vec<BPt> = Vec::with_capacity(r.pts.len());
        for &q in &r.pts {
            if let Some(&prev) = out.last()
                && prev.sing
                && q.sing
                && prev.mesh == q.mesh
                && (q.p[0] - prev.p[0]).abs() > 1e-12 * px
            {
                let m = ((q.p[0] - prev.p[0]).abs() / hx).ceil().max(1.0) as usize;
                for k in 1..m {
                    let x = prev.p[0] + (q.p[0] - prev.p[0]) * k as f64 / m as f64;
                    out.push(BPt { p: [x, q.p[1]], ..q });
                }
            }
            out.push(q);
        }
        r.pts = out;
    }
    let pole_side = |top: bool, vb: &mut Verts| -> GResult<Side<'static>> {
        let v = match (poles.as_slice(), top) {
            ([a, b], _) => if top { *b } else { *a },
            ([a], _) if (top && *a >= vr_rings.1 - 1e-12) || (!top && *a <= vr_rings.0 + 1e-12) => *a,
            _ => return Err(unsupported("loops wind round the axis but bound no finite region")),
        };
        let mesh = vb.get(VKey::Pole(if top { 1 } else { -1 }), s.point(0.0, v));
        Ok(Side::Pole { y: sv * v, mesh })
    };
    // assemble the outer polygon and the holes
    let mut lower: Vec<Side> = rings.iter().filter(|r| r.wind > 0).map(Side::Ring).collect();
    let mut upper: Vec<Side> = rings.iter().filter(|r| r.wind < 0).map(Side::Ring).collect();
    let zero: Vec<&Ring> = rings.iter().filter(|r| r.wind == 0).collect();
    let positive: Vec<&Ring> = zero.iter().copied().filter(|r| ring_area_lifted(r) > 0.0).collect();
    let mut outer: Vec<BPt>;
    let holes_src: Vec<&Ring>;
    if lower.is_empty() && upper.is_empty() && !positive.is_empty() {
        if positive.len() > 1 {
            return Err(unsupported("more than one outer loop"));
        }
        outer = positive[0].pts.clone();
        holes_src = zero.iter().copied().filter(|r| !std::ptr::eq(*r, positive[0])).collect();
    } else {
        if !positive.is_empty() {
            return Err(unsupported("an outer loop next to loops winding round the axis"));
        }
        if total > 0 || (lower.is_empty() && upper.is_empty()) {
            upper.push(pole_side(true, &mut vb)?);
        }
        if total < 0 || (lower.is_empty() && upper.len() == 1 && matches!(upper[0], Side::Pole { .. })) {
            lower.push(pole_side(false, &mut vb)?);
        }
        if lower.len() != 1 || upper.len() != 1 {
            return Err(unsupported("more than one loop winding round the axis on one side"));
        }
        if let (Some(pyv), Side::Ring(l), Side::Ring(u)) = (py, lower[0], upper[0]) {
            // torus band: lift the upper loop to lie above the lower one within one period
            let mean = |r: &Ring| r.pts.iter().map(|p| p.p[1]).sum::<f64>() / r.pts.len() as f64;
            let k = ((mean(l) - mean(u)) / pyv).floor() + 1.0;
            let shifted = Ring { pts: u.pts.iter().map(|p| BPt { p: [p.p[0], p.p[1] + k * pyv], ..*p }).collect(), wind: u.wind };
            return finish_band(&tr, &mut vb, Side::Ring(l), Side::Ring(&shifted), &zero, px, py, hx, hy, nv, face);
        }
        holes_src = zero.clone();
        return finish_band(&tr, &mut vb, lower[0], upper[0], &holes_src, px, py, hx, hy, nv, face);
    }
    dedup_bpts(&mut outer);
    let holes = place_holes(&outer, &holes_src, px, py);
    triangulate_trimmed(&tr, &mut vb, &outer, &holes, hx, hy, nv)
}

/// Twice the area enclosed by a winding-0 ring in its lifted coordinates.
fn ring_area_lifted(r: &Ring) -> f64 {
    ring_area2(&r.pts.iter().map(|p| p.p).collect::<Vec<_>>())
}

fn dedup_bpts(r: &mut Vec<BPt>) {
    let same = |a: &BPt, b: &BPt| a.mesh == b.mesh && a.p == b.p;
    r.dedup_by(|b, a| same(a, b));
    while r.len() > 1 && same(&r[0], &r[r.len() - 1]) {
        r.pop();
    }
}

/// Re-lifts a walked loop: rotates it to start at a regular point; pole/apex points take the u
/// of the point before them (the boundary arrives along a meridian), and on leaving a pole the
/// step along the pole line is the one that keeps the face on the left (−ũ at a top pole, +ũ at a
/// bottom one; the walk's own step is kept when it already complies, as for degenerate edges).
fn lift_ring(raw: &[BPt], pv: Option<f64>) -> GResult<Option<Ring>> {
    let Some(i0) = raw.iter().position(|p| !p.sing) else { return Ok(None) };
    let seq: Vec<(BPt, bool)> = raw[i0..].iter().map(|&p| (p, false)).chain(raw[..=i0].iter().map(|&p| (p, true))).collect();
    let mut out: Vec<BPt> = vec![seq[0].0];
    let mut off = 0.0;
    let eps = 1e-9 * TAU;
    for &(q0, wrapped) in &seq[1..] {
        let mut q = q0;
        let prev = *out.last().expect("non-empty");
        let wx = q.p[0] + off;
        if let (Some(per), true) = (pv, wrapped) {
            q.p[1] = unwrap_near(q.p[1], prev.p[1], per);
        }
        if q.sing {
            q.p[0] = if prev.sing && !wrapped { wx } else { prev.p[0] };
        } else if prev.sing {
            let top = prev.p[1] > q.p[1];
            let d = if wrapped { q.p[0] - prev.p[0] } else { wx - prev.p[0] };
            let ok = d.abs() <= TAU + eps && if top { d <= eps } else { d >= -eps };
            let d2 = if ok {
                d
            } else {
                let r = d.rem_euclid(TAU);
                if top {
                    if r > eps { r - TAU } else { 0.0 }
                } else if r > TAU - eps {
                    0.0
                } else {
                    r
                }
            };
            if !wrapped {
                off += d2 - d;
            }
            q.p[0] = prev.p[0] + d2;
            if d2.abs() > eps {
                // the boundary runs along the pole line, then leaves it along a meridian
                out.push(BPt { p: [q.p[0], prev.p[1]], ..prev });
            }
        } else if wrapped {
            q.p[0] = unwrap_near(q.p[0], prev.p[0], TAU);
        } else {
            q.p[0] = wx;
        }
        out.push(q);
    }
    let same = |a: &BPt, b: &BPt| a.mesh == b.mesh && (a.p[0] - b.p[0]).abs() <= eps && (a.p[1] - b.p[1]).abs() <= 1e-12 * (1.0 + a.p[1].abs());
    out.dedup_by(|b, a| same(a, b));
    if out.len() < 2 {
        return Ok(None);
    }
    let close = out.pop().expect("closing point");
    if close.mesh != out[0].mesh {
        return Err(GeomError::Numerical("trimmed face: loop does not close".into()));
    }
    let wind = ((close.p[0] - out[0].p[0]) / TAU).round() as i64;
    if let Some(per) = pv
        && ((close.p[1] - out[0].p[1]) / per).round() != 0.0
    {
        return Err(GeomError::Unsupported("tessellation of a torus face with a loop winding in v".into()));
    }
    Ok(Some(Ring { pts: out, wind }))
}

/// Holes shifted by whole periods to lie inside `outer`.
fn place_holes(outer: &[BPt], holes: &[&Ring], px: f64, py: Option<f64>) -> Vec<Vec<BPt>> {
    let poly: Vec<P2> = outer.iter().map(|p| p.p).collect();
    let kys: Vec<f64> = if py.is_some() { vec![-1.0, 0.0, 1.0, -2.0, 2.0] } else { vec![0.0] };
    holes
        .iter()
        .map(|h| {
            let mut best = (0usize, 0.0, 0.0);
            for kx in [0.0, -1.0, 1.0, -2.0, 2.0] {
                for &ky in &kys {
                    let d = [kx * px, ky * py.unwrap_or(0.0)];
                    let n = h.pts.iter().filter(|p| point_in_polygon(&poly, [p.p[0] + d[0], p.p[1] + d[1]])).count();
                    if n > best.0 {
                        best = (n, d[0], d[1]);
                    }
                }
            }
            h.pts.iter().map(|p| BPt { p: [p.p[0] + best.1, p.p[1] + best.2], ..*p }).collect()
        })
        .collect()
}

/// Builds the polygon of a band between a lower side (travelling +x) and an upper side
/// (travelling −x), cut open along the shortest admissible segment, and triangulates it.
#[allow(clippy::too_many_arguments)]
fn finish_band(
    tr: &Trim,
    vb: &mut Verts,
    lower: Side,
    upper: Side,
    zero: &[&Ring],
    px: f64,
    py: Option<f64>,
    hx: f64,
    hy: f64,
    nv: usize,
    face: FaceId,
) -> GResult<Mesh> {
    // segments that a cut must not meet (every ring, in neighbouring period copies)
    let mut segs: Vec<(P2, P2)> = Vec::new();
    let mut add_ring = |r: &Ring| {
        let n = r.pts.len();
        for i in 0..n {
            let a = r.pts[i].p;
            let b = if i + 1 < n { r.pts[i + 1].p } else { [r.pts[0].p[0] + r.wind as f64 * px, r.pts[0].p[1]] };
            for k in -2..=2 {
                let d = k as f64 * px;
                segs.push(([a[0] + d, a[1]], [b[0] + d, b[1]]));
            }
        }
    };
    for side in [lower, upper] {
        if let Side::Ring(r) = side {
            add_ring(r);
        }
    }
    for r in zero {
        add_ring(r);
    }
    let lifted = |r: &Ring, i: isize| -> P2 {
        let n = r.pts.len() as isize;
        let k = i.rem_euclid(n);
        let laps = (i - k) / n;
        let p = r.pts[k as usize].p;
        [p[0] + (laps * r.wind as isize) as f64 * px, p[1]]
    };
    let wedge = |r: &Ring, i: usize, shift: f64, q: P2| {
        let at = |j: isize| {
            let p = lifted(r, j);
            [p[0] + shift, p[1]]
        };
        let (pr, cu, nx) = (at(i as isize - 1), at(i as isize), at(i as isize + 1));
        let (l, rr) = (orient(pr, cu, q) > 0.0, orient(cu, nx, q) > 0.0);
        if orient(pr, cu, nx) > 0.0 { l && rr } else { l || rr }
    };
    let clear = |a: P2, c: P2| !segs.iter().any(|&(p, q)| segments_meet(a, c, p, q));
    // candidates (a, lower index, c, upper index, upper shift), shortest first
    let mut cands: Vec<CutCandidate> = Vec::new();
    match (lower, upper) {
        (Side::Ring(l), Side::Ring(u)) => {
            for (i, a) in l.pts.iter().enumerate() {
                // the three nearest copies of upper points for each lower point
                let mut best: Vec<CutCandidate> = Vec::with_capacity(4);
                for (j, c) in u.pts.iter().enumerate() {
                    let k = ((a.p[0] - c.p[0]) / px).round();
                    let cp = [c.p[0] + k * px, c.p[1]];
                    let d2 = (cp[0] - a.p[0]).powi(2) + (cp[1] - a.p[1]).powi(2);
                    if best.len() < 3 || d2 < best[best.len() - 1].0 {
                        best.push((d2, a.p, Some(i), cp, Some(j), k * px));
                        best.sort_by(|x, y| x.0.total_cmp(&y.0));
                        best.truncate(3);
                    }
                }
                cands.extend(best);
            }
        }
        (Side::Ring(l), Side::Pole { y, .. }) => {
            for (i, a) in l.pts.iter().enumerate() {
                cands.push(((y - a.p[1]).abs(), a.p, Some(i), [a.p[0], y], None, 0.0));
            }
        }
        (Side::Pole { y, .. }, Side::Ring(u)) => {
            for (j, c) in u.pts.iter().enumerate() {
                cands.push(((c.p[1] - y).abs(), [c.p[0], y], None, c.p, Some(j), 0.0));
            }
        }
        (Side::Pole { y: y0, .. }, Side::Pole { y: y1, .. }) => {
            // a full meridian through the middle of the widest gap between the holes' x ranges
            let mut iv: Vec<(f64, f64)> = zero
                .iter()
                .map(|r| {
                    let (lo, hi) = r.pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.p[0]), hi.max(p.p[0])));
                    let s = px * (lo / px).floor();
                    (lo - s, hi - s)
                })
                .collect();
            iv.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut xs: Vec<(f64, f64)> = Vec::new(); // (−gap, x)
            if iv.is_empty() {
                xs.push((0.0, 0.0));
            }
            let mut reach = iv.first().map_or(0.0, |v| v.1);
            for w in 1..=iv.len() {
                let next = if w < iv.len() { iv[w].0 } else { iv[0].0 + px };
                if next > reach {
                    xs.push((reach - next, 0.5 * (reach + next)));
                }
                if w < iv.len() {
                    reach = reach.max(iv[w].1);
                }
            }
            for k in 0..16 {
                xs.push((0.0, px * k as f64 / 16.0));
            }
            for (g, x) in xs {
                cands.push((g, [x, y0], None, [x, y1], None, 0.0));
            }
        }
    }
    cands.sort_by(|a, b| a.0.total_cmp(&b.0));
    let ok = |cand: &CutCandidate| {
        let (_, a, li, c, uj, sh) = *cand;
        if let (Side::Ring(l), Some(i)) = (lower, li)
            && !wedge(l, i, 0.0, c)
        {
            return false;
        }
        if let (Side::Ring(u), Some(j)) = (upper, uj)
            && !wedge(u, j, sh, a)
        {
            return false;
        }
        clear(a, c)
    };
    let Some(&(_, a, li, c, uj, sh)) = cands.iter().find(|c| ok(c)) else {
        return Err(GeomError::Numerical(format!("trimmed face F{}: no admissible cut between its boundary loops", face.0)));
    };
    // the cut's interior points, shared by its two copies
    let m = tr.pieces(a, c);
    let cut: Vec<BPt> = (1..m)
        .map(|k| {
            let p = [a[0] + (c[0] - a[0]) * k as f64 / m as f64, a[1] + (c[1] - a[1]) * k as f64 / m as f64];
            BPt { p, mesh: vb.fresh(tr.point(p)), sing: false }
        })
        .collect();
    let pole_line = |x0: f64, x1: f64, y: f64, mesh: usize| -> Vec<BPt> {
        let n = ((x1 - x0).abs() / hx).ceil().max(1.0) as usize;
        (0..=n).map(|k| BPt { p: [x0 + (x1 - x0) * k as f64 / n as f64, y], mesh, sing: true }).collect()
    };
    let ring_from = |r: &Ring, start: usize, shift: f64| -> Vec<BPt> {
        (0..=r.pts.len())
            .map(|k| {
                let p = lifted(r, (start + k) as isize);
                BPt { p: [p[0] + shift, p[1]], ..r.pts[(start + k) % r.pts.len()] }
            })
            .collect()
    };
    let mut outer: Vec<BPt> = match (lower, li) {
        (Side::Ring(l), Some(i)) => ring_from(l, i, 0.0),
        (Side::Pole { y, mesh }, _) => pole_line(a[0], a[0] + px, y, mesh),
        _ => unreachable!(),
    };
    outer.extend(cut.iter().map(|q| BPt { p: [q.p[0] + px, q.p[1]], ..*q }));
    outer.extend(match (upper, uj) {
        (Side::Ring(u), Some(j)) => ring_from(u, j, sh + px),
        (Side::Pole { y, mesh }, _) => pole_line(c[0] + px, c[0], y, mesh),
        _ => unreachable!(),
    });
    outer.extend(cut.iter().rev().copied());
    dedup_bpts(&mut outer);
    let holes = place_holes(&outer, zero, px, py);
    triangulate_trimmed(tr, vb, &outer, &holes, hx, hy, nv)
}

/// Triangulates the scaled parameter polygon, adds interior grid points, refines to the chord and
/// maps the result onto the surface.
fn triangulate_trimmed(tr: &Trim, vb: &mut Verts, outer: &[BPt], holes: &[Vec<BPt>], hx: f64, hy: f64, nv: usize) -> GResult<Mesh> {
    // 2D points, shared when mesh vertex and coordinates agree
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in outer {
        lo = [lo[0].min(p.p[0]), lo[1].min(p.p[1])];
        hi = [hi[0].max(p.p[0]), hi[1].max(p.p[1])];
    }
    let scale = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-300);
    let q = 1e-10 * scale;
    let mut ids: HashMap<(usize, i64, i64), usize> = HashMap::new();
    let mut pts: Vec<P2> = Vec::new();
    let mut mesh_of: Vec<usize> = Vec::new();
    let mut index = |b: &BPt, pts: &mut Vec<P2>, mesh_of: &mut Vec<usize>| -> usize {
        let key = (b.mesh, (b.p[0] / q).round() as i64, (b.p[1] / q).round() as i64);
        *ids.entry(key).or_insert_with(|| {
            pts.push(b.p);
            mesh_of.push(b.mesh);
            pts.len() - 1
        })
    };
    let mut ring_ids = |r: &[BPt], pts: &mut Vec<P2>, mesh_of: &mut Vec<usize>| -> Vec<usize> {
        let mut v: Vec<usize> = r.iter().map(|b| index(b, pts, mesh_of)).collect();
        dedup_ring(&mut v);
        v
    };
    let outer_ids = ring_ids(outer, &mut pts, &mut mesh_of);
    let mut hole_ids: Vec<Vec<usize>> = Vec::new();
    for h in holes {
        let mut v = ring_ids(h, &mut pts, &mut mesh_of);
        if v.len() >= 3 {
            let area = ring_area2(&v.iter().map(|&i| pts[i]).collect::<Vec<_>>());
            if area > 0.0 {
                v.reverse();
            }
            hole_ids.push(v);
        }
    }
    let n_boundary = pts.len();
    let mut cdt = Cdt::new(pts, &outer_ids, &hole_ids)?;
    // constrained segments, for keeping interior points away from the boundary
    let mut fixed: Vec<(P2, P2)> = Vec::new();
    for r in std::iter::once(&outer_ids).chain(hole_ids.iter()) {
        for i in 0..r.len() {
            fixed.push((cdt.pts[r[i]], cdt.pts[r[(i + 1) % r.len()]]));
        }
    }
    let seg_dist = |p: P2, a: P2, b: P2| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dy * dy;
        let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
        ((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt()
    };
    // interior points on the curvature grid (rows only where v is curved, as in the grid path)
    let h = if nv > 1 { hx.min(hy) } else { hx };
    let gap = 0.35 * h;
    let nx = ((hi[0] - lo[0]) / hx).round().max(1.0) as usize;
    if nv > 1 {
        let ny = ((hi[1] - lo[1]) / hy).round().max(1.0) as usize;
        let (dx, dy) = ((hi[0] - lo[0]) / nx as f64, (hi[1] - lo[1]) / ny as f64);
        for j in 1..ny {
            for i in 1..nx {
                let p = [lo[0] + dx * i as f64, lo[1] + dy * j as f64];
                if fixed.iter().any(|&(a, b)| {
                    p[0] >= a[0].min(b[0]) - gap && p[0] <= a[0].max(b[0]) + gap && p[1] >= a[1].min(b[1]) - gap && p[1] <= a[1].max(b[1]) + gap && seg_dist(p, a, b) < gap
                }) {
                    continue;
                }
                if cdt.insert(p, gap).is_some() {
                    mesh_of.push(vb.fresh(tr.point(p)));
                }
            }
        }
    }
    // refine where a triangle strays from the surface by more than the chord: the distance to the
    // (analytic) surface from the flat triangle's centroid, its unconstrained edge midpoints and
    // its circumcentre when that lies inside it. For a triangle inscribed in a sphere the deepest
    // point is exactly the circumcentre (the foot of the centre) or the midpoint of an edge.
    // (Not |S(uv centroid) − flat centroid|: next to a boundary edge that compares the chord with
    // the image of the straight parameter segment, which bulges away from the edge curve.)
    let tiny = 1e-9 * scale;
    let dist = |q: V3| crate::param::surface_distance(tr.s, &P3::from(q)).unwrap_or(0.0);
    for _round in 0..40 {
        let pos = |i: usize, vb: &Verts| vb.pos[mesh_of[i]];
        let mut bad: Vec<P2> = Vec::new();
        for t in &cdt.tris {
            let p = |k: usize| cdt.pts[t[k]];
            let c = [(p(0)[0] + p(1)[0] + p(2)[0]) / 3.0, (p(0)[1] + p(1)[1] + p(2)[1]) / 3.0];
            let (a3, b3, c3) = (pos(t[0], vb).coords, pos(t[1], vb).coords, pos(t[2], vb).coords);
            let mut dev = dist((a3 + b3 + c3) / 3.0);
            if let Some(q) = circumcentre_inside(a3, b3, c3) {
                dev = dev.max(dist(q));
            }
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if !cdt.is_fixed(a, b) {
                    dev = dev.max(dist((pos(a, vb).coords + pos(b, vb).coords) / 2.0));
                }
            }
            if dev > tr.chord && orient(p(0), p(1), p(2)) > 0.0 {
                bad.push(c);
            }
        }
        if bad.is_empty() || cdt.pts.len() > 2_000_000 {
            break;
        }
        let before = cdt.pts.len();
        for c in bad {
            if cdt.insert(c, tiny).is_some() {
                mesh_of.push(vb.fresh(tr.point(c)));
            }
        }
        if cdt.pts.len() == before {
            break;
        }
    }
    debug_assert_eq!(mesh_of.len(), cdt.pts.len());
    debug_assert!(n_boundary <= cdt.pts.len());
    // mesh: positions, normals (outward; mean over the parameter copies at a pole or apex)
    let sign = tr.dir;
    let mut m = Mesh { positions: vb.pos.iter().map(|p| (*p).into()).collect(), ..Default::default() };
    let mut nsum = vec![V3::zeros(); vb.pos.len()];
    let mut exact: Vec<Option<V3>> = vec![None; vb.pos.len()];
    let vmid = 0.5 * (lo[1] + hi[1]) / tr.sv;
    for (i, p) in cdt.pts.iter().enumerate() {
        let (u, v) = tr.uv(*p);
        let mi = mesh_of[i];
        match tr.s.normal(u, v) {
            Some(n) => {
                exact[mi].get_or_insert(n * sign);
            }
            None => {
                if let Some(n) = tr.s.normal(u, v + (vmid - v) * 1e-6) {
                    nsum[mi] += n * sign;
                }
            }
        }
    }
    m.normals = (0..vb.pos.len()).map(|i| exact[i].or_else(|| nsum[i].try_normalize(1e-300)).unwrap_or_else(V3::zeros).into()).collect();
    for t in &cdt.tris {
        push_tri(&mut m, [mesh_of[t[0]] as u32, mesh_of[t[1]] as u32, mesh_of[t[2]] as u32], false);
    }
    Ok(m)
}

/// Circumcentre of the triangle (a, b, c) when it lies inside the triangle.
fn circumcentre_inside(a: V3, b: V3, c: V3) -> Option<V3> {
    let (u, v) = (b - a, c - a);
    let w = u.cross(&v);
    let w2 = w.norm_squared();
    if w2 <= 0.0 {
        return None;
    }
    let q = a + (w.cross(&u) * v.norm_squared() - w.cross(&v) * u.norm_squared()) / (2.0 * w2);
    let inside = [(a, b), (b, c), (c, a)].iter().all(|&(p, r)| (r - p).cross(&(q - p)).dot(&w) >= 0.0);
    inside.then_some(q)
}
