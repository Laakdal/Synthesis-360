//! Shell (hollow) of planar solids: `shell.top` of `research/feature-cases.json` against the
//! OCCT reference (11 faces / 24 edges / 16 vertices, volume 5432) and closed forms, plus the
//! L-shaped prism, two opened faces, a removed face next to a reflex edge, a closed hollow (no
//! face removed), a rotated base, and the error cases (bad thickness, curved base).
//!
//! Topology bookkeeping used below (Euler–Poincaré per shell, V − E + F − (L − F) = 2(1 − G)):
//! opening a face of a convex block keeps its other faces, adds the cavity's faces (one per kept
//! face) and turns the removed face into a rim; the result is still a ball (G = 0).

#[path = "support/features.rs"]
mod features;

use cad_geom::mass::solid_properties;
use cad_geom::tess::tessellate_solid;
use cad_geom::transform::isometry;
use cad_geom::{extrude, shell, validate, Brep, FaceId, GeomError, MassProperties, SolidId, Surface, Tolerance, P3, V3};
use features::profile;

fn build(id: &str) -> Brep {
    let c = features::case(id);
    let (from, to) = (c["from"].as_f64().unwrap(), c["to"].as_f64().unwrap());
    extrude(&profile(&c), from, to, &Tolerance::asm()).unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// The one face whose plane contains `p` and whose vertex bounds contain it.
fn face_at(b: &Brep, p: P3) -> FaceId {
    let hits: Vec<FaceId> = b
        .face_ids()
        .filter(|&f| {
            let Surface::Plane { frame } = &b.face(f).surface else { return false };
            if frame.z.dot(&(p - frame.origin)).abs() > 1e-9 {
                return false;
            }
            let pts: Vec<P3> = b.face_coedges(f).map(|c| b.vertex(b.coedge_start(c)).point).collect();
            let lo = pts.iter().fold(pts[0], |a, q| a.inf(q));
            let hi = pts.iter().fold(pts[0], |a, q| a.sup(q));
            (0..3).all(|k| p[k] >= lo[k] - 1e-9 && p[k] <= hi[k] + 1e-9)
        })
        .collect();
    assert_eq!(hits.len(), 1, "face at {p:?}: {hits:?}");
    hits[0]
}

fn counts(b: &Brep) -> (usize, usize, usize, usize) {
    (b.faces.len(), b.edges.len(), b.vertices.len(), b.loops.len())
}

fn mass(b: &Brep) -> MassProperties {
    solid_properties(b, SolidId(0)).unwrap()
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-300)
}

/// Valid at both tolerance presets, one solid, `shells` shells, genus 0, every face planar and
/// every edge a line.
fn assert_valid(id: &str, b: &Brep, shells: usize) {
    assert_eq!(b.solids.len(), 1, "{id}: one solid");
    assert_eq!(b.solids[0].shells.len(), shells, "{id}: shells");
    for tol in [Tolerance::asm(), Tolerance::occt()] {
        let r = validate(b, &tol);
        assert!(r.is_valid(), "{id} at {tol:?}: {:?}", r.issues);
        assert_eq!(r.genus, 0, "{id}: genus");
    }
    assert!(b.faces.iter().all(|f| f.surface.type_name() == "plane"), "{id}: all planes");
    assert!(b.edges.iter().all(|e| e.curve.as_ref().is_some_and(|c| c.type_name() == "line")), "{id}: all lines");
}

/// Watertight mesh whose enclosed volume and area equal the exact B-rep values (planar faces
/// are tessellated exactly).
fn assert_mesh(id: &str, b: &Brep, m: &MassProperties) {
    let mesh = tessellate_solid(b, SolidId(0), 0.1).unwrap();
    features::assert_watertight(id, &mesh);
    assert!(rel(mesh.signed_volume(), m.volume) < 1e-12, "{id}: mesh volume {} vs {}", mesh.signed_volume(), m.volume);
    assert!(rel(mesh.area(), m.area) < 1e-12, "{id}: mesh area {} vs {}", mesh.area(), m.area);
}

