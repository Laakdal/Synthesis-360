//! Analytic curves/surfaces: derivatives, lengths, exact NURBS conversion, projection and
//! intersections.

use cad_geom::intersect::*;
use cad_geom::{Curve, Frame, NurbsCurve, NurbsSurface, Surface, Tolerance, P3, V3};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn tilted() -> Frame {
    Frame::from_origin_z_x(P3::new(1.5, -2.0, 0.7), V3::new(0.3, -0.4, 1.0), V3::new(1.0, 0.2, 0.1))
}

fn tol() -> Tolerance {
    Tolerance::default()
}

fn grid(a: f64, b: f64, n: usize) -> Vec<f64> {
    (0..=n).map(|i| a + (b - a) * i as f64 / n as f64).collect()
}

fn surfaces() -> Vec<Surface> {
    let f = tilted();
    vec![
        Surface::Plane { frame: f },
        Surface::Cylinder { frame: f, radius: 3.0 },
        Surface::Cone { frame: f, radius: 2.0, half_angle: 0.4 },
        Surface::Cone { frame: f, radius: 2.0, half_angle: -0.3 },
        Surface::Sphere { frame: f, radius: 4.0 },
        Surface::Torus { frame: f, major: 5.0, minor: 1.5 },
    ]
}

/// Angular difference modulo 2π, in [0, π].
fn ang_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(TAU);
    d.min(TAU - d)
}

// ---------------------------------------------------------------------------------------------
// Derivatives and lengths

#[test]
fn analytic_second_derivatives_match_finite_differences() {
    let f = tilted();
    let h = 1e-5;
    for c in [Curve::Circle { frame: f, radius: 2.5 }, Curve::Ellipse { frame: f, major: 3.0, minor: 1.2 }, Curve::line_through(P3::origin(), P3::new(1.0, 2.0, 3.0))] {
        for t in [0.0, 0.7, 2.0, 4.4] {
            let fd = (c.d1(t + h) - c.d1(t - h)) / (2.0 * h);
            assert!((fd - c.d2(t)).norm() < 1e-8, "{} d2 at {t}", c.type_name());
        }
    }
    for s in surfaces() {
        for (u, v) in [(0.3, 0.4), (2.0, -0.9), (4.0, 1.1)] {
            let (suu, suv, svv) = s.d2(u, v);
            let (a1, b1) = s.d1(u + h, v);
            let (a0, b0) = s.d1(u - h, v);
            let (_, c1) = s.d1(u, v + h);
            let (_, c0) = s.d1(u, v - h);
            assert!(((a1 - a0) / (2.0 * h) - suu).norm() < 1e-8, "{} Suu", s.type_name());
            assert!(((b1 - b0) / (2.0 * h) - suv).norm() < 1e-8, "{} Suv", s.type_name());
            assert!(((c1 - c0) / (2.0 * h) - svv).norm() < 1e-8, "{} Svv", s.type_name());
        }
    }
}

#[test]
fn curve_lengths() {
    let f = tilted();
    let line = Curve::Line { origin: P3::new(1.0, 2.0, 3.0), dir: V3::new(0.0, 0.6, 0.8) };
    assert_eq!(line.length(-2.0, 5.0), 7.0);
    let circle = Curve::Circle { frame: f, radius: 3.0 };
    assert!((circle.length(0.5, 2.0) - 4.5).abs() < 1e-15);
    assert_eq!(circle.length(2.0, 0.5), circle.length(0.5, 2.0));
    // Complete elliptic integral: perimeter of a = 2, b = 1 is 8·E(k² = 3/4).
    let e = Curve::Ellipse { frame: f, major: 2.0, minor: 1.0 };
    assert!((e.length(0.0, TAU) - 9.688448220547675).abs() < 1e-11, "{}", e.length(0.0, TAU));
    assert!((e.length(0.0, FRAC_PI_2) * 4.0 - e.length(0.0, TAU)).abs() < 1e-11);
    let ne = Curve::Nurbs(NurbsCurve::ellipse_arc(&f, 2.0, 1.0, 0.0, TAU).unwrap());
    assert!((ne.length(0.0, TAU) - 9.688448220547675).abs() < 1e-10);
    // Periodic NURBS: one full period equals the closed length regardless of the start.
    let pts = [P3::new(1.0, 0.0, 0.0), P3::new(0.0, 1.0, 0.0), P3::new(-1.0, 0.0, 0.0), P3::new(0.0, -1.0, 0.0)];
    let pc = Curve::Nurbs(NurbsCurve::periodic_uniform(2, &pts, &[1.0; 4]).unwrap());
    assert!((pc.length(0.0, 4.0) - pc.length(1.3, 5.3)).abs() < 1e-11);
}

// ---------------------------------------------------------------------------------------------
// Exact NURBS conversion: point-set equality checked by projection in both directions.

fn check_point_set(s: &Surface, u: (f64, f64), v: (f64, f64)) {
    let n = s.to_nurbs(u, v).unwrap();
    n.validate().unwrap();
    let ns = Surface::Nurbs(n.clone());
    let ((a0, a1), (b0, b1)) = n.domain();
    let scale = 10.0;
    for uu in grid(a0, a1, 23) {
        for vv in grid(b0, b1, 17) {
            let p = n.point(uu, vv);
            let (_, _, _, d) = s.project(&p);
            assert!(d <= 1e-12 * scale, "{}: NURBS point off the analytic surface by {d:e}", s.type_name());
        }
    }
    for uu in grid(u.0, u.1, 11) {
        for vv in grid(v.0, v.1, 9) {
            let p = s.point(uu, vv);
            let (_, _, q, d) = ns.project(&p);
            assert!(d <= 1e-9 * scale, "{}: analytic point ({uu}, {vv}) off the NURBS by {d:e} (nearest {q:?})", s.type_name());
        }
    }
    // Boundary correspondence: the NURBS corners are the analytic corners.
    for (x, y, xx, yy) in [(u.0, v.0, a0, b0), (u.1, v.1, a1, b1), (u.0, v.1, a0, b1)] {
        assert!((s.point(x, y) - n.point(xx, yy)).norm() <= 1e-12 * scale, "{} corner", s.type_name());
    }
}

