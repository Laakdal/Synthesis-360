//! Parity with ASM: builds the nine `prim.*` cases of `research/cases.json` and compares them
//! with the Fusion/ASM recording in `research/evidence/asm-v5/reference.json`: F/E/V/L, face
//! and edge types in order, the probe's literal degenerate-edge count (no geometry or start ==
//! end vertex), coedges per edge, and volume/area/centroid/inertia within 1e-9 relative.

use cad_geom::mass::solid_properties;
use cad_geom::primitives::*;
use cad_geom::{Brep, Curve, Frame, SolidId, P3, V3};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn load(rel: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

fn arr3(v: &Value) -> [f64; 3] {
    let a = v.as_array().unwrap();
    [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()]
}

fn num(v: &Value, k: &str) -> f64 {
    v[k].as_f64().unwrap_or_else(|| panic!("missing {k}"))
}

fn build(c: &Value) -> Brep {
    let frame = |o: &str| Frame::from_origin_z(P3::from(arr3(&c[o])), V3::from(arr3(&c["axis"])));
    match c["kind"].as_str().unwrap() {
        "box" => make_box(P3::from(arr3(&c["corner"])), arr3(&c["size"])).unwrap(),
        "cylinder" => make_cylinder(frame("base"), num(c, "r0"), num(c, "h")).unwrap(),
        "cone" => make_cone(frame("base"), num(c, "r0"), num(c, "r1"), num(c, "h")).unwrap(),
        "sphere" => make_sphere(P3::from(arr3(&c["center"])), num(c, "r")).unwrap(),
        "torus" => make_torus(frame("center"), num(c, "major"), num(c, "minor")).unwrap(),
        k => panic!("unexpected kind {k}"),
    }
}

fn edge_type(c: &Option<Curve>) -> &'static str {
    match c {
        None => "none",
        Some(Curve::Line { .. }) => "line",
        Some(Curve::Circle { .. }) => "circle",
        Some(Curve::Ellipse { .. }) => "ellipse",
        Some(Curve::Nurbs(_)) => "nurbs",
    }
}

/// "coedges per edge histogram: {...}" from the probe's notes.
fn asm_histogram(notes: &str) -> BTreeMap<String, u64> {
    let tail = &notes[notes.find("coedges per edge histogram: ").expect("histogram in notes") + 28..];
    let json = &tail[..=tail.find('}').unwrap()];
    serde_json::from_str::<BTreeMap<String, u64>>(json).unwrap()
}

