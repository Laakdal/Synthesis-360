//! Boolean edge cases: contact without overlap, containment, identical operands and the empty
//! result, near-coincidence on both sides of the tolerance, non-commutativity of cut, scale,
//! non-convex and holed operands, general position (rotated boxes, checked by set identities),
//! unsupported geometry, and mass properties of plane faces with inner loops.

use cad_geom::mass::{face_area, solid_properties};
use cad_geom::primitives::{make_box, make_cylinder};
use cad_geom::{boolean, validate, BoolOp, Brep, FaceId, Frame, GeomError, SolidId, Tolerance, P3, V3};
use nalgebra::Rotation3;

fn bx(c: [f64; 3], s: [f64; 3]) -> Brep {
    make_box(P3::from(c), s).unwrap()
}

fn cube(c: [f64; 3], s: f64) -> Brep {
    bx(c, [s; 3])
}

fn asm() -> Tolerance {
    Tolerance::asm()
}

fn run(a: &Brep, b: &Brep, op: BoolOp) -> Brep {
    let r = boolean(a, b, op, &asm()).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    let rep = validate(&r, &Tolerance::default());
    assert!(rep.is_valid(), "{op:?}: {:?}", rep.issues);
    r
}

fn fev(b: &Brep) -> [usize; 3] {
    [b.faces.len(), b.edges.len(), b.vertices.len()]
}

fn vol(b: &Brep) -> f64 {
    b.solid_ids().map(|s| solid_properties(b, s).unwrap().volume).sum()
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * a.abs().max(b.abs()).max(1e-300)
}

#[test]
fn touching_vertex_only() {
    let (a, b) = (cube([0.0; 3], 10.0), cube([10.0; 3], 10.0));
    let u = run(&a, &b, BoolOp::Union);
    // two lumps, each a full box with its own vertices
    assert_eq!((u.solids.len(), fev(&u)), (2, [12, 24, 16]));
    for s in u.solid_ids() {
        assert!(close(solid_properties(&u, s).unwrap().volume, 1000.0, 1e-12));
    }
    let c = run(&a, &b, BoolOp::Cut);
    assert_eq!((c.solids.len(), fev(&c)), (1, [6, 12, 8]));
    assert!(close(vol(&c), 1000.0, 1e-12));
    let i = run(&a, &b, BoolOp::Intersect);
    assert_eq!((i.solids.len(), i.faces.len()), (0, 0));
    // the same within tolerance (gap 5e-6 mm per axis) is still a vertex contact
    let near = cube([10.0 + 5e-6; 3], 10.0);
    assert_eq!(run(&a, &near, BoolOp::Union).solids.len(), 2);
    assert_eq!(run(&a, &near, BoolOp::Intersect).solids.len(), 0);
}

#[test]
fn touching_edge_only() {
    let (a, b) = (cube([0.0; 3], 10.0), cube([10.0, 10.0, 0.0], 10.0));
    let u = run(&a, &b, BoolOp::Union);
    assert_eq!((u.solids.len(), u.shells.len(), fev(&u)), (2, 2, [12, 24, 16]));
    assert!(close(vol(&u), 2000.0, 1e-12));
    assert_eq!(fev(&run(&a, &b, BoolOp::Cut)), [6, 12, 8]);
    assert_eq!(run(&a, &b, BoolOp::Intersect).solids.len(), 0);
    // a partial edge contact: the tool's edge covers half of the target's edge
    let half = bx([10.0, 10.0, 5.0], [10.0, 10.0, 10.0]);
    let u = run(&a, &half, BoolOp::Union);
    assert_eq!((u.solids.len(), fev(&u)), (2, [12, 24, 16]));
}