#[test]
fn to_nurbs_is_the_same_point_set() {
    let f = tilted();
    check_point_set(&Surface::Plane { frame: f }, (-2.0, 3.0), (1.0, 4.0));
    check_point_set(&Surface::Cylinder { frame: f, radius: 3.0 }, (0.0, TAU), (-1.0, 2.0));
    check_point_set(&Surface::Cylinder { frame: f, radius: 3.0 }, (0.4, 2.1), (0.0, 1.0));
    check_point_set(&Surface::Cone { frame: f, radius: 2.0, half_angle: 0.4 }, (0.0, TAU), (0.0, 3.0));
    // Apex inside the range (v = −r/tan α ≈ −4.73): both nappes.
    check_point_set(&Surface::Cone { frame: f, radius: 2.0, half_angle: 0.4 }, (1.0, 4.0), (-8.0, 1.0));
    check_point_set(&Surface::Sphere { frame: f, radius: 4.0 }, (0.0, TAU), (-FRAC_PI_2, FRAC_PI_2));
    check_point_set(&Surface::Sphere { frame: f, radius: 4.0 }, (0.5, 2.0), (-0.3, 1.2));
    check_point_set(&Surface::Torus { frame: f, major: 5.0, minor: 1.5 }, (0.0, TAU), (0.0, TAU));
    check_point_set(&Surface::Torus { frame: f, major: 5.0, minor: 1.5 }, (2.0, 3.0), (-1.0, 2.5));
    let s = Surface::Sphere { frame: f, radius: 4.0 };
    assert!(s.to_nurbs((0.0, 1.0), (-2.0, 0.0)).is_err(), "sphere v beyond the pole");
    assert!(s.to_nurbs((0.0, 7.0), (0.0, 1.0)).is_err(), "angular range over 2π");
}

#[test]
fn curve_to_nurbs_matches() {
    let f = tilted();
    for c in [Curve::Circle { frame: f, radius: 2.0 }, Curve::Ellipse { frame: f, major: 3.0, minor: 1.0 }, Curve::line_through(P3::origin(), P3::new(1.0, 1.0, 0.0))] {
        let n = Curve::Nurbs(c.to_nurbs(0.3, 5.0).unwrap());
        for t in grid(0.3, 5.0, 40) {
            let p = n.point(t);
            assert!(c.project(&p).2 < 1e-12, "{}", c.type_name());
        }
        assert!((n.point(0.3) - c.point(0.3)).norm() < 1e-12 && (n.point(5.0) - c.point(5.0)).norm() < 1e-12);
    }
}

// ---------------------------------------------------------------------------------------------
// Projection

#[test]
fn curve_projection_round_trips() {
    let f = tilted();
    let curves = [
        Curve::Line { origin: P3::new(1.0, -1.0, 2.0), dir: V3::new(0.6, 0.0, 0.8) },
        Curve::Circle { frame: f, radius: 2.5 },
        Curve::Ellipse { frame: f, major: 4.0, minor: 1.5 },
        Curve::Ellipse { frame: f, major: 1.5, minor: 4.0 },
    ];
    for c in &curves {
        for t in grid(-0.3, 6.5, 37) {
            let p = c.point(t);
            let (tp, q, d) = c.project(&p);
            assert!(d < 1e-12, "{} at {t}: dist {d:e}", c.type_name());
            assert!((q - p).norm() < 1e-12);
            match c.period() {
                Some(_) => {
                    assert!(ang_diff(tp, t) < 1e-9, "{} at {t}: got {tp}", c.type_name());
                    assert!((0.0..TAU).contains(&tp), "wrapped to [0, 2π)");
                }
                None => assert!((tp - t).abs() < 1e-12),
            }
            // Offsets along the in-plane normal (inside the radius of curvature) invert to the same t.
            if c.period().is_some() {
                let tan = c.d1(t).normalize();
                let n = f.z.cross(&tan);
                for off in [0.3, -0.3] {
                    let (to, _, d) = c.project(&(p + n * off + f.z * 0.7));
                    assert!(ang_diff(to, t) < 1e-9, "{} offset {off} at {t}: got {to}", c.type_name());
                    assert!((d - (off * off + 0.49f64).sqrt()).abs() < 1e-9);
                }
            }
        }
    }
    // Degenerate: on the circle axis → t = 0.
    let circle = Curve::Circle { frame: f, radius: 2.5 };
    assert_eq!(circle.project(&(f.origin + f.z * 3.0)).0, 0.0);
    // Ellipse centre: the nearest points are the minor vertices.
    let e = Curve::Ellipse { frame: f, major: 4.0, minor: 1.5 };
    let (t, _, d) = e.project(&f.origin);
    assert!((d - 1.5).abs() < 1e-12 && (ang_diff(t, FRAC_PI_2) < 1e-12 || ang_diff(t, 3.0 * FRAC_PI_2) < 1e-12));
    // Near the evolute: compare with a dense brute-force search.
    for p in [P3::new(2.2, 0.05, 0.0), P3::new(-2.5, -0.02, 0.0), P3::new(0.3, 0.1, 0.0), P3::new(5.0, 3.0, 0.0)] {
        let pw = f.to_world(p);
        let (_, _, d) = e.project(&pw);
        let brute = grid(0.0, TAU, 200_000).into_iter().map(|t| (e.point(t) - pw).norm()).fold(f64::MAX, f64::min);
        assert!(d <= brute + 1e-12 && d >= brute - 1e-9, "ellipse near evolute {p:?}: {d} vs {brute}");
    }
}

