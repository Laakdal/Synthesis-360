//! Markdown report (`research/VALIDATION_REPORT.md`) and timestamp formatting.

use crate::RunOutput;
use crate::cases::Catalogue;
use crate::compare::{Comparison, Outcome};
use crate::occt::KERNEL_ASM_STEP;
use crate::result::{KERNEL_ASM, KERNEL_OCCT, KERNEL_RUST, KernelResult};
use std::collections::BTreeMap;
use std::fmt::Write;

/// Current UTC time as `YYYY-MM-DDTHH:MM:SSZ` (no date-time dependency).
pub fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format_utc(secs)
}

/// Formats seconds since the Unix epoch (proleptic Gregorian, H. Hinnant's civil-from-days).
pub fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn g(v: f64) -> String {
    let a = v.abs();
    if a != 0.0 && !(1e-3..1e7).contains(&a) {
        format!("{v:.6e}")
    } else {
        format!("{v:.6}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn cell(r: Option<&KernelResult>) -> String {
    match r {
        None => "—".into(),
        Some(r) if !r.ok => format!("✗ {}", r.error.as_deref().unwrap_or("failed")),
        Some(r) => {
            let mut s = r.volume.map_or("V=?".into(), |v| format!("V={}", g(v)));
            if let (Some(f), Some(e), Some(v)) = (r.faces, r.edges, r.vertices) {
                let _ = write!(s, " F/E/V={f}/{e}/{v}");
            }
            if r.valid == Some(false) {
                s.push_str(" INVALID");
            }
            s
        }
    }
}

fn esc(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// Per-case verdict. `REFERENCE_VERIFIED` only when an ASM row exists and cad-geom matches it.
pub fn verdict(rust_occt: &Comparison, rust_asm: &Comparison) -> String {
    match rust_asm.outcome {
        Outcome::Match => return "REFERENCE_VERIFIED".into(),
        // mass properties agree with ASM, counts differ by convention: deliberately not "verified"
        Outcome::TopologyConvention => {
            return "MASS_MATCHES_ASM (topology convention differs)".into();
        }
        _ => {}
    }
    match rust_occt.outcome {
        Outcome::Match => "AGREES_WITH_OCCT (not verified against ASM)".into(),
        Outcome::TopologyConvention => {
            "AGREES_WITH_OCCT_MASS (topology convention; not verified against ASM)".into()
        }
        o => format!("UNVERIFIED ({})", o.label()),
    }
}

fn tally(cmp: &[&Comparison]) -> String {
    let mut m: BTreeMap<&str, usize> = BTreeMap::new();
    for c in cmp {
        *m.entry(c.outcome.label()).or_default() += 1;
    }
    m.iter()
        .map(|(k, v)| format!("{k} {v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Renders the Markdown report.
pub fn render(out: &RunOutput, cat: &Catalogue) -> String {
    let find = |rows: &[KernelResult], id: &str| rows.iter().find(|r| r.case_id == id).cloned();
    let pair = |id: &str, a: &str, b: &str| {
        out.comparisons
            .iter()
            .find(|c| c.case_id == id && c.a == a && c.b == b)
            .cloned()
    };
    let pairs_of = |a: &str, b: &str| {
        out.comparisons
            .iter()
            .filter(|c| c.a == a && c.b == b)
            .collect::<Vec<_>>()
    };
    let performed = |v: &[&Comparison]| {
        v.iter()
            .filter(|c| c.outcome != Outcome::NotCompared)
            .count()
    };
    let ro = pairs_of(KERNEL_RUST, KERNEL_OCCT);
    let oa = pairs_of(KERNEL_OCCT, KERNEL_ASM);
    let ra = pairs_of(KERNEL_RUST, KERNEL_ASM);
    let verified = cat
        .cases
        .iter()
        .filter(|c| {
            pair(&c.id, KERNEL_RUST, KERNEL_ASM).is_some_and(|p| p.outcome == Outcome::Match)
        })
        .count();
    let rust_built = out.rust.iter().filter(|r| r.ok).count();
    let occt_built = out.occt.iter().filter(|r| r.ok).count();

    let mut s = String::new();
    let _ = writeln!(s, "# Differential validation report\n");
    let _ = writeln!(
        s,
        "Generated **{}** by `cargo run -p cad-diff` (crate `crates/cad-diff`). Raw data: `research/evidence/diff/results.json`. Case catalogues: `research/cases.json` (version {}) and `research/feature-cases.json`, {} cases in all.\n",
        out.generated_utc,
        out.catalogue_version,
        cat.cases.len()
    );

    let _ = writeln!(s, "## What was actually compared\n");
    let _ = writeln!(
        s,
        "* **cad-geom vs OCCT**: {} of {} cases compared field by field ({}). cad-geom built {} of {} cases; OCCT built {} of {}.",
        performed(&ro),
        ro.len(),
        tally(&ro),
        rust_built,
        cat.cases.len(),
        occt_built,
        cat.cases.len()
    );
    if out.asm.is_empty() {
        let _ = writeln!(
            s,
            "* **OCCT vs ASM** and **cad-geom vs ASM**: **not performed** — {}. No case is `REFERENCE_VERIFIED`.",
            out.asm_status
        );
    } else {
        let _ = writeln!(
            s,
            "* **OCCT vs ASM**: {} of {} cases compared ({}).",
            performed(&oa),
            oa.len(),
            tally(&oa)
        );
        let _ = writeln!(
            s,
            "* **cad-geom vs ASM**: {} of {} cases compared ({}).",
            performed(&ra),
            ra.len(),
            tally(&ra)
        );
        let _ = writeln!(s, "* ASM source: {}.", out.asm_status);
        let sa = pairs_of(KERNEL_ASM, KERNEL_ASM_STEP);
        if sa.is_empty() {
            let _ = writeln!(
                s,
                "* **ASM vs its own STEP export re-measured by OCCT**: not performed (no STEP files)."
            );
        } else {
            let _ = writeln!(
                s,
                "* **ASM vs its own STEP export re-measured by OCCT** (cross-check of the exported ASM geometry): {} cases ({}).",
                sa.len(),
                tally(&sa)
            );
        }
    }
    let _ = writeln!(
        s,
        "* `REFERENCE_VERIFIED` cases (an ASM row exists and cad-geom matches it within tolerance): **{verified}**."
    );
    let _ = writeln!(
        s,
        "* Only the fields present on both sides of a pair are compared; each comparison in `results.json` lists them under `compared`.\n"
    );

    let _ = writeln!(
        s,
        "## Kernels\n\n| Kernel | Version | How the cases are built |\n|---|---|---|"
    );
    for k in &out.kernels {
        let _ = writeln!(s, "| {} | {} | {} |", k.name, esc(&k.version), esc(&k.how));
    }
    if let Some(m) = &out.asm_meta {
        let _ = writeln!(s, "\nASM reference metadata: `{}`", m);
        if let Some(g) = m.get("global") {
            let v = |k: &str| g.get(k).map_or("?".to_string(), |v| v.to_string());
            let _ = writeln!(
                s,
                "\nASM global tolerances (from the reference): point tolerance **{} mm**, vector angle tolerance **{} rad**. For comparison, OCCT `Precision::Confusion()` = 1e-7 mm and cad-geom `Tolerance::default()` = 1e-7 mm / 1e-12 rad; cad-geom booleans use `Tolerance::asm()` = 1e-5 mm / 1e-10 rad.",
                v("point_tolerance_mm"),
                v("vector_angle_tolerance_rad")
            );
        }
    }

    let _ = writeln!(s, "\n## Method\n");
    let _ = writeln!(
        s,
        "1. Every case in `research/cases.json` is built by each kernel. **cad-geom**: `cad_geom::primitives::make_*`, `mass::solid_properties`, `validate`; every call is wrapped in `catch_unwind`, so bodies that are still `unimplemented!()` become `NOT_IMPLEMENTED` rows. Booleans whose operands are all boxes, cylinders or spheres fold the tools into the target in order with `cad_geom::boolean` (polyhedral pipeline for boxes, quadric pipeline otherwise; ASM tolerance 1e-5 mm / 1e-10 rad); their results are validated at 1e-7 mm, falling back to the boolean tolerance with a note (the near-tangent case). Line contact is one edge with 4 coedges flagged in `Brep::nonmanifold_edges`, as ASM builds it. Booleans with a cone or torus operand are `NOT_IMPLEMENTED` (cad-geom returns `Unsupported`). Feature cases (`research/feature-cases.json`) run `cad_geom::extrude`, `revolve`, `fillet`, `chamfer` and `shell` at `Tolerance::asm()`; edges are picked by exact point-to-curve distance (≤ 1e-6 mm, exactly one edge per point), faces by point-to-surface distance; an operation returning `Unsupported` gives a `NOT_IMPLEMENTED` row."
    );
    let _ = writeln!(
        s,
        "2. **OCCT**: primitives are true `BRepPrimAPI_Make*` primitives from the vendored `opencascade` crate. Its shape type is private and cad-kernel has no raw-shape constructor, so each primitive is handed to cad-kernel through a temporary STEP file (`write_step` → `cad_kernel::occ::Shape::read_step`, serialised by a mutex). Booleans fold the operands in order with cad-kernel `fuse`/`cut`/`common` (`BRepAlgoAPI_*` behind the exception-safe shim). Cones use the crate's `ConeBuilder`, which only builds +Z cones; every catalogue cone is +Z. Feature cases are built natively by cad-kernel, the calls the application's recompute makes: `face_from_loops` + `prism` (BRepPrimAPI_MakePrism) or `revolve` (BRepPrimAPI_MakeRevol), `fillet`/`chamfer` (BRepFilletAPI_MakeFillet/MakeChamfer), `shell` (BRepOffsetAPI_MakeThickSolid). A full circle is passed as two half arcs (cad-kernel's `Seg3` has no closed circle), so OCCT shows two edges per circle; edges are picked by distance to their polyline (≤ 1e-3 mm), and where a point hits a vertex of OCCT's split circle only the curved edges (both halves) are kept; faces are picked by bounding box."
    );
    let _ = writeln!(
        s,
        "3. OCCT measurements use cad-kernel's public API only: counts are unique TopoDS entities (`TopExp::MapShapes`: solids, shells, faces, wires = loops, edges, vertices); surface and curve types from `BRepAdaptor` (GeomAbs); native degenerate edges from `BRep_Tool::Degenerated`; seam = non-degenerate edge with one distinct adjacent face; `degenerate_edges_literal` = the ASM probe's literal definition (no geometry, zero length, or start == end, so closed circles count), computed the same way for every kernel; volume/area/centroid from `inspect::mass_props` (BRepGProp); validity from `BRepCheck_Analyzer`; bbox from `BRepBndLib::Add`. **OCCT inertia is not exposed by cad-kernel**, so the OCCT inertia column is empty (harness limitation; no binding was added)."
    );
    let _ = writeln!(
        s,
        "4. cad-geom counts are its arena sizes (edges include degenerate edges; seams are edges used twice by one face; literal count = no curve or start vertex == end vertex). ASM rows are loaded leniently from `research/evidence/asm-v5/reference.json` (primitives, booleans) and `research/evidence/asm-v6/reference.json` (features, when recorded) and type names normalised (e.g. `Arc3D`→circle, `NurbsSurface`→bspline)."
    );
    let _ = writeln!(
        s,
        "5. Each kernel pair is classified: `MATCH`, `MISMATCH(field, a, b)`, `TOPOLOGY_CONVENTION` (volume and area agree; only counts or type multiplicities differ, e.g. seam/pole edges), `ONE_SIDED_FAILURE`, `BOTH_FAILED_SAME`, `NOT_IMPLEMENTED`, `NOT_COMPARED` (a side has no row)."
    );
    let _ = writeln!(
        s,
        "6. Kernel-native `degenerate_edges` and `seam_edges` are **informational only** (ASM has no seam or pole edges; its `degenerate_edges` field is the literal count, compared as `degenerate_edges_literal`). A field that is null on either side is not compared (e.g. ASM's centroid and inertia for the tiny box, a Fusion mass-property failure at tiny volumes). Inertia is about the centroid, unit density, mm⁵, products of inertia as −∫xy dV (ASM and cad-geom)."
    );
    let _ = writeln!(
        s,
        "7. Where an ASM probe exported a STEP file (`research/evidence/asm-v5/step/<id>.step` or `asm-v6/step/<id>.step`), it is imported with cad-kernel's STEP reader, measured by OCCT (`asm-step(occt)`) and compared with the ASM row: a cross-check of the exported geometry.\n"
    );

    let _ = writeln!(
        s,
        "## Tolerances\n\n| Constant | Value | Applies to | Reason |\n|---|---|---|---|"
    );
    let rows = [
        (
            "REL_TOL_EXACT",
            crate::compare::REL_TOL_EXACT,
            "volume, area, inertia (relative), cad-geom vs OCCT",
            "both integrate the exact B-rep; quadrature error on analytic faces is far below this",
        ),
        (
            "REL_TOL_ASM",
            crate::compare::REL_TOL_ASM,
            "same, when one side is ASM",
            "ASM mass_path = base_feature (PhysicalProperties, VeryHigh accuracy, exact B-rep); measured OCCT-vs-ASM agreement is ~2e-8 or better, so the exact tolerance applies (a looser one would hide the ~9e-6 STEP-export loss on box∩sphere)",
        ),
        (
            "REL_TOL_MESH",
            crate::compare::REL_TOL_MESH,
            "same, when a side's `mass_path` is mesh/low/medium accuracy",
            "tessellation error dominates",
        ),
        (
            "CENTROID_REL_SIZE_EXACT",
            crate::compare::CENTROID_REL_SIZE_EXACT,
            "centroid distance ÷ bbox diagonal (exact pair)",
            "scales with model size (0.01 mm to 100 m boxes)",
        ),
        (
            "CENTROID_REL_SIZE_ASM",
            crate::compare::CENTROID_REL_SIZE_ASM,
            "same, ASM pairs",
            "as REL_TOL_ASM",
        ),
        (
            "CENTROID_ABS_FLOOR_MM",
            crate::compare::CENTROID_ABS_FLOOR_MM,
            "minimum centroid tolerance (mm)",
            "keeps the tiny box above round-off",
        ),
        (
            "BBOX_NOTE_REL",
            crate::compare::BBOX_NOTE_REL,
            "bbox (informational only)",
            "OCCT `BRepBndLib::Add` adds the shape tolerance gap; cad-geom exposes no bbox",
        ),
    ];
    for (n, v, a, r) in rows {
        let _ = writeln!(s, "| `{n}` | {v:e} | {a} | {r} |");
    }
    let _ = writeln!(
        s,
        "| counts | exact | bodies, lumps, shells, faces, edges, vertices, loops, `degenerate_edges_literal`, edge-type multiset (native degenerate/seam counts: informational) | |"
    );
    let _ = writeln!(
        s,
        "| face types | multiset equality | a different *set* of distinct face types is a MISMATCH; different multiplicities only are a convention difference | |\n"
    );

    let _ = writeln!(s, "## STEP bridge fidelity (OCCT harness self-check)\n");
    for b in &out.bridge_fidelity {
        let _ = writeln!(
            s,
            "* {}: {}{}",
            b.primitive,
            if b.identical {
                "bridged == native (counts, types, validity, volume within 1e-9)"
            } else {
                "**DIFFERS**: "
            },
            b.differences.join("; ")
        );
    }
    let _ = writeln!(
        s,
        "* cone, sphere and torus cannot be built natively in cad-kernel, so the bridge is not cross-checked for them (the STEP reader may rebuild their seam and degenerate edges).\n"
    );

    let _ = writeln!(s, "## Per-case results\n");
    let _ = writeln!(
        s,
        "Kernel cells: volume (mm³) and unique face/edge/vertex counts. Closed-form error = |V_occt − V_exact| / V_exact for primitives.\n"
    );
    let _ = writeln!(
        s,
        "| Case | cad-geom | OCCT | ASM | OCCT vs closed form | cad-geom vs OCCT | OCCT vs ASM | cad-geom vs ASM | Verdict |\n|---|---|---|---|---|---|---|---|---|"
    );
    for c in &cat.cases {
        let (r, o, a) = (
            find(&out.rust, &c.id),
            find(&out.occt, &c.id),
            find(&out.asm, &c.id),
        );
        let cf = match (
            c.spec.closed_form_volume(),
            o.as_ref().and_then(|o| o.volume),
        ) {
            (Some(x), Some(v)) => format!("{:.2e}", ((v - x) / x).abs()),
            _ => "—".into(),
        };
        let p = |a: &str, b: &str| pair(&c.id, a, b).expect("every pair is compared");
        let (ro, oa, ra) = (
            p(KERNEL_RUST, KERNEL_OCCT),
            p(KERNEL_OCCT, KERNEL_ASM),
            p(KERNEL_RUST, KERNEL_ASM),
        );
        let _ = writeln!(
            s,
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} |",
            c.id,
            esc(&cell(r.as_ref())),
            esc(&cell(o.as_ref())),
            esc(&cell(a.as_ref())),
            cf,
            esc(&short(&ro)),
            esc(&short(&oa)),
            esc(&short(&ra)),
            verdict(&ro, &ra)
        );
    }

    let sa = pairs_of(KERNEL_ASM, KERNEL_ASM_STEP);
    if !sa.is_empty() {
        let _ = writeln!(
            s,
            "\n### ASM STEP exports re-measured by OCCT\n\n| Case | ASM (reported) | ASM STEP → OCCT | ASM vs ASM-STEP(OCCT) |\n|---|---|---|---|"
        );
        for c in &sa {
            let st = find(&out.asm_step, &c.case_id);
            let _ = writeln!(
                s,
                "| `{}` | {} | {} | {} |",
                c.case_id,
                esc(&cell(find(&out.asm, &c.case_id).as_ref())),
                esc(&cell(st.as_ref())),
                esc(&short(c))
            );
        }
    }

    let _ = writeln!(s, "\n## Discrepancies\n");
    let mut any = false;
    for c in &out.comparisons {
        if matches!(c.outcome, Outcome::Match | Outcome::NotCompared) {
            continue;
        }
        if c.outcome == Outcome::NotImplemented && c.diffs.is_empty() {
            continue;
        }
        any = true;
        let _ = writeln!(
            s,
            "* `{}` {} vs {}: **{}**",
            c.case_id,
            c.a,
            c.b,
            c.outcome.label()
        );
        for d in &c.diffs {
            let _ = writeln!(
                s,
                "  * {}: {} = {}, {} = {}",
                d.field,
                c.a,
                esc(&d.a),
                c.b,
                esc(&d.b)
            );
        }
        for n in &c.notes {
            let _ = writeln!(s, "  * note: {}", esc(n));
        }
    }
    let ni: Vec<&str> = out
        .comparisons
        .iter()
        .filter(|c| c.outcome == Outcome::NotImplemented && c.diffs.is_empty())
        .map(|c| c.case_id.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if !ni.is_empty() {
        any = true;
        let _ = writeln!(
            s,
            "* `NOT_IMPLEMENTED` (no field compared) in: {}",
            ni.iter()
                .map(|i| format!("`{i}`"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    for r in out.rust.iter().chain(&out.occt) {
        if !r.ok && !r.is_not_implemented() {
            any = true;
            let _ = writeln!(
                s,
                "* `{}` {} failed: {}",
                r.case_id,
                r.kernel,
                esc(r.error.as_deref().unwrap_or(""))
            );
        }
        if r.valid == Some(false) {
            any = true;
            let _ = writeln!(s, "* `{}` {} result is INVALID", r.case_id, r.kernel);
        }
    }
    if !any {
        let _ = writeln!(s, "None.");
    }
    let _ = writeln!(s, "\n## Harness limitations\n");
    let _ = writeln!(
        s,
        "* OCCT inertia tensors are not compared: cad-kernel exposes `mass_props` = (volume, area, centroid) only."
    );
    let _ = writeln!(
        s,
        "* OCCT primitives pass through a STEP write/read (see the fidelity check above); booleans therefore operate on STEP-read operands."
    );
    let _ = writeln!(
        s,
        "* OCCT bounding boxes are tolerance-enlarged and not tight on curved faces; bbox is never a MATCH criterion."
    );
    let _ = writeln!(
        s,
        "* cad-geom is under active development by other workers; its rows are a snapshot at the timestamp above."
    );
    s
}

fn short(c: &Comparison) -> String {
    let s = c.summary();
    if s.len() > 160 {
        format!(
            "{}…",
            &s[..s
                .char_indices()
                .take_while(|(i, _)| *i < 157)
                .last()
                .map_or(0, |(i, ch)| i + ch.len_utf8())]
        )
    } else {
        s
    }
}
