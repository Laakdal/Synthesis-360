//! Chamfer and fillet: every `fillet.*` / `chamfer.*` case of `research/feature-cases.json` on its
//! extruded base body, against closed forms (volume, area, centroid; Pappus for the circular
//! rims), validation at the ASM and OCCT tolerances, watertight tessellation, cross-checks
//! against extrudes of the equivalent rounded / cut profiles (convex and concave corners) and
//! the error cases.

#[path = "support/features.rs"]
mod features;

use cad_geom::blend::edge_at_point;
use cad_geom::mass::solid_properties;
use cad_geom::tess::tessellate_solid;
use cad_geom::{chamfer, extrude, fillet, validate, Brep, EdgeId, GeomError, MassProperties, Profile, Seg, SolidId, Tolerance, P2, P3, V3};
use features::profile;
use std::f64::consts::{FRAC_PI_2, PI};

fn base(id: &str) -> Brep {
    let c = features::case(id);
    let (from, to) = (c["from"].as_f64().unwrap(), c["to"].as_f64().unwrap());
    extrude(&profile(&c), from, to, &Tolerance::asm()).unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// Edges of a blend case, picked by the world points of the case.
fn picked(b: &Brep, points: &[P3]) -> Vec<EdgeId> {
    points.iter().map(|p| edge_at_point(b, p, &Tolerance::asm()).unwrap_or_else(|| panic!("no edge through {p:?}"))).collect()
}

/// Runs a `fillet.*` / `chamfer.*` case of the catalogue.
fn run(id: &str) -> Brep {
    let c = features::case(id);
    let b = base(c["base"].as_str().unwrap());
    let pts: Vec<P3> = c["edges"].as_array().unwrap().iter().map(features::p3).collect();
    let edges = picked(&b, &pts);
    let r = match c["kind"].as_str().unwrap() {
        "fillet" => fillet(&b, &edges, c["radius"].as_f64().unwrap(), &Tolerance::asm()),
        "chamfer" => chamfer(&b, &edges, c["distance"].as_f64().unwrap(), &Tolerance::asm()),
        k => panic!("{k}"),
    };
    r.unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn mass(b: &Brep) -> MassProperties {
    solid_properties(b, SolidId(0)).unwrap()
}

fn counts(b: &Brep) -> (usize, usize, usize, usize) {
    (b.faces.len(), b.edges.len(), b.vertices.len(), b.loops.len())
}

fn face_types(b: &Brep) -> Vec<&'static str> {
    let mut t: Vec<_> = b.faces.iter().map(|f| f.surface.type_name()).collect();
    t.sort_unstable();
    t
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-300)
}

fn assert_valid(id: &str, b: &Brep, genus: i64) {
    for tol in [Tolerance::asm(), Tolerance::occt()] {
        let r = validate(b, &tol);
        assert!(r.is_valid(), "{id} at {tol:?}: {:?}", r.issues);
        assert_eq!(r.genus, genus, "{id} genus");
    }
}

/// A solid made of a body of volume `v0` with centroid `c0` minus pieces (volume, centroid).
fn minus(v0: f64, c0: P3, pieces: &[(f64, P3)]) -> (f64, P3) {
    let mut v = v0;
    let mut m = c0.coords * v0;
    for (pv, pc) in pieces {
        v -= pv;
        m -= pc.coords * *pv;
    }
    (v, P3::from(m / v))
}

/// Area and centroid offset (from the corner, along each leg) of the region a fillet of radius r
/// removes from a right-angled corner: the square r×r minus the quarter disc.
fn spandrel(r: f64) -> (f64, f64) {
    (r * r * (1.0 - PI / 4.0), r * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI)))
}

/// (∫ρ dA, ∫ρ z dA) of the region a fillet of radius r removes at the corner (ρc, zc) of a
/// revolved section whose material extends from the corner along (sρ, sz) (each ±1): the
/// square r×r there minus the quarter disc about (ρc + sρ r, zc + sz r) facing the corner.
fn spandrel_moments(rho_c: f64, z_c: f64, s_rho: f64, s_z: f64, r: f64) -> (f64, f64) {
    let (mr, mz) = (rho_c + s_rho * r / 2.0, z_c + s_z * r / 2.0);
    let square = (r * r * mr, r * r * mr * mz);
    let (qr, qz) = (rho_c + s_rho * r, z_c + s_z * r);
    let quarter = PI * r * r / 4.0;
    let (fx, fz) = (-s_rho * r.powi(3) / 3.0, -s_z * r.powi(3) / 3.0);
    let disc = (qr * quarter + fx, qr * qz * quarter + qr * fz + qz * fx + s_rho * s_z * r.powi(4) / 8.0);
    (square.0 - disc.0, square.1 - disc.1)
}

