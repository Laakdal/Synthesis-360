//! Replays the workflow recorded in the Fusion 2705.1.30 probe (docs/evidence/fusion-probe) and
//! checks CAD360's geometry against the volumes Fusion reported for the same operations.

use cad_core::ops;
use cad_core::sketch::{ConstraintKind as K, DistanceOrientation as O, EntityKind, ORIGIN};
use cad_core::*;
use cad_kernel::{refs, Health, Kernel, Model};

/// Rectangle with H/V constraints, corner on the origin, width/height dimensions (probe sketch).
fn rectangle_sketch(doc: &mut Document, w: &str, h: &str) -> (FeatureId, String, String) {
    let sk = ops::new_sketch(doc, PlaneRef::Origin { plane: OriginPlane::XY });
    let s = ops::sketch_mut(doc, &sk).unwrap();
    let lines = s.add_rectangle([1.0, 1.0], [40.0, 20.0]);
    let corner = match &s.entity(&lines[0]).unwrap().kind {
        EntityKind::Line { p1, .. } => p1.clone(),
        _ => unreachable!(),
    };
    s.add_constraint(K::Coincident { a: corner, b: ORIGIN.into() });
    let (_, pw) = ops::add_dimension(doc, &sk, K::Distance { a: lines[0].clone(), b: lines[0].clone(), orientation: O::Horizontal }, w, [20.0, -8.0], false).unwrap();
    let (_, ph) = ops::add_dimension(doc, &sk, K::Distance { a: lines[1].clone(), b: lines[1].clone(), orientation: O::Vertical }, h, [48.0, 10.0], false).unwrap();
    (sk, pw, ph)
}

fn whole_profile(model: &Model, sk: &str) -> Vec<ProfileRef> {
    let r = &model.sketches[sk];
    let p = &r.profiles[0];
    vec![ProfileRef { curves: p.curves.clone(), sample: p.interior_point() }]
}

fn recompute(k: &mut Kernel, doc: &mut Document) -> Model {
    let m = k.recompute(doc);
    cad_kernel::store_solved_sketches(doc, &m);
    m
}

