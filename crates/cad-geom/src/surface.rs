//! Surfaces. Parameterisations (fixed; topology, mass properties and tests rely on them), all in
//! the surface's `frame` (axis = frame.z):
//! * `Plane`:    S(u,v) = o + u·x + v·y
//! * `Cylinder`: S(u,v) = o + r(cos u·x + sin u·y) + v·z
//! * `Cone`:     S(u,v) = o + (r + v·tan α)(cos u·x + sin u·y) + v·z   (r = radius at v = 0,
//!   α = half angle, signed: negative narrows towards +z; apex at v = −r / tan α)
//! * `Sphere`:   S(u,v) = o + r cos v(cos u·x + sin u·y) + r sin v·z,  v ∈ [−π/2, π/2]
//! * `Torus`:    S(u,v) = o + (R + r cos v)(cos u·x + sin u·y) + r sin v·z
//! * `Nurbs`:    see `nurbs.rs`.
//!
//! The natural normal is du × dv normalised: it points away from the axis for cylinder, cone
//! (for α within ±π/2), sphere and torus, and along +z for the plane.

use crate::nurbs::unit_arc;
use crate::{Frame, GResult, GeomError, NurbsSurface, P3, V3};
use serde::{Deserialize, Serialize};

/// Sphere v ranges may overshoot ±π/2 by this much (round-off in callers' angle arithmetic).
const POLE_SLACK: f64 = 1e-12;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Surface {
    Plane { frame: Frame },
    Cylinder { frame: Frame, radius: f64 },
    Cone { frame: Frame, radius: f64, half_angle: f64 },
    Sphere { frame: Frame, radius: f64 },
    Torus { frame: Frame, major: f64, minor: f64 },
    Nurbs(NurbsSurface),
}

impl Surface {
    pub fn frame(&self) -> Option<&Frame> {
        match self {
            Surface::Plane { frame }
            | Surface::Cylinder { frame, .. }
            | Surface::Cone { frame, .. }
            | Surface::Sphere { frame, .. }
            | Surface::Torus { frame, .. } => Some(frame),
            Surface::Nurbs(_) => None,
        }
    }