/// (∫ρ dA, ∫ρ z dA) of a triangle in the (ρ, z) half-plane (exact for the bilinear integrand).
fn triangle_moments(p: [[f64; 2]; 3]) -> (f64, f64) {
    let a = 0.5 * ((p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1])).abs();
    let (sr, sz) = (p[0][0] + p[1][0] + p[2][0], p[0][1] + p[1][1] + p[2][1]);
    let srz: f64 = p.iter().map(|q| q[0] * q[1]).sum();
    (a * sr / 3.0, a / 12.0 * (srz + sr * sz))
}

/// Solid of revolution about the z axis through (x0, y0) swept by a section with the given
/// moments: (volume, centroid) by Pappus.
fn revolved((m_rho, m_rho_z): (f64, f64), x0: f64, y0: f64) -> (f64, P3) {
    (2.0 * PI * m_rho, P3::new(x0, y0, m_rho_z / m_rho))
}

fn check_mass(id: &str, b: &Brep, volume: f64, area: f64, centroid: P3) {
    let m = mass(b);
    assert!(rel(m.volume, volume) < 1e-12, "{id} volume {} vs {volume}", m.volume);
    assert!(rel(m.area, area) < 1e-12, "{id} area {} vs {area}", m.area);
    assert!((m.centroid - centroid).norm() < 1e-9, "{id} centroid {:?} vs {centroid:?}", m.centroid);
}

/// Watertight mesh whose volume is within 2·chord/r_min of the exact one (0 = polyhedral).
fn check_mesh(id: &str, b: &Brep, rmin: f64) {
    let chord = 0.01;
    let mesh = tessellate_solid(b, SolidId(0), chord).unwrap_or_else(|e| panic!("{id}: {e}"));
    features::assert_watertight(id, &mesh);
    let (v, exact) = (mesh.signed_volume(), mass(b).volume);
    let bound = if rmin == 0.0 { 1e-12 } else { 2.0 * chord / rmin };
    assert!(rel(v, exact) < bound, "{id}: mesh volume {v} vs {exact} (bound {bound:.1e})");
}

// ----- straight edges between planes -----

#[test]
fn chamfer_edge() {
    // box 50×30×10, top-front edge (y = 0, z = 10), d = 2. Topology: one chamfer plane (F 6+1);
    // E 12 − 1 + 2 trimmed lines + 2 end lines = 15; V 8 − 2 + 4 = 10; L = F = 7.
    let b = run("chamfer.edge");
    assert_eq!(counts(&b), (7, 15, 10, 7));
    assert_eq!(face_types(&b), vec!["plane"; 7]);
    assert_valid("chamfer.edge", &b, 0);
    let d = 2.0;
    let prism = (0.5 * d * d * 50.0, P3::new(25.0, d / 3.0, 10.0 - d / 3.0));
    let (v, c) = minus(15000.0, P3::new(25.0, 15.0, 5.0), &[prism]);
    assert!(rel(v, 14900.0) < 1e-15);
    // top and front lose a d-wide strip, the ends a triangle each, the chamfer adds d√2 × 50
    let area = 4600.0 - 2.0 * d * 50.0 - 2.0 * 0.5 * d * d + d * 2f64.sqrt() * 50.0;
    check_mass("chamfer.edge", &b, v, area, c);
    check_mesh("chamfer.edge", &b, 0.0);
}

#[test]
fn fillet_edge() {
    // same edge, r = 3: one cylinder face; same counts as the chamfer (the end edges are arcs)
    let b = run("fillet.edge");
    assert_eq!(counts(&b), (7, 15, 10, 7));
    let mut want = vec!["plane"; 6];
    want.push("cylinder");
    want.sort_unstable();
    assert_eq!(face_types(&b), want);
    assert_eq!(b.edges.iter().filter(|e| e.curve.as_ref().unwrap().type_name() == "circle").count(), 2);
    assert_valid("fillet.edge", &b, 0);
    let r = 3.0;
    let (a, off) = spandrel(r);
    let (v, c) = minus(15000.0, P3::new(25.0, 15.0, 5.0), &[(a * 50.0, P3::new(25.0, off, 10.0 - off))]);
    assert!(rel(v, 15000.0 - (1.0 - PI / 4.0) * 9.0 * 50.0) < 1e-15);
    assert!(rel(v, 14903.429173528853) < 1e-13, "OCCT reference");
    let area = 4600.0 - 2.0 * r * 50.0 - 2.0 * a + FRAC_PI_2 * r * 50.0;
    check_mass("fillet.edge", &b, v, area, c);
    check_mesh("fillet.edge", &b, r);
}

fn line(a: [f64; 2], b: [f64; 2]) -> Seg {
    Seg::Line { a: P2::new(a[0], a[1]), b: P2::new(b[0], b[1]) }
}

fn arc(c: [f64; 2], r: f64, a0_deg: f64, sweep_deg: f64) -> Seg {
    Seg::Arc { c: P2::new(c[0], c[1]), r, a0: a0_deg.to_radians(), sweep: sweep_deg.to_radians() }
}

