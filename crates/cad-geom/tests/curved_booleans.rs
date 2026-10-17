//! Booleans of solids bounded by planes, cylinders and spheres (the quadric pipeline,
//! `boolean_quadric.rs`): containment and disjointness, through and partial cuts, a sphere cut by
//! a box corner, repeated cuts, line contact on both sides of ASM's tolerance, and exact volumes
//! against closed forms written out here (circle segment, spherical cap, ball octant).

use cad_geom::mass::solid_properties;
use cad_geom::primitives::{make_box, make_cone, make_cylinder, make_sphere, make_torus};
use cad_geom::{boolean, validate, BoolOp, Brep, Frame, GeomError, Tolerance, P3, V3};
use nalgebra::Rotation3;
use std::f64::consts::PI;

fn bx(c: [f64; 3], s: [f64; 3]) -> Brep {
    make_box(P3::from(c), s).unwrap()
}

fn cyl(base: [f64; 3], r: f64, h: f64) -> Brep {
    make_cylinder(Frame::from_origin_z(P3::from(base), V3::z()), r, h).unwrap()
}

fn sph(c: [f64; 3], r: f64) -> Brep {
    make_sphere(P3::from(c), r).unwrap()
}

/// Boolean at ASM's tolerance; the result must validate at `vtol`.
fn run_at(a: &Brep, b: &Brep, op: BoolOp, vtol: Tolerance) -> Brep {
    let r = boolean(a, b, op, &Tolerance::asm()).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    let rep = validate(&r, &vtol);
    assert!(rep.is_valid(), "{op:?}: {:?}", rep.issues);
    r
}

