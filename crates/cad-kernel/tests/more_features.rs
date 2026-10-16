//! Shell, Combine, Mirror (bodies), Rectangular / Circular Pattern (bodies) and Move/Copy as
//! parametric timeline features: exact volumes, body identity, positions, dependencies,
//! persistence and error handling.

use cad_core::ops;
use cad_core::sketch::{ConstraintKind as K, DistanceOrientation as O, EntityKind, ORIGIN};
use cad_core::*;
use cad_kernel::{refs, Health, Kernel, Model};

/// Rectangle with its corner on the origin and width/height dimensions.
fn rectangle_sketch(doc: &mut Document, w: &str, h: &str) -> (FeatureId, String) {
    let sk = ops::new_sketch(doc, PlaneRef::Origin { plane: OriginPlane::XY });
    let s = ops::sketch_mut(doc, &sk).unwrap();
    let lines = s.add_rectangle([1.0, 1.0], [40.0, 20.0]);
    let corner = match &s.entity(&lines[0]).unwrap().kind {
        EntityKind::Line { p1, .. } => p1.clone(),
        _ => unreachable!(),
    };
    s.add_constraint(K::Coincident { a: corner, b: ORIGIN.into() });
    let (_, pw) = ops::add_dimension(doc, &sk, K::Distance { a: lines[0].clone(), b: lines[0].clone(), orientation: O::Horizontal }, w, [20.0, -8.0], false).unwrap();
    ops::add_dimension(doc, &sk, K::Distance { a: lines[1].clone(), b: lines[1].clone(), orientation: O::Vertical }, h, [48.0, 10.0], false).unwrap();
    (sk, pw)
}

fn recompute(k: &mut Kernel, doc: &mut Document) -> Model {
    let m = k.recompute(doc);
    cad_kernel::store_solved_sketches(doc, &m);
    m
}

fn whole_profile(model: &Model, sk: &str) -> Vec<ProfileRef> {
    let p = &model.sketches[sk].profiles[0];
    vec![ProfileRef { curves: p.curves.clone(), sample: p.interior_point() }]
}

/// w x h x d box with its corner at the origin. Returns (extrude id, width parameter).
fn origin_box(k: &mut Kernel, doc: &mut Document, w: &str, h: &str, d: &str) -> (FeatureId, String) {
    let (sk, pw) = rectangle_sketch(doc, w, h);
    let m = recompute(k, doc);
    let ext = ops::new_extrude(doc, &sk, whole_profile(&m, &sk), d, ExtentDirection::Normal, Operation::New);
    let m = recompute(k, doc);
    assert_eq!(m.health_of(&ext), Health::Healthy);
    (ext, pw)
}

/// Unconstrained axis-aligned box [x0,x1] x [y0,y1] x [0,d] as a new body.
fn free_box(k: &mut Kernel, doc: &mut Document, a: [f64; 2], b: [f64; 2], d: &str) -> FeatureId {
    let sk = ops::new_sketch(doc, PlaneRef::Origin { plane: OriginPlane::XY });
    ops::sketch_mut(doc, &sk).unwrap().add_rectangle(a, b);
    let m = recompute(k, doc);
    let ext = ops::new_extrude(doc, &sk, whole_profile(&m, &sk), d, ExtentDirection::Normal, Operation::New);
    let m = recompute(k, doc);
    assert_eq!(m.health_of(&ext), Health::Healthy, "{:?}", m.health_of(&ext));
    ext
}

