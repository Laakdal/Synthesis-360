//! CPU picking against the kernel's display meshes (faces), edge polylines (screen distance),
//! visible sketch profiles and origin planes.

use super::camera::Camera;
use crate::state::{AppState, Pick};
use cad_core::OriginPlane;
use glam::{Vec2, Vec3};

pub struct PickContext<'a> {
    pub cam: &'a Camera,
    pub aspect: f32,
    /// Viewport size in points.
    pub size: Vec2,
    /// Cursor in NDC.
    pub ndc: Vec2,
}

fn ray_tri(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() < 1e-12 { 0.0 } else { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) };
    (a + ab * t - p).length()
}

/// Nearest face hit along the ray: (body id, face index, distance).
pub fn pick_face(state: &AppState, o: Vec3, d: Vec3) -> Option<(String, usize, f32)> {
    let mut best: Option<(String, usize, f32)> = None;
    for b in state.visible_bodies() {
        let m = b.mesh();
        let v = |i: u32| Vec3::new(m.positions[i as usize * 3], m.positions[i as usize * 3 + 1], m.positions[i as usize * 3 + 2]);
        for (fi, &(s, e)) in m.face_ranges.iter().enumerate() {
            for t in (s..e).step_by(3) {
                let (a, bb, c) = (v(m.indices[t as usize]), v(m.indices[t as usize + 1]), v(m.indices[t as usize + 2]));
                if let Some(dist) = ray_tri(o, d, a, bb, c) {
                    if best.as_ref().is_none_or(|x| dist < x.2) {
                        best = Some((b.id.clone(), fi, dist));
                    }
                }
            }
        }
    }
    best
}

/// Nearest edge within `px` pixels of the cursor (screen space).
pub fn pick_edge(state: &AppState, ctx: &PickContext, px: f32) -> Option<(String, usize, f32)> {
    let cursor = ctx.ndc;
    let to_px = |n: Vec2| Vec2::new(n.x * ctx.size.x * 0.5, n.y * ctx.size.y * 0.5);
    let c = to_px(cursor);
    let mut best: Option<(String, usize, f32)> = None;
    for b in state.visible_bodies() {
        let m = b.mesh();
        for (ei, poly) in m.edge_lines.iter().enumerate() {
            let pts: Vec<Option<Vec2>> = poly.iter().map(|p| ctx.cam.project(Vec3::from(*p), ctx.aspect).map(to_px)).collect();
            for w in pts.windows(2) {
                if let (Some(a), Some(bb)) = (w[0], w[1]) {
                    let dd = seg_dist(c, a, bb);
                    if dd <= px && best.as_ref().is_none_or(|x| dd < x.2) {
                        best = Some((b.id.clone(), ei, dd));
                    }
                }
            }
        }
    }
    best
}

/// Profile of a visible, finished sketch under the cursor.
pub fn pick_profile(state: &AppState, o: Vec3, d: Vec3, max_t: f32) -> Option<Pick> {
    let mut best: Option<(f32, Pick)> = None;
    for f in state.doc.features.iter() {
        let cad_core::FeatureKind::Sketch { visible, .. } = &f.kind else { continue };
        if !*visible || !state.doc.is_active(&f.id) || !state.doc.component_visible(f.component.as_deref()) {
            continue;
        }
        let Some(r) = state.model.sketches.get(&f.id) else { continue };
        let n = Vec3::new(r.frame.n[0] as f32, r.frame.n[1] as f32, r.frame.n[2] as f32);
        let den = d.dot(n);
        if den.abs() < 1e-9 {
            continue;
        }
        let org = Vec3::new(r.frame.origin[0] as f32, r.frame.origin[1] as f32, r.frame.origin[2] as f32);
        let t = (org - o).dot(n) / den;
        if t <= 0.0 || t > max_t {
            continue;
        }
        let hit = o + d * t;
        let local = r.frame.to_local([hit.x as f64, hit.y as f64, hit.z as f64]);
        // smallest containing profile wins (a disc inside a ring)
        let mut inner: Option<(f64, usize)> = None;
        for (i, p) in r.profiles.iter().enumerate() {
            if p.contains(local) && inner.is_none_or(|(a, _)| p.area < a) {
                inner = Some((p.area, i));
            }
        }
        if let Some((_, i)) = inner {
            if best.as_ref().is_none_or(|(bt, _)| t < *bt) {
                best = Some((t, Pick::Profile { sketch: f.id.clone(), index: i }));
            }
        }
    }
    best.map(|b| b.1)
}

/// Origin planes are drawn as squares of half-size `half` around the origin.
pub fn pick_origin_plane(o: Vec3, d: Vec3, half: f32) -> Option<(OriginPlane, f32)> {
    let mut best: Option<(OriginPlane, f32)> = None;
    for (p, n) in [(OriginPlane::XY, Vec3::Z), (OriginPlane::XZ, Vec3::Y), (OriginPlane::YZ, Vec3::X)] {
        let den = d.dot(n);
        if den.abs() < 1e-9 {
            continue;
        }
        let t = -o.dot(n) / den;
        if t <= 0.0 {
            continue;
        }
        let h = o + d * t;
        // the hit lies on the plane, so its normal component is ~0; the other two must be inside the square
        let inside = h.x.abs() <= half && h.y.abs() <= half && h.z.abs() <= half;
        if inside && best.is_none_or(|b| t < b.1) {
            best = Some((p, t));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ray_triangle() {
        let t = ray_tri(Vec3::new(0.2, 0.2, 5.0), -Vec3::Z, Vec3::ZERO, Vec3::X, Vec3::Y).unwrap();
        assert!((t - 5.0).abs() < 1e-6);
        assert!(ray_tri(Vec3::new(2.0, 2.0, 5.0), -Vec3::Z, Vec3::ZERO, Vec3::X, Vec3::Y).is_none());
    }
}
