//! Edge blends: equal-distance chamfer and constant-radius (rolling-ball) fillet, built by local
//! surgery on a copy of an exact B-rep.
//!
//! Supported configurations (everything else is `GeomError::Unsupported`; nothing is
//! approximated):
//!
//! * **Straight edge between two planar faces**, convex or concave, at any dihedral angle. The
//!   blend face is a plane (chamfer) or a cylinder of radius r tangent to both faces (fillet); the
//!   two faces are trimmed back to new straight edges parallel to the blended edge, set back
//!   `distance` (chamfer) or the tangent distance r·cot(θ/2) (fillet, θ = interior angle) along
//!   each face. Each end vertex must be a corner of exactly three edges and three planar faces,
//!   and is resolved in one of two ways:
//!   - *end face* (only the blended edge is selected at the vertex): the third face gets the
//!     corner cut — its loop gains the blend's end edge (a line for a chamfer, an arc of the
//!     cross-section circle for a fillet) and the two unselected edges at the vertex are
//!     shortened. A fillet's end face must be perpendicular to the edge (an oblique end face would
//!     need an elliptical end edge: unsupported). A chamfer accepts any end face;
//!   - *vertex blend* (all three edges of a convex corner filleted in the same call): the rolling
//!     ball touching the three faces gives a spherical corner patch bounded by three arcs, one per
//!     fillet cylinder; each face's corner moves to the ball's touching point. Two selected edges
//!     at a vertex, chamfered corners and concave or mixed corners are unsupported.
//! * **Closed circular edge between a plane and a coaxial cylinder** (a cylinder's rim, a hole's
//!   rim), convex or concave: the blend face is a cone (chamfer) or a torus (fillet) bounded by
//!   two closed circles, ASM's seamless convention (one vertex each, on the original circle's
//!   parameter start; see [`crate::primitives`]). The plane and the cylinder keep their loops with
//!   the circle replaced. A fillet whose torus would be a spindle torus (fillet radius ≥ the
//!   radius of the torus' centre circle) is unsupported.
//!
//! Topology. A straight blend adds one face, replaces the edge by two trimmed edges, and per end
//! adds one end edge; an end-face corner replaces its vertex by two (the shortened edges' new
//! ends), a vertex blend replaces it by three and adds the sphere face. A circular blend adds one
//! face and two closed circles (replacing one), and one vertex. So on a box, one blended edge
//! gives F7 E15 V10 L7 and four disjoint ones F10 E24 V16 L10; on a cylinder F4 E3 V3 L6.
//!
//! Orientation. A trimmed face keeps its coedge's direction on the new edge; the blend face uses
//! every new edge against its neighbour. End edges run from side 0 to side 1 (side 0 = the face
//! of the edge's first coedge); the blend face uses an end edge forward at the start of side 0's
//! coedge and reversed at its end. Blend surfaces are oriented from geometry: a convex blend
//! (material inside the rolling ball) has its outward normal pointing away from the ball centre,
//! a concave one towards it; `reversed` follows from comparing that with the natural normal.
//!
//! Checks, all before anything is returned: size finite and > 0; every edge exists, is not
//! degenerate; shortened edges keep a positive length and trimmed edges their direction
//! (`InvalidInput`: the blend is too large); every planar face touched by the blend is checked
//! in its plane — no two of its edges cross or touch except at a shared vertex, exactly one loop
//! runs counter-clockwise about the outward normal and contains every hole (`InvalidInput`: the
//! blend runs into another boundary of the face); a cylinder face keeps its other edges off the
//! trimmed band; and the result must pass [`crate::validate`] at the given tolerance
//! (`Numerical` otherwise). Not detected: another part of the solid (a void, a face of another
//! feature) passing through the blend region without touching the boundary of the faces it
//! trims.

use crate::param::inverse;
use crate::topology::Coedge;
use crate::{validate, Brep, CoedgeId, Curve, EdgeId, FaceId, Frame, GResult, GeomError, LoopId, ShellId, Surface, Tolerance, VertexId, P3, V3};
use nalgebra::{Matrix3, Vector2};
use std::collections::{BTreeMap, HashMap};
use std::f64::consts::TAU;

type V2 = Vector2<f64>;

/// Equal-distance chamfer of `edges` (each set back `distance` along both adjacent faces).
pub fn chamfer(brep: &Brep, edges: &[EdgeId], distance: f64, tol: &Tolerance) -> GResult<Brep> {
    blend(brep, edges, distance, Kind::Chamfer, tol)
}

/// Constant-radius (rolling-ball) fillet of `edges`.
pub fn fillet(brep: &Brep, edges: &[EdgeId], radius: f64, tol: &Tolerance) -> GResult<Brep> {
    blend(brep, edges, radius, Kind::Fillet, tol)
}

/// The edge passing within `tol.linear` of the world point `p` (inside its parameter range);
/// the nearest one when several do. Lines and circles only (other curves are skipped).
pub fn edge_at_point(brep: &Brep, p: &P3, tol: &Tolerance) -> Option<EdgeId> {
    let mut best: Option<(f64, EdgeId)> = None;
    for e in brep.edge_ids() {
        let edge = brep.edge(e);
        let Some(curve) = &edge.curve else { continue };
        let ends = (curve.point(edge.t0) - p).norm().min((curve.point(edge.t1) - p).norm());
        let d = match curve {
            Curve::Line { origin, dir } => {
                let t = ((p - origin).dot(dir) / dir.norm_squared()).clamp(edge.t0, edge.t1);
                (curve.point(t) - p).norm()
            }
            Curve::Circle { frame, .. } => {
                let l = frame.to_local(*p);
                let t = edge.t0 + (l.y.atan2(l.x) - edge.t0).rem_euclid(TAU);
                if t <= edge.t1 { (curve.point(t) - p).norm().min(ends) } else { ends }
            }
            _ => continue,
        };
        if d <= tol.linear && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, e));
        }
    }
    best.map(|(_, e)| e)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Chamfer,
    Fillet,
}

impl Kind {
    fn size_name(self) -> &'static str {
        match self {
            Kind::Chamfer => "chamfer distance",
            Kind::Fillet => "fillet radius",
        }
    }
}

fn invalid<T>(m: String) -> GResult<T> {
    Err(GeomError::InvalidInput(m))
}

fn unsupported<T>(m: String) -> GResult<T> {
    Err(GeomError::Unsupported(m))
}

fn numerical<T>(m: String) -> GResult<T> {
    Err(GeomError::Numerical(m))
}

