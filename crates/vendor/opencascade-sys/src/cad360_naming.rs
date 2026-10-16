//! CAD360 kernel shim, naming section (see the `// ===== naming =====` section of
//! include/cad360.hxx for the `hist` layout): each function mirrors its plain counterpart in
//! `cad360` and also reports, per result face, the input sub-shapes it was made from. Re-exported
//! from `cad360`. A separate bridge file because cxx rejects a type declared in two bridges of
//! one file.
pub use inner::*;

#[cxx::bridge]
mod inner {
    unsafe extern "C++" {
        include!("opencascade-sys/include/cad360.hxx");

        type TopoDS_Shape = crate::topo_ds::TopoDS_Shape;

        pub fn cad_prism_named(face: &TopoDS_Shape, dx: f64, dy: f64, dz: f64, hist: &mut Vec<i32>) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_revol_named(face: &TopoDS_Shape, ox: f64, oy: f64, oz: f64, ax: f64, ay: f64, az: f64, angle: f64, hist: &mut Vec<i32>) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_boolean_named(a: &TopoDS_Shape, b: &TopoDS_Shape, op: i32, hist: &mut Vec<i32>) -> UniquePtr<TopoDS_Shape>;
        /// fillet = true: constant-radius fillet; false: equal-distance chamfer.
        pub fn cad_blend_named(shape: &TopoDS_Shape, edges: &TopoDS_Shape, v: f64, fillet: bool, hist: &mut Vec<i32>) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_shell_named(shape: &TopoDS_Shape, faces: &TopoDS_Shape, thickness: f64, hist: &mut Vec<i32>) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_transform_named(shape: &TopoDS_Shape, kind: i32, m: &[f64], hist: &mut Vec<i32>) -> UniquePtr<TopoDS_Shape>;
        pub fn cad_subshape_indices(parent: &TopoDS_Shape, child: &TopoDS_Shape, kind: i32, out: &mut Vec<i32>) -> bool;
    }
}
