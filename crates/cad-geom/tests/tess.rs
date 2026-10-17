//! Tessellation of trimmed faces: plane faces with holes (from the planar booleans and hand-built
//! touching/pinched loops) and curved faces whose loops are not the full parameter rectangle
//! (half cylinder, half cone, hemisphere, spherical cap, sphere minus a cap, sphere ∩ cube,
//! obliquely cut cylinder). Every closed result is checked for watertightness (after welding
//! coincident positions, every directed edge occurs exactly once and so does its reverse),
//! outward winding and normals, volume (exact for planar solids, converging for curved ones) and
//! the chord bound.

use cad_geom::mass::{face_area, solid_properties};
use cad_geom::param::surface_distance;
use cad_geom::primitives::make_box;
use cad_geom::tess::{tessellate_face, tessellate_solid, Mesh};
use cad_geom::{boolean, validate, BoolOp, Brep, Curve, FaceId, Frame, SolidId, Surface, Tolerance, P3, V3};
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn bx(c: [f64; 3], s: [f64; 3]) -> Brep {
    make_box(P3::from(c), s).unwrap()
}

fn run(a: &Brep, b: &Brep, op: BoolOp) -> Brep {
    let r = boolean(a, b, op, &Tolerance::asm()).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    let rep = validate(&r, &Tolerance::default());
    assert!(rep.is_valid(), "{op:?}: {:?}", rep.issues);
    r
}

fn exact_volume(b: &Brep) -> f64 {
    b.solid_ids().map(|s| solid_properties(b, s).unwrap().volume).sum()
}

fn mesh_all(b: &Brep, chord: f64) -> Mesh {
    let mut m = Mesh::default();
    for s in b.solid_ids() {
        m.append(&tessellate_solid(b, s, chord).unwrap());
    }
    m
}

/// Welds positions closer than 1e-9·size (hash grid, neighbouring cells searched).
fn weld(m: &Mesh) -> Vec<usize> {
    let size = m.positions.iter().map(|p| V3::from(*p).amax()).fold(1.0, f64::max);
    let tol = 1e-9 * size;
    let cell = |p: &[f64; 3]| [(p[0] / tol).floor() as i64, (p[1] / tol).floor() as i64, (p[2] / tol).floor() as i64];
    let mut grid: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
    let mut reps: Vec<usize> = Vec::with_capacity(m.positions.len());
    for (i, p) in m.positions.iter().enumerate() {
        let c = cell(p);
        let mut found = None;
        'search: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) {
                        for &j in list {
                            if (V3::from(m.positions[j]) - V3::from(*p)).norm() <= tol {
                                found = Some(reps[j]);
                                break 'search;
                            }
                        }
                    }
                }
            }
        }
        reps.push(found.unwrap_or(i));
        grid.entry(c).or_default().push(i);
    }
    reps
}

/// Directed welded edge → number of uses.
fn edge_uses(m: &Mesh) -> HashMap<(usize, usize), usize> {
    let w = weld(m);
    let mut edges = HashMap::new();
    for t in &m.triangles {
        let v = [w[t[0] as usize], w[t[1] as usize], w[t[2] as usize]];
        assert!(v[0] != v[1] && v[1] != v[2] && v[2] != v[0], "triangle {t:?} collapses after welding");
        for k in 0..3 {
            *edges.entry((v[k], v[(k + 1) % 3])).or_insert(0) += 1;
        }
    }
    edges
}

/// Closed and consistently oriented: every directed edge once, its reverse once.
fn assert_watertight(id: &str, m: &Mesh) {
    let edges = edge_uses(m);
    for (&(a, c), &n) in &edges {
        assert_eq!(n, 1, "{id}: directed edge {a}->{c} used {n} times");
        assert_eq!(edges.get(&(c, a)).copied().unwrap_or(0), 1, "{id}: edge {a}->{c} has no twin");
    }
}

