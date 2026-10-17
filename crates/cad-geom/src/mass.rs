//! Mass properties of solids and faces (unit density), computed from the exact geometry by
//! integrating over each face's parameter domain (divergence theorem), not from a mesh.
//!
//! Method. With P measured from a reference point c near the solid and N = S_u × S_v the
//! natural normal, the divergence theorem gives (div(P) = 3, div(P xᵢ) = 4xᵢ,
//! div(P xᵢxⱼ) = 5xᵢxⱼ):
//!   V = ⅓∮ P·n dA,   ∫xᵢ dV = ¼∮ xᵢ (P·n) dA,   ∫xᵢxⱼ dV = ⅕∮ xᵢxⱼ (P·n) dA.
//! Each face integral ∫∫_D g(u,v) du dv over the trimmed domain D is turned into a boundary
//! integral by Green's theorem, ∫∫_D g = ∮_{∂D} G dv with G(u,v) = ∫_{u_ref}^{u} g(s,v) ds,
//! evaluated on the pcurves of the face's loops (see [`crate::param`]). Loops run
//! counter-clockwise in (u,v) for a face with `reversed = false` and clockwise otherwise, so the
//! loop integral already carries the outward-normal sign for the flux terms; the area integrand
//! |N| is multiplied by ±1. A face with no loops (closed sphere or torus, ASM's convention) is
//! integrated over its full periodic domain directly.
//!
//! Both seam conventions are handled. Seamed loops (OCCT) close in the (u, v) plane. A seamless
//! loop (ASM: a closed circle bounding a cylinder band) winds w = ±1 times round the u period;
//! the domain is then cut along a virtual seam, which adds −w·Ψ(v₀) per wrapping loop, where
//! Φ(v) = ∫ over one u period of g(s, v) ds, Ψ(v) = ∫₀^v Φ, and v₀ is v at the loop's start
//! vertex (the sum is independent of where the cut and the starting points are, because
//! G(u + T, v) − G(u, v) = Φ(v)). When the windings do not cancel, the residual belongs to a
//! point inside the domain: the apex of a cone (ASM's apex loop is a lone degenerate edge) or a
//! sphere pole; on a torus the two wrapping circles are lifted so the domain lies between them.
//! A sphere face whose loops all close in (u, v) and run clockwise about it (a ball with a cap or
//! an octant removed away from the poles) is the complement of what they enclose and contains
//! both poles: the whole-sphere integral is added to the (negative) loop integral.
//!
//! Quadrature and error. Both the boundary integral (over each edge's curve parameter) and the
//! inner integral G use composite 16-point Gauss–Legendre: one or two pieces where the integrand
//! is polynomial (lines; the u direction of planes, where g has degree ≤ 3) and pieces of at
//! most π/8 in angular parameters. For the analytic primitives every integrand is a polynomial
//! or a low-order trigonometric polynomial (frequency ≤ ~6), for which the rule's truncation
//! error (~(kh)^32/32!) is far below rounding, so the results are exact to a few ulps times the
//! number of terms (≈1e-14 relative). Moments are taken about the vertex-bounds centre and then
//! shifted with the parallel-axis theorem, which keeps tiny (1e-2 mm) and huge (1e5 mm) models
//! equally accurate. NURBS faces or edges return `GeomError::Unsupported`.

use crate::param::{angular_pieces, composite_nodes, edge_pieces, walk_loop, LoopWalk};
use crate::{Brep, FaceId, GResult, GeomError, SolidId, Surface, P3, V3};
use serde::{Deserialize, Serialize};
use std::f64::consts::{FRAC_PI_2, TAU};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MassProperties {
    /// mm³
    pub volume: f64,
    /// Total boundary area, mm².
    pub area: f64,
    /// Centre of mass (unit density).
    pub centroid: P3,
    /// Inertia tensor about the centroid, unit density (mm⁵), world axes. Standard tensor with
    /// negative products of inertia: I_xx = ∫(y² + z²) dV, I_xy = −∫xy dV (x, y, z measured
    /// from the centroid), and so on.
    pub inertia: [[f64; 3]; 3],
}