/// Size, kind and tolerance of one blend call.
#[derive(Clone, Copy)]
struct Spec<'a> {
    size: f64,
    kind: Kind,
    tol: &'a Tolerance,
}

impl Spec<'_> {
    fn too_large<T>(&self, why: String) -> GResult<T> {
        invalid(format!("{} {} is too large: {why}", self.kind.size_name(), self.size))
    }
}

fn blend(brep: &Brep, edges: &[EdgeId], size: f64, kind: Kind, tol: &Tolerance) -> GResult<Brep> {
    if !(size.is_finite() && size > 0.0) {
        return invalid(format!("{} must be finite and > 0, got {size}", kind.size_name()));
    }
    if edges.is_empty() {
        return invalid("no edges to blend".into());
    }
    let spec = Spec { size, kind, tol };
    let mut sel: Vec<EdgeId> = Vec::new();
    for &e in edges {
        if e.idx() >= brep.edges.len() {
            return invalid(format!("edge E{} does not exist", e.0));
        }
        if brep.is_degenerate(e) {
            return invalid(format!("edge E{} is degenerate", e.0));
        }
        if brep.is_flagged_nonmanifold(e) {
            return unsupported(format!("blend of the non-manifold edge E{}", e.0));
        }
        if !sel.contains(&e) {
            sel.push(e);
        }
    }
    let mut lines = Vec::new();
    let mut circles = Vec::new();
    for &e in &sel {
        match &brep.edge(e).curve {
            Some(Curve::Line { .. }) => lines.push(analyse_line(brep, e, &spec)?),
            Some(Curve::Circle { .. }) => circles.push(analyse_circle(brep, e, &spec)?),
            Some(c) => return unsupported(format!("blend of a {} edge (E{})", c.type_name(), e.0)),
            None => unreachable!("degenerate edges are rejected above"),
        }
    }
    let mut b = brep.clone();
    let mut planar: Vec<FaceId> = Vec::new();
    if !lines.is_empty() {
        build_lines(&mut b, brep, &lines, &spec, &mut planar)?;
    }
    for c in &circles {
        build_circle(&mut b, c, &mut planar);
    }
    compact(&mut b);
    planar.sort();
    planar.dedup();
    for f in planar {
        check_planar_face(&b, f, &spec)?;
    }
    let report = validate(&b, tol);
    if !report.is_valid() {
        return numerical(format!("blend produced an invalid model: {:?}", report.issues));
    }
    Ok(b)
}

// ---------------------------------------------------------------------------------------------
// Cross-section of an edge: the two faces' outward normals and in-face directions at a point,
// and the blend's touching points (and rolling-ball centre).
// ---------------------------------------------------------------------------------------------

/// One face along a blended edge: the coedge in it, its outward normal `n` and the unit
/// direction `w` from the edge into the face (perpendicular to the edge, material side), at the
/// section point.
#[derive(Debug, Clone, Copy)]
struct Side {
    coedge: CoedgeId,
    face: FaceId,
    n: V3,
    w: V3,
}

fn outward_normal(b: &Brep, f: FaceId, p: &P3) -> GResult<V3> {
    let face = b.face(f);
    let s = &face.surface;
    let Some((u, v)) = inverse(s, p) else { return unsupported(format!("blend next to the NURBS face F{}", f.0)) };
    let Some(n) = s.normal(u.unwrap_or(0.0), v) else { return unsupported(format!("blend through a singular point of face F{}", f.0)) };
    Ok(if face.reversed { -n } else { n })
}

/// Side of coedge `c` at the edge point `p`, where the edge curve runs along the unit `tangent`.
/// The material lies to the left of the coedge seen from the outward normal: w = n × travel.
fn side(b: &Brep, c: CoedgeId, p: &P3, tangent: &V3) -> GResult<Side> {
    let face = b.coedge_face(c);
    let n = outward_normal(b, face, p)?;
    let travel = if b.coedge(c).reversed { -tangent } else { *tangent };
    Ok(Side { coedge: c, face, n, w: n.cross(&travel).normalize() })
}

/// The blend's section at an edge point: touching points on side 0 and side 1, the rolling-ball
/// centre (the edge point for a chamfer) and convexity (material inside the ball).
#[derive(Debug, Clone, Copy)]
struct Section {
    t: [P3; 2],
    center: P3,
    convex: bool,
}

fn section(a: &P3, s: &[Side; 2], spec: &Spec) -> GResult<Section> {
    // w0·n1 < 0: moving into face 0 goes below face 1, so the edge is convex
    let k = s[0].w.dot(&s[1].n);
    if k.abs() <= spec.tol.angular {
        return invalid("the faces meet tangentially along the edge; there is no edge to blend".into());
    }
    let convex = k < 0.0;
    let r = spec.size;
    match spec.kind {
        Kind::Chamfer => Ok(Section { t: [a + s[0].w * r, a + s[1].w * r], center: *a, convex }),
        Kind::Fillet => {
            // touching points a + t·wᵢ with the centre a + t·wᵢ − σ r nᵢ the same for both sides:
            // t (w0 − w1) = σ r (n0 − n1), both vectors lying along the section's bisector normal
            let sigma = if convex { 1.0 } else { -1.0 };
            let dw = s[0].w - s[1].w;
            let setback = sigma * r * (s[0].n - s[1].n).dot(&dw) / dw.norm_squared();
            if !(setback.is_finite() && setback > 0.0) {
                return numerical(format!("fillet setback {setback} is not positive"));
            }
            let t = [a + s[0].w * setback, a + s[1].w * setback];
            let center = t[0] - s[0].n * (sigma * r);
            let other = t[1] - s[1].n * (sigma * r);
            if (center - other).norm() > spec.tol.linear {
                return numerical(format!("fillet centres from the two faces differ by {:e}", (center - other).norm()));
            }
            Ok(Section { t, center, convex })
        }
    }
}

/// `reversed` flag for a blend surface whose outward normal at `p` is `outward`.
fn reversed_for(s: &Surface, p: &P3, outward: &V3) -> GResult<bool> {
    let n = inverse(s, p).and_then(|(u, v)| s.normal(u.unwrap_or(0.0), v));
    match n {
        Some(n) if n.dot(outward).abs() > 0.5 => Ok(n.dot(outward) < 0.0),
        _ => numerical(format!("blend surface normal at {p:?} does not match the expected {outward:?}")),
    }
}