fn volume(m: &Model) -> f64 {
    m.bodies.iter().map(|b| b.volume()).sum()
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn bbox_of(m: &Model, id: &str) -> [f64; 6] {
    m.body(id).unwrap_or_else(|| panic!("body {id} missing; have {:?}", ids(m))).topology().bbox
}

fn assert_bbox(m: &Model, id: &str, expect: [f64; 6]) {
    let b = bbox_of(m, id);
    for (x, y) in b.iter().zip(expect) {
        assert!(close(*x, y, 1e-6), "{id}: bbox {b:?}, expected {expect:?}");
    }
}

fn ids(m: &Model) -> Vec<String> {
    m.bodies.iter().map(|b| b.id.clone()).collect()
}

fn centre(m: &Model, id: &str) -> [f64; 3] {
    let b = bbox_of(m, id);
    [(b[0] + b[3]) / 2.0, (b[1] + b[4]) / 2.0, (b[2] + b[5]) / 2.0]
}

fn top_face(m: &Model, body: &str) -> FaceRef {
    let b = m.body(body).unwrap();
    let t = b.topology();
    let i = t.face_info.iter().position(|f| f.surface == SurfaceKind::Plane && f.normal[2] > 0.999).unwrap();
    refs::face_ref(b, i).unwrap()
}

#[test]
fn shell_box_removing_top_face() {
    let mut doc = Document::new("shell");
    let mut k = Kernel::new();
    let (ext, pw) = origin_box(&mut k, &mut doc, "50 mm", "30 mm", "20 mm");
    let m = recompute(&mut k, &mut doc);
    let top = top_face(&m, &ext);
    let sh = ops::new_shell(&mut doc, vec![top], "2 mm");
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&sh), Health::Healthy, "{:?}", m.health_of(&sh));
    assert_eq!(ids(&m), vec![ext.clone()], "the shelled body keeps its id");
    assert_eq!(m.bodies[0].created_by, ext);
    let expect = 50.0 * 30.0 * 20.0 - 46.0 * 26.0 * 18.0;
    assert!(close(volume(&m), expect, 1e-3), "{} vs {expect}", volume(&m));
    assert_bbox(&m, &ext, [0.0, 0.0, 0.0, 50.0, 30.0, 20.0]);
    assert_eq!(m.bodies[0].topology().faces.len(), 11, "5 outer + 5 inner faces + the rim");
    assert_eq!(doc.feature(&sh).unwrap().type_label(), "Shell");
    assert_eq!(doc.feature(&sh).unwrap().name, "Shell1");

    // parametric: thickness 3 mm, then an upstream width edit re-resolves the removed face
    let pt = ops::primary_parameter(doc.feature(&sh).unwrap()).unwrap().to_string();
    doc.set_expression(&pt, "3 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert!(close(volume(&m), 30000.0 - 44.0 * 24.0 * 17.0, 1e-3), "{}", volume(&m));
    doc.set_expression(&pt, "2 mm").unwrap();
    doc.set_expression(&pw, "60 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&sh), Health::Healthy);
    assert!(close(volume(&m), 60.0 * 30.0 * 20.0 - 56.0 * 26.0 * 18.0, 1e-3), "{}", volume(&m));

    // invalid thickness: an error with a message, the body as before the shell, no crash
    for bad in ["0 mm", "-1 mm"] {
        doc.set_expression(&pt, bad).unwrap();
        let m = recompute(&mut k, &mut doc);
        assert!(matches!(m.health_of(&sh), Health::Error(ref e) if e.contains("greater than zero")), "{bad}: {:?}", m.health_of(&sh));
        assert!(close(volume(&m), 36000.0, 1e-6));
    }
    doc.set_expression(&pt, "20 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    match m.health_of(&sh) {
        Health::Error(e) => assert!(!e.is_empty()),
        h => panic!("20 mm walls in a 30 mm wide box must fail, got {h:?} with volume {}", volume(&m)),
    }
    assert!(close(volume(&m), 36000.0, 1e-6), "a failed shell leaves the body unchanged");
    doc.set_expression(&pt, "2 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&sh), Health::Healthy, "recovers");
}

#[test]
fn mirror_bodies_across_origin_face_and_construction_planes() {
    let mut doc = Document::new("mirror");
    let mut k = Kernel::new();
    let (ext, _) = origin_box(&mut k, &mut doc, "50 mm", "30 mm", "10 mm");
    let mi = ops::new_mirror(&mut doc, vec![ext.clone()], PlaneRef::Origin { plane: OriginPlane::YZ });
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&mi), Health::Healthy, "{:?}", m.health_of(&mi));
    assert_eq!(ids(&m), vec![ext.clone(), mi.clone()]);
    assert_eq!(m.body(&mi).unwrap().created_by, mi);
    assert!(close(volume(&m), 2.0 * 15000.0, 1e-6), "mirror doubles the volume");
    assert_bbox(&m, &mi, [-50.0, 0.0, 0.0, 0.0, 30.0, 10.0]);
    assert!(m.body(&mi).unwrap().shape.is_valid());

    // across the +X face of the box (x = 50)
    let face = {
        let b = m.body(&ext).unwrap();
        let t = b.topology();
        let i = t.face_info.iter().position(|f| f.normal[0] > 0.999).unwrap();
        refs::face_ref(b, i).unwrap()
    };
    let mi2 = ops::new_mirror(&mut doc, vec![ext.clone()], PlaneRef::Face { face });
    // across a construction plane offset 100 mm from YZ
    let pl = ops::new_plane(&mut doc, PlaneRef::Origin { plane: OriginPlane::YZ }, "100 mm");
    let mi3 = ops::new_mirror(&mut doc, vec![ext.clone(), mi.clone()], PlaneRef::Construction { feature: pl.clone() });
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&mi2), Health::Healthy);
    assert_eq!(m.health_of(&mi3), Health::Healthy);
    assert_bbox(&m, &mi2, [50.0, 0.0, 0.0, 100.0, 30.0, 10.0]);
    assert_bbox(&m, &mi3, [150.0, 0.0, 0.0, 200.0, 30.0, 10.0]);
    assert_bbox(&m, &format!("{mi3}~1"), [200.0, 0.0, 0.0, 250.0, 30.0, 10.0]);
    assert!(close(volume(&m), 5.0 * 15000.0, 1e-6));

    let deps = feature_dependencies(&doc, doc.feature(&mi3).unwrap());
    assert!(deps.contains(&pl) && deps.contains(&ext) && deps.contains(&mi), "{deps:?}");
    assert!(feature_dependencies(&doc, doc.feature(&mi2).unwrap()).contains(&ext));
    assert!(!doc.reorder_feature(&mi3, 0), "cannot move before its inputs");

    // deleting the construction plane: the mirror reports a lost reference, others still work
    doc.delete_feature(&pl).unwrap();
    let m = recompute(&mut k, &mut doc);
    assert!(matches!(m.health_of(&mi3), Health::Error(ref e) if e.contains("plane reference is lost")), "{:?}", m.health_of(&mi3));
    assert_eq!(m.health_of(&mi2), Health::Healthy);
}

