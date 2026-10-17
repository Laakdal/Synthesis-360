//! Parity with ASM for the boolean cases of `research/cases.json` (the six box-only cases, and the
//! four with a cylinder or sphere in `curved_booleans_match_asm`): the tools are folded
//! into the target in order with `cad_geom::boolean` at ASM's tolerance, and the result is compared
//! with `research/evidence/asm-v5/reference.json`: F/E/V/L, shells, lumps, face and edge types
//! (sorted multisets), the literal degenerate-edge count, volume/area/centroid (and inertia where
//! recorded) within 1e-9 relative, and a clean `validate` at the default (1e-7 mm) tolerance.

use cad_geom::mass::solid_properties;
use cad_geom::primitives::{make_box, make_cylinder, make_sphere};
use cad_geom::{boolean, validate, BoolOp, Brep, Curve, Frame, SolidId, Tolerance, P3, V3};
use serde_json::Value;
use std::path::PathBuf;

const CASES: [&str; 6] = [
    "bool.union.overlap",
    "bool.union.coincident_face",
    "bool.union.coincident_partial",
    "bool.cut.coplanar_through",
    "bool.cut.disjoint",
    "bool.union.repeated",
];

fn load(rel: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

fn arr3(v: &Value) -> [f64; 3] {
    let a = v.as_array().unwrap();
    [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()]
}

fn make(spec: &Value) -> Brep {
    match spec["kind"].as_str().unwrap() {
        "box" => make_box(P3::from(arr3(&spec["corner"])), arr3(&spec["size"])).unwrap(),
        "cylinder" => {
            assert_eq!(spec["r0"], spec["r1"]);
            let frame = Frame::from_origin_z(P3::from(arr3(&spec["base"])), V3::from(arr3(&spec["axis"])));
            make_cylinder(frame, spec["r0"].as_f64().unwrap(), spec["h"].as_f64().unwrap()).unwrap()
        }
        "sphere" => make_sphere(P3::from(arr3(&spec["center"])), spec["r"].as_f64().unwrap()).unwrap(),
        k => panic!("operand kind {k}"),
    }
}

fn op(c: &Value) -> BoolOp {
    match c["op"].as_str().unwrap() {
        "union" => BoolOp::Union,
        "cut" => BoolOp::Cut,
        "intersect" => BoolOp::Intersect,
        o => panic!("op {o}"),
    }
}

/// F/E/V/L after each tool of the fold.
fn fold(c: &Value) -> (Brep, Vec<[usize; 4]>) {
    let mut acc = make(&c["target"]);
    let mut steps = Vec::new();
    for t in c["tools"].as_array().unwrap() {
        acc = boolean(&acc, &make(t), op(c), &Tolerance::asm()).unwrap_or_else(|e| panic!("{}: {e}", c["id"]));
        steps.push([acc.faces.len(), acc.edges.len(), acc.vertices.len(), acc.loops.len()]);
    }
    (acc, steps)
}

fn edge_type(c: &Option<Curve>) -> &'static str {
    c.as_ref().map_or("none", |c| c.type_name())
}