/// Unit normals that agree with the triangle winding.
fn assert_normals(id: &str, m: &Mesh) {
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

/// Largest distance from the face's surface over each triangle's centroid and edge midpoints,
/// and over a finer barycentric sampling.
fn chord_errors(b: &Brep, face: FaceId, m: &Mesh) -> (f64, f64) {
    let s = &b.face(face).surface;
    let (mut at_checks, mut anywhere) = (0.0f64, 0.0f64);
    for t in &m.triangles {
        let p = |i: u32| V3::from(m.positions[i as usize]);
        let (a, bb, c) = (p(t[0]), p(t[1]), p(t[2]));
        for q in [(a + bb + c) / 3.0, (a + bb) / 2.0, (bb + c) / 2.0, (c + a) / 2.0] {
            at_checks = at_checks.max(surface_distance(s, &P3::from(q)).unwrap());
        }
        for i in 0..=6 {
            for j in 0..=6 - i {
                let (x, y) = (i as f64 / 6.0, j as f64 / 6.0);
                let q = a + (bb - a) * x + (c - a) * y;
                anywhere = anywhere.max(surface_distance(s, &P3::from(q)).unwrap());
            }
        }
    }
    (at_checks, anywhere)
}

/// Plane faces: exact triangle count n + 2h − 2 (n boundary vertices, h holes, all distinct) and
/// exact area.
fn check_plane_faces(id: &str, b: &Brep) {
    for f in b.face_ids() {
        if !matches!(b.face(f).surface, Surface::Plane { .. }) {
            continue;
        }
        let m = tessellate_face(b, f, 0.1).unwrap();
        let n: usize = b.face(f).loops.iter().map(|&l| b.lp(l).coedges.len()).sum();
        let h = b.face(f).loops.len() - 1;
        assert_eq!(m.triangles.len(), n + 2 * h - 2, "{id}: face F{} ({} loops)", f.0, h + 1);
        let a = face_area(b, f).unwrap();
        assert!((m.area() - a).abs() <= 1e-12 * a, "{id}: face F{} area {} vs {a}", f.0, m.area());
    }
}

fn check_planar_solid(id: &str, b: &Brep, exact: f64) {
    let m = mesh_all(b, 0.1);
    assert_watertight(id, &m);
    assert_normals(id, &m);
    let v = m.signed_volume();
    assert!((v - exact).abs() <= 1e-9 * exact, "{id}: mesh volume {v} vs exact {exact}");
    assert!((exact_volume(b) - exact).abs() <= 1e-9 * exact, "{id}: B-rep volume {} vs {exact}", exact_volume(b));
    check_plane_faces(id, b);
}

#[test]
fn box_with_through_hole() {
    let r = run(&bx([0.0; 3], [30.0, 20.0, 10.0]), &bx([10.0, 5.0, -5.0], [10.0, 10.0, 20.0]), BoolOp::Cut);
    assert_eq!(r.faces.iter().filter(|f| f.loops.len() == 2).count(), 2, "top and bottom faces carry the hole");
    check_planar_solid("bool.cut.coplanar_through", &r, 6000.0 - 1000.0);
}

#[test]
fn l_shaped_union() {
    let r = run(&bx([0.0; 3], [20.0, 10.0, 10.0]), &bx([0.0; 3], [10.0, 20.0, 10.0]), BoolOp::Union);
    assert_eq!(r.faces.len(), 8);
    check_planar_solid("bool.union.l_shape", &r, 3000.0);
}

#[test]
fn face_with_three_holes() {
    let mut r = bx([0.0; 3], [40.0, 20.0, 10.0]);
    for x in [5.0, 17.0, 30.0] {
        r = run(&r, &bx([x, 7.5, -5.0], [5.0, 5.0, 20.0]), BoolOp::Cut);
    }
    assert_eq!(r.faces.iter().filter(|f| f.loops.len() == 4).count(), 2);
    check_planar_solid("bool.cut.three_holes", &r, 8000.0 - 3.0 * 250.0);
}

#[test]
fn hole_inside_hole_bridging_order() {
    // holes at different heights and overlapping x ranges (exercises Eberly's ordering and the
    // reflex-vertex search), plus a non-convex outer loop
    let mut r = run(&bx([0.0; 3], [40.0, 30.0, 10.0]), &bx([20.0, 15.0, -1.0], [30.0, 30.0, 12.0]), BoolOp::Cut);
    for (x, y) in [(3.0, 3.0), (6.0, 20.0), (12.0, 10.0), (25.0, 4.0)] {
        r = run(&r, &bx([x, y, -5.0], [4.0, 3.0, 20.0]), BoolOp::Cut);
    }
    let exact = 40.0 * 30.0 * 10.0 - 20.0 * 15.0 * 10.0 - 4.0 * 12.0 * 10.0;
    check_planar_solid("bool.cut.four_holes_l", &r, exact);
}

/// One plane face z = 0 (outward +z): the square [0, 10]² with a diamond hole whose left
/// corner is the vertex (0, 5) of the outer loop. `pinched`: one loop visiting that vertex twice;
/// otherwise two loops sharing the vertex.
fn touching_hole_face(pinched: bool) -> (Brep, FaceId) {
    let mut b = Brep::new();
    let s = b.add_solid();
    let sh = b.add_shell(Some(s));
    let v: Vec<_> = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 5.0], [3.0, 7.0], [6.0, 5.0], [3.0, 3.0]]
        .iter()
        .map(|p| b.add_vertex(P3::new(p[0], p[1], 0.0)))
        .collect();
    let f = b.add_face(sh, Surface::Plane { frame: Frame::world() }, false);
    let mut line = |i: usize, j: usize| (b.add_line_edge(v[i], v[j]), false);
    let outer = [line(0, 1), line(1, 2), line(2, 3), line(3, 4), line(4, 0)];
    // the hole clockwise: (0,5) → (3,7) → (6,5) → (3,3)
    let hole = [line(4, 5), line(5, 6), line(6, 7), line(7, 4)];
    if pinched {
        let uses = [outer[0], outer[1], outer[2], outer[3], hole[0], hole[1], hole[2], hole[3], outer[4]];
        b.add_loop(f, &uses);
    } else {
        b.add_loop(f, &outer);
        b.add_loop(f, &hole);
    }
    (b, f)
}