#[test]
fn rectangular_pattern_one_and_two_directions_and_join() {
    let mut doc = Document::new("rect");
    let mut k = Kernel::new();
    let (ext, _) = origin_box(&mut k, &mut doc, "50 mm", "30 mm", "10 mm");
    let rp = ops::new_rectangular_pattern(&mut doc, vec![ext.clone()], OriginAxis::X, "3", "60 mm", None, false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&rp), Health::Healthy, "{:?}", m.health_of(&rp));
    assert_eq!(ids(&m), vec![ext.clone(), rp.clone(), format!("{rp}~1")], "quantity counts the original");
    assert!(close(volume(&m), 3.0 * 15000.0, 1e-6));
    assert_bbox(&m, &rp, [60.0, 0.0, 0.0, 110.0, 30.0, 10.0]);
    assert_bbox(&m, &format!("{rp}~1"), [120.0, 0.0, 0.0, 170.0, 30.0, 10.0]);
    let qp = match &doc.feature(&rp).unwrap().kind {
        FeatureKind::RectangularPattern { quantity1, .. } => quantity1.clone(),
        _ => unreachable!(),
    };
    assert_eq!(doc.parameter(&qp).unwrap().quantity, Quantity::Unitless);

    // second direction along Y: 3 x 2 = 6 instances
    let (d1p, q2p, d2p) = match &mut doc.feature_mut(&rp).unwrap().kind {
        FeatureKind::RectangularPattern { axis2, distance1, quantity2, distance2, .. } => {
            *axis2 = Some(OriginAxis::Y);
            (distance1.clone(), quantity2.clone(), distance2.clone())
        }
        _ => unreachable!(),
    };
    doc.set_expression(&q2p, "2").unwrap();
    doc.set_expression(&d2p, "-40 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.bodies.len(), 6);
    assert!(close(volume(&m), 6.0 * 15000.0, 1e-6));
    assert_bbox(&m, &format!("{rp}~4"), [120.0, -40.0, 0.0, 170.0, -10.0, 10.0]);

    // quantity parameters: non-integer -> error; the model is as before the pattern
    doc.set_expression(&qp, "2.5").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert!(matches!(m.health_of(&rp), Health::Error(ref e) if e.contains("whole number")), "{:?}", m.health_of(&rp));
    assert_eq!(m.bodies.len(), 1);
    doc.set_expression(&qp, "0").unwrap();
    assert!(recompute(&mut k, &mut doc).health_of(&rp).is_error());

    // Join with overlapping spacing: one body 100 x 30 x 10
    doc.set_expression(&qp, "3").unwrap();
    if let FeatureKind::RectangularPattern { axis2, join, .. } = &mut doc.feature_mut(&rp).unwrap().kind {
        *axis2 = None;
        *join = true;
    }
    doc.set_expression(&d1p, "25 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&rp), Health::Healthy);
    assert_eq!(ids(&m), vec![ext.clone()], "joined to the original, which keeps its id");
    assert!(close(volume(&m), 100.0 * 30.0 * 10.0, 1e-6), "{}", volume(&m));
    assert_bbox(&m, &ext, [0.0, 0.0, 0.0, 100.0, 30.0, 10.0]);
}

#[test]
fn circular_pattern_of_an_off_axis_box() {
    let mut doc = Document::new("circ");
    let mut k = Kernel::new();
    let bx = free_box(&mut k, &mut doc, [20.0, -5.0], [30.0, 5.0], "10 mm");
    let m = recompute(&mut k, &mut doc);
    assert_bbox(&m, &bx, [20.0, -5.0, 0.0, 30.0, 5.0, 10.0]);
    let cp = ops::new_circular_pattern(&mut doc, vec![bx.clone()], OriginAxis::Z, "4", "360 deg", false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&cp), Health::Healthy, "{:?}", m.health_of(&cp));
    assert_eq!(ids(&m), vec![bx.clone(), cp.clone(), format!("{cp}~1"), format!("{cp}~2")]);
    assert!(close(volume(&m), 4.0 * 1000.0, 1e-6));
    // instances 90 deg apart (counter-clockwise about +Z)
    let expect = [[0.0, 25.0], [-25.0, 0.0], [0.0, -25.0]];
    for (i, id) in [cp.clone(), format!("{cp}~1"), format!("{cp}~2")].iter().enumerate() {
        let c = centre(&m, id);
        assert!(close(c[0], expect[i][0], 1e-6) && close(c[1], expect[i][1], 1e-6) && close(c[2], 5.0, 1e-6), "{id}: {c:?}");
    }

    // partial angle: 3 instances over 90 deg -> 0, 45, 90
    let (qp, ap) = match &doc.feature(&cp).unwrap().kind {
        FeatureKind::CircularPattern { quantity, angle, .. } => (quantity.clone(), angle.clone()),
        _ => unreachable!(),
    };
    doc.set_expression(&qp, "3").unwrap();
    doc.set_expression(&ap, "90 deg").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.bodies.len(), 3);
    assert!(close(m.body(&cp).unwrap().volume(), 1000.0, 1e-6));
    let b = bbox_of(&m, &cp);
    assert!(b[0] > 0.0 && b[1] > 0.0, "45 deg instance in the first quadrant: {b:?}");
    // the 45 deg instance is the box rotated: its centre (25, 0) maps to 25 (cos 45, sin 45);
    // the bounding box of a rotated square is symmetric about it
    let c = centre(&m, &cp);
    let r = 25.0 * std::f64::consts::FRAC_1_SQRT_2;
    assert!(close(c[0], r, 1e-3) && close(c[1], r, 1e-3), "{c:?}");
    assert!(close(centre(&m, &format!("{cp}~1"))[0], 0.0, 1e-6) && close(centre(&m, &format!("{cp}~1"))[1], 25.0, 1e-6));
}