#[test]
fn box_booleans_match_asm() {
    let cases = load("research/cases.json");
    let asm = load("research/evidence/asm-v5/reference.json");
    let mut failures = Vec::new();
    for id in CASES {
        let c = cases["cases"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap();
        let a = asm["cases"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap();
        let (b, _) = fold(c);
        let mut fail = |what: String| failures.push(format!("{id}: {what}"));
        let u = |k: &str| a[k].as_u64().unwrap() as usize;
        let ours = (b.faces.len(), b.edges.len(), b.vertices.len(), b.loops.len(), b.shells.len(), b.solids.len());
        let theirs = (u("faces"), u("edges"), u("vertices"), u("loops"), u("shells"), u("lumps"));
        if ours != theirs {
            fail(format!("F/E/V/L/shells/lumps {ours:?} vs ASM {theirs:?}"));
        }
        let sorted = |mut v: Vec<String>| {
            v.sort();
            v
        };
        let ft = sorted(b.faces.iter().map(|f| f.surface.type_name().to_string()).collect());
        let aft = sorted(a["face_types"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect());
        if ft != aft {
            fail(format!("face types {ft:?} vs ASM {aft:?}"));
        }
        let et = sorted(b.edges.iter().map(|e| edge_type(&e.curve).to_string()).collect());
        let aet = sorted(a["edge_types"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect());
        if et != aet {
            fail(format!("edge types {et:?} vs ASM {aet:?}"));
        }
        let literal = b.edges.iter().filter(|e| e.curve.is_none() || e.start == e.end).count();
        if literal != u("degenerate_edges") {
            fail(format!("literal degenerate edges {literal} vs ASM {}", u("degenerate_edges")));
        }
        let rep = validate(&b, &Tolerance::default());
        if !rep.is_valid() {
            fail(format!("validate: {:?}", rep.issues));
        }
        let m = solid_properties(&b, SolidId(0)).unwrap();
        let rel = |x: f64, y: f64| (x - y).abs() / y.abs();
        let (va, aa) = (a["volume"].as_f64().unwrap(), a["area"].as_f64().unwrap());
        if rel(m.volume, va) > 1e-9 || rel(m.area, aa) > 1e-9 {
            fail(format!("volume {} / area {} vs ASM {va} / {aa}", m.volume, m.area));
        }
        let size = b.vertex_bounds().map_or(1.0, |(lo, hi)| (hi - lo).norm());
        let ac = P3::from(arr3(&a["centroid"]));
        if (m.centroid - ac).norm() / size > 1e-9 {
            fail(format!("centroid {:?} vs ASM {ac:?}", m.centroid));
        }
        if let Some(ai) = a["inertia_centroid"].as_array() {
            let ai: Vec<[f64; 3]> = ai.iter().map(arr3).collect();
            let scale = ai.iter().flatten().fold(0.0f64, |s, x| s.max(x.abs()));
            let d = (0..3).flat_map(|i| (0..3).map(move |j| (i, j))).map(|(i, j)| (m.inertia[i][j] - ai[i][j]).abs()).fold(0.0, f64::max);
            if d / scale > 1e-9 {
                fail(format!("inertia {:?} vs ASM {ai:?}", m.inertia));
            }
        }
        eprintln!("{id:32} F/E/V/L {:?} V {:.6} A {:.6}", &ours, m.volume, m.area);
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The per-step counts recorded for `bool.union.repeated` (REFERENCE_BEHAVIOR.md): merging and
/// loop bookkeeping hold after every step of the fold, not only at the end.
#[test]
fn repeated_union_steps_match_asm() {
    let cases = load("research/cases.json");
    let c = cases["cases"].as_array().unwrap().iter().find(|x| x["id"] == "bool.union.repeated").unwrap();
    let (b, steps) = fold(c);
    let fev: Vec<[usize; 3]> = steps.iter().map(|s| [s[0], s[1], s[2]]).collect();
    assert_eq!(fev, [[11, 24, 16], [16, 36, 24], [21, 48, 32], [26, 60, 40], [31, 72, 48]]);
    // the base top face ends with 1 outer and 5 inner loops
    assert_eq!(b.faces.iter().map(|f| f.loops.len()).max(), Some(6));
}

/// Loop structure: the through-hole's top and bottom faces carry 2 loops each, the partial
/// coincident union's contact face 2 loops; every other face 1.
#[test]
fn loop_structure_matches_asm() {
    let cases = load("research/cases.json");
    for (id, want) in [("bool.cut.coplanar_through", vec![1, 1, 1, 1, 1, 1, 1, 1, 2, 2]), ("bool.union.coincident_partial", [vec![1; 10], vec![2]].concat())] {
        let c = cases["cases"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap();
        let (b, _) = fold(c);
        let mut loops: Vec<usize> = b.faces.iter().map(|f| f.loops.len()).collect();
        loops.sort_unstable();
        assert_eq!(loops, want, "{id}");
    }
}

/// The four cylinder/sphere cases (quadric pipeline). `vtol` is the tolerance the result must
/// validate at, `rel` the relative bound on volume, area and centroid (× model size), `irel` on
/// the inertia tensor (× its largest entry).
struct Curved {
    id: &'static str,
    vtol: Tolerance,
    rel: f64,
    irel: f64,
    /// Sorted loops per face.
    loops: &'static [usize],
    /// Sorted coedges per edge.
    uses: &'static [(usize, usize)],
}

/// Parity with ASM for `bool.cut.hole`, `bool.union.tangent_cylinder`,
/// `bool.union.near_tangent_cylinder` and `bool.intersect.box_sphere`: F/E/V/L, shells, lumps,
/// face and edge types (sorted multisets), the literal degenerate-edge count, loops per face,
/// coedges per edge (the tangent cases' contact line: ONE edge with 4 coedges, flagged
/// non-manifold), volume, area, centroid and inertia. The exact cases validate at 1e-7 mm and agree
/// with ASM within 1e-9 (inertia 1e-8: ASM's base-feature accuracy). The near-tangent case
/// validates at ASM's 1e-5 mm (its contact line lies within tolerance of both the plane and the
/// cylinder, halfway between them) and agrees within 1e-7: placing the contact vertices halfway
/// moves the box's top and bottom faces by 5e-6 mm, about 3e-4 mm³ (2.3e-8 relative), where ASM
/// reports the plain sum of the two volumes.
#[test]
fn curved_booleans_match_asm() {
    let exact = Tolerance::default();
    let cases = [
        Curved { id: "bool.cut.hole", vtol: exact, rel: 1e-9, irel: 1e-8, loops: &[1, 1, 1, 1, 2, 2, 2], uses: &[(2, 14)] },
        Curved { id: "bool.union.tangent_cylinder", vtol: exact, rel: 1e-9, irel: 1e-8, loops: &[1; 10], uses: &[(2, 16), (4, 1)] },
        Curved { id: "bool.union.near_tangent_cylinder", vtol: Tolerance::asm(), rel: 1e-7, irel: 1e-7, loops: &[1; 10], uses: &[(2, 16), (4, 1)] },
        Curved { id: "bool.intersect.box_sphere", vtol: exact, rel: 1e-9, irel: 1e-8, loops: &[1, 1, 1, 1, 1, 1, 6], uses: &[(2, 6)] },
    ];
    let catalogue = load("research/cases.json");
    let asm = load("research/evidence/asm-v5/reference.json");
    let mut failures = Vec::new();
    for k in &cases {
        let id = k.id;
        let c = catalogue["cases"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap();
        let a = asm["cases"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap();
        let (b, _) = fold(c);
        let mut fail = |what: String| failures.push(format!("{id}: {what}"));
        let u = |k: &str| a[k].as_u64().unwrap() as usize;
        let ours = (b.faces.len(), b.edges.len(), b.vertices.len(), b.loops.len(), b.shells.len(), b.solids.len());
        let theirs = (u("faces"), u("edges"), u("vertices"), u("loops"), u("shells"), u("lumps"));
        if ours != theirs {
            fail(format!("F/E/V/L/shells/lumps {ours:?} vs ASM {theirs:?}"));
        }
        let sorted = |mut v: Vec<String>| {
            v.sort();
            v
        };
        let ft = sorted(b.faces.iter().map(|f| f.surface.type_name().to_string()).collect());
        let aft = sorted(a["face_types"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect());
        if ft != aft {
            fail(format!("face types {ft:?} vs ASM {aft:?}"));
        }
        let et = sorted(b.edges.iter().map(|e| edge_type(&e.curve).to_string()).collect());
        let aet = sorted(a["edge_types"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect());
        if et != aet {
            fail(format!("edge types {et:?} vs ASM {aet:?}"));
        }
        let literal = b.edges.iter().filter(|e| e.curve.is_none() || e.start == e.end).count();
        if literal != u("degenerate_edges") {
            fail(format!("literal degenerate edges {literal} vs ASM {}", u("degenerate_edges")));
        }
        let mut loops: Vec<usize> = b.faces.iter().map(|f| f.loops.len()).collect();
        loops.sort_unstable();
        if loops != k.loops {
            fail(format!("loops per face {loops:?} vs {:?}", k.loops));
        }
        let mut hist = std::collections::BTreeMap::new();
        for e in b.edge_ids() {
            *hist.entry(b.edge_coedges(e).len()).or_insert(0usize) += 1;
        }
        let hist: Vec<(usize, usize)> = hist.into_iter().collect();
        if hist != k.uses {
            fail(format!("coedges per edge {hist:?} vs {:?}", k.uses));
        }
        let flagged = k.uses.iter().filter(|x| x.0 > 2).map(|x| x.1).sum::<usize>();
        if b.nonmanifold_edges.len() != flagged {
            fail(format!("{} flagged non-manifold edges vs {flagged}", b.nonmanifold_edges.len()));
        }
        let rep = validate(&b, &k.vtol);
        if !rep.is_valid() {
            fail(format!("validate: {:?}", rep.issues));
        }
        let m = solid_properties(&b, SolidId(0)).unwrap();
        let rel = |x: f64, y: f64| (x - y).abs() / y.abs();
        let (va, aa) = (a["volume"].as_f64().unwrap(), a["area"].as_f64().unwrap());
        if rel(m.volume, va) > k.rel || rel(m.area, aa) > k.rel {
            fail(format!("volume {} / area {} vs ASM {va} / {aa}", m.volume, m.area));
        }
        let size = b.vertex_bounds().map_or(1.0, |(lo, hi)| (hi - lo).norm()).max(1.0);
        let ac = P3::from(arr3(&a["centroid"]));
        if (m.centroid - ac).norm() / size > k.rel {
            fail(format!("centroid {:?} vs ASM {ac:?}", m.centroid));
        }
        let ai: Vec<[f64; 3]> = a["inertia_centroid"].as_array().unwrap().iter().map(arr3).collect();
        let scale = ai.iter().flatten().fold(0.0f64, |s, x| s.max(x.abs()));
        let d = (0..3).flat_map(|i| (0..3).map(move |j| (i, j))).map(|(i, j)| (m.inertia[i][j] - ai[i][j]).abs()).fold(0.0, f64::max);
        if d / scale > k.irel {
            fail(format!("inertia {:?} vs ASM {ai:?} (rel {:.2e})", m.inertia, d / scale));
        }
        eprintln!(
            "{id:34} F/E/V/L/sh/lumps {ours:?} V {:.9} (ASM {va:.9}, rel {:.1e}) A {:.9} (rel {:.1e}) inertia rel {:.1e}",
            m.volume,
            rel(m.volume, va),
            m.area,
            rel(m.area, aa),
            d / scale
        );
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
