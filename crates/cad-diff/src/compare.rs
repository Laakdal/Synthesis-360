//! Comparator with explicit tolerances.
//!
//! Outcome per case and kernel pair:
//! * `MATCH` — every field both sides report agrees (volume must be among them);
//! * `MISMATCH` — a mass property, the body/lump/solid structure, validity or the *set* of
//!   distinct face types differs (each difference listed as `field, a, b`);
//! * `TOPOLOGY_CONVENTION` — volume and area agree but entity counts or type multiplicities
//!   differ (e.g. OCCT's seam and degenerate pole edges vs a kernel without them): a modelling
//!   convention, reported but not treated as a bug;
//! * `ONE_SIDED_FAILURE` — one kernel built the case, the other failed;
//! * `BOTH_FAILED_SAME` — both kernels failed;
//! * `NOT_IMPLEMENTED` — a kernel does not implement the operation (or the mass properties) yet;
//! * `NOT_COMPARED` — one side has no row (e.g. no ASM reference).
//!
//! A field that is null on either side (e.g. ASM's centroid and inertia for the tiny box, a Fusion
//! mass-property failure) is not compared and is not a mismatch.

use crate::result::{KERNEL_ASM, KernelResult, NOT_IMPLEMENTED};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Relative volume/area/inertia tolerance between two exact B-rep kernels (OCCT BRepGProp and
/// cad-geom both integrate the exact surfaces; their quadrature errors on analytic primitives
/// are far below 1e-9, so 1e-6 leaves room for different parameterisations and booleans).
pub const REL_TOL_EXACT: f64 = 1e-6;
/// Relative tolerance when one side is ASM with an exact mass path. The probe records
/// `mass_path = base_feature` (Fusion PhysicalProperties at VeryHighCalculationAccuracy on the
/// B-rep, not a mesh), and the measured OCCT-vs-ASM agreement on every primitive and boolean
/// with equal topology is about 2e-8 relative or better (box∩sphere: 6383.716137 vs 6383.716274),
/// so the exact-kernel tolerance applies; anything looser would hide the ~9e-6 STEP-export loss
/// seen on `bool.intersect.box_sphere`.
pub const REL_TOL_ASM: f64 = 1e-6;
/// Relative tolerance when either side's mass properties come from a mesh (or a low/medium
/// accuracy setting): tessellation error dominates, typically 1e-4..1e-3 at default chords.
pub const REL_TOL_MESH: f64 = 1e-3;
/// Centroid tolerance = model size (bbox diagonal) × this factor, for exact kernels...
pub const CENTROID_REL_SIZE_EXACT: f64 = 1e-6;
/// ...and when one side is ASM (same evidence as `REL_TOL_ASM`). Mesh-based pairs use
/// `REL_TOL_MESH` for the centroid factor too.
pub const CENTROID_REL_SIZE_ASM: f64 = 1e-6;
/// Absolute centroid floor (mm) so the 0.01 mm box is not judged at round-off level.
pub const CENTROID_ABS_FLOOR_MM: f64 = 1e-9;
/// Bounding boxes are informational only: OCCT's `BRepBndLib::Add` enlarges by the shape
/// tolerances (~1e-7 mm per side; 1e-5 relative on the 0.01 mm box) and is not tight on curved
/// faces, and cad-geom exposes no bbox. Differences above this relative size are noted.
pub const BBOX_NOTE_REL: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Outcome {
    Match,
    Mismatch,
    TopologyConvention,
    OneSidedFailure,
    BothFailedSame,
    NotImplemented,
    NotCompared,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Match => "MATCH",
            Outcome::Mismatch => "MISMATCH",
            Outcome::TopologyConvention => "TOPOLOGY_CONVENTION",
            Outcome::OneSidedFailure => "ONE_SIDED_FAILURE",
            Outcome::BothFailedSame => "BOTH_FAILED_SAME",
            Outcome::NotImplemented => "NOT_IMPLEMENTED",
            Outcome::NotCompared => "NOT_COMPARED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDiff {
    pub field: String,
    pub a: String,
    pub b: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    pub case_id: String,
    pub a: String,
    pub b: String,
    pub outcome: Outcome,
    pub diffs: Vec<FieldDiff>,
    /// Fields actually compared (present on both sides).
    pub compared: Vec<String>,
    pub rel_tol: f64,
    pub centroid_tol_mm: Option<f64>,
    pub notes: Vec<String>,
}

impl Comparison {
    /// `MISMATCH(volume, 1, 2; faces, 3, 4)`-style summary.
    pub fn summary(&self) -> String {
        if self.diffs.is_empty()
            || matches!(
                self.outcome,
                Outcome::NotImplemented | Outcome::NotCompared | Outcome::BothFailedSame
            )
        {
            return self.outcome.label().into();
        }
        let d: Vec<String> = self
            .diffs
            .iter()
            .map(|d| format!("{}, {}, {}", d.field, d.a, d.b))
            .collect();
        format!("{}({})", self.outcome.label(), d.join("; "))
    }
}

fn mesh_based(r: &KernelResult) -> bool {
    r.mass_path.as_deref().is_some_and(|p| {
        let p = p.to_ascii_lowercase();
        ["mesh", "tessell", "low", "medium"]
            .iter()
            .any(|k| p.contains(k))
    })
}

/// Relative mass tolerance and centroid size factor for a pair.
pub fn tolerances_for(a: &KernelResult, b: &KernelResult) -> (f64, f64) {
    if mesh_based(a) || mesh_based(b) {
        (REL_TOL_MESH, REL_TOL_MESH)
    } else if a.kernel == KERNEL_ASM || b.kernel == KERNEL_ASM {
        (REL_TOL_ASM, CENTROID_REL_SIZE_ASM)
    } else {
        (REL_TOL_EXACT, CENTROID_REL_SIZE_EXACT)
    }
}

fn rel_close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * a.abs().max(b.abs()).max(f64::MIN_POSITIVE)
}

