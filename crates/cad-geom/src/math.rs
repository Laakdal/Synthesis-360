//! Points, vectors and right-handed orthonormal frames.

use serde::{Deserialize, Serialize};

pub type P3 = nalgebra::Point3<f64>;
pub type V3 = nalgebra::Vector3<f64>;

/// Right-handed orthonormal frame: `z = x × y`. Analytic curves and surfaces are defined in one.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub origin: P3,
    pub x: V3,
    pub y: V3,
    pub z: V3,
}

impl Frame {
    pub fn world() -> Self {
        Self { origin: P3::origin(), x: V3::x(), y: V3::y(), z: V3::z() }
    }

    /// Frame with the given origin and z axis; x is chosen deterministically perpendicular to z
    /// (world X projected, or world Y when z is parallel to X).
    pub fn from_origin_z(origin: P3, z: V3) -> Self {
        let z = z.normalize();
        let seed = if z.x.abs() < 0.9 { V3::x() } else { V3::y() };
        let x = (seed - z * seed.dot(&z)).normalize();
        Self { origin, x, y: z.cross(&x), z }
    }

    /// Frame from origin, z axis and an x direction (made perpendicular to z).
    pub fn from_origin_z_x(origin: P3, z: V3, x: V3) -> Self {
        let z = z.normalize();
        let x = (x - z * x.dot(&z)).normalize();
        Self { origin, x, y: z.cross(&x), z }
    }

    pub fn to_world(&self, local: P3) -> P3 {
        self.origin + self.x * local.x + self.y * local.y + self.z * local.z
    }

    pub fn dir_to_world(&self, local: V3) -> V3 {
        self.x * local.x + self.y * local.y + self.z * local.z
    }

    pub fn to_local(&self, world: P3) -> P3 {
        let d = world - self.origin;
        P3::new(d.dot(&self.x), d.dot(&self.y), d.dot(&self.z))
    }
}