fn xy_extrude(segs: Vec<Seg>, h: f64) -> Brep {
    let p = Profile { plane: cad_geom::Frame::world(), loops: vec![segs] };
    extrude(&p, 0.0, h, &Tolerance::asm()).unwrap()
}

/// Same topology counts, face types and exact mass properties (inertia included).
fn assert_same_solid(id: &str, a: &Brep, b: &Brep) {
    assert_eq!(counts(a), counts(b), "{id} F/E/V/L");
    assert_eq!(face_types(a), face_types(b), "{id} face types");
    let (ma, mb) = (mass(a), mass(b));
    assert!(rel(ma.volume, mb.volume) < 1e-12, "{id} volume {} vs {}", ma.volume, mb.volume);
    assert!(rel(ma.area, mb.area) < 1e-12, "{id} area {} vs {}", ma.area, mb.area);
    assert!((ma.centroid - mb.centroid).norm() < 1e-9, "{id} centroid {:?} vs {:?}", ma.centroid, mb.centroid);
    let scale = mb.inertia.iter().flatten().fold(0.0f64, |s, x| s.max(x.abs()));
    for i in 0..3 {
        for j in 0..3 {
            assert!((ma.inertia[i][j] - mb.inertia[i][j]).abs() < 1e-10 * scale, "{id} inertia {i}{j}");
        }
    }
}

/// Corners of the 50×30 rectangle with the directions pointing into it.
const RECT_CORNERS: [[f64; 4]; 4] = [[0.0, 0.0, 1.0, 1.0], [50.0, 0.0, -1.0, 1.0], [50.0, 30.0, -1.0, -1.0], [0.0, 30.0, 1.0, -1.0]];

#[test]
fn fillet_vertical4_equals_rounded_rectangle_extrude() {
    // four vertical edges (no shared vertex), r = 5: F 6+4, E 12 − 4 + 8 lines + 8 arcs = 24,
    // V 8 − 8 + 16 = 16, L 10 — the extrude of the rounded rectangle (8 segments) exactly
    let b = run("fillet.vertical4");
    assert_eq!(counts(&b), (10, 24, 16, 10));
    assert_valid("fillet.vertical4", &b, 0);
    let r = 5.0;
    let (a, off) = spandrel(r);
    let pieces: Vec<(f64, P3)> = RECT_CORNERS.iter().map(|c| (a * 10.0, P3::new(c[0] + c[2] * off, c[1] + c[3] * off, 5.0))).collect();
    let (v, c) = minus(15000.0, P3::new(25.0, 15.0, 5.0), &pieces);
    assert!(rel(v, (1500.0 - (4.0 - PI) * 25.0) * 10.0) < 1e-15);
    assert!(rel(v, 14785.398163397447) < 1e-13, "OCCT reference");
    let area = 4600.0 - 8.0 * a - 4.0 * 2.0 * r * 10.0 + 4.0 * FRAC_PI_2 * r * 10.0;
    check_mass("fillet.vertical4", &b, v, area, c);
    check_mesh("fillet.vertical4", &b, r);
    let rounded = xy_extrude(
        vec![
            line([5.0, 0.0], [45.0, 0.0]),
            arc([45.0, 5.0], 5.0, -90.0, 90.0),
            line([50.0, 5.0], [50.0, 25.0]),
            arc([45.0, 25.0], 5.0, 0.0, 90.0),
            line([45.0, 30.0], [5.0, 30.0]),
            arc([5.0, 25.0], 5.0, 90.0, 90.0),
            line([0.0, 25.0], [0.0, 5.0]),
            arc([5.0, 5.0], 5.0, 180.0, 90.0),
        ],
        10.0,
    );
    assert_same_solid("fillet.vertical4", &b, &rounded);
}

#[test]
fn chamfer_vertical4_equals_octagon_extrude() {
    let b = run("chamfer.vertical4");
    assert_eq!(counts(&b), (10, 24, 16, 10));
    assert_eq!(face_types(&b), vec!["plane"; 10]);
    assert_valid("chamfer.vertical4", &b, 0);
    let d = 5.0;
    let pieces: Vec<(f64, P3)> = RECT_CORNERS.iter().map(|c| (0.5 * d * d * 10.0, P3::new(c[0] + c[2] * d / 3.0, c[1] + c[3] * d / 3.0, 5.0))).collect();
    let (v, c) = minus(15000.0, P3::new(25.0, 15.0, 5.0), &pieces);
    assert!(rel(v, (1500.0 - 4.0 * 0.5 * 25.0) * 10.0) < 1e-15 && rel(v, 14500.0) < 1e-15);
    let area = 4600.0 - 8.0 * 0.5 * d * d - 4.0 * 2.0 * d * 10.0 + 4.0 * d * 2f64.sqrt() * 10.0;
    check_mass("chamfer.vertical4", &b, v, area, c);
    check_mesh("chamfer.vertical4", &b, 0.0);
    let octagon = xy_extrude(
        vec![
            line([5.0, 0.0], [45.0, 0.0]),
            line([45.0, 0.0], [50.0, 5.0]),
            line([50.0, 5.0], [50.0, 25.0]),
            line([50.0, 25.0], [45.0, 30.0]),
            line([45.0, 30.0], [5.0, 30.0]),
            line([5.0, 30.0], [0.0, 25.0]),
            line([0.0, 25.0], [0.0, 5.0]),
            line([0.0, 5.0], [5.0, 0.0]),
        ],
        10.0,
    );
    assert_same_solid("chamfer.vertical4", &b, &octagon);
}

