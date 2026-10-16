//! SURFACE tab features (Extrude / Revolve as surfaces, Patch, Stitch, Offset, Thicken) and
//! INSERT ▸ Insert Mesh as parametric timeline features: exact areas and volumes, body kinds,
//! body identity, mesh conversion and persistence.

use cad_core::ops;
use cad_core::*;
use cad_kernel::{refs, BodyKind, Frame, Health, Kernel, Model};
use std::f64::consts::PI;

fn recompute(k: &mut Kernel, doc: &mut Document) -> Model {
    let m = k.recompute(doc);
    cad_kernel::store_solved_sketches(doc, &m);
    m
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn healthy(m: &Model, id: &str) {
    assert_eq!(m.health_of(id), Health::Healthy, "{id}: {:?}", m.health_of(id));
}

fn area(m: &Model, id: &str) -> f64 {
    m.body(id).unwrap_or_else(|| panic!("no body {id}")).shape.area()
}

fn xy_sketch(doc: &mut Document) -> FeatureId {
    ops::new_sketch(doc, PlaneRef::Origin { plane: OriginPlane::XY })
}

/// Rectangle with world corners `a`, `b` (both on the sketch plane `frame`).
fn rect_on(doc: &mut Document, plane: PlaneRef, frame: Frame, a: [f64; 3], b: [f64; 3]) -> FeatureId {
    let sk = ops::new_sketch(doc, plane);
    let (p, q) = (frame.to_local(a), frame.to_local(b));
    ops::sketch_mut(doc, &sk).unwrap().add_rectangle(p, q);
    sk
}

fn profile(m: &Model, sk: &str) -> Vec<ProfileRef> {
    let p = &m.sketches[sk].profiles[0];
    vec![ProfileRef { curves: p.curves.clone(), sample: p.interior_point() }]
}

/// 50 x 30 planar patch on XY at the origin.
fn plate(k: &mut Kernel, doc: &mut Document) -> FeatureId {
    let sk = rect_on(doc, PlaneRef::Origin { plane: OriginPlane::XY }, Frame::origin_plane(OriginPlane::XY), [0.0, 0.0, 0.0], [50.0, 30.0, 0.0]);
    let m = recompute(k, doc);
    let p = ops::new_patch(doc, Some(sk.clone()), profile(&m, &sk), vec![]);
    let m = recompute(k, doc);
    healthy(&m, &p);
    p
}

fn all_faces(m: &Model, body: &str) -> Vec<FaceRef> {
    let b = m.body(body).unwrap();
    (0..b.topology().faces.len()).map(|i| refs::face_ref(b, i).unwrap()).collect()
}

#[test]
fn surface_extrude_of_an_open_chain_gives_one_face_per_curve() {
    let mut k = Kernel::new();
    let mut doc = Document::new("t");
    let sk = xy_sketch(&mut doc);
    let s = ops::sketch_mut(&mut doc, &sk).unwrap();
    // open chain: (0,0) -> (40,0) -> (40,20) -> (10,20), lengths 40 + 20 + 30 = 90
    let l1 = s.add_line([0.0, 0.0], [40.0, 0.0]);
    let l2 = s.add_line([40.0, 0.0], [40.0, 20.0]);
    let l3 = s.add_line([40.0, 20.0], [10.0, 20.0]);
    let m = recompute(&mut k, &mut doc);
    assert!(m.sketches[&sk].profiles.is_empty(), "an open chain has no profile");
    // ids given out of path order: the kernel orders the chain
    let e = ops::new_surface_extrude(&mut doc, &sk, vec![l3.clone(), l1.clone(), l2.clone()], "15 mm", ExtentDirection::Normal);
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &e);
    let b = m.body(&e).unwrap();
    assert_eq!(b.kind, BodyKind::Surface);
    assert_eq!(b.topology().faces.len(), 3);
    assert!(close(area(&m, &e), 90.0 * 15.0, 1e-6), "area {}", area(&m, &e));
    assert_eq!(b.volume(), 0.0, "a surface body has no volume");
    let bb = b.topology().bbox;
    assert!(close(bb[2], 0.0, 1e-6) && close(bb[5], 15.0, 1e-6), "{bb:?}");
    assert!(!b.mesh().indices.is_empty(), "surfaces are displayed");

    // symmetric, parametric distance
    let p = ops::primary_parameter(doc.feature(&e).unwrap()).unwrap().to_string();
    doc.set_expression(&p, "20 mm").unwrap();
    if let FeatureKind::SurfaceExtrude { direction, .. } = &mut doc.feature_mut(&e).unwrap().kind {
        *direction = ExtentDirection::Symmetric;
    }
    let m = recompute(&mut k, &mut doc);
    let bb = m.body(&e).unwrap().topology().bbox;
    assert!(close(bb[2], -10.0, 1e-6) && close(bb[5], 10.0, 1e-6), "{bb:?}");
    assert!(close(area(&m, &e), 90.0 * 20.0, 1e-6));

    // a circle extrudes into a tube surface
    let c = ops::sketch_mut(&mut doc, &sk).unwrap().add_circle([100.0, 0.0], 10.0);
    let e2 = ops::new_surface_extrude(&mut doc, &sk, vec![c], "20 mm", ExtentDirection::Normal);
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &e2);
    assert!(close(area(&m, &e2), 2.0 * PI * 10.0 * 20.0, 1e-6));
    // lost and missing inputs
    if let FeatureKind::SurfaceExtrude { curves, .. } = &mut doc.feature_mut(&e2).unwrap().kind {
        curves.push("gone".into());
    }
    let m = recompute(&mut k, &mut doc);
    assert!(matches!(m.health_of(&e2), Health::Warning(_)), "{:?}", m.health_of(&e2));
    if let FeatureKind::SurfaceExtrude { curves, .. } = &mut doc.feature_mut(&e2).unwrap().kind {
        curves.clear();
    }
    let m = recompute(&mut k, &mut doc);
    assert!(m.health_of(&e2).is_error());
}

