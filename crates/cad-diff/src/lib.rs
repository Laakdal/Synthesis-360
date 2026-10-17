//! Differential testing harness for CAD360's kernels.
//!
//! Runs the shared case catalogues (`research/cases.json`, `research/feature-cases.json`) through
//! * `cad-geom`, the pure-Rust kernel under development ([`geom`]),
//! * Open CASCADE 7.8.1 through `cad-kernel` and the vendored `opencascade` crate ([`occt`]).
//!
//! It also loads the recorded ASM (Fusion) reference ([`asm`]); [`compare`] classifies every case and
//! kernel pair with explicit tolerances and [`report`] writes `research/VALIDATION_REPORT.md`
//! plus `research/evidence/diff/results.json`.

pub mod asm;
pub mod cases;
pub mod compare;
pub mod geom;
pub mod occt;
pub mod report;
pub mod result;

pub use compare::{Comparison, Outcome, compare};
pub use result::KernelResult;

use cases::Catalogue;
use serde::Serialize;
use std::path::Path;

/// Everything one run produced (serialised as `results.json`).
#[derive(Debug, Clone, Serialize)]
pub struct RunOutput {
    pub generated_utc: String,
    pub catalogue_version: u32,
    pub kernels: Vec<KernelInfo>,
    pub asm_status: String,
    pub asm_meta: Option<serde_json::Value>,
    pub bridge_fidelity: Vec<occt::BridgeCheck>,
    pub rust: Vec<KernelResult>,
    pub occt: Vec<KernelResult>,
    pub asm: Vec<KernelResult>,
    /// ASM results re-measured by OCCT from the probe's STEP exports.
    pub asm_step: Vec<KernelResult>,
    pub comparisons: Vec<Comparison>,
    pub tolerances: serde_json::Value,
    /// The catalogue the run used (not serialised; it is `research/cases.json`).
    #[serde(skip)]
    pub catalogue: Catalogue,
}

#[derive(Debug, Clone, Serialize)]
pub struct KernelInfo {
    pub name: String,
    pub version: String,
    pub how: String,
}

/// Recorded ASM experiments, in order: `research/evidence/<name>/reference.json` and `step/`.
pub const ASM_EXPERIMENTS: [&str; 2] = ["asm-v5", "asm-v6"];

