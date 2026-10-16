//! Components and joints: component ownership of features and bodies, Rigid / Revolute /
//! Slider joints solved as an assembly graph (Fusion probe F35-F37: aligned joint axes, the
//! earlier-created / grounded / root component stays, over-constraint refused), placement of
//! features created after a joint, reference errors, persistence, and loading documents
//! written before components existed.

use cad_core::ops;
use cad_core::*;
use cad_kernel::{refs, Health, Kernel, Model};

fn recompute(k: &mut Kernel, doc: &mut Document) -> Model {
    let m = k.recompute(doc);
    cad_kernel::store_solved_sketches(doc, &m);
    m
}

/// Axis-aligned box [x0,x1] x [y0,y1] x [0,d] as a new body in the active component.
fn free_box(k: &mut Kernel, doc: &mut Document, a: [f64; 2], b: [f64; 2], d: &str) -> FeatureId {
    let sk = ops::new_sketch(doc, PlaneRef::Origin { plane: OriginPlane::XY });
    ops::sketch_mut(doc, &sk).unwrap().add_rectangle(a, b);
    let m = recompute(k, doc);
    let p = &m.sketches[&sk].profiles[0];
    let prof = vec![ProfileRef { curves: p.curves.clone(), sample: p.interior_point() }];
    let ext = ops::new_extrude(doc, &sk, prof, d, ExtentDirection::Normal, Operation::New);
    let m = recompute(k, doc);
    assert_eq!(m.health_of(&ext), Health::Healthy, "{:?}", m.health_of(&ext));
    ext
}

/// Cylinder of radius `r` centred at `c`, height `d`.
fn cylinder(k: &mut Kernel, doc: &mut Document, c: [f64; 2], r: f64, d: &str) -> FeatureId {
    let sk = ops::new_sketch(doc, PlaneRef::Origin { plane: OriginPlane::XY });
    ops::sketch_mut(doc, &sk).unwrap().add_circle(c, r);
    let m = recompute(k, doc);
    let p = &m.sketches[&sk].profiles[0];
    let prof = vec![ProfileRef { curves: p.curves.clone(), sample: p.interior_point() }];
    let ext = ops::new_extrude(doc, &sk, prof, d, ExtentDirection::Normal, Operation::New);
    assert_eq!(recompute(k, doc).health_of(&ext), Health::Healthy);
    ext
}

/// Plate [x0,x1] x [y0,y1] x [0,d] with a through hole of radius `r` at `c`.
fn plate_with_hole(k: &mut Kernel, doc: &mut Document, a: [f64; 2], b: [f64; 2], c: [f64; 2], r: f64, d: &str) -> FeatureId {
    let sk = ops::new_sketch(doc, PlaneRef::Origin { plane: OriginPlane::XY });
    let s = ops::sketch_mut(doc, &sk).unwrap();
    s.add_rectangle(a, b);
    s.add_circle(c, r);
    let m = recompute(k, doc);
    let p = m.sketches[&sk].profiles.iter().find(|p| p.holes.len() == 1).expect("the plate region");
    let prof = vec![ProfileRef { curves: p.curves.clone(), sample: p.interior_point() }];
    let ext = ops::new_extrude(doc, &sk, prof, d, ExtentDirection::Normal, Operation::New);
    assert_eq!(recompute(k, doc).health_of(&ext), Health::Healthy);
    ext
}

fn bbox(m: &Model, id: &str) -> [f64; 6] {
    m.body(id).unwrap_or_else(|| panic!("body {id} missing")).topology().bbox
}

fn assert_bbox(m: &Model, id: &str, expect: [f64; 6]) {
    let b = bbox(m, id);
    for (x, y) in b.iter().zip(expect) {
        assert!((x - y).abs() <= 1e-6, "{id}: bbox {b:?}, expected {expect:?}");
    }
}

fn face_origin(m: &Model, doc: &Document, body: &str, n: [f64; 3]) -> JointOrigin {
    let b = m.body(body).unwrap();
    let t = b.topology();
    let i = t.face_info.iter().position(|f| f.surface == SurfaceKind::Plane && cad_kernel::frame::dot(f.normal, n) > 0.999).unwrap();
    JointOrigin { geometry: JointGeometry::Face { face: refs::face_ref(b, i).unwrap() }, component: m.component_of(doc, body).map(String::from) }
}

/// Circular edge of `body` whose centre is at height `z`.
fn circle_origin(m: &Model, doc: &Document, body: &str, z: f64) -> JointOrigin {
    let b = m.body(body).unwrap();
    let t = b.topology();
    let i = t.edge_info.iter().position(|e| e.curve == CurveKind::Circle && (e.center[2] - z).abs() < 1e-6).expect("circular edge");
    JointOrigin { geometry: JointGeometry::Edge { edge: refs::edge_ref(b, i).unwrap() }, component: m.component_of(doc, body).map(String::from) }
}

