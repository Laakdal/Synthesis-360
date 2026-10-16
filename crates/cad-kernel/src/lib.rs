//! CAD360 geometry kernel integration.
//!
//! Evaluates the document's feature history into exact B-rep solids with Open CASCADE
//! Technology (statically linked through the vendored `opencascade-sys` bindings and the
//! exception-safe `cad360` shim), and provides display meshes, persistent references and
//! STEP/STL exchange. Contains no UI code.

pub mod body;
pub mod exchange;
pub mod frame;
pub mod inspect;
pub mod mesh;
pub mod naming;
pub mod occ;
pub mod recompute;
pub mod refs;
pub mod surface;

pub use body::{new_body_with, Body, BodyKind, BodyMesh, EdgeInfo, FaceInfo, Topology};
pub use frame::Frame;
pub use recompute::{edge_joint_frame, face_joint_frame, joint_frame, joint_moves_b, mate_transform, store_solved_sketches, OVER_CONSTRAINED, Health, Kernel, Model, SketchResult};