fn volume(m: &Model) -> f64 {
    m.bodies.iter().map(|b| b.volume()).sum()
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn sketch_extrude_edit_fillet_cut_like_fusion() {
    let mut doc = Document::new("probe");
    let mut k = Kernel::new();
    let (sk, pw, _ph) = rectangle_sketch(&mut doc, "50 mm", "30 mm");
    let m = recompute(&mut k, &mut doc);
    assert!(m.sketches[&sk].report.is_fully_constrained(), "probe: isFullyConstrained = True");
    assert_eq!(m.sketches[&sk].profiles.len(), 1);

    // Extrude 10 mm -> Fusion: V = 15000 mm³, bbox [0,0,0]-[50,30,10], 6 faces, 12 edges
    let ext = ops::new_extrude(&mut doc, &sk, whole_profile(&m, &sk), "10 mm", ExtentDirection::Normal, Operation::New);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&ext), Health::Healthy);
    assert_eq!(m.bodies.len(), 1);
    let b = &m.bodies[0];
    assert!(close(b.volume(), 15000.0, 1e-6), "{}", b.volume());
    let t = b.topology();
    assert_eq!((t.faces.len(), t.edges.len()), (6, 12));
    for (x, y) in t.bbox.iter().zip([0.0, 0.0, 0.0, 50.0, 30.0, 10.0]) {
        assert!(close(*x, y, 1e-6), "{:?}", t.bbox);
    }

    // Edit extrude distance -> 30000; edit sketch width -> 36000 (Fusion values)
    let dist = ops::primary_parameter(doc.feature(&ext).unwrap()).unwrap().to_string();
    doc.set_expression(&dist, "20 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert!(close(volume(&m), 30000.0, 1e-6));
    assert_eq!(m.recomputed_from, Some(1), "the sketch is served from cache; the extrude recomputes");
    doc.set_expression(&pw, "60 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert!(close(volume(&m), 36000.0, 1e-6));

    // Fillet the four vertical edges, R5 -> Fusion: 35570.8 mm³
    let body = &m.bodies[0];
    let topo = body.topology();
    let verticals: Vec<EdgeRef> = topo
        .edge_info
        .iter()
        .enumerate()
        .filter(|(_, e)| e.curve == CurveKind::Line && e.dir[2].abs() > 0.999)
        .map(|(i, _)| refs::edge_ref(body, i).unwrap())
        .collect();
    assert_eq!(verticals.len(), 4);
    let fil = ops::new_fillet(&mut doc, verticals, "5 mm");
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&fil), Health::Healthy);
    assert!(close(volume(&m), 35570.8, 0.1), "{}", volume(&m));
    assert_eq!(m.bodies[0].topology().faces.len(), 10, "Fusion: F10");

    // Upstream width edit with the fillet downstream -> Fusion: 41570.8 mm³ (fillet survives)
    doc.set_expression(&pw, "70 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&fil), Health::Healthy, "edge references re-resolve after the edit");
    assert!(close(volume(&m), 41570.8, 0.1), "{}", volume(&m));

    // Sketch on the top face + Ø12 through-all cut -> Fusion: 39308.8 mm³
    let top = {
        let b = &m.bodies[0];
        let t = b.topology();
        let i = t.face_info.iter().enumerate().filter(|(_, f)| f.surface == SurfaceKind::Plane && f.normal[2] > 0.99).map(|(i, _)| i).next().unwrap();
        refs::face_ref(b, i).unwrap()
    };
    let sk2 = ops::new_sketch(&mut doc, PlaneRef::Face { face: top });
    ops::sketch_mut(&mut doc, &sk2).unwrap().add_circle([35.0, 15.0], 6.0);
    let m = recompute(&mut k, &mut doc);
    let sr = &m.sketches[&sk2];
    assert_eq!(sr.frame.origin, [0.0, 0.0, 20.0], "probe: face sketch origin (0,0,2 cm)");
    let disc = sr.profiles.iter().find(|p| p.area < 200.0).unwrap();
    let cut = ops::new_extrude(&mut doc, &sk2, vec![ProfileRef { curves: disc.curves.clone(), sample: disc.interior_point() }], "0 mm", ExtentDirection::Reverse, Operation::Cut);
    if let FeatureKind::Extrude { through_all, .. } = &mut doc.feature_mut(&cut).unwrap().kind {
        *through_all = true;
    }
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&cut), Health::Healthy);
    assert!(close(volume(&m), 39308.8, 0.2), "{}", volume(&m));

    // Width 20: the hole now misses the body -> Fusion: "No target body! Compute Failed"
    doc.set_expression(&pw, "20 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert!(matches!(m.health_of(&cut), Health::Warning(ref s) if s.contains("No target body")), "{:?}", m.health_of(&cut));
    assert!(close(volume(&m), 11570.8, 0.1), "Fusion: 11570.8 mm³ ({})", volume(&m));
    doc.set_expression(&pw, "70 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&cut), Health::Healthy, "recovers like Fusion");

    // Timeline: roll back before the cut, suppress the fillet
    doc.set_marker(doc.feature_index(&cut).unwrap());
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&cut), Health::RolledBack);
    assert!(close(volume(&m), 41570.8, 0.1));
    doc.set_marker(doc.features.len());
    doc.feature_mut(&fil).unwrap().suppressed = true;
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&fil), Health::Suppressed);
    assert!(close(volume(&m), 70.0 * 30.0 * 20.0 - std::f64::consts::PI * 36.0 * 20.0, 0.1));
    doc.feature_mut(&fil).unwrap().suppressed = false;

    // Deleting the cut's sketch -> the cut reports a lost profile reference (Fusion: warning +
    // cached geometry; CAD360 reports an error and does not use stale geometry)
    let mut d2 = doc.clone();
    d2.delete_feature(&sk2).unwrap();
    let m2 = Kernel::new().recompute(&d2);
    assert!(m2.health_of(&cut).is_error());

    // Persistence round trip preserves the history and geometry
    let text = doc.to_json();
    let back = Document::from_json(&text).unwrap();
    let m3 = Kernel::new().recompute(&back);
    assert!(close(volume(&m3), 39308.8, 0.2));
    assert_eq!(back.features.len(), doc.features.len());
    assert_eq!(back.parameters, doc.parameters);
}

