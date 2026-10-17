//! NURBS kernel: basis functions, evaluation, derivatives, construction and knot operations.

use cad_geom::nurbs::{basis_funs, ders_basis_funs, find_span};
use cad_geom::{Curve, Frame, GeomError, NurbsCurve, NurbsSurface, P3, V3};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn tilted() -> Frame {
    Frame::from_origin_z_x(P3::new(1.5, -2.0, 0.7), V3::new(0.3, -0.4, 1.0), V3::new(1.0, 0.2, 0.1))
}

/// Non-uniform clamped cubic knot vector with a double interior knot.
fn knots() -> Vec<f64> {
    vec![0.0, 0.0, 0.0, 0.0, 0.7, 1.3, 1.3, 2.9, 4.0, 4.0, 4.0, 4.0]
}

fn cubic() -> NurbsCurve {
    let pts = vec![
        P3::new(0.0, 0.0, 0.0),
        P3::new(1.0, 2.0, 0.5),
        P3::new(3.0, 2.5, -1.0),
        P3::new(4.0, 0.0, 2.0),
        P3::new(6.0, -1.0, 1.0),
        P3::new(7.0, 1.0, 0.0),
        P3::new(8.5, 3.0, -2.0),
        P3::new(10.0, 0.0, 0.0),
    ];
    let w = vec![1.0, 0.6, 2.0, 1.4, 0.8, 1.7, 0.5, 1.0];
    NurbsCurve::new(3, knots(), pts, w).unwrap()
}

fn samples(a: f64, b: f64, n: usize) -> impl Iterator<Item = f64> {
    (0..=n).map(move |i| a + (b - a) * i as f64 / n as f64)
}

fn assert_close(a: &P3, b: &P3, tol: f64, what: &str) {
    assert!((a - b).norm() <= tol, "{what}: {a:?} vs {b:?} (|d| = {:e})", (a - b).norm());
}

#[test]
fn find_span_brackets_parameter() {
    let k = knots();
    let n = k.len() - 4;
    assert_eq!(find_span(&k, 3, n, 0.0), 3);
    assert_eq!(find_span(&k, 3, n, 0.69), 3);
    assert_eq!(find_span(&k, 3, n, 0.7), 4);
    assert_eq!(find_span(&k, 3, n, 1.3), 6);
    assert_eq!(find_span(&k, 3, n, 4.0), 7, "domain end maps to the last non-empty span");
    for u in samples(0.0, 4.0, 97) {
        let s = find_span(&k, 3, n, u);
        assert!(k[s] <= u && (u < k[s + 1] || u == 4.0));
    }
}

#[test]
fn basis_partition_of_unity_and_derivative_sums() {
    let k = knots();
    let n = k.len() - 4;
    for u in samples(0.0, 4.0, 211) {
        let s = find_span(&k, 3, n, u);
        let b = basis_funs(&k, 3, s, u);
        assert!((b.iter().sum::<f64>() - 1.0).abs() < 1e-14, "sum at {u}");
        assert!(b.iter().all(|x| *x >= -1e-15), "non-negative at {u}");
        let d = ders_basis_funs(&k, 3, s, u, 4);
        for (x, y) in d[0].iter().zip(&b) {
            assert!((x - y).abs() < 1e-14);
        }
        for (order, row) in d.iter().enumerate().skip(1) {
            assert!(row.iter().sum::<f64>().abs() < 1e-9, "derivative order {order} sums to 0 at {u}");
        }
        assert!(d[4].iter().all(|x| *x == 0.0), "order > degree vanishes");
    }
}

#[test]
fn basis_derivatives_match_finite_differences() {
    let k = knots();
    let n = k.len() - 4;
    let h = 1e-6;
    for u in [0.2, 0.9, 1.31, 2.0, 3.5] {
        let s = find_span(&k, 3, n, u);
        let d = ders_basis_funs(&k, 3, s, u, 2);
        let (bp, bm) = (basis_funs(&k, 3, s, u + h), basis_funs(&k, 3, s, u - h));
        let b0 = basis_funs(&k, 3, s, u);
        for j in 0..4 {
            let fd1 = (bp[j] - bm[j]) / (2.0 * h);
            let fd2 = (bp[j] - 2.0 * b0[j] + bm[j]) / (h * h);
            assert!((fd1 - d[1][j]).abs() < 1e-7, "N' j={j} u={u}: {fd1} vs {}", d[1][j]);
            assert!((fd2 - d[2][j]).abs() < 1e-3, "N'' j={j} u={u}: {fd2} vs {}", d[2][j]);
        }
    }
}