#[test]
fn combine_join_cut_intersect_and_keep_tools() {
    let mut doc = Document::new("combine");
    let mut k = Kernel::new();
    let (a, _) = origin_box(&mut k, &mut doc, "50 mm", "30 mm", "20 mm");
    let b = free_box(&mut k, &mut doc, [40.0, 10.0], [60.0, 20.0], "20 mm");
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.bodies.len(), 2);
    assert!(close(volume(&m), 30000.0 + 4000.0, 1e-6));

    let c = ops::new_combine(&mut doc, a.clone(), vec![b.clone()], Operation::Cut, false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&c), Health::Healthy, "{:?}", m.health_of(&c));
    assert_eq!(ids(&m), vec![a.clone()], "the tool is consumed; the target keeps its id");
    assert!(close(volume(&m), 30000.0 - 2000.0, 1e-6), "{}", volume(&m));

    let set = |doc: &mut Document, op: Operation, keep: bool| {
        if let FeatureKind::Combine { operation, keep_tools, .. } = &mut doc.feature_mut(&c).unwrap().kind {
            *operation = op;
            *keep_tools = keep;
        }
    };
    set(&mut doc, Operation::Cut, true);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(ids(&m), vec![a.clone(), b.clone()]);
    assert!(close(m.body(&a).unwrap().volume(), 28000.0, 1e-6) && close(m.body(&b).unwrap().volume(), 4000.0, 1e-6));
    set(&mut doc, Operation::Join, false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(ids(&m), vec![a.clone()]);
    assert!(close(volume(&m), 32000.0, 1e-6), "{}", volume(&m));
    assert_bbox(&m, &a, [0.0, 0.0, 0.0, 60.0, 30.0, 20.0]);
    set(&mut doc, Operation::Intersect, false);
    let m = recompute(&mut k, &mut doc);
    assert!(close(volume(&m), 2000.0, 1e-6), "{}", volume(&m));

    // dependencies: the combine depends on both bodies' features
    let deps = feature_dependencies(&doc, doc.feature(&c).unwrap());
    assert!(deps.contains(&a) && deps.contains(&b), "{deps:?}");

    // a missing tool / target -> reference errors, no crash
    if let FeatureKind::Combine { tools, .. } = &mut doc.feature_mut(&c).unwrap().kind {
        *tools = vec!["nosuchbody".into()];
    }
    let m = recompute(&mut k, &mut doc);
    assert!(matches!(m.health_of(&c), Health::Error(ref e) if e.contains("reference failure")), "{:?}", m.health_of(&c));
    assert_eq!(m.bodies.len(), 2, "model as before the combine");
    doc.delete_feature(&a).unwrap();
    let m = recompute(&mut k, &mut doc);
    assert!(matches!(m.health_of(&c), Health::Error(ref e) if e.contains("target body reference is lost")), "{:?}", m.health_of(&c));
}