/// Joint from the top face (+Z) of `a` to the bottom face (-Z) of `b`.
#[allow(clippy::too_many_arguments)]
fn face_joint(doc: &mut Document, m: &Model, a: &str, b: &str, kind: JointKind, angle: &str, offset: &str, flip: bool) -> FeatureId {
    let oa = face_origin(m, doc, a, [0.0, 0.0, 1.0]);
    let ob = face_origin(m, doc, b, [0.0, 0.0, -1.0]);
    ops::new_joint(doc, oa, ob, kind, angle, offset, flip)
}

fn param(doc: &Document, joint: &str, n: usize) -> String {
    doc.feature(joint).unwrap().parameter_names()[n].to_string()
}

/// A: 50 x 30 x 20 box in Component1. B: 20 x 10 x 10 box in Component2, away from A.
fn two_components(k: &mut Kernel, doc: &mut Document) -> (ComponentId, ComponentId, FeatureId, FeatureId) {
    let c1 = ops::new_component(doc, &[]);
    let a = free_box(k, doc, [0.0, 0.0], [50.0, 30.0], "20 mm");
    let c2 = ops::new_component(doc, &[]);
    let b = free_box(k, doc, [100.0, 50.0], [120.0, 60.0], "10 mm");
    (c1, c2, a, b)
}

#[test]
fn features_and_bodies_belong_to_the_active_component() {
    let mut doc = Document::new("comp");
    let mut k = Kernel::new();
    let root_box = free_box(&mut k, &mut doc, [-40.0, 0.0], [-20.0, 10.0], "5 mm");
    let (c1, c2, a, b) = two_components(&mut k, &mut doc);
    assert_eq!(doc.components.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Component1", "Component2"]);
    assert_eq!(doc.active_component.as_deref(), Some(c2.as_str()));
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.component_of(&doc, &root_box), None);
    assert_eq!(m.component_of(&doc, &a), Some(c1.as_str()));
    assert_eq!(m.component_of(&doc, &b), Some(c2.as_str()));
    assert_eq!(doc.feature(&a).unwrap().component.as_deref(), Some(c1.as_str()), "features record the active component");
    assert_eq!(m.bodies_of_component(&doc, Some(&c1)).len(), 1);
    assert_eq!(m.bodies_of_component(&doc, None).len(), 1);
    // deleting a component that still owns features is refused
    assert!(matches!(doc.delete_component(&c1), Err(DocError::ComponentInUse(..))));
    // New Component from selected bodies moves them (via their creating feature)
    let c3 = ops::new_component(&mut doc, std::slice::from_ref(&root_box));
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.component_of(&doc, &root_box), Some(c3.as_str()));
    // an empty component can be deleted; the active component falls back to root
    let c4 = ops::new_component(&mut doc, &[]);
    doc.delete_component(&c4).unwrap();
    assert_eq!(doc.active_component, None);
}

#[test]
fn rigid_joint_aligns_the_joint_axes() {
    let mut doc = Document::new("rigid");
    let mut k = Kernel::new();
    let (_, _, a, b) = two_components(&mut k, &mut doc);
    let m = recompute(&mut k, &mut doc);
    let oa = face_origin(&m, &doc, &a, [0.0, 0.0, 1.0]);
    let ob = face_origin(&m, &doc, &b, [0.0, 0.0, -1.0]);
    let j = ops::new_joint(&mut doc, oa, ob, JointKind::Rigid, "0 deg", "0 mm", false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j), Health::Healthy, "{:?}", m.health_of(&j));
    assert_eq!(doc.feature(&j).unwrap().name, "Joint1");
    // F35: the joint z axes are aligned (same direction): B's bottom-face centre lands on A's
    // top-face centre (25, 15, 20) with B's -Z turned to +Z, so B occupies the volume just below
    // A's top face. A does not move.
    assert_bbox(&m, &a, [0.0, 0.0, 0.0, 50.0, 30.0, 20.0]);
    assert_bbox(&m, &b, [15.0, 10.0, 10.0, 35.0, 20.0, 20.0]);
    assert_eq!(m.body(&b).unwrap().created_by, b, "moved bodies keep their identity");
    assert!((m.body(&b).unwrap().volume() - 2000.0).abs() < 1e-6);

    // flip reverses B's joint z: B sits on top of A
    if let FeatureKind::Joint { flip, .. } = &mut doc.feature_mut(&j).unwrap().kind {
        *flip = true;
    }
    let m = recompute(&mut k, &mut doc);
    assert_bbox(&m, &b, [15.0, 10.0, 20.0, 35.0, 20.0, 30.0]);
}

