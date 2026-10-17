//! Mass properties of the `research/cases.json` primitives against closed-form formulas
//! (≤ 1e-9 relative), plus the mesh-volume cross-check of the tessellator.

use cad_geom::mass::{face_area, solid_properties, MassProperties};
use cad_geom::primitives::*;
use cad_geom::tess::tessellate_solid;
use cad_geom::{Brep, FaceId, Frame, SolidId, P3, V3};
use nalgebra::{Matrix3, Rotation3};
use std::f64::consts::PI;

#[path = "support/seamed.rs"]
mod seamed;

const TOL: f64 = 1e-9;

struct Expect {
    volume: f64,
    area: f64,
    centroid: P3,
    inertia: Matrix3<f64>,
    size: f64,
}

fn mat(m: &[[f64; 3]; 3]) -> Matrix3<f64> {
    Matrix3::from_fn(|i, j| m[i][j])
}

/// Inertia of a body of revolution about axis `a` (unit): I⊥·(Id − aaᵀ) + I_ax·aaᵀ.
fn axial(a: V3, i_perp: f64, i_ax: f64) -> Matrix3<f64> {
    let aa = a * a.transpose();
    (Matrix3::identity() - aa) * i_perp + aa * i_ax
}

fn check(id: &str, mp: &MassProperties, e: &Expect) -> [f64; 4] {
    let ev = (mp.volume - e.volume).abs() / e.volume;
    let ea = (mp.area - e.area).abs() / e.area;
    let ec = (mp.centroid - e.centroid).norm() / e.size;
    let ei = (mat(&mp.inertia) - e.inertia).abs().max() / e.inertia.abs().max();
    eprintln!("{id:22} vol {ev:.1e} area {ea:.1e} centroid {ec:.1e} inertia {ei:.1e}");
    assert!(ev <= TOL, "{id}: volume {} vs {}", mp.volume, e.volume);
    assert!(ea <= TOL, "{id}: area {} vs {}", mp.area, e.area);
    assert!(ec <= TOL, "{id}: centroid {:?} vs {:?}", mp.centroid, e.centroid);
    assert!(ei <= TOL, "{id}: inertia {:?} vs {:?}", mp.inertia, e.inertia);
    [ev, ea, ec, ei]
}

fn props(b: &Brep) -> MassProperties {
    solid_properties(b, SolidId(0)).unwrap()
}

fn box_case(id: &str, corner: [f64; 3], s: [f64; 3]) {
    let b = make_box(P3::from(corner), s).unwrap();
    let [a, bb, c] = s;
    let v = a * bb * c;
    let e = Expect {
        volume: v,
        area: 2.0 * (a * bb + bb * c + c * a),
        centroid: P3::from(corner) + V3::from(s) / 2.0,
        inertia: Matrix3::from_diagonal(&V3::new(bb * bb + c * c, a * a + c * c, a * a + bb * bb)) * (v / 12.0),
        size: a.max(bb).max(c),
    };
    check(id, &props(&b), &e);
}

#[test]
fn boxes_exact() {
    box_case("prim.box", [0.0; 3], [50.0, 30.0, 10.0]);
    box_case("prim.box.tiny", [0.0; 3], [0.01, 0.02, 0.03]);
    box_case("prim.box.huge", [0.0; 3], [100000.0, 50000.0, 20000.0]);
    box_case("box.offset", [-7.0, 3.5, 1e3], [2.0, 9.0, 4.0]);
}

fn cylinder_case(id: &str, base: [f64; 3], axis: [f64; 3], r: f64, h: f64) {
    let a = V3::from(axis).normalize();
    let b = make_cylinder(Frame::from_origin_z(P3::from(base), a), r, h).unwrap();
    let v = PI * r * r * h;
    let e = Expect {
        volume: v,
        area: 2.0 * PI * r * r + 2.0 * PI * r * h,
        centroid: P3::from(base) + a * (h / 2.0),
        inertia: axial(a, v * (3.0 * r * r + h * h) / 12.0, v * r * r / 2.0),
        size: r.max(h),
    };
    check(&format!("{id} [asm]"), &props(&b), &e);
    let s = seamed::cylinder(Frame::from_origin_z(P3::from(base), a), r, h).unwrap();
    check(&format!("{id} [seamed]"), &props(&s), &e);
}