#[test]
fn contained_tool() {
    let (a, b) = (cube([0.0; 3], 20.0), cube([5.0; 3], 10.0));
    let u = run(&a, &b, BoolOp::Union);
    assert_eq!((u.solids.len(), fev(&u)), (1, [6, 12, 8]));
    assert!(close(vol(&u), 8000.0, 1e-12));
    let c = run(&a, &b, BoolOp::Cut);
    assert_eq!((c.solids.len(), c.shells.len(), fev(&c)), (1, 2, [12, 24, 16]));
    assert_eq!(c.solid(SolidId(0)).shells.len(), 2, "outer shell + void");
    let m = solid_properties(&c, SolidId(0)).unwrap();
    assert!(close(m.volume, 7000.0, 1e-12) && close(m.area, 2400.0 + 600.0, 1e-12));
    let i = run(&a, &b, BoolOp::Intersect);
    assert_eq!(fev(&i), [6, 12, 8]);
    assert!(close(vol(&i), 1000.0, 1e-12));
    // the reverse cut of a contained solid is empty
    assert_eq!(run(&b, &a, BoolOp::Cut).solids.len(), 0);
}

/// Empty-result convention: `Ok` with an empty B-rep; an empty operand is the empty set.
#[test]
fn identical_solids_and_empty_results() {
    let a = bx([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]);
    for op in [BoolOp::Union, BoolOp::Intersect] {
        let r = run(&a, &a.clone(), op);
        assert_eq!((r.solids.len(), fev(&r)), (1, [6, 12, 8]), "{op:?}");
        assert!(close(vol(&r), 120.0, 1e-12));
    }
    let c = boolean(&a, &a, BoolOp::Cut, &asm()).unwrap();
    assert_eq!(c, Brep::new());
    let e = Brep::new();
    assert_eq!(fev(&run(&e, &a, BoolOp::Union)), [6, 12, 8]);
    assert_eq!(fev(&run(&a, &e, BoolOp::Cut)), [6, 12, 8]);
    assert_eq!(run(&a, &e, BoolOp::Intersect), Brep::new());
    assert_eq!(run(&e, &a, BoolOp::Cut), Brep::new());
    // folding on from an empty result works
    assert_eq!(fev(&run(&c, &a, BoolOp::Union)), [6, 12, 8]);
}

/// Faces closer than ASM's 1e-5 mm are coincident (the two boxes merge into one, with exact
/// snapped geometry valid at 1e-7 mm); farther apart they are separate.
#[test]
fn near_coincident_faces() {
    let a = cube([0.0; 3], 20.0);
    let side = |dx: f64| cube([20.0 + dx, 0.0, 0.0], 20.0);
    for d in [5e-6, -5e-6, 9e-6] {
        let u = run(&a, &side(d), BoolOp::Union);
        assert_eq!((u.solids.len(), fev(&u)), (1, [6, 12, 8]), "offset {d}");
        // the tool's near face snaps onto x = 20; its far face stays at 40 + d
        assert!(close(vol(&u), 400.0 * (40.0 + d), 1e-12), "offset {d}: {}", vol(&u));
        assert_eq!(run(&a, &side(d), BoolOp::Intersect).solids.len(), 0, "offset {d}: contact only");
    }
    // a gap beyond tolerance: two lumps
    let u = run(&a, &side(2e-5), BoolOp::Union);
    assert_eq!((u.solids.len(), fev(&u)), (2, [12, 24, 16]));
    // an overlap beyond tolerance is resolved: a real 2e-5 mm sliver
    let o = side(-2e-5);
    let u = run(&a, &o, BoolOp::Union);
    assert_eq!(fev(&u), [6, 12, 8]);
    assert!(close(vol(&u), 400.0 * (40.0 - 2e-5), 1e-12));
    let i = run(&a, &o, BoolOp::Intersect);
    assert_eq!(fev(&i), [6, 12, 8]);
    assert!(close(vol(&i), 400.0 * 2e-5, 1e-6), "{}", vol(&i));
}

