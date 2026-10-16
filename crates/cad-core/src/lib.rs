//! CAD360 core: document model, parameters and expressions, feature history, dependency graph,
//! undo/redo and native persistence. Pure Rust; no geometry kernel or UI dependency.

pub mod document;
pub mod expr;
pub mod history;
pub mod ops;

pub use cad_sketch as sketch;
pub use document::*;
pub use expr::Quantity;