#[test]
fn cylinders_exact() {
    cylinder_case("prim.cylinder", [0.0; 3], [0.0, 0.0, 1.0], 10.0, 20.0);
    cylinder_case("prim.cylinder.tilted", [1.0, 2.0, 3.0], [1.0, 1.0, 1.0], 4.0, 15.0);
}

/// Frustum/cone by exact integration of disk slices: ρ(z) = R + kz, k = (r − R)/h.
fn frustum_expect(base: P3, axis: V3, rr: f64, r: f64, h: f64) -> Expect {
    let k = (r - rr) / h;
    let v = PI * (rr * rr * h + rr * k * h * h + k * k * h.powi(3) / 3.0);
    let mz = PI * (rr * rr * h * h / 2.0 + 2.0 * rr * k * h.powi(3) / 3.0 + k * k * h.powi(4) / 4.0);
    let zbar = mz / v;
    let int_rho4 = ((rr + k * h).powi(5) - rr.powi(5)) / (5.0 * k);
    let int_rho2_z2 = rr * rr * h.powi(3) / 3.0 + 2.0 * rr * k * h.powi(4) / 4.0 + k * k * h.powi(5) / 5.0;
    let i_ax = PI / 2.0 * int_rho4;
    let i_perp = PI / 4.0 * int_rho4 + PI * int_rho2_z2 - v * zbar * zbar;
    let area = PI * (rr + r) * ((rr - r).powi(2) + h * h).sqrt() + PI * (rr * rr + r * r);
    Expect { volume: v, area, centroid: base + axis * zbar, inertia: axial(axis, i_perp, i_ax), size: rr.max(h) }
}

#[test]
fn cones_exact() {
    let f = Frame::world();
    // prim.cone: pointed, classic formulas (ASM convention, then the seamed fixture)
    let (rr, h) = (10.0, 20.0);
    let v = PI * rr * rr * h / 3.0;
    let classic = Expect {
        volume: v,
        area: PI * rr * rr + PI * rr * (rr * rr + h * h).sqrt(),
        centroid: P3::new(0.0, 0.0, h / 4.0),
        inertia: axial(V3::z(), v * (3.0 * rr * rr / 20.0 + 3.0 * h * h / 80.0), 0.3 * v * rr * rr),
        size: h,
    };
    for (conv, b) in [("asm", make_cone(f, rr, 0.0, h).unwrap()), ("seamed", seamed::cone(f, rr, 0.0, h).unwrap())] {
        let mp = props(&b);
        check(&format!("prim.cone [{conv}]"), &mp, &classic);
        check(&format!("prim.cone slices [{conv}]"), &mp, &frustum_expect(P3::origin(), V3::z(), rr, 0.0, h));
    }
    // prim.frustum
    for (conv, b) in [("asm", make_cone(f, 10.0, 5.0, 20.0).unwrap()), ("seamed", seamed::cone(f, 10.0, 5.0, 20.0).unwrap())] {
        check(&format!("prim.frustum [{conv}]"), &props(&b), &frustum_expect(P3::origin(), V3::z(), 10.0, 5.0, 20.0));
    }
    // apex at the base, tilted frame
    let a = V3::new(-2.0, 1.0, 0.5).normalize();
    let fr = Frame::from_origin_z(P3::new(3.0, -1.0, 2.0), a);
    for (conv, b) in [("asm", make_cone(fr, 0.0, 6.0, 9.0).unwrap()), ("seamed", seamed::cone(fr, 0.0, 6.0, 9.0).unwrap())] {
        check(&format!("cone.inverted.tilted [{conv}]"), &props(&b), &frustum_expect(P3::new(3.0, -1.0, 2.0), a, 0.0, 6.0, 9.0));
    }
}

