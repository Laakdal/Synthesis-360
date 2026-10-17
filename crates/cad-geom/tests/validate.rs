//! Validation: deliberately broken B-reps each produce the expected `Issue`, naming the entity.

use cad_geom::primitives::*;
use cad_geom::{validate, Brep, CoedgeId, EdgeId, FaceId, Frame, Issue, SolidId, Surface, Tolerance, P3, V3};

fn tol() -> Tolerance {
    Tolerance::default()
}

fn issues(b: &Brep) -> Vec<Issue> {
    validate(b, &tol()).issues
}

fn has(is: &[Issue], pred: impl Fn(&Issue) -> bool) -> bool {
    is.iter().any(pred)
}

fn bx() -> Brep {
    make_box(P3::origin(), [50.0, 30.0, 10.0]).unwrap()
}

/// Removes a coedge from its loop and from the arena (swap-remove, relinking the moved one).
fn remove_coedge(b: &mut Brep, c: CoedgeId) {
    let owner = b.coedge(c).owner;
    b.loops[owner.idx()].coedges.retain(|&x| x != c);
    let last = CoedgeId(b.coedges.len() as u32 - 1);
    b.coedges.swap_remove(c.idx());
    if c != last {
        let lp = b.coedges[c.idx()].owner;
        for x in &mut b.loops[lp.idx()].coedges {
            if *x == last {
                *x = c;
            }
        }
    }
}

#[test]
fn removed_coedge() {
    let mut b = bx();
    let c = b.lp(b.face(FaceId(0)).loops[0]).coedges[1];
    let e = b.coedge(c).edge;
    remove_coedge(&mut b, c);
    let is = issues(&b);
    assert!(has(&is, |i| matches!(i, Issue::OpenLoop(m) if m.contains("loop L0"))), "{is:?}");
    let tag = format!("edge E{} ", e.0);
    assert!(has(&is, |i| matches!(i, Issue::NonManifoldEdge(m) if m.contains(&tag) && m.contains("1 use"))), "{is:?}");
}

#[test]
fn flipped_coedge() {
    let mut b = bx();
    let c = b.lp(b.face(FaceId(2)).loops[0]).coedges[0];
    b.coedges[c.idx()].reversed ^= true;
    let e = b.coedge(c).edge;
    let is = issues(&b);
    let tag = format!("edge E{} ", e.0);
    assert!(has(&is, |i| matches!(i, Issue::InconsistentOrientation(m) if m.contains(&tag))), "{is:?}");
    assert!(has(&is, |i| matches!(i, Issue::OpenLoop(m) if m.contains("loop L2"))), "{is:?}");
}

#[test]
fn moved_vertex() {
    let mut b = bx();
    b.vertices[5].point += V3::new(0.0, 0.0, 0.01);
    let is = issues(&b);
    assert!(!is.is_empty());
    assert!(is.iter().all(|i| matches!(i, Issue::GeometryMismatch(_))), "{is:?}");
    assert!(has(&is, |i| matches!(i, Issue::GeometryMismatch(m) if m.contains("vertex V5"))), "{is:?}");
    // well within tolerance is fine
    let mut b = bx();
    b.vertices[5].point += V3::new(0.0, 0.0, 1e-9);
    assert!(issues(&b).is_empty());
}

#[test]
fn dangling_ids() {
    let mut b = bx();
    b.coedges[3].edge = EdgeId(999);
    let is = issues(&b);
    assert!(has(&is, |i| matches!(i, Issue::DanglingReference(m) if m.contains("C3") && m.contains("E999"))), "{is:?}");

    let mut b = bx();
    b.edges[4].end = cad_geom::VertexId(77);
    assert!(has(&issues(&b), |i| matches!(i, Issue::DanglingReference(m) if m.contains("E4") && m.contains("V77"))));

    let mut b = bx();
    b.faces[1].shell = cad_geom::ShellId(5);
    assert!(has(&issues(&b), |i| matches!(i, Issue::DanglingReference(m) if m.contains("F1") && m.contains("Sh5"))));

    // back-link disagreement: a coedge listed by a loop that is not its owner
    let mut b = bx();
    b.coedges[0].owner = cad_geom::LoopId(3);
    assert!(has(&issues(&b), |i| matches!(i, Issue::DanglingReference(m) if m.contains("C0"))));
}

#[test]
fn flipped_face_flag() {
    // Toggling `reversed` without reversing the loop makes the loop run clockwise about the
    // claimed outward normal.
    let mut b = bx();
    b.faces[3].reversed ^= true;
    let is = issues(&b);
    assert!(has(&is, |i| matches!(i, Issue::InconsistentOrientation(m) if m.contains("face F3"))), "{is:?}");
}

