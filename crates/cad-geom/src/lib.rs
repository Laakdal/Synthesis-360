//! CAD360's own B-rep kernel, written from published geometric methods (no OCCT, no ASM code).
//! It grows next to the OCCT-backed `cad-kernel`, which the application keeps using; the
//! `cad-diff` crate runs the same cases through both and against recorded ASM reference data.
//!
//! Units are millimetres and radians throughout. Layering: `math`/`tolerance` ← `curve`,
//! `surface`, `nurbs` ← `topology` ← `primitives`, `mass`, `validate`, `tess`; `predicates` ←
//! `boolean` (polyhedra; `boolean_quadric` for planes, cylinders and spheres); `profile` ←
//! `extrude`, `revolve` (planar profiles swept into solids); `blend` (chamfer, fillet), `shell`.

pub mod bbox;
pub mod blend;
pub mod boolean;
mod boolean_quadric;
pub mod curve;
pub mod extrude;
pub mod intersect;
pub mod mass;
pub mod math;
pub mod nurbs;
pub mod param;
pub mod predicates;
pub mod primitives;
pub mod profile;
pub mod project;
pub mod revolve;
pub mod shell;
pub mod surface;
pub mod tess;
pub mod tolerance;
pub mod topology;
pub mod transform;
mod triangulate;
pub mod validate;

pub use bbox::Aabb;
pub use blend::{chamfer, fillet};
pub use boolean::{boolean, boolean_default, BoolOp};
pub use curve::Curve;
pub use extrude::extrude;
pub use profile::{Profile, Region, Seg, P2};
pub use revolve::revolve;
pub use shell::shell;
pub use mass::MassProperties;
pub use math::{Frame, P3, V3};
pub use nurbs::{NurbsCurve, NurbsSurface};
pub use surface::Surface;
pub use tolerance::Tolerance;
pub use topology::{Brep, CoedgeId, EdgeId, FaceId, LoopId, ShellId, SolidId, VertexId};
pub use validate::{validate, Issue, ValidationReport};

/// Errors from kernel operations (invalid input, unsupported case, numerical failure).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum GeomError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not supported: {0}")]
    Unsupported(String),
    #[error("numerical failure: {0}")]
    Numerical(String),
}

pub type GResult<T> = Result<T, GeomError>;