#[test]
fn rational_curve_derivatives_match_finite_differences() {
    let c = cubic();
    let h = 1e-5;
    for t in [0.05, 0.5, 1.0, 1.6, 2.5, 3.3, 3.95] {
        let d = c.derivs(t, 3);
        let fd1 = (c.point(t + h) - c.point(t - h)) / (2.0 * h);
        let fd2 = (c.point(t + h) - 2.0 * c.point(t).coords + c.point(t - h).coords).coords / (h * h);
        let fd3 = (c.d2(t + h) - c.d2(t - h)) / (2.0 * h);
        assert!((fd1 - d[1]).norm() < 1e-6 * d[1].norm().max(1.0), "C' at {t}");
        assert!((fd2 - d[2]).norm() < 1e-3 * d[2].norm().max(1.0), "C'' at {t}");
        assert!((fd3 - d[3]).norm() < 1e-4 * d[3].norm().max(1.0), "C''' at {t}");
        assert_eq!(c.d1(t), d[1]);
    }
}

#[test]
fn nurbs_circle_points_lie_on_circle() {
    let f = tilted();
    let r = 50.0;
    for (a, b) in [(0.0, TAU), (0.3, 0.3 + FRAC_PI_2), (-1.0, 2.5), (0.0, PI), (1.0, 1.0 + 3.0 * FRAC_PI_2 + 0.01)] {
        let c = NurbsCurve::circle_arc(&f, r, a, b).unwrap();
        assert_eq!(c.domain(), (a, b));
        assert_close(&c.point(a), &Curve::Circle { frame: f, radius: r }.point(a), 1e-12, "start");
        assert_close(&c.point(b), &Curve::Circle { frame: f, radius: r }.point(b), 1e-12, "end");
        for t in samples(a, b, 500) {
            let l = f.to_local(c.point(t));
            assert!((l.x.hypot(l.y) - r).abs() <= 1e-12, "radius error {:e}", (l.x.hypot(l.y) - r).abs());
            assert!(l.z.abs() <= 1e-12, "out of plane {:e}", l.z);
            let ang = l.y.atan2(l.x);
            let rel = (ang - a).rem_euclid(TAU);
            assert!(rel <= (b - a) + 1e-12 || rel >= TAU - 1e-12, "angle {ang} outside arc [{a}, {b}]");
        }
        let segs = ((b - a) / FRAC_PI_2 - 1e-9).ceil() as usize;
        assert_eq!(c.control_points.len(), 2 * segs + 1, "segments ≤ 90°");
    }
}

#[test]
fn circle_arc_length_and_tangent() {
    let f = tilted();
    let c = Curve::Nurbs(NurbsCurve::circle_arc(&f, 7.0, 0.2, 4.0).unwrap());
    assert!((c.length(0.2, 4.0) - 7.0 * 3.8).abs() < 1e-10, "{}", c.length(0.2, 4.0));
    let analytic = Curve::Circle { frame: f, radius: 7.0 };
    for k in [0.2, 1.5, 2.71, 3.9] {
        let tn = c.d1(k).normalize();
        let ta = analytic.d1(analytic.project(&c.point(k)).0).normalize();
        assert!((tn - ta).norm() < 1e-10);
    }
}

#[test]
fn knot_insertion_leaves_curve_unchanged() {
    let c = cubic();
    for (u, times) in [(0.35, 1), (2.0, 3), (1.3, 1), (0.7, 2), (3.999, 2)] {
        let d = c.insert_knot(u, times).unwrap();
        assert_eq!(d.control_points.len(), c.control_points.len() + times);
        assert_eq!(d.knots.len(), c.knots.len() + times);
        d.validate().unwrap();
        for t in samples(0.0, 4.0, 300) {
            assert_close(&c.point(t), &d.point(t), 1e-12, "point after insertion");
        }
    }
    assert!(matches!(c.insert_knot(1.3, 2), Err(GeomError::InvalidInput(_))), "multiplicity would exceed degree");
    assert!(c.insert_knot(0.0, 1).is_err() && c.insert_knot(5.0, 1).is_err());
}

