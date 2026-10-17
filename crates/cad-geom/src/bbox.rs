//! Axis-aligned bounding boxes, and control-polygon bounds of NURBS (which contain the curve or
//! surface by the convex-hull property, since weights are positive).

use crate::{NurbsCurve, NurbsSurface, P3};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: P3,
    pub max: P3,
}

impl Aabb {
    /// Box of a non-empty point set (`None` when empty).
    pub fn from_points(points: &[P3]) -> Option<Self> {
        let first = *points.first()?;
        Some(points.iter().fold(Self { min: first, max: first }, |b, p| Self { min: b.min.inf(p), max: b.max.sup(p) }))
    }

    /// Grown by `d` on every side.
    pub fn inflated(&self, d: f64) -> Self {
        let e = nalgebra::Vector3::repeat(d);
        Self { min: self.min - e, max: self.max + e }
    }

    pub fn overlaps(&self, o: &Aabb) -> bool {
        (0..3).all(|i| self.min[i] <= o.max[i] && o.min[i] <= self.max[i])
    }

    pub fn contains(&self, p: &P3) -> bool {
        (0..3).all(|i| self.min[i] <= p[i] && p[i] <= self.max[i])
    }

    pub fn union(&self, o: &Aabb) -> Self {
        Self { min: self.min.inf(&o.min), max: self.max.sup(&o.max) }
    }

    pub fn diagonal(&self) -> f64 {
        (self.max - self.min).norm()
    }

    pub fn center(&self) -> P3 {
        nalgebra::center(&self.min, &self.max)
    }
}

impl NurbsCurve {
    /// Box of the control polygon; contains the curve on its domain.
    pub fn control_bbox(&self) -> Aabb {
        Aabb::from_points(&self.control_points).expect("validated curve has control points")
    }
}

impl NurbsSurface {
    /// Box of the control net; contains the surface on its domain.
    pub fn control_bbox(&self) -> Aabb {
        Aabb::from_points(&self.control_points).expect("validated surface has control points")
    }
}