#[test]
fn move_and_copy_bodies() {
    let mut doc = Document::new("move");
    let mut k = Kernel::new();
    let (ext, pw) = origin_box(&mut k, &mut doc, "50 mm", "30 mm", "10 mm");
    let mv = ops::new_move(&mut doc, vec![ext.clone()], "10 mm", "0 mm", "0 mm", OriginAxis::Z, "0 deg", false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&mv), Health::Healthy, "{:?}", m.health_of(&mv));
    assert_eq!(ids(&m), vec![ext.clone()], "moved in place, same id");
    assert_bbox(&m, &ext, [10.0, 0.0, 0.0, 60.0, 30.0, 10.0]);
    assert!(close(volume(&m), 15000.0, 1e-6));

    // rotate 90 deg about Z (then translate +10 X): [0,50]x[0,30] -> [-30,0]x[0,50] -> +10
    let ap = match &doc.feature(&mv).unwrap().kind {
        FeatureKind::MoveBodies { angle, .. } => angle.clone(),
        _ => unreachable!(),
    };
    doc.set_expression(&ap, "90 deg").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_bbox(&m, &ext, [-20.0, 0.0, 0.0, 10.0, 50.0, 10.0]);

    // a fillet downstream of the move still finds its edges after an upstream width edit
    let verticals: Vec<EdgeRef> = {
        let b = m.body(&ext).unwrap();
        let t = b.topology();
        t.edge_info.iter().enumerate().filter(|(_, e)| e.curve == CurveKind::Line && e.dir[2].abs() > 0.999).map(|(i, _)| refs::edge_ref(b, i).unwrap()).collect()
    };
    let fil = ops::new_fillet(&mut doc, verticals, "2 mm");
    doc.set_expression(&pw, "60 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&fil), Health::Healthy);
    assert_bbox(&m, &ext, [-20.0, 0.0, 0.0, 10.0, 60.0, 10.0]);
    doc.delete_feature(&fil).unwrap();

    // Copy: the original stays, the copy is a new body
    doc.set_expression(&ap, "0 deg").unwrap();
    if let FeatureKind::MoveBodies { copy, .. } = &mut doc.feature_mut(&mv).unwrap().kind {
        *copy = true;
    }
    let m = recompute(&mut k, &mut doc);
    assert_eq!(ids(&m), vec![ext.clone(), mv.clone()]);
    assert_eq!(m.body(&mv).unwrap().created_by, mv);
    assert_bbox(&m, &ext, [0.0, 0.0, 0.0, 60.0, 30.0, 10.0]);
    assert_bbox(&m, &mv, [10.0, 0.0, 0.0, 70.0, 30.0, 10.0]);
    assert!(close(volume(&m), 2.0 * 18000.0, 1e-6));

    // deleting the moved body's feature -> reference error
    doc.delete_feature(&ext).unwrap();
    let m = recompute(&mut k, &mut doc);
    assert!(matches!(m.health_of(&mv), Health::Error(ref e) if e.contains("reference failure")), "{:?}", m.health_of(&mv));
}

