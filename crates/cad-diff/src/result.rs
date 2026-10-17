//! Normalised per-kernel result of one case. Field names and units (mm, mm², mm³, mm⁵) follow the
//! ASM `reference.json` case schema so the three kernels line up column by column.

use serde::{Deserialize, Serialize};

pub const KERNEL_RUST: &str = "cad-geom";
pub const KERNEL_OCCT: &str = "occt";
pub const KERNEL_ASM: &str = "asm";

/// Error string for operations a kernel does not implement yet.
pub const NOT_IMPLEMENTED: &str = "NOT_IMPLEMENTED";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KernelResult {
    pub kernel: String,
    pub case_id: String,
    /// The case was built (a shape exists).
    pub ok: bool,
    pub error: Option<String>,
    /// Number of separate bodies in the result.
    pub bodies: Option<u32>,
    /// The result is (contains) a closed solid.
    pub solid: Option<bool>,
    pub lumps: Option<u32>,
    pub shells: Option<u32>,
    pub faces: Option<u32>,
    pub edges: Option<u32>,
    pub vertices: Option<u32>,
    pub loops: Option<u32>,
    /// Normalised surface type per face, sorted (multiset).
    pub face_types: Option<Vec<String>>,
    /// Normalised curve type per non-degenerate edge, sorted (multiset).
    pub edge_types: Option<Vec<String>>,
    /// Kernel-native degenerate edges (no geometry / zero length: OCCT `Degenerated`, cad-geom
    /// curve-less edges, ASM `isDegenerate`). Informational: kernels differ by convention.
    pub degenerate_edges: Option<u32>,
    /// Edges used twice by one face. Informational (ASM has no seam edges).
    pub seam_edges: Option<u32>,
    /// The ASM probe's literal definition, computed identically for every kernel: edges whose
    /// start vertex equals their end vertex, OR of zero length, OR without geometry (so closed
    /// full circles count). ASM's `degenerate_edges` field holds this number.
    pub degenerate_edges_literal: Option<u32>,
    /// mm³ (unit density mass).
    pub volume: Option<f64>,
    /// Total boundary area, mm².
    pub area: Option<f64>,
    pub centroid: Option<[f64; 3]>,
    /// Inertia tensor about the centroid, unit density, world axes (mm⁵).
    pub inertia_centroid: Option<[[f64; 3]; 3]>,
    /// [[minx, miny, minz], [maxx, maxy, maxz]]
    pub bbox: Option<[[f64; 3]; 2]>,
    pub valid: Option<bool>,
    pub max_edge_tolerance: Option<f64>,
    pub max_vertex_tolerance: Option<f64>,
    /// How mass properties were obtained (exact integration, mesh, ...).
    pub mass_path: Option<String>,
    pub notes: Vec<String>,
}

impl KernelResult {
    pub fn new(kernel: &str, case_id: &str) -> Self {
        Self {
            kernel: kernel.into(),
            case_id: case_id.into(),
            ..Default::default()
        }
    }

    pub fn failed(kernel: &str, case_id: &str, error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::new(kernel, case_id)
        }
    }

    pub fn not_implemented(kernel: &str, case_id: &str, note: impl Into<String>) -> Self {
        let mut r = Self::failed(kernel, case_id, NOT_IMPLEMENTED);
        r.notes.push(note.into());
        r
    }

    pub fn is_not_implemented(&self) -> bool {
        self.error.as_deref() == Some(NOT_IMPLEMENTED)
    }

    /// Characteristic model size (bbox diagonal, else cube root of the volume).
    pub fn size(&self) -> Option<f64> {
        if let Some([lo, hi]) = self.bbox {
            let d = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2))
                .sqrt();
            if d > 0.0 {
                return Some(d);
            }
        }
        self.volume.filter(|v| *v > 0.0).map(f64::cbrt)
    }
}

/// Canonical surface type name. Accepts OCCT GeomAbs names, cad-geom names and Fusion/ASM
/// names (`Plane`, `PlaneSurfaceType`, `adsk.core.Cylinder`, `NurbsSurface`, ...).
pub fn normalize_surface_type(raw: &str) -> String {
    let s = raw.rsplit('.').next().unwrap_or(raw).to_ascii_lowercase();
    let s = s
        .trim_end_matches("surfacetype")
        .trim_end_matches("surface")
        .trim_end_matches("type");
    let s = s.trim_end_matches('_').trim_start_matches("geomabs_");
    match s {
        "plane" | "planar" => "plane",
        "cylinder" | "cylindrical" | "ellipticalcylinder" => "cylinder",
        "cone" | "conical" | "ellipticalcone" => "cone",
        "sphere" | "spherical" => "sphere",
        "torus" | "toroidal" => "torus",
        "nurbs" | "bspline" | "bsplinesurface" => "bspline",
        "bezier" | "beziersurface" => "bezier",
        "revolution" | "surfaceofrevolution" => "revolution",
        "extrusion" | "surfaceofextrusion" => "extrusion",
        "offset" | "offsetsurface" => "offset",
        "" => "other",
        other => return other.to_string(),
    }
    .to_string()
}

/// Canonical curve type name (arcs are circles: a trimmed circle is still a circle).
pub fn normalize_curve_type(raw: &str) -> String {
    let s = raw.rsplit('.').next().unwrap_or(raw).to_ascii_lowercase();
    let s = s
        .trim_end_matches("curvetype")
        .trim_end_matches("type")
        .trim_end_matches("3d");
    let s = s.trim_end_matches('_').trim_start_matches("geomabs_");
    match s {
        "line" | "infiniteline" | "linesegment" => "line",
        "circle" | "arc" | "circulararc" => "circle",
        "ellipse" | "ellipticalarc" => "ellipse",
        "hyperbola" => "hyperbola",
        "parabola" => "parabola",
        "nurbs" | "nurbscurve" | "bspline" | "bsplinecurve" => "bspline",
        "bezier" | "beziercurve" => "bezier",
        "offset" | "offsetcurve" => "offset",
        "degenerate" | "degenerated" | "none" | "null" => "degenerate",
        "" => "other",
        other => return other.to_string(),
    }
    .to_string()
}

/// OCCT `GeomAbs_SurfaceType` ordinal → canonical name.
pub fn occt_surface_name(t: i32) -> &'static str {
    match t {
        0 => "plane",
        1 => "cylinder",
        2 => "cone",
        3 => "sphere",
        4 => "torus",
        5 => "bezier",
        6 => "bspline",
        7 => "revolution",
        8 => "extrusion",
        9 => "offset",
        _ => "other",
    }
}

/// OCCT `GeomAbs_CurveType` ordinal → canonical name (-1 = degenerated edge, from the shim).
pub fn occt_curve_name(t: i32) -> &'static str {
    match t {
        -1 => "degenerate",
        0 => "line",
        1 => "circle",
        2 => "ellipse",
        3 => "hyperbola",
        4 => "parabola",
        5 => "bezier",
        6 => "bspline",
        7 => "offset",
        _ => "other",
    }
}