#[test]
fn surface_revolve_of_a_line_gives_a_cylinder_surface() {
    let mut k = Kernel::new();
    let mut doc = Document::new("t");
    let sk = xy_sketch(&mut doc);
    let s = ops::sketch_mut(&mut doc, &sk).unwrap();
    let l = s.add_line([10.0, 0.0], [10.0, 20.0]);
    let axis = s.add_line([0.0, 0.0], [0.0, 20.0]);
    recompute(&mut k, &mut doc);
    let r = ops::new_surface_revolve(&mut doc, &sk, vec![l.clone(), axis.clone()], RevolveAxis::SketchLine { entity: axis }, "360 deg");
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &r);
    assert_eq!(m.body(&r).unwrap().kind, BodyKind::Surface);
    assert!(close(area(&m, &r), 2.0 * PI * 10.0 * 20.0, 1e-5), "{}", area(&m, &r));
    let p = ops::primary_parameter(doc.feature(&r).unwrap()).unwrap().to_string();
    doc.set_expression(&p, "90 deg").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert!(close(area(&m, &r), 0.5 * PI * 10.0 * 20.0, 1e-5));
    // origin axis
    if let FeatureKind::SurfaceRevolve { axis, .. } = &mut doc.feature_mut(&r).unwrap().kind {
        *axis = RevolveAxis::Origin { axis: OriginAxis::Y };
    }
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &r);
}