#[test]
fn fillet_corner3_rolling_ball_corner() {
    // the three edges at the box corner (0, 0, 10), r = 3: three cylinders and a sphere octant
    // about (3, 3, 7) bounded by three quarter arcs. F 6 + 3 + 1 = 10; E 12 − 3 + 6 trimmed
    // lines + 3 end arcs at the far ends + 3 sphere arcs = 21; V 8 − 4 + 3·2 + 3 = 13; L 10.
    let b = run("fillet.corner3");
    assert_eq!(counts(&b), (10, 21, 13, 10));
    let mut want = vec!["plane"; 6];
    want.extend(["cylinder"; 3]);
    want.push("sphere");
    want.sort_unstable();
    assert_eq!(face_types(&b), want);
    let arcs = b.edges.iter().filter(|e| e.curve.as_ref().unwrap().type_name() == "circle").count();
    assert_eq!((arcs, b.edges.len() - arcs), (6, 15));
    assert_valid("fillet.corner3", &b, 0);
    let r = 3.0;
    let (a, off) = spandrel(r);
    // the corner cube r³ keeps only the ball octant; its removed part's centroid offset per axis
    let cube_cut = r.powi(3) * (1.0 - PI / 6.0);
    let oc = (r.powi(4) / 2.0 - PI * r.powi(3) / 6.0 * (r - 3.0 * r / 8.0)) / cube_cut;
    let pieces = [
        (a * (50.0 - r), P3::new((50.0 + r) / 2.0, off, 10.0 - off)),
        (a * (30.0 - r), P3::new(off, (30.0 + r) / 2.0, 10.0 - off)),
        (a * (10.0 - r), P3::new(off, off, (10.0 - r) / 2.0)),
        (cube_cut, P3::new(oc, oc, 10.0 - oc)),
    ];
    let (v, c) = minus(15000.0, P3::new(25.0, 15.0, 5.0), &pieces);
    assert!(rel(v, 14830.692428057893) < 1e-13, "OCCT reference {v}");
    let planes = (1500.0 - 80.0 * r + r * r) + (500.0 - 60.0 * r + r * r) + (300.0 - 40.0 * r + r * r) + (300.0 - a) + (500.0 - a) + (1500.0 - a);
    let area = planes + FRAC_PI_2 * r * (90.0 - 3.0 * r) + PI * r * r / 2.0;
    check_mass("fillet.corner3", &b, v, area, c);
    check_mesh("fillet.corner3", &b, r);
}

/// The L shape of `ext.lshape` with its inner (concave) corner at (10, 10) replaced by `corner`,
/// which runs from (10 + a, 10) to (10, 10 + a).
fn l_profile(corner: Vec<Seg>, a: f64) -> Vec<Seg> {
    let mut l = vec![line([0.0, 0.0], [40.0, 0.0]), line([40.0, 0.0], [40.0, 10.0]), line([40.0, 10.0], [10.0 + a, 10.0])];
    l.extend(corner);
    l.extend([line([10.0, 10.0 + a], [10.0, 30.0]), line([10.0, 30.0], [0.0, 30.0]), line([0.0, 30.0], [0.0, 0.0])]);
    l
}

#[test]
fn concave_edge_blends_equal_extrudes() {
    // the inner vertical edge of the L is concave: the blend adds material. F 8+1, E 18 + 3,
    // V 12 + 2, L 9, like the extrude of the L with that corner rounded (clockwise arc) / cut
    let l = base("ext.lshape");
    let e = picked(&l, &[P3::new(10.0, 10.0, 4.0)]);
    let r = 2.0;
    let f = fillet(&l, &e, r, &Tolerance::asm()).unwrap();
    assert_eq!(counts(&f), (9, 21, 14, 9));
    assert_valid("concave fillet", &f, 0);
    let (a, _) = spandrel(r);
    assert!(rel(mass(&f).volume, 4800.0 + a * 8.0) < 1e-12);
    // the fillet face is a cylinder with the material outside it: reversed
    let cyl = f.faces.iter().find(|x| x.surface.type_name() == "cylinder").unwrap();
    assert!(cyl.reversed);
    assert_same_solid("concave fillet", &f, &xy_extrude(l_profile(vec![arc([12.0, 12.0], r, -90.0, -90.0)], r), 8.0));
    check_mesh("concave fillet", &f, r);
    let c = chamfer(&l, &e, r, &Tolerance::asm()).unwrap();
    assert_valid("concave chamfer", &c, 0);
    assert!(rel(mass(&c).volume, 4800.0 + 0.5 * r * r * 8.0) < 1e-12);
    assert_same_solid("concave chamfer", &c, &xy_extrude(l_profile(vec![line([12.0, 10.0], [10.0, 12.0])], r), 8.0));
    check_mesh("concave chamfer", &c, 0.0);
}