#[test]
fn hole_touching_outer_loop_at_a_vertex() {
    for pinched in [false, true] {
        let (b, f) = touching_hole_face(pinched);
        let m = tessellate_face(&b, f, 0.1).unwrap();
        let id = if pinched { "pinched loop" } else { "two loops" };
        // V = 8 distinct vertices, L = 9 boundary edges, Euler characteristic 0: T = 2V − L = 7
        assert_eq!(m.positions.len(), 8, "{id}: shared vertex must be one mesh vertex");
        assert_eq!(m.triangles.len(), 7, "{id}");
        assert!((m.area() - 88.0).abs() < 1e-12, "{id}: area {}", m.area());
        assert_normals(id, &m);
        // boundary edges once (in loop direction), interior edges twice with opposite directions
        let edges = edge_uses(&m);
        let boundary: Vec<(usize, usize)> = b
            .face_coedges(f)
            .map(|c| {
                let (p, q) = b.coedge_points(c);
                let idx = |x: P3| m.positions.iter().position(|y| P3::from(*y) == x).unwrap();
                (idx(p), idx(q))
            })
            .collect();
        for (&(a, c), &n) in &edges {
            assert_eq!(n, 1, "{id}: {a}->{c}");
            let twin = edges.contains_key(&(c, a));
            assert!(twin != boundary.contains(&(a, c)), "{id}: edge {a}->{c} boundary/interior mismatch");
        }
        assert_eq!(boundary.iter().filter(|e| edges.contains_key(e)).count(), 9, "{id}");
    }
}

#[test]
fn collinear_boundary_points_give_no_slivers() {
    // a plane face whose outer loop has collinear vertices on every side and a hole whose
    // rightmost vertex lies level with an outer vertex
    let mut b = Brep::new();
    let s = b.add_solid();
    let sh = b.add_shell(Some(s));
    let outer_pts = [[0.0, 0.0], [2.0, 0.0], [4.0, 0.0], [6.0, 0.0], [6.0, 2.0], [6.0, 4.0], [3.0, 4.0], [0.0, 4.0], [0.0, 2.0]];
    let hole_pts = [[2.0, 1.0], [2.0, 3.0], [4.0, 3.0], [4.0, 2.0], [4.0, 1.0]];
    let f = b.add_face(sh, Surface::Plane { frame: Frame::world() }, false);
    for ring in [&outer_pts[..], &hole_pts[..]] {
        let vs: Vec<_> = ring.iter().map(|p| b.add_vertex(P3::new(p[0], p[1], 0.0))).collect();
        let uses: Vec<_> = (0..vs.len()).map(|i| (b.add_line_edge(vs[i], vs[(i + 1) % vs.len()]), false)).collect();
        b.add_loop(f, &uses);
    }
    let m = tessellate_face(&b, f, 0.1).unwrap();
    assert_eq!(m.triangles.len(), 9 + 5 + 2 - 2);
    assert!((m.area() - 20.0).abs() < 1e-12, "area {}", m.area());
    let p = |i: u32| V3::from(m.positions[i as usize]);
    for t in &m.triangles {
        assert!((p(t[1]) - p(t[0])).cross(&(p(t[2]) - p(t[0]))).z > 1e-9, "zero-area triangle {t:?}");
    }
}

