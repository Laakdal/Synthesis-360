//! History-based (generative) persistent naming: references to faces and edges must survive
//! upstream edits that change the bounding box, add faces or reorder geometric candidates —
//! cases where the purely geometric descriptor (normalised position, normals) picks the wrong
//! face/edge or gives up.

use cad_core::ops;
use cad_core::sketch::{ConstraintKind as K, DistanceOrientation as O, EntityKind, ORIGIN};
use cad_core::*;
use cad_kernel::{refs, Health, Kernel, Model};
use std::f64::consts::PI;

fn recompute(k: &mut Kernel, doc: &mut Document) -> Model {
    let m = k.recompute(doc);
    cad_kernel::store_solved_sketches(doc, &m);
    m
}

fn all_profiles(model: &Model, sk: &str) -> Vec<ProfileRef> {
    model.sketches[sk].profiles.iter().map(|p| ProfileRef { curves: p.curves.clone(), sample: p.interior_point() }).collect()
}

fn volume(m: &Model) -> f64 {
    m.bodies.iter().map(|b| b.volume()).sum()
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// Unconstrained rectangles on XY extruded by `d` with `op`.
fn boxes(k: &mut Kernel, doc: &mut Document, rects: &[([f64; 2], [f64; 2])], d: &str, op: Operation) -> FeatureId {
    let sk = ops::new_sketch(doc, PlaneRef::Origin { plane: OriginPlane::XY });
    for (a, b) in rects {
        ops::sketch_mut(doc, &sk).unwrap().add_rectangle(*a, *b);
    }
    let m = recompute(k, doc);
    let ext = ops::new_extrude(doc, &sk, all_profiles(&m, &sk), d, ExtentDirection::Normal, op);
    let m = recompute(k, doc);
    assert!(!m.health_of(&ext).is_error(), "{:?}", m.health_of(&ext));
    ext
}

/// Runs `add` with the timeline marker just before `before`, then restores the marker to the end.
fn insert_before(k: &mut Kernel, doc: &mut Document, before: &str, add: impl FnOnce(&mut Kernel, &mut Document) -> FeatureId) -> FeatureId {
    doc.set_marker(doc.feature_index(before).unwrap());
    let id = add(k, doc);
    let n = doc.features.len();
    doc.set_marker(n);
    id
}

fn edge_where(m: &Model, body: &str, pred: impl Fn(&cad_kernel::EdgeInfo) -> bool) -> EdgeRef {
    let b = m.body(body).unwrap();
    let t = b.topology();
    let hits: Vec<usize> = t.edge_info.iter().enumerate().filter(|(_, e)| pred(e)).map(|(i, _)| i).collect();
    assert_eq!(hits.len(), 1, "expected exactly one matching edge");
    refs::edge_ref(b, hits[0]).unwrap()
}

fn face_where(m: &Model, body: &str, pred: impl Fn(&cad_kernel::FaceInfo) -> bool) -> FaceRef {
    let b = m.body(body).unwrap();
    let t = b.topology();
    let hits: Vec<usize> = t.face_info.iter().enumerate().filter(|(_, f)| pred(f)).map(|(i, _)| i).collect();
    assert_eq!(hits.len(), 1, "expected exactly one matching face");
    refs::face_ref(b, hits[0]).unwrap()
}

/// Cylindrical faces of all bodies as (axis origin projected to XY, radius).
fn cylinders(m: &Model) -> Vec<([f64; 2], f64)> {
    let mut out = Vec::new();
    for b in &m.bodies {
        for f in &b.topology().face_info {
            if f.surface == SurfaceKind::Cylinder {
                out.push(([f.axis_origin[0], f.axis_origin[1]], f.radius));
            }
        }
    }
    out
}

fn has_cylinder_at(m: &Model, xy: [f64; 2], r: f64) -> bool {
    cylinders(m).iter().any(|(c, rr)| close(c[0], xy[0], 1e-6) && close(c[1], xy[1], 1e-6) && close(*rr, r, 1e-6))
}

/// The scenario defeats the purely geometric descriptor: with its names stripped, the reference
/// (resolved in the body as it is *before* the downstream feature) fails or finds another edge.
fn geometry_alone_fails_edge(before: &Model, r: &EdgeRef) {
    let b = before.body(&r.body).unwrap();
    let by_name = refs::resolve_edge_ex(b, r).unwrap();
    assert_eq!(by_name.via, refs::Via::Name);
    let mut g = r.clone();
    g.face_names.clear();
    let geo = refs::resolve_edge(b, &g);
    assert!(geo.as_ref().map_or(true, |&i| i != by_name.index), "geometry alone would have found the edge too: {geo:?}");
}

fn geometry_alone_fails_face(before: &Model, r: &FaceRef) {
    let b = before.body(&r.body).unwrap();
    let by_name = refs::resolve_face_ex(b, r).unwrap();
    assert_eq!(by_name.via, refs::Via::Name);
    let mut g = r.clone();
    g.name = None;
    let geo = refs::resolve_face(b, &g);
    assert!(geo.as_ref().map_or(true, |&i| i != by_name.index), "geometry alone would have found the face too: {geo:?}");
}

/// The model with the timeline marker just before `feature`.
fn before(k: &mut Kernel, doc: &mut Document, feature: &str) -> Model {
    let n = doc.features.len();
    doc.set_marker(doc.feature_index(feature).unwrap());
    let m = k.recompute(doc);
    doc.set_marker(n);
    m
}

/// (a) Fillet on a box corner edge; an upstream Join extends the body along +X (bounding box
/// 50 -> 150 mm). The fillet must stay on the original corner at (50, 30).
#[test]
fn fillet_survives_upstream_join_that_grows_the_bbox() {
    let mut doc = Document::new("naming-a");
    let mut k = Kernel::new();
    let ext = boxes(&mut k, &mut doc, &[([0.0, 0.0], [50.0, 30.0])], "10 mm", Operation::New);
    let m = recompute(&mut k, &mut doc);
    let e = edge_where(&m, &ext, |e| e.curve == CurveKind::Line && e.dir[2].abs() > 0.999 && close(e.mid[0], 50.0, 1e-6) && close(e.mid[1], 30.0, 1e-6));
    let fil = ops::new_fillet(&mut doc, vec![e.clone()], "5 mm");
    let blend = (25.0 - 25.0 * PI / 4.0) * 10.0;
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&fil), Health::Healthy);
    assert!(close(volume(&m), 15000.0 - blend, 1e-3), "{}", volume(&m));

    let boss = insert_before(&mut k, &mut doc, &fil, |k, d| boxes(k, d, &[([50.0, 0.0], [150.0, 10.0])], "10 mm", Operation::Join));
    geometry_alone_fails_edge(&before(&mut k, &mut doc, &fil), &e);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&boss), Health::Healthy);
    assert_eq!(m.health_of(&fil), Health::Healthy, "the fillet keeps its edge after the upstream join");
    assert_eq!(m.bodies.len(), 1);
    assert!(close(volume(&m), 25000.0 - blend, 1e-3), "{}", volume(&m));
    assert!(has_cylinder_at(&m, [45.0, 25.0], 5.0), "fillet must stay on the (50,30) corner: {:?}", cylinders(&m));
}