fn shell_top(base: &Brep) -> Brep {
    let c = features::case("shell.top");
    assert_eq!(c["base"], "ext.rect");
    assert_eq!(c["direction"], "inside");
    let t = c["thickness"].as_f64().unwrap();
    let f = face_at(base, features::p3(&c["faces"][0]));
    shell(base, &[f], t, &Tolerance::asm()).unwrap()
}

/// Box 50×30×10, top removed, t = 2. Cavity [2,48]×[2,28]×[2,10] = 46·26·8 = 9568.
/// F = 5 kept outer faces + 1 rim (the old top, now a ring) + 5 cavity faces = 11;
/// E = 12 outer + 12 cavity = 24; V = 8 + 8 = 16; L = 11 + 1 (the rim's inner loop) = 12;
/// Euler: 16 − 24 + 11 − 1 = 2 → G = 0. Matches OCCT (11/24/16) and the provisional ASM record.
#[test]
fn shell_top_matches_reference_and_closed_forms() {
    let r = shell_top(&build("ext.rect"));
    assert_eq!(counts(&r), (11, 24, 16, 12), "shell.top F/E/V/L");
    assert_valid("shell.top", &r, 1);
    let rim = face_at(&r, P3::new(1.0, 15.0, 10.0));
    assert_eq!(r.face(rim).loops.len(), 2, "the rim has an outer and an inner loop");
    let m = mass(&r);
    let vol = 15000.0 - 46.0 * 26.0 * 8.0;
    assert_eq!(vol, 5432.0);
    assert!(rel(m.volume, vol) < 1e-12, "volume {}", m.volume);
    // outside: bottom 1500 + sides 1600; rim 1500 − 1196; cavity floor 1196 + walls 2·(46+26)·8
    let area = 1500.0 + 1600.0 + (1500.0 - 1196.0) + 1196.0 + 2.0 * (46.0 + 26.0) * 8.0;
    assert_eq!(area, 5752.0);
    assert!(rel(m.area, area) < 1e-12, "area {}", m.area);
    let cz = (15000.0 * 5.0 - 9568.0 * 6.0) / 5432.0;
    assert!((m.centroid - P3::new(25.0, 15.0, cz)).norm() < 1e-9, "centroid {:?}", m.centroid);
    // inertia about the centroid: box minus cavity (parallel-axis shift of each to the centroid)
    let block = |v: f64, d: [f64; 3], c: [f64; 3]| {
        let s = [d[0] * d[0], d[1] * d[1], d[2] * d[2]].map(|x| v * x / 12.0);
        let o = [c[0] - 25.0, c[1] - 15.0, c[2] - cz];
        [s[1] + s[2] + v * (o[1] * o[1] + o[2] * o[2]), s[0] + s[2] + v * (o[0] * o[0] + o[2] * o[2]), s[0] + s[1] + v * (o[0] * o[0] + o[1] * o[1])]
    };
    let (b, c) = (block(15000.0, [50.0, 30.0, 10.0], [25.0, 15.0, 5.0]), block(9568.0, [46.0, 26.0, 8.0], [25.0, 15.0, 6.0]));
    for k in 0..3 {
        assert!(rel(m.inertia[k][k], b[k] - c[k]) < 1e-12, "I{k}{k} {} vs {}", m.inertia[k][k], b[k] - c[k]);
        for j in 0..3 {
            if j != k {
                assert!(m.inertia[k][j].abs() < 1e-9 * b[0], "I{k}{j} {}", m.inertia[k][j]);
            }
        }
    }
    assert_mesh("shell.top", &r, &m);
    // the caller's tolerance governs the offset and result checks: OCCT's 1e-7 gives the same
    let base = build("ext.rect");
    let r7 = shell(&base, &[face_at(&base, P3::new(25.0, 15.0, 10.0))], 2.0, &Tolerance::occt()).unwrap();
    assert_eq!(counts(&r7), (11, 24, 16, 12));
    assert!(rel(mass(&r7).volume, vol) < 1e-12);
}