/// Exact inputs: the result validates at the default (1e-7 mm) tolerance.
fn run(a: &Brep, b: &Brep, op: BoolOp) -> Brep {
    run_at(a, b, op, Tolerance::default())
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

fn assert_vol(b: &Brep, want: f64, rel: f64) {
    let v = vol(b);
    assert!(close(v, want, rel), "volume {v} vs {want} (rel {:.2e})", (v - want).abs() / want);
}

/// Area of the circular segment cut off a disc of radius `r` by a chord at distance `d` from the
/// centre (the smaller side for d ≥ 0): r²·θ − d·w with half-chord w = √((r − d)(r + d)) and
/// θ = atan2(w, d) (acos(d/r) loses ~1e-5 relative near tangency, where d/r → 1).
fn segment(r: f64, d: f64) -> f64 {
    let w = ((r - d) * (r + d)).sqrt();
    r * r * w.atan2(d) - d * w
}

/// Volume of a spherical cap of height `h` on a ball of radius `r`: πh²(3r − h)/3.
fn cap(r: f64, h: f64) -> f64 {
    PI * h * h * (3.0 - h / r) * r / 3.0
}

fn ball(r: f64) -> f64 {
    4.0 / 3.0 * PI * r.powi(3)
}

#[test]
fn closed_forms_are_consistent() {
    // a chord through the centre halves the disc; a cap of height 2r is the ball
    assert!(close(segment(3.0, 0.0), PI * 9.0 / 2.0, 1e-15));
    assert!(close(cap(2.0, 4.0), ball(2.0), 1e-15));
    assert!(close(cap(2.0, 2.0), ball(2.0) / 2.0, 1e-15));
}

#[test]
fn cylinder_inside_box() {
    let (b, c) = (bx([0.0; 3], [40.0; 3]), cyl([20.0, 20.0, 10.0], 5.0, 20.0));
    let vc = PI * 25.0 * 20.0;
    let u = run(&b, &c, BoolOp::Union);
    assert_eq!((u.solids.len(), fev(&u)), (1, [6, 12, 8]));
    assert_vol(&u, 64000.0, 1e-13);
    let i = run(&b, &c, BoolOp::Intersect);
    assert_eq!((i.solids.len(), fev(&i)), (1, [3, 2, 2]));
    assert_vol(&i, vc, 1e-13);
    // the cylinder becomes a void: one solid, two shells
    let k = run(&b, &c, BoolOp::Cut);
    assert_eq!((k.solids.len(), k.shells.len(), fev(&k)), (1, 2, [9, 14, 10]));
    assert_vol(&k, 64000.0 - vc, 1e-13);
    let e = run(&c, &b, BoolOp::Cut);
    assert_eq!(e.solids.len(), 0);
}

#[test]
fn cylinder_outside_box() {
    let (b, c) = (bx([0.0; 3], [10.0; 3]), cyl([30.0, 5.0, 0.0], 4.0, 10.0));
    let u = run(&b, &c, BoolOp::Union);
    assert_eq!((u.solids.len(), fev(&u)), (2, [9, 14, 10]));
    assert_vol(&u, 1000.0 + PI * 160.0, 1e-13);
    let k = run(&b, &c, BoolOp::Cut);
    assert_eq!((k.solids.len(), fev(&k)), (1, [6, 12, 8]));
    assert_eq!(run(&b, &c, BoolOp::Intersect).solids.len(), 0);
}

/// A through hole away from the centre, and a cylinder crossing the box's x = 0 face (a notch):
/// volumes from the circle-segment formula.
#[test]
fn off_centre_cuts() {
    let b = bx([0.0; 3], [50.0, 30.0, 10.0]);
    let h = run(&b, &cyl([8.0, 21.0, -5.0], 6.0, 20.0), BoolOp::Cut);
    assert_eq!((h.solids.len(), fev(&h), h.loops.len()), (1, [7, 14, 10], 10));
    assert_vol(&h, 15000.0 - 360.0 * PI, 1e-13);
    // notch: cylinder axis at x = 2, r = 6, so x ≥ 0 keeps the disc minus the segment beyond x < 0
    let n = run(&b, &cyl([2.0, 15.0, -5.0], 6.0, 20.0), BoolOp::Cut);
    let inside = PI * 36.0 - segment(6.0, 2.0);
    assert_eq!((n.solids.len(), fev(&n)), (1, [8, 18, 12]));
    assert_vol(&n, 15000.0 - 10.0 * inside, 1e-12);
    let types: Vec<&str> = n.edges.iter().map(|e| e.curve.as_ref().unwrap().type_name()).collect();
    assert_eq!(types.iter().filter(|t| **t == "circle").count(), 2);
    // the material removed is the intersection
    let i = run(&b, &cyl([2.0, 15.0, -5.0], 6.0, 20.0), BoolOp::Intersect);
    assert_eq!((i.solids.len(), fev(&i)), (1, [4, 6, 4]));
    assert_vol(&i, 10.0 * inside, 1e-12);
    // union: box plus the parts of the cylinder outside it
    let u = run(&b, &cyl([2.0, 15.0, -5.0], 6.0, 20.0), BoolOp::Union);
    assert_eq!(u.solids.len(), 1);
    assert_vol(&u, 15000.0 + PI * 36.0 * 20.0 - 10.0 * inside, 1e-12);
}

/// A box whose corner sits at the sphere's centre (rotated off the axes) removes exactly one
/// octant of the ball; an off-centre corner is checked by inclusion–exclusion.
#[test]
fn sphere_cut_by_box_corner() {
    let r = 10.0;
    let s = sph([0.0; 3], r);
    let rot = Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(V3::new(1.0, 2.0, 3.0)), 0.7);
    let corner = bx([0.0; 3], [30.0; 3]).transformed(&rot, &V3::zeros());
    let k = run(&s, &corner, BoolOp::Cut);
    assert_eq!((k.solids.len(), fev(&k)), (1, [4, 6, 4]));
    assert_vol(&k, ball(r) * 7.0 / 8.0, 1e-12);
    let i = run(&s, &corner, BoolOp::Intersect);
    assert_eq!((i.solids.len(), fev(&i)), (1, [4, 6, 4]));
    assert_vol(&i, ball(r) / 8.0, 1e-12);
    let u = run(&s, &corner, BoolOp::Union);
    assert_eq!(u.solids.len(), 1);
    assert_vol(&u, 27000.0 + ball(r) * 7.0 / 8.0, 1e-12);
    // off-centre corner, axis-aligned: V(A ∪ B) + V(A ∩ B) = V(A) + V(B), V(A − B) = V(A) − V(A ∩ B)
    let b = bx([3.0, 2.0, 1.0], [20.0; 3]);
    let (vu, vi, vc) = (vol(&run(&s, &b, BoolOp::Union)), vol(&run(&s, &b, BoolOp::Intersect)), vol(&run(&s, &b, BoolOp::Cut)));
    assert!(close(vu + vi, ball(r) + 8000.0, 1e-12), "{vu} + {vi}");
    assert!(close(vc, ball(r) - vi, 1e-12), "{vc} vs {}", ball(r) - vi);
    // a single box face cutting the sphere leaves a spherical cap
    let slab = bx([-20.0, -20.0, 6.0], [40.0, 40.0, 40.0]);
    let c = run(&s, &slab, BoolOp::Intersect);
    assert_eq!((c.solids.len(), fev(&c)), (1, [2, 1, 1]));
    assert_vol(&c, cap(r, 4.0), 1e-12);
    let rest = run(&s, &slab, BoolOp::Cut);
    assert_vol(&rest, ball(r) - cap(r, 4.0), 1e-12);
}