/// Trapezoid prism: (0,0) (20,0) (15,10) (5,10), height 10.
fn trapezoid() -> Brep {
    xy_extrude(vec![line([0.0, 0.0], [20.0, 0.0]), line([20.0, 0.0], [15.0, 10.0]), line([15.0, 10.0], [5.0, 10.0]), line([5.0, 10.0], [0.0, 0.0])], 10.0)
}

#[test]
fn fillet_of_an_acute_edge_equals_rounded_trapezoid() {
    // vertical edge at (20, 0): interior angle θ = atan2(10, 5) between the faces; the fillet
    // touches both faces at r·cot(θ/2) from the edge and sweeps π − θ
    let t = trapezoid();
    let e = picked(&t, &[P3::new(20.0, 0.0, 5.0)]);
    let r = 2.0;
    let f = fillet(&t, &e, r, &Tolerance::asm()).unwrap();
    assert_eq!(counts(&f), (7, 15, 10, 7));
    assert_valid("acute fillet", &f, 0);
    let theta = 10f64.atan2(5.0);
    let s = r / (theta / 2.0).tan();
    let p = P3::new(20.0, 0.0, 0.0) + V3::new(-5.0, 10.0, 0.0).normalize() * s;
    let rounded = xy_extrude(
        vec![
            line([0.0, 0.0], [20.0 - s, 0.0]),
            arc([20.0 - s, r], r, -90.0, (PI - theta).to_degrees()),
            line([p.x, p.y], [15.0, 10.0]),
            line([15.0, 10.0], [5.0, 10.0]),
            line([5.0, 10.0], [0.0, 0.0]),
        ],
        10.0,
    );
    assert_same_solid("acute fillet", &f, &rounded);
    check_mesh("acute fillet", &f, r);
}

#[test]
fn chamfer_with_oblique_end_faces() {
    // bottom edge of the slanted face, from (20,0,0) to (15,10,0): its end faces (y = 0 and
    // y = 10) are oblique to it. The removed prism has the right-triangle section d²/2, and its
    // centroid line runs between the two end planes over the edge length √125.
    let t = trapezoid();
    let e = picked(&t, &[P3::new(17.5, 5.0, 0.0)]);
    let d = 1.0;
    let c = chamfer(&t, &e, d, &Tolerance::asm()).unwrap();
    assert_eq!(counts(&c), (7, 15, 10, 7));
    assert_valid("oblique chamfer", &c, 0);
    assert!(rel(mass(&c).volume, 1500.0 - 0.5 * d * d * 125f64.sqrt()) < 1e-12);
    check_mesh("oblique chamfer", &c, 0.0);
    // a fillet there would need elliptical end edges
    assert!(matches!(fillet(&t, &e, d, &Tolerance::asm()), Err(GeomError::Unsupported(_))));
}

// ----- circular rims between a plane and a cylinder -----

#[test]
fn fillet_cylinder_top_by_pappus() {
    // ext.circle (R 10, H 20), top rim, r = 2: a torus quarter (major 8, minor 2) between a
    // circle of radius 10 at z = 18 on the side and one of radius 8 on the top. F 3+1; E: bottom
    // circle + 2 new circles = 3; V 3 (one per circle); L: side 2, caps 1+1, torus 2 = 6.
    let b = run("fillet.cyl_top");
    assert_eq!(counts(&b), (4, 3, 3, 6));
    assert_eq!(face_types(&b), vec!["cylinder", "plane", "plane", "torus"]);
    assert_valid("fillet.cyl_top", &b, 0);
    let (rr, h, r) = (10.0, 20.0, 2.0);
    let (a, off) = spandrel(r);
    let removed = revolved(spandrel_moments(rr, h, -1.0, -1.0, r), 0.0, 0.0);
    assert!(rel(removed.0, 2.0 * PI * (rr - off) * a) < 1e-14, "Pappus with the section centroid");
    let (v, c) = minus(PI * rr * rr * h, P3::new(0.0, 0.0, h / 2.0), &[removed]);
    assert!(rel(v, 6231.6594673576965) < 1e-13, "OCCT reference {v}");
    let area = PI * rr * rr + PI * (rr - r) * (rr - r) + 2.0 * PI * rr * (h - r) + 2.0 * PI * (rr - r + 2.0 * r / PI) * (FRAC_PI_2 * r);
    check_mass("fillet.cyl_top", &b, v, area, c);
    check_mesh("fillet.cyl_top", &b, r);
}