#[test]
fn patch_of_a_rectangle_and_of_a_face_boundary() {
    let mut k = Kernel::new();
    let mut doc = Document::new("t");
    let p = plate(&mut k, &mut doc);
    let m = recompute(&mut k, &mut doc);
    let b = m.body(&p).unwrap();
    assert_eq!(b.kind, BodyKind::Surface);
    assert!(close(area(&m, &p), 1500.0, 1e-6));
    assert_eq!(b.volume(), 0.0);

    // a closed loop of body edges: the four edges of the plate
    let edges: Vec<EdgeRef> = (0..b.topology().edges.len()).map(|i| refs::edge_ref(b, i).unwrap()).collect();
    assert_eq!(edges.len(), 4);
    let p2 = ops::new_patch(&mut doc, None, vec![], edges);
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &p2);
    assert!(close(area(&m, &p2), 1500.0, 1e-4));
    assert!(feature_dependencies(&doc, doc.feature(&p2).unwrap()).contains(&p), "patch depends on the edge owner");
}

#[test]
fn non_planar_edge_loop_is_filled() {
    let mut k = Kernel::new();
    let mut doc = Document::new("t");
    // open half-tube: a semicircle extruded; its boundary (2 arcs + 2 lines) is not planar
    let sk = xy_sketch(&mut doc);
    let a = ops::sketch_mut(&mut doc, &sk).unwrap().add_arc_3pt([10.0, 0.0], [0.0, 10.0], [-10.0, 0.0]).unwrap();
    recompute(&mut k, &mut doc);
    let e = ops::new_surface_extrude(&mut doc, &sk, vec![a], "30 mm", ExtentDirection::Normal);
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &e);
    let b = m.body(&e).unwrap();
    // the arc at z = 0 plus the straight edge at x = 10 and the arc at z = 30 and the edge at x = -10
    let edges: Vec<EdgeRef> = (0..b.topology().edges.len()).map(|i| refs::edge_ref(b, i).unwrap()).collect();
    let p = ops::new_patch(&mut doc, None, vec![], edges);
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &p);
    // the filling spans the open side: larger than the 20 x 30 rectangle across it is not
    // required, but it must be a real surface
    assert!(area(&m, &p) > 100.0, "{}", area(&m, &p));
}

/// Six planar patches forming the boundary of [0,40] x [0,20] x [0,10].
fn box_patches(k: &mut Kernel, doc: &mut Document) -> Vec<FeatureId> {
    let (w, h, d) = (40.0, 20.0, 10.0);
    let xy = Frame::origin_plane(OriginPlane::XY);
    let xz = Frame::origin_plane(OriginPlane::XZ);
    let yz = Frame::origin_plane(OriginPlane::YZ);
    let top = ops::new_plane(doc, PlaneRef::Origin { plane: OriginPlane::XY }, "10 mm");
    let back = ops::new_plane(doc, PlaneRef::Origin { plane: OriginPlane::XZ }, "20 mm");
    let right = ops::new_plane(doc, PlaneRef::Origin { plane: OriginPlane::YZ }, "40 mm");
    let specs: Vec<(PlaneRef, Frame, [f64; 3], [f64; 3])> = vec![
        (PlaneRef::Origin { plane: OriginPlane::XY }, xy, [0.0, 0.0, 0.0], [w, h, 0.0]),
        (PlaneRef::Construction { feature: top }, xy.offset(d), [0.0, 0.0, d], [w, h, d]),
        (PlaneRef::Origin { plane: OriginPlane::XZ }, xz, [0.0, 0.0, 0.0], [w, 0.0, d]),
        (PlaneRef::Construction { feature: back }, xz.offset(h), [0.0, h, 0.0], [w, h, d]),
        (PlaneRef::Origin { plane: OriginPlane::YZ }, yz, [0.0, 0.0, 0.0], [0.0, h, d]),
        (PlaneRef::Construction { feature: right }, yz.offset(w), [w, 0.0, 0.0], [w, h, d]),
    ];
    let mut out = Vec::new();
    for (plane, frame, a, b) in specs {
        let sk = rect_on(doc, plane, frame, a, b);
        let m = recompute(k, doc);
        let p = ops::new_patch(doc, Some(sk.clone()), profile(&m, &sk), vec![]);
        let m = recompute(k, doc);
        healthy(&m, &p);
        out.push(p);
    }
    out
}

