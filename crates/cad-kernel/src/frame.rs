//! Sketch-plane coordinate frames and small vector helpers.
//!
//! Origin-plane frames reproduce Fusion exactly (Fusion 2705.1.30 probe, `Sketch.xDirection`,
//! `yDirection` and transform normal for sketches on the origin planes):
//!   XY: x = +X, y = +Y, n = +Z      XZ: x = +X, y = -Z, n = +Y      YZ: x = -Z, y = +Y, n = +X
//! A sketch on a planar face puts its origin at the world origin projected onto the face (probe:
//! top face at z = 20 mm -> origin (0, 0, 20), x = +X, y = +Y). The x axis rule below reproduces
//! all of these observations.

use cad_core::OriginPlane;

pub type V3 = [f64; 3];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub fn normalize(a: V3) -> V3 {
    let l = len(a);
    if l < 1e-15 { [0.0, 0.0, 0.0] } else { scale(a, 1.0 / l) }
}
pub fn dist(a: V3, b: V3) -> f64 {
    len(sub(a, b))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: V3,
    pub x: V3,
    pub y: V3,
    pub n: V3,
}

impl Frame {
    pub fn origin_plane(p: OriginPlane) -> Frame {
        match p {
            OriginPlane::XY => Frame { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], n: [0.0, 0.0, 1.0] },
            OriginPlane::XZ => Frame { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 0.0, -1.0], n: [0.0, 1.0, 0.0] },
            OriginPlane::YZ => Frame { origin: [0.0; 3], x: [0.0, 0.0, -1.0], y: [0.0, 1.0, 0.0], n: [1.0, 0.0, 0.0] },
        }
    }

    /// Frame on a plane through `point` with normal `n`, origin = world origin projected on it.
    pub fn on_plane(point: V3, n: V3) -> Frame {
        let n = normalize(n);
        let origin = scale(n, dot(point, n));
        let pick = |a: V3| normalize(sub(a, scale(n, dot(a, n))));
        let x = if dot([1.0, 0.0, 0.0], n).abs() < 0.9 { pick([1.0, 0.0, 0.0]) } else { pick([0.0, 0.0, -1.0]) };
        let y = cross(n, x);
        Frame { origin, x, y, n }
    }

    pub fn to_world(&self, p: [f64; 2]) -> V3 {
        add(self.origin, add(scale(self.x, p[0]), scale(self.y, p[1])))
    }
    pub fn to_local(&self, p: V3) -> [f64; 2] {
        let d = sub(p, self.origin);
        [dot(d, self.x), dot(d, self.y)]
    }
    /// Offset along the normal.
    pub fn offset(&self, d: f64) -> Frame {
        Frame { origin: add(self.origin, scale(self.n, d)), ..*self }
    }
    /// Intersection of a ray with the plane, in sketch coordinates.
    pub fn intersect_ray(&self, o: V3, dir: V3) -> Option<[f64; 2]> {
        let den = dot(dir, self.n);
        if den.abs() < 1e-12 {
            return None;
        }
        let t = dot(sub(self.origin, o), self.n) / den;
        Some(self.to_local(add(o, scale(dir, t))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_frames_match_fusion() {
        // probe: sketchToModelSpace((2, 3)) -> XY (2,3,0), XZ (2,0,-3), YZ (0,3,-2) [cm]
        assert_eq!(Frame::origin_plane(OriginPlane::XY).to_world([2.0, 3.0]), [2.0, 3.0, 0.0]);
        assert_eq!(Frame::origin_plane(OriginPlane::XZ).to_world([2.0, 3.0]), [2.0, 0.0, -3.0]);
        assert_eq!(Frame::origin_plane(OriginPlane::YZ).to_world([2.0, 3.0]), [0.0, 3.0, -2.0]);
    }

    #[test]
    fn plane_rule_reproduces_origin_frames_and_face_frame() {
        for p in [OriginPlane::XY, OriginPlane::XZ, OriginPlane::YZ] {
            let f = Frame::origin_plane(p);
            let g = Frame::on_plane([0.0; 3], f.n);
            for (a, b) in [(f.x, g.x), (f.y, g.y), (f.n, g.n)] {
                assert!(dist(a, b) < 1e-12, "{p:?}");
            }
        }
        // probe: sketch on the top face (z = 20 mm) -> origin (0,0,20), x = +X, y = +Y
        let f = Frame::on_plane([35.0, 15.0, 20.0], [0.0, 0.0, 1.0]);
        assert_eq!(f.origin, [0.0, 0.0, 20.0]);
        assert_eq!((f.x, f.y), ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
    }

    #[test]
    fn ray_intersection() {
        let f = Frame::origin_plane(OriginPlane::XY);
        assert_eq!(f.intersect_ray([5.0, 6.0, 10.0], [0.0, 0.0, -1.0]), Some([5.0, 6.0]));
        assert_eq!(f.intersect_ray([5.0, 6.0, 10.0], [1.0, 0.0, 0.0]), None);
    }
}