// ----- hand-built solids with trimmed curved faces -----

fn solid() -> (Brep, cad_geom::ShellId) {
    let mut b = Brep::new();
    let s = b.add_solid();
    let sh = b.add_shell(Some(s));
    (b, sh)
}

/// Half cylinder y ≥ 0 of radius r, height h: cylinder face u ∈ [0, π] bounded by two half
/// arcs and two lines, the flat face y = 0 and two half-disk caps.
fn half_cylinder(r: f64, h: f64) -> Brep {
    let (mut b, sh) = solid();
    let top = Frame { origin: P3::new(0.0, 0.0, h), ..Frame::world() };
    let a = b.add_vertex(P3::new(r, 0.0, 0.0));
    let bb = b.add_vertex(P3::new(-r, 0.0, 0.0));
    let c = b.add_vertex(P3::new(r, 0.0, h));
    let d = b.add_vertex(P3::new(-r, 0.0, h));
    let arc0 = b.add_edge(Some(Curve::Circle { frame: Frame::world(), radius: r }), 0.0, PI, a, bb);
    let arc1 = b.add_edge(Some(Curve::Circle { frame: top, radius: r }), 0.0, PI, c, d);
    let l1 = b.add_line_edge(a, c);
    let l2 = b.add_line_edge(bb, d);
    let m0 = b.add_line_edge(bb, a);
    let m1 = b.add_line_edge(c, d);
    let cyl = b.add_face(sh, Surface::Cylinder { frame: Frame::world(), radius: r }, false);
    b.add_loop(cyl, &[(arc0, false), (l2, false), (arc1, true), (l1, true)]);
    let flat = b.add_face(sh, Surface::Plane { frame: Frame::from_origin_z_x(P3::origin(), -V3::y(), V3::x()) }, false);
    b.add_loop(flat, &[(m0, false), (l1, false), (m1, false), (l2, true)]);
    let bot = b.add_face(sh, Surface::Plane { frame: Frame::world() }, true);
    b.add_loop(bot, &[(arc0, true), (m0, true)]);
    let tp = b.add_face(sh, Surface::Plane { frame: top }, false);
    b.add_loop(tp, &[(arc1, false), (m1, true)]);
    b
}

/// Half of a pointed cone (base radius r at z = 0, apex at z = h), y ≥ 0: the cone face is
/// bounded by a half arc and two generators meeting at the apex.
fn half_cone(r: f64, h: f64) -> Brep {
    let (mut b, sh) = solid();
    let a = b.add_vertex(P3::new(r, 0.0, 0.0));
    let bb = b.add_vertex(P3::new(-r, 0.0, 0.0));
    let t = b.add_vertex(P3::new(0.0, 0.0, h));
    let arc = b.add_edge(Some(Curve::Circle { frame: Frame::world(), radius: r }), 0.0, PI, a, bb);
    let bt = b.add_line_edge(bb, t);
    let ta = b.add_line_edge(t, a);
    let m = b.add_line_edge(a, bb);
    let cone = b.add_face(sh, Surface::Cone { frame: Frame::world(), radius: r, half_angle: (-r / h).atan() }, false);
    b.add_loop(cone, &[(arc, false), (bt, false), (ta, false)]);
    let flat = b.add_face(sh, Surface::Plane { frame: Frame::from_origin_z_x(P3::origin(), -V3::y(), V3::x()) }, false);
    b.add_loop(flat, &[(m, true), (ta, true), (bt, true)]);
    let base = b.add_face(sh, Surface::Plane { frame: Frame::world() }, true);
    b.add_loop(base, &[(m, false), (arc, true)]);
    b
}

