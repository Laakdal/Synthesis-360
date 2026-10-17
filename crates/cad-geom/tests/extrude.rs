//! Extrude: every `ext.*` case of `research/feature-cases.json` against closed forms (volume,
//! area, centroid), validation at the ASM and OCCT tolerances, watertight tessellation, the
//! primitives it must reproduce exactly (box, cylinder) and the ASM record of the same solid
//! built by a boolean (`bool.cut.hole` in `research/evidence/asm-v5`).

#[path = "support/features.rs"]
mod features;

use cad_geom::mass::solid_properties;
use cad_geom::primitives::{make_box, make_cylinder};
use cad_geom::tess::tessellate_solid;
use cad_geom::{extrude, validate, Brep, Frame, GeomError, MassProperties, Profile, Seg, SolidId, Tolerance, P2, P3, V3};
use serde_json::Value;
use std::f64::consts::PI;
use features::profile;

fn build(id: &str) -> Brep {
    let c = features::case(id);
    let (from, to) = (c["from"].as_f64().unwrap(), c["to"].as_f64().unwrap());
    extrude(&profile(&c), from, to, &Tolerance::asm()).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn mass(b: &Brep) -> MassProperties {
    solid_properties(b, SolidId(0)).unwrap()
}

fn counts(b: &Brep) -> (usize, usize, usize, usize) {
    (b.faces.len(), b.edges.len(), b.vertices.len(), b.loops.len())
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-300)
}

fn assert_valid(id: &str, b: &Brep, genus: i64) {
    for tol in [Tolerance::asm(), Tolerance::occt()] {
        let r = validate(b, &tol);
        assert!(r.is_valid(), "{id} at {tol:?}: {:?}", r.issues);
        assert_eq!(r.genus, genus, "{id}: a through hole is a handle");
    }
}

/// Expected (F, E, V, L), volume, area and local centroid (profile plane coordinates and offset).
struct Expect {
    id: &'static str,
    fevl: (usize, usize, usize, usize),
    volume: f64,
    area: f64,
    centroid_local: [f64; 3],
}

fn expectations() -> Vec<Expect> {
    let rect_area = 2.0 * 1500.0 + 160.0 * 10.0;
    let hole = 1500.0 - 36.0 * PI;
    let slot_a = 300.0 + 25.0 * PI;
    let slot_p = 60.0 + 10.0 * PI;
    let notch_a = 800.0 - 50.0 * PI;
    let notch_p = 40.0 + 20.0 + 10.0 + 10.0 * PI + 10.0 + 20.0;
    // notch centroid y: rectangle (800 at y=10) minus half disc below y=20 (centroid 20 − 4r/3π)
    let half = 50.0 * PI;
    let notch_cy = (800.0 * 10.0 - half * (20.0 - 40.0 / (3.0 * PI))) / notch_a;
    let tilted_a = 200.0 - 9.0 * PI;
    vec![
        Expect { id: "ext.rect", fevl: (6, 12, 8, 6), volume: 15000.0, area: rect_area, centroid_local: [25.0, 15.0, 5.0] },
        Expect { id: "ext.rect.negative", fevl: (6, 12, 8, 6), volume: 15000.0, area: rect_area, centroid_local: [25.0, 15.0, -5.0] },
        Expect { id: "ext.rect.symmetric", fevl: (6, 12, 8, 6), volume: 15000.0, area: rect_area, centroid_local: [25.0, 15.0, 0.0] },
        Expect { id: "ext.circle", fevl: (3, 2, 2, 4), volume: 2000.0 * PI, area: 200.0 * PI + 400.0 * PI, centroid_local: [0.0, 0.0, 10.0] },
        Expect { id: "ext.rect_hole", fevl: (7, 14, 10, 10), volume: hole * 10.0, area: 2.0 * hole + 1600.0 + 120.0 * PI, centroid_local: [25.0, 15.0, 5.0] },
        Expect { id: "ext.slot", fevl: (6, 12, 8, 6), volume: slot_a * 5.0, area: 2.0 * slot_a + slot_p * 5.0, centroid_local: [15.0, 5.0, 2.5] },
        Expect { id: "ext.lshape", fevl: (8, 18, 12, 8), volume: 4800.0, area: 1200.0 + 140.0 * 8.0, centroid_local: [(400.0 * 20.0 + 200.0 * 5.0) / 600.0, (400.0 * 5.0 + 200.0 * 20.0) / 600.0, 4.0] },
        Expect { id: "ext.notch", fevl: (8, 18, 12, 8), volume: notch_a * 6.0, area: 2.0 * notch_a + notch_p * 6.0, centroid_local: [20.0, notch_cy, 3.0] },
        Expect { id: "ext.tilted", fevl: (7, 14, 10, 10), volume: tilted_a * 7.0, area: 2.0 * tilted_a + (60.0 + 6.0 * PI) * 7.0, centroid_local: [10.0, 5.0, 3.5] },
    ]
}

