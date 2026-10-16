//! CAD360 kernel shim, inspect section (see the `// ===== inspect =====` section of
//! include/cad360.hxx): minimum distance and mass properties. Re-exported from `cad360`. A
//! separate bridge file because cxx rejects a type declared in two bridges of one file.
pub use inner::*;

#[cxx::bridge]
mod inner {
    unsafe extern "C++" {
        include!("opencascade-sys/include/cad360.hxx");

        type TopoDS_Shape = crate::topo_ds::TopoDS_Shape;

        /// out = [distance, ax, ay, az, bx, by, bz, inner (1 = one solid inside the other)].
        pub fn cad_min_distance(a: &TopoDS_Shape, b: &TopoDS_Shape, out: &mut [f64]) -> bool;
        /// out = [volume, area, cx, cy, cz] (unit density; surface centre when no volume).
        pub fn cad_mass_props(s: &TopoDS_Shape, out: &mut [f64]) -> bool;
    }
}