#[test]
fn invalid_operations_report_errors_instead_of_crashing() {
    let mut doc = Document::new("errors");
    let mut k = Kernel::new();
    let (sk, _pw, _) = rectangle_sketch(&mut doc, "50 mm", "30 mm");
    let m = recompute(&mut k, &mut doc);
    // zero distance -> Fusion: "Some input argument is invalid."
    let ext = ops::new_extrude(&mut doc, &sk, whole_profile(&m, &sk), "0 mm", ExtentDirection::Normal, Operation::New);
    let m = recompute(&mut k, &mut doc);
    assert!(m.health_of(&ext).is_error());
    assert!(m.bodies.is_empty());
    let p = ops::primary_parameter(doc.feature(&ext).unwrap()).unwrap().to_string();
    doc.set_expression(&p, "20 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&ext), Health::Healthy);

    // A fillet radius far too large must fail cleanly (OCCT reports failure; no crash)
    let body = &m.bodies[0];
    let e: Vec<EdgeRef> = (0..body.topology().edges.len()).map(|i| refs::edge_ref(body, i).unwrap()).collect();
    let fil = ops::new_fillet(&mut doc, e, "40 mm");
    let m = recompute(&mut k, &mut doc);
    assert!(m.health_of(&fil).is_error(), "{:?}", m.health_of(&fil));
    assert!(close(volume(&m), 30000.0, 1e-6), "a failed fillet leaves the body unchanged");

    // A cut that misses everything warns
    let sk3 = ops::new_sketch(&mut doc, PlaneRef::Origin { plane: OriginPlane::XY });
    ops::sketch_mut(&mut doc, &sk3).unwrap().add_circle([500.0, 500.0], 5.0);
    let m = recompute(&mut k, &mut doc);
    let c = ops::new_extrude(&mut doc, &sk3, whole_profile(&m, &sk3), "10 mm", ExtentDirection::Normal, Operation::Cut);
    let m = recompute(&mut k, &mut doc);
    assert!(matches!(m.health_of(&c), Health::Warning(_)));
}

#[test]
fn revolve_chamfer_hole_plane_and_exchange() {
    let mut doc = Document::new("misc");
    let mut k = Kernel::new();
    // revolve a 10x20 rectangle on XZ around the Z axis (sketch y = -Z on XZ, verified frame)
    let sk = ops::new_sketch(&mut doc, PlaneRef::Origin { plane: OriginPlane::XZ });
    ops::sketch_mut(&mut doc, &sk).unwrap().add_rectangle([10.0, 0.0], [20.0, -20.0]);
    let m = recompute(&mut k, &mut doc);
    let rev = ops::new_revolve(&mut doc, &sk, whole_profile(&m, &sk), RevolveAxis::Origin { axis: OriginAxis::Z }, "360 deg", Operation::New);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&rev), Health::Healthy);
    let ring = std::f64::consts::PI * (400.0 - 100.0) * 20.0;
    assert!(close(volume(&m), ring, 0.05), "{} vs {ring}", volume(&m));
    let bb = m.bodies[0].topology().bbox;
    assert!(close(bb[2], 0.0, 1e-6) && close(bb[5], 20.0, 1e-6), "ring spans z 0..20: {bb:?}");

    // chamfer one circular edge
    let b = &m.bodies[0];
    let circ = b.topology().edge_info.iter().position(|e| e.curve == CurveKind::Circle).unwrap();
    let ch = ops::new_chamfer(&mut doc, vec![refs::edge_ref(b, circ).unwrap()], "1 mm");
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&ch), Health::Healthy);
    assert!(volume(&m) < ring);

    // hole through the top face
    let b = &m.bodies[0];
    let top = b.topology().face_info.iter().position(|f| f.surface == SurfaceKind::Plane && f.normal[2] > 0.99).unwrap();
    let fr = refs::face_ref(b, top).unwrap();
    let v_before = volume(&m);
    let hole = ops::new_hole(&mut doc, fr, [15.0, 0.0], "4 mm", "5 mm", true);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&hole), Health::Healthy);
    assert!(volume(&m) < v_before - 100.0);

    // offset construction plane
    let pl = ops::new_plane(&mut doc, PlaneRef::Origin { plane: OriginPlane::XY }, "25 mm");
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.planes[&pl].origin, [0.0, 0.0, 25.0]);

    // STEP export -> import round trip, STL export
    let dir = std::env::temp_dir().join("cad360-kernel-test");
    std::fs::create_dir_all(&dir).unwrap();
    let step = dir.join("ring.step");
    let bodies: Vec<_> = m.bodies.iter().collect();
    cad_kernel::exchange::export_step(&bodies, &step.to_string_lossy()).unwrap();
    let text = cad_kernel::exchange::read_step_text(&step.to_string_lossy()).unwrap();
    assert!(text.starts_with("ISO-10303-21"));
    let mut d2 = Document::new("import");
    let imp = ops::new_import(&mut d2, "ring.step", text);
    let m2 = Kernel::new().recompute(&d2);
    assert_eq!(m2.health_of(&imp), Health::Healthy);
    assert!(close(volume(&m2), volume(&m), 0.5), "{} vs {}", volume(&m2), volume(&m));
    let stl = dir.join("ring.stl");
    cad_kernel::exchange::export_stl(&bodies, &stl.to_string_lossy(), true).unwrap();
    let stl_text = std::fs::read_to_string(&stl).unwrap();
    assert!(stl_text.starts_with("solid") && stl_text.contains("facet normal"));
    let mesh = m.bodies[0].mesh();
    assert!(!mesh.indices.is_empty() && mesh.face_ranges.len() == m.bodies[0].topology().faces.len());
}

