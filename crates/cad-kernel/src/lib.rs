//! CAD360 geometry kernel integration.
//!
//! Evaluates the document's feature history into exact B-rep solids with Open CASCADE
//! Technology (statically linked through the vendored `opencascade-sys` bindings and the
//! exception-safe `cad360` shim), and provides display meshes, persistent references and
//! STEP/STL exchange. Contains no UI code.

pub mod body;
pub mod exchange;
pub mod frame;
pub mod occ;
pub mod recompute;
pub mod refs;

pub use body::{Body, BodyMesh, EdgeInfo, FaceInfo, Topology};
pub use frame::Frame;
pub use recompute::{store_solved_sketches, Health, Kernel, Model, SketchResult};