/// L prism (outline (0,0)(40,0)(40,10)(10,10)(10,30)(0,30), height 8), top removed, t = 2.
/// Cavity outline (2,2)(38,2)(38,8)(8,8)(8,28)(2,28): area 36·6 + 6·20 = 336, perimeter 124,
/// z ∈ [2, 8]. F = 7 kept + 1 rim + 7 cavity = 15; E = 18 + 18 = 36; V = 12 + 12 = 24;
/// L = 15 + 1 = 16; Euler: 24 − 36 + 15 − 1 = 2.
#[test]
fn lshape_top_removed() {
    let base = build("ext.lshape");
    let top = face_at(&base, P3::new(5.0, 5.0, 8.0));
    let r = shell(&base, &[top], 2.0, &Tolerance::asm()).unwrap();
    assert_eq!(counts(&r), (15, 36, 24, 16), "F/E/V/L");
    assert_valid("lshape.top", &r, 1);
    let m = mass(&r);
    let (cav_a, cav_v) = (336.0, 336.0 * 6.0);
    assert!(rel(m.volume, 4800.0 - cav_v) < 1e-12, "volume {}", m.volume);
    let area = 600.0 + 140.0 * 8.0 + (600.0 - cav_a) + cav_a + 124.0 * 6.0;
    assert!(rel(m.area, area) < 1e-12, "area {} vs {area}", m.area);
    let base_c = V3::new(15.0, 10.0, 4.0);
    let cav_c = V3::new((216.0 * 20.0 + 120.0 * 5.0) / 336.0, (216.0 * 5.0 + 120.0 * 18.0) / 336.0, 5.0);
    let want = (base_c * 4800.0 - cav_c * cav_v) / (4800.0 - cav_v);
    assert!((m.centroid.coords - want).norm() < 1e-9, "centroid {:?} vs {want:?}", m.centroid);
    assert_mesh("lshape.top", &r, &m);
}

/// Box, top and the y = 0 side removed: cavity [2,48]×[0,28]×[2,10] = 46·28·8 = 10304.
/// F = 4 kept outer + 2 U-shaped rims + 4 cavity faces (floor, x = 2, x = 48, y = 28) = 10;
/// V = 8 box corners + 8 cavity corners = 16; every loop single (L = 10); Euler gives
/// E = V + F − (L − F) − 2 = 24.
#[test]
fn box_open_on_two_sides() {
    let base = build("ext.rect");
    let faces = [face_at(&base, P3::new(25.0, 15.0, 10.0)), face_at(&base, P3::new(25.0, 0.0, 5.0))];
    let r = shell(&base, &faces, 2.0, &Tolerance::asm()).unwrap();
    assert_eq!(counts(&r), (10, 24, 16, 10), "F/E/V/L");
    assert_valid("box.top_front", &r, 1);
    let m = mass(&r);
    let cav = 46.0 * 28.0 * 8.0;
    assert!(rel(m.volume, 15000.0 - cav) < 1e-12, "volume {}", m.volume);
    // outer bottom 1500, x walls 2·300, back wall 500; rims 1500 − 1288 and 500 − 368;
    // cavity floor 1288, x walls 2·28·8, back wall 46·8
    let area = 1500.0 + 600.0 + 500.0 + 212.0 + 132.0 + 1288.0 + 448.0 + 368.0;
    assert!(rel(m.area, area) < 1e-12, "area {} vs {area}", m.area);
    let want = (V3::new(25.0, 15.0, 5.0) * 15000.0 - V3::new(25.0, 14.0, 6.0) * cav) / (15000.0 - cav);
    assert!((m.centroid.coords - want).norm() < 1e-9, "centroid {:?}", m.centroid);
    assert_mesh("box.top_front", &r, &m);
}

