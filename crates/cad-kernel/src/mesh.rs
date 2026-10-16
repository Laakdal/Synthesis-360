//! Triangle meshes: STL reading (binary and ASCII) for INSERT ▸ Insert Mesh, and STL export with
//! a selectable refinement for UTILITIES ▸ Make ▸ 3D Print.

use crate::body::Body;
use crate::occ::{KResult, KernelError, Shape};

/// Triangles of an STL file as 9 coordinates per triangle (facet normals are recomputed from the
/// vertex order, as every CAD importer does).
pub fn parse_stl(bytes: &[u8]) -> Result<Vec<f64>, String> {
    let binary_len = |n: usize| 84 + 50 * n;
    if bytes.len() >= 84 {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if bytes.len() == binary_len(n) && n > 0 {
            let mut out = Vec::with_capacity(9 * n);
            for t in 0..n {
                let base = 84 + 50 * t + 12;
                for k in 0..9 {
                    let o = base + 4 * k;
                    out.push(f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]) as f64);
                }
            }
            return Ok(out);
        }
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "The file is not an STL file (binary size mismatch, not ASCII).".to_string())?;
    if !text.trim_start().to_ascii_lowercase().starts_with("solid") {
        return Err("The file is not an STL file.".into());
    }
    let mut verts: Vec<f64> = Vec::new();
    let mut it = text.split_ascii_whitespace();
    while let Some(w) = it.next() {
        if w.eq_ignore_ascii_case("vertex") {
            for _ in 0..3 {
                let v: f64 = it.next().and_then(|s| s.parse().ok()).ok_or("Malformed vertex in the ASCII STL file.")?;
                verts.push(v);
            }
        }
    }
    if verts.is_empty() || verts.len() % 9 != 0 {
        return Err("The STL file contains no complete triangles.".into());
    }
    Ok(verts)
}

/// Area of a triangle soup (9 coordinates per triangle).
pub fn triangles_area(tris: &[f64]) -> f64 {
    tris.chunks_exact(9)
        .map(|c| {
            let u = [c[3] - c[0], c[4] - c[1], c[5] - c[2]];
            let v = [c[6] - c[0], c[7] - c[1], c[8] - c[2]];
            let x = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            0.5 * (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt()
        })
        .sum()
}

/// Meshing refinement of an STL export (Fusion's 3D Print / Mesh Refinement: Low / Medium / High).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Refinement {
    Coarse,
    #[default]
    Medium,
    Fine,
}

impl Refinement {
    pub const ALL: [Refinement; 3] = [Refinement::Coarse, Refinement::Medium, Refinement::Fine];
    pub fn label(self) -> &'static str {
        match self {
            Refinement::Coarse => "Coarse",
            Refinement::Medium => "Medium",
            Refinement::Fine => "Fine",
        }
    }
    /// (chordal deflection as a fraction of the bounding-box diagonal, angular deflection rad).
    pub fn tolerances(self) -> (f64, f64) {
        match self {
            Refinement::Coarse => (0.004, 0.6),
            // the plain STL export's values
            Refinement::Medium => (0.0008, 0.25),
            Refinement::Fine => (0.0001, 0.1),
        }
    }
}

/// Writes the bodies as one STL file meshed with the given refinement.
pub fn export_stl_refined(bodies: &[&Body], path: &str, ascii: bool, refinement: Refinement) -> KResult<()> {
    if bodies.is_empty() {
        return Err(KernelError("There are no bodies to export.".into()));
    }
    let shapes: Vec<&Shape> = bodies.iter().map(|b| &b.shape).collect();
    let c = Shape::compound(&shapes)?;
    let bb = c.bbox().unwrap_or([0.0; 6]);
    let diag = ((bb[3] - bb[0]).powi(2) + (bb[4] - bb[1]).powi(2) + (bb[5] - bb[2]).powi(2)).sqrt().max(1.0);
    let (k, ang) = refinement.tolerances();
    c.mesh((diag * k).clamp(0.0005, 2.0), ang);
    c.write_stl(path, ascii)
}

/// Number of triangles in an STL file written by [`export_stl_refined`] (binary or ASCII).
pub fn stl_triangle_count(bytes: &[u8]) -> usize {
    parse_stl(bytes).map(|t| t.len() / 9).unwrap_or(0)
}