#[test]
fn sphere_exact() {
    for (id, c, r) in [("prim.sphere", [0.0; 3], 10.0), ("sphere.offset", [5.0, -2.0, 7.0], 0.5)] {
        let v = 4.0 / 3.0 * PI * r * r * r;
        let e = Expect {
            volume: v,
            area: 4.0 * PI * r * r,
            centroid: P3::from(c),
            inertia: Matrix3::identity() * (0.4 * v * r * r),
            size: r,
        };
        check(&format!("{id} [asm]"), &props(&make_sphere(P3::from(c), r).unwrap()), &e);
        check(&format!("{id} [seamed]"), &props(&seamed::sphere(P3::from(c), r).unwrap()), &e);
    }
}

#[test]
fn torus_exact() {
    let (rr, r) = (20.0, 5.0);
    let v = 2.0 * PI * PI * rr * r * r;
    let e = Expect {
        volume: v,
        area: 4.0 * PI * PI * rr * r,
        centroid: P3::origin(),
        inertia: axial(V3::z(), v * (rr * rr / 2.0 + 5.0 * r * r / 8.0), v * (rr * rr + 0.75 * r * r)),
        size: rr + r,
    };
    check("prim.torus [asm]", &props(&make_torus(Frame::world(), rr, r).unwrap()), &e);
    check("prim.torus [seamed]", &props(&seamed::torus(Frame::world(), rr, r).unwrap()), &e);
}

#[test]
fn face_areas() {
    let b = make_box(P3::origin(), [50.0, 30.0, 10.0]).unwrap();
    let mut areas: Vec<f64> = b.face_ids().map(|f| face_area(&b, f).unwrap()).collect();
    areas.sort_by(f64::total_cmp);
    for (got, want) in areas.iter().zip([300.0, 300.0, 500.0, 500.0, 1500.0, 1500.0]) {
        assert!((got - want).abs() <= TOL * want, "{got} vs {want}");
    }
    let (r, h) = (10.0, 20.0);
    let c = make_cylinder(Frame::world(), r, h).unwrap();
    for f in c.face_ids() {
        let want = if c.face(f).surface.type_name() == "plane" { PI * r * r } else { 2.0 * PI * r * h };
        let got = face_area(&c, f).unwrap();
        assert!((got - want).abs() <= TOL * want, "F{}: {got} vs {want}", f.0);
    }
    let s = make_sphere(P3::origin(), 3.0).unwrap();
    let a = face_area(&s, FaceId(0)).unwrap();
    assert!((a - 36.0 * PI).abs() <= TOL * a);
}

#[test]
fn rigid_motion_rotates_inertia() {
    let b = make_cone(Frame::world(), 10.0, 5.0, 20.0).unwrap();
    let m0 = props(&b);
    let rot = Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(V3::new(0.3, -1.0, 2.0)), 1.1);
    let t = V3::new(100.0, -40.0, 7.5);
    let m1 = props(&b.transformed(&rot, &t));
    let r = rot.matrix();
    let want = r * mat(&m0.inertia) * r.transpose();
    assert!((m1.volume - m0.volume).abs() <= TOL * m0.volume);
    assert!((m1.area - m0.area).abs() <= TOL * m0.area);
    assert!((m1.centroid - (rot * m0.centroid + t)).norm() <= TOL * 20.0);
    assert!((mat(&m1.inertia) - want).abs().max() <= TOL * want.abs().max());
}

