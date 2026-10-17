//! Rigid motions of B-reps. Only proper rotations plus translations are accepted (the types
//! cannot express a reflection), so frames stay right-handed and face orientation is preserved.
//! NURBS geometry is moved by transforming control points (affine invariance), never evaluated.

use crate::{Brep, Curve, Frame, Surface, P3, V3};
use nalgebra::{Isometry3, Rotation3, Translation3, UnitQuaternion};

/// Maps a point: p ↦ R·p + t.
fn point(rot: &Rotation3<f64>, tr: &V3, p: &P3) -> P3 {
    rot * p + tr
}

/// The frame moved rigidly (origin mapped, axes rotated).
pub fn transform_frame(f: &Frame, rot: &Rotation3<f64>, tr: &V3) -> Frame {
    Frame { origin: point(rot, tr, &f.origin), x: rot * f.x, y: rot * f.y, z: rot * f.z }
}

/// The curve moved rigidly; its parameterisation is unchanged.
pub fn transform_curve(c: &Curve, rot: &Rotation3<f64>, tr: &V3) -> Curve {
    match c {
        Curve::Line { origin, dir } => Curve::Line { origin: point(rot, tr, origin), dir: rot * dir },
        Curve::Circle { frame, radius } => Curve::Circle { frame: transform_frame(frame, rot, tr), radius: *radius },
        Curve::Ellipse { frame, major, minor } => {
            Curve::Ellipse { frame: transform_frame(frame, rot, tr), major: *major, minor: *minor }
        }
        Curve::Nurbs(n) => {
            let mut n = n.clone();
            n.control_points.iter_mut().for_each(|p| *p = point(rot, tr, p));
            Curve::Nurbs(n)
        }
    }
}

/// The surface moved rigidly; its parameterisation (and natural normal) is unchanged.
pub fn transform_surface(s: &Surface, rot: &Rotation3<f64>, tr: &V3) -> Surface {
    let f = |fr: &Frame| transform_frame(fr, rot, tr);
    match s {
        Surface::Plane { frame } => Surface::Plane { frame: f(frame) },
        Surface::Cylinder { frame, radius } => Surface::Cylinder { frame: f(frame), radius: *radius },
        Surface::Cone { frame, radius, half_angle } => Surface::Cone { frame: f(frame), radius: *radius, half_angle: *half_angle },
        Surface::Sphere { frame, radius } => Surface::Sphere { frame: f(frame), radius: *radius },
        Surface::Torus { frame, major, minor } => Surface::Torus { frame: f(frame), major: *major, minor: *minor },
        Surface::Nurbs(n) => {
            let mut n = n.clone();
            n.control_points.iter_mut().for_each(|p| *p = point(rot, tr, p));
            Surface::Nurbs(n)
        }
    }
}

impl Brep {
    /// Applies p ↦ `rotation`·p + `translation` to every vertex, curve and surface in place.
    /// Topology, parameter ranges and orientation flags are unchanged.
    pub fn transform(&mut self, rotation: &Rotation3<f64>, translation: &V3) {
        for v in &mut self.vertices {
            v.point = point(rotation, translation, &v.point);
        }
        for e in &mut self.edges {
            if let Some(c) = &e.curve {
                e.curve = Some(transform_curve(c, rotation, translation));
            }
        }
        for f in &mut self.faces {
            f.surface = transform_surface(&f.surface, rotation, translation);
        }
    }

    /// [`Brep::transform`] taking an isometry (rotation then translation).
    pub fn transform_iso(&mut self, iso: &Isometry3<f64>) {
        let rot: UnitQuaternion<f64> = iso.rotation;
        self.transform(&rot.to_rotation_matrix(), &iso.translation.vector);
    }

    /// Transformed copy.
    pub fn transformed(&self, rotation: &Rotation3<f64>, translation: &V3) -> Brep {
        let mut b = self.clone();
        b.transform(rotation, translation);
        b
    }
}

/// Convenience: isometry from an axis-angle rotation (axis need not be unit) and a translation.
pub fn isometry(axis: V3, angle: f64, translation: V3) -> Isometry3<f64> {
    Isometry3::from_parts(Translation3::from(translation), UnitQuaternion::from_scaled_axis(axis.normalize() * angle))
}
