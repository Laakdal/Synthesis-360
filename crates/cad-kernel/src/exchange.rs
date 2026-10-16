//! Geometry exchange. STEP (ISO 10303-21) carries exact B-rep solids; STL carries a triangle mesh.
//! Neither preserves the parametric history — only the native `.c360` document does (Fusion's
//! documentation states the same for its exports: "An exported design does not maintain
//! associativity with the original Fusion design.").

use crate::body::Body;
use crate::occ::{KResult, KernelError, Shape};

fn compound(bodies: &[&Body]) -> KResult<Shape> {
    if bodies.is_empty() {
        return Err(KernelError("There are no bodies to export.".into()));
    }
    let shapes: Vec<&Shape> = bodies.iter().map(|b| &b.shape).collect();
    Shape::compound(&shapes)
}

pub fn export_step(bodies: &[&Body], path: &str) -> KResult<()> {
    compound(bodies)?.write_step(path)
}

pub fn export_stl(bodies: &[&Body], path: &str, ascii: bool) -> KResult<()> {
    let c = compound(bodies)?;
    let bb = c.bbox().unwrap_or([0.0; 6]);
    let diag = ((bb[3] - bb[0]).powi(2) + (bb[4] - bb[1]).powi(2) + (bb[5] - bb[2]).powi(2)).sqrt().max(1.0);
    c.mesh((diag * 0.0008).clamp(0.002, 0.2), 0.25);
    c.write_stl(path, ascii)
}

/// Reads a STEP file's text for an Import feature (the data is embedded in the document so the
/// project stays self-contained).
pub fn read_step_text(path: &str) -> KResult<String> {
    let text = std::fs::read_to_string(path).map_err(|e| KernelError(format!("Could not read {path}: {e}")))?;
    if !text.trim_start().starts_with("ISO-10303-21") {
        return Err(KernelError("The file is not a STEP (ISO 10303-21) file.".into()));
    }
    Ok(text)
}