#[test]
fn chamfer_cylinder_top_by_pappus() {
    let b = run("chamfer.cyl_top");
    assert_eq!(counts(&b), (4, 3, 3, 6));
    assert_eq!(face_types(&b), vec!["cone", "cylinder", "plane", "plane"]);
    assert_valid("chamfer.cyl_top", &b, 0);
    let (rr, h, d) = (10.0, 20.0, 2.0);
    let removed = revolved(triangle_moments([[rr, h], [rr - d, h], [rr, h - d]]), 0.0, 0.0);
    assert!(rel(removed.0, 2.0 * PI * (rr - d / 3.0) * 0.5 * d * d) < 1e-14);
    let (v, c) = minus(PI * rr * rr * h, P3::new(0.0, 0.0, h / 2.0), &[removed]);
    assert!(rel(v, 6165.899181445567) < 1e-13, "OCCT reference {v}");
    let area = PI * rr * rr + PI * (rr - d) * (rr - d) + 2.0 * PI * rr * (h - d) + PI * (rr + rr - d) * d * 2f64.sqrt();
    check_mass("chamfer.cyl_top", &b, v, area, c);
    check_mesh("chamfer.cyl_top", &b, rr - d);
}

#[test]
fn fillet_hole_rim_by_pappus() {
    // ext.rect_hole (7/14/10/10, genus 1), top rim of the Ø12 hole, r = 1: the torus quarter
    // facing the hole axis (major 7, minor 1, v ∈ [π/2, π]); the rim itself is a convex 90°
    // material corner. F 8, E 15, V 11, L 12.
    let b = run("fillet.hole_edge");
    assert_eq!(counts(&b), (8, 15, 11, 12));
    assert_valid("fillet.hole_edge", &b, 1);
    let tor = b.faces.iter().find(|f| f.surface.type_name() == "torus").unwrap();
    assert!(!tor.reversed, "convex rim: material on the tube-centre side");
    let (rh, r) = (6.0, 1.0);
    let (a, off) = spandrel(r);
    let removed = revolved(spandrel_moments(rh, 10.0, 1.0, -1.0, r), 25.0, 15.0);
    assert!(rel(removed.0, 2.0 * PI * (rh + off) * a) < 1e-14);
    let (v, c) = minus((1500.0 - 36.0 * PI) * 10.0, P3::new(25.0, 15.0, 5.0), &[removed]);
    assert!(rel(v, 13860.635160512426) < 1e-13, "OCCT reference {v}");
    let base_area = 2.0 * (1500.0 - 36.0 * PI) + 1600.0 + 120.0 * PI;
    let area = base_area - PI * ((rh + r) * (rh + r) - rh * rh) - 2.0 * PI * rh * r + 2.0 * PI * (rh + r - 2.0 * r / PI) * (FRAC_PI_2 * r);
    check_mass("fillet.hole_edge", &b, v, area, c);
    check_mesh("fillet.hole_edge", &b, r);
}

#[test]
fn chamfer_hole_rim_and_fillet_bottom_rim() {
    // the hole rim chamfered (a cone widening upwards, material outside it: reversed), and the
    // bottom rim of the cylinder filleted (torus quarter below the centre circle)
    let h = base("ext.rect_hole");
    let e = picked(&h, &[P3::new(31.0, 15.0, 10.0)]);
    let c = chamfer(&h, &e, 1.0, &Tolerance::asm()).unwrap();
    assert_eq!(counts(&c), (8, 15, 11, 12));
    assert_valid("hole chamfer", &c, 1);
    assert!(c.faces.iter().find(|f| f.surface.type_name() == "cone").unwrap().reversed);
    let removed = 2.0 * PI * (6.0 + 1.0 / 3.0) * 0.5;
    assert!(rel(mass(&c).volume, (1500.0 - 36.0 * PI) * 10.0 - removed) < 1e-12);
    check_mesh("hole chamfer", &c, 6.0);
    let cyl = base("ext.circle");
    let e = picked(&cyl, &[P3::new(10.0, 0.0, 0.0)]);
    let f = fillet(&cyl, &e, 2.0, &Tolerance::asm()).unwrap();
    assert_eq!(counts(&f), (4, 3, 3, 6));
    assert_valid("bottom fillet", &f, 0);
    let (a, off) = spandrel(2.0);
    let m = mass(&f);
    let removed = 2.0 * PI * (10.0 - off) * a;
    assert!(rel(m.volume, 2000.0 * PI - removed) < 1e-12);
    let (rv, rc) = revolved(spandrel_moments(10.0, 0.0, -1.0, 1.0, 2.0), 0.0, 0.0);
    assert!(rel(rv, removed) < 1e-14);
    let (_, c) = minus(2000.0 * PI, P3::new(0.0, 0.0, 10.0), &[(rv, rc)]);
    assert!((m.centroid - c).norm() < 1e-9, "{:?} vs {c:?}", m.centroid);
    check_mesh("bottom fillet", &f, 2.0);
}