/// Number of accumulated quantities: V·3, (x,y,z)·4, (xx,yy,zz,xy,yz,zx)·5, area.
const NQ: usize = 11;

/// Integrand at (u, v): the moment densities with the natural normal, and |N|.
fn integrand(s: &Surface, c: &P3, u: f64, v: f64) -> [f64; NQ] {
    let p: V3 = s.point(u, v) - c;
    let (su, sv) = s.d1(u, v);
    let n = su.cross(&sv);
    let q = p.dot(&n);
    let (x, y, z) = (p.x, p.y, p.z);
    let (q4, q5) = (q / 4.0, q / 5.0);
    [q / 3.0, x * q4, y * q4, z * q4, x * x * q5, y * y * q5, z * z * q5, x * y * q5, y * z * q5, z * x * q5, n.norm()]
}

/// G(u, v) = ∫_{u_ref}^{u} g(s, v) ds by composite Gauss–Legendre.
fn inner(s: &Surface, c: &P3, u_ref: f64, u: f64, v: f64) -> [f64; NQ] {
    let pieces = if matches!(s, Surface::Plane { .. }) { 1 } else { angular_pieces(u - u_ref) };
    let mut acc = [0.0; NQ];
    for (t, w) in composite_nodes(u_ref, u, pieces) {
        let g = integrand(s, c, t, v);
        for k in 0..NQ {
            acc[k] += w * g[k];
        }
    }
    acc
}

/// Raw face integrals about `c`: flux moments with the outward normal (entries 0..10) and the
/// *signed* area (entry 10; negative when the loops run against the face orientation).
pub(crate) fn face_integrals(brep: &Brep, face: FaceId, c: &P3) -> GResult<[f64; NQ]> {
    let f = brep.face(face);
    let s = &f.surface;
    if matches!(s, Surface::Nurbs(_)) {
        return Err(GeomError::Unsupported(format!("mass properties of NURBS face F{}", face.0)));
    }
    let sign = if f.reversed { -1.0 } else { 1.0 };
    let mut acc = [0.0; NQ];
    if f.loops.is_empty() {
        if !matches!(s, Surface::Sphere { .. } | Surface::Torus { .. }) {
            return Err(GeomError::InvalidInput(format!("face F{} has no loops but its surface is not closed", face.0)));
        }
        let full = full_domain(s, c);
        for k in 0..NQ {
            acc[k] = full[k] * sign;
        }
        acc[NQ - 1] *= sign; // area is orientation-free
        return Ok(acc);
    }
    let sampler = |e: &crate::topology::Edge| Ok(composite_nodes(e.t0, e.t1, edge_pieces(e)?));
    let walks = f.loops.iter().map(|&lp| walk_loop(brep, face, lp, &sampler)).collect::<GResult<Vec<_>>>()?;
    // 1. ∮ G dv along each loop's continuous lift
    let mut u_ref = None;
    for track in walks.iter().flat_map(|w| &w.tracks) {
        for smp in &track.samples {
            let dv = smp.duv[1];
            if dv == 0.0 {
                continue;
            }
            let ur = *u_ref.get_or_insert(smp.uv[0]);
            let g = inner(s, c, ur, smp.uv[0], smp.uv[1]);
            let wt = track.sign * smp.w * dv;
            for k in 0..NQ {
                acc[k] += wt * g[k];
            }
        }
    }
    // 2. virtual seam for loops that wrap the u period: −w·Ψ(v_start) per loop
    for (w, v) in wrap_terms(face, f.reversed, s, &walks)? {
        let psi = psi(s, c, v);
        for k in 0..NQ {
            acc[k] -= w as f64 * psi[k];
        }
    }
    // 3. a sphere face whose loops all close in (u, v) bounds either the regions they enclose
    // (loops counter-clockwise about the face: positive signed area) or, when they run
    // clockwise, the rest of the sphere, which contains both poles (a face containing exactly
    // one pole has a loop winding round the axis). Green's theorem then gives minus the
    // enclosed part; the whole sphere is added.
    if matches!(s, Surface::Sphere { .. }) && walks.iter().all(|w| w.winding == [0, 0]) && acc[NQ - 1] * sign < 0.0 {
        let full = full_domain(s, c);
        for k in 0..NQ {
            acc[k] += sign * full[k];
        }
    }
    acc[NQ - 1] *= sign;
    Ok(acc)
}