#[test]
fn split_and_extract_reproduce_curve() {
    let c = cubic();
    let (l, r) = c.split(1.7).unwrap();
    assert_eq!(l.domain(), (0.0, 1.7));
    assert_eq!(r.domain(), (1.7, 4.0));
    assert_close(&l.point(1.7), &r.point(1.7), 1e-12, "joint");
    for t in samples(0.0, 1.7, 100) {
        assert_close(&c.point(t), &l.point(t), 1e-12, "left");
    }
    for t in samples(1.7, 4.0, 100) {
        assert_close(&c.point(t), &r.point(t), 1e-12, "right");
    }
    // Splitting exactly at an existing knot (1.3, multiplicity 2) and near it (snapped).
    let (a, b) = c.split(1.3 + 1e-15).unwrap();
    assert_eq!(a.domain().1, 1.3);
    assert_close(&a.point(1.0), &c.point(1.0), 1e-12, "snap left");
    assert_close(&b.point(2.0), &c.point(2.0), 1e-12, "snap right");
    let e = c.extract(0.5, 3.1).unwrap();
    for t in samples(0.5, 3.1, 100) {
        assert_close(&c.point(t), &e.point(t), 1e-12, "extract");
    }
    let segs = c.bezier_segments();
    assert_eq!(segs.len(), 4);
    assert!(segs.iter().all(|s| s.control_points.len() == 4));
    assert!(c.split(0.0).is_err());
}

#[test]
fn degree_elevation_preserves_geometry() {
    let c = cubic();
    let e = c.elevate_degree();
    assert_eq!(e.degree, 4);
    e.validate().unwrap();
    for t in samples(0.0, 4.0, 200) {
        assert_close(&c.point(t), &e.point(t), 1e-12, "elevated");
    }
}

#[test]
fn periodic_curve_wraps_and_is_smooth_at_seam() {
    let pts = [P3::new(1.0, 0.0, 0.0), P3::new(0.0, 1.0, 0.3), P3::new(-1.0, 0.0, 0.0), P3::new(0.0, -1.0, -0.3), P3::new(0.5, -0.5, 0.0)];
    let c = NurbsCurve::periodic_uniform(3, &pts, &[1.0, 2.0, 1.0, 0.5, 1.0]).unwrap();
    assert!(c.periodic);
    assert_eq!(c.domain(), (0.0, 5.0));
    assert_eq!(c.period(), Some(5.0));
    for t in [0.0, 0.3, 2.2, 4.9] {
        assert_close(&c.point(t), &c.point(t + 5.0), 1e-12, "period");
        assert_close(&c.point(t), &c.point(t - 10.0), 1e-12, "period");
    }
    // C2 across the seam for a cubic.
    let (s0, s1) = (c.derivs(0.0, 2), c.derivs(5.0 - 1e-12, 2));
    assert!((s0[0] - s1[0]).norm() < 1e-9 && (s0[1] - s1[1]).norm() < 1e-8 && (s0[2] - s1[2]).norm() < 1e-6);
    let cl = c.clamped();
    assert!(!cl.periodic);
    for t in samples(0.0, 5.0, 100) {
        assert_close(&c.point(t), &cl.point(t), 1e-12, "clamped");
    }
    let ins = c.insert_knot(2.5, 2).unwrap();
    for t in samples(0.0, 5.0, 100) {
        assert_close(&c.point(t), &ins.point(t), 1e-12, "periodic insertion");
    }
    // Breaking the wrap is rejected.
    let mut bad = c.clone();
    bad.control_points[0].x += 0.1;
    assert!(bad.validate().is_err());
}