#[test]
fn cut_is_not_commutative() {
    let (a, b) = (cube([0.0; 3], 20.0), cube([10.0; 3], 20.0));
    let (ab, ba) = (run(&a, &b, BoolOp::Cut), run(&b, &a, BoolOp::Cut));
    let (ma, mb) = (solid_properties(&ab, SolidId(0)).unwrap(), solid_properties(&ba, SolidId(0)).unwrap());
    assert!(close(ma.volume, 7000.0, 1e-12) && close(mb.volume, 7000.0, 1e-12));
    assert!((ma.centroid - mb.centroid).norm() > 1.0, "{:?} vs {:?}", ma.centroid, mb.centroid);
    assert!(ma.centroid.x < 10.0 && mb.centroid.x > 20.0);
    // (A − B) ∪ (A ∩ B) rebuilds A exactly, merged back to a plain box
    let back = run(&ab, &run(&a, &b, BoolOp::Intersect), BoolOp::Union);
    assert_eq!(fev(&back), [6, 12, 8]);
    assert!(close(vol(&back), 8000.0, 1e-12));
}

fn scaled(c: [f64; 3], s: [f64; 3], k: f64) -> Brep {
    bx(c.map(|x| x * k), s.map(|x| x * k))
}

/// The ASM box cases at 1e-2 and 1e4 scale: same topology, volume scaled by k³.
#[test]
fn scale_invariance() {
    type Case = (BoolOp, [f64; 3], [f64; 3], [f64; 3], [f64; 3], [usize; 3], f64);
    let cases: [Case; 4] = [
        (BoolOp::Union, [0.0; 3], [20.0; 3], [10.0; 3], [20.0; 3], [12, 30, 20], 15000.0),
        (BoolOp::Union, [0.0; 3], [20.0; 3], [20.0, 0.0, 0.0], [20.0; 3], [6, 12, 8], 16000.0),
        (BoolOp::Union, [0.0; 3], [20.0; 3], [20.0, 5.0, 5.0], [20.0, 10.0, 10.0], [11, 24, 16], 10000.0),
        (BoolOp::Cut, [0.0; 3], [20.0; 3], [5.0, 5.0, 0.0], [10.0, 10.0, 20.0], [10, 24, 16], 6000.0),
    ];
    for k in [1e-2, 1e4] {
        for (op, c0, s0, c1, s1, want, v) in cases {
            let r = run(&scaled(c0, s0, k), &scaled(c1, s1, k), op);
            assert_eq!(fev(&r), want, "scale {k}");
            assert!(close(vol(&r), v * k * k * k, 1e-9), "scale {k}: {} vs {}", vol(&r), v * k * k * k);
        }
    }
}

/// Non-convex operands and operands whose faces have inner loops.
#[test]
fn non_convex_and_holed_operands() {
    let a = cube([0.0; 3], 20.0);
    let notch = bx([10.0, 10.0, -1.0], [20.0, 20.0, 22.0]);
    let l = run(&a, &notch, BoolOp::Cut);
    assert_eq!(fev(&l), [8, 18, 12]);
    assert!(close(vol(&l), 6000.0, 1e-12));
    // filling the notch merges back to the box
    let back = run(&l, &bx([10.0, 10.0, 0.0], [10.0, 10.0, 20.0]), BoolOp::Union);
    assert_eq!(fev(&back), [6, 12, 8]);
    // a bar through one arm of the L
    let bar = bx([-5.0, 12.0, 5.0], [30.0, 4.0, 4.0]);
    let i = run(&l, &bar, BoolOp::Intersect);
    assert_eq!((i.solids.len(), fev(&i)), (1, [6, 12, 8]));
    assert!(close(vol(&i), 10.0 * 4.0 * 4.0, 1e-12));
    assert!(close(vol(&run(&l, &bar, BoolOp::Union)), 6000.0 + 30.0 * 16.0 - 160.0, 1e-12));
    assert!(close(vol(&run(&l, &bar, BoolOp::Cut)), 6000.0 - 160.0, 1e-12));
    // a U and a bar across both arms: the intersection has two lumps, the cut splits both arms
    let u_shape = run(&bx([0.0; 3], [30.0, 20.0, 20.0]), &bx([10.0, 10.0, -1.0], [10.0, 15.0, 22.0]), BoolOp::Cut);
    assert!(close(vol(&u_shape), 10000.0, 1e-12));
    let bar = bx([-5.0, 12.0, 5.0], [40.0, 4.0, 4.0]);
    let i = run(&u_shape, &bar, BoolOp::Intersect);
    assert_eq!((i.solids.len(), fev(&i)), (2, [12, 24, 16]));
    assert!(close(vol(&i), 320.0, 1e-12));
    let u = run(&u_shape, &bar, BoolOp::Union);
    assert_eq!(u.solids.len(), 1);
    assert!(close(vol(&u), 10000.0 + 640.0 - 320.0, 1e-12));
    let c = run(&u_shape, &bar, BoolOp::Cut);
    assert_eq!(c.solids.len(), 1);
    assert!(close(vol(&c), 10000.0 - 320.0, 1e-12));
    // a through-hole (faces with inner loops) plugged again
    let ring = run(&a, &bx([5.0, 5.0, 0.0], [10.0, 10.0, 20.0]), BoolOp::Cut);
    let plugged = run(&ring, &bx([5.0, 5.0, 0.0], [10.0, 10.0, 20.0]), BoolOp::Union);
    assert_eq!(fev(&plugged), [6, 12, 8]);
    assert!(close(vol(&plugged), 8000.0, 1e-12));
    // a box through the ring's wall
    let pin = bx([-2.0, 8.0, 8.0], [10.0, 4.0, 4.0]);
    let r = run(&ring, &pin, BoolOp::Union);
    assert!(close(vol(&r), 6000.0 + 2.0 * 16.0 + 3.0 * 16.0, 1e-12), "{}", vol(&r));
}