#[test]
fn slider_offset_translates_along_the_joint_axis() {
    let mut doc = Document::new("slider");
    let mut k = Kernel::new();
    let (_, _, a, b) = two_components(&mut k, &mut doc);
    let m = recompute(&mut k, &mut doc);
    let j = face_joint(&mut doc, &m, &a, &b, JointKind::Slider, "0 deg", "15 mm", false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j), Health::Healthy);
    assert_bbox(&m, &b, [15.0, 10.0, 25.0, 35.0, 20.0, 35.0]);
    assert_eq!(ops::primary_parameter(doc.feature(&j).unwrap()), Some(param(&doc, &j, 1).as_str()), "a slider's primary value is the offset");
    // the offset is a model parameter: Change Parameters drives the motion
    doc.set_expression(&param(&doc, &j, 1), "-5 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_bbox(&m, &b, [15.0, 10.0, 5.0, 35.0, 20.0, 15.0]);
}

#[test]
fn revolute_joint_on_circular_edges_rotates_b() {
    let mut doc = Document::new("revolute");
    let mut k = Kernel::new();
    // A: cylinder r 5 at (0,0), height 20. B: 40 x 16 x 4 plate with an r 4 hole at its centre.
    let _c1 = ops::new_component(&mut doc, &[]);
    let a = cylinder(&mut k, &mut doc, [0.0, 0.0], 5.0, "20 mm");
    let _c2 = ops::new_component(&mut doc, &[]);
    let b = plate_with_hole(&mut k, &mut doc, [60.0, 0.0], [100.0, 16.0], [80.0, 8.0], 4.0, "4 mm");
    let m = recompute(&mut k, &mut doc);
    let oa = circle_origin(&m, &doc, &a, 20.0);
    let ob = circle_origin(&m, &doc, &b, 0.0);
    let j = ops::new_joint(&mut doc, oa, ob, JointKind::Revolute, "0 deg", "0 mm", true);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j), Health::Healthy, "{:?}", m.health_of(&j));
    // flipped: the hole's bottom circle sits on the cylinder's top circle, centred on (0,0), z 20..24
    assert_bbox(&m, &b, [-20.0, -8.0, 20.0, 20.0, 8.0, 24.0]);
    // 90 deg about the joint axis swaps the plate's X and Y extents
    let pa = param(&doc, &j, 0);
    assert_eq!(ops::primary_parameter(doc.feature(&j).unwrap()), Some(pa.as_str()));
    doc.set_expression(&pa, "90 deg").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.recomputed_from, doc.feature_index(&j), "editing the angle recomputes from the joint");
    assert_bbox(&m, &b, [-8.0, -20.0, 20.0, 8.0, 20.0, 24.0]);
    // a vertex check: the plate corner (60,0,0) (relative (-20,-8)) turns by -90 deg about +Z
    // (origin 1 turns +90 deg relative to origin 2, as in the probe) to (-8,20,20)
    let t = m.body(&b).unwrap().topology();
    let has = |p: [f64; 3]| t.edge_info.iter().any(|e| cad_kernel::frame::dist(e.start, p) < 1e-6 || cad_kernel::frame::dist(e.end, p) < 1e-6);
    assert!(has([-8.0, 20.0, 20.0]), "rotated corner");
    assert!(!has([-20.0, -8.0, 20.0]));
    // 45 deg
    doc.set_expression(&pa, "45 deg").unwrap();
    let m = recompute(&mut k, &mut doc);
    let bb = bbox(&m, &b);
    let half = (20.0 + 8.0) / 2f64.sqrt();
    assert!((bb[3] - half).abs() < 1e-6 && (bb[4] - half).abs() < 1e-6, "{bb:?}");
    // unflipped (F35): the joint z axes align, the plate goes into the cylinder (z 16..20)
    doc.set_expression(&pa, "0 deg").unwrap();
    if let FeatureKind::Joint { flip, .. } = &mut doc.feature_mut(&j).unwrap().kind {
        *flip = false;
    }
    let m = recompute(&mut k, &mut doc);
    assert_bbox(&m, &b, [-20.0, -8.0, 16.0, 20.0, 8.0, 20.0]);
}

