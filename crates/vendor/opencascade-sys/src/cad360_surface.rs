//! CAD360 kernel shim, surface section (see the `// ===== surface =====` section of
//! include/cad360.hxx): wires from curves, sewing, offset, thicken, patch filling and mesh faces.
//! Re-exported from `cad360`. A separate bridge file because cxx rejects a type declared in two
//! bridges of one file.
pub use inner::*;

#[cxx::bridge]
mod inner {
    unsafe extern "C++" {
        include!("opencascade-sys/include/cad360.hxx");

        type TopoDS_Shape = crate::topo_ds::TopoDS_Shape;

        /// Compound of wires from curve chains (kind 0 line, 1 arc s-m-e, 2 circle centre/point/normal).
        pub fn cad_make_wires(data: &[f64]) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_children(shape: &TopoDS_Shape) -> UniquePtr<CxxVector<TopoDS_Shape>>;
        pub fn cad_sew(shape: &TopoDS_Shape, tolerance: f64, make_solid: bool) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_offset_surface(shape: &TopoDS_Shape, distance: f64) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_thicken(shape: &TopoDS_Shape, thickness: f64) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_fill(edges: &TopoDS_Shape) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_mesh_face(tris: &[f64]) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_mesh_to_brep(tris: &[f64], tolerance: f64) -> UniquePtr<TopoDS_Shape>;
    }
}