/// General position: rotated boxes (no coplanar faces). Checked by the set identities
/// V(A ∪ B) + V(A ∩ B) = V(A) + V(B) and V(A − B) = V(A) − V(A ∩ B).
#[test]
fn rotated_boxes_satisfy_set_identities() {
    let a = bx([0.0; 3], [20.0, 15.0, 10.0]);
    for (k, angle) in [0.3, 0.7, 1.1].into_iter().enumerate() {
        let rot = Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(V3::new(1.0, 2.0, 3.0 + k as f64)), angle);
        let b = bx([0.0; 3], [12.0, 9.0, 14.0]).transformed(&rot, &V3::new(10.0, 6.0, 2.0));
        let (u, i, c) = (run(&a, &b, BoolOp::Union), run(&a, &b, BoolOp::Intersect), run(&a, &b, BoolOp::Cut));
        let (va, vb) = (vol(&a), vol(&b));
        let (vu, vi, vc) = (vol(&u), vol(&i), vol(&c));
        assert!(vi > 1.0, "angle {angle}: boxes should overlap");
        assert!(close(vu + vi, va + vb, 1e-10), "angle {angle}: {vu} + {vi} vs {va} + {vb}");
        assert!(close(vc, va - vi, 1e-10), "angle {angle}: {vc} vs {va} - {vi}");
    }
}

/// Cylinders and spheres go through the quadric pipeline (`boolean_quadric.rs`, tested in
/// `tests/curved_booleans.rs`); other curved faces (cone, torus) are still `Unsupported`.
#[test]
fn curved_faces_are_unsupported() {
    let a = cube([0.0; 3], 20.0);
    let cyl = make_cylinder(Frame::from_origin_z(P3::new(10.0, 10.0, -5.0), V3::z()), 5.0, 30.0).unwrap();
    let holed = run(&a, &cyl, BoolOp::Cut);
    assert!(close(vol(&holed), 8000.0 - std::f64::consts::PI * 25.0 * 20.0, 1e-13));
    let cone = cad_geom::primitives::make_cone(Frame::from_origin_z(P3::new(10.0, 10.0, -5.0), V3::z()), 5.0, 2.0, 30.0).unwrap();
    let torus = cad_geom::primitives::make_torus(Frame::from_origin_z(P3::new(10.0, 10.0, 10.0), V3::z()), 6.0, 2.0).unwrap();
    for t in [&cone, &torus] {
        for (x, y) in [(&a, t), (t, &a)] {
            match boolean(x, y, BoolOp::Cut, &asm()) {
                Err(GeomError::Unsupported(m)) => assert!(m.contains("cone") || m.contains("torus"), "{m}"),
                r => panic!("expected Unsupported, got {r:?}"),
            }
        }
    }
}