/// A full circle edge of radius rho about `frame` (vertex at frame.x).
fn ring(b: &mut Brep, frame: Frame, rho: f64) -> cad_geom::EdgeId {
    let v = b.add_vertex(frame.origin + frame.x * rho);
    b.add_edge(Some(Curve::Circle { frame, radius: rho }), 0.0, TAU, v, v)
}

/// Upper hemisphere: sphere face bounded by the equator (winds once round the axis; the north
/// pole closes it) and the base disk.
fn hemisphere(r: f64) -> Brep {
    let (mut b, sh) = solid();
    let eq = ring(&mut b, Frame::world(), r);
    let sp = b.add_face(sh, Surface::Sphere { frame: Frame::world(), radius: r }, false);
    b.add_loop(sp, &[(eq, false)]);
    let base = b.add_face(sh, Surface::Plane { frame: Frame::world() }, true);
    b.add_loop(base, &[(eq, true)]);
    b
}

/// Sphere of radius r cut by the plane x = a: `keep_cap` keeps x ≥ a (the sphere face is a disk
/// in (u, v)), otherwise x ≤ a (the sphere face is everything but a hole: both poles inside).
fn sphere_cut(r: f64, a: f64, keep_cap: bool) -> Brep {
    let (mut b, sh) = solid();
    let fr = Frame::from_origin_z_x(P3::new(a, 0.0, 0.0), V3::x(), V3::y());
    let c = ring(&mut b, fr, (r * r - a * a).sqrt());
    let sp = b.add_face(sh, Surface::Sphere { frame: Frame::world(), radius: r }, false);
    b.add_loop(sp, &[(c, !keep_cap)]);
    let disk = b.add_face(sh, Surface::Plane { frame: fr }, keep_cap);
    b.add_loop(disk, &[(c, keep_cap)]);
    b
}

/// Sphere of radius r intersected with the cube [−a, a]³ (r/√2 < a < r): the sphere face has
/// two circles winding round the axis (z caps) and four holes (x and y caps).
fn sphere_cube(r: f64, a: f64) -> Brep {
    let (mut b, sh) = solid();
    let rho = (r * r - a * a).sqrt();
    let sp = b.add_face(sh, Surface::Sphere { frame: Frame::world(), radius: r }, false);
    for d in [V3::x(), -V3::x(), V3::y(), -V3::y(), V3::z(), -V3::z()] {
        let seed = if d.x.abs() > 0.5 { V3::y() } else { V3::x() };
        let fr = Frame::from_origin_z_x(P3::from(d * a), d, seed);
        let c = ring(&mut b, fr, rho);
        b.add_loop(sp, &[(c, true)]);
        let disk = b.add_face(sh, Surface::Plane { frame: fr }, false);
        b.add_loop(disk, &[(c, false)]);
    }
    b
}

/// Sphere of radius r cut by the tilted plane z = h + m·x: `keep_cap` keeps the side above it,
/// which contains the north pole (the circle winds once round the axis and is closed by the
/// pole; it is not a parallel, so the face is not a parameter rectangle), otherwise the side
/// below (closed by the south pole).
fn tilted_sphere_cut(r: f64, h: f64, m: f64, keep_cap: bool) -> Brep {
    let (mut b, sh) = solid();
    let n = V3::new(-m, 0.0, 1.0).normalize();
    let d = h / (1.0 + m * m).sqrt();
    let fr = Frame::from_origin_z_x(P3::from(n * d), n, V3::y());
    let c = ring(&mut b, fr, (r * r - d * d).sqrt());
    let sp = b.add_face(sh, Surface::Sphere { frame: Frame::world(), radius: r }, false);
    b.add_loop(sp, &[(c, !keep_cap)]);
    let disk = b.add_face(sh, Surface::Plane { frame: fr }, keep_cap);
    b.add_loop(disk, &[(c, keep_cap)]);
    b
}