fn has_not_implemented_note(r: &KernelResult) -> bool {
    r.notes.iter().any(|n| n.contains(NOT_IMPLEMENTED))
}

fn fmt<T: std::fmt::Debug>(v: &T) -> String {
    format!("{v:?}")
}

/// Compares two rows of the same case.
pub fn compare(a: &KernelResult, b: &KernelResult) -> Comparison {
    let (rel, csize) = tolerances_for(a, b);
    let mut c = Comparison {
        case_id: a.case_id.clone(),
        a: a.kernel.clone(),
        b: b.kernel.clone(),
        outcome: Outcome::Match,
        diffs: Vec::new(),
        compared: Vec::new(),
        rel_tol: rel,
        centroid_tol_mm: None,
        notes: Vec::new(),
    };
    if a.is_not_implemented() || b.is_not_implemented() {
        c.outcome = Outcome::NotImplemented;
        let who: Vec<&str> = [a, b]
            .iter()
            .filter(|r| r.is_not_implemented())
            .map(|r| r.kernel.as_str())
            .collect();
        c.notes
            .push(format!("not implemented in {}", who.join(", ")));
        return c;
    }
    match (a.ok, b.ok) {
        (false, false) => {
            c.outcome = Outcome::BothFailedSame;
            c.notes.push(format!(
                "{}: {:?}; {}: {:?}",
                a.kernel, a.error, b.kernel, b.error
            ));
            return c;
        }
        (true, false) | (false, true) => {
            c.outcome = Outcome::OneSidedFailure;
            c.diffs.push(FieldDiff {
                field: "ok".into(),
                a: format!("{} {}", a.ok, a.error.clone().unwrap_or_default())
                    .trim()
                    .into(),
                b: format!("{} {}", b.ok, b.error.clone().unwrap_or_default())
                    .trim()
                    .into(),
            });
            return c;
        }
        _ => {}
    }

    // substantive differences (bugs until shown otherwise) vs convention differences
    let mut hard: Vec<FieldDiff> = Vec::new();
    let mut soft: Vec<FieldDiff> = Vec::new();

    macro_rules! scalar {
        ($f:ident) => {
            if let (Some(x), Some(y)) = (a.$f, b.$f) {
                c.compared.push(stringify!($f).into());
                if !rel_close(x, y, rel) {
                    hard.push(FieldDiff {
                        field: stringify!($f).into(),
                        a: x.to_string(),
                        b: y.to_string(),
                    });
                }
            }
        };
    }
    scalar!(volume);
    scalar!(area);

    if let (Some(x), Some(y)) = (a.centroid, b.centroid) {
        let size = a.size().or(b.size()).unwrap_or(1.0);
        let tol = (size * csize).max(CENTROID_ABS_FLOOR_MM);
        c.centroid_tol_mm = Some(tol);
        c.compared.push("centroid".into());
        let d = ((x[0] - y[0]).powi(2) + (x[1] - y[1]).powi(2) + (x[2] - y[2]).powi(2)).sqrt();
        if d > tol {
            hard.push(FieldDiff {
                field: "centroid".into(),
                a: fmt(&x),
                b: fmt(&y),
            });
        }
    }
    if let (Some(x), Some(y)) = (a.inertia_centroid, b.inertia_centroid) {
        c.compared.push("inertia_centroid".into());
        let scale = x
            .iter()
            .chain(y.iter())
            .flatten()
            .fold(0.0_f64, |m, v| m.max(v.abs()));
        let bad = (0..3).any(|i| {
            (0..3).any(|j| (x[i][j] - y[i][j]).abs() > rel * scale.max(f64::MIN_POSITIVE))
        });
        if bad {
            hard.push(FieldDiff {
                field: "inertia_centroid".into(),
                a: fmt(&x),
                b: fmt(&y),
            });
        }
    }

    macro_rules! exact {
        ($f:ident, $into:ident) => {
            if let (Some(x), Some(y)) = (&a.$f, &b.$f) {
                c.compared.push(stringify!($f).into());
                if x != y {
                    $into.push(FieldDiff {
                        field: stringify!($f).into(),
                        a: fmt(x),
                        b: fmt(y),
                    });
                }
            }
        };
    }
    exact!(bodies, hard);
    exact!(lumps, hard);
    exact!(solid, hard);
    exact!(valid, hard);
    exact!(shells, soft);
    exact!(faces, soft);
    exact!(edges, soft);
    exact!(vertices, soft);
    exact!(loops, soft);
    exact!(degenerate_edges_literal, soft);
    exact!(edge_types, soft);
    // kernel-native degenerate and seam counts follow each kernel's convention (ASM has no seams
    // and no pole edges): recorded, never a comparison criterion
    for (name, x, y) in [
        ("degenerate_edges", a.degenerate_edges, b.degenerate_edges),
        ("seam_edges", a.seam_edges, b.seam_edges),
    ] {
        if let (Some(x), Some(y)) = (x, y)
            && x != y
        {
            c.notes.push(format!(
                "{name} (informational): {} {x}, {} {y}",
                a.kernel, b.kernel
            ));
        }
    }
    if let (Some(x), Some(y)) = (&a.face_types, &b.face_types) {
        c.compared.push("face_types".into());
        if x != y {
            let sx: BTreeSet<&String> = x.iter().collect();
            let sy: BTreeSet<&String> = y.iter().collect();
            let d = FieldDiff {
                field: "face_types".into(),
                a: fmt(x),
                b: fmt(y),
            };
            if sx == sy { soft.push(d) } else { hard.push(d) }
        }
    }

    if let (Some(x), Some(y)) = (a.bbox, b.bbox) {
        let size = a.size().unwrap_or(1.0);
        let worst = (0..2)
            .flat_map(|i| (0..3).map(move |k| (x[i][k] - y[i][k]).abs()))
            .fold(0.0_f64, f64::max);
        if worst > BBOX_NOTE_REL * size {
            c.notes.push(format!(
                "bbox differs by up to {worst:.3e} mm (informational)"
            ));
        }
    }

    let mass_compared =
        c.compared.iter().any(|f| f == "volume") && c.compared.iter().any(|f| f == "area");
    c.outcome = if !hard.is_empty() {
        Outcome::Mismatch
    } else if mass_compared {
        if soft.is_empty() {
            Outcome::Match
        } else {
            Outcome::TopologyConvention
        }
    } else if has_not_implemented_note(a) || has_not_implemented_note(b) {
        c.notes.push(if soft.is_empty() {
            "mass properties not implemented on one side; compared topology agrees".to_string()
        } else {
            "mass properties not implemented on one side; topology differs (listed) and cannot be classified as a convention without mass agreement".to_string()
        });
        Outcome::NotImplemented
    } else {
        c.notes.push("volume/area missing on one side".into());
        Outcome::Mismatch
    };
    c.diffs = hard.into_iter().chain(soft).collect();
    c
}

/// Compares optional rows (a missing side gives `NOT_COMPARED`).
pub fn compare_opt(
    case_id: &str,
    a: Option<&KernelResult>,
    b: Option<&KernelResult>,
    a_name: &str,
    b_name: &str,
) -> Comparison {
    match (a, b) {
        (Some(a), Some(b)) => compare(a, b),
        _ => Comparison {
            case_id: case_id.into(),
            a: a_name.into(),
            b: b_name.into(),
            outcome: Outcome::NotCompared,
            diffs: Vec::new(),
            compared: Vec::new(),
            rel_tol: 0.0,
            centroid_tol_mm: None,
            notes: vec![format!(
                "no {} row",
                if a.is_none() { a_name } else { b_name }
            )],
        },
    }
}