#[test]
fn stitching_six_patches_gives_a_solid_box() {
    let mut k = Kernel::new();
    let mut doc = Document::new("t");
    let patches = box_patches(&mut k, &mut doc);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.bodies.iter().filter(|b| b.kind == BodyKind::Surface).count(), 6);
    let st = ops::new_stitch(&mut doc, patches.clone(), "0.01 mm");
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &st);
    assert_eq!(m.bodies.len(), 1, "the patches are consumed: {:?}", m.bodies.iter().map(|b| &b.id).collect::<Vec<_>>());
    let b = &m.bodies[0];
    assert_eq!(b.id, patches[0], "the stitched body keeps the first input's id");
    assert_eq!(b.kind, BodyKind::Solid);
    assert!(close(b.volume(), 8000.0, 1e-6), "volume {}", b.volume());
    assert!(b.shape.is_valid());
    assert_eq!(b.topology().faces.len(), 6);

    // the stitched solid works with solid features: fillet a vertical edge
    let e = (0..b.topology().edges.len()).find(|&i| b.topology().edge_info[i].dir[2].abs() > 0.99).unwrap();
    let fl = ops::new_fillet(&mut doc, vec![refs::edge_ref(b, e).unwrap()], "2 mm");
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &fl);
    let removed = (4.0 - PI) * 10.0;
    assert!(close(m.bodies[0].volume(), 8000.0 - removed, 1e-6));

    // only five patches: an open stitched surface
    if let FeatureKind::Stitch { bodies, .. } = &mut doc.feature_mut(&st).unwrap().kind {
        bodies.pop();
    }
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &st);
    let kinds: Vec<BodyKind> = m.bodies.iter().map(|b| b.kind).collect();
    assert!(kinds.contains(&BodyKind::Surface) && !kinds.contains(&BodyKind::Solid), "{kinds:?}");
    assert_eq!(m.health_of(&fl).is_error(), true, "the fillet's solid is gone");
}

#[test]
fn thicken_a_plate_into_a_solid() {
    let mut k = Kernel::new();
    let mut doc = Document::new("t");
    let p = plate(&mut k, &mut doc);
    let m = recompute(&mut k, &mut doc);
    let t = ops::new_thicken(&mut doc, all_faces(&m, &p), "2 mm", false);
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &t);
    assert!(m.body(&p).is_none(), "the surface body is consumed");
    let b = m.body(&t).unwrap();
    assert_eq!(b.kind, BodyKind::Solid);
    assert!(close(b.volume().abs(), 3000.0, 1e-6), "volume {}", b.volume());
    assert!(b.volume() > 0.0, "the solid is oriented outward");
    let bb = b.topology().bbox;
    assert!(close(bb[5] - bb[2], 2.0, 1e-6), "{bb:?}");

    // symmetric: 1 mm on each side
    if let FeatureKind::Thicken { symmetric, .. } = &mut doc.feature_mut(&t).unwrap().kind {
        *symmetric = true;
    }
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &t);
    let b = m.body(&t).unwrap();
    assert!(close(b.volume(), 3000.0, 1e-6));
    let bb = b.topology().bbox;
    assert!(close(bb[2], -1.0, 1e-6) && close(bb[5], 1.0, 1e-6), "{bb:?}");

    // parametric thickness
    let tp = ops::primary_parameter(doc.feature(&t).unwrap()).unwrap().to_string();
    doc.set_expression(&tp, "5 mm").unwrap();
    let m = recompute(&mut k, &mut doc);
    assert!(close(m.body(&t).unwrap().volume(), 7500.0, 1e-6));

    // thickening a 3-face open surface (U channel)
    let sk = xy_sketch(&mut doc);
    let s = ops::sketch_mut(&mut doc, &sk).unwrap();
    let ids = vec![s.add_line([100.0, 0.0], [100.0, 20.0]), s.add_line([100.0, 0.0], [140.0, 0.0]), s.add_line([140.0, 0.0], [140.0, 20.0])];
    recompute(&mut k, &mut doc);
    let e = ops::new_surface_extrude(&mut doc, &sk, ids, "10 mm", ExtentDirection::Normal);
    let m = recompute(&mut k, &mut doc);
    let t2 = ops::new_thicken(&mut doc, all_faces(&m, &e), "1 mm", false);
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &t2);
    let v = m.body(&t2).unwrap().volume();
    // 80 mm of wall x 10 mm x 1 mm, +/- the corner overlap / gap
    assert!(v > 700.0 && v < 900.0, "channel volume {v}");
}