#[test]
fn inertia_is_symmetric_with_negative_products() {
    // An off-axis box: products about the centroid vanish; a rotated box has I_xy of the sign of −∫xy.
    let b = make_box(P3::origin(), [4.0, 1.0, 1.0]).unwrap();
    let rot = Rotation3::from_axis_angle(&V3::z_axis(), PI / 4.0);
    let m = props(&b.transformed(&rot, &V3::zeros()));
    // long axis along (1,1,0)/√2: ∫xy dV > 0 about the centroid, so I_xy < 0
    assert!(m.inertia[0][1] < 0.0 && (m.inertia[0][1] - m.inertia[1][0]).abs() < 1e-12);
    let v = 4.0;
    let (ia, ib) = (v * (1.0 + 1.0) / 12.0, v * (16.0 + 1.0) / 12.0); // about long / short axis
    assert!((m.inertia[0][1] - (ia - ib) / 2.0).abs() <= TOL * ib);
}
/// Mesh volume of every closed primitive converges linearly to the exact volume as the chord
/// shrinks; an inscribed mesh loses ≈ (4/3)·chord/ρ (ρ = governing radius of curvature), so
/// the error is also bounded by 2·chord/ρ.
#[test]
fn mesh_volume_converges() {
    let cases: Vec<(&str, Brep, f64)> = vec![
        ("prim.box", make_box(P3::origin(), [50.0, 30.0, 10.0]).unwrap(), f64::INFINITY),
        ("prim.cylinder", make_cylinder(Frame::world(), 10.0, 20.0).unwrap(), 10.0),
        ("prim.cylinder.tilted", make_cylinder(Frame::from_origin_z(P3::new(1.0, 2.0, 3.0), V3::new(1.0, 1.0, 1.0)), 4.0, 15.0).unwrap(), 4.0),
        ("prim.cone", make_cone(Frame::world(), 10.0, 0.0, 20.0).unwrap(), 10.0),
        ("prim.frustum", make_cone(Frame::world(), 10.0, 5.0, 20.0).unwrap(), 10.0),
        ("prim.sphere", make_sphere(P3::origin(), 10.0).unwrap(), 10.0),
        ("prim.torus", make_torus(Frame::world(), 20.0, 5.0).unwrap(), 5.0),
        ("seamed.cylinder", seamed::cylinder(Frame::world(), 10.0, 20.0).unwrap(), 10.0),
        ("seamed.cone", seamed::cone(Frame::world(), 10.0, 0.0, 20.0).unwrap(), 10.0),
        ("seamed.sphere", seamed::sphere(P3::origin(), 10.0).unwrap(), 10.0),
        ("seamed.torus", seamed::torus(Frame::world(), 20.0, 5.0).unwrap(), 5.0),
    ];
    for (id, b, rho) in &cases {
        let exact = props(b).volume;
        let mut last = f64::INFINITY;
        for chord in [1.0, 0.1, 0.01] {
            let mesh = tessellate_solid(b, SolidId(0), chord).unwrap();
            let err = (mesh.signed_volume() - exact).abs() / exact;
            eprintln!("{id:22} chord {chord:5} tris {:7} rel vol err {err:.3e}", mesh.triangles.len());
            assert!(err <= 2.0 * chord / rho + 1e-12, "{id}: error {err} above 2·chord/ρ at chord {chord}");
            if chord < 1.0 {
                assert!(err <= 0.2 * last || err < 1e-12, "{id}: not converging at chord {chord} ({err} after {last})");
            }
            last = err;
        }
    }
}

/// prim.sphere: an inscribed mesh loses ≈ (5/3)·chord/r of the volume, so at chord 0.005 mm on
/// r = 10 the relative error is ≈ 8e-4 < 1e-3.
#[test]
fn sphere_mesh_volume_spec() {
    let b = make_sphere(P3::origin(), 10.0).unwrap();
    let exact = 4.0 / 3.0 * PI * 1000.0;
    let mesh = tessellate_solid(&b, SolidId(0), 0.005).unwrap();
    let err = (mesh.signed_volume() - exact).abs() / exact;
    assert!(err < 1e-3, "prim.sphere at chord 0.005: {err}");
    let predicted = 5.0 / 3.0 * 0.005 / 10.0;
    assert!((err - predicted).abs() < 0.15 * predicted, "{err} vs (5/3)·chord/r = {predicted}");
}