#[test]
fn bounded_curve_projection() {
    let f = tilted();
    let c = Curve::Circle { frame: f, radius: 2.0 };
    let p = c.point(3.0) + (c.point(3.0) - f.origin) * 0.5;
    let (t, _, _) = c.project_in(&p, 0.0, 1.0);
    assert_eq!(t, 1.0, "closest end of the arc");
    let (t, _, _) = c.project_in(&p, 2.0, 4.0);
    assert!((t - 3.0).abs() < 1e-12);
    let (t, _, _) = c.project_in(&c.point(0.2), 5.0, 7.0);
    assert!((t - (0.2 + TAU)).abs() < 1e-12, "arc crossing 2π returns the shifted parameter");
    let e = Curve::Ellipse { frame: f, major: 4.0, minor: 1.5 };
    let (t, _, _) = e.project_in(&e.point(4.0), 3.5, 5.0);
    assert!((t - 4.0).abs() < 1e-9);
    let l = Curve::line_through(P3::origin(), P3::new(1.0, 0.0, 0.0));
    assert_eq!(l.project_in(&P3::new(5.0, 1.0, 0.0), 0.0, 2.0).0, 2.0);
}

#[test]
fn nurbs_curve_projection_round_trips() {
    let pts = vec![
        P3::new(0.0, 0.0, 0.0),
        P3::new(1.0, 2.0, 0.5),
        P3::new(3.0, 2.5, -1.0),
        P3::new(4.0, 0.0, 2.0),
        P3::new(6.0, -1.0, 1.0),
        P3::new(7.0, 1.0, 0.0),
    ];
    let c = NurbsCurve::new(3, vec![0.0, 0.0, 0.0, 0.0, 0.4, 0.7, 1.0, 1.0, 1.0, 1.0], pts, vec![1.0, 0.7, 1.5, 1.0, 0.9, 1.0]).unwrap();
    for t in grid(0.0, 1.0, 50) {
        let (tp, _, d) = c.project(&c.point(t));
        assert!(d < 1e-10 && (tp - t).abs() < 1e-8, "t {t} → {tp}, d {d:e}");
    }
    // Beyond the end: the end point.
    let (t, _, _) = c.project(&P3::new(20.0, 1.0, 0.0));
    assert_eq!(t, 1.0);
    // Periodic: wrap.
    let ring = [P3::new(3.0, 0.0, 0.0), P3::new(0.0, 2.0, 0.2), P3::new(-3.0, 0.0, 0.0), P3::new(0.0, -2.0, -0.2), P3::new(2.0, -2.0, 0.0)];
    let pc = NurbsCurve::periodic_uniform(3, &ring, &[1.0; 5]).unwrap();
    for t in grid(0.0, 5.0, 31) {
        let (tp, _, d) = pc.project(&pc.point(t));
        assert!(d < 1e-10);
        let dt = (tp - t).rem_euclid(5.0);
        assert!(dt.min(5.0 - dt) < 1e-8, "periodic t {t} → {tp}");
    }
    let nc = Curve::Nurbs(NurbsCurve::circle_arc(&tilted(), 3.0, 0.0, TAU).unwrap());
    let p = tilted().to_world(P3::new(4.0, 1.0, 0.5));
    let (_, q, d) = nc.project(&p);
    let exact = Curve::Circle { frame: tilted(), radius: 3.0 }.project(&p);
    assert!((q - exact.1).norm() < 1e-9 && (d - exact.2).abs() < 1e-12);
}

#[test]
fn surface_projection_round_trips() {
    for s in surfaces() {
        let ((_, _), (v0, v1)) = s.domain();
        let vs = if v0.is_finite() { grid(v0 + 0.05, v1 - 0.05, 9) } else { grid(-2.5, 3.0, 9) };
        for u in grid(0.0, TAU - 0.01, 13) {
            for &v in &vs {
                let p = s.point(u, v);
                let (pu, pv, q, d) = s.project(&p);
                assert!(d < 1e-12 && (q - p).norm() < 1e-12, "{} ({u}, {v}) dist {d:e}", s.type_name());
                let (wu, wv) = s.wrap(u, v);
                let ok_u = if s.periods().0.is_some() { ang_diff(pu, wu) < 1e-9 } else { (pu - u).abs() < 1e-12 };
                let ok_v = if s.periods().1.is_some() { ang_diff(pv, wv) < 1e-9 } else { (pv - v).abs() < 1e-9 };
                assert!(ok_u && ok_v, "{} ({u}, {v}) → ({pu}, {pv})", s.type_name());
                // Offset along the normal (outside, and inside within the curvature radius).
                if let Some(n) = s.normal(u, v) {
                    for off in [0.25, -0.25] {
                        let (ou, ov, _, od) = s.project(&(p + n * off));
                        assert!((od - 0.25).abs() < 1e-9, "{} offset dist {od}", s.type_name());
                        assert!((s.point(ou, ov) - p).norm() < 1e-9, "{} offset foot", s.type_name());
                    }
                }
            }
        }
    }
}