/// Cylinder r with base z = 0 cut obliquely by z = h + m·x (elliptic top edge, seamless).
fn oblique_cylinder(r: f64, h: f64, m: f64) -> Brep {
    let n = (1.0 + m * m).sqrt();
    let f0 = Frame::world();
    let ftop = Frame::from_origin_z_x(P3::new(0.0, 0.0, h), V3::new(-m, 0.0, 1.0), V3::new(1.0, 0.0, m));
    let (mut b, sh) = solid();
    let v0 = b.add_vertex(P3::new(r, 0.0, 0.0));
    let v1 = b.add_vertex(P3::new(r, 0.0, h + m * r));
    let c0 = b.add_edge(Some(Curve::Circle { frame: f0, radius: r }), 0.0, TAU, v0, v0);
    let el = b.add_edge(Some(Curve::Ellipse { frame: ftop, major: r * n, minor: r }), 0.0, TAU, v1, v1);
    let side = b.add_face(sh, Surface::Cylinder { frame: f0, radius: r }, false);
    b.add_loop(side, &[(c0, false)]);
    b.add_loop(side, &[(el, true)]);
    let bot = b.add_face(sh, Surface::Plane { frame: f0 }, true);
    b.add_loop(bot, &[(c0, true)]);
    let top = b.add_face(sh, Surface::Plane { frame: ftop }, false);
    b.add_loop(top, &[(el, false)]);
    b
}

/// Watertight, outward, chord-bounded, and volume / curved-face area converging linearly with
/// the chord. References from `cad_geom::mass` after `validate`, or analytic ones (`reference` =
/// (volume, curved area)) where the kernel's integrators do not cover the face.
fn check_curved_solid(id: &str, b: &Brep, rho: f64, chords: &[f64], reference: Option<(f64, f64)>) {
    let curved: Vec<FaceId> = b.face_ids().filter(|&f| !matches!(b.face(f).surface, Surface::Plane { .. })).collect();
    let (exact, exact_area) = reference.unwrap_or_else(|| {
        let rep = validate(b, &Tolerance::default());
        assert!(rep.is_valid(), "{id}: fixture invalid: {:?}", rep.issues);
        (solid_properties(b, SolidId(0)).unwrap().volume, curved.iter().map(|&f| face_area(b, f).unwrap()).sum())
    });
    let (mut last_v, mut last_a, mut last_c) = (f64::INFINITY, f64::INFINITY, f64::INFINITY);
    for &chord in chords {
        let m = tessellate_solid(b, SolidId(0), chord).unwrap();
        assert_watertight(id, &m);
        assert_normals(id, &m);
        let ev = (m.signed_volume() - exact).abs() / exact;
        let mut area = 0.0;
        let (mut worst, mut worst_any) = (0.0f64, 0.0f64);
        for &f in &curved {
            let fm = tessellate_face(b, f, chord).unwrap();
            area += fm.area();
            let (c, any) = chord_errors(b, f, &fm);
            worst = worst.max(c);
            worst_any = worst_any.max(any);
        }
        let ea = (area - exact_area).abs() / exact_area;
        eprintln!(
            "{id:24} chord {chord:6} tris {:7} vol err {ev:.3e} area err {ea:.3e} chord err {:.3} (anywhere {:.3})",
            m.triangles.len(),
            worst / chord,
            worst_any / chord
        );
        assert!(worst <= chord * (1.0 + 1e-9), "{id}: deviation {worst} above chord {chord}");
        assert!(worst_any <= chord * (1.0 + 1e-6), "{id}: deviation {worst_any} inside a triangle at chord {chord}");
        assert!(ev <= 2.0 * chord / rho, "{id}: volume error {ev} at chord {chord}");
        assert!(ea <= 2.0 * chord / rho, "{id}: area error {ea} at chord {chord}");
        // linear convergence: the error shrinks with the chord
        let ratio = if last_c.is_finite() { 2.0 * chord / last_c } else { f64::INFINITY };
        assert!(ev <= ratio * last_v || ev < 1e-12, "{id}: volume not converging ({ev} after {last_v})");
        assert!(ea <= ratio * last_a || ea < 1e-12, "{id}: area not converging ({ea} after {last_a})");
        (last_v, last_a, last_c) = (ev, ea, chord);
    }
}

const FINE: [f64; 3] = [0.1, 0.01, 0.001];
const COARSE: [f64; 2] = [0.1, 0.02];