/// L prism with the face y = 10 (x ∈ [10, 40], next to the reflex edge at (10, 10)) removed,
/// t = 2. The exact cavity is {x∈[2,38], y∈[2,10]} ∪ {x∈[2,8], y∈[10,28]}, z ∈ [2, 6]:
/// (288 + 108)·4 = 1584. The wall x ∈ [8, 10] of the arm hangs down to y = 10, so pushing the
/// removed plane outward would notch it; the volume check rejects that tool and the exact cavity
/// is used. F = 7 kept outer + 1 U-shaped rim + 7 cavity faces + the hanging wall's end face
/// (y = 10, x ∈ [8, 10], facing −y) = 16; V = 12 base + 2 on the reflex edge (z = 2, 6) + 12
/// cavity = 26; all loops single (L = 16); Euler gives E = 26 + 16 − 2 = 40.
#[test]
fn removed_face_next_to_reflex_edge() {
    let base = build("ext.lshape");
    let f = face_at(&base, P3::new(25.0, 10.0, 4.0));
    let r = shell(&base, &[f], 2.0, &Tolerance::asm()).unwrap();
    assert_eq!(counts(&r), (16, 40, 26, 16), "F/E/V/L");
    assert_valid("lshape.reflex", &r, 1);
    let m = mass(&r);
    assert!(rel(m.volume, 4800.0 - 1584.0) < 1e-12, "volume {}", m.volume);
    let end = face_at(&r, P3::new(9.0, 10.0, 4.0));
    let Surface::Plane { frame } = &r.face(end).surface else { unreachable!() };
    let n = if r.face(end).reversed { -frame.z } else { frame.z };
    assert!((n - V3::new(0.0, -1.0, 0.0)).norm() < 1e-12, "hanging wall end faces −y: {n:?}");
    assert_mesh("lshape.reflex", &r, &m);
}

/// No face removed: a closed hollow. Cavity [2,48]×[2,28]×[2,8] = 7176 is a void shell.
/// F = 6 + 6, E = 12 + 12, V = 8 + 8, L = 12, two shells of genus 0.
#[test]
fn closed_hollow_without_removed_faces() {
    let r = shell(&build("ext.rect"), &[], 2.0, &Tolerance::asm()).unwrap();
    assert_eq!(counts(&r), (12, 24, 16, 12), "F/E/V/L");
    assert_valid("box.closed", &r, 2);
    let m = mass(&r);
    assert!(rel(m.volume, 15000.0 - 7176.0) < 1e-12, "volume {}", m.volume);
    let area = 4600.0 + 2.0 * (46.0 * 26.0 + 46.0 * 6.0 + 26.0 * 6.0);
    assert!(rel(m.area, area) < 1e-12, "area {} vs {area}", m.area);
    assert!((m.centroid - P3::new(25.0, 15.0, 5.0)).norm() < 1e-9);
    assert_mesh("box.closed", &r, &m);
}

/// The same shell of a box rotated about a generic axis and translated: identical topology,
/// volume and area; centroid R·c + t; inertia R·I·Rᵀ.
#[test]
fn rotated_box_shell_rotates_mass_properties() {
    let base = build("ext.rect");
    let flat = mass(&shell_top(&base));
    let iso = isometry(V3::new(1.0, 2.0, 3.0), 0.7, V3::new(5.0, -3.0, 2.0));
    let mut tilted = base.clone();
    tilted.transform_iso(&iso);
    let top = face_at(&tilted, iso * P3::new(25.0, 15.0, 10.0));
    let r = shell(&tilted, &[top], 2.0, &Tolerance::asm()).unwrap();
    assert!(r.faces.iter().all(|f| f.surface.frame().unwrap().z.amax() < 1.0 - 1e-3), "no face plane is axis-aligned");
    assert_eq!(counts(&r), (11, 24, 16, 12), "F/E/V/L");
    assert_valid("shell.top.rotated", &r, 1);
    let m = mass(&r);
    assert!(rel(m.volume, 5432.0) < 1e-12, "volume {}", m.volume);
    assert!(rel(m.area, 5752.0) < 1e-12, "area {}", m.area);
    assert!((m.centroid - iso * flat.centroid).norm() < 1e-9, "centroid {:?}", m.centroid);
    let rot = *iso.rotation.to_rotation_matrix().matrix();
    let i0 = nalgebra::Matrix3::from_fn(|i, j| flat.inertia[i][j]);
    let want = rot * i0 * rot.transpose();
    let scale = i0.amax();
    for i in 0..3 {
        for j in 0..3 {
            assert!((m.inertia[i][j] - want[(i, j)]).abs() < 1e-11 * scale, "I{i}{j} {} vs {}", m.inertia[i][j], want[(i, j)]);
        }
    }
    assert_mesh("shell.top.rotated", &r, &m);
}