#[test]
fn surface_projection_degeneracies() {
    let f = tilted();
    let sphere = Surface::Sphere { frame: f, radius: 4.0 };
    let (u, v, _, _) = sphere.project(&f.origin);
    assert_eq!((u, v), (0.0, 0.0), "sphere centre");
    let (u, v, _, d) = sphere.project(&(f.origin + f.z * 7.0));
    assert_eq!(u, 0.0);
    assert!((v - FRAC_PI_2).abs() < 1e-15 && (d - 3.0).abs() < 1e-12, "north pole");
    let (u, v, _, _) = sphere.project(&(f.origin - f.z * 1.0));
    assert!(u == 0.0 && (v + FRAC_PI_2).abs() < 1e-15, "south pole from inside");
    let cyl = Surface::Cylinder { frame: f, radius: 3.0 };
    let (u, v, _, d) = cyl.project(&(f.origin + f.z * 2.0));
    assert!(u == 0.0 && (v - 2.0).abs() < 1e-12 && (d - 3.0).abs() < 1e-12);
    let torus = Surface::Torus { frame: f, major: 5.0, minor: 1.5 };
    let (u, v, _, d) = torus.project(&f.to_world(P3::new(0.0, 5.0, 0.0)));
    assert!((u - FRAC_PI_2).abs() < 1e-12 && v == 0.0 && (d - 1.5).abs() < 1e-12, "core circle → v = 0");
    let (u, _, _, d) = torus.project(&(f.origin + f.z * 1.0));
    assert!(u == 0.0 && (d - (26.0f64.sqrt() - 1.5)).abs() < 1e-12, "torus axis → u = 0");
    // Spindle torus (r > R): the far tube can be nearer.
    let spindle = Surface::Torus { frame: f, major: 1.0, minor: 3.0 };
    let p = f.to_world(P3::new(0.1, 0.0, 0.0));
    let (u, v, q, d) = spindle.project(&p);
    assert!((d - 1.9).abs() < 1e-12, "spindle distance {d}");
    assert!((spindle.point(u, v) - q).norm() < 1e-12 && (u - PI).abs() < 1e-12);
    // Double cone: a point beyond the apex projects onto the other nappe.
    let cone = Surface::Cone { frame: f, radius: 2.0, half_angle: FRAC_PI_2 / 2.0 };
    let p = f.to_world(P3::new(1.0, 0.0, -6.0));
    let (u, v, q, d) = cone.project(&p);
    let brute = grid(0.0, TAU, 720).into_iter().flat_map(|u| grid(-10.0, 5.0, 1500).into_iter().map(move |v| (u, v))).map(|(u, v)| (cone.point(u, v) - p).norm()).fold(f64::MAX, f64::min);
    assert!(d <= brute + 1e-12 && d > brute - 1e-2, "double cone {d} vs {brute}");
    assert!((cone.point(u, v) - q).norm() < 1e-12 && v < -2.0, "other nappe (v = {v})");
    assert_eq!(cone.project(&f.to_world(P3::new(0.0, 0.0, -2.0))).0, 0.0, "apex region on axis → u = 0");
}

#[test]
fn nurbs_surface_projection_round_trips() {
    let f = tilted();
    let torus = Surface::Torus { frame: f, major: 5.0, minor: 1.5 };
    let n = torus.to_nurbs((0.0, TAU), (0.0, TAU)).unwrap();
    let ns = Surface::Nurbs(n.clone());
    let ((a0, a1), (b0, b1)) = n.domain();
    for u in grid(a0, a1, 9) {
        for v in grid(b0, b1, 9) {
            let p = n.point(u, v);
            let (pu, pv, _, d) = ns.project(&p);
            assert!(d < 1e-9, "({u}, {v}) dist {d:e}");
            assert!((n.point(pu, pv) - p).norm() < 1e-9);
        }
    }
    // Off-surface points agree with the analytic projection distance.
    for p in [P3::new(7.0, 1.0, 2.0), P3::new(-3.0, 2.0, -0.5), P3::new(0.5, 0.2, 0.1)] {
        let pw = f.to_world(p);
        let (_, _, _, dn) = ns.project(&pw);
        let (_, _, _, da) = torus.project(&pw);
        assert!((dn - da).abs() < 1e-9, "{p:?}: {dn} vs {da}");
    }
    // Sphere NURBS with degenerate poles.
    let sphere = Surface::Sphere { frame: f, radius: 4.0 };
    let sn = Surface::Nurbs(sphere.to_nurbs((0.0, TAU), (-FRAC_PI_2, FRAC_PI_2)).unwrap());
    for p in [f.origin + f.z * 6.0, f.origin - f.z * 5.0, f.to_world(P3::new(1.0, 2.0, 3.0))] {
        let (_, _, _, dn) = sn.project(&p);
        let (_, _, _, da) = sphere.project(&p);
        assert!((dn - da).abs() < 1e-9, "{dn} vs {da}");
    }
    let _: &NurbsSurface = &n;
}

// ---------------------------------------------------------------------------------------------
// Intersections

fn on_surface(s: &Surface, p: &P3) -> f64 {
    s.project(p).3
}

#[test]
fn plane_plane_cases() {
    let a = tilted();
    let b = Frame::from_origin_z(P3::new(3.0, 1.0, -2.0), V3::new(1.0, 1.0, 0.2));
    let SurfaceIntersection::Curves { curves, tangent: false } = plane_plane(&a, &b, &tol()) else { panic!("line expected") };
    let Curve::Line { origin, dir } = curves[0] else { panic!("line") };
    for t in [-10.0, 0.0, 7.0] {
        let p = origin + dir * t;
        assert!(on_surface(&Surface::Plane { frame: a }, &p) < 1e-12 && on_surface(&Surface::Plane { frame: b }, &p) < 1e-12);
    }
    let shifted = Frame { origin: a.origin + a.z * 1e-3, ..a };
    assert_eq!(plane_plane(&a, &shifted, &tol()), SurfaceIntersection::None, "parallel");
    let flipped = Frame { origin: a.origin + a.x * 5.0 + a.z * 1e-9, x: a.x, y: -a.y, z: -a.z };
    assert_eq!(plane_plane(&a, &flipped, &tol()), SurfaceIntersection::Coincident, "coincident, opposite normals");
    assert_eq!(plane_plane(&a, &a, &tol()), SurfaceIntersection::Coincident);
}

#[test]
fn line_plane_cases() {
    let f = tilted();
    let r = line_plane(&P3::new(0.0, 0.0, 10.0), &V3::new(0.1, 0.2, -2.0), &f, &tol());
    let h = r.points();
    assert_eq!(h.len(), 1);
    assert!(on_surface(&Surface::Plane { frame: f }, &h[0].point) < 1e-12);
    assert!((P3::new(0.0, 0.0, 10.0) + V3::new(0.1, 0.2, -2.0) * h[0].t - h[0].point).norm() < 1e-12, "t in the caller's scale");
    assert_eq!(line_plane(&(f.origin + f.z), &f.x, &f, &tol()), CurveSurfaceIntersection::None);
    assert_eq!(line_plane(&(f.origin + f.y * 3.0), &f.x, &f, &tol()), CurveSurfaceIntersection::Coincident);
}

