//! INSPECT panel: minimum distance, angles, interference and centre of mass on exact B-rep
//! solids.

use cad_kernel::inspect::{self, MinDistance};
use cad_kernel::occ::{Seg3, Shape};
use cad_kernel::{new_body_with, Body, BodyKind};

/// Prism of the closed polygon `pts` (on z = z0) with height `h`.
fn prism(pts: &[[f64; 2]], z0: f64, h: f64) -> Shape {
    let segs: Vec<Seg3> = (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            Seg3::Line([a[0], a[1], z0], [b[0], b[1], z0])
        })
        .collect();
    Shape::face_from_loops(&[segs]).unwrap().prism([0.0, 0.0, h]).unwrap()
}

fn cuboid(a: [f64; 3], b: [f64; 3]) -> Shape {
    prism(&[[a[0], a[1]], [b[0], a[1]], [b[0], b[1]], [a[0], b[1]]], a[2], b[2] - a[2])
}

fn body(id: &str, s: Shape) -> Body {
    new_body_with(id.into(), id.into(), s, BodyKind::Solid, Vec::new())
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn min_distance_between_boxes_10mm_apart() {
    let a = cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let b = cuboid([20.0, -5.0, 2.0], [30.0, 5.0, 8.0]);
    let d = inspect::min_distance(&a, &b).unwrap();
    assert!(close(d.distance, 10.0), "{d:?}");
    assert!(close(d.a[0], 10.0) && close(d.b[0], 20.0), "closest points lie on the facing faces: {d:?}");
    assert!(!d.inside);
    // symmetric, and the same between the two facing faces
    let r = inspect::min_distance(&b, &a).unwrap();
    assert!(close(r.distance, 10.0));
    let fa = &body("a", a.clone()).topology();
    let fb = &body("b", b.clone()).topology();
    let face_x = |t: &cad_kernel::Topology, nx: f64| t.faces.iter().zip(&t.face_info).find(|(_, i)| close(i.normal[0], nx)).map(|(f, _)| f.clone()).unwrap();
    let d = inspect::min_distance(&face_x(fa, 1.0), &face_x(fb, -1.0)).unwrap();
    assert!(close(d.distance, 10.0));
    // a corner edge of `a` to box `b`: the closest edge point is on the x = 10 face
    let e = fa.edges.iter().zip(&fa.edge_info).find(|(_, i)| close(i.start[0], 10.0) && close(i.end[0], 10.0)).map(|(e, _)| e.clone()).unwrap();
    assert!(close(inspect::min_distance(&e, &b).unwrap().distance, 10.0));
}

#[test]
fn min_distance_is_diagonal_when_boxes_are_offset() {
    let a = cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let b = cuboid([13.0, 14.0, 0.0], [20.0, 20.0, 10.0]);
    let MinDistance { distance, .. } = inspect::min_distance(&a, &b).unwrap();
    assert!(close(distance, 5.0), "3-4-5 between the nearest vertical edges: {distance}");
}

#[test]
fn touching_and_nested_solids_have_zero_distance() {
    let a = cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let touching = cuboid([10.0, 0.0, 0.0], [20.0, 10.0, 10.0]);
    assert!(inspect::min_distance(&a, &touching).unwrap().distance.abs() < 1e-7);
    let inner = cuboid([2.0, 2.0, 2.0], [4.0, 4.0, 4.0]);
    let d = inspect::min_distance(&a, &inner).unwrap();
    assert!(d.distance.abs() < 1e-7, "{d:?}");
}

#[test]
fn angle_between_adjacent_faces_and_edges_is_90() {
    let b = body("a", cuboid([0.0, 0.0, 0.0], [40.0, 20.0, 10.0]));
    let t = b.topology();
    let top = t.face_info.iter().find(|f| close(f.normal[2], 1.0)).unwrap();
    let side = t.face_info.iter().find(|f| close(f.normal[0], 1.0)).unwrap();
    let bottom = t.face_info.iter().find(|f| close(f.normal[2], -1.0)).unwrap();
    assert!(close(inspect::plane_plane_angle(top.normal, side.normal), 90.0));
    assert!(close(inspect::plane_plane_angle(top.normal, bottom.normal), 0.0), "opposite faces are parallel");
    let lines: Vec<_> = t.edge_info.iter().map(|e| e.dir).collect();
    let x = lines.iter().find(|d| close(d[0].abs(), 1.0)).unwrap();
    let z = lines.iter().find(|d| close(d[2].abs(), 1.0)).unwrap();
    assert!(close(inspect::line_line_angle(*x, *z), 90.0));
    assert!(close(inspect::line_plane_angle(*z, top.normal), 90.0), "a vertical edge is normal to the top face");
    assert!(close(inspect::line_plane_angle(*x, top.normal), 0.0), "a horizontal edge lies parallel to the top face");
    assert!(close(inspect::line_line_angle([1.0, 0.0, 0.0], [1.0, 1.0, 0.0]), 45.0));
}

#[test]
fn interference_volume_of_overlapping_boxes_is_exact() {
    let a = body("a", cuboid([0.0, 0.0, 0.0], [20.0, 20.0, 10.0]));
    let b = body("b", cuboid([10.0, 5.0, 0.0], [30.0, 15.0, 10.0]));
    let far = body("c", cuboid([100.0, 0.0, 0.0], [110.0, 10.0, 10.0]));
    let touching = body("d", cuboid([-10.0, 0.0, 0.0], [0.0, 20.0, 10.0]));
    let r = inspect::interference(&[&a, &b, &far, &touching], 1e-6);
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!((r[0].a, r[0].b), (0, 1));
    assert!(close(r[0].volume, 10.0 * 10.0 * 10.0), "{}", r[0].volume);
    // three-way overlap: every pair is reported with its own shared volume
    let c = body("e", cuboid([5.0, 0.0, 5.0], [25.0, 20.0, 20.0]));
    let r = inspect::interference(&[&a, &b, &c], 1e-6);
    let vol = |i, j| r.iter().find(|p| p.a == i && p.b == j).map(|p| p.volume);
    assert!(close(vol(0, 1).unwrap(), 1000.0));
    assert!(close(vol(0, 2).unwrap(), 15.0 * 20.0 * 5.0));
    assert!(close(vol(1, 2).unwrap(), 15.0 * 10.0 * 5.0));
}

#[test]
fn center_of_mass_of_a_box_and_an_l_shape() {
    let b = body("a", cuboid([0.0, 0.0, 0.0], [40.0, 20.0, 10.0]));
    let p = inspect::center_of_mass(&[&b]).unwrap();
    assert!(close(p.volume, 8000.0));
    assert!(close(p.area, 2.0 * (800.0 + 400.0 + 200.0)));
    assert!(p.centroid.iter().zip([20.0, 10.0, 5.0]).all(|(a, b)| close(*a, b)), "{:?}", p.centroid);
    // L: 30 x 10 foot + 10 x 30 upright, 5 thick -> (10, 15, 2.5)
    let l = body("l", prism(&[[0.0, 0.0], [30.0, 0.0], [30.0, 10.0], [10.0, 10.0], [10.0, 40.0], [0.0, 40.0]], 0.0, 5.0));
    let p = inspect::center_of_mass(&[&l]).unwrap();
    assert!(close(p.volume, 3000.0));
    assert!(p.centroid.iter().zip([10.0, 15.0, 2.5]).all(|(a, b)| close(*a, b)), "{:?}", p.centroid);
    // several bodies: volume-weighted
    let q = body("q", cuboid([100.0, 0.0, 0.0], [110.0, 10.0, 10.0]));
    let p = inspect::center_of_mass(&[&b, &q]).unwrap();
    let x = (20.0 * 8000.0 + 105.0 * 1000.0) / 9000.0;
    assert!(close(p.volume, 9000.0) && close(p.centroid[0], x), "{p:?}");
    // a surface body has no mass
    let s = new_body_with("s".into(), "s".into(), cuboid([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]), BodyKind::Surface, Vec::new());
    assert!(inspect::center_of_mass(&[&s]).is_none());
}