#[test]
fn sphere_contains_box_and_box_contains_sphere() {
    let (s, b) = (sph([1.0, 2.0, 3.0], 20.0), bx([-4.0, -3.0, -2.0], [10.0; 3]));
    assert_vol(&run(&s, &b, BoolOp::Union), ball(20.0), 1e-13);
    let i = run(&s, &b, BoolOp::Intersect);
    assert_eq!(fev(&i), [6, 12, 8]);
    assert_vol(&i, 1000.0, 1e-13);
    let k = run(&s, &b, BoolOp::Cut);
    assert_eq!((k.solids.len(), k.shells.len(), fev(&k)), (1, 2, [7, 12, 8]));
    assert_vol(&k, ball(20.0) - 1000.0, 1e-13);
    assert_eq!(run(&b, &s, BoolOp::Cut).solids.len(), 0);

    let (big, small) = (bx([0.0; 3], [40.0; 3]), sph([20.0, 18.0, 21.0], 5.0));
    let u = run(&big, &small, BoolOp::Union);
    assert_eq!(fev(&u), [6, 12, 8]);
    let i = run(&big, &small, BoolOp::Intersect);
    assert_eq!(fev(&i), [1, 0, 0]);
    assert_vol(&i, ball(5.0), 1e-13);
    let k = run(&big, &small, BoolOp::Cut);
    assert_eq!((k.solids.len(), k.shells.len(), fev(&k)), (1, 2, [7, 12, 8]));
    assert_vol(&k, 64000.0 - ball(5.0), 1e-13);
}

/// Repeated booleans on curved results: a second hole, and the same hole cut again (its wall
/// lies on the tool's cylinder: coincident curved faces).
#[test]
fn hole_cut_twice() {
    let b = bx([0.0; 3], [50.0, 30.0, 10.0]);
    let tool = cyl([25.0, 15.0, -5.0], 6.0, 20.0);
    let once = run(&b, &tool, BoolOp::Cut);
    let again = run(&once, &tool, BoolOp::Cut);
    assert_eq!((fev(&again), again.loops.len()), ([7, 14, 10], 10));
    assert_vol(&again, 15000.0 - 360.0 * PI, 1e-13);
    let two = run(&once, &cyl([10.0, 10.0, -1.0], 3.0, 12.0), BoolOp::Cut);
    assert_eq!((fev(&two), two.loops.len()), ([8, 16, 12], 14));
    assert_vol(&two, 15000.0 - 360.0 * PI - 90.0 * PI, 1e-13);
    // a hole overlapping the first one: walls meet along lines parallel to the axis
    let three = run(&once, &cyl([31.0, 15.0, -5.0], 3.0, 20.0), BoolOp::Cut);
    assert_eq!(three.solids.len(), 1);
    // the lens of two discs (radii 6 and 3, centres 6 apart) is two circle segments
    let (r1, r2, d) = (6.0f64, 3.0f64, 6.0f64);
    let a = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
    let lens = segment(r1, a) + segment(r2, d - a);
    assert_vol(&three, 15000.0 - 360.0 * PI - 10.0 * (PI * r2 * r2 - lens), 1e-12);
    // filling the hole back restores the box volume
    let filled = run(&once, &tool, BoolOp::Union);
    assert_eq!(filled.solids.len(), 1);
    assert_vol(&filled, 15000.0 + PI * 36.0 * 10.0, 1e-13);
}