#[test]
fn joint_errors_same_component_grounding_and_lost_references() {
    let mut doc = Document::new("errors");
    let mut k = Kernel::new();
    let (c1, c2, a, b) = two_components(&mut k, &mut doc);
    // a second body in Component1
    doc.active_component = Some(c1.clone());
    let a2 = free_box(&mut k, &mut doc, [0.0, 50.0], [10.0, 60.0], "5 mm");
    let m = recompute(&mut k, &mut doc);
    let same = face_joint(&mut doc, &m, &a, &a2, JointKind::Rigid, "0 deg", "0 mm", false);
    let m = recompute(&mut k, &mut doc);
    assert!(matches!(m.health_of(&same), Health::Error(ref e) if e.contains("same component")), "{:?}", m.health_of(&same));
    doc.delete_feature(&same).unwrap();

    // F36: with nothing grounded the earlier-created component stays, whatever the pick order:
    // origin 1 on B (Component2), origin 2 on A (Component1) -> B moves
    let m = recompute(&mut k, &mut doc);
    let j = face_joint(&mut doc, &m, &b, &a, JointKind::Rigid, "0 deg", "0 mm", false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j), Health::Healthy);
    assert_bbox(&m, &a, [0.0, 0.0, 0.0, 50.0, 30.0, 20.0]);
    assert_bbox(&m, &a2, [0.0, 50.0, 0.0, 10.0, 60.0, 5.0]);
    // B's top face (z 10) lands on A's bottom face (z 0) with the z axes aligned: inside A
    assert_bbox(&m, &b, [15.0, 10.0, 0.0, 35.0, 20.0, 10.0]);

    // grounding Component2: now Component1 moves, every body of it (A and A2) by the same
    // transform (half turn about X, A's bottom centre (25,15,0) -> B's top centre (110,55,10))
    doc.component_mut(&c2).unwrap().grounded = true;
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j), Health::Healthy);
    assert_bbox(&m, &b, [100.0, 50.0, 0.0, 120.0, 60.0, 10.0]);
    assert_bbox(&m, &a, [85.0, 40.0, -10.0, 135.0, 70.0, 10.0]);
    assert_bbox(&m, &a2, [85.0, 10.0, 5.0, 95.0, 20.0, 10.0]);

    // deleting a referenced body's feature: the joint reports a lost reference, the rest survives
    doc.delete_feature(&b).unwrap();
    let m = recompute(&mut k, &mut doc);
    assert!(matches!(m.health_of(&j), Health::Error(ref e) if e.contains("reference is lost")), "{:?}", m.health_of(&j));
    assert_bbox(&m, &a, [0.0, 0.0, 0.0, 50.0, 30.0, 20.0]);
}

#[test]
fn root_component_never_moves_in_either_pick_order() {
    let mut doc = Document::new("root");
    let mut k = Kernel::new();
    let r = free_box(&mut k, &mut doc, [0.0, 0.0], [10.0, 10.0], "10 mm");
    ops::new_component(&mut doc, &[]);
    let c = free_box(&mut k, &mut doc, [50.0, 0.0], [60.0, 10.0], "10 mm");
    let m = recompute(&mut k, &mut doc);
    // the root geometry as origin 2: the component (origin 1) moves
    let j = face_joint(&mut doc, &m, &c, &r, JointKind::Rigid, "0 deg", "0 mm", true);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j), Health::Healthy, "{:?}", m.health_of(&j));
    assert_bbox(&m, &r, [0.0, 0.0, 0.0, 10.0, 10.0, 10.0]);
    // flipped: c's top face on r's bottom face, c below r
    assert_bbox(&m, &c, [0.0, 0.0, -10.0, 10.0, 10.0, 0.0]);
    doc.delete_feature(&j).unwrap();
    let m = recompute(&mut k, &mut doc);
    let ok = face_joint(&mut doc, &m, &r, &c, JointKind::Rigid, "0 deg", "0 mm", true);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&ok), Health::Healthy);
    assert_bbox(&m, &r, [0.0, 0.0, 0.0, 10.0, 10.0, 10.0]);
    assert_bbox(&m, &c, [0.0, 0.0, 10.0, 10.0, 10.0, 20.0]);
}

/// The Fusion probe (F35/F36, `out-v4/probe-log.jsonl`): A = 40 x 40 x 20 at the origin in
/// Component1, B = 30 x 15 x 10 at (100,0,0) in Component2.
fn probe_pair(k: &mut Kernel, doc: &mut Document) -> (ComponentId, ComponentId, FeatureId, FeatureId) {
    let c1 = ops::new_component(doc, &[]);
    let a = free_box(k, doc, [0.0, 0.0], [40.0, 40.0], "20 mm");
    let c2 = ops::new_component(doc, &[]);
    let b = free_box(k, doc, [100.0, 0.0], [130.0, 15.0], "10 mm");
    (c1, c2, a, b)
}

fn assert_bbox_near(m: &Model, id: &str, expect: [f64; 6], tol: f64) {
    let b = bbox(m, id);
    for (x, y) in b.iter().zip(expect) {
        assert!((x - y).abs() <= tol, "{id}: bbox {b:?}, expected {expect:?}");
    }
}

fn has_vertex(m: &Model, id: &str, p: [f64; 3], tol: f64) -> bool {
    let t = m.body(id).unwrap().topology();
    t.edge_info.iter().any(|e| cad_kernel::frame::dist(e.start, p) < tol || cad_kernel::frame::dist(e.end, p) < tol)
}