/// Regression (tess): a curved face bounded by two closed circles whose neighbours need
/// different sample counts — here a cylinder band between a plane cap (bottom) and a hemisphere
/// (top, finer grid) — must still mesh watertight: the cylinder grid and both circles have to
/// agree on one count. Built directly, without any blend; the torus and cone bands of the rim
/// blends hit the same case.
#[test]
fn capsule_tessellates_watertight() {
    use cad_geom::{Curve, Frame, Surface};
    let (r, h) = (10.0, 20.0);
    let mut b = Brep::new();
    let solid = b.add_solid();
    let shell = b.add_shell(Some(solid));
    let ring = |b: &mut Brep, z: f64| {
        let frame = Frame { origin: P3::new(0.0, 0.0, z), ..Frame::world() };
        let v = b.add_vertex(frame.origin + frame.x * r);
        b.add_edge(Some(Curve::Circle { frame, radius: r }), 0.0, 2.0 * PI, v, v)
    };
    let (c0, c1) = (ring(&mut b, 0.0), ring(&mut b, h));
    let side = b.add_face(shell, Surface::Cylinder { frame: Frame::world(), radius: r }, false);
    b.add_loop(side, &[(c0, false)]);
    b.add_loop(side, &[(c1, true)]);
    let cap = b.add_face(shell, Surface::Plane { frame: Frame::world() }, true);
    b.add_loop(cap, &[(c0, true)]);
    let dome = b.add_face(shell, Surface::Sphere { frame: Frame { origin: P3::new(0.0, 0.0, h), ..Frame::world() }, radius: r }, false);
    b.add_loop(dome, &[(c1, false)]);
    assert_valid("capsule", &b, 0);
    assert!(rel(mass(&b).volume, PI * r * r * h + 2.0 / 3.0 * PI * r.powi(3)) < 1e-12);
    check_mesh("capsule", &b, r);
}