fn all_primitives() -> Vec<(&'static str, Brep)> {
    vec![
        ("prim.box", make_box(P3::origin(), [50.0, 30.0, 10.0]).unwrap()),
        ("prim.cylinder.tilted", make_cylinder(Frame::from_origin_z(P3::new(1.0, 2.0, 3.0), V3::new(1.0, 1.0, 1.0)), 4.0, 15.0).unwrap()),
        ("prim.cone", make_cone(Frame::world(), 10.0, 0.0, 20.0).unwrap()),
        ("prim.frustum", make_cone(Frame::world(), 10.0, 5.0, 20.0).unwrap()),
        ("prim.sphere", make_sphere(P3::origin(), 10.0).unwrap()),
        ("prim.torus", make_torus(Frame::world(), 20.0, 5.0).unwrap()),
        ("seamed.frustum", seamed::cone(Frame::world(), 10.0, 5.0, 20.0).unwrap()),
        ("seamed.cone", seamed::cone(Frame::world(), 10.0, 0.0, 20.0).unwrap()),
        ("seamed.sphere", seamed::sphere(P3::origin(), 10.0).unwrap()),
        ("seamed.torus", seamed::torus(Frame::world(), 20.0, 5.0).unwrap()),
    ]
}

/// Vertex normals are unit, outward, and agree with the triangle winding (pole/apex included).
#[test]
fn mesh_normals_outward() {
    for (id, b) in all_primitives() {
        let m = tessellate_solid(&b, SolidId(0), 0.1).unwrap();
        for t in &m.triangles {
            let p = |i: u32| V3::from(m.positions[i as usize]);
            let g = (p(t[1]) - p(t[0])).cross(&(p(t[2]) - p(t[0])));
            for &i in t {
                let n = V3::from(m.normals[i as usize]);
                assert!((n.norm() - 1.0).abs() < 1e-9, "{id}: normal {n:?}");
                assert!(g.dot(&n) > 0.0, "{id}: triangle {t:?} normal {n:?} against winding {g:?}");
            }
        }
    }
}

/// Faces share boundary vertices: after merging coincident positions (1e-9·size), every directed
/// triangle edge has an opposite twin, i.e. the solid's mesh is closed.
#[test]
fn mesh_watertight() {
    for (id, b) in all_primitives() {
        let m = tessellate_solid(&b, SolidId(0), 0.1).unwrap();
        let size = m.positions.iter().map(|p| V3::from(*p).amax()).fold(1.0, f64::max);
        let tol = 1e-9 * size;
        let mut uniq: Vec<V3> = Vec::new();
        let remap: Vec<usize> = m
            .positions
            .iter()
            .map(|p| {
                let p = V3::from(*p);
                uniq.iter().position(|q| (q - p).norm() <= tol).unwrap_or_else(|| {
                    uniq.push(p);
                    uniq.len() - 1
                })
            })
            .collect();
        let mut edges = std::collections::HashMap::<(usize, usize), i32>::new();
        for t in &m.triangles {
            for k in 0..3 {
                let (a, c) = (remap[t[k] as usize], remap[t[(k + 1) % 3] as usize]);
                *edges.entry((a, c)).or_default() += 1;
            }
        }
        for (&(a, c), &n) in &edges {
            assert_eq!(edges.get(&(c, a)).copied().unwrap_or(0), n, "{id}: edge {a}->{c} has no twin");
        }
    }
}

/// Seamless band: the side face of the ASM cylinder (two closed circles, no seam) has area 2πrh.
#[test]
fn seamless_band_area() {
    for (r, h, fr) in [
        (10.0, 20.0, Frame::world()),
        (4.0, 15.0, Frame::from_origin_z(P3::new(1.0, 2.0, 3.0), V3::new(1.0, 1.0, 1.0))),
    ] {
        let b = make_cylinder(fr, r, h).unwrap();
        let side = b.face_ids().find(|&f| b.face(f).surface.type_name() == "cylinder").unwrap();
        assert_eq!(b.face(side).loops.len(), 2);
        let a = face_area(&b, side).unwrap();
        let want = 2.0 * PI * r * h;
        assert!((a - want).abs() <= TOL * want, "{a} vs {want}");
    }
    // the frustum's side face: π(R + r)·slant
    let b = make_cone(Frame::world(), 10.0, 5.0, 20.0).unwrap();
    let want = PI * 15.0 * (25.0f64 + 400.0).sqrt();
    let a = face_area(&b, FaceId(0)).unwrap();
    assert!((a - want).abs() <= TOL * want, "{a} vs {want}");
}