#[test]
fn line_sphere_cases() {
    let f = tilted();
    let r = 4.0;
    let s = Surface::Sphere { frame: f, radius: r };
    let o = f.to_world(P3::new(-10.0, 1.0, 0.5));
    let d = f.x;
    let h = line_sphere(&o, &d, &f, r, &tol());
    assert_eq!(h.points().len(), 2);
    for x in h.points() {
        assert!(!x.tangent && on_surface(&s, &x.point) < 1e-12 && Curve::Line { origin: o, dir: d }.project(&x.point).2 < 1e-12);
        assert!((s.point(x.u, x.v) - x.point).norm() < 1e-12);
    }
    // Exactly tangent: one point.
    let tangent = line_sphere(&f.to_world(P3::new(-10.0, r, 0.0)), &d, &f, r, &tol());
    let pts = tangent.points();
    assert_eq!(pts.len(), 1);
    assert!(pts[0].tangent && (pts[0].point - f.to_world(P3::new(0.0, r, 0.0))).norm() < 1e-12);
    // Recorded near-tangent behaviour with the default tolerance (1e-7 mm): a line missing or
    // cutting the sphere by 1e-9 mm gives ONE tangent point (the closest approach).
    for gap in [1e-9, -1e-9] {
        let near = line_sphere(&f.to_world(P3::new(-10.0, r + gap, 0.0)), &d, &f, r, &tol());
        assert_eq!(near.points().len(), 1, "gap {gap}");
        assert!(near.points()[0].tangent && on_surface(&s, &near.points()[0].point) <= 1e-7);
    }
    // With a tightened tolerance (1e-12 mm) the same lines miss / cross twice.
    let tight = Tolerance { linear: 1e-12, angular: 1e-12 };
    assert_eq!(line_sphere(&f.to_world(P3::new(-10.0, r + 1e-9, 0.0)), &d, &f, r, &tight), CurveSurfaceIntersection::None);
    let two = line_sphere(&f.to_world(P3::new(-10.0, r - 1e-9, 0.0)), &d, &f, r, &tight);
    assert_eq!(two.points().len(), 2);
    assert!(two.points().iter().all(|x| !x.tangent));
    // A deeper cut (1e-6 mm > tol) gives two points even with the default tolerance.
    assert_eq!(line_sphere(&f.to_world(P3::new(-10.0, r - 1e-6, 0.0)), &d, &f, r, &tol()).points().len(), 2);
    assert_eq!(line_sphere(&f.to_world(P3::new(-10.0, r + 1e-6, 0.0)), &d, &f, r, &tol()), CurveSurfaceIntersection::None);
}

fn check_line_hits(s: &Surface, o: &P3, d: &V3, r: &CurveSurfaceIntersection, expect: usize) {
    let h = r.points();
    assert_eq!(h.len(), expect, "{}: {r:?}", s.type_name());
    for x in h {
        assert!(on_surface(s, &x.point) <= 1e-9, "{} hit off surface by {:e}", s.type_name(), on_surface(s, &x.point));
        assert!((o + d * x.t - x.point).norm() <= 1e-9, "{} hit off line", s.type_name());
        assert!((s.point(x.u, x.v) - x.point).norm() <= 1e-9, "{} (u, v) wrong", s.type_name());
    }
    assert!(h.windows(2).all(|w| w[0].t < w[1].t), "sorted by t");
}

#[test]
fn line_cylinder_and_cone_cases() {
    let f = tilted();
    let cyl = Surface::Cylinder { frame: f, radius: 3.0 };
    let o = f.to_world(P3::new(-8.0, 1.0, 0.0));
    let d = f.dir_to_world(V3::new(1.0, 0.1, 0.4));
    check_line_hits(&cyl, &o, &d, &line_cylinder(&o, &d, &f, 3.0, &tol()), 2);
    let t = line_cylinder(&f.to_world(P3::new(-8.0, 3.0, 0.0)), &f.x, &f, 3.0, &tol());
    assert!(t.points().len() == 1 && t.points()[0].tangent);
    assert_eq!(line_cylinder(&f.to_world(P3::new(3.0, 0.0, 0.0)), &f.z, &f, 3.0, &tol()), CurveSurfaceIntersection::Coincident);
    assert_eq!(line_cylinder(&f.to_world(P3::new(1.0, 0.0, 0.0)), &f.z, &f, 3.0, &tol()), CurveSurfaceIntersection::None);

    let alpha = 0.4f64;
    let cone = Surface::Cone { frame: f, radius: 2.0, half_angle: alpha };
    check_line_hits(&cone, &o, &d, &line_cone(&o, &d, &f, 2.0, alpha, &tol()), 2);
    // Along the axis: crosses the double cone once, at the apex.
    let apex = f.to_world(P3::new(0.0, 0.0, -2.0 / alpha.tan()));
    let r = line_cone(&f.origin, &f.z, &f, 2.0, alpha, &tol());
    assert_eq!(r.points().len(), 1);
    assert!((r.points()[0].point - apex).norm() < 1e-9);
    // A generator lies on the cone.
    let gen_dir = f.dir_to_world(V3::new(alpha.tan(), 0.0, 1.0));
    assert_eq!(line_cone(&apex, &gen_dir, &f, 2.0, alpha, &tol()), CurveSurfaceIntersection::Coincident);
    // Parallel to a generator but offset outward: a single crossing of the far nappe.
    let par = line_cone(&f.to_world(P3::new(0.0, 1.0, 0.0)), &gen_dir, &f, 2.0, alpha, &tol());
    check_line_hits(&cone, &f.to_world(P3::new(0.0, 1.0, 0.0)), &gen_dir, &par, 1);
    // Steep line through both nappes.
    let o2 = f.to_world(P3::new(0.5, 0.2, -12.0));
    let d2 = f.dir_to_world(V3::new(0.02, 0.01, 1.0));
    check_line_hits(&cone, &o2, &d2, &line_cone(&o2, &d2, &f, 2.0, alpha, &tol()), 2);
}