#[test]
fn fusion_probe_revolute_and_rigid_numbers() {
    let mut doc = Document::new("probe");
    let mut k = Kernel::new();
    let (_, _, a, b) = probe_pair(&mut k, &mut doc);
    let m0 = recompute(&mut k, &mut doc);
    let before = m0.body(&b).unwrap().clone();
    // revolute 30 deg on A top centre / B bottom centre
    let j = face_joint(&mut doc, &m0, &a, &b, JointKind::Revolute, "30 deg", "0 mm", false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j), Health::Healthy, "{:?}", m.health_of(&j));
    assert_bbox(&m, &a, [0.0, 0.0, 0.0, 40.0, 40.0, 20.0]);
    // probe: B world bbox [[3.26, 6.005, 10], [36.74, 33.995, 20]]
    assert_bbox_near(&m, &b, [3.26, 6.005, 10.0, 36.74, 33.995, 20.0], 1e-3);
    // the bbox is blind to the rotation sense; the probe's component origin (B's corner
    // (100,0,0) here) went to translation_mm (10.76, 33.995, 20), rotation row (0.866, -0.5, 0)
    assert!(has_vertex(&m, &b, [10.76, 33.995, 20.0], 1e-3), "probe translation of B's origin corner");

    // every face moved with the transform and kept its index (face names stay aligned)
    let ta = m0.body(&a).unwrap().topology();
    let fa = cad_kernel::face_joint_frame(m0.body(&a).unwrap(), ta.face_info.iter().position(|f| f.normal[2] > 0.999).unwrap()).unwrap();
    let fb = cad_kernel::face_joint_frame(&before, before.topology().face_info.iter().position(|f| f.normal[2] < -0.999).unwrap()).unwrap();
    let r = cad_kernel::mate_transform(&fa, &fb, 30.0, 0.0, false);
    let moved = m.body(&b).unwrap();
    let (t0, t1) = (before.topology(), moved.topology());
    assert_eq!(t0.face_info.len(), t1.face_info.len());
    assert!(!before.face_names.is_empty(), "extrude bodies carry generative face names");
    assert_eq!(before.face_names, moved.face_names, "face names are carried over");
    for (f0, f1) in t0.face_info.iter().zip(t1.face_info.iter()) {
        let c = f0.centroid;
        let p = [0, 1, 2].map(|i| r[i * 4] * c[0] + r[i * 4 + 1] * c[1] + r[i * 4 + 2] * c[2] + r[i * 4 + 3]);
        assert!(cad_kernel::frame::dist(p, f1.centroid) < 1e-9, "face order kept: {p:?} vs {:?}", f1.centroid);
    }

    // rigid: probe B world bbox [[5, 12.5, 10], [35, 27.5, 20]] (B's corner (100,0,0) -> (5,27.5,20))
    if let FeatureKind::Joint { kind, .. } = &mut doc.feature_mut(&j).unwrap().kind {
        *kind = JointKind::Rigid;
    }
    doc.set_expression(&param(&doc, &j, 0), "0 deg").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_bbox(&m, &b, [5.0, 12.5, 10.0, 35.0, 27.5, 20.0]);
    assert!(has_vertex(&m, &b, [5.0, 27.5, 20.0], 1e-9));

    // F36: the other pick order (origin 1 on B, origin 2 on A) moves B the same way
    doc.delete_feature(&j).unwrap();
    let m = recompute(&mut k, &mut doc);
    let oa = face_origin(&m, &doc, &a, [0.0, 0.0, 1.0]);
    let ob = face_origin(&m, &doc, &b, [0.0, 0.0, -1.0]);
    let j2 = ops::new_joint(&mut doc, ob, oa, JointKind::Rigid, "0 deg", "0 mm", false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j2), Health::Healthy);
    assert_bbox(&m, &a, [0.0, 0.0, 0.0, 40.0, 40.0, 20.0]);
    assert_bbox(&m, &b, [5.0, 12.5, 10.0, 35.0, 27.5, 20.0]);
}