#[test]
fn invalid_thickness_is_rejected() {
    let base = build("ext.rect");
    let top = face_at(&base, P3::new(25.0, 15.0, 10.0));
    // 10: the floor reaches the opening (cavity height 0); 12: the floor passes it (cavity inverts
    // vertically while its sides are still positive); 15: the y walls meet; 30: everything inverts
    for t in [0.0, -1.0, f64::NAN, f64::INFINITY, 10.0, 12.0, 15.0, 30.0] {
        let e = shell(&base, &[top], t, &Tolerance::asm()).unwrap_err();
        assert!(matches!(e, GeomError::InvalidInput(_)), "t = {t}: {e:?}");
    }
    // without an opening the cavity closes at t = 5 (box height 10)
    let e = shell(&base, &[], 5.0, &Tolerance::asm()).unwrap_err();
    assert!(matches!(e, GeomError::InvalidInput(_)), "{e:?}");
    // a face id that does not exist; every face removed
    assert!(matches!(shell(&base, &[FaceId(99)], 2.0, &Tolerance::asm()), Err(GeomError::InvalidInput(_))));
    let all: Vec<FaceId> = base.face_ids().collect();
    assert!(matches!(shell(&base, &all, 2.0, &Tolerance::asm()), Err(GeomError::InvalidInput(_))));
    // just below the limit still works: floor at z = 9.9, walls 9.9 thick
    let r = shell(&base, &[top], 9.9, &Tolerance::asm()).unwrap();
    assert!(rel(mass(&r).volume, 15000.0 - 30.2 * 10.2 * 0.1) < 1e-12);
}

#[test]
fn curved_base_is_unsupported() {
    let base = build("ext.circle");
    // the top disc (a plane through z = 20; its single circle edge has one vertex)
    let top = base
        .face_ids()
        .find(|&f| matches!(&base.face(f).surface, Surface::Plane { frame } if (frame.origin.z - 20.0).abs() < 1e-9))
        .unwrap();
    let e = shell(&base, &[top], 2.0, &Tolerance::asm()).unwrap_err();
    assert!(matches!(e, GeomError::Unsupported(_)), "{e:?}");
    let e = shell(&base, &[], 2.0, &Tolerance::asm()).unwrap_err();
    assert!(matches!(e, GeomError::Unsupported(_)), "{e:?}");
}

/// Closed polyhedron from points and faces (vertex indices counter-clockwise about the outward
/// normal), built through the topology builder.
fn polyhedron(pts: &[P3], faces: &[&[usize]]) -> Brep {
    use std::collections::HashMap;
    let mut b = Brep::new();
    let s = b.add_solid();
    let sh = b.add_shell(Some(s));
    let vs: Vec<_> = pts.iter().map(|&p| b.add_vertex(p)).collect();
    let mut edges: HashMap<(usize, usize), cad_geom::EdgeId> = HashMap::new();
    for f in faces {
        let (p0, p1, p2) = (pts[f[0]], pts[f[1]], pts[f[2]]);
        let n = (p1 - p0).cross(&(p2 - p0));
        let fid = b.add_face(sh, Surface::Plane { frame: cad_geom::Frame::from_origin_z(p0, n) }, false);
        let mut uses = Vec::new();
        for k in 0..f.len() {
            let (i, j) = (f[k], f[(k + 1) % f.len()]);
            let e = match edges.get(&(j, i)) {
                Some(&e) => (e, true),
                None => {
                    let e = b.add_line_edge(vs[i], vs[j]);
                    edges.insert((i, j), e);
                    (e, false)
                }
            };
            uses.push(e);
        }
        b.add_loop(fid, &uses);
    }
    b
}