#[test]
fn dependencies_follow_numbered_bodies() {
    let mut doc = Document::new("deps");
    let mut k = Kernel::new();
    let (ext, _) = origin_box(&mut k, &mut doc, "50 mm", "30 mm", "10 mm");
    let rp = ops::new_rectangular_pattern(&mut doc, vec![ext.clone()], OriginAxis::X, "3", "50 mm", None, false);
    // combine the third instance ("{rp}~1", x 100..150) into the second ("{rp}", x 50..100)
    let c = ops::new_combine(&mut doc, rp.clone(), vec![format!("{rp}~1")], Operation::Join, false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&c), Health::Healthy);
    assert_eq!(ids(&m), vec![ext.clone(), rp.clone()]);
    assert_bbox(&m, &rp, [50.0, 0.0, 0.0, 150.0, 30.0, 10.0]);
    assert!(feature_dependencies(&doc, doc.feature(&c).unwrap()).contains(&rp), "a '~k' body id depends on its feature");
    assert_eq!(dependent_features(&doc, &rp), vec![c.clone()]);
    assert!(dependent_features(&doc, &ext).contains(&rp) && dependent_features(&doc, &ext).contains(&c));
    let i = doc.feature_index(&rp).unwrap();
    assert!(!doc.reorder_feature(&c, i), "the combine cannot move before the pattern");
}