#[test]
fn offset_surface_moves_faces_along_their_normal() {
    let mut k = Kernel::new();
    let mut doc = Document::new("t");
    let p = plate(&mut k, &mut doc);
    let m = recompute(&mut k, &mut doc);
    let o = ops::new_offset_surface(&mut doc, all_faces(&m, &p), "5 mm");
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &o);
    assert!(m.body(&p).is_some(), "offset keeps the original");
    let b = m.body(&o).unwrap();
    assert_eq!(b.kind, BodyKind::Surface);
    assert!(close(area(&m, &o), 1500.0, 1e-6));
    let bb = b.topology().bbox;
    assert!(close(bb[2].abs(), 5.0, 1e-6) && close(bb[5], bb[2], 1e-6), "{bb:?}");

    // a cylinder surface: the offset radius is 10 +/- 5
    let sk = xy_sketch(&mut doc);
    let c = ops::sketch_mut(&mut doc, &sk).unwrap().add_circle([100.0, 0.0], 10.0);
    recompute(&mut k, &mut doc);
    let e = ops::new_surface_extrude(&mut doc, &sk, vec![c], "20 mm", ExtentDirection::Normal);
    let m = recompute(&mut k, &mut doc);
    let o2 = ops::new_offset_surface(&mut doc, all_faces(&m, &e), "5 mm");
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &o2);
    let a = area(&m, &o2);
    assert!(close(a, 2.0 * PI * 15.0 * 20.0, 1e-4) || close(a, 2.0 * PI * 5.0 * 20.0, 1e-4), "area {a}");
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cad360-surface-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// 40 x 20 x 10 box exported as binary STL.
fn box_stl(name: &str) -> Vec<u8> {
    let mut k = Kernel::new();
    let mut doc = Document::new("t");
    let sk = rect_on(&mut doc, PlaneRef::Origin { plane: OriginPlane::XY }, Frame::origin_plane(OriginPlane::XY), [0.0, 0.0, 0.0], [40.0, 20.0, 0.0]);
    let m = recompute(&mut k, &mut doc);
    ops::new_extrude(&mut doc, &sk, profile(&m, &sk), "10 mm", ExtentDirection::Normal, Operation::New);
    let m = recompute(&mut k, &mut doc);
    let path = temp(name);
    let bodies: Vec<&cad_kernel::Body> = m.bodies.iter().collect();
    cad_kernel::exchange::export_stl(&bodies, &path.to_string_lossy(), false).unwrap();
    std::fs::read(&path).unwrap()
}