#[test]
fn second_joint_between_the_same_pair_is_over_constrained() {
    let mut doc = Document::new("over");
    let mut k = Kernel::new();
    let (_, _, a, b) = probe_pair(&mut k, &mut doc);
    let m = recompute(&mut k, &mut doc);
    let j1 = face_joint(&mut doc, &m, &a, &b, JointKind::Rigid, "0 deg", "0 mm", false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j1), Health::Healthy);
    // F37: a second joint between the same two components is refused, the first stays healthy
    let oa = face_origin(&m, &doc, &a, [1.0, 0.0, 0.0]);
    let ob = face_origin(&m, &doc, &b, [1.0, 0.0, 0.0]);
    let j2 = ops::new_joint(&mut doc, oa, ob, JointKind::Rigid, "0 deg", "0 mm", false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j2), Health::Error(cad_kernel::OVER_CONSTRAINED.into()));
    assert_eq!(m.health_of(&j1), Health::Healthy);
    assert_bbox(&m, &b, [5.0, 12.5, 10.0, 35.0, 27.5, 20.0]);
    assert!(cad_kernel::joint_moves_b(&doc, Some(&j2), Some("component1"), Some("component2")).is_err(), "the dialog check agrees");
    // suppressing the first joint lets the second one solve
    doc.feature_mut(&j1).unwrap().suppressed = true;
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j2), Health::Healthy, "{:?}", m.health_of(&j2));

    // a cycle through a third component is over-constrained too
    doc.feature_mut(&j1).unwrap().suppressed = false;
    doc.delete_feature(&j2).unwrap();
    let c3 = ops::new_component(&mut doc, &[]);
    let c = free_box(&mut k, &mut doc, [-60.0, 0.0], [-50.0, 10.0], "10 mm");
    let m = recompute(&mut k, &mut doc);
    let j3 = face_joint(&mut doc, &m, &a, &c, JointKind::Rigid, "0 deg", "0 mm", true);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j3), Health::Healthy);
    let j4 = face_joint(&mut doc, &m, &b, &c, JointKind::Rigid, "0 deg", "0 mm", true);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j4), Health::Error(cad_kernel::OVER_CONSTRAINED.into()));
    assert_eq!(m.health_of(&j3), Health::Healthy);

    // the root and a grounded component cannot be jointed to each other (both fixed)
    doc.delete_feature(&j4).unwrap();
    doc.delete_feature(&j3).unwrap();
    doc.component_mut(&c3).unwrap().grounded = true;
    doc.active_component = None;
    let r = free_box(&mut k, &mut doc, [0.0, -60.0], [10.0, -50.0], "10 mm");
    let m = recompute(&mut k, &mut doc);
    let j5 = face_joint(&mut doc, &m, &r, &c, JointKind::Rigid, "0 deg", "0 mm", true);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j5), Health::Error(cad_kernel::OVER_CONSTRAINED.into()));
    assert_bbox(&m, &c, [-60.0, 0.0, 0.0, -50.0, 10.0, 10.0]);
}

#[test]
fn chained_joints_move_whole_subassemblies() {
    let mut doc = Document::new("chain");
    let mut k = Kernel::new();
    let (c1, _, a, b) = probe_pair(&mut k, &mut doc);
    ops::new_component(&mut doc, &[]);
    let c = free_box(&mut k, &mut doc, [200.0, 0.0], [210.0, 10.0], "10 mm");
    let m = recompute(&mut k, &mut doc);
    // B (Component2) on A (Component1), then C (Component3) on B, both flipped (stacked)
    let j1 = face_joint(&mut doc, &m, &a, &b, JointKind::Rigid, "0 deg", "0 mm", true);
    let m = recompute(&mut k, &mut doc);
    let j2 = face_joint(&mut doc, &m, &b, &c, JointKind::Rigid, "0 deg", "0 mm", true);
    let m = recompute(&mut k, &mut doc);
    assert_eq!((m.health_of(&j1), m.health_of(&j2)), (Health::Healthy, Health::Healthy));
    assert_bbox(&m, &b, [5.0, 12.5, 20.0, 35.0, 27.5, 30.0]);
    assert_bbox(&m, &c, [15.0, 15.0, 30.0, 25.0, 25.0, 40.0]);
    // editing the first joint moves the whole chain
    doc.set_expression(&param(&doc, &j1, 1), "5 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_bbox(&m, &b, [5.0, 12.5, 25.0, 35.0, 27.5, 35.0]);
    assert_bbox(&m, &c, [15.0, 15.0, 35.0, 25.0, 25.0, 45.0]);
    // grounding A's component keeps everything in place (it already is the anchor)
    doc.component_mut(&c1).unwrap().grounded = true;
    let m = recompute(&mut k, &mut doc);
    assert_bbox(&m, &a, [0.0, 0.0, 0.0, 40.0, 40.0, 20.0]);
    assert_bbox(&m, &c, [15.0, 15.0, 35.0, 25.0, 25.0, 45.0]);
}

#[test]
fn features_after_a_joint_follow_the_component_placement() {
    let mut doc = Document::new("after");
    let mut k = Kernel::new();
    let (_, c2, a, b) = probe_pair(&mut k, &mut doc);
    let m = recompute(&mut k, &mut doc);
    let j = face_joint(&mut doc, &m, &a, &b, JointKind::Rigid, "0 deg", "0 mm", true);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j), Health::Healthy);
    // flipped rigid: pure translation by (-95, 12.5, 20)
    assert_bbox(&m, &b, [5.0, 12.5, 20.0, 35.0, 27.5, 30.0]);

    // a body created later in Component2 on its origin plane appears in the moved frame
    doc.active_component = Some(c2.clone());
    let b2 = free_box(&mut k, &mut doc, [100.0, 20.0], [110.0, 30.0], "5 mm");
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.component_of(&doc, &b2), Some(c2.as_str()));
    assert_bbox(&m, &b2, [5.0, 32.5, 20.0, 15.0, 42.5, 25.0]);
    // ... while one created in the root stays in world coordinates
    doc.active_component = None;
    let r = free_box(&mut k, &mut doc, [100.0, 20.0], [110.0, 30.0], "5 mm");
    let m = recompute(&mut k, &mut doc);
    assert_bbox(&m, &r, [100.0, 20.0, 0.0, 110.0, 30.0, 5.0]);

    // downstream features reference the placed geometry: a hole on B's (moved) top face
    let top = face_origin(&m, &doc, &b, [0.0, 0.0, 1.0]);
    let JointGeometry::Face { face } = top.geometry else { unreachable!() };
    doc.active_component = Some(c2.clone());
    let h = ops::new_hole(&mut doc, face, [20.0, 20.0], "4 mm", "5 mm", false);
    let m = recompute(&mut k, &mut doc);
    let cut = 4500.0 - std::f64::consts::PI * 4.0 * 5.0;
    assert_eq!(m.health_of(&h), Health::Healthy, "{:?}", m.health_of(&h));
    assert!((m.body(&b).unwrap().volume() - cut).abs() < 1e-3);

    // editing the joint moves bodies created before and after it, and the hole goes along
    doc.set_expression(&param(&doc, &j, 1), "10 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&h), Health::Healthy, "{:?}", m.health_of(&h));
    assert_bbox(&m, &b, [5.0, 12.5, 30.0, 35.0, 27.5, 40.0]);
    assert_bbox(&m, &b2, [5.0, 32.5, 30.0, 15.0, 42.5, 35.0]);
    assert!((m.body(&b).unwrap().volume() - cut).abs() < 1e-3);

    // rolling the marker back before the joint returns Component2 to where it was modelled
    doc.marker = doc.feature_index(&j).unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_bbox(&m, &b, [100.0, 0.0, 0.0, 130.0, 15.0, 10.0]);
}