#[test]
fn half_cylinder_trimmed_by_lines_and_arcs() {
    let b = half_cylinder(5.0, 10.0);
    // the curved face's area is exactly π r h
    assert!((face_area(&b, FaceId(0)).unwrap() - PI * 50.0).abs() < 1e-9);
    check_curved_solid("half_cylinder", &b, 5.0, &FINE, None);
}

#[test]
fn half_cone_with_apex_on_the_boundary() {
    check_curved_solid("half_cone", &half_cone(6.0, 9.0), 6.0, &FINE, None);
}

#[test]
fn hemisphere_closed_by_the_pole() {
    check_curved_solid("hemisphere", &hemisphere(10.0), 10.0, &FINE, None);
}

#[test]
fn spherical_cap_and_sphere_minus_cap() {
    let (r, a) = (10.0, 4.0);
    // errors are relative to the solid, so the size that matters for the thin cap is its height
    check_curved_solid("sphere_cap", &sphere_cut(r, a, true), r - a, &COARSE, None);
    // the cap x ≤ −a, bounded by the same circle used the other way round
    check_curved_solid("sphere_cap_mirrored", &sphere_cut(r, -a, false), r - a, &COARSE, None);
    // the sphere minus the cap x ≥ a: one loop of winding 0 that bounds the face from outside,
    // so both poles are inside the face (the tessellator closes it with two pole lines and a
    // meridian cut); references are the closed forms, which `mass` must agree with
    let h = r - a;
    let cap_v = PI * h * h * (3.0 * r - h) / 3.0;
    let reference = (4.0 / 3.0 * PI * r.powi(3) - cap_v, 4.0 * PI * r * r - TAU * r * h);
    let b = sphere_cut(r, a, false);
    assert!((solid_properties(&b, SolidId(0)).unwrap().volume - reference.0).abs() < 1e-9 * reference.0);
    assert!((face_area(&b, FaceId(0)).unwrap() - reference.1).abs() < 1e-9 * reference.1);
    check_curved_solid("sphere_minus_cap", &b, r, &COARSE, Some(reference));
}

#[test]
fn sphere_cube_intersection() {
    let (r, a) = (10.0, 8.0);
    let b = sphere_cube(r, a);
    // sphere volume minus six caps of height r − a
    let hgt = r - a;
    let exact = 4.0 / 3.0 * PI * r.powi(3) - 6.0 * PI * hgt * hgt * (3.0 * r - hgt) / 3.0;
    assert!((solid_properties(&b, SolidId(0)).unwrap().volume - exact).abs() < 1e-9 * exact);
    check_curved_solid("sphere_cube", &b, r, &COARSE, None);
}

#[test]
fn oblique_cylinder_band() {
    check_curved_solid("oblique_cylinder", &oblique_cylinder(3.0, 10.0, 0.6), 3.0, &FINE, None);
}

#[test]
fn full_primitives_keep_the_grid() {
    // a primitive's curved face still uses the parameter grid: 2(nu + 1) vertices, 2·nu triangles
    let b = cad_geom::primitives::make_cylinder(Frame::world(), 10.0, 20.0).unwrap();
    let m = tessellate_face(&b, FaceId(0), 0.1).unwrap();
    assert_eq!(m.triangles.len(), 2 * (m.positions.len() / 2 - 1));
}

#[test]
fn tilted_sphere_caps_closed_by_a_pole() {
    // a winding loop that is not a parallel, closed by the north pole (cap) or the south pole
    check_curved_solid("tilted_cap_north", &tilted_sphere_cut(10.0, 3.0, 0.4, true), 7.0, &COARSE, None);
    check_curved_solid("tilted_rest_south", &tilted_sphere_cut(10.0, 3.0, 0.4, false), 10.0, &COARSE, None);
}

