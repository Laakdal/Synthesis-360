//! CAD360 kernel shim (see include/cad360.hxx): exception-safe OCCT operations.
//! Functions returning a shape return a null `UniquePtr` on failure; read the reason with
//! [`cad_last_error`].
pub use inner::*;

#[cxx::bridge]
mod inner {
    unsafe extern "C++" {
        include!("opencascade-sys/include/cad360.hxx");

        type TopoDS_Shape = crate::topo_ds::TopoDS_Shape;

        pub fn cad_last_error() -> String;
        pub fn cad_quiet();

        pub fn cad_make_face(data: &[f64]) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_prism(face: &TopoDS_Shape, dx: f64, dy: f64, dz: f64) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_revol(face: &TopoDS_Shape, ox: f64, oy: f64, oz: f64, ax: f64, ay: f64, az: f64, angle: f64) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_boolean(a: &TopoDS_Shape, b: &TopoDS_Shape, op: i32) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_compound_new() -> UniquePtr<TopoDS_Shape>;
        pub fn cad_compound_add(compound: Pin<&mut TopoDS_Shape>, s: &TopoDS_Shape) -> bool;
        pub fn cad_fillet(shape: &TopoDS_Shape, edges: &TopoDS_Shape, r: f64) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_chamfer(shape: &TopoDS_Shape, edges: &TopoDS_Shape, d: f64) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_cylinder(ox: f64, oy: f64, oz: f64, dx: f64, dy: f64, dz: f64, r: f64, h: f64) -> UniquePtr<TopoDS_Shape>;

        pub fn cad_subshapes(shape: &TopoDS_Shape, kind: i32) -> UniquePtr<CxxVector<TopoDS_Shape>>;
        pub fn cad_edge_faces(shape: &TopoDS_Shape, out: &mut Vec<i32>) -> bool;
        pub fn cad_mesh(shape: &TopoDS_Shape, deflection: f64, angular: f64) -> bool;
        pub fn cad_face_triangles(face: &TopoDS_Shape, pos: &mut Vec<f32>, nrm: &mut Vec<f32>, idx: &mut Vec<u32>) -> bool;
        pub fn cad_edge_polyline(edge: &TopoDS_Shape, deflection: f64, pts: &mut Vec<f32>) -> bool;
        pub fn cad_face_info(face: &TopoDS_Shape, out: &mut [f64]) -> bool;
        pub fn cad_edge_info(edge: &TopoDS_Shape, out: &mut [f64]) -> bool;
        pub fn cad_volume(shape: &TopoDS_Shape) -> f64;
        pub fn cad_area(shape: &TopoDS_Shape) -> f64;
        pub fn cad_bbox(shape: &TopoDS_Shape, out: &mut [f64]) -> bool;
        pub fn cad_is_valid(shape: &TopoDS_Shape) -> bool;
        pub fn cad_shape_type(shape: &TopoDS_Shape) -> i32;
        pub fn cad_copy(shape: &TopoDS_Shape) -> UniquePtr<TopoDS_Shape>;

        pub fn cad_write_step(shape: &TopoDS_Shape, path: &str) -> bool;
        pub fn cad_read_step(path: &str) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_write_stl(shape: &TopoDS_Shape, path: &str, ascii: bool) -> bool;

        pub fn cad_shell(shape: &TopoDS_Shape, faces: &TopoDS_Shape, thickness: f64) -> UniquePtr<TopoDS_Shape>;
        /// kind 0: 12 affine coefficients (row-major 3x4); kind 1: mirror plane (point, normal).
        pub fn cad_transform(shape: &TopoDS_Shape, kind: i32, m: &[f64]) -> UniquePtr<TopoDS_Shape>;
    }
}

// ===== naming =====
/// History-based face naming (see the `naming` section of include/cad360.hxx for the `hist`
/// layout): each function mirrors its plain counterpart and also reports, per result face, the
/// input sub-shapes it was made from. The bridge lives in `cad360_naming.rs` (cxx rejects a type
/// declared in two bridges of one file); its functions are re-exported here.
pub use crate::cad360_naming::{cad_blend_named, cad_boolean_named, cad_prism_named, cad_revol_named, cad_shell_named, cad_subshape_indices, cad_transform_named};

// ===== surface =====
/// Surface and mesh features (see the `surface` section of include/cad360.hxx). The bridge lives in
/// `cad360_surface.rs` (cxx rejects a type declared in two bridges of one file).
pub use crate::cad360_surface::{cad_children, cad_fill, cad_make_wires, cad_mesh_face, cad_mesh_to_brep, cad_offset_surface, cad_sew, cad_thicken};

// ===== inspect =====
/// INSPECT panel: minimum distance and mass properties (see the `inspect` section of
/// include/cad360.hxx). The bridge lives in `cad360_inspect.rs` (cxx rejects a type declared in two
/// bridges of one file).
pub use crate::cad360_inspect::{cad_mass_props, cad_min_distance};
