//! Boundary representation: topology in typed arenas, geometry attached by value.
//!
//! Solid → Shells → Faces → Loops → Coedges → Edges → Vertices. An edge is shared by the
//! coedges of its adjacent faces (two for a closed manifold edge, one per side of a seam edge
//! of a periodic face: the same face then owns both coedges). Degenerate edges (sphere poles,
//! cone apex) have no curve and coincident start/end vertices. An edge listed in
//! [`Brep::nonmanifold_edges`] may carry 2k coedges of one shell (line contact between two
//! solids, as ASM builds it).

use crate::{Curve, Surface, P3};
use serde::{Deserialize, Serialize};

macro_rules! id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub u32);
        impl $name {
            pub fn idx(self) -> usize {
                self.0 as usize
            }
        }
    };
}
id!(VertexId);
id!(EdgeId);
id!(CoedgeId);
id!(LoopId);
id!(FaceId);
id!(ShellId);
id!(SolidId);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    pub point: P3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// `None` for a degenerate (zero-length) edge.
    pub curve: Option<Curve>,
    /// Parameter range on `curve` (start < end), running from `start` to `end` vertex.
    pub t0: f64,
    pub t1: f64,
    pub start: VertexId,
    pub end: VertexId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Coedge {
    pub edge: EdgeId,
    /// True when this use runs against the edge direction (end → start).
    pub reversed: bool,
    pub owner: LoopId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Loop {
    /// Coedges in order; each one's end vertex is the next one's start vertex.
    pub coedges: Vec<CoedgeId>,
    pub face: FaceId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub surface: Surface,
    /// True when the face's outward normal is opposite to the surface normal (du × dv).
    pub reversed: bool,
    /// Loops bounding the face; the material lies to the left of each coedge seen from the
    /// outward normal. A closed periodic face (full sphere) may have no loop.
    pub loops: Vec<LoopId>,
    pub shell: ShellId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shell {
    pub faces: Vec<FaceId>,
    pub solid: Option<SolidId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Solid {
    /// The first shell is the outer one; any others bound voids.
    pub shells: Vec<ShellId>,
}

/// One B-rep model (possibly several solids or open shells).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Brep {
    pub vertices: Vec<Vertex>,
    pub edges: Vec<Edge>,
    pub coedges: Vec<Coedge>,
    pub loops: Vec<Loop>,
    pub faces: Vec<Face>,
    pub shells: Vec<Shell>,
    pub solids: Vec<Solid>,
    /// Edges that are *deliberately* non-manifold: used by 2k (k ≥ 2) coedges of one shell, k in
    /// each direction (ASM's line contact, `bool.union.tangent_cylinder`: one edge with 4 coedges).
    /// [`crate::validate`] accepts exactly these edges with more than two uses; every other edge
    /// is still held to the manifold rule. Empty for every model except such boolean results.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nonmanifold_edges: Vec<EdgeId>,
}

impl Brep {
    /// True when `e` is listed in [`Brep::nonmanifold_edges`].
    pub fn is_flagged_nonmanifold(&self, e: EdgeId) -> bool {
        self.nonmanifold_edges.contains(&e)
    }

    pub fn vertex(&self, id: VertexId) -> &Vertex {
        &self.vertices[id.idx()]
    }
    pub fn edge(&self, id: EdgeId) -> &Edge {
        &self.edges[id.idx()]
    }
    pub fn coedge(&self, id: CoedgeId) -> &Coedge {
        &self.coedges[id.idx()]
    }
    pub fn lp(&self, id: LoopId) -> &Loop {
        &self.loops[id.idx()]
    }
    pub fn face(&self, id: FaceId) -> &Face {
        &self.faces[id.idx()]
    }
    pub fn shell(&self, id: ShellId) -> &Shell {
        &self.shells[id.idx()]
    }
    pub fn solid(&self, id: SolidId) -> &Solid {
        &self.solids[id.idx()]
    }
}

// ---------------------------------------------------------------------------------------------
// Builder: every `add_*` sets the child→parent back-link and appends to the parent's list, so a
// model assembled only through these calls is referentially consistent by construction.
// Conventions used by the builder, the primitives and the integrators:
// * a degenerate edge's `[t0, t1]` is the surface-u range it spans on its face (the pcurve is
//   u = t at the pole's v); a forward use runs u from t0 to t1;
// * a seam edge's two uses lie in the same face and run in opposite directions.
// ---------------------------------------------------------------------------------------------

impl Brep {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_vertex(&mut self, point: P3) -> VertexId {
        self.vertices.push(Vertex { point });
        VertexId(self.vertices.len() as u32 - 1)
    }

    /// Edge on `curve` over `[t0, t1]` from `start` to `end` (`curve = None`: degenerate edge).
    pub fn add_edge(&mut self, curve: Option<Curve>, t0: f64, t1: f64, start: VertexId, end: VertexId) -> EdgeId {
        self.edges.push(Edge { curve, t0, t1, start, end });
        EdgeId(self.edges.len() as u32 - 1)
    }

    /// Straight edge between two existing vertices (t is arc length from `a`).
    pub fn add_line_edge(&mut self, a: VertexId, b: VertexId) -> EdgeId {
        let (pa, pb) = (self.vertex(a).point, self.vertex(b).point);
        self.add_edge(Some(Curve::line_through(pa, pb)), 0.0, (pb - pa).norm(), a, b)
    }

    /// Degenerate (zero-length) edge at `v` spanning surface u ∈ `[u0, u1]` (pole, apex).
    pub fn add_degenerate_edge(&mut self, v: VertexId, u0: f64, u1: f64) -> EdgeId {
        self.add_edge(None, u0, u1, v, v)
    }

    pub fn add_solid(&mut self) -> SolidId {
        self.solids.push(Solid { shells: Vec::new() });
        SolidId(self.solids.len() as u32 - 1)
    }

    /// New empty shell, appended to `solid`'s shells when given.
    pub fn add_shell(&mut self, solid: Option<SolidId>) -> ShellId {
        let id = ShellId(self.shells.len() as u32);
        self.shells.push(Shell { faces: Vec::new(), solid });
        if let Some(s) = solid {
            self.solids[s.idx()].shells.push(id);
        }
        id
    }

    /// New face without loops, appended to `shell`.
    pub fn add_face(&mut self, shell: ShellId, surface: Surface, reversed: bool) -> FaceId {
        let id = FaceId(self.faces.len() as u32);
        self.faces.push(Face { surface, reversed, loops: Vec::new(), shell });
        self.shells[shell.idx()].faces.push(id);
        id
    }

    /// New loop on `face` made of the given edge uses `(edge, reversed)` in order.
    pub fn add_loop(&mut self, face: FaceId, uses: &[(EdgeId, bool)]) -> LoopId {
        let id = LoopId(self.loops.len() as u32);
        self.loops.push(Loop { coedges: Vec::new(), face });
        self.faces[face.idx()].loops.push(id);
        for &(e, r) in uses {
            self.add_coedge(id, e, r);
        }
        id
    }

    /// Appends one edge use to the end of `lp`.
    pub fn add_coedge(&mut self, lp: LoopId, edge: EdgeId, reversed: bool) -> CoedgeId {
        let id = CoedgeId(self.coedges.len() as u32);
        self.coedges.push(Coedge { edge, reversed, owner: lp });
        self.loops[lp.idx()].coedges.push(id);
        id
    }

    // ----- queries -----

    pub fn vertex_ids(&self) -> impl Iterator<Item = VertexId> {
        (0..self.vertices.len() as u32).map(VertexId)
    }
    pub fn edge_ids(&self) -> impl Iterator<Item = EdgeId> {
        (0..self.edges.len() as u32).map(EdgeId)
    }
    pub fn face_ids(&self) -> impl Iterator<Item = FaceId> {
        (0..self.faces.len() as u32).map(FaceId)
    }
    pub fn solid_ids(&self) -> impl Iterator<Item = SolidId> {
        (0..self.solids.len() as u32).map(SolidId)
    }

    /// True for a zero-length edge (no curve).
    pub fn is_degenerate(&self, e: EdgeId) -> bool {
        self.edge(e).curve.is_none()
    }

    /// Faces of all shells of a solid (outer shell first).
    pub fn solid_faces(&self, s: SolidId) -> impl Iterator<Item = FaceId> + '_ {
        self.solid(s).shells.iter().flat_map(move |&sh| self.shell(sh).faces.iter().copied())
    }

    /// Coedges of all loops of a face, loop by loop in order.
    pub fn face_coedges(&self, f: FaceId) -> impl Iterator<Item = CoedgeId> + '_ {
        self.face(f).loops.iter().flat_map(move |&l| self.lp(l).coedges.iter().copied())
    }

    /// Distinct edges of a face in order of first use (a seam edge appears once).
    pub fn face_edges(&self, f: FaceId) -> Vec<EdgeId> {
        let mut out: Vec<EdgeId> = Vec::new();
        for c in self.face_coedges(f) {
            let e = self.coedge(c).edge;
            if !out.contains(&e) {
                out.push(e);
            }
        }
        out
    }

    /// All coedges (uses) of an edge, found by scanning the loops of every face.
    pub fn edge_coedges(&self, e: EdgeId) -> Vec<CoedgeId> {
        self.loops.iter().flat_map(|l| l.coedges.iter().copied()).filter(|&c| self.coedge(c).edge == e).collect()
    }

    /// Distinct faces around an edge (one face for a seam edge, two for an ordinary manifold edge).
    pub fn edge_faces(&self, e: EdgeId) -> Vec<FaceId> {
        let mut out: Vec<FaceId> = Vec::new();
        for c in self.edge_coedges(e) {
            let f = self.coedge_face(c);
            if !out.contains(&f) {
                out.push(f);
            }
        }
        out
    }

    pub fn coedge_face(&self, c: CoedgeId) -> FaceId {
        self.lp(self.coedge(c).owner).face
    }

    /// Vertex where the coedge starts, respecting `reversed`.
    pub fn coedge_start(&self, c: CoedgeId) -> VertexId {
        let (ce, e) = (self.coedge(c), self.edge(self.coedge(c).edge));
        if ce.reversed { e.end } else { e.start }
    }

    /// Vertex where the coedge ends, respecting `reversed`.
    pub fn coedge_end(&self, c: CoedgeId) -> VertexId {
        let (ce, e) = (self.coedge(c), self.edge(self.coedge(c).edge));
        if ce.reversed { e.start } else { e.end }
    }

    /// Start and end positions of a coedge in its direction of travel.
    pub fn coedge_points(&self, c: CoedgeId) -> (P3, P3) {
        (self.vertex(self.coedge_start(c)).point, self.vertex(self.coedge_end(c)).point)
    }

    /// Distinct vertices used by the edges of a shell.
    pub fn shell_vertices(&self, s: ShellId) -> Vec<VertexId> {
        let mut out: Vec<VertexId> = Vec::new();
        for e in self.shell_edges(s) {
            for v in [self.edge(e).start, self.edge(e).end] {
                if !out.contains(&v) {
                    out.push(v);
                }
            }
        }
        out
    }

    /// Distinct edges used by the faces of a shell (degenerate edges included).
    pub fn shell_edges(&self, s: ShellId) -> Vec<EdgeId> {
        let mut out: Vec<EdgeId> = Vec::new();
        for &f in &self.shell(s).faces {
            for e in self.face_edges(f) {
                if !out.contains(&e) {
                    out.push(e);
                }
            }
        }
        out
    }

    /// Axis-aligned bounds of the vertex positions (`None` without vertices).
    pub fn vertex_bounds(&self) -> Option<(P3, P3)> {
        let mut it = self.vertices.iter().map(|v| v.point);
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.inf(&p), hi.sup(&p))))
    }
}