/// Integrals over the whole parameter domain of a closed surface (sphere, torus), natural
/// orientation.
fn full_domain(s: &Surface, c: &P3) -> [f64; NQ] {
    let (v0, v1) = match s {
        Surface::Torus { .. } => (0.0, TAU),
        _ => (-FRAC_PI_2, FRAC_PI_2),
    };
    let mut acc = [0.0; NQ];
    for (v, wv) in composite_nodes(v0, v1, angular_pieces(v1 - v0)) {
        for (u, wu) in composite_nodes(0.0, TAU, angular_pieces(TAU)) {
            let g = integrand(s, c, u, v);
            for k in 0..NQ {
                acc[k] += wu * wv * g[k];
            }
        }
    }
    acc
}

/// Winding terms (w, v_start) of the virtual seam: one per u-wrapping loop, plus the residual
/// winding at a pole or apex inside the domain (a sphere cap bounded by one circle, or ASM's
/// cone whose apex loop is a lone degenerate edge). The windings of a face that bounds a finite
/// region sum to zero once the residual is added.
pub(crate) fn wrap_terms(face: FaceId, reversed: bool, s: &Surface, walks: &[LoopWalk]) -> GResult<Vec<(i64, f64)>> {
    Ok(wrap_terms_indexed(face, reversed, s, walks)?.into_iter().map(|(w, v, _)| (w, v)).collect())
}

/// [`wrap_terms`] with the walk index of each term (`None` for the pole/apex residual).
pub(crate) fn wrap_terms_indexed(face: FaceId, reversed: bool, s: &Surface, walks: &[LoopWalk]) -> GResult<Vec<(i64, f64, Option<usize>)>> {
    let mut terms: Vec<(i64, f64, Option<usize>)> = walks
        .iter()
        .enumerate()
        .filter(|(_, w)| w.winding[0] != 0)
        .map(|(i, w)| (w.winding[0], w.start.map_or(0.0, |p| p[1]), Some(i)))
        .collect();
    let total: i64 = terms.iter().map(|t| t.0).sum();
    if total != 0 {
        let pole = match s {
            Surface::Sphere { .. } => if (total > 0) != reversed { FRAC_PI_2 } else { -FRAC_PI_2 },
            Surface::Cone { radius, half_angle, .. } if half_angle.tan() != 0.0 => -radius / half_angle.tan(),
            _ => {
                return Err(GeomError::InvalidInput(format!(
                    "face F{}: loops wind {total} times round the axis and bound no finite region",
                    face.0
                )))
            }
        };
        terms.push((-total, pole, None));
    }
    if let (Some(per), false) = (s.periods().1, terms.is_empty()) {
        // v is periodic too (torus): the two boundary circles must be lifted so that the domain
        // lies between them; the lower one is the loop with material on its +v side
        if terms.len() != 2 {
            return Err(GeomError::Unsupported(format!(
                "face F{}: {} u-wrapping loops on a doubly periodic surface",
                face.0,
                terms.len()
            )));
        }
        let lower = usize::from((terms[0].0 > 0) == reversed);
        let (vl, vu) = (terms[lower].1, terms[1 - lower].1);
        // into (vl, vl + per]
        terms[1 - lower].1 = vu - per * ((vu - vl) / per).ceil() + per;
    }
    Ok(terms)
}

