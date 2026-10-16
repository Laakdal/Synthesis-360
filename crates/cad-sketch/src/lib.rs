//! 2D sketch engine: entities, constraints, a numerical constraint solver with degree-of-freedom
//! analysis, and profile (closed region) detection. Pure Rust; no CAD-kernel dependency.

pub mod interact;
pub mod model;
pub mod modify;
pub mod profiles;
pub mod shapes;
pub mod solver;

pub use model::*;
pub use profiles::{find_profiles, Loop, Profile, Segment};
pub use solver::{measure, solve, would_overconstrain, DimensionValues, SolveReport, SolveStatus};
