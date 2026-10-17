//! The shared case catalogue (`research/cases.json`). Units: mm.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Solid specification, tagged by `kind` exactly as in `research/cases.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Spec {
    /// Axis-aligned box: corner + edge lengths.
    Box {
        corner: [f64; 3],
        size: [f64; 3],
    },
    /// Right circular cylinder (r0 == r1).
    Cylinder {
        base: [f64; 3],
        axis: [f64; 3],
        r0: f64,
        r1: f64,
        h: f64,
    },
    /// Cone (r1 == 0) or frustum.
    Cone {
        base: [f64; 3],
        axis: [f64; 3],
        r0: f64,
        r1: f64,
        h: f64,
    },
    Sphere {
        center: [f64; 3],
        r: f64,
    },
    Torus {
        center: [f64; 3],
        axis: [f64; 3],
        major: f64,
        minor: f64,
    },
    /// `tools` are applied to `target` in order with `op` (union, cut, intersect).
    Boolean {
        op: BoolOp,
        target: Box<Spec>,
        tools: Vec<Spec>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoolOp {
    Union,
    Cut,
    Intersect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Case {
    pub id: String,
    #[serde(flatten)]
    pub spec: Spec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalogue {
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub version: u32,
    pub cases: Vec<Case>,
}

impl Catalogue {
    pub fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("invalid case catalogue: {e}"))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))?;
        Self::parse(&text)
    }
}

impl Spec {
    pub fn is_boolean(&self) -> bool {
        matches!(self, Spec::Boolean { .. })
    }

    /// Closed-form volume for primitives (None for booleans).
    pub fn closed_form_volume(&self) -> Option<f64> {
        use std::f64::consts::PI;
        match *self {
            Spec::Box { size, .. } => Some(size[0] * size[1] * size[2]),
            Spec::Cylinder { r0, h, .. } => Some(PI * r0 * r0 * h),
            Spec::Cone { r0, r1, h, .. } => Some(PI * h * (r0 * r0 + r0 * r1 + r1 * r1) / 3.0),
            Spec::Sphere { r, .. } => Some(4.0 / 3.0 * PI * r * r * r),
            Spec::Torus { major, minor, .. } => Some(2.0 * PI * PI * major * minor * minor),
            Spec::Boolean { .. } => None,
        }
    }
}

/// Repository root (two levels above this crate's manifest).
pub fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}