/// Ψ(v) = ∫_0^v Φ(v′) dv′ with Φ(v′) = ∫ over one u period of g(s, v′) ds.
fn psi(s: &Surface, c: &P3, v: f64) -> [f64; NQ] {
    let per = s.periods().0.unwrap_or(TAU);
    let pieces = if s.periods().1.is_some() || matches!(s, Surface::Sphere { .. }) { angular_pieces(v) } else { 2 };
    let mut acc = [0.0; NQ];
    for (vv, wv) in composite_nodes(0.0, v, pieces) {
        for (u, wu) in composite_nodes(0.0, per, angular_pieces(per)) {
            let g = integrand(s, c, u, vv);
            for k in 0..NQ {
                acc[k] += wu * wv * g[k];
            }
        }
    }
    acc
}

/// Area of one face (mm²) from its exact geometry and loops. Fails when the loops run against
/// the face orientation (negative signed area), for NURBS faces, or for loops that do not close
/// in the parameter plane.
pub fn face_area(brep: &Brep, face: FaceId) -> GResult<f64> {
    let c = brep.vertex_bounds().map(|(a, b)| nalgebra::center(&a, &b)).unwrap_or_else(P3::origin);
    let a = face_integrals(brep, face, &c)?[NQ - 1];
    if a < 0.0 {
        return Err(GeomError::InvalidInput(format!("face F{}: loops run clockwise about the outward normal (signed area {a})", face.0)));
    }
    Ok(a)
}

/// Reference point for a solid: centre of the bounds of its vertices (frame origin of the first
/// face when there are none).
fn reference_point(brep: &Brep, solid: SolidId) -> P3 {
    let mut lo_hi: Option<(P3, P3)> = None;
    for &sh in &brep.solid(solid).shells {
        for v in brep.shell_vertices(sh) {
            let p = brep.vertex(v).point;
            lo_hi = Some(lo_hi.map_or((p, p), |(lo, hi)| (lo.inf(&p), hi.sup(&p))));
        }
    }
    match lo_hi {
        Some((lo, hi)) => nalgebra::center(&lo, &hi),
        None => brep.solid_faces(solid).find_map(|f| brep.face(f).surface.frame().map(|fr| fr.origin)).unwrap_or_else(P3::origin),
    }
}

/// Summed raw integrals of all faces of a solid about its reference point (signed volume first).
pub(crate) fn solid_integrals(brep: &Brep, solid: SolidId) -> GResult<(P3, [f64; NQ])> {
    let c = reference_point(brep, solid);
    let mut acc = [0.0; NQ];
    for f in brep.solid_faces(solid) {
        let g = face_integrals(brep, f, &c)?;
        for k in 0..NQ {
            acc[k] += if k == NQ - 1 { g[k].abs() } else { g[k] };
        }
    }
    Ok((c, acc))
}

/// Volume, boundary area, centroid and central inertia tensor of a solid at unit density
/// (void shells, oriented inward, subtract automatically). Fails for a non-positive volume
/// (inside-out or open solid) and for unsupported (NURBS) geometry.
pub fn solid_properties(brep: &Brep, solid: SolidId) -> GResult<MassProperties> {
    let (c, m) = solid_integrals(brep, solid)?;
    let vol = m[0];
    if vol.is_nan() || vol <= 0.0 {
        return Err(GeomError::InvalidInput(format!("solid S{} has non-positive volume {vol} (inside out or open)", solid.0)));
    }
    let d = V3::new(m[1], m[2], m[3]) / vol;
    // second moments about c, then about the centroid (parallel axis)
    let s = [[m[4], m[7], m[9]], [m[7], m[5], m[8]], [m[9], m[8], m[6]]];
    let mut sc = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            sc[i][j] = s[i][j] - vol * d[i] * d[j];
        }
    }
    let tr = sc[0][0] + sc[1][1] + sc[2][2];
    let mut inertia = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            inertia[i][j] = if i == j { tr - sc[i][i] } else { -sc[i][j] };
        }
    }
    Ok(MassProperties { volume: vol, area: m[NQ - 1], centroid: c + d, inertia })
}