/// (b) Hole on the top face of a box; an upstream Join adds a taller boss beside it (new faces,
/// bigger bounding box, a larger +Z face). The hole must still be drilled into the original top.
#[test]
fn hole_stays_on_original_top_face_after_upstream_boss() {
    let mut doc = Document::new("naming-b");
    let mut k = Kernel::new();
    let ext = boxes(&mut k, &mut doc, &[([0.0, 0.0], [50.0, 30.0])], "10 mm", Operation::New);
    let m = recompute(&mut k, &mut doc);
    let top = face_where(&m, &ext, |f| f.surface == SurfaceKind::Plane && f.normal[2] > 0.99);
    let hole = ops::new_hole(&mut doc, top.clone(), [25.0, 15.0], "10 mm", "5 mm", false);
    let drilled = PI * 25.0 * 5.0;
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&hole), Health::Healthy);
    assert!(close(volume(&m), 15000.0 - drilled, 1e-3));

    insert_before(&mut k, &mut doc, &hole, |k, d| boxes(k, d, &[([50.0, 0.0], [150.0, 30.0])], "20 mm", Operation::Join));
    geometry_alone_fails_face(&before(&mut k, &mut doc, &hole), &top);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&hole), Health::Healthy, "{:?}", m.health_of(&hole));
    assert!(close(volume(&m), 15000.0 + 60000.0 - drilled, 1e-3), "{}", volume(&m));
    assert!(has_cylinder_at(&m, [25.0, 15.0], 5.0), "{:?}", cylinders(&m));
}