/// mass.rs integrates plane faces with inner loops exactly (Green's theorem over every loop).
#[test]
fn mass_of_faces_with_inner_loops() {
    let ring = run(&cube([0.0; 3], 20.0), &bx([5.0, 5.0, 0.0], [10.0, 10.0, 20.0]), BoolOp::Cut);
    let holed: Vec<FaceId> = ring.face_ids().filter(|&f| ring.face(f).loops.len() == 2).collect();
    assert_eq!(holed.len(), 2);
    for f in holed {
        assert!(close(face_area(&ring, f).unwrap(), 400.0 - 100.0, 1e-13));
    }
    let m = solid_properties(&ring, SolidId(0)).unwrap();
    assert!(close(m.volume, 6000.0, 1e-13) && close(m.area, 3000.0, 1e-13));
    assert!((m.centroid - P3::new(10.0, 10.0, 10.0)).norm() < 1e-12);
    // I_zz of a square tube: (a⁴ − b⁴)·h/6 with a = 20, b = 10, h = 20
    assert!(close(m.inertia[2][2], (160000.0 - 10000.0) * 20.0 / 6.0, 1e-12), "{:?}", m.inertia);
}

/// Regression: pieces are tested strictly (no tolerance) against the face they came from and
/// against coplanar faces of the other solid. Before the fix a piece narrower than 2·tol was
/// dropped (its sample point lies within tol of an edge; the −2e-5 overlap in
/// `near_coincident_faces` failed with an open shell), and a strip 1.5e-5 wide outside a
/// coplanar face would have been taken as "on" it.
#[test]
fn thin_pieces_are_classified_strictly() {
    let a = cube([0.0; 3], 20.0);
    // z faces coplanar; B spans A in x, so A's z faces keep an isolated strip y < 1.5e-5 outside
    // B's z faces, whose sample point lies 7.5e-6 (< tol) from B's edge
    let b = bx([-1.0, 1.5e-5, 0.0], [22.0, 20.0 - 1.5e-5, 20.0]);
    let i = run(&a, &b, BoolOp::Intersect);
    assert_eq!((i.solids.len(), fev(&i)), (1, [6, 12, 8]));
    assert!(close(vol(&i), 20.0 * (20.0 - 1.5e-5) * 20.0, 1e-12), "{}", vol(&i));
    let u = run(&a, &b, BoolOp::Union);
    assert!(close(vol(&u), 22.0 * (20.0 - 1.5e-5) * 20.0 + 20.0 * 1.5e-5 * 20.0, 1e-12), "{}", vol(&u));
    // a sliver 2e-5 thick (twice the tolerance) survives as a solid
    let s = run(&a, &cube([20.0 - 2e-5, 0.0, 0.0], 20.0), BoolOp::Intersect);
    assert_eq!(fev(&s), [6, 12, 8]);
}

/// The catalogue's boundary value: an overlap written as 1e-5 mm (`bool.union.near_tangent_cylinder`
/// places its cylinder at x = 29.99999). In f64, 20 − 19.99999 = 9.9999999996e-6 ≤ 1e-5, so, as in
/// ASM, the overlap is contact: one merged box, an empty intersection, the target uncut.
#[test]
fn asm_boundary_overlap_is_contact() {
    assert!(20.0 - 19.99999 <= Tolerance::asm().linear);
    let (a, b) = (cube([0.0; 3], 20.0), cube([19.99999, 0.0, 0.0], 20.0));
    let u = run(&a, &b, BoolOp::Union);
    assert_eq!((u.solids.len(), fev(&u)), (1, [6, 12, 8]));
    assert!(close(vol(&u), 400.0 * 39.99999, 1e-12));
    assert_eq!(run(&a, &b, BoolOp::Intersect).solids.len(), 0);
    let c = run(&a, &b, BoolOp::Cut);
    assert_eq!(fev(&c), [6, 12, 8]);
    assert!(close(vol(&c), 8000.0, 1e-12));
}
