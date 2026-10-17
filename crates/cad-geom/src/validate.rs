//! Topological and geometric validity checks with diagnostic reports.
//!
//! Checks, in order (geometry is only examined once every reference resolves, so a broken
//! model never panics):
//! 1. references in range and back-links consistent (`DanglingReference`);
//! 2. each loop non-empty and closed: coedge i ends where coedge i+1 starts (`OpenLoop`);
//! 3. edge uses, counted by walking solid → shell → face → loop → coedge: in a shell that
//!    belongs to a solid every curved edge has exactly two uses and every degenerate edge
//!    exactly one; in an open shell at most two (`NonManifoldEdge`); the two uses of an edge run
//!    in opposite directions (`InconsistentOrientation`; seam edges included). The only
//!    exemption is an edge explicitly listed in [`Brep::nonmanifold_edges`]: it may have any even
//!    number 2k ≥ 2 of uses in a closed shell, k in each direction (`NonManifoldEdge` /
//!    `InconsistentOrientation` otherwise); unlisted edges are checked as above;
//! 4. Euler–Poincaré per shell, V − E + F − (L − F) = 2(1 − G) (`EulerPoincare` when the genus
//!    comes out negative or non-integral). Degenerate edges are excluded from E (a collapsed
//!    edge is not a 1-cell); a loopless face on a torus counts as a genus-1 handle; a flagged
//!    non-manifold edge with 2k uses counts as k edges (the manifold edges it would split into,
//!    its end vertices staying shared);
//! 5. geometry (`GeometryMismatch`): edge curve ends at its vertices, vertices and interior
//!    curve points on every face surface using the edge (closed-form distance), degenerate
//!    edges start and end at the same vertex;
//! 6. orientation from the exact integrals of [`crate::mass`]: every face's signed area must be
//!    positive (loops counter-clockwise about the outward normal) and every solid's volume
//!    positive (`InconsistentOrientation`). A face whose integrals fail (NURBS, or loops that
//!    bound no finite region) is reported as `Unevaluable`, never silently passed.
//!
//! The report's counts are raw totals over the model (degenerate edges included), matching what
//! OCCT's `TopExp::MapShapes` reports.

