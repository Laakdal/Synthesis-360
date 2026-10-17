//! Primitive construction: topology counts (ASM seamless convention), validity, orientation,
//! queries and rigid transforms. Inputs are the `research/cases.json` primitives.

use cad_geom::primitives::*;
use cad_geom::{validate, Brep, EdgeId, FaceId, Frame, SolidId, Surface, Tolerance, P3, V3};
use nalgebra::{Rotation3, Unit};

fn cases() -> Vec<(&'static str, Brep)> {
    vec![
        ("prim.box", make_box(P3::origin(), [50.0, 30.0, 10.0]).unwrap()),
        ("prim.box.tiny", make_box(P3::origin(), [0.01, 0.02, 0.03]).unwrap()),
        ("prim.box.huge", make_box(P3::origin(), [100000.0, 50000.0, 20000.0]).unwrap()),
        ("prim.cylinder", make_cylinder(Frame::world(), 10.0, 20.0).unwrap()),
        ("prim.cylinder.tilted", make_cylinder(Frame::from_origin_z(P3::new(1.0, 2.0, 3.0), V3::new(1.0, 1.0, 1.0)), 4.0, 15.0).unwrap()),
        ("prim.cone", make_cone(Frame::world(), 10.0, 0.0, 20.0).unwrap()),
        ("prim.frustum", make_cone(Frame::world(), 10.0, 5.0, 20.0).unwrap()),
        ("prim.sphere", make_sphere(P3::origin(), 10.0).unwrap()),
        ("prim.torus", make_torus(Frame::world(), 20.0, 5.0).unwrap()),
    ]
}

/// (F, E incl. degenerate, V, L, degenerate edges, genus)
fn expected(id: &str) -> (usize, usize, usize, usize, usize, i64) {
    match id {
        s if s.starts_with("prim.box") => (6, 12, 8, 6, 0, 0),
        "prim.cylinder" | "prim.cylinder.tilted" | "prim.frustum" => (3, 2, 2, 4, 0, 0),
        "prim.cone" => (2, 2, 2, 3, 1, 0),
        "prim.sphere" => (1, 0, 0, 0, 0, 0),
        "prim.torus" => (1, 0, 0, 0, 0, 1),
        _ => unreachable!(),
    }
}

#[test]
fn counts_and_validity() {
    for (id, b) in cases() {
        let r = validate(&b, &Tolerance::default());
        assert!(r.is_valid(), "{id}: {:?}", r.issues);
        let got = (r.faces, r.edges, r.vertices, r.loops, r.degenerate_edges, r.genus);
        assert_eq!(got, expected(id), "{id}");
        assert_eq!((r.shells, b.solids.len()), (1, 1), "{id}");
    }
}

#[test]
fn outward_orientation() {
    // At a sample point of every face the outward normal points away from the solid's centre.
    for (id, b) in cases() {
        let mp = cad_geom::mass::solid_properties(&b, SolidId(0)).unwrap();
        for f in b.face_ids() {
            let face = b.face(f);
            let (u, v) = match &face.surface {
                Surface::Plane { frame } => {
                    // a vertex of the face projected into the plane, nudged towards the centroid
                    let c = b.face_coedges(f).next().unwrap();
                    let p = b.coedge_points(c).0 + (frame.to_world(frame.to_local(mp.centroid)) - b.coedge_points(c).0) * 0.1;
                    let l = frame.to_local(p);
                    (l.x, l.y)
                }
                Surface::Cone { .. } | Surface::Cylinder { .. } => (1.0, 5.0),
                _ => (1.0, 0.3),
            };
            let n = face.surface.normal(u, v).unwrap() * if face.reversed { -1.0 } else { 1.0 };
            let p = face.surface.point(u, v);
            let away = p - mp.centroid;
            assert!(n.dot(&away) > 0.0, "{id} F{}: normal {n:?} at {p:?}", f.0);
        }
    }
}

#[test]
fn bottom_cap_is_reversed_plane() {
    let b = make_cylinder(Frame::world(), 10.0, 20.0).unwrap();
    let flags: Vec<(&str, bool)> = b.faces.iter().map(|f| (f.surface.type_name(), f.reversed)).collect();
    assert_eq!(flags, vec![("cylinder", false), ("plane", true), ("plane", false)]);
}