#[test]
fn modified_bodies_never_reuse_existing_ids() {
    let mut doc = Document::new("ids");
    let mut k = Kernel::new();
    let (ext, _) = origin_box(&mut k, &mut doc, "50 mm", "30 mm", "10 mm");
    let rp = ops::new_rectangular_pattern(&mut doc, vec![ext.clone()], OriginAxis::X, "3", "60 mm", None, false);
    // join two non-touching bodies: the result has two lumps; the second must not take the
    // id of the pattern's own "{rp}~1"
    let c = ops::new_combine(&mut doc, rp.clone(), vec![ext.clone()], Operation::Join, false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&c), Health::Healthy);
    let all = ids(&m);
    let unique: std::collections::HashSet<&String> = all.iter().collect();
    assert_eq!(unique.len(), all.len(), "duplicate body ids: {all:?}");
    assert_eq!(all, vec![rp.clone(), format!("{rp}~2"), format!("{rp}~1")]);
    assert!(close(volume(&m), 3.0 * 15000.0, 1e-6));
}

#[test]
fn shell_thickness_just_past_the_limit_fails() {
    // 30 mm wide box: 2 x 15 mm walls meet; 16 mm must not report a healthy shell
    let mut doc = Document::new("shell-limit");
    let mut k = Kernel::new();
    let (ext, _) = origin_box(&mut k, &mut doc, "50 mm", "30 mm", "20 mm");
    let m = recompute(&mut k, &mut doc);
    let sh = ops::new_shell(&mut doc, vec![top_face(&m, &ext)], "16 mm");
    let m = recompute(&mut k, &mut doc);
    let h = m.health_of(&sh);
    eprintln!("16 mm shell: {h:?}");
    assert!(h.is_error(), "{h:?} (volume {})", volume(&m));
    assert!(close(volume(&m), 30000.0, 1e-6));
    let pt = ops::primary_parameter(doc.feature(&sh).unwrap()).unwrap().to_string();
    doc.set_expression(&pt, "14 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&sh), Health::Healthy, "14 mm walls still fit");
    assert!(close(volume(&m), 30000.0 - 22.0 * 2.0 * 6.0, 1e-3), "{}", volume(&m));
}

#[test]
fn new_kinds_persist_and_recompute_identically() {
    let mut doc = Document::new("persist");
    let mut k = Kernel::new();
    let (a, _) = origin_box(&mut k, &mut doc, "50 mm", "30 mm", "20 mm");
    let b = free_box(&mut k, &mut doc, [40.0, 10.0], [60.0, 20.0], "20 mm");
    let m = recompute(&mut k, &mut doc);
    let top = top_face(&m, &a);
    ops::new_combine(&mut doc, a.clone(), vec![b.clone()], Operation::Cut, true);
    ops::new_shell(&mut doc, vec![top], "2 mm");
    ops::new_mirror(&mut doc, vec![b.clone()], PlaneRef::Origin { plane: OriginPlane::XZ });
    ops::new_rectangular_pattern(&mut doc, vec![b.clone()], OriginAxis::Z, "2", "30 mm", Some((OriginAxis::Y, "2", "40 mm")), false);
    ops::new_circular_pattern(&mut doc, vec![b.clone()], OriginAxis::Z, "3", "360 deg", false);
    ops::new_move(&mut doc, vec![b.clone()], "0 mm", "0 mm", "100 mm", OriginAxis::X, "45 deg", true);
    let m = recompute(&mut k, &mut doc);
    for f in &doc.features {
        assert!(!m.health_of(&f.id).is_error(), "{}: {:?}", f.name, m.health_of(&f.id));
    }
    let names: Vec<&str> = doc.features.iter().skip(4).map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Combine1", "Shell1", "Mirror1", "RectangularPattern1", "CircularPattern1", "Move1"]);

    let text = doc.to_json();
    let back = Document::from_json(&text).unwrap();
    // (solved sketch coordinates like 1e-24 may differ in the last bit through JSON)
    assert_eq!(back.features[4..], doc.features[4..], "the new kinds round-trip exactly");
    assert_eq!(back.parameters, doc.parameters);
    let m2 = Kernel::new().recompute(&back);
    assert_eq!(ids(&m2), ids(&m));
    assert!(close(volume(&m2), volume(&m), 1e-6));
    for (x, y) in m.bodies.iter().zip(&m2.bodies) {
        assert!(close(x.volume(), y.volume(), 1e-6), "{}", x.id);
    }
}