    /// Short type name used in reports and differential comparisons.
    pub fn type_name(&self) -> &'static str {
        match self {
            Surface::Plane { .. } => "plane",
            Surface::Cylinder { .. } => "cylinder",
            Surface::Cone { .. } => "cone",
            Surface::Sphere { .. } => "sphere",
            Surface::Torus { .. } => "torus",
            Surface::Nurbs(_) => "nurbs",
        }
    }

    pub fn point(&self, u: f64, v: f64) -> P3 {
        match self {
            Surface::Plane { frame } => frame.origin + frame.x * u + frame.y * v,
            Surface::Cylinder { frame, radius } => frame.origin + radial(frame, u) * *radius + frame.z * v,
            Surface::Cone { frame, radius, half_angle } => {
                frame.origin + radial(frame, u) * (radius + v * half_angle.tan()) + frame.z * v
            }
            Surface::Sphere { frame, radius } => frame.origin + radial(frame, u) * (radius * v.cos()) + frame.z * (radius * v.sin()),
            Surface::Torus { frame, major, minor } => {
                frame.origin + radial(frame, u) * (major + minor * v.cos()) + frame.z * (minor * v.sin())
            }
            Surface::Nurbs(n) => n.point(u, v),
        }
    }

    /// Partial derivatives (∂S/∂u, ∂S/∂v).
    pub fn d1(&self, u: f64, v: f64) -> (V3, V3) {
        match self {
            Surface::Plane { frame } => (frame.x, frame.y),
            Surface::Cylinder { frame, radius } => (tangential(frame, u) * *radius, frame.z),
            Surface::Cone { frame, radius, half_angle } => {
                let t = half_angle.tan();
                (tangential(frame, u) * (radius + v * t), radial(frame, u) * t + frame.z)
            }
            Surface::Sphere { frame, radius } => (
                tangential(frame, u) * (radius * v.cos()),
                radial(frame, u) * (-radius * v.sin()) + frame.z * (radius * v.cos()),
            ),
            Surface::Torus { frame, major, minor } => (
                tangential(frame, u) * (major + minor * v.cos()),
                radial(frame, u) * (-minor * v.sin()) + frame.z * (minor * v.cos()),
            ),
            Surface::Nurbs(n) => n.d1(u, v),
        }
    }

    /// Unit natural normal du × dv; `None` where the surface is singular (poles, apex).
    pub fn normal(&self, u: f64, v: f64) -> Option<V3> {
        let (du, dv) = self.d1(u, v);
        let n = du.cross(&dv);
        let len = n.norm();
        (len > 1e-14).then(|| n / len)
    }

    /// Periods in u and v (None = not periodic in that direction).
    pub fn periods(&self) -> (Option<f64>, Option<f64>) {
        use std::f64::consts::TAU;
        match self {
            Surface::Plane { .. } => (None, None),
            Surface::Cylinder { .. } | Surface::Cone { .. } | Surface::Sphere { .. } => (Some(TAU), None),
            Surface::Torus { .. } => (Some(TAU), Some(TAU)),
            Surface::Nurbs(n) => n.periods(),
        }
    }

    /// Second partials (S_uu, S_uv, S_vv).
    pub fn d2(&self, u: f64, v: f64) -> (V3, V3, V3) {
        let z = V3::zeros();
        match self {
            Surface::Plane { .. } => (z, z, z),
            Surface::Cylinder { frame, radius } => (-radial(frame, u) * *radius, z, z),
            Surface::Cone { frame, radius, half_angle } => {
                let t = half_angle.tan();
                (-radial(frame, u) * (radius + v * t), tangential(frame, u) * t, z)
            }
            Surface::Sphere { frame, radius } => {
                let (c, s) = (v.cos(), v.sin());
                let meridian = -(radial(frame, u) * (radius * c) + frame.z * (radius * s));
                (-radial(frame, u) * (radius * c), -tangential(frame, u) * (radius * s), meridian)
            }
            Surface::Torus { frame, major, minor } => {
                let (c, s) = (v.cos(), v.sin());
                let meridian = -(radial(frame, u) * (minor * c) + frame.z * (minor * s));
                (-radial(frame, u) * (major + minor * c), -tangential(frame, u) * (minor * s), meridian)
            }
            Surface::Nurbs(n) => n.d2(u, v),
        }
    }

    /// Natural parameter ranges `((u0, u1), (v0, v1))`; infinite where the surface is unbounded
    /// (plane; v of cylinder and of the double cone). Sphere v is [−π/2, π/2].
    pub fn domain(&self) -> ((f64, f64), (f64, f64)) {
        use std::f64::consts::{FRAC_PI_2, TAU};
        let all = (f64::NEG_INFINITY, f64::INFINITY);
        match self {
            Surface::Plane { .. } => (all, all),
            Surface::Cylinder { .. } | Surface::Cone { .. } => ((0.0, TAU), all),
            Surface::Sphere { .. } => ((0.0, TAU), (-FRAC_PI_2, FRAC_PI_2)),
            Surface::Torus { .. } => ((0.0, TAU), (0.0, TAU)),
            Surface::Nurbs(n) => n.domain(),
        }
    }

    /// Parameters wrapped into `[0, 2π)` (or the NURBS domain) in periodic directions.
    pub fn wrap(&self, u: f64, v: f64) -> (f64, f64) {
        if let Surface::Nurbs(n) = self {
            return n.wrap(u, v);
        }
        let (pu, pv) = self.periods();
        let w = |x: f64, p: f64| {
            let r = x.rem_euclid(p);
            if r >= p { 0.0 } else { r }
        };
        (pu.map_or(u, |p| w(u, p)), pv.map_or(v, |p| w(v, p)))
    }

    /// Exact rational NURBS form of the surface over `[u0, u1] × [v0, v1]` (finite, increasing;
    /// angular ranges at most 2π, sphere v within [−π/2, π/2]). Plane: bilinear patch with the
    /// same parameterisation. Cylinder, cone, sphere, torus: surfaces of revolution (P&T A8.1)
    /// of a line or rational-quadratic arc profile swept by a rational-quadratic arc (A7.1).
    /// The result is the same point set as the analytic surface, but in the angular directions
    /// the NURBS parameter equals the angle only at the knots. A cone apex or sphere pole inside
    /// the range gives coincident control points (a valid degenerate edge).
    pub fn to_nurbs(&self, u: (f64, f64), v: (f64, f64)) -> GResult<NurbsSurface> {
        let ok = |r: (f64, f64)| r.0.is_finite() && r.1.is_finite() && r.0 < r.1;
        if !ok(u) || !ok(v) {
            return Err(GeomError::InvalidInput(format!("to_nurbs: invalid ranges {u:?} × {v:?}")));
        }
        let lin = |r: (f64, f64)| vec![r.0, r.0, r.1, r.1];
        match self {
            Surface::Plane { .. } => {
                let cps = vec![self.point(u.0, v.0), self.point(u.0, v.1), self.point(u.1, v.0), self.point(u.1, v.1)];
                NurbsSurface::new(1, 1, lin(u), lin(v), cps, vec![1.0; 4], 2, 2)
            }
            Surface::Cylinder { frame, radius } => revolve(frame, u, 1, lin(v), &[(*radius, v.0), (*radius, v.1)], &[1.0, 1.0]),
            Surface::Cone { frame, radius, half_angle } => {
                let t = half_angle.tan();
                let prof = [(radius + v.0 * t, v.0), (radius + v.1 * t, v.1)];
                revolve(frame, u, 1, lin(v), &prof, &[1.0, 1.0])
            }
            Surface::Sphere { frame, radius } => {
                if v.0 < -std::f64::consts::FRAC_PI_2 - POLE_SLACK || v.1 > std::f64::consts::FRAC_PI_2 + POLE_SLACK {
                    return Err(GeomError::InvalidInput(format!("to_nurbs: sphere v range {v:?} outside [−π/2, π/2]")));
                }
                let (kv, pts, wv) = unit_arc(v.0, v.1)?;
                let prof: Vec<(f64, f64)> = pts.iter().map(|q| (radius * q[0], radius * q[1])).collect();
                revolve(frame, u, 2, kv, &prof, &wv)
            }
            Surface::Torus { frame, major, minor } => {
                let (kv, pts, wv) = unit_arc(v.0, v.1)?;
                let prof: Vec<(f64, f64)> = pts.iter().map(|q| (major + minor * q[0], minor * q[1])).collect();
                revolve(frame, u, 2, kv, &prof, &wv)
            }
            Surface::Nurbs(n) => n.extract(u, v),
        }
    }
}

fn radial(f: &Frame, u: f64) -> V3 {
    f.x * u.cos() + f.y * u.sin()
}

fn tangential(f: &Frame, u: f64) -> V3 {
    f.y * u.cos() - f.x * u.sin()
}

/// Surface of revolution about `frame.z` (A8.1): the profile `(radial, axial)` control points
/// with weights `wv` (degree `qv`, knots `kv`) swept through the angles `u`.
fn revolve(frame: &Frame, u: (f64, f64), qv: usize, kv: Vec<f64>, prof: &[(f64, f64)], wv: &[f64]) -> GResult<NurbsSurface> {
    let (ku, arc, wu) = unit_arc(u.0, u.1)?;
    let mut cps = Vec::with_capacity(arc.len() * prof.len());
    let mut w = Vec::with_capacity(cps.capacity());
    for (c, wi) in arc.iter().zip(&wu) {
        let dir = frame.x * c[0] + frame.y * c[1];
        for ((x, z), wj) in prof.iter().zip(wv) {
            cps.push(frame.origin + dir * *x + frame.z * *z);
            w.push(wi * wj);
        }
    }
    NurbsSurface::new(2, qv, ku, kv, cps, w, arc.len(), prof.len())
}