/// Every blend case against the ASM record of the same feature (`research/evidence/asm-v6`,
/// Fusion probe v6): F/E/V/L, sorted face and edge types (ASM calls a partial circle "arc"),
/// volume, area and centroid.
#[test]
fn blend_cases_match_asm_record() {
    let asm = features::load("research/evidence/asm-v6/reference.json");
    let ids = ["fillet.edge", "fillet.vertical4", "fillet.corner3", "fillet.cyl_top", "fillet.hole_edge", "chamfer.edge", "chamfer.vertical4", "chamfer.cyl_top"];
    for id in ids {
        let a = asm["cases"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap_or_else(|| panic!("{id} not recorded"));
        let b = run(id);
        let u = |k: &str| a[k].as_u64().unwrap() as usize;
        assert_eq!(counts(&b), (u("faces"), u("edges"), u("vertices"), u("loops")), "{id} F/E/V/L");
        let sorted = |k: &str| {
            let mut s: Vec<String> = a[k].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
            s.sort();
            s
        };
        let ft: Vec<String> = face_types(&b).into_iter().map(String::from).collect();
        assert_eq!(ft, sorted("face_types"), "{id} face types");
        let mut et: Vec<String> = b
            .edges
            .iter()
            .map(|e| match e.curve.as_ref().unwrap().type_name() {
                "circle" if e.t1 - e.t0 < 2.0 * PI - 1e-9 => "arc".to_string(),
                t => t.to_string(),
            })
            .collect();
        et.sort();
        assert_eq!(et, sorted("edge_types"), "{id} edge types");
        let m = mass(&b);
        let num = |k: &str| a[k].as_f64().unwrap();
        assert!(rel(m.volume, num("volume")) < 1e-9, "{id} volume {} vs ASM {}", m.volume, num("volume"));
        assert!(rel(m.area, num("area")) < 1e-9, "{id} area {} vs ASM {}", m.area, num("area"));
        assert!((m.centroid - features::p3(&a["centroid"])).norm() < 1e-6, "{id} centroid {:?} vs ASM {}", m.centroid, a["centroid"]);
    }
}

// ----- edge picking and errors -----

#[test]
fn edges_are_picked_by_world_points() {
    let b = base("ext.rect");
    let tol = Tolerance::asm();
    let e = edge_at_point(&b, &P3::new(25.0, 0.0, 10.0), &tol).unwrap();
    let mut ends = [b.vertex(b.edge(e).start).point, b.vertex(b.edge(e).end).point];
    ends.sort_by(|p, q| p.x.total_cmp(&q.x));
    assert_eq!(ends, [P3::new(0.0, 0.0, 10.0), P3::new(50.0, 0.0, 10.0)]);
    assert!(edge_at_point(&b, &P3::new(25.0, 15.0, 10.0), &tol).is_none(), "face interior");
    assert!(edge_at_point(&b, &P3::new(60.0, 0.0, 10.0), &tol).is_none(), "beyond the edge's end");
    let c = base("ext.circle");
    assert!(edge_at_point(&c, &P3::new(0.0, -10.0, 20.0), &tol).is_some(), "on the top circle");
    assert!(edge_at_point(&c, &P3::new(0.0, -10.0, 10.0), &tol).is_none(), "on the side face only");
}

#[test]
fn blends_reject_bad_input() {
    let tol = Tolerance::asm();
    let b = base("ext.rect");
    let top_front = picked(&b, &[P3::new(25.0, 0.0, 10.0)]);
    let inv = |r: Result<Brep, GeomError>| matches!(r, Err(GeomError::InvalidInput(_)));
    let uns = |r: Result<Brep, GeomError>| matches!(r, Err(GeomError::Unsupported(_)));
    for s in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(inv(fillet(&b, &top_front, s, &tol)), "size {s}");
        assert!(inv(chamfer(&b, &top_front, s, &tol)), "size {s}");
    }
    assert!(inv(fillet(&b, &[], 1.0, &tol)));
    assert!(inv(fillet(&b, &[EdgeId(99)], 1.0, &tol)));
    // the front face is 10 high: r = 10 consumes it, d = 12 overshoots
    assert!(inv(fillet(&b, &top_front, 10.0, &tol)));
    assert!(inv(chamfer(&b, &top_front, 12.0, &tol)));
    // four vertical edges with r = 15 make the 30-long sides vanish
    let v4 = picked(&b, &[P3::new(0.0, 0.0, 5.0), P3::new(50.0, 0.0, 5.0), P3::new(50.0, 30.0, 5.0), P3::new(0.0, 30.0, 5.0)]);
    assert!(inv(fillet(&b, &v4, 15.0, &tol)));
    assert!(fillet(&b, &v4, 14.9, &tol).is_ok());
    // two edges sharing a vertex
    let two = picked(&b, &[P3::new(25.0, 0.0, 10.0), P3::new(0.0, 15.0, 10.0)]);
    assert!(uns(fillet(&b, &two, 1.0, &tol)));
    assert!(uns(chamfer(&b, &two, 1.0, &tol)));
    // three edges at a corner: the chamfer is unsupported
    let corner = picked(&b, &[P3::new(25.0, 0.0, 10.0), P3::new(0.0, 15.0, 10.0), P3::new(0.0, 0.0, 5.0)]);
    assert!(uns(chamfer(&b, &corner, 1.0, &tol)));
    // cylinder rim: r ≥ R consumes the top; R/2 ≤ r < R would need a spindle torus
    let c = base("ext.circle");
    let rim = picked(&c, &[P3::new(10.0, 0.0, 20.0)]);
    assert!(inv(fillet(&c, &rim, 10.0, &tol)));
    assert!(inv(chamfer(&c, &rim, 10.0, &tol)));
    assert!(uns(fillet(&c, &rim, 5.0, &tol)));
    assert!(chamfer(&c, &rim, 5.0, &tol).is_ok());
    // a disc 3 high: a chamfer of 5 leaves the top a radius of 5 but consumes the side face
    let disc = xy_extrude(vec![Seg::circle(P2::origin(), 10.0)], 3.0);
    let rim = picked(&disc, &[P3::new(10.0, 0.0, 3.0)]);
    assert!(chamfer(&disc, &rim, 2.9, &tol).is_ok());
    assert!(inv(chamfer(&disc, &rim, 3.0, &tol)));
    assert!(inv(chamfer(&disc, &rim, 5.0, &tol)));
    // a small through hole lying wholly inside the strip the fillet trims off the top face
    let p = Profile { plane: cad_geom::Frame::world(), loops: vec![features::profile(&features::case("ext.rect")).loops[0].clone(), vec![Seg::circle(P2::new(25.0, 2.0), 0.5)]] };
    let plate = extrude(&p, 0.0, 10.0, &tol).unwrap();
    let tf = picked(&plate, &[P3::new(10.0, 0.0, 10.0)]);
    assert!(fillet(&plate, &tf, 1.0, &tol).is_ok());
    assert!(inv(fillet(&plate, &tf, 2.0, &tol)), "the fillet strip crosses the hole");
    assert!(inv(fillet(&plate, &tf, 3.0, &tol)), "the hole lies inside the trimmed strip");
    // a fillet of the plate's top-front edge reaching the hole (hole min y = 9)
    let h = base("ext.rect_hole");
    let tf = picked(&h, &[P3::new(25.0, 0.0, 10.0)]);
    assert!(fillet(&h, &tf, 8.9, &tol).is_ok());
    assert!(inv(fillet(&h, &tf, 9.5, &tol)), "blend runs into the hole");
    assert!(inv(chamfer(&h, &tf, 9.5, &tol)), "blend runs into the hole");
}