/// Runs every case through cad-geom and OCCT, loads ASM, and compares all pairs.
pub fn run(root: &Path) -> Result<RunOutput, String> {
    let cat = Catalogue::load_all(root)?;
    let rust: Vec<KernelResult> = cat.cases.iter().map(geom::run_case).collect();
    let occt: Vec<KernelResult> = cat.cases.iter().map(occt::run_case).collect();

    // v5: primitives and booleans (TemporaryBRepManager); v6: features (design feature API)
    let evidence = root.join("research").join("evidence");
    let mut asm = Vec::new();
    let mut statuses = Vec::new();
    let mut metas = Vec::new();
    let mut asm_version = "n/a".to_string();
    for exp in ASM_EXPERIMENTS {
        let rel = format!("research/evidence/{exp}/reference.json");
        match asm::AsmReference::load(&evidence.join(exp).join("reference.json")) {
            Ok(Some(r)) => {
                let rows = r.results();
                statuses.push(format!("loaded {} case rows from {rel}", rows.len()));
                metas.push(serde_json::json!({ "experiment": exp, "kernel": r.kernel, "source": r.source, "fusion_version": r.fusion_version, "units": r.units, "global": r.global, "extras": r.extras.len() }));
                if asm_version == "n/a" {
                    asm_version = r.fusion_version.clone().unwrap_or_else(|| "unknown".into());
                }
                asm.extend(rows);
            }
            Ok(None) => statuses.push(format!("{rel}: {}", asm::ASM_UNAVAILABLE)),
            Err(e) => statuses.push(format!("{rel}: {}: {e}", asm::ASM_UNAVAILABLE)),
        }
    }
    let asm_status = statuses.join("; ");
    let asm_meta = (!metas.is_empty()).then(|| serde_json::Value::Array(metas));

    let asm_step: Vec<KernelResult> = cat
        .cases
        .iter()
        .filter_map(|c| {
            ASM_EXPERIMENTS
                .iter()
                .find_map(|exp| occt::remeasure_asm_step(&evidence.join(exp).join("step"), &c.id))
        })
        .collect();

    let find = |rows: &[KernelResult], id: &str| rows.iter().find(|r| r.case_id == id).cloned();
    let mut comparisons = Vec::new();
    for c in &cat.cases {
        let (r, o, a) = (find(&rust, &c.id), find(&occt, &c.id), find(&asm, &c.id));
        comparisons.push(compare::compare_opt(
            &c.id,
            r.as_ref(),
            o.as_ref(),
            result::KERNEL_RUST,
            result::KERNEL_OCCT,
        ));
        comparisons.push(compare::compare_opt(
            &c.id,
            o.as_ref(),
            a.as_ref(),
            result::KERNEL_OCCT,
            result::KERNEL_ASM,
        ));
        comparisons.push(compare::compare_opt(
            &c.id,
            r.as_ref(),
            a.as_ref(),
            result::KERNEL_RUST,
            result::KERNEL_ASM,
        ));
        if let Some(s) = find(&asm_step, &c.id) {
            comparisons.push(compare::compare_opt(
                &c.id,
                a.as_ref(),
                Some(&s),
                result::KERNEL_ASM,
                occt::KERNEL_ASM_STEP,
            ));
        }
    }

    let kernels = vec![
        KernelInfo { name: result::KERNEL_RUST.into(), version: geom::CAD_GEOM_VERSION.into(), how: "crates/cad-geom (pure Rust, in development): primitives, boolean (planes, cylinders, spheres), extrude, revolve, chamfer/fillet, shell, mass::solid_properties, validate".into() },
        KernelInfo {
            name: result::KERNEL_OCCT.into(),
            version: occt::OCCT_VERSION.into(),
            how: "Open CASCADE (occt-sys 7.8.1, static) via vendored opencascade 0.3 primitives + STEP bridge into cad-kernel; booleans, features (face_from_loops + prism/revolve, fillet, chamfer, shell) and measurements via cad-kernel".into(),
        },
        KernelInfo { name: result::KERNEL_ASM.into(), version: asm_version, how: "Autodesk ShapeManager through Fusion (recorded reference.json)".into() },
        KernelInfo {
            name: occt::KERNEL_ASM_STEP.into(),
            version: occt::OCCT_VERSION.into(),
            how: "the ASM probes' STEP exports (research/evidence/asm-v5/step, asm-v6/step) imported by cad-kernel and measured by OCCT".into(),
        },
    ];
    let tolerances = serde_json::json!({
        "REL_TOL_EXACT": compare::REL_TOL_EXACT,
        "REL_TOL_ASM": compare::REL_TOL_ASM,
        "REL_TOL_MESH": compare::REL_TOL_MESH,
        "CENTROID_REL_SIZE_EXACT": compare::CENTROID_REL_SIZE_EXACT,
        "CENTROID_REL_SIZE_ASM": compare::CENTROID_REL_SIZE_ASM,
        "CENTROID_ABS_FLOOR_MM": compare::CENTROID_ABS_FLOOR_MM,
        "BBOX_NOTE_REL": compare::BBOX_NOTE_REL,
        "counts": "exact",
        "face_types": "multiset equality (distinct-type set difference = MISMATCH)"
    });

    Ok(RunOutput {
        generated_utc: report::utc_now(),
        catalogue_version: cat.version,
        kernels,
        asm_status,
        asm_meta,
        bridge_fidelity: occt::bridge_fidelity(),
        rust,
        occt,
        asm,
        asm_step,
        comparisons,
        tolerances,
        catalogue: cat,
    })
}