use crate::param::surface_distance;
use crate::topology::Edge;
use crate::{Brep, EdgeId, FaceId, ShellId, Surface, Tolerance};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Issue {
    /// A reference points outside its arena or back-links disagree.
    DanglingReference(String),
    /// Consecutive coedges of a loop do not share a vertex.
    OpenLoop(String),
    /// An edge is used by a number of coedges other than two (in a closed shell).
    NonManifoldEdge(String),
    /// The two uses of an edge run in the same direction (inconsistent orientation).
    InconsistentOrientation(String),
    /// A vertex or edge does not lie on the geometry it bounds, within tolerance.
    GeometryMismatch(String),
    /// V − E + F − (L − F) − 2(S − G) ≠ 0.
    EulerPoincare(String),
    /// A check could not be carried out: the exact integrals of a face failed (unsupported
    /// geometry such as NURBS, or loops that bound no finite region). The model is not known to
    /// be valid, so this counts against `is_valid`.
    Unevaluable(String),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub issues: Vec<Issue>,
    pub vertices: usize,
    pub edges: usize,
    pub faces: usize,
    pub loops: usize,
    pub shells: usize,
    /// Genus derived from the Euler–Poincaré formula (sum over shells).
    pub genus: i64,
    /// Degenerate (zero-length) edges among `edges`; excluded from E in the Euler formula.
    pub degenerate_edges: usize,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Runs every check on the whole model (see module docs).
pub fn validate(brep: &Brep, tol: &Tolerance) -> ValidationReport {
    let mut r = ValidationReport {
        vertices: brep.vertices.len(),
        edges: brep.edges.len(),
        faces: brep.faces.len(),
        loops: brep.loops.len(),
        shells: brep.shells.len(),
        degenerate_edges: brep.edges.iter().filter(|e| e.curve.is_none()).count(),
        ..Default::default()
    };
    check_references(brep, &mut r.issues);
    if !r.issues.is_empty() {
        return r;
    }
    check_loops(brep, &mut r.issues);
    check_edge_uses(brep, &mut r.issues);
    for s in 0..brep.shells.len() {
        r.genus += euler(brep, ShellId(s as u32), &mut r.issues);
    }
    check_geometry(brep, tol, &mut r.issues);
    // the exact integrals need closed, manifold loops; skip them when the topology is broken
    if !has_topology_issue(&r.issues) {
        check_orientation(brep, &mut r.issues);
    }
    r
}

fn has_topology_issue(issues: &[Issue]) -> bool {
    issues.iter().any(|i| !matches!(i, Issue::GeometryMismatch(_)))
}

fn check_references(b: &Brep, out: &mut Vec<Issue>) {
    let mut bad = |m: String| out.push(Issue::DanglingReference(m));
    let (nv, ne, nc, nl, nf, ns, nso) =
        (b.vertices.len(), b.edges.len(), b.coedges.len(), b.loops.len(), b.faces.len(), b.shells.len(), b.solids.len());
    for (i, e) in b.edges.iter().enumerate() {
        for (what, v) in [("start", e.start), ("end", e.end)] {
            if v.idx() >= nv {
                bad(format!("edge E{i} {what} vertex V{} does not exist", v.0));
            }
        }
    }
    for (i, c) in b.coedges.iter().enumerate() {
        if c.edge.idx() >= ne {
            bad(format!("coedge C{i} references missing edge E{}", c.edge.0));
        }
        if c.owner.idx() >= nl {
            bad(format!("coedge C{i} owner loop L{} does not exist", c.owner.0));
        } else if !b.loops[c.owner.idx()].coedges.iter().any(|x| x.idx() == i) {
            bad(format!("coedge C{i} names loop L{} as owner but is not in its list", c.owner.0));
        }
    }
    for (i, l) in b.loops.iter().enumerate() {
        for c in &l.coedges {
            if c.idx() >= nc {
                bad(format!("loop L{i} references missing coedge C{}", c.0));
            } else if b.coedges[c.idx()].owner.idx() != i {
                bad(format!("loop L{i} lists coedge C{} whose owner is L{}", c.0, b.coedges[c.idx()].owner.0));
            }
        }
        if l.face.idx() >= nf {
            bad(format!("loop L{i} face F{} does not exist", l.face.0));
        } else if !b.faces[l.face.idx()].loops.iter().any(|x| x.idx() == i) {
            bad(format!("loop L{i} names face F{} but is not in its loop list", l.face.0));
        }
    }
    for (i, f) in b.faces.iter().enumerate() {
        for l in &f.loops {
            if l.idx() >= nl {
                bad(format!("face F{i} references missing loop L{}", l.0));
            } else if b.loops[l.idx()].face.idx() != i {
                bad(format!("face F{i} lists loop L{} whose face is F{}", l.0, b.loops[l.idx()].face.0));
            }
        }
        if f.shell.idx() >= ns {
            bad(format!("face F{i} shell Sh{} does not exist", f.shell.0));
        } else if !b.shells[f.shell.idx()].faces.iter().any(|x| x.idx() == i) {
            bad(format!("face F{i} names shell Sh{} but is not in its face list", f.shell.0));
        }
    }
    for (i, s) in b.shells.iter().enumerate() {
        for f in &s.faces {
            if f.idx() >= nf {
                bad(format!("shell Sh{i} references missing face F{}", f.0));
            } else if b.faces[f.idx()].shell.idx() != i {
                bad(format!("shell Sh{i} lists face F{} whose shell is Sh{}", f.0, b.faces[f.idx()].shell.0));
            }
        }
        if let Some(so) = s.solid {
            if so.idx() >= nso {
                bad(format!("shell Sh{i} solid S{} does not exist", so.0));
            } else if !b.solids[so.idx()].shells.iter().any(|x| x.idx() == i) {
                bad(format!("shell Sh{i} names solid S{} but is not in its shell list", so.0));
            }
        }
    }
    for e in &b.nonmanifold_edges {
        if e.idx() >= ne {
            bad(format!("non-manifold flag names missing edge E{}", e.0));
        }
    }
    for (i, s) in b.solids.iter().enumerate() {
        for sh in &s.shells {
            if sh.idx() >= ns {
                bad(format!("solid S{i} references missing shell Sh{}", sh.0));
            } else if b.shells[sh.idx()].solid.map(|x| x.idx()) != Some(i) {
                bad(format!("solid S{i} lists shell Sh{} which does not name it", sh.0));
            }
        }
    }
}

fn check_loops(b: &Brep, out: &mut Vec<Issue>) {
    for (i, l) in b.loops.iter().enumerate() {
        if l.coedges.is_empty() {
            out.push(Issue::OpenLoop(format!("loop L{i} has no coedges")));
            continue;
        }
        let n = l.coedges.len();
        for k in 0..n {
            let (a, c) = (l.coedges[k], l.coedges[(k + 1) % n]);
            let (end, start) = (b.coedge_end(a), b.coedge_start(c));
            if end != start {
                out.push(Issue::OpenLoop(format!(
                    "loop L{i}: coedge C{} ends at V{} but next coedge C{} starts at V{}",
                    a.0, end.0, c.0, start.0
                )));
            }
        }
    }
}

fn check_edge_uses(b: &Brep, out: &mut Vec<Issue>) {
    for (si, s) in b.shells.iter().enumerate() {
        let closed = s.solid.is_some();
        let mut uses: HashMap<EdgeId, Vec<(bool, FaceId)>> = HashMap::new();
        for &f in &s.faces {
            for c in b.face_coedges(f) {
                let ce = b.coedge(c);
                uses.entry(ce.edge).or_default().push((ce.reversed, f));
            }
        }
        let mut keys: Vec<_> = uses.keys().copied().collect();
        keys.sort();
        for e in keys {
            let u = &uses[&e];
            let degenerate = b.is_degenerate(e);
            if !degenerate && b.is_flagged_nonmanifold(e) {
                // explicitly non-manifold (line contact): 2k uses, k each way
                let fwd = u.iter().filter(|x| !x.0).count();
                if u.len() < 2 || !u.len().is_multiple_of(2) {
                    out.push(Issue::NonManifoldEdge(format!(
                        "flagged non-manifold edge E{} in shell Sh{si} has {} use(s) (expected an even number ≥ 2)",
                        e.0,
                        u.len()
                    )));
                } else if fwd * 2 != u.len() {
                    out.push(Issue::InconsistentOrientation(format!(
                        "flagged non-manifold edge E{} in shell Sh{si}: {fwd} of {} uses run forward (expected half)",
                        e.0,
                        u.len()
                    )));
                }
                continue;
            }
            let want = if degenerate { 1 } else { 2 };
            let bad = if closed { u.len() != want } else { u.len() > want };
            if bad {
                let faces: Vec<String> = u.iter().map(|(_, f)| format!("F{}", f.0)).collect();
                out.push(Issue::NonManifoldEdge(format!(
                    "edge E{} in shell Sh{si} has {} use(s) (expected {want}) in faces [{}]",
                    e.0,
                    u.len(),
                    faces.join(", ")
                )));
            }
            if !degenerate && u.len() == 2 && u[0].0 == u[1].0 {
                out.push(Issue::InconsistentOrientation(format!(
                    "edge E{} is used in the same direction by faces F{} and F{}",
                    e.0, u[0].1 .0, u[1].1 .0
                )));
            }
        }
    }
}

/// Euler–Poincaré for one shell; returns its genus (0 when it cannot be derived).
fn euler(b: &Brep, s: ShellId, out: &mut Vec<Issue>) -> i64 {
    let edges = b.shell_edges(s);
    let v = b.shell_vertices(s).len() as i64;
    let faces = &b.shell(s).faces;
    // a flagged non-manifold edge with 2k uses in this shell counts as k edges
    let uses_in_shell = |e: EdgeId| faces.iter().flat_map(|&f| b.face_coedges(f)).filter(|&c| b.coedge(c).edge == e).count() as i64;
    let e: i64 = edges
        .iter()
        .filter(|&&e| !b.is_degenerate(e))
        .map(|&e| if b.is_flagged_nonmanifold(e) { (uses_in_shell(e) / 2).max(1) } else { 1 })
        .sum();
    let f = faces.len() as i64;
    let l: i64 = faces.iter().map(|&x| b.face(x).loops.len() as i64).sum();
    // a loopless torus face is a closed genus-1 surface on its own: its cell count V − E + F − (L − F)
    // would be 2, its Euler characteristic is 0
    let handles = faces.iter().filter(|&&x| b.face(x).loops.is_empty() && matches!(b.face(x).surface, Surface::Torus { .. })).count() as i64;
    let chi = v - e + f - (l - f) - 2 * handles;
    let twice_g = 2 - chi;
    if twice_g < 0 || twice_g % 2 != 0 {
        out.push(Issue::EulerPoincare(format!(
            "shell Sh{}: V{v} − E{e} + F{f} − (L{l} − F{f}) = {chi} gives no integer genus ≥ 0",
            s.0
        )));
        return 0;
    }
    twice_g / 2
}

fn check_geometry(b: &Brep, tol: &Tolerance, out: &mut Vec<Issue>) {
    let lin = tol.linear;
    let mut mism = |m: String| out.push(Issue::GeometryMismatch(m));
    let nurbs_curve = |e: &Edge| matches!(e.curve, Some(crate::Curve::Nurbs(_)));
    for (i, e) in b.edges.iter().enumerate() {
        match &e.curve {
            None => {
                if e.start != e.end {
                    mism(format!("degenerate edge E{i} joins different vertices V{} and V{}", e.start.0, e.end.0));
                }
            }
            Some(_) if nurbs_curve(e) => {}
            Some(c) => {
                if e.t0.is_nan() || e.t1.is_nan() || e.t0 >= e.t1 {
                    mism(format!("edge E{i} has an empty parameter range [{}, {}]", e.t0, e.t1));
                }
                for (what, t, v) in [("start", e.t0, e.start), ("end", e.t1, e.end)] {
                    let d = (c.point(t) - b.vertex(v).point).norm();
                    if d > lin {
                        mism(format!("edge E{i} {what} point is {d:.3e} from vertex V{}", v.0));
                    }
                }
            }
        }
    }
    for (fi, f) in b.faces.iter().enumerate() {
        if matches!(f.surface, Surface::Nurbs(_)) {
            continue;
        }
        let mut seen: Vec<EdgeId> = Vec::new();
        for c in b.face_coedges(FaceId(fi as u32)) {
            let v = b.coedge_start(c);
            if let Some(d) = surface_distance(&f.surface, &b.vertex(v).point).filter(|&d| d > lin) {
                mism(format!("vertex V{} is {d:.3e} from the surface of face F{fi}", v.0));
            }
            let eid = b.coedge(c).edge;
            let e = b.edge(eid);
            if seen.contains(&eid) || nurbs_curve(e) {
                continue;
            }
            seen.push(eid);
            if let Some(curve) = &e.curve {
                for k in 1..8 {
                    let t = e.t0 + (e.t1 - e.t0) * k as f64 / 8.0;
                    if let Some(d) = surface_distance(&f.surface, &curve.point(t)).filter(|&d| d > lin) {
                        mism(format!("edge E{} at t = {t} is {d:.3e} from the surface of face F{fi}", eid.0));
                        break;
                    }
                }
            }
        }
    }
}

fn check_orientation(b: &Brep, out: &mut Vec<Issue>) {
    let c = b.vertex_bounds().map(|(a, z)| nalgebra::center(&a, &z)).unwrap_or_else(crate::P3::origin);
    for fi in 0..b.faces.len() {
        match crate::mass::face_integrals(b, FaceId(fi as u32), &c) {
            Ok(g) if g[g.len() - 1] < 0.0 => out.push(Issue::InconsistentOrientation(format!(
                "face F{fi}: loops run clockwise about its outward normal (signed area {:.6e})",
                g[g.len() - 1]
            ))),
            Ok(_) => {}
            Err(e) => out.push(Issue::Unevaluable(format!("face F{fi}: {e}"))),
        }
    }
    for si in 0..b.solids.len() {
        match crate::mass::solid_integrals(b, crate::SolidId(si as u32)) {
            Ok((_, m)) if m[0] <= 0.0 => {
                out.push(Issue::InconsistentOrientation(format!("solid S{si} has non-positive volume {:.6e} (inside out)", m[0])));
            }
            _ => {}
        }
    }
}
