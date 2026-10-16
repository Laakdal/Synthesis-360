//! Solid bodies produced by the feature history, with lazily computed topology and display mesh.

use crate::frame::{dist, normalize, V3};
use crate::occ::{Shape, EDGE, FACE};
use cad_core::{BodyId, CurveKind, FeatureId, SurfaceKind};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Debug)]
pub struct FaceInfo {
    pub surface: SurfaceKind,
    pub area: f64,
    pub centroid: V3,
    /// Outward normal (at the parametric centre).
    pub normal: V3,
    pub axis: V3,
    pub axis_origin: V3,
    pub radius: f64,
}

#[derive(Clone, Debug)]
pub struct EdgeInfo {
    pub curve: CurveKind,
    pub length: f64,
    pub start: V3,
    pub mid: V3,
    pub end: V3,
    /// Unit tangent at the midpoint.
    pub dir: V3,
    /// Adjacent face indices (-1 when absent).
    pub faces: [i32; 2],
    pub center: V3,
    pub radius: f64,
    pub closed: bool,
}

pub struct Topology {
    pub faces: Vec<Shape>,
    pub edges: Vec<Shape>,
    pub face_info: Vec<FaceInfo>,
    pub edge_info: Vec<EdgeInfo>,
    /// [minx, miny, minz, maxx, maxy, maxz]
    pub bbox: [f64; 6],
}

/// Triangle mesh for display and picking. Triangles of face `i` are
/// `indices[face_ranges[i].0 .. face_ranges[i].1]` (index range, multiple of 3).
#[derive(Clone, Debug, Default)]
pub struct BodyMesh {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    pub face_ranges: Vec<(u32, u32)>,
    pub edge_lines: Vec<Vec<[f32; 3]>>,
}

pub struct BodyData {
    pub id: BodyId,
    pub created_by: FeatureId,
    pub shape: Shape,
    topo: OnceLock<Arc<Topology>>,
    mesh: OnceLock<Arc<BodyMesh>>,
    volume: OnceLock<f64>,
}

pub type Body = Arc<BodyData>;

pub fn new_body(id: BodyId, created_by: FeatureId, shape: Shape) -> Body {
    Arc::new(BodyData { id, created_by, shape, topo: OnceLock::new(), mesh: OnceLock::new(), volume: OnceLock::new() })
}

fn surface_kind(t: f64) -> SurfaceKind {
    match t as i32 {
        0 => SurfaceKind::Plane,
        1 => SurfaceKind::Cylinder,
        _ => SurfaceKind::Other,
    }
}
fn curve_kind(t: f64) -> CurveKind {
    match t as i32 {
        0 => CurveKind::Line,
        1 => CurveKind::Circle,
        _ => CurveKind::Other,
    }
}
fn v(a: &[f64]) -> V3 {
    [a[0], a[1], a[2]]
}

impl BodyData {
    pub fn volume(&self) -> f64 {
        *self.volume.get_or_init(|| self.shape.volume())
    }

    pub fn topology(&self) -> Arc<Topology> {
        self.topo
            .get_or_init(|| {
                let faces = self.shape.subshapes(FACE);
                let edges = self.shape.subshapes(EDGE);
                let adjacency = self.shape.edge_faces();
                let face_info = faces
                    .iter()
                    .map(|f| {
                        let o = f.face_info().unwrap_or([0.0; 15]);
                        FaceInfo { surface: surface_kind(o[0]), area: o[1], centroid: v(&o[2..5]), normal: normalize(v(&o[5..8])), axis: v(&o[8..11]), axis_origin: v(&o[11..14]), radius: o[14] }
                    })
                    .collect();
                let edge_info = edges
                    .iter()
                    .enumerate()
                    .map(|(i, e)| {
                        let o = e.edge_info().unwrap_or([0.0; 19]);
                        EdgeInfo {
                            curve: if o[0] < 0.0 { CurveKind::Other } else { curve_kind(o[0]) },
                            length: o[1],
                            start: v(&o[2..5]),
                            mid: v(&o[5..8]),
                            end: v(&o[8..11]),
                            dir: v(&o[11..14]),
                            faces: adjacency.get(i).copied().unwrap_or([-1, -1]),
                            center: v(&o[14..17]),
                            radius: o[17],
                            closed: o[18] > 0.5,
                        }
                    })
                    .collect();
                let bbox = self.shape.bbox().unwrap_or([0.0; 6]);
                Arc::new(Topology { faces, edges, face_info, edge_info, bbox })
            })
            .clone()
    }

    pub fn mesh(&self) -> Arc<BodyMesh> {
        self.mesh
            .get_or_init(|| {
                let topo = self.topology();
                let b = topo.bbox;
                let diag = dist([b[0], b[1], b[2]], [b[3], b[4], b[5]]).max(1.0);
                let defl = (diag * 0.0012).clamp(0.005, 0.5);
                self.shape.mesh(defl, 0.3);
                let mut m = BodyMesh::default();
                for f in &topo.faces {
                    let start = m.indices.len() as u32;
                    f.face_triangles(&mut m.positions, &mut m.normals, &mut m.indices);
                    m.face_ranges.push((start, m.indices.len() as u32));
                }
                m.edge_lines = topo.edges.iter().map(|e| e.edge_polyline(defl)).collect();
                Arc::new(m)
            })
            .clone()
    }
}