/// (c) Two fixed bosses on a box whose width is a parameter; fillet one boss edge, then widen the
/// box 50 -> 300 mm so the normalised positions of all candidates shift and reorder.
#[test]
fn fillet_survives_large_width_change() {
    let mut doc = Document::new("naming-c");
    let mut k = Kernel::new();
    let sk = ops::new_sketch(&mut doc, PlaneRef::Origin { plane: OriginPlane::XY });
    let s = ops::sketch_mut(&mut doc, &sk).unwrap();
    let lines = s.add_rectangle([1.0, 1.0], [40.0, 20.0]);
    let corner = match &s.entity(&lines[0]).unwrap().kind {
        EntityKind::Line { p1, .. } => p1.clone(),
        _ => unreachable!(),
    };
    s.add_constraint(K::Coincident { a: corner, b: ORIGIN.into() });
    let (_, pw) = ops::add_dimension(&mut doc, &sk, K::Distance { a: lines[0].clone(), b: lines[0].clone(), orientation: O::Horizontal }, "50 mm", [20.0, -8.0], false).unwrap();
    ops::add_dimension(&mut doc, &sk, K::Distance { a: lines[1].clone(), b: lines[1].clone(), orientation: O::Vertical }, "30 mm", [48.0, 10.0], false).unwrap();
    let m = recompute(&mut k, &mut doc);
    let ext = ops::new_extrude(&mut doc, &sk, all_profiles(&m, &sk), "10 mm", ExtentDirection::Normal, Operation::New);
    recompute(&mut k, &mut doc);
    boxes(&mut k, &mut doc, &[([10.0, 10.0], [20.0, 20.0]), ([30.0, 10.0], [40.0, 20.0])], "30 mm", Operation::Join);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.bodies.len(), 1);
    let e = edge_where(&m, &ext, |e| e.curve == CurveKind::Line && e.dir[2].abs() > 0.999 && close(e.mid[0], 40.0, 1e-6) && close(e.mid[1], 10.0, 1e-6));
    let fil = ops::new_fillet(&mut doc, vec![e.clone()], "2 mm");
    let blend = (4.0 - PI) * 20.0;
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&fil), Health::Healthy);
    assert!(close(volume(&m), 15000.0 + 4000.0 - blend, 1e-3), "{}", volume(&m));

    doc.set_expression(&pw, "300 mm").unwrap();
    geometry_alone_fails_edge(&before(&mut k, &mut doc, &fil), &e);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&fil), Health::Healthy, "{:?}", m.health_of(&fil));
    assert!(close(volume(&m), 90000.0 + 4000.0 - blend, 1e-3), "{}", volume(&m));
    assert!(has_cylinder_at(&m, [38.0, 12.0], 2.0), "{:?}", cylinders(&m));
}