#[test]
fn inserted_stl_is_a_mesh_body_and_converts_to_a_solid() {
    let stl = box_stl("box.stl");
    assert_eq!(cad_kernel::mesh::stl_triangle_count(&stl), 12, "a box exports as 12 triangles");
    let mut k = Kernel::new();
    let mut doc = Document::new("t");
    let mesh = ops::new_insert_mesh(&mut doc, "box.stl", &stl, false);
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &mesh);
    let b = m.body(&mesh).unwrap();
    assert_eq!(b.kind, BodyKind::Mesh);
    let mesh_data = b.mesh();
    assert_eq!(mesh_data.indices.len(), 36, "the triangles are displayed as they are");
    assert_eq!(mesh_data.normals.len(), mesh_data.positions.len());
    assert!(close(b.volume(), 8000.0, 1e-3), "mesh volume {}", b.volume());
    assert!(close(b.topology().face_info[0].area, 2800.0, 1e-3), "mesh area {}", b.topology().face_info[0].area);
    let bb = b.topology().bbox;
    assert!(close(bb[3], 40.0, 1e-3) && close(bb[5], 10.0, 1e-3), "{bb:?}");

    // Convert to B-rep: a solid box with merged planar faces
    if let FeatureKind::InsertMesh { to_brep, .. } = &mut doc.feature_mut(&mesh).unwrap().kind {
        *to_brep = true;
    }
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &mesh);
    let b = m.body(&mesh).unwrap();
    assert_eq!(b.kind, BodyKind::Solid);
    assert!(close(b.volume(), 8000.0, 1e-3), "brep volume {}", b.volume());
    assert_eq!(b.topology().faces.len(), 6, "coplanar triangles are merged");
    assert!(b.shape.is_valid());
    // solid features work on it
    let e = (0..b.topology().edges.len()).find(|&i| b.topology().edge_info[i].dir[2].abs() > 0.99).unwrap();
    let fl = ops::new_fillet(&mut doc, vec![refs::edge_ref(b, e).unwrap()], "2 mm");
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &fl);
    assert!(m.body(&mesh).unwrap().volume() < 8000.0 - 1.0);

    // ASCII STL and garbage
    let mut ascii = String::from("solid t\n");
    for tri in [[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]]] {
        ascii.push_str(" facet normal 0 0 1\n  outer loop\n");
        for v in tri {
            ascii.push_str(&format!("   vertex {} {} {}\n", v[0], v[1], v[2]));
        }
        ascii.push_str("  endloop\n endfacet\n");
    }
    ascii.push_str("endsolid t\n");
    let tri = ops::new_insert_mesh(&mut doc, "tri.stl", ascii.as_bytes(), false);
    let bad = ops::new_insert_mesh(&mut doc, "bad.stl", b"not a mesh", false);
    let m = recompute(&mut k, &mut doc);
    healthy(&m, &tri);
    assert!(close(m.body(&tri).unwrap().topology().face_info[0].area, 50.0, 1e-9));
    assert!(m.health_of(&bad).is_error());
}

#[test]
fn refined_stl_export_changes_the_triangle_count() {
    use cad_kernel::mesh::{export_stl_refined, stl_triangle_count, Refinement};
    let mut k = Kernel::new();
    let mut doc = Document::new("t");
    let sk = xy_sketch(&mut doc);
    ops::sketch_mut(&mut doc, &sk).unwrap().add_circle([0.0, 0.0], 20.0);
    let m = recompute(&mut k, &mut doc);
    ops::new_extrude(&mut doc, &sk, profile(&m, &sk), "10 mm", ExtentDirection::Normal, Operation::New);
    let m = recompute(&mut k, &mut doc);
    let bodies: Vec<&cad_kernel::Body> = m.bodies.iter().collect();
    let mut counts = Vec::new();
    for r in Refinement::ALL {
        let p = temp(&format!("cyl-{}.stl", r.label()));
        export_stl_refined(&bodies, &p.to_string_lossy(), false, r).unwrap();
        counts.push(stl_triangle_count(&std::fs::read(&p).unwrap()));
    }
    assert!(counts[0] < counts[1] && counts[1] < counts[2], "coarse < medium < fine: {counts:?}");
}

