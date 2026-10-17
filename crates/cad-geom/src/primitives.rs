//! Primitive solids built as exact B-reps. Signatures follow `research/cases.json`
//! (the case catalogue shared with the OCCT run and the ASM probe).
//!
//! Topology follows ASM's *seamless* convention, as recorded from Fusion in
//! `research/evidence/asm-v5/reference.json` (counts include degenerate edges):
//!
//! | primitive        | F | E | V | L | notes                                                        |
//! |------------------|---|---|---|---|--------------------------------------------------------------|
//! | box              | 6 | 12| 8 | 6 | planes; min-side faces `reversed`                            |
//! | cylinder/frustum | 3 | 2 | 2 | 4 | 2 closed circles (1 vertex each); side face has 2 loops      |
//! | pointed cone     | 2 | 2 | 2 | 3 | base circle + degenerate apex edge (no curve); side 2 loops  |
//! | sphere           | 1 | 0 | 0 | 0 | one face, no loops                                           |
//! | torus            | 1 | 0 | 0 | 0 | one face, no loops (genus 1)                                 |
//!
//! There are no seam edges: a periodic side face is bounded by closed circles that each wind once
//! round the axis (the integrators close such loops with a virtual seam, see [`crate::mass`]).
//! Faces are listed side face first, as ASM reports them. A circle's vertex sits on `frame.x`
//! (circle parameter t = 0, surface u = 0). The side face's loops are [bottom circle, forward]
//! and [top circle, reversed] (the apex edge, reversed, for a pointed cone), so the material lies
//! to the left of each about the natural (outward) normal. Curved faces use the natural normal
//! (`reversed = false`); plane caps reuse the primitive's frame, so the bottom cap (outward −z)
//! is `reversed = true` and the top cap is not. Euler–Poincaré (degenerate edges excluded from E,
//! a loopless torus face counted as one handle): genus 0 for all but the torus (genus 1).

use crate::{Brep, Curve, EdgeId, Frame, GResult, GeomError, Surface, P3, V3};
use std::f64::consts::TAU;

fn positive(name: &str, x: f64) -> GResult<()> {
    if x.is_finite() && x > 0.0 { Ok(()) } else { Err(GeomError::InvalidInput(format!("{name} must be finite and > 0, got {x}"))) }
}

/// Axis-aligned box with one corner at `corner` and edge lengths `size` (all > 0).
/// F6 E12 V8 L6. Face k-min/k-max lie on planes whose frame z is +axis k (x, y cyclic), so the
/// three min faces are `reversed`.
pub fn make_box(corner: P3, size: [f64; 3]) -> GResult<Brep> {
    for (i, s) in size.iter().enumerate() {
        positive(&format!("box size[{i}]"), *s)?;
    }
    let mut b = Brep::new();
    let solid = b.add_solid();
    let shell = b.add_shell(Some(solid));
    // vertex index bits: 1 = +x, 2 = +y, 4 = +z
    let vs: Vec<_> = (0..8)
        .map(|i| {
            let f = |bit: usize, k: usize| if i & bit != 0 { size[k] } else { 0.0 };
            b.add_vertex(corner + V3::new(f(1, 0), f(2, 1), f(4, 2)))
        })
        .collect();
    let axes = [V3::x(), V3::y(), V3::z()];
    // (axis, is_max, quad counter-clockwise seen from outside)
    let quads: [(usize, bool, [usize; 4]); 6] = [
        (0, false, [0, 4, 6, 2]),
        (0, true, [1, 3, 7, 5]),
        (1, false, [0, 1, 5, 4]),
        (1, true, [2, 6, 7, 3]),
        (2, false, [0, 2, 3, 1]),
        (2, true, [4, 5, 7, 6]),
    ];
    let mut edges: Vec<((usize, usize), EdgeId)> = Vec::new();
    for (k, is_max, q) in quads {
        let origin = if is_max { corner + axes[k] * size[k] } else { corner };
        let frame = Frame { origin, x: axes[(k + 1) % 3], y: axes[(k + 2) % 3], z: axes[k] };
        let face = b.add_face(shell, Surface::Plane { frame }, !is_max);
        let mut uses = Vec::new();
        for i in 0..4 {
            let (a, c) = (q[i], q[(i + 1) % 4]);
            let key = (a.min(c), a.max(c));
            let e = match edges.iter().find(|(k2, _)| *k2 == key) {
                Some(&(_, e)) => e,
                None => {
                    let e = b.add_line_edge(vs[key.0], vs[key.1]);
                    edges.push((key, e));
                    e
                }
            };
            uses.push((e, a != key.0));
        }
        b.add_loop(face, &uses);
    }
    Ok(b)
}