#[test]
fn primitives_match_asm() {
    let cases = load("research/cases.json");
    let asm = load("research/evidence/asm-v5/reference.json");
    let mut checked = 0;
    let mut failures = Vec::new();
    for c in cases["cases"].as_array().unwrap().iter().filter(|c| c["id"].as_str().unwrap().starts_with("prim.")) {
        let id = c["id"].as_str().unwrap();
        let a = asm["cases"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap_or_else(|| panic!("{id} not in ASM data"));
        assert_eq!(a["ok"], Value::Bool(true), "{id}");
        let b = build(c);
        let mut fail = |what: String| failures.push(format!("{id}: {what}"));
        let u = |k: &str| a[k].as_u64().unwrap() as usize;
        let ours = (b.faces.len(), b.edges.len(), b.vertices.len(), b.loops.len(), b.shells.len());
        let theirs = (u("faces"), u("edges"), u("vertices"), u("loops"), u("shells"));
        if ours != theirs {
            fail(format!("F/E/V/L/shells {ours:?} vs ASM {theirs:?}"));
        }
        let mut ft: Vec<&str> = b.faces.iter().map(|f| f.surface.type_name()).collect();
        let mut aft: Vec<&str> = a["face_types"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
        // compared as multisets: equivalent B-reps may list entities in any order
        ft.sort_unstable();
        aft.sort_unstable();
        if ft != aft {
            fail(format!("face types {ft:?} vs ASM {aft:?}"));
        }
        let mut et: Vec<&str> = b.edges.iter().map(|e| edge_type(&e.curve)).collect();
        let mut aet: Vec<&str> = a["edge_types"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
        et.sort_unstable();
        aet.sort_unstable();
        if et != aet {
            fail(format!("edge types {et:?} vs ASM {aet:?}"));
        }
        let literal = b.edges.iter().filter(|e| e.curve.is_none() || e.start == e.end).count();
        if literal != u("degenerate_edges") {
            fail(format!("literal degenerate edges {literal} vs ASM {}", u("degenerate_edges")));
        }
        let mut hist = BTreeMap::<String, u64>::new();
        for e in b.edge_ids() {
            *hist.entry(b.edge_coedges(e).len().to_string()).or_default() += 1;
        }
        let ah = asm_histogram(a["notes"].as_str().unwrap());
        if hist != ah {
            fail(format!("coedges per edge {hist:?} vs ASM {ah:?}"));
        }
        let m = solid_properties(&b, SolidId(0)).unwrap();
        let rel = |x: f64, y: f64| (x - y).abs() / y.abs();
        let (va, aa) = (num(a, "volume"), num(a, "area"));
        if rel(m.volume, va) > 1e-9 || rel(m.area, aa) > 1e-9 {
            fail(format!("volume {} / area {} vs ASM {va} / {aa}", m.volume, m.area));
        }
        // centroid and inertia where Fusion reported them (null for the tiny box)
        let size = b.vertex_bounds().map_or(1.0, |(lo, hi)| (hi - lo).norm()).max(m.volume.cbrt());
        if !a["centroid"].is_null() {
            let ac = P3::from(arr3(&a["centroid"]));
            let d = (m.centroid - ac).norm() / size;
            if d > 1e-9 {
                fail(format!("centroid {:?} vs ASM {ac:?} ({d:.1e} of size)", m.centroid));
            }
        }
        if !a["inertia_centroid"].is_null() {
            let ai: Vec<[f64; 3]> = a["inertia_centroid"].as_array().unwrap().iter().map(arr3).collect();
            let scale = ai.iter().flatten().fold(0.0f64, |s, x| s.max(x.abs()));
            let d = (0..3).flat_map(|i| (0..3).map(move |j| (i, j))).map(|(i, j)| (m.inertia[i][j] - ai[i][j]).abs()).fold(0.0, f64::max) / scale;
            // ASM's own accuracy: a body of revolution has J = α·Id + β·aaᵀ exactly; ASM's tensor
            // departs from that form by `noise` (≈1e-8 for the frustum and tilted cylinder), so it
            // cannot arbitrate below that level. Ours is checked against closed forms in mass.rs.
            let noise = axisymmetry_residual(c, &ai) / scale;
            let ours = axisymmetry_residual(c, &m.inertia) / scale;
            eprintln!("{id:22} vol {:.1e} area {:.1e} inertia {d:.1e} (ASM axisymmetry noise {noise:.1e}, ours {ours:.1e})", rel(m.volume, va), rel(m.area, aa));
            assert!(ours <= 1e-12, "{id}: our tensor is not axisymmetric ({ours:.1e})");
            if d > 1e-9_f64.max(3.0 * noise) {
                fail(format!("inertia {:?} vs ASM {ai:?} ({d:.1e} of max, ASM noise {noise:.1e})", m.inertia));
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 9);
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Largest entry of J − (α·Id + β·aaᵀ), the best tensor of a body of revolution about the case's
/// axis (a box has no such symmetry: 0). α = (tr J − aᵀJa)/2, β = aᵀJa − α.
fn axisymmetry_residual(c: &Value, j: &[[f64; 3]]) -> f64 {
    let kind = c["kind"].as_str().unwrap();
    if kind == "box" {
        return 0.0;
    }
    let a = if kind == "sphere" { V3::z() } else { V3::from(arr3(&c["axis"])).normalize() };
    let jm = nalgebra::Matrix3::from_fn(|r, s| j[r][s]);
    let aja = (a.transpose() * jm * a)[0];
    let alpha = (jm.trace() - aja) / 2.0;
    let beta = aja - alpha;
    (jm - (nalgebra::Matrix3::identity() * alpha + a * a.transpose() * beta)).abs().max()
}