/// (d) Chamfer one top edge of a boss on a box; an upstream joined pattern places an identical
/// copy next to the box, creating a parallel boss edge of equal length at the mirrored
/// normalised position (the edges are collinear but not connected, so the chamfer cannot chain
/// along tangent edges onto the copy).
#[test]
fn chamfer_keeps_its_edge_after_upstream_joined_pattern() {
    let mut doc = Document::new("naming-d");
    let mut k = Kernel::new();
    let ext = boxes(&mut k, &mut doc, &[([0.0, 0.0], [50.0, 30.0])], "10 mm", Operation::New);
    let boss = boxes(&mut k, &mut doc, &[([20.0, 10.0], [30.0, 20.0])], "20 mm", Operation::Join);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.bodies.len(), 1);
    let e = edge_where(&m, &ext, |e| e.curve == CurveKind::Line && e.dir[0].abs() > 0.999 && close(e.mid[1], 10.0, 1e-6) && close(e.mid[2], 20.0, 1e-6));
    let ch = ops::new_chamfer(&mut doc, vec![e.clone()], "2 mm");
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&ch), Health::Healthy);
    assert!(close(volume(&m), 16000.0 - 20.0, 1e-3), "{}", volume(&m));

    let ext_id = ext.clone();
    let rp = insert_before(&mut k, &mut doc, &ch, move |_, d| ops::new_rectangular_pattern(d, vec![ext_id], OriginAxis::X, "2", "50 mm", None, true));
    assert!(doc.feature_index(&rp).unwrap() > doc.feature_index(&boss).unwrap());
    geometry_alone_fails_edge(&before(&mut k, &mut doc, &ch), &e);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&rp), Health::Healthy);
    assert_eq!(m.bodies.len(), 1);
    assert_eq!(m.health_of(&ch), Health::Healthy, "{:?}", m.health_of(&ch));
    assert!(close(volume(&m), 32000.0 - 20.0, 1e-3), "{}", volume(&m));
    // the chamfer face (normal (0,-1,1)/sqrt2) lies on the original boss (x in 20..30)
    let b = &m.bodies[0];
    let bevel: Vec<_> = b.topology().face_info.iter().filter(|f| f.surface == SurfaceKind::Plane && f.normal[1] < -0.5 && f.normal[2] > 0.5).map(|f| f.centroid).collect();
    assert_eq!(bevel.len(), 1);
    assert!(close(bevel[0][0], 25.0, 1e-6), "chamfer moved to the copy: {:?}", bevel);
}

/// Every face of every body the kernel produces carries a generative name.
#[test]
fn kernel_bodies_carry_face_names() {
    let mut doc = Document::new("naming-names");
    let mut k = Kernel::new();
    let ext = boxes(&mut k, &mut doc, &[([0.0, 0.0], [50.0, 30.0])], "10 mm", Operation::New);
    let m = recompute(&mut k, &mut doc);
    {
        let b = m.body(&ext).unwrap();
        let names: Vec<&str> = (0..b.topology().faces.len()).map(|i| b.face_name(i).unwrap_or("")).collect();
        assert!(names.contains(&format!("{ext}/start").as_str()) && names.contains(&format!("{ext}/end").as_str()), "{names:?}");
        assert_eq!(names.iter().filter(|n| n.starts_with(&format!("{ext}/side:"))).count(), 4, "{names:?}");
        let mut uniq = names.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 6, "box faces have distinct names: {names:?}");
    }
    let e = edge_where(&m, &ext, |e| e.curve == CurveKind::Line && e.dir[2].abs() > 0.999 && close(e.mid[0], 50.0, 1e-6) && close(e.mid[1], 30.0, 1e-6));
    assert_eq!(e.face_names.len(), 2);
    let fil = ops::new_fillet(&mut doc, vec![e], "3 mm");
    let m = recompute(&mut k, &mut doc);
    let top = face_where(&m, &ext, |f| f.surface == SurfaceKind::Plane && f.normal[2] > 0.99);
    assert_eq!(top.name.as_deref(), Some(format!("{ext}/end").as_str()));
    let b = m.body(&ext).unwrap();
    let blend = (0..b.topology().faces.len()).filter_map(|i| b.face_name(i)).find(|n| n.starts_with(&format!("{fil}/blend:"))).map(str::to_string);
    assert!(blend.is_some(), "fillet face is named after the feature and the edge");
    ops::new_hole(&mut doc, top, [10.0, 10.0], "6 mm", "4 mm", false);
    let side = face_where(&m, &ext, |f| f.surface == SurfaceKind::Plane && f.normal[1] < -0.99);
    ops::new_shell(&mut doc, vec![side], "1 mm");
    ops::new_rectangular_pattern(&mut doc, vec![ext.clone()], OriginAxis::Z, "2", "10 mm", None, true);
    ops::new_mirror(&mut doc, vec![ext.clone()], PlaneRef::Origin { plane: OriginPlane::YZ });
    let m = recompute(&mut k, &mut doc);
    for f in &doc.features {
        assert!(!m.health_of(&f.id).is_error(), "{}: {:?}", f.id, m.health_of(&f.id));
    }
    assert!(m.bodies.len() >= 2);
    for b in &m.bodies {
        for i in 0..b.topology().faces.len() {
            assert!(b.face_name(i).is_some(), "body {} face {i} has no name", b.id);
        }
    }
}