#[test]
fn inside_out_solid() {
    // Consistently reversed everywhere: topology fine, but the volume is negative.
    let mut b = make_sphere(P3::origin(), 3.0).unwrap();
    for f in &mut b.faces {
        f.reversed ^= true;
    }
    for l in &mut b.loops {
        l.coedges.reverse();
    }
    for c in &mut b.coedges {
        c.reversed ^= true;
    }
    let is = issues(&b);
    assert_eq!(is.len(), 1, "{is:?}");
    assert!(matches!(&is[0], Issue::InconsistentOrientation(m) if m.contains("solid S0")), "{is:?}");
}

#[test]
fn euler_violation() {
    // Move one box face into a shell of its own: both shells get an odd Euler characteristic.
    let mut b = bx();
    let sh = b.add_shell(None);
    b.shells[0].faces.retain(|&f| f != FaceId(5));
    b.faces[5].shell = sh;
    b.shells[sh.idx()].faces.push(FaceId(5));
    let is = issues(&b);
    assert!(has(&is, |i| matches!(i, Issue::EulerPoincare(m) if m.contains("Sh0"))), "{is:?}");
    assert!(has(&is, |i| matches!(i, Issue::NonManifoldEdge(m) if m.contains("Sh0"))), "{is:?}");
}

#[test]
fn loopless_periodic_faces() {
    // A sphere and a torus as single faces with no loops: genus 0 and 1, exact volumes.
    for (surf, genus, vol) in [
        (Surface::Sphere { frame: Frame::world(), radius: 2.0 }, 0, 4.0 / 3.0 * std::f64::consts::PI * 8.0),
        (Surface::Torus { frame: Frame::world(), major: 3.0, minor: 1.0 }, 1, 2.0 * std::f64::consts::PI.powi(2) * 3.0),
    ] {
        let mut b = Brep::new();
        let s = b.add_solid();
        let sh = b.add_shell(Some(s));
        b.add_face(sh, surf, false);
        let r = validate(&b, &tol());
        assert!(r.is_valid(), "{:?}", r.issues);
        assert_eq!(r.genus, genus);
        let mp = cad_geom::mass::solid_properties(&b, SolidId(0)).unwrap();
        assert!((mp.volume - vol).abs() <= 1e-9 * vol, "{} vs {vol}", mp.volume);
    }
}

#[test]
fn every_primitive_validates_clean() {
    let f = Frame::from_origin_z(P3::new(1.0, 2.0, 3.0), V3::new(1.0, 1.0, 1.0));
    for b in [
        bx(),
        make_cylinder(f, 4.0, 15.0).unwrap(),
        make_cone(f, 10.0, 0.0, 20.0).unwrap(),
        make_cone(f, 10.0, 5.0, 20.0).unwrap(),
        make_sphere(P3::new(-4.0, 0.0, 1.0), 10.0).unwrap(),
        make_torus(f, 20.0, 5.0).unwrap(),
    ] {
        let r = validate(&b, &tol());
        assert!(r.is_valid(), "{:?}", r.issues);
    }
}

/// ASM's seamless convention, built by hand with the builder API: the Euler–Poincaré check (degenerate edges excluded)
/// gives genus 0 for a seamless cylinder (V2 E2 F3 L4) and a pointed cone (V2 E1+1 F2 L3).
#[test]
fn asm_seamless_shapes_euler() {
    use cad_geom::Curve;
    use std::f64::consts::TAU;
    let (r, h) = (10.0, 20.0);
    let f0 = Frame::world();
    let f1 = Frame { origin: P3::new(0.0, 0.0, h), ..f0 };
    // cylinder: two circles, side face with two loops
    let mut b = Brep::new();
    let s = b.add_solid();
    let sh = b.add_shell(Some(s));
    let v0 = b.add_vertex(P3::new(r, 0.0, 0.0));
    let v1 = b.add_vertex(P3::new(r, 0.0, h));
    let c0 = b.add_edge(Some(Curve::Circle { frame: f0, radius: r }), 0.0, TAU, v0, v0);
    let c1 = b.add_edge(Some(Curve::Circle { frame: f1, radius: r }), 0.0, TAU, v1, v1);
    let bot = b.add_face(sh, Surface::Plane { frame: f0 }, true);
    b.add_loop(bot, &[(c0, true)]);
    let side = b.add_face(sh, Surface::Cylinder { frame: f0, radius: r }, false);
    b.add_loop(side, &[(c0, false)]);
    b.add_loop(side, &[(c1, true)]);
    let top = b.add_face(sh, Surface::Plane { frame: f1 }, false);
    b.add_loop(top, &[(c1, false)]);
    let rep = validate(&b, &tol());
    assert!(rep.is_valid(), "{:?}", rep.issues);
    assert_eq!((rep.faces, rep.edges, rep.vertices, rep.loops, rep.genus), (3, 2, 2, 4, 0));
    // pointed cone: base circle + degenerate apex edge, cone face with two loops
    let mut b = Brep::new();
    let s = b.add_solid();
    let sh = b.add_shell(Some(s));
    let v0 = b.add_vertex(P3::new(r, 0.0, 0.0));
    let apex = b.add_vertex(P3::new(0.0, 0.0, h));
    let c0 = b.add_edge(Some(Curve::Circle { frame: f0, radius: r }), 0.0, TAU, v0, v0);
    let d = b.add_degenerate_edge(apex, 0.0, TAU);
    let bot = b.add_face(sh, Surface::Plane { frame: f0 }, true);
    b.add_loop(bot, &[(c0, true)]);
    let side = b.add_face(sh, Surface::Cone { frame: f0, radius: r, half_angle: (-r / h).atan() }, false);
    b.add_loop(side, &[(c0, false)]);
    b.add_loop(side, &[(d, true)]);
    let rep = validate(&b, &tol());
    assert!(rep.is_valid(), "{:?}", rep.issues);
    assert_eq!((rep.faces, rep.edges, rep.vertices, rep.loops, rep.degenerate_edges, rep.genus), (2, 2, 2, 3, 1, 0));
    // and the integrator closes the seamless loops with a virtual seam: V = πr²h/3 exactly
    let v = cad_geom::mass::solid_properties(&b, SolidId(0)).unwrap().volume;
    let want = std::f64::consts::PI * r * r * h / 3.0;
    assert!((v - want).abs() <= 1e-12 * want, "{v} vs {want}");
}