/// Square pyramid, base [0,10]² at z = 0, apex `apex`.
fn pyramid(apex: P3) -> Brep {
    let pts = [P3::new(0.0, 0.0, 0.0), P3::new(10.0, 0.0, 0.0), P3::new(10.0, 10.0, 0.0), P3::new(0.0, 10.0, 0.0), apex];
    let b = polyhedron(&pts, &[&[0, 3, 2, 1], &[0, 1, 4], &[1, 2, 4], &[2, 3, 4], &[3, 0, 4]]);
    assert!(validate(&b, &Tolerance::occt()).is_valid());
    b
}

/// Right square pyramid (apex (5, 5, 6), four faces at the apex), base removed, t = 1/2. The four
/// offset planes are concurrent by symmetry, so the apex has a unique offset point. Lateral plane
/// (−6y + 5z)/√61 = 0 moved in by t meets z = 0 at y = t√61/6: the cavity is the similar pyramid of
/// half-width a = 5 − t√61/6 and height 6a/5, volume (8/5)a³ (the base: (8/5)·125 = 200).
/// F = 4 + 1 rim + 4 = 9, E = 8 + 8, V = 5 + 5, L = 9 + 1; Euler 10 − 16 + 9 − 1 = 2.
#[test]
fn right_pyramid_four_faces_at_apex() {
    let base = pyramid(P3::new(5.0, 5.0, 6.0));
    let r = shell(&base, &[FaceId(0)], 0.5, &Tolerance::asm()).unwrap();
    assert_eq!(counts(&r), (9, 16, 10, 10), "F/E/V/L");
    assert_valid("pyramid", &r, 1);
    let a = 5.0 - 0.5 * 61f64.sqrt() / 6.0;
    let m = mass(&r);
    assert!(rel(m.volume, 1.6 * (125.0 - a * a * a)) < 1e-12, "volume {}", m.volume);
    assert_mesh("pyramid", &r, &m);
}

/// Oblique pyramid (apex (3, 4, 6)): the four offset planes at the apex do not meet in one point
/// (no sphere touches all four faces there), so the shell is not representable by moving planes.
#[test]
fn oblique_pyramid_apex_is_unsupported() {
    let base = pyramid(P3::new(3.0, 4.0, 6.0));
    let e = shell(&base, &[FaceId(0)], 0.5, &Tolerance::asm()).unwrap_err();
    assert!(matches!(&e, GeomError::Unsupported(m) if m.contains("V4") && m.contains("do not meet in one point")), "{e:?}");
}

/// The L prism's arms are 10 wide: t = 5 closes them (cavity edges collapse), t = 4.5 still works.
#[test]
fn lshape_thickness_limit() {
    let base = build("ext.lshape");
    let top = face_at(&base, P3::new(5.0, 5.0, 8.0));
    for t in [5.0, 6.0] {
        let e = shell(&base, &[top], t, &Tolerance::asm()).unwrap_err();
        assert!(matches!(e, GeomError::InvalidInput(_)), "t = {t}: {e:?}");
    }
    // cavity outline (4.5,4.5)(35.5,4.5)(35.5,5.5)(5.5,5.5)(5.5,25.5)(4.5,25.5): 31 + 20 = 51, z ∈ [4.5, 8]
    let r = shell(&base, &[top], 4.5, &Tolerance::asm()).unwrap();
    assert_valid("lshape.4.5", &r, 1);
    assert!(rel(mass(&r).volume, 4800.0 - 51.0 * 3.5) < 1e-12, "volume {}", mass(&r).volume);
}