#[test]
fn line_torus_cases() {
    let f = tilted();
    let (big, small) = (5.0, 1.5);
    let torus = Surface::Torus { frame: f, major: big, minor: small };
    // Through the centre in the equatorial plane: four roots at ±(R ± r).
    let o = f.to_world(P3::new(-20.0, 0.0, 0.0));
    let r = line_torus(&o, &f.x, &f, big, small, &tol());
    check_line_hits(&torus, &o, &f.x, &r, 4);
    let xs: Vec<f64> = r.points().iter().map(|h| f.to_local(h.point).x).collect();
    for (x, e) in xs.iter().zip([-6.5, -3.5, 3.5, 6.5]) {
        assert!((x - e).abs() < 1e-12, "{x} vs {e}");
    }
    // Generic oblique lines: the hit count equals the sign changes of a dense brute-force scan.
    for (o, d) in [((-9.0, 2.0, -1.0), (1.0, 0.3, 0.15)), ((-9.0, 0.5, 0.2), (1.0, 0.05, 0.02)), ((-2.0, -8.0, 3.0), (0.2, 1.0, -0.35)), ((6.0, 0.0, -4.0), (-0.1, 0.05, 1.0))] {
        let (o, d) = (f.to_world(P3::new(o.0, o.1, o.2)), f.dir_to_world(V3::new(d.0, d.1, d.2)).normalize());
        let fs: Vec<f64> = grid(-40.0, 40.0, 400_000).into_iter().map(|t| torus.signed_distance(&(o + d * t)).unwrap()).collect();
        let crossings = fs.windows(2).filter(|w| w[0] * w[1] < 0.0).count();
        assert!(crossings > 0);
        check_line_hits(&torus, &o, &d, &line_torus(&o, &d, &f, big, small, &tol()), crossings);
    }
    // Degenerate quartic: the top plane z = r through the axis touches at two points
    // (two double roots).
    let o = f.to_world(P3::new(-20.0, 0.0, small));
    let r = line_torus(&o, &f.x, &f, big, small, &tol());
    check_line_hits(&torus, &o, &f.x, &r, 2);
    assert!(r.points().iter().all(|h| h.tangent));
    assert!((f.to_local(r.points()[0].point).x + big).abs() < 1e-6 && (f.to_local(r.points()[1].point).x - big).abs() < 1e-6);
    // Tangent to the inner equator: one tangent contact plus two crossings of the outer wall.
    let o = f.to_world(P3::new(-20.0, big - small, 0.0));
    let r = line_torus(&o, &f.x, &f, big, small, &tol());
    check_line_hits(&torus, &o, &f.x, &r, 3);
    let tang: Vec<bool> = r.points().iter().map(|h| h.tangent).collect();
    assert_eq!(tang, vec![false, true, false]);
    assert!((f.to_local(r.points()[2].point).x - 2.0 * (big * small).sqrt()).abs() < 1e-9);
    // Horn torus (R = r) along its axis: a quadruple root at the centre.
    let horn = Surface::Torus { frame: f, major: 2.0, minor: 2.0 };
    let o = f.origin - f.z * 5.0;
    let r = line_torus(&o, &f.z, &f, 2.0, 2.0, &tol());
    check_line_hits(&horn, &o, &f.z, &r, 1);
    assert!((r.points()[0].point - f.origin).norm() < 1e-3 && r.points()[0].tangent);
    // Along the axis of an ordinary torus: no intersection; and a miss above it.
    assert_eq!(line_torus(&o, &f.z, &f, big, small, &tol()), CurveSurfaceIntersection::None);
    assert_eq!(line_torus(&f.to_world(P3::new(-20.0, 0.0, small + 1e-3)), &f.x, &f, big, small, &tol()), CurveSurfaceIntersection::None);
}

#[test]
fn plane_quadric_cases() {
    let f = tilted();
    let sphere = Surface::Sphere { frame: f, radius: 4.0 };
    let p = Frame::from_origin_z_x(f.to_world(P3::new(0.0, 0.0, 1.5)), f.dir_to_world(V3::new(0.2, 0.1, 1.0)), f.x);
    let SurfaceIntersection::Curves { curves, .. } = plane_sphere(&p, &f, 4.0, &tol()) else { panic!() };
    for t in grid(0.0, TAU, 12) {
        let q = curves[0].point(t);
        assert!(on_surface(&sphere, &q) < 1e-12 && on_surface(&Surface::Plane { frame: p }, &q) < 1e-12);
    }
    let touch = Frame { origin: f.origin + f.z * 4.0, ..f };
    let SurfaceIntersection::Point(q) = plane_sphere(&touch, &f, 4.0, &tol()) else { panic!("tangent point") };
    assert!((q - (f.origin + f.z * 4.0)).norm() < 1e-12);
    let away = Frame { origin: f.origin + f.z * 4.001, ..f };
    assert_eq!(plane_sphere(&away, &f, 4.0, &tol()), SurfaceIntersection::None);

    let cyl = Surface::Cylinder { frame: f, radius: 3.0 };
    for plane in [p, Frame { origin: f.origin + f.z * 2.0, ..f }, Frame::from_origin_z(f.origin + f.x * 1.0, f.x), Frame::from_origin_z(f.origin + f.x * 3.0, f.x)] {
        let SurfaceIntersection::Curves { curves, tangent } = plane_cylinder(&plane, &f, 3.0, &tol()) else { panic!() };
        for c in &curves {
            for t in grid(-2.0, 6.0, 10) {
                let q = c.point(t);
                assert!(on_surface(&cyl, &q) < 1e-12 && on_surface(&Surface::Plane { frame: plane }, &q) < 1e-12, "{} {tangent}", c.type_name());
            }
        }
        let kinds: Vec<&str> = curves.iter().map(|c| c.type_name()).collect();
        assert!(matches!(kinds.as_slice(), ["ellipse"] | ["circle"] | ["line", "line"] | ["line"]));
        assert_eq!(tangent, kinds == ["line"]);
    }
    assert_eq!(plane_cylinder(&Frame::from_origin_z(f.origin + f.x * 3.1, f.x), &f, 3.0, &tol()), SurfaceIntersection::None);

    let cone = Surface::Cone { frame: f, radius: 2.0, half_angle: 0.4 };
    let SurfaceIntersection::Curves { curves, .. } = plane_cone(&Frame { origin: f.origin + f.z * 1.5, ..f }, &f, 2.0, 0.4, &tol()).unwrap() else { panic!() };
    for t in grid(0.0, TAU, 9) {
        assert!(on_surface(&cone, &curves[0].point(t)) < 1e-12);
    }
    let apex = f.origin - f.z * (2.0 / 0.4f64.tan());
    let SurfaceIntersection::Point(q) = plane_cone(&Frame { origin: apex, ..f }, &f, 2.0, 0.4, &tol()).unwrap() else { panic!("apex point") };
    assert!((q - apex).norm() < 1e-12);
    assert!(plane_cone(&p, &f, 2.0, 0.4, &tol()).is_err(), "oblique plane is unsupported");
    assert!(surface_surface(&Surface::Plane { frame: p }, &sphere, &tol()).is_ok());
    assert!(surface_surface(&sphere, &cyl, &tol()).is_err());
}

