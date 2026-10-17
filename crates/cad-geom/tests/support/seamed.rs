//! Test fixtures: the OCCT-style *seamed* primitives (the convention cad-geom used before it
//! switched to ASM's seamless one). Built with the public builder API; used to check that the
//! integrators and validation handle both conventions. Not public API.
#![allow(dead_code)]

use cad_geom::{Brep, Curve, Frame, GResult, Surface, P3, V3};
use std::f64::consts::{FRAC_PI_2, TAU};

/// Cylinder, seamed: F3 E3 V2 L3 (2 circles + seam line used twice by the side face).
pub fn cylinder(frame: Frame, r: f64, h: f64) -> GResult<Brep> {
    Ok(revolved(frame, r, r, h, Surface::Cylinder { frame, radius: r }))
}

/// Cone/frustum, seamed: frustum F3 E3 V2 L3; pointed cone F2 E3 V2 L2 (degenerate apex edge in
/// the side face's single loop).
pub fn cone(frame: Frame, r0: f64, r1: f64, h: f64) -> GResult<Brep> {
    Ok(revolved(frame, r0, r1, h, Surface::Cone { frame, radius: r0, half_angle: ((r1 - r0) / h).atan() }))
}
/// Seamed cylinder / cone / frustum (a radius of 0 gives a degenerate apex edge in the loop).
pub fn revolved(frame: Frame, r0: f64, r1: f64, h: f64, lateral: Surface) -> Brep {
    let mut b = Brep::new();
    let solid = b.add_solid();
    let shell = b.add_shell(Some(solid));
    let top = Frame { origin: frame.origin + frame.z * h, ..frame };
    let v0 = b.add_vertex(frame.origin + frame.x * r0);
    let v1 = b.add_vertex(top.origin + frame.x * r1);
    let ring = |b: &mut Brep, f: Frame, r: f64, v| {
        if r > 0.0 { b.add_edge(Some(Curve::Circle { frame: f, radius: r }), 0.0, TAU, v, v) } else { b.add_degenerate_edge(v, 0.0, TAU) }
    };
    let c0 = ring(&mut b, frame, r0, v0);
    let c1 = ring(&mut b, top, r1, v1);
    let seam = b.add_line_edge(v0, v1);
    if r0 > 0.0 {
        let f = b.add_face(shell, Surface::Plane { frame }, true);
        b.add_loop(f, &[(c0, true)]);
    }
    let f = b.add_face(shell, lateral, false);
    b.add_loop(f, &[(c0, false), (seam, false), (c1, true), (seam, true)]);
    if r1 > 0.0 {
        let f = b.add_face(shell, Surface::Plane { frame: top }, false);
        b.add_loop(f, &[(c1, false)]);
    }
    b
}

/// Sphere, seamed: F1 E3 V2 L1 (meridian seam ×2 + 2 degenerate pole edges).
pub fn sphere(center: P3, r: f64) -> GResult<Brep> {
    let frame = Frame { origin: center, ..Frame::world() };
    let mut b = Brep::new();
    let solid = b.add_solid();
    let shell = b.add_shell(Some(solid));
    let s = b.add_vertex(center - V3::z() * r);
    let n = b.add_vertex(center + V3::z() * r);
    let meridian = Frame::from_origin_z_x(center, -V3::y(), V3::x());
    let seam = b.add_edge(Some(Curve::Circle { frame: meridian, radius: r }), -FRAC_PI_2, FRAC_PI_2, s, n);
    let ds = b.add_degenerate_edge(s, 0.0, TAU);
    let dn = b.add_degenerate_edge(n, 0.0, TAU);
    let f = b.add_face(shell, Surface::Sphere { frame, radius: r }, false);
    b.add_loop(f, &[(ds, false), (seam, false), (dn, true), (seam, true)]);
    Ok(b)
}

/// Torus, seamed: F1 E2 V1 L1 (meridian and equator seams, each used twice).
pub fn torus(frame: Frame, major: f64, minor: f64) -> GResult<Brep> {
    let mut b = Brep::new();
    let solid = b.add_solid();
    let shell = b.add_shell(Some(solid));
    let v = b.add_vertex(frame.origin + frame.x * (major + minor));
    let meridian = Frame::from_origin_z_x(frame.origin + frame.x * major, -frame.y, frame.x);
    let a = b.add_edge(Some(Curve::Circle { frame: meridian, radius: minor }), 0.0, TAU, v, v);
    let e = b.add_edge(Some(Curve::Circle { frame, radius: major + minor }), 0.0, TAU, v, v);
    let f = b.add_face(shell, Surface::Torus { frame, major, minor }, false);
    b.add_loop(f, &[(e, false), (a, false), (e, true), (a, true)]);
    Ok(b)
}