#[test]
fn persistence_round_trip_with_components_and_joints() {
    let mut doc = Document::new("persist");
    let mut k = Kernel::new();
    let (c1, _, a, b) = two_components(&mut k, &mut doc);
    if let Some(c) = doc.component_mut(&c1) {
        c.name = "Base".into();
        c.visible = false;
    }
    let m = recompute(&mut k, &mut doc);
    let j = face_joint(&mut doc, &m, &a, &b, JointKind::Revolute, "30 deg", "2 mm", true);
    let m = recompute(&mut k, &mut doc);
    let text = doc.to_json();
    assert!(text.contains("\"components\"") && text.contains("\"component\": \"component1\""), "components are saved");
    let back = Document::from_json(&text).unwrap();
    // (JSON float parsing may differ from the written value in the last bit, so compare the
    // assembly structure rather than the whole document)
    assert_eq!(back.components, doc.components);
    assert_eq!(back.active_component, doc.active_component);
    assert_eq!(back.features.iter().map(|f| f.component.clone()).collect::<Vec<_>>(), doc.features.iter().map(|f| f.component.clone()).collect::<Vec<_>>());
    assert_eq!(back.to_json(), Document::from_json(&back.to_json()).unwrap().to_json());
    match (&back.feature(&j).unwrap().kind, &doc.feature(&j).unwrap().kind) {
        (FeatureKind::Joint { a, b, kind, angle, offset, flip }, FeatureKind::Joint { a: a0, b: b0, .. }) => {
            assert_eq!((*kind, *flip, angle.as_str(), offset.as_str()), (JointKind::Revolute, true, "d3", "d4"));
            assert_eq!((a.component.as_deref(), b.component.as_deref()), (a0.component.as_deref(), b0.component.as_deref()));
            assert!(matches!(a.geometry, JointGeometry::Face { .. }));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(back.component(&c1).unwrap().name, "Base");
    assert!(!back.component(&c1).unwrap().visible);
    let mut k2 = Kernel::new();
    let m2 = k2.recompute(&back);
    assert_eq!(m2.health_of(&j), Health::Healthy);
    for (x, y) in bbox(&m, &b).iter().zip(bbox(&m2, &b)) {
        assert!((x - y).abs() < 1e-9);
    }
    // STEP export carries the moved (world-space) bodies
    let path = std::env::temp_dir().join(format!("cad360-assembly-{}.step", std::process::id()));
    let bodies: Vec<&cad_kernel::Body> = m2.bodies.iter().collect();
    cad_kernel::exchange::export_step(&bodies, &path.to_string_lossy()).unwrap();
    let shape = cad_kernel::occ::Shape::read_step(&path.to_string_lossy()).unwrap();
    let _ = std::fs::remove_file(&path);
    let bb = shape.bbox().unwrap();
    let want = bbox(&m2, &b);
    assert!(bb[5] >= want[5] - 1e-3, "exported bbox {bb:?} contains the moved body {want:?}");
}

#[test]
fn documents_without_components_still_load() {
    // a document as written before components existed (no "components", no "component")
    let text = r#"{
      "format": "cad360", "version": 1, "name": "old", "units": "mm",
      "parameters": [], "marker": 1, "counters": {"Sketch": 1, "__uid": 1},
      "features": [{"id": "sketch1", "name": "Sketch1", "type": "sketch",
                    "plane": {"kind": "origin", "plane": "XY"},
                    "sketch": {"entities": [], "constraints": []}}]
    }"#;
    let doc = Document::from_json(text).unwrap_or_else(|e| panic!("old document did not load: {e}"));
    // root-only documents do not write the new fields either
    let mut d = Document::new("new");
    ops::new_sketch(&mut d, PlaneRef::Origin { plane: OriginPlane::XY });
    assert!(!d.to_json().contains("component"));
    assert!(doc.components.is_empty());
    assert_eq!(doc.active_component, None);
    assert_eq!(doc.features[0].component, None);
    // an active component that does not exist is dropped on load
    let mut d = Document::new("x");
    d.active_component = Some("ghost".into());
    assert_eq!(Document::from_json(&d.to_json()).unwrap().active_component, None);
}

#[test]
fn merging_clusters_keeps_the_earliest_component_and_fixed_components_transitively() {
    // C2-C3 first (C2 stays), then C1-C3: the merged cluster's earliest component C1 stays and
    // the whole C2+C3 sub-assembly moves
    let mut doc = Document::new("merge");
    let mut k = Kernel::new();
    let (_, _, a, b) = probe_pair(&mut k, &mut doc);
    ops::new_component(&mut doc, &[]);
    let c = free_box(&mut k, &mut doc, [200.0, 0.0], [210.0, 10.0], "10 mm");
    let m = recompute(&mut k, &mut doc);
    let j1 = face_joint(&mut doc, &m, &b, &c, JointKind::Rigid, "0 deg", "0 mm", true);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j1), Health::Healthy);
    assert_bbox(&m, &b, [100.0, 0.0, 0.0, 130.0, 15.0, 10.0]);
    assert_bbox(&m, &c, [110.0, 2.5, 10.0, 120.0, 12.5, 20.0]);
    let j2 = face_joint(&mut doc, &m, &a, &c, JointKind::Rigid, "0 deg", "0 mm", true);
    assert_eq!(cad_kernel::joint_moves_b(&doc, Some(&j2), Some("component1"), Some("component3")), Ok(true), "the dialog rule agrees: C3's side moves");
    let m = recompute(&mut k, &mut doc);
    assert_eq!((m.health_of(&j1), m.health_of(&j2)), (Health::Healthy, Health::Healthy));
    assert_bbox(&m, &a, [0.0, 0.0, 0.0, 40.0, 40.0, 20.0]);
    assert_bbox(&m, &c, [15.0, 15.0, 20.0, 25.0, 25.0, 30.0]);
    assert_bbox(&m, &b, [5.0, 12.5, 10.0, 35.0, 27.5, 20.0]);

    // root-C1, then grounded C2 to C1: C1's cluster already contains the (fixed) root
    let mut doc = Document::new("fixed");
    let mut k = Kernel::new();
    let r = free_box(&mut k, &mut doc, [0.0, 0.0], [10.0, 10.0], "10 mm");
    ops::new_component(&mut doc, &[]);
    let c1 = free_box(&mut k, &mut doc, [50.0, 0.0], [60.0, 10.0], "10 mm");
    let g = ops::new_component(&mut doc, &[]);
    let c2 = free_box(&mut k, &mut doc, [100.0, 0.0], [110.0, 10.0], "10 mm");
    doc.component_mut(&g).unwrap().grounded = true;
    let m = recompute(&mut k, &mut doc);
    let j1 = face_joint(&mut doc, &m, &r, &c1, JointKind::Rigid, "0 deg", "0 mm", true);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j1), Health::Healthy);
    assert_bbox(&m, &c1, [0.0, 0.0, 10.0, 10.0, 10.0, 20.0]);
    let j2 = face_joint(&mut doc, &m, &c2, &c1, JointKind::Rigid, "0 deg", "0 mm", true);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&j2), Health::Error(cad_kernel::OVER_CONSTRAINED.into()));
    assert_eq!(m.health_of(&j1), Health::Healthy);
    assert_bbox(&m, &c2, [100.0, 0.0, 0.0, 110.0, 10.0, 10.0]);
    assert_bbox(&m, &c1, [0.0, 0.0, 10.0, 10.0, 10.0, 20.0]);
}