/// ASM's line contact on both sides of its 1e-5 mm tolerance: box 20³ and a cylinder r 10 whose
/// axis is at x = 30 ∓ offset (overlap > 0, gap < 0). Within the tolerance the result is ASM's
/// one lump with a 4-coedge edge (10/17/10); a 2e-5 overlap is a real intersection (the sliver,
/// a circle segment × 20, is removed); a 2e-5 gap leaves two lumps.
#[test]
fn tangent_contact_tolerance_boundary() {
    let b = bx([0.0; 3], [20.0; 3]);
    let exact = 8000.0 + PI * 100.0 * 20.0;
    for off in [0.0, 5e-6, 1e-5, -5e-6, -1e-5] {
        let c = cyl([30.0 - off, 10.0, 0.0], 10.0, 20.0);
        let u = run_at(&b, &c, BoolOp::Union, Tolerance::asm());
        assert_eq!((u.solids.len(), u.shells.len(), fev(&u), u.loops.len()), (1, 1, [10, 17, 10], 10), "offset {off}");
        assert_eq!(u.nonmanifold_edges.len(), 1, "offset {off}");
        assert_eq!(u.edge_coedges(u.nonmanifold_edges[0]).len(), 4);
        assert_vol(&u, exact, 1e-7);
        // contact only: intersect is empty, cut leaves the box
        assert_eq!(run_at(&b, &c, BoolOp::Intersect, Tolerance::asm()).solids.len(), 0, "offset {off}");
        let k = run_at(&b, &c, BoolOp::Cut, Tolerance::asm());
        assert_vol(&k, 8000.0, 1e-7);
        // the contact line splits the box face only while the cylinder is there: re-merged
        assert_eq!((k.solids.len(), fev(&k), k.nonmanifold_edges.len()), (1, [6, 12, 8], 0), "offset {off}");
        let kc = run_at(&c, &b, BoolOp::Cut, Tolerance::asm());
        assert_eq!((kc.solids.len(), fev(&kc)), (1, [3, 2, 2]), "offset {off}");
        // union is symmetric
        let u2 = run_at(&c, &b, BoolOp::Union, Tolerance::asm());
        assert_eq!((u2.solids.len(), fev(&u2), u2.nonmanifold_edges.len()), (1, [10, 17, 10], 1), "offset {off}");
    }
    // the exact tangent case is exact: it validates at 1e-7 and its volume is the plain sum
    let t = run(&b, &cyl([30.0, 10.0, 0.0], 10.0, 20.0), BoolOp::Union);
    assert_vol(&t, exact, 1e-13);
    // 2e-5 overlap: a real sliver
    let c = cyl([30.0 - 2e-5, 10.0, 0.0], 10.0, 20.0);
    let u = run(&b, &c, BoolOp::Union);
    assert_eq!((u.solids.len(), u.nonmanifold_edges.len()), (1, 0));
    assert_eq!(fev(&u), [8, 18, 12]);
    // the axis sits at fl(30 − 2e-5); the chord is the box face x = 20
    let sliver = 20.0 * segment(10.0, (30.0 - 2e-5) - 20.0);
    assert!(sliver > 1e-6);
    assert_vol(&u, exact - sliver, 1e-12);
    let i = run(&b, &c, BoolOp::Intersect);
    assert_eq!((i.solids.len(), fev(&i)), (1, [4, 6, 4]));
    assert!(close(vol(&i), sliver, 1e-6), "{} vs {sliver}", vol(&i));
    // 2e-5 gap: two lumps
    let g = run(&b, &cyl([30.0 + 2e-5, 10.0, 0.0], 10.0, 20.0), BoolOp::Union);
    assert_eq!((g.solids.len(), fev(&g)), (2, [9, 14, 10]));
}

/// Sphere ∩ box faces through the sphere's equator and caps (`bool.intersect.box_sphere` shape,
/// other sizes): every box face cuts a cap off the ball.
#[test]
fn ball_minus_six_caps() {
    for (half, r) in [(10.0, 12.0), (5.0, 7.0)] {
        let i = run(&bx([-half; 3], [2.0 * half; 3]), &sph([0.0; 3], r), BoolOp::Intersect);
        assert_eq!((fev(&i), i.loops.len()), ([7, 6, 6], 12));
        assert_vol(&i, ball(r) - 6.0 * cap(r, r - half), 1e-12);
    }
}

#[test]
fn unsupported_surfaces_are_reported() {
    let b = bx([0.0; 3], [20.0; 3]);
    let cone = make_cone(Frame::from_origin_z(P3::new(10.0, 10.0, -5.0), V3::z()), 5.0, 2.0, 30.0).unwrap();
    let torus = make_torus(Frame::from_origin_z(P3::new(10.0, 10.0, 10.0), V3::z()), 6.0, 2.0).unwrap();
    for t in [&cone, &torus] {
        for (x, y) in [(&b, t), (t, &b)] {
            match boolean(x, y, BoolOp::Cut, &Tolerance::asm()) {
                Err(GeomError::Unsupported(m)) => assert!(m.contains("cone") || m.contains("torus"), "{m}"),
                r => panic!("expected Unsupported, got {r:?}"),
            }
        }
    }
    // an oblique plane section of a cylinder (an ellipse) is not supported either
    let tilted = make_cylinder(Frame::from_origin_z(P3::new(10.0, 10.0, -5.0), V3::new(0.3, 0.0, 1.0)), 3.0, 30.0).unwrap();
    assert!(matches!(boolean(&b, &tilted, BoolOp::Cut, &Tolerance::asm()), Err(GeomError::Unsupported(_))));
}

