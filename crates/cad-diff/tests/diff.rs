//! cad-diff harness tests: OCCT primitives against closed forms, the comparator's classification,
//! the ASM loader, the STEP bridge, and cad-geom vs OCCT volume agreement.

use cad_diff::asm::{AsmReference, case_result};
use cad_diff::cases::{Catalogue, Spec, repo_root};
use cad_diff::compare::{Outcome, compare};
use cad_diff::result::{KERNEL_ASM, KERNEL_OCCT, KERNEL_RUST, KernelResult};
use cad_diff::{geom, occt, report};
use std::f64::consts::PI;

fn occt_volume(spec: &Spec) -> f64 {
    let s = occt::build(spec).expect("OCCT builds the primitive");
    cad_kernel::inspect::mass_props(&s)
        .expect("mass properties")
        .volume
}

#[test]
fn occt_primitive_volumes_equal_closed_form() {
    let o = [0.0; 3];
    let z = [0.0, 0.0, 1.0];
    let cases = [
        (
            "box",
            Spec::Box {
                corner: o,
                size: [50.0, 30.0, 10.0],
            },
            15000.0,
        ),
        (
            "cylinder",
            Spec::Cylinder {
                base: o,
                axis: z,
                r0: 10.0,
                r1: 10.0,
                h: 20.0,
            },
            PI * 100.0 * 20.0,
        ),
        (
            "cone",
            Spec::Cone {
                base: o,
                axis: z,
                r0: 10.0,
                r1: 0.0,
                h: 20.0,
            },
            PI * 100.0 * 20.0 / 3.0,
        ),
        (
            "sphere",
            Spec::Sphere { center: o, r: 10.0 },
            4.0 / 3.0 * PI * 1000.0,
        ),
        (
            "torus",
            Spec::Torus {
                center: o,
                axis: z,
                major: 20.0,
                minor: 5.0,
            },
            2.0 * PI * PI * 20.0 * 25.0,
        ),
    ];
    let mut failures = Vec::new();
    for (name, spec, exact) in cases {
        let v = occt_volume(&spec);
        let rel = ((v - exact) / exact).abs();
        if rel > 1e-6 {
            failures.push(format!(
                "{name}: OCCT {v} vs closed form {exact} (relative error {rel:e})"
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn step_bridge_preserves_box_and_cylinder() {
    for b in occt::bridge_fidelity() {
        assert!(b.identical, "{}: {:?}", b.primitive, b.differences);
    }
}

#[test]
fn occt_box_topology_is_six_twelve_eight() {
    let s = occt::build(&Spec::Box {
        corner: [0.0; 3],
        size: [50.0, 30.0, 10.0],
    })
    .unwrap();
    let r = occt::measure("prim.box", &s);
    assert_eq!(
        (r.faces, r.edges, r.vertices, r.loops, r.shells, r.lumps),
        (Some(6), Some(12), Some(8), Some(6), Some(1), Some(1))
    );
    assert_eq!(
        r.face_types.as_ref().unwrap(),
        &vec!["plane".to_string(); 6]
    );
    assert_eq!(r.valid, Some(true));
}

#[test]
fn catalogue_parses() {
    let cat = Catalogue::load(&repo_root().join("research").join("cases.json")).unwrap();
    assert!(cat.cases.len() >= 19, "{}", cat.cases.len());
    assert!(cat.cases.iter().any(|c| c.id == "bool.union.repeated"
        && matches!(&c.spec, Spec::Boolean { tools, .. } if tools.len() == 5)));
}

fn row(kernel: &str, volume: f64, area: f64, faces: u32, types: &[&str]) -> KernelResult {
    let mut r = KernelResult::new(kernel, "synthetic");
    r.ok = true;
    r.bodies = Some(1);
    r.solid = Some(true);
    r.volume = Some(volume);
    r.area = Some(area);
    r.centroid = Some([1.0, 2.0, 3.0]);
    r.bbox = Some([[0.0; 3], [10.0, 10.0, 10.0]]);
    r.faces = Some(faces);
    let mut t: Vec<String> = types.iter().map(|s| s.to_string()).collect();
    t.sort();
    r.face_types = Some(t);
    r
}

#[test]
fn comparator_match() {
    let a = row(
        KERNEL_RUST,
        1000.0,
        600.0,
        3,
        &["cylinder", "plane", "plane"],
    );
    let mut b = row(
        KERNEL_OCCT,
        1000.0 * (1.0 + 1e-9),
        600.0,
        3,
        &["plane", "cylinder", "plane"],
    );
    b.centroid = Some([1.0 + 1e-9, 2.0, 3.0]);
    let c = compare(&a, &b);
    assert_eq!(c.outcome, Outcome::Match, "{c:?}");
    assert!(
        c.compared.contains(&"volume".to_string())
            && c.compared.contains(&"face_types".to_string())
    );
}

#[test]
fn comparator_mismatch_lists_field_and_values() {
    let a = row(KERNEL_RUST, 1000.0, 600.0, 6, &["plane"; 6]);
    let b = row(KERNEL_OCCT, 1001.0, 600.0, 6, &["plane"; 6]);
    let c = compare(&a, &b);
    assert_eq!(c.outcome, Outcome::Mismatch);
    assert_eq!(c.diffs[0].field, "volume");
    assert_eq!(c.summary(), "MISMATCH(volume, 1000, 1001)");

    let mut d = row(KERNEL_OCCT, 1000.0, 600.0, 6, &["plane"; 6]);
    d.centroid = Some([1.1, 2.0, 3.0]);
    assert_eq!(compare(&a, &d).diffs[0].field, "centroid");
}

#[test]
fn comparator_topology_convention_when_mass_agrees() {
    // OCCT sphere: one face with a seam and two degenerate edges; a kernel without them
    let mut a = row(KERNEL_OCCT, 4188.79, 1256.64, 1, &["sphere"]);
    a.edges = Some(3);
    a.seam_edges = Some(1);
    a.degenerate_edges = Some(2);
    a.degenerate_edges_literal = Some(2);
    let mut b = row(KERNEL_ASM, 4188.79, 1256.64, 1, &["sphere"]);
    b.edges = Some(0);
    b.seam_edges = Some(0);
    b.degenerate_edges = Some(0);
    b.degenerate_edges_literal = Some(0);
    let c = compare(&a, &b);
    assert_eq!(c.outcome, Outcome::TopologyConvention, "{c:?}");
    // edges and the literal degenerate count are compared; native seam/degenerate counts are notes
    let fields: Vec<&str> = c.diffs.iter().map(|d| d.field.as_str()).collect();
    assert_eq!(fields, ["edges", "degenerate_edges_literal"]);
    assert!(
        c.notes
            .iter()
            .any(|n| n.starts_with("seam_edges (informational)"))
    );

    // a null field on one side (ASM tiny-box centroid) is not compared and not a mismatch
    let mut n = b.clone();
    n.centroid = None;
    n.edges = Some(3);
    n.degenerate_edges_literal = Some(2);
    let cn = compare(&a, &n);
    assert_eq!(cn.outcome, Outcome::Match, "{cn:?}");
    assert!(!cn.compared.contains(&"centroid".to_string()));

    // split faces of the same types (multiplicity only) are also a convention difference
    let t1 = row(KERNEL_RUST, 100.0, 50.0, 1, &["torus"]);
    let t2 = row(KERNEL_OCCT, 100.0, 50.0, 2, &["torus", "torus"]);
    assert_eq!(compare(&t1, &t2).outcome, Outcome::TopologyConvention);

    // ...but a different set of surface types is a mismatch even with equal mass
    let p = row(KERNEL_RUST, 100.0, 50.0, 2, &["plane", "cylinder"]);
    let q = row(KERNEL_OCCT, 100.0, 50.0, 2, &["plane", "bspline"]);
    assert_eq!(compare(&p, &q).outcome, Outcome::Mismatch);
}

#[test]
fn comparator_failure_classes() {
    let ok = row(KERNEL_OCCT, 1.0, 6.0, 6, &["plane"; 6]);
    let failed = KernelResult::failed(KERNEL_ASM, "synthetic", "boolean failed");
    let ni = KernelResult::not_implemented(KERNEL_RUST, "synthetic", "booleans");
    assert_eq!(compare(&ok, &failed).outcome, Outcome::OneSidedFailure);
    assert_eq!(compare(&ni, &ok).outcome, Outcome::NotImplemented);
    assert_eq!(
        compare(
            &failed,
            &KernelResult::failed(KERNEL_OCCT, "synthetic", "x")
        )
        .outcome,
        Outcome::BothFailedSame
    );
}

#[test]
fn asm_loader_parses_minimal_sample() {
    let text = r#"{
        "kernel": "ASM", "source": "Fusion TemporaryBRepManager", "fusion_version": "2705.1.30", "units": "mm",
        "global": { "point_tolerance_mm": 1e-6, "vector_angle_tolerance_rad": 1e-10 },
        "cases": [
          { "id": "prim.box", "ok": true, "error": null, "bodies": 1, "solid": true, "lumps": 1, "shells": 1,
            "faces": 6, "edges": 12, "vertices": 8, "loops": 6, "face_types": ["PlaneSurfaceType", "Plane", "Plane", "Plane", "Plane", "Plane"],
            "edge_types": {"Line3DCurveType": 12}, "degenerate_edges": 0, "seam_edges": 0,
            "volume": 15000, "area": 4600.0, "centroid": {"x": 25, "y": 15, "z": 5},
            "inertia_centroid": [1, 2, 3, 0, 0, 0], "bbox": {"min": [0,0,0], "max": [50,30,10]},
            "max_edge_tolerance": 1e-6, "max_vertex_tolerance": null, "mass_path": "physicalProperties VeryHigh", "notes": "sample" },
          { "id": "bool.x", "ok": false, "error": "Compute Failed" },
          { "no_id": true }
        ],
        "extras": [ { "anything": 1 } ]
    }"#;
    let r = AsmReference::parse(text).unwrap();
    assert_eq!(r.fusion_version.as_deref(), Some("2705.1.30"));
    assert_eq!(
        r.global.as_ref().and_then(|g| g.point_tolerance_mm),
        Some(1e-6)
    );
    let rows = r.results();
    assert_eq!(rows.len(), 2);
    let b = &rows[0];
    assert!(b.ok);
    assert_eq!((b.faces, b.edges, b.vertices), (Some(6), Some(12), Some(8)));
    assert_eq!(
        b.face_types.as_ref().unwrap(),
        &vec!["plane".to_string(); 6]
    );
    assert_eq!(
        b.edge_types.as_ref().unwrap(),
        &vec!["line".to_string(); 12]
    );
    assert_eq!(b.centroid, Some([25.0, 15.0, 5.0]));
    assert_eq!(b.bbox, Some([[0.0; 3], [50.0, 30.0, 10.0]]));
    assert_eq!(b.inertia_centroid.unwrap()[2][2], 3.0);
    assert_eq!(b.max_vertex_tolerance, None);
    // the probe's degenerate_edges is the literal count; the truly degenerate count comes from notes
    assert_eq!(b.degenerate_edges_literal, Some(0));
    assert_eq!(b.degenerate_edges, None);
    let cone = case_result(&serde_json::json!({"id": "prim.cone", "degenerate_edges": 2, "notes": "x = 1: 1 closed non-degenerate edge(s) (full circles) + 1 truly degenerate edge(s) (ASM isDegenerate)"})).unwrap();
    assert_eq!(
        (cone.degenerate_edges_literal, cone.degenerate_edges),
        (Some(2), Some(1))
    );
    assert!(!rows[1].ok);
    assert_eq!(rows[1].error.as_deref(), Some("Compute Failed"));
    // an empty object is tolerated
    assert!(AsmReference::parse("{}").unwrap().results().is_empty());
    assert!(
        case_result(&serde_json::json!({"id": "x"}))
            .unwrap()
            .volume
            .is_none()
    );
}

#[test]
fn utc_formatting() {
    assert_eq!(report::format_utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(report::format_utc(1_700_000_000), "2023-11-14T22:13:20Z");
    assert_eq!(report::format_utc(951_782_400), "2000-02-29T00:00:00Z");
}

/// cad-geom primitives against OCCT: volume within REL_TOL_EXACT for every catalogue primitive.
#[test]
fn cad_geom_volumes_agree_with_occt() {
    let cat = Catalogue::load(&repo_root().join("research").join("cases.json")).unwrap();
    let mut problems = Vec::new();
    for c in cat.cases.iter().filter(|c| !c.spec.is_boolean()) {
        let r = geom::run_case(c);
        let Some(v) = r.volume else {
            problems.push(format!(
                "{}: cad-geom gave no volume ({:?}, {:?})",
                c.id,
                r.error,
                r.notes.first()
            ));
            continue;
        };
        let o = occt_volume(&c.spec);
        if ((v - o) / o).abs() > cad_diff::compare::REL_TOL_EXACT {
            problems.push(format!("{}: cad-geom {v} vs OCCT {o}", c.id));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// All ten boolean cases (the six box-only ones and the four with a cylinder or sphere) run
/// through `cad_geom::boolean` and MATCH the ASM reference on every compared field (F/E/V/L,
/// shells, face and edge types, volume, area, centroid, inertia), and validate.
#[test]
fn cad_geom_booleans_match_asm() {
    let root = repo_root();
    let cat = Catalogue::load(&root.join("research").join("cases.json")).unwrap();
    let asm = AsmReference::load(
        &root
            .join("research")
            .join("evidence")
            .join("asm-v5")
            .join("reference.json"),
    )
    .unwrap()
    .expect("ASM reference present")
    .results();
    let all = [
        "bool.union.overlap",
        "bool.union.coincident_face",
        "bool.union.coincident_partial",
        "bool.cut.coplanar_through",
        "bool.cut.disjoint",
        "bool.union.repeated",
        "bool.cut.hole",
        "bool.union.tangent_cylinder",
        "bool.union.near_tangent_cylinder",
        "bool.intersect.box_sphere",
    ];
    let mut problems = Vec::new();
    let booleans: Vec<_> = cat.cases.iter().filter(|c| c.spec.is_boolean()).collect();
    assert_eq!(booleans.len(), all.len());
    for c in booleans {
        let r = geom::run_case(c);
        if !all.contains(&c.id.as_str()) {
            problems.push(format!("{}: not in the expected list", c.id));
            continue;
        }
        let a = asm.iter().find(|x| x.case_id == c.id).expect("ASM row");
        // regression: a seam is an edge some face uses twice, so ASM's 4-coedge contact edge
        // (used twice by the cylinder face) counts, as in ASM's own `seam_edges`
        let seams = match c.id.as_str() {
            "bool.union.tangent_cylinder" | "bool.union.near_tangent_cylinder" => Some(1),
            "bool.cut.hole" | "bool.intersect.box_sphere" => Some(0),
            _ => r.seam_edges,
        };
        if r.seam_edges != seams || (seams == Some(1) && a.seam_edges != Some(1)) {
            problems.push(format!(
                "{}: seam edges {:?} (ASM {:?})",
                c.id, r.seam_edges, a.seam_edges
            ));
        }
        let cmp = compare(&r, a);
        for f in [
            "volume",
            "area",
            "centroid",
            "inertia_centroid",
            "faces",
            "edges",
            "vertices",
            "loops",
            "shells",
            "face_types",
            "edge_types",
        ] {
            if !cmp.compared.iter().any(|x| x == f) {
                problems.push(format!("{}: {f} not compared", c.id));
            }
        }
        if cmp.outcome != Outcome::Match || r.valid != Some(true) {
            problems.push(format!(
                "{}: {} (valid {:?}, notes {:?})",
                c.id,
                cmp.summary(),
                r.valid,
                r.notes
            ));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}