#[test]
fn constructor_validation() {
    let p = vec![P3::origin(), P3::new(1.0, 0.0, 0.0), P3::new(2.0, 1.0, 0.0)];
    assert!(NurbsCurve::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], p.clone(), vec![1.0; 3]).is_ok());
    assert!(NurbsCurve::new(2, vec![0.0, 0.0, 1.0, 1.0, 1.0], p.clone(), vec![1.0; 3]).is_err(), "knot count");
    assert!(NurbsCurve::new(2, vec![0.0, 0.0, 0.0, 1.0, 0.5, 1.0], p.clone(), vec![1.0; 3]).is_err(), "decreasing");
    assert!(NurbsCurve::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], p.clone(), vec![1.0, 0.0, 1.0]).is_err(), "zero weight");
    assert!(NurbsCurve::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], p.clone(), vec![1.0, -2.0, 1.0]).is_err(), "negative weight");
    assert!(NurbsCurve::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], p.clone(), vec![1.0; 2]).is_err(), "weight count");
    assert!(NurbsCurve::new(3, vec![0.0; 7], p, vec![1.0; 3]).is_err(), "too few points");
}

fn bump_surface() -> NurbsSurface {
    let (nu, nv) = (5, 4);
    let mut cps = Vec::new();
    let mut w = Vec::new();
    for i in 0..nu {
        for j in 0..nv {
            let z = ((i * 7 + j * 3) % 5) as f64 * 0.4 - 0.8;
            cps.push(P3::new(i as f64 * 2.0, j as f64 * 1.5 + 0.2 * i as f64, z));
            w.push(1.0 + 0.3 * ((i + 2 * j) % 3) as f64);
        }
    }
    NurbsSurface::new(3, 2, vec![0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0], cps, w, nu, nv).unwrap()
}

#[test]
fn surface_derivatives_match_finite_differences() {
    let s = bump_surface();
    let h = 1e-5;
    for (u, v) in [(0.1, 0.2), (0.45, 0.7), (0.9, 0.95), (0.3, 0.55)] {
        let d = s.derivs(u, v, 2);
        let fu = (s.point(u + h, v) - s.point(u - h, v)) / (2.0 * h);
        let fv = (s.point(u, v + h) - s.point(u, v - h)) / (2.0 * h);
        assert!((fu - d[1][0]).norm() < 1e-6 * d[1][0].norm().max(1.0));
        assert!((fv - d[0][1]).norm() < 1e-6 * d[0][1].norm().max(1.0), "fv {fv:?} vs {:?} at ({u},{v})", d[0][1]);
        let (du1, _) = s.d1(u + h, v);
        let (du0, _) = s.d1(u - h, v);
        let (_, dv1) = s.d1(u, v + h);
        let (_, dv0) = s.d1(u, v - h);
        let (suu, suv, svv) = s.d2(u, v);
        assert!(((du1 - du0) / (2.0 * h) - suu).norm() < 1e-5 * suu.norm().max(1.0));
        assert!(((dv1 - dv0) / (2.0 * h) - svv).norm() < 1e-5 * svv.norm().max(1.0));
        let (_, dvu1) = s.d1(u + h, v);
        let (_, dvu0) = s.d1(u - h, v);
        assert!(((dvu1 - dvu0) / (2.0 * h) - suv).norm() < 1e-5 * suv.norm().max(1.0));
    }
}

#[test]
fn surface_extract_and_bezier_patches_reproduce() {
    let s = bump_surface();
    let e = s.extract((0.2, 0.8), (0.1, 0.6)).unwrap();
    for u in samples(0.2, 0.8, 12) {
        for v in samples(0.1, 0.6, 9) {
            assert_close(&s.point(u, v), &e.point(u, v), 1e-12, "sub-patch");
        }
    }
    let patches = s.bezier_patches();
    assert_eq!(patches.len(), 4);
    for p in &patches {
        assert_eq!((p.count_u, p.count_v), (4, 3));
        let ((a, b), (c, d)) = p.domain();
        let (u, v) = (0.3 * a + 0.7 * b, 0.6 * c + 0.4 * d);
        assert_close(&s.point(u, v), &p.point(u, v), 1e-12, "bezier patch");
    }
    // Corner interpolation of a clamped surface.
    assert_close(&s.point(0.0, 0.0), &s.control_points[0], 1e-14, "corner");
}

#[test]
fn surface_validation() {
    let s = bump_surface();
    let mut bad = s.clone();
    bad.weights[3] = 0.0;
    assert!(bad.validate().is_err());
    let mut bad = s.clone();
    bad.count_v = 3;
    assert!(bad.validate().is_err());
    let mut bad = s;
    bad.knots_u.pop();
    assert!(bad.validate().is_err());
}