#[test]
fn queries() {
    let b = make_cylinder(Frame::world(), 10.0, 20.0).unwrap();
    assert_eq!(b.solid_faces(SolidId(0)).count(), 3);
    let side = FaceId(0);
    assert_eq!(b.face(side).loops.len(), 2);
    assert_eq!(b.face_coedges(side).count(), 2);
    assert_eq!(b.face_edges(side), vec![EdgeId(0), EdgeId(1)]);
    // each circle bounds the side face and one cap, used once by each in opposite directions
    assert_eq!(b.edge_faces(EdgeId(0)), vec![side, FaceId(1)]);
    assert_eq!(b.edge_faces(EdgeId(1)), vec![side, FaceId(2)]);
    for e in [EdgeId(0), EdgeId(1)] {
        let uses = b.edge_coedges(e);
        assert_eq!(uses.len(), 2);
        assert_ne!(b.coedge(uses[0]).reversed, b.coedge(uses[1]).reversed);
        // closed circle on one vertex, on frame.x
        assert_eq!(b.edge(e).start, b.edge(e).end);
        assert!((b.vertex(b.edge(e).start).point.x - 10.0).abs() < 1e-12);
    }
    // coedge endpoints respect `reversed`
    let bx = make_box(P3::origin(), [1.0, 2.0, 3.0]).unwrap();
    for c in bx.face_coedges(FaceId(0)) {
        let (s, e) = bx.coedge_points(c);
        let edge = bx.edge(bx.coedge(c).edge);
        let (ps, pe) = (bx.vertex(edge.start).point, bx.vertex(edge.end).point);
        assert_eq!((s, e), if bx.coedge(c).reversed { (pe, ps) } else { (ps, pe) });
    }
    // box: every edge bounds exactly two faces, every face has 4 edges
    for e in bx.edge_ids() {
        assert_eq!(bx.edge_faces(e).len(), 2);
    }
    for f in bx.face_ids() {
        assert_eq!(bx.face_edges(f).len(), 4);
    }
    // pointed cone: the apex edge has a single use, on the cone face
    let cone = make_cone(Frame::world(), 10.0, 0.0, 20.0).unwrap();
    let apex = cone.edge_ids().find(|&e| cone.is_degenerate(e)).unwrap();
    assert_eq!(cone.edge_coedges(apex).len(), 1);
    assert_eq!(cone.edge_faces(apex), vec![FaceId(0)]);
    assert!((cone.vertex(cone.edge(apex).start).point - P3::new(0.0, 0.0, 20.0)).norm() < 1e-12);
}

#[test]
fn transformed_primitives_stay_valid() {
    let rot = Rotation3::from_axis_angle(&Unit::new_normalize(V3::new(1.0, -2.0, 0.7)), 0.9);
    let t = V3::new(12.0, -3.0, 40.0);
    for (id, b) in cases() {
        let mut moved = b.clone();
        moved.transform(&rot, &t);
        let r = validate(&moved, &Tolerance::for_size(1e5));
        assert!(r.is_valid(), "{id}: {:?}", r.issues);
        for (a, c) in b.vertices.iter().zip(&moved.vertices) {
            assert!((rot * a.point + t - c.point).norm() <= 1e-9 * (1.0 + a.point.coords.norm()));
        }
    }
    let iso = cad_geom::transform::isometry(V3::z(), 0.5, V3::new(1.0, 2.0, 3.0));
    let mut s = make_sphere(P3::origin(), 2.0).unwrap();
    s.transform_iso(&iso);
    let o = s.faces[0].surface.frame().unwrap().origin;
    assert!((o - P3::new(1.0, 2.0, 3.0)).norm() < 1e-12);
}

#[test]
fn invalid_inputs_rejected() {
    assert!(make_box(P3::origin(), [1.0, 0.0, 1.0]).is_err());
    assert!(make_box(P3::origin(), [1.0, f64::NAN, 1.0]).is_err());
    assert!(make_cylinder(Frame::world(), -1.0, 1.0).is_err());
    assert!(make_cone(Frame::world(), 2.0, 2.0, 1.0).is_err());
    assert!(make_cone(Frame::world(), 0.0, 0.0, 1.0).is_err());
    assert!(make_sphere(P3::origin(), 0.0).is_err());
    assert!(make_torus(Frame::world(), 5.0, 5.0).is_err());
}

#[test]
fn inverted_cone_valid() {
    let b = make_cone(Frame::world(), 0.0, 4.0, 3.0).unwrap();
    let r = validate(&b, &Tolerance::default());
    assert!(r.is_valid(), "{:?}", r.issues);
    assert_eq!((r.faces, r.edges, r.vertices, r.loops, r.degenerate_edges), (2, 2, 2, 3, 1));
}