/// A point of the blend surface in the section and the blend face's outward normal there.
fn section_probe(sec: &Section, s: &[Side; 2], spec: &Spec) -> (P3, V3) {
    match spec.kind {
        // the chamfer normal is exactly along n0 + n1 (equal set-backs)
        Kind::Chamfer => (nalgebra::center(&sec.t[0], &sec.t[1]), (s[0].n + s[1].n).normalize()),
        Kind::Fillet => {
            let mid = (nalgebra::center(&sec.t[0], &sec.t[1]) - sec.center).normalize();
            let sigma = if sec.convex { 1.0 } else { -1.0 };
            (sec.center + mid * spec.size, mid * sigma)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Straight edges between planes.
// ---------------------------------------------------------------------------------------------

struct LineBlend {
    edge: EdgeId,
    sides: [Side; 2],
    /// Unit direction of the edge curve.
    dir: V3,
    /// Section at the edge's start vertex; the blend's lines run along `dir` from `sec.t`.
    sec: Section,
    surface: Surface,
    reversed: bool,
    shell: ShellId,
}

fn two_sides(b: &Brep, e: EdgeId) -> GResult<[CoedgeId; 2]> {
    let cs = b.edge_coedges(e);
    if cs.len() != 2 || b.coedge_face(cs[0]) == b.coedge_face(cs[1]) {
        return unsupported(format!("edge E{} is not an ordinary edge between two faces ({} uses)", e.0, cs.len()));
    }
    Ok([cs[0], cs[1]])
}

fn analyse_line(b: &Brep, e: EdgeId, spec: &Spec) -> GResult<LineBlend> {
    let edge = b.edge(e);
    let Some(Curve::Line { dir, .. }) = &edge.curve else { unreachable!("analyse_line of a non-line") };
    let dir = dir.normalize();
    let cs = two_sides(b, e)?;
    for &c in &cs {
        let f = b.coedge_face(c);
        if !matches!(b.face(f).surface, Surface::Plane { .. }) {
            return unsupported(format!("straight edge E{} next to the {} face F{} (only plane–plane)", e.0, b.face(f).surface.type_name(), f.0));
        }
    }
    let a = b.vertex(edge.start).point;
    let sides = [side(b, cs[0], &a, &dir)?, side(b, cs[1], &a, &dir)?];
    let sec = section(&a, &sides, spec)?;
    let surface = match spec.kind {
        Kind::Chamfer => {
            let mut z = dir.cross(&(sec.t[1] - sec.t[0])).normalize();
            if z.dot(&(sides[0].n + sides[1].n)) < 0.0 {
                z = -z;
            }
            Surface::Plane { frame: Frame { origin: sec.t[0], x: dir, y: z.cross(&dir), z } }
        }
        Kind::Fillet => Surface::Cylinder { frame: arc_frame(&sec.center, &sec.t[0], &sec.t[1])?.0, radius: spec.size },
    };
    let (probe, outward) = section_probe(&sec, &sides, spec);
    let reversed = reversed_for(&surface, &probe, &outward)?;
    Ok(LineBlend { edge: e, sides, dir, sec, surface, reversed, shell: b.face(sides[0].face).shell })
}

/// Frame at centre `c` with x towards `a` and `b` at a positive angle in (0, π) from x; returns
/// the frame and that angle.
fn arc_frame(c: &P3, a: &P3, b: &P3) -> GResult<(Frame, f64)> {
    let (da, db) = (a - c, b - c);
    let x = da.normalize();
    let yv = db - x * db.dot(&x);
    if yv.norm() <= 1e-12 * db.norm() {
        return numerical("blend arc of zero or half-turn sweep".into());
    }
    let y = yv.normalize();
    Ok((Frame { origin: *c, x, y, z: x.cross(&y) }, db.dot(&y).atan2(db.dot(&x))))
}

/// Circular arc edge of radius `r` about `c` from vertex `va` to vertex `vb` (sweep < π).
fn arc_edge(b: &mut Brep, c: &P3, r: f64, va: VertexId, vb: VertexId, tol: &Tolerance) -> GResult<EdgeId> {
    let (pa, pb) = (b.vertex(va).point, b.vertex(vb).point);
    for p in [pa, pb] {
        if ((p - c).norm() - r).abs() > tol.linear {
            return numerical(format!("blend arc end {p:?} is {:e} off its circle", ((p - c).norm() - r).abs()));
        }
    }
    let (frame, sweep) = arc_frame(c, &pa, &pb)?;
    Ok(b.add_edge(Some(Curve::Circle { frame, radius: r }), 0.0, sweep, va, vb))
}

/// Edges and faces around a vertex that the straight blends can resolve: three edges, three
/// planar faces.
struct Star {
    edges: Vec<EdgeId>,
    faces: Vec<FaceId>,
}

fn vertex_star(b: &Brep, v: VertexId) -> GResult<Star> {
    let p = b.vertex(v).point;
    let edges: Vec<EdgeId> = b.edge_ids().filter(|&e| b.edge(e).start == v || b.edge(e).end == v).collect();
    if edges.len() != 3 {
        return unsupported(format!("blend ending at the vertex {p:?} of {} edges (only corners of three edges)", edges.len()));
    }
    let mut faces: Vec<FaceId> = Vec::new();
    for &e in &edges {
        if b.is_degenerate(e) {
            return unsupported(format!("blend ending at the vertex {p:?} of a degenerate edge"));
        }
        let cs = b.edge_coedges(e);
        let fs = b.edge_faces(e);
        if cs.len() != 2 || fs.len() != 2 {
            return unsupported(format!("blend ending at the vertex {p:?} next to the non-manifold or seam edge E{}", e.0));
        }
        for f in fs {
            if !faces.contains(&f) {
                faces.push(f);
            }
        }
    }
    if faces.len() != 3 {
        return unsupported(format!("blend ending at the vertex {p:?} of {} faces (only corners of three faces)", faces.len()));
    }
    for &f in &faces {
        if !matches!(b.face(f).surface, Surface::Plane { .. }) {
            return unsupported(format!("blend ending at the vertex {p:?} next to the {} face F{}", b.face(f).surface.type_name(), f.0));
        }
    }
    Ok(Star { edges, faces })
}

fn plane_of(b: &Brep, f: FaceId) -> (Frame, V3) {
    let face = b.face(f);
    let Surface::Plane { frame } = &face.surface else { unreachable!("plane_of a non-plane") };
    (*frame, if face.reversed { -frame.z } else { frame.z })
}

/// New loop element to splice into an end face after the coedge that ended at `vertex`.
struct Insert {
    face: FaceId,
    vertex: VertexId,
    edge: EdgeId,
    reversed: bool,
}

/// Sphere corner patch of a vertex blend.
struct Ball {
    shell: ShellId,
    surface: Surface,
    reversed: bool,
    uses: Vec<(EdgeId, bool)>,
}

/// State of the straight-edge construction: new corner vertices per (old vertex, face), the
/// end edges per (blend, old vertex), endpoint moves of unselected edges, end-face splices and
/// sphere corners.
#[derive(Default)]
struct Corners {
    corner: HashMap<(VertexId, FaceId), VertexId>,
    end_edge: HashMap<(usize, VertexId), EdgeId>,
    moves: Vec<(EdgeId, VertexId, VertexId)>,
    inserts: Vec<Insert>,
    balls: Vec<Ball>,
}

fn build_lines(b: &mut Brep, orig: &Brep, lbs: &[LineBlend], spec: &Spec, planar: &mut Vec<FaceId>) -> GResult<()> {
    let sel: Vec<EdgeId> = lbs.iter().map(|l| l.edge).collect();
    let mut at: BTreeMap<VertexId, Vec<usize>> = BTreeMap::new();
    for (i, lb) in lbs.iter().enumerate() {
        let e = orig.edge(lb.edge);
        at.entry(e.start).or_default().push(i);
        at.entry(e.end).or_default().push(i);
    }
    let mut st = Corners::default();
    for (&v, list) in &at {
        let star = vertex_star(orig, v)?;
        let n_sel = star.edges.iter().filter(|e| sel.contains(e)).count();
        match n_sel {
            1 => end_corner(b, orig, v, list[0], lbs, &star, spec, &mut st)?,
            3 => ball_corner(b, orig, v, list, lbs, &star, spec, &mut st)?,
            n => {
                return unsupported(format!(
                    "{n} blended edges meeting at the vertex {:?} (one, or all three filleted, are supported)",
                    orig.vertex(v).point
                ))
            }
        }
    }
    planar.extend(st.inserts.iter().map(|i| i.face));

    // shorten the unselected edges at end-face corners (a straight edge between two planes)
    let mut moved: Vec<EdgeId> = Vec::new();
    for &(e, old, new) in &st.moves {
        let x = &mut b.edges[e.idx()];
        if x.start == old {
            x.start = new;
        } else if x.end == old {
            x.end = new;
        } else {
            return numerical(format!("edge E{} does not end at V{}", e.0, old.0));
        }
        if !moved.contains(&e) {
            moved.push(e);
        }
    }
    for e in moved {
        let Some(Curve::Line { origin, dir }) = orig.edge(e).curve.clone() else { unreachable!("only line edges are shortened") };
        let dir = dir.normalize();
        let (pa, pb) = (b.vertex(b.edge(e).start).point, b.vertex(b.edge(e).end).point);
        for p in [pa, pb] {
            let off = (p - origin) - dir * (p - origin).dot(&dir);
            if off.norm() > spec.tol.linear {
                return numerical(format!("shortened edge E{} end {p:?} is {:e} off its line", e.0, off.norm()));
            }
        }
        if (pb - pa).dot(&dir) <= spec.tol.linear {
            return spec.too_large(format!("the edge from {:?} to {:?} would vanish", orig.vertex(orig.edge(e).start).point, orig.vertex(orig.edge(e).end).point));
        }
        let x = &mut b.edges[e.idx()];
        x.curve = Some(Curve::line_through(pa, pb));
        x.t0 = 0.0;
        x.t1 = (pb - pa).norm();
    }

    // trimmed edges and blend faces
    for (i, lb) in lbs.iter().enumerate() {
        let mut lines = [EdgeId(0); 2];
        for (s, sd) in lb.sides.iter().enumerate() {
            let c = sd.coedge;
            let (x, y) = (orig.coedge_start(c), orig.coedge_end(c));
            let (va, vb) = (st.corner[&(x, sd.face)], st.corner[&(y, sd.face)]);
            let (pa, pb) = (b.vertex(va).point, b.vertex(vb).point);
            let travel = if orig.coedge(c).reversed { -lb.dir } else { lb.dir };
            if (pb - pa).dot(&travel) <= spec.tol.linear {
                return spec.too_large(format!("the blend of edge E{} is longer than the edge allows", lb.edge.0));
            }
            let e = b.add_line_edge(va, vb);
            let ce = &mut b.coedges[c.idx()];
            ce.edge = e;
            ce.reversed = false;
            lines[s] = e;
            planar.push(sd.face);
        }
        let c0 = lb.sides[0].coedge;
        let (x, y) = (orig.coedge_start(c0), orig.coedge_end(c0));
        let face = b.add_face(lb.shell, lb.surface.clone(), lb.reversed);
        b.add_loop(face, &[(lines[0], true), (st.end_edge[&(i, x)], false), (lines[1], true), (st.end_edge[&(i, y)], true)]);
        if matches!(lb.surface, Surface::Plane { .. }) {
            planar.push(face);
        }
    }

    // splice the end edges into the end faces
    for ins in &st.inserts {
        let found = orig.face(ins.face).loops.iter().find_map(|&lp| {
            orig.lp(lp).coedges.iter().find(|&&c| orig.coedge_end(c) == ins.vertex).map(|&c| (lp, c))
        });
        let Some((lp, after)) = found else { return numerical(format!("end face F{} does not pass V{}", ins.face.0, ins.vertex.0)) };
        splice(b, lp, after, ins.edge, ins.reversed);
    }

    for ball in st.balls {
        let uses = chain(b, &ball.uses)?;
        let face = b.add_face(ball.shell, ball.surface, ball.reversed);
        b.add_loop(face, &uses);
    }
    Ok(())
}

/// Inserts a use of `edge` into loop `lp` right after coedge `after`.
fn splice(b: &mut Brep, lp: LoopId, after: CoedgeId, edge: EdgeId, reversed: bool) {
    let id = CoedgeId(b.coedges.len() as u32);
    b.coedges.push(Coedge { edge, reversed, owner: lp });
    let list = &mut b.loops[lp.idx()].coedges;
    let k = list.iter().position(|&c| c == after).expect("splice point in loop");
    list.insert(k + 1, id);
}

/// Orders edge uses into a closed chain (each use ends where the next starts).
fn chain(b: &Brep, uses: &[(EdgeId, bool)]) -> GResult<Vec<(EdgeId, bool)>> {
    let ends = |&(e, r): &(EdgeId, bool)| {
        let x = b.edge(e);
        if r { (x.end, x.start) } else { (x.start, x.end) }
    };
    let mut rest = uses.to_vec();
    let mut out = vec![rest.remove(0)];
    while !rest.is_empty() {
        let last = ends(out.last().expect("non-empty")).1;
        let Some(k) = rest.iter().position(|u| ends(u).0 == last) else { return numerical("corner patch boundary does not close".into()) };
        out.push(rest.remove(k));
    }
    if ends(&out[0]).0 != ends(out.last().expect("non-empty")).1 {
        return numerical("corner patch boundary does not close".into());
    }
    Ok(out)
}

/// End-face corner at `v` for blend `i` (the only selected edge at `v`).
#[allow(clippy::too_many_arguments)]
fn end_corner(b: &mut Brep, orig: &Brep, v: VertexId, i: usize, lbs: &[LineBlend], star: &Star, spec: &Spec, st: &mut Corners) -> GResult<()> {
    let lb = &lbs[i];
    let f = [lb.sides[0].face, lb.sides[1].face];
    let Some(&f3) = star.faces.iter().find(|x| !f.contains(x)) else { return numerical("no end face".into()) };
    let (p3, n3) = plane_of(orig, f3);
    let dn = lb.dir.dot(&n3);
    if dn.abs() <= 1e-9 {
        return unsupported(format!("end face F{} of edge E{} is parallel to the edge", f3.0, lb.edge.0));
    }
    if spec.kind == Kind::Fillet && lb.dir.cross(&n3).norm() > spec.tol.angular {
        return unsupported(format!(
            "fillet of edge E{} ending on face F{} that is not perpendicular to it (the end edge would be an ellipse)",
            lb.edge.0, f3.0
        ));
    }
    // where a line along the edge from q meets the end plane
    let hit = |q: &P3| q + lb.dir * ((p3.origin - q).dot(&n3) / dn);
    let mut nv = [VertexId(0); 2];
    for s in 0..2 {
        let other = star.edges.iter().copied().find(|&e| e != lb.edge && orig.edge_faces(e).contains(&f[s]));
        let Some(other) = other.filter(|&e| orig.edge_faces(e).contains(&f3)) else {
            return numerical(format!("no edge between faces F{} and F{} at V{}", f[s].0, f3.0, v.0));
        };
        if !matches!(orig.edge(other).curve, Some(Curve::Line { .. })) {
            return unsupported(format!("end edge E{} is not straight", other.0));
        }
        let id = b.add_vertex(hit(&lb.sec.t[s]));
        st.corner.insert((v, f[s]), id);
        st.moves.push((other, v, id));
        nv[s] = id;
    }
    let edge = match spec.kind {
        Kind::Chamfer => b.add_line_edge(nv[0], nv[1]),
        Kind::Fillet => arc_edge(b, &hit(&lb.sec.center), spec.size, nv[0], nv[1], spec.tol)?,
    };
    st.end_edge.insert((i, v), edge);
    // the blend face uses the end edge forward at the start of side 0's coedge: the end face
    // uses it the other way
    let x = orig.coedge_start(lb.sides[0].coedge);
    st.inserts.push(Insert { face: f3, vertex: v, edge, reversed: v == x });
    Ok(())
}

/// Vertex blend at `v`: all three edges filleted (convex) → sphere corner patch.
#[allow(clippy::too_many_arguments)]
fn ball_corner(b: &mut Brep, orig: &Brep, v: VertexId, list: &[usize], lbs: &[LineBlend], star: &Star, spec: &Spec, st: &mut Corners) -> GResult<()> {
    let p = orig.vertex(v).point;
    if spec.kind == Kind::Chamfer {
        return unsupported(format!("chamfer of three edges meeting at the vertex {p:?}"));
    }
    if list.iter().any(|&i| !lbs[i].sec.convex) {
        return unsupported(format!("vertex blend at {p:?} with a concave edge"));
    }
    let r = spec.size;
    let normals: Vec<V3> = star.faces.iter().map(|&f| plane_of(orig, f).1).collect();
    let m = Matrix3::from_rows(&[normals[0].transpose(), normals[1].transpose(), normals[2].transpose()]);
    let Some(inv) = m.try_inverse() else { return unsupported(format!("vertex blend at {p:?}: the three faces have no single corner")) };
    // ball centre: r below each of the three planes
    let centre = p + inv * V3::new(-r, -r, -r);
    for (k, &f) in star.faces.iter().enumerate() {
        let id = b.add_vertex(centre + normals[k] * r);
        st.corner.insert((v, f), id);
    }
    let mut uses = Vec::with_capacity(3);
    for &i in list {
        let lb = &lbs[i];
        let (va, vb) = (st.corner[&(v, lb.sides[0].face)], st.corner[&(v, lb.sides[1].face)]);
        for (s, vid) in [va, vb].into_iter().enumerate() {
            // the touching point must lie on the blend's trimmed line in that face
            let q = b.vertex(vid).point - lb.sec.t[s];
            let off = q - lb.dir * q.dot(&lb.dir);
            if off.norm() > spec.tol.linear {
                return numerical(format!("vertex blend at {p:?}: corner point is {:e} off the trimmed edge", off.norm()));
            }
        }
        let edge = arc_edge(b, &centre, r, va, vb, spec.tol)?;
        st.end_edge.insert((i, v), edge);
        let x = orig.coedge_start(lb.sides[0].coedge);
        uses.push((edge, v == x));
    }
    // sphere frame with both poles off the patch: the patch is the set of directions that are
    // positive combinations of the three face normals, so every point of it has a positive
    // component along their mean; the poles are put perpendicular to the mean
    let mean = (normals[0] + normals[1] + normals[2]).normalize();
    if normals.iter().any(|n| n.dot(&mean) < 0.05) {
        return unsupported(format!("vertex blend at {p:?}: corner too sharp"));
    }
    let base = Frame::from_origin_z(centre, mean);
    let surface = Surface::Sphere { frame: Frame { origin: centre, x: base.y, y: base.z, z: base.x }, radius: r };
    let reversed = reversed_for(&surface, &(centre + mean * r), &mean)?;
    st.balls.push(Ball { shell: orig.face(star.faces[0]).shell, surface, reversed, uses });
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Closed circles between a plane and a coaxial cylinder.
// ---------------------------------------------------------------------------------------------

struct CircleBlend {
    sides: [Side; 2],
    /// Replacement circle per side (same frame orientation and parameter range as the edge).
    circles: [Curve; 2],
    t0: f64,
    t1: f64,
    surface: Surface,
    reversed: bool,
    shell: ShellId,
}

fn analyse_circle(b: &Brep, e: EdgeId, spec: &Spec) -> GResult<CircleBlend> {
    let tol = spec.tol;
    let edge = b.edge(e);
    let curve = edge.curve.clone().expect("circle");
    let Curve::Circle { frame, radius } = curve else { unreachable!("analyse_circle of a non-circle") };
    if edge.start != edge.end || (edge.t1 - edge.t0 - TAU).abs() > 1e-9 {
        return unsupported(format!("blend of the circular arc E{} (only closed circles)", e.0));
    }
    if b.edge_ids().any(|x| x != e && (b.edge(x).start == edge.start || b.edge(x).end == edge.start)) {
        return unsupported(format!("blend of the circle E{} whose vertex other edges share", e.0));
    }
    let cs = two_sides(b, e)?;
    let axis = frame.z;
    let mut cyl = None;
    let mut plane = None;
    for (i, &c) in cs.iter().enumerate() {
        let f = b.coedge_face(c);
        if b.lp(b.coedge(c).owner).coedges.len() != 1 {
            return unsupported(format!("circle E{} shares its loop in face F{} with other edges", e.0, f.0));
        }
        match &b.face(f).surface {
            Surface::Plane { frame: pf } if pf.z.cross(&axis).norm() <= tol.angular && (frame.origin - pf.origin).dot(&pf.z).abs() <= tol.linear => {
                plane = Some(i)
            }
            Surface::Cylinder { frame: cf, radius: cr } => {
                let off = (frame.origin - cf.origin) - cf.z * (frame.origin - cf.origin).dot(&cf.z);
                if cf.z.cross(&axis).norm() > tol.angular || off.norm() > tol.linear || (cr - radius).abs() > tol.linear {
                    return unsupported(format!("circle E{} is not a rim of the cylinder face F{}", e.0, f.0));
                }
                cyl = Some(i);
            }
            s => return unsupported(format!("circle E{} next to the {} face F{} (only plane–cylinder)", e.0, s.type_name(), f.0)),
        }
    }
    let (Some(ci), Some(_)) = (cyl, plane) else { return unsupported(format!("circle E{} is not between a plane and a cylinder", e.0)) };
    let p = curve.point(edge.t0);
    let tangent = curve.d1(edge.t0).normalize();
    let sides = [side(b, cs[0], &p, &tangent)?, side(b, cs[1], &p, &tangent)?];
    let sec = section(&p, &sides, spec)?;
    // replacement circles through the touching points
    let height = |q: &P3| (q - frame.origin).dot(&axis);
    let ring = |q: &P3| {
        let o = frame.origin + axis * height(q);
        (o, (q - o).norm())
    };
    let mut circles = Vec::with_capacity(2);
    for t in &sec.t {
        let (o, rho) = ring(t);
        if rho <= tol.linear {
            return spec.too_large(format!("it consumes the plane face of circle E{}", e.0));
        }
        circles.push(Curve::Circle { frame: Frame { origin: o, ..frame }, radius: rho });
    }
    // the cylinder face's other edges must stay off the band the blend trims away
    let cyl_face = sides[ci].face;
    let h_new = height(&sec.t[ci]);
    let toward = h_new.signum();
    for x in b.face_edges(cyl_face) {
        if x == e {
            continue;
        }
        let ex = b.edge(x);
        let hs: Vec<f64> = match &ex.curve {
            Some(Curve::Line { .. }) | None => vec![height(&b.vertex(ex.start).point), height(&b.vertex(ex.end).point)],
            Some(Curve::Circle { frame: f2, .. }) if f2.z.cross(&axis).norm() <= tol.angular => vec![height(&f2.origin)],
            Some(c) => return unsupported(format!("cylinder face F{} bounded by a {} edge", cyl_face.0, c.type_name())),
        };
        if hs.iter().any(|h| (h - h_new) * toward <= tol.linear) {
            return spec.too_large(format!("it consumes the cylinder face F{}", cyl_face.0));
        }
    }
    let surface = match spec.kind {
        Kind::Fillet => {
            let (o, major) = ring(&sec.center);
            if major <= spec.size + tol.linear {
                return unsupported(format!("fillet of circle E{} would need a spindle torus (centre circle radius {major})", e.0));
            }
            Surface::Torus { frame: Frame { origin: o, ..frame }, major, minor: spec.size }
        }
        Kind::Chamfer => {
            let ((o0, r0), (o1, r1)) = (ring(&sec.t[0]), ring(&sec.t[1]));
            let dv = (o1 - o0).dot(&axis);
            Surface::Cone { frame: Frame { origin: o0, ..frame }, radius: r0, half_angle: ((r1 - r0) / dv).atan() }
        }
    };
    let (probe, outward) = section_probe(&sec, &sides, spec);
    let reversed = reversed_for(&surface, &probe, &outward)?;
    let [c0, c1]: [Curve; 2] = circles.try_into().expect("two circles");
    Ok(CircleBlend { sides, circles: [c0, c1], t0: edge.t0, t1: edge.t1, surface, reversed, shell: b.face(sides[0].face).shell })
}

fn build_circle(b: &mut Brep, cb: &CircleBlend, planar: &mut Vec<FaceId>) {
    let mut uses = [(EdgeId(0), false); 2];
    for (i, sd) in cb.sides.iter().enumerate() {
        let curve = cb.circles[i].clone();
        let v = b.add_vertex(curve.point(cb.t0));
        let e = b.add_edge(Some(curve), cb.t0, cb.t1, v, v);
        let ce = &mut b.coedges[sd.coedge.idx()];
        ce.edge = e;
        uses[i] = (e, !ce.reversed);
        if matches!(b.face(sd.face).surface, Surface::Plane { .. }) {
            planar.push(sd.face);
        }
    }
    let face = b.add_face(cb.shell, cb.surface.clone(), cb.reversed);
    b.add_loop(face, &[uses[0]]);
    b.add_loop(face, &[uses[1]]);
}

// ---------------------------------------------------------------------------------------------
// Removal of the edges and vertices the blends replaced.
// ---------------------------------------------------------------------------------------------

fn compact(b: &mut Brep) {
    let mut used = vec![false; b.edges.len()];
    for c in &b.coedges {
        used[c.edge.idx()] = true;
    }
    let mut emap = vec![None; b.edges.len()];
    let old = std::mem::take(&mut b.edges);
    for (i, e) in old.into_iter().enumerate() {
        if used[i] {
            emap[i] = Some(EdgeId(b.edges.len() as u32));
            b.edges.push(e);
        }
    }
    for c in &mut b.coedges {
        c.edge = emap[c.edge.idx()].expect("used edge");
    }
    b.nonmanifold_edges = b.nonmanifold_edges.iter().filter_map(|e| emap[e.idx()]).collect();
    let mut vused = vec![false; b.vertices.len()];
    for e in &b.edges {
        vused[e.start.idx()] = true;
        vused[e.end.idx()] = true;
    }
    let mut vmap = vec![None; b.vertices.len()];
    let old = std::mem::take(&mut b.vertices);
    for (i, v) in old.into_iter().enumerate() {
        if vused[i] {
            vmap[i] = Some(VertexId(b.vertices.len() as u32));
            b.vertices.push(v);
        }
    }
    for e in &mut b.edges {
        e.start = vmap[e.start.idx()].expect("used vertex");
        e.end = vmap[e.end.idx()].expect("used vertex");
    }
}

// ---------------------------------------------------------------------------------------------
// Planar face check: edges cross only at shared vertices; one outer loop containing the holes.
// ---------------------------------------------------------------------------------------------

/// A line segment or circular arc in the plane of a face (coordinates counter-clockwise about
/// the outward normal), in the direction of its coedge.
#[derive(Debug, Clone, Copy)]
enum Seg2 {
    Line { a: V2, b: V2 },
    /// From angle `a0` through the signed `sweep` (|sweep| ≤ 2π).
    Arc { c: V2, r: f64, a0: f64, sweep: f64 },
}

fn cross2(a: &V2, b: &V2) -> f64 {
    a.x * b.y - a.y * b.x
}

impl Seg2 {
    fn point(&self, s: f64) -> V2 {
        match *self {
            Seg2::Line { a, b } => a + (b - a) * s,
            Seg2::Arc { c, r, a0, sweep } => {
                let t = a0 + sweep * s;
                c + V2::new(t.cos(), t.sin()) * r
            }
        }
    }

    /// ½∮(x dy − y dx) along the piece.
    fn area_term(&self) -> f64 {
        match *self {
            Seg2::Line { a, b } => 0.5 * cross2(&a, &b),
            Seg2::Arc { c, r, sweep, .. } => {
                let (p0, p1) = (self.point(0.0), self.point(1.0));
                0.5 * (cross2(&c, &(p1 - p0)) + r * r * sweep)
            }
        }
    }

    /// For an arc: does the direction of `q` from the centre lie within the swept range (with
    /// a margin of `tol` along the arc)? Lines: always.
    fn spans(&self, q: &V2, tol: f64) -> bool {
        let Seg2::Arc { c, r, a0, sweep } = *self else { return true };
        if sweep.abs() >= TAU - 1e-12 {
            return true;
        }
        let d = q - c;
        // angle from the start in the direction of travel, in [0, 2π)
        let rel = ((d.y.atan2(d.x) - a0) * sweep.signum()).rem_euclid(TAU);
        let eps = tol / r;
        rel <= sweep.abs() + eps || rel >= TAU - eps
    }

    fn dist(&self, p: &V2) -> f64 {
        match *self {
            Seg2::Line { a, b } => {
                let ab = b - a;
                let s = ((p - a).dot(&ab) / ab.norm_squared()).clamp(0.0, 1.0);
                (a + ab * s - p).norm()
            }
            Seg2::Arc { c, r, .. } => {
                let ends = (self.point(0.0) - p).norm().min((self.point(1.0) - p).norm());
                if self.spans(p, 0.0) && (p - c).norm() > 0.0 { ((p - c).norm() - r).abs().min(ends) } else { ends }
            }
        }
    }

    /// Angle swept by the piece as seen from `p` (for winding numbers; p off the piece).
    fn winding_angle(&self, p: &V2) -> f64 {
        let chord = |a: V2, b: V2| {
            let (u, v) = (a - p, b - p);
            cross2(&u, &v).atan2(u.dot(&v))
        };
        match *self {
            Seg2::Line { a, b } => chord(a, b),
            Seg2::Arc { c, r, sweep, .. } => {
                let inside = (p - c).norm() < r;
                if sweep.abs() >= TAU - 1e-12 {
                    return if inside { TAU * sweep.signum() } else { 0.0 };
                }
                let (s, e, m) = (self.point(0.0), self.point(1.0), self.point(0.5));
                // the arc differs from its chord by one turn when p lies between them
                let same_side = cross2(&(e - s), &(p - s)) * cross2(&(e - s), &(m - s)) > 0.0;
                chord(s, e) + if inside && same_side { TAU * sweep.signum() } else { 0.0 }
            }
        }
    }
}

/// Coedge in its face's plane, with its start and end vertices and their plane positions.
struct Piece {
    seg: Seg2,
    v: [VertexId; 2],
    p: [V2; 2],
}

/// Pieces at most `tol` apart anywhere except at the vertices they share.
fn pieces_touch(p: &Piece, q: &Piece, tol: f64) -> bool {
    let mut shared_pts: Vec<V2> = Vec::new();
    for (k, v) in p.v.iter().enumerate() {
        let x = p.p[k];
        if q.v.contains(v) && !shared_pts.iter().any(|y| (y - x).norm() <= tol) {
            shared_pts.push(x);
        }
    }
    let near_shared = |x: &V2| shared_pts.iter().any(|y| (y - x).norm() <= tol);
    // ends of one piece on the other (except shared ones)
    for (a, other) in [(p, q), (q, p)] {
        for s in [0.0, 1.0] {
            let x = a.seg.point(s);
            if !near_shared(&x) && other.seg.dist(&x) <= tol {
                return true;
            }
        }
    }
    let both = |x: &V2| p.seg.spans(x, tol) && q.seg.spans(x, tol) && !near_shared(x);
    match (p.seg, q.seg) {
        (Seg2::Line { a, b: bb }, Seg2::Line { a: c, b: d }) => {
            if shared_pts.len() >= 2 {
                return true; // two different straight edges between the same two points
            }
            if !shared_pts.is_empty() {
                return false; // non-collinear: they meet only there; collinear: ends checked above
            }
            let (d1, d2) = (cross2(&(bb - a), &(c - a)), cross2(&(bb - a), &(d - a)));
            let (d3, d4) = (cross2(&(d - c), &(a - c)), cross2(&(d - c), &(bb - c)));
            d1 * d2 < 0.0 && d3 * d4 < 0.0
        }
        (Seg2::Line { a, b: bb }, arc @ Seg2::Arc { c, r, .. }) | (arc @ Seg2::Arc { c, r, .. }, Seg2::Line { a, b: bb }) => {
            let len = (bb - a).norm();
            let u = (bb - a) / len;
            let f = a - c;
            let (h, k) = (f.dot(&u), f.norm_squared() - r * r);
            let on_line = |s: f64| s >= -tol && s <= len + tol;
            match shared_pts.len() {
                0 => {
                    let disc = h * h - k;
                    if disc < 0.0 {
                        // closest approach of the line to the circle
                        let foot = a - u * h;
                        ((foot - c).norm() - r).abs() <= tol && on_line(-h) && arc.spans(&foot, tol)
                    } else {
                        [-h - disc.sqrt(), -h + disc.sqrt()].into_iter().any(|s| on_line(s) && both(&(a + u * s)))
                    }
                }
                1 => {
                    // the other root of s² + 2hs + k = 0 besides the shared point's
                    let sp = (shared_pts[0] - a).dot(&u);
                    let s2 = -2.0 * h - sp;
                    (s2 - sp).abs() > tol && on_line(s2) && both(&(a + u * s2))
                }
                _ => false,
            }
        }
        (Seg2::Arc { c: c1, r: r1, .. }, Seg2::Arc { c: c2, r: r2, .. }) => {
            let d = c2 - c1;
            let dn = d.norm();
            if dn <= tol {
                // concentric: apart unless on the same circle and overlapping
                return (r1 - r2).abs() <= tol && (q.seg.spans(&p.seg.point(0.5), -tol) || p.seg.spans(&q.seg.point(0.5), -tol));
            }
            let u = d / dn;
            match shared_pts.len() {
                0 => {
                    let a = (dn * dn + r1 * r1 - r2 * r2) / (2.0 * dn);
                    let h2 = r1 * r1 - a * a;
                    let base = c1 + u * a;
                    if h2 < 0.0 {
                        let tangent = (dn - (r1 + r2)).abs() <= tol || (dn - (r1 - r2).abs()).abs() <= tol;
                        tangent && both(&base)
                    } else {
                        let perp = V2::new(-u.y, u.x) * h2.sqrt();
                        both(&(base + perp)) || both(&(base - perp))
                    }
                }
                1 => {
                    // the other intersection: the shared point mirrored in the line of centres
                    let s = shared_pts[0];
                    let foot = c1 + u * (s - c1).dot(&u);
                    let m = foot * 2.0 - s;
                    (m - s).norm() > tol && both(&m)
                }
                _ => false,
            }
        }
    }
}

/// Every coedge of a planar face in its plane (see [`Seg2`]); lines and coplanar circles only.
fn face_pieces(b: &Brep, f: FaceId) -> GResult<Vec<Vec<Piece>>> {
    let (frame, n) = plane_of(b, f);
    let flip = if n.dot(&frame.z) < 0.0 { -1.0 } else { 1.0 };
    let to2 = move |p: &P3| {
        let l = frame.to_local(*p);
        V2::new(l.x, flip * l.y)
    };
    let mut loops = Vec::new();
    for &lp in &b.face(f).loops {
        let mut pieces = Vec::new();
        for &c in &b.lp(lp).coedges {
            let ce = b.coedge(c);
            let e = b.edge(ce.edge);
            let v = [b.coedge_start(c), b.coedge_end(c)];
            let seg = match &e.curve {
                Some(Curve::Line { .. }) => Seg2::Line { a: to2(&b.vertex(v[0]).point), b: to2(&b.vertex(v[1]).point) },
                Some(Curve::Circle { frame: cf, radius }) if cf.z.cross(&n).norm() <= 1e-9 => {
                    let centre = to2(&cf.origin);
                    let angle = |t: f64| {
                        let d = to2(&(cf.origin + (cf.x * t.cos() + cf.y * t.sin()) * *radius)) - centre;
                        d.y.atan2(d.x)
                    };
                    let ccw = cf.z.dot(&n).signum();
                    let span = (e.t1 - e.t0) * ccw;
                    if ce.reversed {
                        Seg2::Arc { c: centre, r: *radius, a0: angle(e.t1), sweep: -span }
                    } else {
                        Seg2::Arc { c: centre, r: *radius, a0: angle(e.t0), sweep: span }
                    }
                }
                Some(c) => return unsupported(format!("blend next to the plane face F{} bounded by a {} edge", f.0, c.type_name())),
                None => continue,
            };
            pieces.push(Piece { seg, v, p: [to2(&b.vertex(v[0]).point), to2(&b.vertex(v[1]).point)] });
        }
        loops.push(pieces);
    }
    Ok(loops)
}

/// A planar face touched by a blend: its edges meet only at shared vertices, and it still has
/// exactly one outer loop (counter-clockwise about the outward normal) containing every hole,
/// holes not nested.
fn check_planar_face(b: &Brep, f: FaceId, spec: &Spec) -> GResult<()> {
    let tol = spec.tol.linear;
    let loops = face_pieces(b, f)?;
    let all: Vec<&Piece> = loops.iter().flatten().collect();
    for i in 0..all.len() {
        for j in i + 1..all.len() {
            if pieces_touch(all[i], all[j], tol) {
                return spec.too_large(format!("the new boundary of face F{} runs into another of its edges", f.0));
            }
        }
    }
    let area = |l: &[Piece]| l.iter().map(|p| p.seg.area_term()).sum::<f64>();
    let winding = |l: &[Piece], p: &V2| (l.iter().map(|q| q.seg.winding_angle(p)).sum::<f64>() / TAU).round() as i64;
    let outer: Vec<usize> = (0..loops.len()).filter(|&k| area(&loops[k]) > 0.0).collect();
    if outer.len() != 1 {
        return spec.too_large(format!("face F{} no longer has one outer boundary", f.0));
    }
    for (k, l) in loops.iter().enumerate() {
        if k == outer[0] || l.is_empty() {
            continue;
        }
        let p = l[0].seg.point(0.5);
        if winding(&loops[outer[0]], &p) != 1 || loops.iter().enumerate().any(|(j, h)| j != k && j != outer[0] && winding(h, &p) != 0) {
            return spec.too_large(format!("a hole of face F{} is no longer inside it", f.0));
        }
    }
    Ok(())
}