/// Quadric–quadric pairs: two cylinders with parallel axes (lines), two spheres (a circle) and a
/// sphere drilled along a diameter (coaxial cylinder–sphere: two circles; the napkin ring of
/// height L has volume πL³/6 whatever the radii).
#[test]
fn quadric_pairs() {
    let (r, d, h) = (5.0f64, 6.0f64, 10.0f64);
    let (a, b) = (cyl([0.0; 3], r, h), cyl([d, 0.0, 0.0], r, h));
    let lens = 2.0 * segment(r, d / 2.0);
    let u = run(&a, &b, BoolOp::Union);
    assert_eq!((u.solids.len(), fev(&u)), (1, [4, 6, 4]));
    assert_vol(&u, h * (2.0 * PI * r * r - lens), 1e-12);
    let i = run(&a, &b, BoolOp::Intersect);
    assert_eq!((i.solids.len(), fev(&i)), (1, [4, 6, 4]));
    assert_vol(&i, h * lens, 1e-12);
    // a cylinder shorter than the other and inside its height: caps cut the other's wall
    let k = run(&a, &cyl([d, 0.0, 2.0], r, 6.0), BoolOp::Cut);
    assert_vol(&k, h * PI * r * r - 6.0 * lens, 1e-12);

    let (s1, s2) = (sph([0.0; 3], 10.0), sph([12.0, 0.0, 0.0], 8.0));
    // caps of the lens: plane of the intersection circle at x = a from the first centre
    let a1 = (144.0 + 100.0 - 64.0) / 24.0;
    let lens = cap(10.0, 10.0 - a1) + cap(8.0, 8.0 - (12.0 - a1));
    let u = run(&s1, &s2, BoolOp::Union);
    assert_eq!((u.solids.len(), fev(&u)), (1, [2, 1, 1]));
    assert_vol(&u, ball(10.0) + ball(8.0) - lens, 1e-12);
    assert_vol(&run(&s1, &s2, BoolOp::Intersect), lens, 1e-12);
    assert_vol(&run(&s1, &s2, BoolOp::Cut), ball(10.0) - lens, 1e-12);

    let ring = run(&sph([0.0; 3], 10.0), &cyl([0.0, 0.0, -20.0], 6.0, 40.0), BoolOp::Cut);
    let l = 2.0 * (100.0f64 - 36.0).sqrt();
    assert_eq!((ring.solids.len(), fev(&ring)), (1, [2, 2, 2]));
    assert_vol(&ring, PI * l.powi(3) / 6.0, 1e-12);
}

/// The recorded shapes moved rigidly (off the coordinate axes) and scaled: same topology,
/// volumes scaled exactly.
#[test]
fn rigid_motion_and_scale() {
    let rot = Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(V3::new(-0.4, 0.7, 0.2)), 1.1);
    let tr = V3::new(3.0, -7.0, 11.0);
    let hole = |s: f64| {
        let b = bx([0.0; 3], [50.0 * s, 30.0 * s, 10.0 * s]).transformed(&rot, &tr);
        let c = cyl([25.0 * s, 15.0 * s, -5.0 * s], 6.0 * s, 20.0 * s).transformed(&rot, &tr);
        run_at(&b, &c, BoolOp::Cut, Tolerance::asm())
    };
    for s in [1.0, 1e-2, 1e3] {
        let h = hole(s);
        assert_eq!((fev(&h), h.loops.len()), ([7, 14, 10], 10), "scale {s}");
        assert_vol(&h, (15000.0 - 360.0 * PI) * s.powi(3), 1e-11);
    }
    let b = bx([-10.0; 3], [20.0; 3]).transformed(&rot, &tr);
    let s = make_sphere(P3::from(tr), 12.0).unwrap();
    let i = run(&b, &s, BoolOp::Intersect);
    assert_eq!((fev(&i), i.loops.len()), ([7, 6, 6], 12));
    assert_vol(&i, 2032.0 * PI, 1e-12);
    let t = run_at(&bx([0.0; 3], [20.0; 3]).transformed(&rot, &tr), &cyl([30.0, 10.0, 0.0], 10.0, 20.0).transformed(&rot, &tr), BoolOp::Union, Tolerance::asm());
    assert_eq!((t.solids.len(), fev(&t), t.nonmanifold_edges.len()), (1, [10, 17, 10], 1));
    assert_vol(&t, 8000.0 + 2000.0 * PI, 1e-9);
}