#[test]
fn curve_curve_cases() {
    let f = tilted();
    let t = tol();
    let l1 = Curve::line_through(f.to_world(P3::new(-5.0, 0.0, 0.0)), f.to_world(P3::new(5.0, 0.0, 0.0)));
    let l2 = Curve::line_through(f.to_world(P3::new(1.0, -3.0, 0.0)), f.to_world(P3::new(1.0, 3.0, 0.0)));
    let r = curve_curve(&l1, &l2, &t).unwrap();
    assert_eq!(r.points().len(), 1);
    let h = r.points()[0];
    assert!((l1.point(h.ta) - h.point).norm() < 1e-12 && (l2.point(h.tb) - h.point).norm() < 1e-12);
    let skew = Curve::line_through(f.to_world(P3::new(1.0, -3.0, 0.1)), f.to_world(P3::new(1.0, 3.0, 0.1)));
    assert_eq!(curve_curve(&l1, &skew, &t).unwrap(), CurveCurveIntersection::None);
    let same = Curve::line_through(f.to_world(P3::new(7.0, 0.0, 0.0)), f.to_world(P3::new(-1.0, 0.0, 0.0)));
    assert_eq!(curve_curve(&l1, &same, &t).unwrap(), CurveCurveIntersection::Coincident);

    let c1 = Curve::Circle { frame: f, radius: 3.0 };
    let r = curve_curve(&l1, &c1, &t).unwrap();
    assert_eq!(r.points().len(), 2);
    for h in r.points() {
        assert!((l1.point(h.ta) - h.point).norm() < 1e-12 && (c1.point(h.tb) - h.point).norm() < 1e-12);
    }
    let tl = Curve::line_through(f.to_world(P3::new(-5.0, 3.0, 0.0)), f.to_world(P3::new(5.0, 3.0, 0.0)));
    let r = curve_curve(&c1, &tl, &t).unwrap();
    assert!(r.points().len() == 1 && r.points()[0].tangent);
    let pierce = Curve::line_through(f.to_world(P3::new(0.0, 3.0, -2.0)), f.to_world(P3::new(0.0, 3.0, 2.0)));
    assert_eq!(curve_curve(&pierce, &c1, &t).unwrap().points().len(), 1);

    // Coplanar circles: two points, tangent, coincident, none.
    let c2 = Curve::Circle { frame: Frame { origin: f.to_world(P3::new(4.0, 0.0, 0.0)), ..f }, radius: 2.0 };
    let r = curve_curve(&c1, &c2, &t).unwrap();
    assert_eq!(r.points().len(), 2);
    for h in r.points() {
        assert!((c1.point(h.ta) - h.point).norm() < 1e-12 && (c2.point(h.tb) - h.point).norm() < 1e-12);
    }
    let c3 = Curve::Circle { frame: Frame { origin: f.to_world(P3::new(5.0, 0.0, 0.0)), ..f }, radius: 2.0 };
    let r = curve_curve(&c1, &c3, &t).unwrap();
    assert!(r.points().len() == 1 && r.points()[0].tangent && (r.points()[0].point - f.to_world(P3::new(3.0, 0.0, 0.0))).norm() < 1e-12);
    let c4 = Curve::Circle { frame: Frame { origin: f.to_world(P3::new(1.0, 0.0, 0.0)), ..f }, radius: 2.0 };
    assert!(curve_curve(&c1, &c4, &t).unwrap().points()[0].tangent, "internal tangency");
    assert_eq!(curve_curve(&c1, &c1.clone(), &t).unwrap(), CurveCurveIntersection::Coincident);
    // Two great circles of a sphere (non-parallel planes): two antipodal points.
    let g = Curve::Circle { frame: Frame::from_origin_z_x(f.origin, f.x, f.y), radius: 3.0 };
    let r = curve_curve(&c1, &g, &t).unwrap();
    assert_eq!(r.points().len(), 2);
    for h in r.points() {
        assert!((c1.point(h.ta) - h.point).norm() < 1e-12 && (g.point(h.tb) - h.point).norm() < 1e-12);
    }
    assert!(curve_curve(&c1, &Curve::Ellipse { frame: f, major: 3.0, minor: 1.0 }, &t).is_err());
}

