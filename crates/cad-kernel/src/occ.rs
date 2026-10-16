//! Safe wrapper over the exception-safe Open CASCADE shim (`opencascade_sys::cad360`).

use cxx::UniquePtr;
use opencascade_sys::cad360 as ffi;
use opencascade_sys::topo_ds::TopoDS_Shape;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct KernelError(pub String);

pub type KResult<T> = Result<T, KernelError>;

fn last_error(context: &str) -> KernelError {
    let e = ffi::cad_last_error();
    KernelError(if e.is_empty() { context.to_string() } else { format!("{context}: {e}") })
}

/// An Open CASCADE B-rep shape (reference-counted; cloning shares the underlying TShape).
pub struct Shape(UniquePtr<TopoDS_Shape>);

// TopoDS shapes are handles to immutable, atomically reference-counted topology.
unsafe impl Send for Shape {}
unsafe impl Sync for Shape {}

impl Clone for Shape {
    fn clone(&self) -> Self {
        Shape(ffi::cad_copy(&self.0))
    }
}

impl std::fmt::Debug for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Shape(type={})", self.shape_type())
    }
}

pub const SOLID: i32 = 2;
pub const FACE: i32 = 4;
pub const EDGE: i32 = 6;

fn wrap(p: UniquePtr<TopoDS_Shape>, context: &str) -> KResult<Shape> {
    if p.is_null() { Err(last_error(context)) } else { Ok(Shape(p)) }
}

/// One profile loop segment in 3D: a line, or an arc through start/mid/end.
#[derive(Clone, Copy, Debug)]
pub enum Seg3 {
    Line([f64; 3], [f64; 3]),
    Arc([f64; 3], [f64; 3], [f64; 3]),
}

impl Shape {
    pub fn inner(&self) -> &TopoDS_Shape {
        &self.0
    }