#[test]
fn extrude_cases_match_closed_forms() {
    let all = features::cases();
    let ext_ids: Vec<&str> = all.iter().filter_map(|c| c["id"].as_str()).filter(|i| i.starts_with("ext.")).collect();
    let ex = expectations();
    assert_eq!(ext_ids.len(), ex.len(), "every ext.* case has an expectation");
    for e in &ex {
        let c = features::case(e.id);
        let b = build(e.id);
        assert_eq!(counts(&b), e.fevl, "{} F/E/V/L", e.id);
        assert_valid(e.id, &b, c["profile"]["loops"].as_array().unwrap().len() as i64 - 1);
        let m = mass(&b);
        assert!(rel(m.volume, e.volume) < 1e-12, "{} volume {} vs {}", e.id, m.volume, e.volume);
        assert!(rel(m.area, e.area) < 1e-12, "{} area {} vs {}", e.id, m.area, e.area);
        let p = profile(&c);
        let want = p.plane.to_world(P3::from(e.centroid_local));
        assert!((m.centroid - want).norm() < 1e-9, "{} centroid {:?} vs {want:?}", e.id, m.centroid);
        assert!(m.inertia[0][0] > 0.0 && m.inertia[1][1] > 0.0 && m.inertia[2][2] > 0.0, "{}", e.id);
    }
}

#[test]
fn extruded_rectangle_and_circle_equal_the_primitives() {
    let pairs = [
        (build("ext.rect"), make_box(P3::origin(), [50.0, 30.0, 10.0]).unwrap()),
        (build("ext.circle"), make_cylinder(Frame::world(), 10.0, 20.0).unwrap()),
    ];
    for (ext, prim) in pairs {
        assert_eq!(counts(&ext), counts(&prim));
        let mut ft: Vec<_> = ext.faces.iter().map(|f| f.surface.type_name()).collect();
        let mut pt: Vec<_> = prim.faces.iter().map(|f| f.surface.type_name()).collect();
        ft.sort_unstable();
        pt.sort_unstable();
        assert_eq!(ft, pt);
        let (a, b) = (mass(&ext), mass(&prim));
        assert!(rel(a.volume, b.volume) < 1e-13 && rel(a.area, b.area) < 1e-13);
        assert!((a.centroid - b.centroid).norm() < 1e-12);
        let scale = b.inertia.iter().flatten().fold(0.0f64, |s, x| s.max(x.abs()));
        for i in 0..3 {
            for j in 0..3 {
                assert!((a.inertia[i][j] - b.inertia[i][j]).abs() < 1e-12 * scale, "inertia {i}{j}");
            }
        }
    }
}