#[test]
fn generic_curve_surface() {
    let f = tilted();
    let t = tol();
    // Circle crossing a plane twice.
    let c = Curve::Circle { frame: Frame::from_origin_z_x(f.origin, f.x, f.z), radius: 2.0 };
    let plane = Surface::Plane { frame: Frame { origin: f.origin + f.z * 0.5, ..f } };
    let r = curve_surface(&c, (0.0, TAU), &plane, &t).unwrap();
    assert_eq!(r.points().len(), 2);
    for h in r.points() {
        assert!(on_surface(&plane, &h.point) <= 1e-12 && (c.point(h.t) - h.point).norm() < 1e-12 && !h.tangent);
    }
    // Circle touching the plane at its top: one tangent hit.
    let top = Surface::Plane { frame: Frame { origin: f.origin + f.z * 2.0, ..f } };
    let r = curve_surface(&c, (0.0, TAU), &top, &t).unwrap();
    assert!(r.points().len() == 1 && r.points()[0].tangent, "{r:?}");
    // NURBS curve against a torus and against a sphere.
    let nc = Curve::Nurbs(NurbsCurve::new(3, vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0], vec![
        f.to_world(P3::new(-9.0, 0.5, 0.2)),
        f.to_world(P3::new(-4.0, 2.0, -1.0)),
        f.to_world(P3::new(0.0, -1.0, 1.0)),
        f.to_world(P3::new(4.0, 1.0, -0.5)),
        f.to_world(P3::new(9.0, 0.0, 0.3)),
    ], vec![1.0, 0.8, 1.2, 1.0, 1.0]).unwrap());
    let torus = Surface::Torus { frame: f, major: 5.0, minor: 1.5 };
    let r = curve_surface(&nc, (0.0, 1.0), &torus, &t).unwrap();
    assert!(!r.points().is_empty());
    for h in r.points() {
        assert!(on_surface(&torus, &h.point) <= 1e-9 && (nc.point(h.t) - h.point).norm() < 1e-12);
    }
    // Circle lying in a plane: coincident.
    assert_eq!(curve_surface(&c, (0.0, TAU), &Surface::Plane { frame: Frame::from_origin_z_x(f.origin, f.x, f.y) }, &t).unwrap(), CurveSurfaceIntersection::Coincident);
}

#[test]
fn curve_against_nurbs_surface_matches_analytic() {
    let f = tilted();
    let t = tol();
    let sphere = Surface::Sphere { frame: f, radius: 4.0 };
    let ns = Surface::Nurbs(sphere.to_nurbs((0.0, TAU), (-FRAC_PI_2, FRAC_PI_2)).unwrap());
    for (o, d) in [
        (f.to_world(P3::new(-10.0, 1.0, 0.5)), f.x),
        (f.to_world(P3::new(1.0, -10.0, 2.0)), f.dir_to_world(V3::new(0.1, 1.0, 0.2))),
        (f.origin - f.z * 9.0, f.z),
    ] {
        let a = line_surface(&o, &d, &sphere, &t).unwrap();
        let n = line_surface(&o, &d, &ns, &t).unwrap();
        assert_eq!(a.points().len(), n.points().len(), "{a:?} vs {n:?}");
        for (x, y) in a.points().iter().zip(n.points()) {
            assert!((x.point - y.point).norm() < 1e-7 && (x.t - y.t).abs() < 1e-7);
        }
    }
    let line = Curve::Line { origin: f.to_world(P3::new(-10.0, 1.0, 0.5)), dir: f.x };
    assert_eq!(curve_surface(&line, (0.0, 8.0), &ns, &t).unwrap().points().len(), 1, "range filter");
    // Tangent and near-tangent lines: the NURBS path decides tangency like the analytic one.
    for (gap, tl) in [(0.0, t), (1e-9, t), (-1e-9, t), (-1e-6, t), (1e-6, t), (-1e-9, Tolerance { linear: 1e-12, angular: 1e-12 })] {
        let o = f.to_world(P3::new(-10.0, 4.0 + gap, 0.0));
        let a = line_surface(&o, &f.x, &sphere, &tl).unwrap();
        let n = line_surface(&o, &f.x, &ns, &tl).unwrap();
        let ta: Vec<bool> = a.points().iter().map(|h| h.tangent).collect();
        let tn: Vec<bool> = n.points().iter().map(|h| h.tangent).collect();
        assert_eq!(ta, tn, "gap {gap}, tol {}: analytic {a:?} vs NURBS {n:?}", tl.linear);
        for (x, y) in a.points().iter().zip(n.points()) {
            let along = if x.tangent { 1e-3 } else { 1e-7 };
            assert!((x.point - y.point).norm() < along, "gap {gap}: {:?} vs {:?}", x.point, y.point);
        }
    }
    // A circle against a NURBS torus patch.
    let torus = Surface::Torus { frame: f, major: 5.0, minor: 1.5 };
    let nt = Surface::Nurbs(torus.to_nurbs((0.0, TAU), (0.0, TAU)).unwrap());
    let c = Curve::Circle { frame: Frame::from_origin_z_x(f.to_world(P3::new(6.5, 0.0, 0.0)), f.y, f.x), radius: 1.0 };
    let a = curve_surface(&c, (0.0, TAU), &torus, &t).unwrap();
    let n = curve_surface(&c, (0.0, TAU), &nt, &t).unwrap();
    assert_eq!(a.points().len(), 2);
    assert_eq!(n.points().len(), a.points().len(), "{n:?}");
    for h in n.points() {
        assert!(on_surface(&torus, &h.point) < 1e-9 && on_surface(&nt, &h.point) < 1e-9);
        assert!((c.point(h.t) - h.point).norm() < 1e-9, "t is the circle angle");
    }
}

#[test]
fn polynomial_roots_isolation() {
    let (r, _) = poly_real_roots(&[-6.0, 11.0, -6.0, 1.0]);
    assert_eq!(r.len(), 3);
    for (x, e) in r.iter().zip([1.0, 2.0, 3.0]) {
        assert!((x - e).abs() < 1e-12);
    }
    let (r, c) = poly_real_roots(&[1.0, 0.0, -2.0, 0.0, 1.0]);
    // (x² − 1)²: double roots appear as critical points with zero value.
    assert!(c.iter().any(|x| (x - 1.0).abs() < 1e-7) && c.iter().any(|x| (x + 1.0).abs() < 1e-7));
    assert!(r.iter().all(|x| (x.abs() - 1.0).abs() < 1e-6));
    assert!(poly_real_roots(&[1.0, 0.0, 1.0]).0.is_empty());
}