#[path = "support/seamed.rs"]
mod seamed;

/// The OCCT-style seamed fixtures (seam edges used twice, pole/apex edges inside the loop) are
/// valid too: both conventions pass every check, with OCCT's counts.
#[test]
fn seamed_convention_validates_clean() {
    let f = Frame::from_origin_z(P3::new(1.0, 2.0, 3.0), V3::new(1.0, 1.0, 1.0));
    for (b, counts) in [
        (seamed::cylinder(f, 4.0, 15.0).unwrap(), (3, 3, 2, 3, 0, 0)),
        (seamed::cone(f, 10.0, 0.0, 20.0).unwrap(), (2, 3, 2, 2, 1, 0)),
        (seamed::cone(f, 10.0, 5.0, 20.0).unwrap(), (3, 3, 2, 3, 0, 0)),
        (seamed::sphere(P3::new(-4.0, 0.0, 1.0), 10.0).unwrap(), (1, 3, 2, 1, 2, 0)),
        (seamed::torus(f, 20.0, 5.0).unwrap(), (1, 2, 1, 1, 0, 1)),
    ] {
        let r = validate(&b, &tol());
        assert!(r.is_valid(), "{:?}", r.issues);
        assert_eq!((r.faces, r.edges, r.vertices, r.loops, r.degenerate_edges, r.genus), counts);
    }
}

/// An integration failure is reported, never silently passed: a plane face with no loops
/// (topologically a closed cell, so every topology check passes) cannot be integrated.
#[test]
fn integration_failure_is_unevaluable() {
    let mut b = Brep::new();
    let s = b.add_solid();
    let sh = b.add_shell(Some(s));
    b.add_face(sh, Surface::Plane { frame: Frame::world() }, false);
    let is = issues(&b);
    assert_eq!(is.len(), 1, "{is:?}");
    assert!(matches!(&is[0], Issue::Unevaluable(m) if m.contains("face F0")), "{is:?}");
}

/// The non-manifold exemption is explicit: ASM's line contact (box ∪ tangent cylinder) is one edge
/// with 4 coedges, listed in `Brep::nonmanifold_edges` and accepted; the same model without the
/// flag is reported (4 uses, and the Euler count of a pinched shell), and a flag naming a missing
/// edge is a dangling reference.
#[test]
fn nonmanifold_flag_is_an_explicit_exemption() {
    use cad_geom::{boolean, BoolOp};
    let a = make_box(P3::origin(), [20.0; 3]).unwrap();
    let c = make_cylinder(Frame::from_origin_z(P3::new(30.0, 10.0, 0.0), V3::z()), 10.0, 20.0).unwrap();
    let mut u = boolean(&a, &c, BoolOp::Union, &Tolerance::asm()).unwrap();
    assert_eq!(u.nonmanifold_edges.len(), 1);
    let rep = validate(&u, &Tolerance::default());
    assert!(rep.is_valid(), "{:?}", rep.issues);
    assert_eq!(rep.genus, 0);

    u.nonmanifold_edges.clear();
    let rep = validate(&u, &Tolerance::default());
    assert!(rep.issues.iter().any(|i| matches!(i, Issue::NonManifoldEdge(_))), "{:?}", rep.issues);
    assert!(rep.issues.iter().any(|i| matches!(i, Issue::EulerPoincare(_))), "{:?}", rep.issues);
    // flagging an ordinary two-use edge changes nothing
    let mut b = make_box(P3::origin(), [1.0; 3]).unwrap();
    b.nonmanifold_edges.push(cad_geom::EdgeId(0));
    assert!(validate(&b, &Tolerance::default()).is_valid());
    b.nonmanifold_edges.push(cad_geom::EdgeId(99));
    assert!(validate(&b, &Tolerance::default()).issues.iter().any(|i| matches!(i, Issue::DanglingReference(_))));
}