/// Torus patch u ∈ [0, π/2], v ∈ [0, π] bounded by the outer and inner equator arcs and two
/// meridian half circles; area ∫∫ r(R + r cos v) du dv = π² R r / 2.
#[test]
fn torus_patch() {
    let (rr, r) = (20.0, 5.0);
    let mut b = Brep::new();
    let s = b.add_solid();
    let sh = b.add_shell(Some(s));
    let p00 = b.add_vertex(P3::new(rr + r, 0.0, 0.0));
    let p10 = b.add_vertex(P3::new(0.0, rr + r, 0.0));
    let p11 = b.add_vertex(P3::new(0.0, rr - r, 0.0));
    let p01 = b.add_vertex(P3::new(rr - r, 0.0, 0.0));
    let outer = b.add_edge(Some(Curve::Circle { frame: Frame::world(), radius: rr + r }), 0.0, FRAC_PI_2, p00, p10);
    let inner = b.add_edge(Some(Curve::Circle { frame: Frame::world(), radius: rr - r }), 0.0, FRAC_PI_2, p01, p11);
    let m0 = Frame::from_origin_z_x(P3::new(rr, 0.0, 0.0), -V3::y(), V3::x());
    let m1 = Frame::from_origin_z_x(P3::new(0.0, rr, 0.0), V3::x(), V3::y());
    let mer0 = b.add_edge(Some(Curve::Circle { frame: m0, radius: r }), 0.0, PI, p00, p01);
    let mer1 = b.add_edge(Some(Curve::Circle { frame: m1, radius: r }), 0.0, PI, p10, p11);
    let f = b.add_face(sh, Surface::Torus { frame: Frame::world(), major: rr, minor: r }, false);
    b.add_loop(f, &[(outer, false), (mer1, false), (inner, true), (mer0, true)]);
    let exact = PI * PI * rr * r / 2.0;
    let mut last = f64::INFINITY;
    for chord in [0.1, 0.01, 0.001] {
        let m = tessellate_face(&b, f, chord).unwrap();
        let err = (m.area() - exact).abs() / exact;
        let (at_checks, anywhere) = chord_errors(&b, f, &m);
        eprintln!("torus_patch chord {chord:6} tris {:6} area err {err:.3e} chord err {:.3} (anywhere {:.3})", m.triangles.len(), at_checks / chord, anywhere / chord);
        assert!(anywhere <= chord * (1.0 + 1e-6), "deviation {anywhere} at chord {chord}");
        assert!(err <= 2.0 * chord / r && err <= 0.2 * last, "area error {err} at chord {chord}");
        last = err;
        assert_normals("torus_patch", &m);
        // an open patch: interior edges have twins, the boundary is used once
        let edges = edge_uses(&m);
        assert!(edges.values().all(|&n| n == 1));
    }
}

#[test]
fn near_degenerate_slivers() {
    // an outer vertex 1e-9 inside a straight side, and a hole vertex 1e-9 above the outer
    // boundary: every triangle is kept (no zero-area drop) and the area is exact
    let mut b = Brep::new();
    let s = b.add_solid();
    let sh = b.add_shell(Some(s));
    let outer_pts = [[0.0, 0.0], [5.0, 1e-9], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    let hole_pts = [[8.0, 1.4e-9], [6.0, 3.0], [9.0, 3.0]];
    let f = b.add_face(sh, Surface::Plane { frame: Frame::world() }, false);
    for ring in [&outer_pts[..], &hole_pts[..]] {
        let vs: Vec<_> = ring.iter().map(|p| b.add_vertex(P3::new(p[0], p[1], 0.0))).collect();
        let uses: Vec<_> = (0..vs.len()).map(|i| (b.add_line_edge(vs[i], vs[(i + 1) % vs.len()]), false)).collect();
        b.add_loop(f, &uses);
    }
    let shoelace = |p: &[[f64; 2]]| (0..p.len()).map(|i| p[i][0] * p[(i + 1) % p.len()][1] - p[(i + 1) % p.len()][0] * p[i][1]).sum::<f64>() / 2.0;
    let exact = shoelace(&outer_pts) + shoelace(&hole_pts);
    let m = tessellate_face(&b, f, 0.1).unwrap();
    assert_eq!(m.triangles.len(), 5 + 3 + 2 - 2);
    assert!((m.area() - exact).abs() < 1e-12 * exact, "area {} vs {exact}", m.area());
    let p = |i: u32| V3::from(m.positions[i as usize]);
    for t in &m.triangles {
        assert!((p(t[1]) - p(t[0])).cross(&(p(t[2]) - p(t[0]))).z > 0.0, "inverted or flat triangle {t:?}");
    }
    let edges = edge_uses(&m);
    assert!(edges.values().all(|&n| n == 1));
    assert_eq!(edges.keys().filter(|(a, c)| !edges.contains_key(&(*c, *a))).count(), 8, "8 boundary edges");
}