    /// Planar face from an outer loop followed by hole loops.
    pub fn face_from_loops(loops: &[Vec<Seg3>]) -> KResult<Shape> {
        let mut data = vec![loops.len() as f64];
        for l in loops {
            data.push(l.len() as f64);
            for s in l {
                let (k, a, m, b) = match *s {
                    Seg3::Line(a, b) => (0.0, a, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0], b),
                    Seg3::Arc(a, m, b) => (1.0, a, m, b),
                };
                data.push(k);
                data.extend_from_slice(&a);
                data.extend_from_slice(&m);
                data.extend_from_slice(&b);
            }
        }
        wrap(ffi::cad_make_face(&data), "Profile could not be converted to a face")
    }

    pub fn prism(&self, v: [f64; 3]) -> KResult<Shape> {
        wrap(ffi::cad_prism(&self.0, v[0], v[1], v[2]), "Extrusion failed")
    }

    pub fn revolve(&self, origin: [f64; 3], axis: [f64; 3], angle_rad: f64) -> KResult<Shape> {
        wrap(ffi::cad_revol(&self.0, origin[0], origin[1], origin[2], axis[0], axis[1], axis[2], angle_rad), "Revolve failed")
    }

    pub fn fuse(&self, other: &Shape) -> KResult<Shape> {
        wrap(ffi::cad_boolean(&self.0, &other.0, 0), "Join failed")
    }
    pub fn cut(&self, other: &Shape) -> KResult<Shape> {
        wrap(ffi::cad_boolean(&self.0, &other.0, 1), "Cut failed")
    }
    pub fn common(&self, other: &Shape) -> KResult<Shape> {
        wrap(ffi::cad_boolean(&self.0, &other.0, 2), "Intersect failed")
    }

    pub fn compound(shapes: &[&Shape]) -> KResult<Shape> {
        let mut c = ffi::cad_compound_new();
        for s in shapes {
            if !ffi::cad_compound_add(c.pin_mut(), &s.0) {
                return Err(last_error("Could not build a compound"));
            }
        }
        wrap(c, "Could not build a compound")
    }

    pub fn fillet(&self, edges: &[&Shape], radius: f64) -> KResult<Shape> {
        let c = Shape::compound(edges)?;
        wrap(ffi::cad_fillet(&self.0, &c.0, radius), "Fillet failed")
    }
    pub fn chamfer(&self, edges: &[&Shape], distance: f64) -> KResult<Shape> {
        let c = Shape::compound(edges)?;
        wrap(ffi::cad_chamfer(&self.0, &c.0, distance), "Chamfer failed")
    }

    pub fn cylinder(origin: [f64; 3], dir: [f64; 3], r: f64, h: f64) -> KResult<Shape> {
        wrap(ffi::cad_cylinder(origin[0], origin[1], origin[2], dir[0], dir[1], dir[2], r, h), "Cylinder failed")
    }

    /// Unique sub-shapes (faces, edges, solids) in a stable order.
    pub fn subshapes(&self, kind: i32) -> Vec<Shape> {
        let v = ffi::cad_subshapes(&self.0, kind);
        v.iter().map(|s| Shape(opencascade_sys::topo_ds::TopoDS_Shape_to_owned(s))).collect()
    }

    /// For each edge (subshapes(EDGE) order) the indices of its two adjacent faces (-1 if none).
    pub fn edge_faces(&self) -> Vec<[i32; 2]> {
        let mut v = Vec::new();
        ffi::cad_edge_faces(&self.0, &mut v);
        v.chunks(2).map(|c| [c[0], c.get(1).copied().unwrap_or(-1)]).collect()
    }

    pub fn mesh(&self, deflection: f64, angular: f64) -> bool {
        ffi::cad_mesh(&self.0, deflection, angular)
    }

    /// Appends this face's triangles (requires `mesh` on the owning shape).
    pub fn face_triangles(&self, pos: &mut Vec<f32>, nrm: &mut Vec<f32>, idx: &mut Vec<u32>) -> bool {
        ffi::cad_face_triangles(&self.0, pos, nrm, idx)
    }

    pub fn edge_polyline(&self, deflection: f64) -> Vec<[f32; 3]> {
        let mut v = Vec::new();
        ffi::cad_edge_polyline(&self.0, deflection, &mut v);
        v.chunks(3).map(|c| [c[0], c[1], c[2]]).collect()
    }

    pub fn face_info(&self) -> Option<[f64; 15]> {
        let mut out = [0.0; 15];
        ffi::cad_face_info(&self.0, &mut out).then_some(out)
    }
    pub fn edge_info(&self) -> Option<[f64; 19]> {
        let mut out = [0.0; 19];
        ffi::cad_edge_info(&self.0, &mut out).then_some(out)
    }

    pub fn volume(&self) -> f64 {
        ffi::cad_volume(&self.0)
    }
    pub fn area(&self) -> f64 {
        ffi::cad_area(&self.0)
    }
    pub fn bbox(&self) -> Option<[f64; 6]> {
        let mut out = [0.0; 6];
        ffi::cad_bbox(&self.0, &mut out).then_some(out)
    }
    pub fn is_valid(&self) -> bool {
        ffi::cad_is_valid(&self.0)
    }
    pub fn shape_type(&self) -> i32 {
        ffi::cad_shape_type(&self.0)
    }

    pub fn write_step(&self, path: &str) -> KResult<()> {
        if ffi::cad_write_step(&self.0, path) { Ok(()) } else { Err(last_error("STEP export failed")) }
    }
    pub fn read_step(path: &str) -> KResult<Shape> {
        wrap(ffi::cad_read_step(path), "STEP import failed")
    }
    pub fn write_stl(&self, path: &str, ascii: bool) -> KResult<()> {
        if ffi::cad_write_stl(&self.0, path, ascii) { Ok(()) } else { Err(last_error("STL export failed")) }
    }
}

/// Silences Open CASCADE's console messages (e.g. STEP transfer statistics).
pub fn quiet() {
    ffi::cad_quiet();
}