/// Revolve: lateral faces are named after the sketch curves, caps (partial angle only) start/end.
#[test]
fn revolve_faces_are_named() {
    for (angle, caps) in [("360 deg", false), ("90 deg", true)] {
        let mut doc = Document::new("naming-rev");
        let mut k = Kernel::new();
        let sk = ops::new_sketch(&mut doc, PlaneRef::Origin { plane: OriginPlane::XY });
        ops::sketch_mut(&mut doc, &sk).unwrap().add_rectangle([10.0, 0.0], [20.0, 10.0]);
        let m = recompute(&mut k, &mut doc);
        let rev = ops::new_revolve(&mut doc, &sk, all_profiles(&m, &sk), RevolveAxis::Origin { axis: OriginAxis::Y }, angle, Operation::New);
        let m = recompute(&mut k, &mut doc);
        assert_eq!(m.health_of(&rev), Health::Healthy, "{:?}", m.health_of(&rev));
        let b = m.body(&rev).unwrap();
        let names: Vec<&str> = (0..b.topology().faces.len()).map(|i| b.face_name(i).unwrap_or("")).collect();
        assert_eq!(names.iter().filter(|n| n.starts_with(&format!("{rev}/side:"))).count(), 4, "{names:?}");
        assert_eq!(names.contains(&format!("{rev}/start").as_str()), caps, "{names:?}");
        assert_eq!(names.contains(&format!("{rev}/end").as_str()), caps, "{names:?}");
        assert!(names.iter().all(|n| !n.is_empty()));
    }
}

/// A named reference whose name no longer exists still resolves by geometry, with a warning.
#[test]
fn lost_name_falls_back_to_geometry_with_warning() {
    let mut doc = Document::new("naming-fallback");
    let mut k = Kernel::new();
    let ext = boxes(&mut k, &mut doc, &[([0.0, 0.0], [50.0, 30.0])], "10 mm", Operation::New);
    let m = recompute(&mut k, &mut doc);
    let mut top = face_where(&m, &ext, |f| f.surface == SurfaceKind::Plane && f.normal[2] > 0.99);
    top.name = Some("gone/feature".into());
    let hole = ops::new_hole(&mut doc, top, [25.0, 15.0], "10 mm", "5 mm", false);
    let m = recompute(&mut k, &mut doc);
    assert!(matches!(m.health_of(&hole), Health::Warning(ref s) if s.contains("re-resolved geometrically")), "{:?}", m.health_of(&hole));
    assert!(close(volume(&m), 15000.0 - PI * 25.0 * 5.0, 1e-3));
}

/// References from older documents (no names) keep resolving geometrically and silently.
#[test]
fn unnamed_references_still_resolve() {
    let mut doc = Document::new("naming-old");
    let mut k = Kernel::new();
    let ext = boxes(&mut k, &mut doc, &[([0.0, 0.0], [50.0, 30.0])], "10 mm", Operation::New);
    let m = recompute(&mut k, &mut doc);
    let mut e = edge_where(&m, &ext, |e| e.curve == CurveKind::Line && e.dir[2].abs() > 0.999 && close(e.mid[0], 50.0, 1e-6) && close(e.mid[1], 30.0, 1e-6));
    e.face_names.clear();
    let mut top = face_where(&m, &ext, |f| f.surface == SurfaceKind::Plane && f.normal[2] > 0.99);
    top.name = None;
    let fil = ops::new_fillet(&mut doc, vec![e], "5 mm");
    let hole = ops::new_hole(&mut doc, top, [20.0, 15.0], "10 mm", "5 mm", false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.health_of(&fil), Health::Healthy);
    assert_eq!(m.health_of(&hole), Health::Healthy);
    assert!(close(volume(&m), 15000.0 - (25.0 - 25.0 * PI / 4.0) * 10.0 - PI * 25.0 * 5.0, 1e-3), "{}", volume(&m));
}