/// `ext.rect_hole` is the solid ASM recorded for `bool.cut.hole` (box 50×30×10 minus a Ø12 through
/// cylinder at (25, 15)). This compares the *same solid*; it does not show that ASM's extrude
/// builds it with this topology (that needs `research/evidence/asm-v6`).
#[test]
fn extruded_plate_with_hole_matches_asm_boolean_record() {
    let asm = features::load("research/evidence/asm-v5/reference.json");
    let a = asm["cases"].as_array().unwrap().iter().find(|x| x["id"] == "bool.cut.hole").unwrap().clone();
    let b = build("ext.rect_hole");
    let u = |k: &str| a[k].as_u64().unwrap() as usize;
    assert_eq!((b.faces.len(), b.edges.len(), b.vertices.len(), b.loops.len(), b.shells.len()), (u("faces"), u("edges"), u("vertices"), u("loops"), u("shells")));
    let sorted = |v: &Value| {
        let mut s: Vec<String> = v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
        s.sort();
        s
    };
    let mut ft: Vec<String> = b.faces.iter().map(|f| f.surface.type_name().to_string()).collect();
    ft.sort();
    assert_eq!(ft, sorted(&a["face_types"]));
    let mut et: Vec<String> = b.edges.iter().map(|e| e.curve.as_ref().unwrap().type_name().to_string()).collect();
    et.sort();
    assert_eq!(et, sorted(&a["edge_types"]));
    let m = mass(&b);
    let num = |k: &str| a[k].as_f64().unwrap();
    assert!(rel(m.volume, num("volume")) < 1e-9 && rel(m.area, num("area")) < 1e-9);
    let ac = features::p3(&a["centroid"]);
    assert!((m.centroid - ac).norm() < 1e-9 * 60.0);
    let ai: Vec<V3> = a["inertia_centroid"].as_array().unwrap().iter().map(features::v3).collect();
    assert_inertia("ext.rect_hole", &m.inertia, &ai);
}

#[test]
fn extrude_tessellates_watertight_with_exact_planar_volume() {
    // (case, smallest arc radius; 0 = polyhedral). An inscribed polygon of sagitta s loses about
    // (4/3)·s/r of a circle's area, so curved cases are held to 2·chord/r.
    let chord = 0.01;
    for (id, rmin) in [("ext.rect", 0.0), ("ext.lshape", 0.0), ("ext.rect_hole", 6.0), ("ext.slot", 5.0), ("ext.notch", 10.0), ("ext.tilted", 3.0), ("ext.circle", 10.0)] {
        let b = build(id);
        let exact = mass(&b).volume;
        let mesh = tessellate_solid(&b, SolidId(0), chord).unwrap_or_else(|e| panic!("{id}: {e}"));
        features::assert_watertight(id, &mesh);
        let v = mesh.signed_volume();
        let bound = if rmin == 0.0 { 1e-12 } else { 2.0 * chord / rmin };
        assert!(rel(v, exact) < bound, "{id}: mesh volume {v} vs {exact} (bound {bound:.1e})");
    }
}

#[test]
fn extrude_is_independent_of_input_loop_orientation_and_order() {
    let c = features::case("ext.rect_hole");
    let p = profile(&c);
    let flipped = Profile { plane: p.plane, loops: p.loops.iter().rev().map(|l| l.iter().rev().map(Seg::reversed).collect()).collect() };
    let a = mass(&extrude(&p, 0.0, 10.0, &Tolerance::asm()).unwrap());
    let b = mass(&extrude(&flipped, 10.0, 0.0, &Tolerance::asm()).unwrap());
    assert!(rel(a.volume, b.volume) < 1e-14 && rel(a.area, b.area) < 1e-14 && (a.centroid - b.centroid).norm() < 1e-12);
}

#[test]
fn extrude_rejects_bad_input() {
    let c = features::case("ext.rect");
    let p = profile(&c);
    let tol = Tolerance::asm();
    assert!(matches!(extrude(&p, 3.0, 3.0, &tol), Err(GeomError::InvalidInput(_))));
    assert!(matches!(extrude(&p, 0.0, f64::NAN, &tol), Err(GeomError::InvalidInput(_))));
    let mut open = p.clone();
    open.loops[0].pop();
    assert!(matches!(extrude(&open, 0.0, 1.0, &tol), Err(GeomError::InvalidInput(_))));
    let mut skew = p.clone();
    skew.plane.y = V3::new(0.0, 1.0, 1.0);
    assert!(matches!(extrude(&skew, 0.0, 1.0, &tol), Err(GeomError::InvalidInput(_))));
}