/// Cylinder r, base z = 0, cut obliquely by the plane z = h + m·x: the top edge is an ellipse,
/// and on the side face v varies along it (exercises the v₀ term of the virtual seam).
/// Closed forms: V = πr²h, side area 2πrh, top area πr²√(1+m²), x̄ = m r²/(4h),
/// z̄ = h/2 + m²r²/(8h); second moments from ∫∫_disk ∫_0^{h+mx} (…) dz.
fn oblique_cylinder(r: f64, h: f64, m: f64, seamed: bool) -> Brep {
    use cad_geom::Curve;
    use std::f64::consts::TAU;
    let n = (1.0 + m * m).sqrt();
    let f0 = Frame::world();
    let ftop = Frame::from_origin_z_x(P3::new(0.0, 0.0, h), V3::new(-m, 0.0, 1.0), V3::new(1.0, 0.0, m));
    let mut b = Brep::new();
    let s = b.add_solid();
    let sh = b.add_shell(Some(s));
    let v0 = b.add_vertex(P3::new(r, 0.0, 0.0));
    let v1 = b.add_vertex(P3::new(r, 0.0, h + m * r));
    let c0 = b.add_edge(Some(Curve::Circle { frame: f0, radius: r }), 0.0, TAU, v0, v0);
    let el = b.add_edge(Some(Curve::Ellipse { frame: ftop, major: r * n, minor: r }), 0.0, TAU, v1, v1);
    let side = b.add_face(sh, cad_geom::Surface::Cylinder { frame: f0, radius: r }, false);
    if seamed {
        let seam = b.add_line_edge(v0, v1);
        b.add_loop(side, &[(c0, false), (seam, false), (el, true), (seam, true)]);
    } else {
        b.add_loop(side, &[(c0, false)]);
        b.add_loop(side, &[(el, true)]);
    }
    let bot = b.add_face(sh, cad_geom::Surface::Plane { frame: f0 }, true);
    b.add_loop(bot, &[(c0, true)]);
    let top = b.add_face(sh, cad_geom::Surface::Plane { frame: ftop }, false);
    b.add_loop(top, &[(el, false)]);
    b
}

#[test]
fn oblique_cut_cylinder_exact() {
    let (r, h, m) = (3.0, 10.0, 0.6);
    let v = PI * r * r * h;
    // ∫∫_disk ∫_0^{h+mx} f dz dA with ∫_disk x² = ∫_disk y² = πr⁴/4, ∫_disk x⁴ = πr⁶/8,
    // ∫_disk x²y² = πr⁶/24
    let (d2, d4, d22) = (PI * r.powi(4) / 4.0, PI * r.powi(6) / 8.0, PI * r.powi(6) / 24.0);
    let sx = m * d2; // ∫ x dV
    let sz = (PI * r * r * h * h + m * m * d2) / 2.0; // ∫ z dV
    let sxx = h * d2; // ∫ x² dV (odd terms vanish)
    let syy = h * d2;
    let szz = (PI * r * r * h.powi(3) + 3.0 * h * m * m * d2) / 3.0; // ∫ (h+mx)³/3
    let sxz = (2.0 * h * m * d2) / 2.0; // ∫ x (h+mx)²/2
    let _ = (d4, d22);
    let c = V3::new(sx / v, 0.0, sz / v);
    let cm = |a: f64, i: usize, j: usize| a - v * c[i] * c[j];
    let (xx, yy, zz, xz) = (cm(sxx, 0, 0), syy, cm(szz, 2, 2), cm(sxz, 0, 2));
    let inertia = Matrix3::new(yy + zz, 0.0, -xz, 0.0, xx + zz, 0.0, -xz, 0.0, xx + yy);
    let e = Expect {
        volume: v,
        area: 2.0 * PI * r * h + PI * r * r + PI * r * r * (1.0 + m * m).sqrt(),
        centroid: P3::from(c),
        inertia,
        size: h,
    };
    for seamed in [false, true] {
        let b = oblique_cylinder(r, h, m, seamed);
        let rep = cad_geom::validate(&b, &cad_geom::Tolerance::default());
        assert!(rep.is_valid(), "seamed {seamed}: {:?}", rep.issues);
        check(&format!("oblique cut [seamed {seamed}]"), &props(&b), &e);
    }
}