#[test]
fn constraint_only_sketch_edits_are_not_served_from_cache() {
    let mut doc = Document::new("cache");
    let mut k = Kernel::new();
    let (sk, _, _) = rectangle_sketch(&mut doc, "50 mm", "30 mm");
    recompute(&mut k, &mut doc);
    let before = doc.feature(&sk).unwrap().sketch().unwrap().constraints.len();
    let victim = doc.feature(&sk).unwrap().sketch().unwrap().constraints.iter().find(|c| c.param.is_none()).unwrap().id.clone();
    ops::sketch_mut(&mut doc, &sk).unwrap().delete_constraint(&victim);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(doc.feature(&sk).unwrap().sketch().unwrap().constraints.len(), before - 1, "stale constraints must not be written back");
    assert!(!m.sketches[&sk].report.is_fully_constrained());
}

#[test]
fn face_sketch_projects_face_boundary_like_fusion() {
    // Fusion probe: a sketch on the 70x30 top face with one Ø12 circle had 2 profiles
    // (1965.4 mm² ring and 113.1 mm² disc) because the face boundary is projected.
    let mut doc = Document::new("proj");
    let mut k = Kernel::new();
    let (sk, pw, _) = rectangle_sketch(&mut doc, "70 mm", "30 mm");
    let m = recompute(&mut k, &mut doc);
    ops::new_extrude(&mut doc, &sk, whole_profile(&m, &sk), "20 mm", ExtentDirection::Normal, Operation::New);
    let m = recompute(&mut k, &mut doc);
    let b = &m.bodies[0];
    let top = b.topology().face_info.iter().position(|f| f.surface == SurfaceKind::Plane && f.normal[2] > 0.99).unwrap();
    let sk2 = ops::new_sketch(&mut doc, PlaneRef::Face { face: refs::face_ref(b, top).unwrap() });
    ops::sketch_mut(&mut doc, &sk2).unwrap().add_circle([35.0, 15.0], 6.0);
    let m = recompute(&mut k, &mut doc);
    let r = &m.sketches[&sk2];
    assert_eq!(r.profiles.len(), 2);
    assert!(close(r.profiles[0].area, 2100.0 - 113.097, 0.05), "{}", r.profiles[0].area);
    assert!(close(r.profiles[1].area, 113.097, 0.01));
    assert_eq!(r.projected.len(), 4);
    // associative: the projection follows an upstream width change
    doc.set_expression(&pw, "80 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert!(close(m.sketches[&sk2].profiles[0].area, 2400.0 - 113.097, 0.05));
}