#[test]
fn surface_features_survive_save_and_load() {
    let mut k = Kernel::new();
    let mut doc = Document::new("t");
    let patches = box_patches(&mut k, &mut doc);
    ops::new_stitch(&mut doc, patches, "0.01 mm");
    let p = plate(&mut k, &mut doc);
    let m = recompute(&mut k, &mut doc);
    ops::new_thicken(&mut doc, all_faces(&m, &p), "2 mm", true);
    let stl = box_stl("persist.stl");
    ops::new_insert_mesh(&mut doc, "persist.stl", &stl, false);
    let m = recompute(&mut k, &mut doc);
    assert!(m.health.values().all(|h| !h.is_error()), "{:?}", m.health);

    let text = doc.to_json();
    let back = Document::from_json(&text).unwrap();
    assert_eq!(back, doc);
    let m2 = Kernel::new().recompute(&back);
    let summary = |m: &Model| m.bodies.iter().map(|b| (b.id.clone(), b.kind, (b.volume() * 1e3).round(), (b.shape.area() * 1e3).round())).collect::<Vec<_>>();
    assert_eq!(summary(&m), summary(&m2));
    assert_eq!(m2.bodies.iter().map(|b| b.kind).collect::<Vec<_>>(), vec![BodyKind::Solid, BodyKind::Solid, BodyKind::Mesh]);
}

#[test]
fn stl_export_of_solids_and_mesh_bodies_together() {
    use cad_kernel::mesh::{export_stl_refined, stl_triangle_count, Refinement};
    let stl = box_stl("mixed-src.stl");
    let mut k = Kernel::new();
    let mut doc = Document::new("t");
    let sk = rect_on(&mut doc, PlaneRef::Origin { plane: OriginPlane::XY }, Frame::origin_plane(OriginPlane::XY), [100.0, 0.0, 0.0], [140.0, 20.0, 0.0]);
    let m = recompute(&mut k, &mut doc);
    ops::new_extrude(&mut doc, &sk, profile(&m, &sk), "10 mm", ExtentDirection::Normal, Operation::New);
    ops::new_insert_mesh(&mut doc, "box.stl", &stl, false);
    let m = recompute(&mut k, &mut doc);
    assert_eq!(m.bodies.len(), 2);
    let all: Vec<&cad_kernel::Body> = m.bodies.iter().collect();
    let p1 = temp("mixed-plain.stl");
    cad_kernel::exchange::export_stl(&all, &p1.to_string_lossy(), false).unwrap();
    assert_eq!(stl_triangle_count(&std::fs::read(&p1).unwrap()), 24, "box (12) + mesh body (12)");
    let p2 = temp("mixed-print.stl");
    export_stl_refined(&all, &p2.to_string_lossy(), false, Refinement::Fine).unwrap();
    assert_eq!(stl_triangle_count(&std::fs::read(&p2).unwrap()), 24);
}


#[test]
fn solid_join_next_to_a_surface_body_ignores_the_surface() {
    // regression: Join/Cut targets used bbox overlap only, so a solid extrude overlapping a
    // surface patch tried to fuse with it and failed ("join (fuse) failed")
    let mut doc = Document::new("join-near-surface");
    let mut k = Kernel::new();
    plate(&mut k, &mut doc);
    let sk = rect_on(&mut doc, PlaneRef::Origin { plane: OriginPlane::XY }, Frame::origin_plane(OriginPlane::XY), [10.0, 5.0, 0.0], [40.0, 25.0, 0.0]);
    let m = recompute(&mut k, &mut doc);
    let ext = ops::new_extrude(&mut doc, &sk, profile(&m, &sk), "10 mm", ExtentDirection::Normal, Operation::Join);
    let m = recompute(&mut k, &mut doc);
    assert!(!m.health_of(&ext).is_error(), "{:?}", m.health_of(&ext));
    let solids: Vec<_> = m.bodies.iter().filter(|b| b.kind == BodyKind::Solid).collect();
    assert_eq!(solids.len(), 1);
    assert!(close(solids[0].volume(), 30.0 * 20.0 * 10.0, 1e-6));
    assert_eq!(m.bodies.iter().filter(|b| b.kind == BodyKind::Surface).count(), 1, "the patch is untouched");
}