/// Upper hemisphere: a sphere face bounded by one equator circle (its loop winds once; the
/// residual winding sits at the north pole) plus a disc. V = 2πr³/3, z̄ = 3r/8,
/// I_zz = (2/5)·M r², I_xx = I_yy = (83/320)·M r² about the centroid.
#[test]
fn hemisphere_pole_residual() {
    use cad_geom::Curve;
    use std::f64::consts::TAU;
    let r = 4.0;
    let f = Frame { origin: P3::new(1.0, -2.0, 0.5), ..Frame::world() };
    let mut b = Brep::new();
    let s = b.add_solid();
    let sh = b.add_shell(Some(s));
    let v = b.add_vertex(f.origin + V3::x() * r);
    let c = b.add_edge(Some(Curve::Circle { frame: f, radius: r }), 0.0, TAU, v, v);
    let cap = b.add_face(sh, cad_geom::Surface::Sphere { frame: f, radius: r }, false);
    b.add_loop(cap, &[(c, false)]);
    let disc = b.add_face(sh, cad_geom::Surface::Plane { frame: f }, true);
    b.add_loop(disc, &[(c, true)]);
    let rep = cad_geom::validate(&b, &cad_geom::Tolerance::default());
    assert!(rep.is_valid(), "{:?}", rep.issues);
    let vol = 2.0 * PI * r.powi(3) / 3.0;
    let e = Expect {
        volume: vol,
        area: 3.0 * PI * r * r,
        centroid: f.origin + V3::z() * (3.0 * r / 8.0),
        inertia: axial(V3::z(), 83.0 / 320.0 * vol * r * r, 0.4 * vol * r * r),
        size: r,
    };
    check("hemisphere", &props(&b), &e);
    let a = face_area(&b, cap).unwrap();
    assert!((a - 2.0 * PI * r * r).abs() <= TOL * a);
}

/// Regression: a sphere face whose only loop closes in (u, v) without winding round the sphere's
/// axis, with the face *outside* it (a ball with a cap cut off sideways, so the face keeps both
/// poles), used to integrate to minus the cap instead of the sphere minus the cap.
#[test]
fn sphere_face_outside_a_non_winding_loop() {
    let (r, x0) = (10.0f64, 6.0f64);
    let rc = (r * r - x0 * x0).sqrt();
    let mut b = Brep::new();
    let solid = b.add_solid();
    let shell = b.add_shell(Some(solid));
    let cf = Frame::from_origin_z(P3::new(x0, 0.0, 0.0), V3::x());
    let v = b.add_vertex(cf.origin + cf.x * rc);
    let e = b.add_edge(Some(cad_geom::Curve::Circle { frame: cf, radius: rc }), 0.0, 2.0 * PI, v, v);
    // the circle runs counter-clockwise about +x, so the big part (x < 6) lies on its right
    let s = b.add_face(shell, cad_geom::Surface::Sphere { frame: Frame::world(), radius: r }, false);
    b.add_loop(s, &[(e, true)]);
    let d = b.add_face(shell, cad_geom::Surface::Plane { frame: cf }, false);
    b.add_loop(d, &[(e, false)]);
    let h = r - x0;
    let cap = PI * h * h * (3.0 * r - h) / 3.0;
    let m = solid_properties(&b, SolidId(0)).unwrap();
    assert!(((m.volume - (4.0 / 3.0 * PI * r.powi(3) - cap)) / m.volume).abs() < 1e-13, "{}", m.volume);
    let a = face_area(&b, s).unwrap();
    assert!(((a - (4.0 * PI * r * r - 2.0 * PI * r * h)) / a).abs() < 1e-13, "{a}");
    // centroid on the x axis, shifted away from the cut: −∫x dV / V = −(cap moment) / V
    assert!(m.centroid.y.abs() < 1e-12 && m.centroid.z.abs() < 1e-12 && m.centroid.x < 0.0);
    let rep = cad_geom::validate(&b, &cad_geom::Tolerance::default());
    assert!(rep.is_valid(), "{:?}", rep.issues);
}