/// Right circular cylinder on `frame` (base centre = origin, axis = z) of radius `r`, height `h`.
/// F3 E2 V2 L4: side cylinder face (loops [bottom circle], [top circle reversed]), bottom cap
/// (plane, reversed), top cap; vertices at `origin + r·x` and `origin + h·z + r·x`.
pub fn make_cylinder(frame: Frame, r: f64, h: f64) -> GResult<Brep> {
    positive("cylinder radius", r)?;
    positive("cylinder height", h)?;
    Ok(revolved(frame, r, r, h, Surface::Cylinder { frame, radius: r }))
}

/// Cone or frustum on `frame`: base radius `r0` at z = 0, top radius `r1` at z = h (`r1` may be
/// 0 for a pointed cone; `r0 == r1` is rejected — use a cylinder). `r0 = 0` (apex at the base)
/// is accepted symmetrically. Frustum: F3 E2 V2 L4 like the cylinder. Pointed cone: F2 E2 V2 L3
/// — the side cone face (loops [base circle], [apex edge]) and one cap; edges = base circle and
/// a degenerate apex edge (no curve, parameter range u ∈ [0, 2π]); vertices = base circle vertex
/// and apex. The surface is `Surface::Cone` with radius `r0`, half angle atan((r1 − r0)/h).
pub fn make_cone(frame: Frame, r0: f64, r1: f64, h: f64) -> GResult<Brep> {
    positive("cone height", h)?;
    if !(r0.is_finite() && r1.is_finite() && r0 >= 0.0 && r1 >= 0.0 && r0.max(r1) > 0.0) {
        return Err(GeomError::InvalidInput(format!("cone radii must be ≥ 0 and not both 0, got {r0}, {r1}")));
    }
    if r0 == r1 {
        return Err(GeomError::InvalidInput("cone with r0 == r1 is a cylinder; use make_cylinder".into()));
    }
    Ok(revolved(frame, r0, r1, h, Surface::Cone { frame, radius: r0, half_angle: ((r1 - r0) / h).atan() }))
}

/// Shared builder for cylinder / cone / frustum (a radius of 0 gives a degenerate apex edge).
fn revolved(frame: Frame, r0: f64, r1: f64, h: f64, lateral: Surface) -> Brep {
    let mut b = Brep::new();
    let solid = b.add_solid();
    let shell = b.add_shell(Some(solid));
    let top = Frame { origin: frame.origin + frame.z * h, ..frame };
    let ring = |b: &mut Brep, f: Frame, r: f64| {
        let v = b.add_vertex(f.origin + f.x * r);
        if r > 0.0 { b.add_edge(Some(Curve::Circle { frame: f, radius: r }), 0.0, TAU, v, v) } else { b.add_degenerate_edge(v, 0.0, TAU) }
    };
    // ASM lists the circle before the apex edge
    let (c0, c1) = if r0 > 0.0 {
        let c0 = ring(&mut b, frame, r0);
        (c0, ring(&mut b, top, r1))
    } else {
        let c1 = ring(&mut b, top, r1);
        (ring(&mut b, frame, r0), c1)
    };
    let side = b.add_face(shell, lateral, false);
    b.add_loop(side, &[(c0, false)]);
    b.add_loop(side, &[(c1, true)]);
    if r0 > 0.0 {
        let f = b.add_face(shell, Surface::Plane { frame }, true);
        b.add_loop(f, &[(c0, true)]);
    }
    if r1 > 0.0 {
        let f = b.add_face(shell, Surface::Plane { frame: top }, false);
        b.add_loop(f, &[(c1, false)]);
    }
    b
}

/// Sphere of radius `r` about `center` (world axes). F1 E0 V0 L0: a single sphere face with no
/// loops (its parameter domain is the whole sphere).
pub fn make_sphere(center: P3, r: f64) -> GResult<Brep> {
    positive("sphere radius", r)?;
    let mut b = Brep::new();
    let solid = b.add_solid();
    let shell = b.add_shell(Some(solid));
    b.add_face(shell, Surface::Sphere { frame: Frame { origin: center, ..Frame::world() }, radius: r }, false);
    Ok(b)
}

/// Torus on `frame` (axis = z) with `major` > `minor` > 0. F1 E0 V0 L0: a single torus face with
/// no loops (genus 1). Spindle and horn tori (minor ≥ major), which ASM accepts as the outer
/// "apple" surface with apex edges, are rejected here.
pub fn make_torus(frame: Frame, major: f64, minor: f64) -> GResult<Brep> {
    positive("torus minor radius", minor)?;
    if !(major.is_finite() && major > minor) {
        return Err(GeomError::InvalidInput(format!("torus needs major > minor, got {major} ≤ {minor}")));
    }
    let mut b = Brep::new();
    let solid = b.add_solid();
    let shell = b.add_shell(Some(solid));
    b.add_face(shell, Surface::Torus { frame, major, minor }, false);
    Ok(b)
}