#[test]
fn extrude_scale_range() {
    // the same L shape at 1e-2 and 1e4 scale: exact volume scales with s³, validates at ASM tolerance
    let c = features::case("ext.lshape");
    let p = profile(&c);
    for s in [1e-2, 1e4] {
        let loops = p
            .loops
            .iter()
            .map(|l| {
                l.iter()
                    .map(|g| match *g {
                        Seg::Line { a, b } => Seg::Line { a: P2::from(a.coords * s), b: P2::from(b.coords * s) },
                        Seg::Arc { c, r, a0, sweep } => Seg::Arc { c: P2::from(c.coords * s), r: r * s, a0, sweep },
                    })
                    .collect()
            })
            .collect();
        let b = extrude(&Profile { plane: p.plane, loops }, 0.0, 8.0 * s, &Tolerance::asm()).unwrap();
        assert!(validate(&b, &Tolerance::asm()).is_valid());
        assert!(rel(mass(&b).volume, 4800.0 * s * s * s) < 1e-12);
    }
}

/// Every `ext.*` case against ASM's own extrude (`research/evidence/asm-v6/reference.json`, Fusion
/// ExtrudeFeature, probe v6): F/E/V/L/shells, face and edge types as multisets (ASM's `arc` is a
/// bounded circle), the literal degenerate-edge count, volume/area/centroid within 1e-9 and the
/// inertia tensor within 1e-9 of its largest entry.
#[test]
fn extrude_matches_asm_feature_reference() {
    let asm = features::load("research/evidence/asm-v6/reference.json");
    let rows = asm["cases"].as_array().unwrap();
    let mut checked = 0;
    for a in rows.iter().filter(|a| a["id"].as_str().unwrap().starts_with("ext.")) {
        let id = a["id"].as_str().unwrap();
        assert_eq!(a["ok"], Value::Bool(true), "{id}");
        let b = build(id);
        let u = |k: &str| a[k].as_u64().unwrap() as usize;
        assert_eq!(
            (b.faces.len(), b.edges.len(), b.vertices.len(), b.loops.len(), b.shells.len()),
            (u("faces"), u("edges"), u("vertices"), u("loops"), u("shells")),
            "{id} F/E/V/L/shells"
        );
        let sorted = |v: &Value| {
            let mut s: Vec<String> = v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().replace("arc", "circle")).collect();
            s.sort();
            s
        };
        let mut ft: Vec<String> = b.faces.iter().map(|f| f.surface.type_name().to_string()).collect();
        ft.sort();
        assert_eq!(ft, sorted(&a["face_types"]), "{id} face types");
        let mut et: Vec<String> = b.edges.iter().map(|e| e.curve.as_ref().map_or("none", |c| c.type_name()).to_string()).collect();
        et.sort();
        assert_eq!(et, sorted(&a["edge_types"]), "{id} edge types");
        let literal = b.edges.iter().filter(|e| e.curve.is_none() || e.start == e.end).count();
        assert_eq!(literal, u("degenerate_edges"), "{id} literal degenerate edges");
        let m = mass(&b);
        let num = |k: &str| a[k].as_f64().unwrap();
        assert!(rel(m.volume, num("volume")) < 1e-9, "{id} volume {} vs ASM {}", m.volume, num("volume"));
        assert!(rel(m.area, num("area")) < 1e-9, "{id} area {} vs ASM {}", m.area, num("area"));
        let size = b.vertex_bounds().map_or(1.0, |(lo, hi)| (hi - lo).norm());
        assert!((m.centroid - features::p3(&a["centroid"])).norm() < 1e-9 * size, "{id} centroid");
        let ai: Vec<V3> = a["inertia_centroid"].as_array().unwrap().iter().map(features::v3).collect();
        assert_inertia(id, &m.inertia, &ai);
        checked += 1;
    }
    assert_eq!(checked, 9);
}

/// Inertia tensors agree within 1e-9 of the reference's largest entry.
fn assert_inertia(id: &str, ours: &[[f64; 3]; 3], reference: &[V3]) {
    let scale = reference.iter().map(|r| r.amax()).fold(0.0, f64::max);
    for (i, (row, r)) in ours.iter().zip(reference).enumerate() {
        for (j, x) in row.iter().enumerate() {
            assert!((x - r[j]).abs() <= 1e-9 * scale, "{id} inertia {i}{j}: {x} vs {}", r[j]);
        }
    }
}
