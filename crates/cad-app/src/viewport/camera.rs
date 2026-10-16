//! Viewport camera: Z-up world, orthographic by default (Fusion probe: camera type 0 =
//! orthographic), standard views and the Home view taken from the probe's camera data.

use glam::{Mat4, Quat, Vec2, Vec3, Vec4};

/// Right-handed view matrix (glam 0.34 removed the Mat4 camera constructors).
pub fn look_at_rh(eye: Vec3, center: Vec3, up: Vec3) -> Mat4 {
    let f = (center - eye).normalize();
    let s = f.cross(up).normalize();
    let u = s.cross(f);
    Mat4::from_cols(Vec4::new(s.x, u.x, -f.x, 0.0), Vec4::new(s.y, u.y, -f.y, 0.0), Vec4::new(s.z, u.z, -f.z, 0.0), Vec4::new(-s.dot(eye), -u.dot(eye), f.dot(eye), 1.0))
}

/// Right-handed perspective projection with depth in [0, 1] (wgpu convention).
pub fn perspective_rh(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let h = 1.0 / (0.5 * fov_y).tan();
    let w = h / aspect;
    let r = far / (near - far);
    Mat4::from_cols(Vec4::new(w, 0.0, 0.0, 0.0), Vec4::new(0.0, h, 0.0, 0.0), Vec4::new(0.0, 0.0, r, -1.0), Vec4::new(0.0, 0.0, r * near, 0.0))
}

/// Right-handed orthographic projection with depth in [0, 1].
pub fn orthographic_rh(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Mat4 {
    let rw = 1.0 / (right - left);
    let rh = 1.0 / (top - bottom);
    let r = 1.0 / (near - far);
    Mat4::from_cols(Vec4::new(2.0 * rw, 0.0, 0.0, 0.0), Vec4::new(0.0, 2.0 * rh, 0.0, 0.0), Vec4::new(0.0, 0.0, r, 0.0), Vec4::new(-(left + right) * rw, -(top + bottom) * rh, r * near, 1.0))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    Orthographic,
    Perspective,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandardView {
    Top,
    Bottom,
    Front,
    Back,
    Left,
    Right,
    Home,
}

impl StandardView {
    /// (direction from target to eye, up vector). Probe: top eye +Z (up +Y), front eye −Y,
    /// right eye +X (up +Z); iso eye − target ∝ (1, −1, 1).
    pub fn axes(self) -> (Vec3, Vec3) {
        match self {
            StandardView::Top => (Vec3::Z, Vec3::Y),
            StandardView::Bottom => (-Vec3::Z, -Vec3::Y),
            StandardView::Front => (-Vec3::Y, Vec3::Z),
            StandardView::Back => (Vec3::Y, Vec3::Z),
            StandardView::Left => (-Vec3::X, Vec3::Z),
            StandardView::Right => (Vec3::X, Vec3::Z),
            StandardView::Home => (Vec3::new(1.0, -1.0, 1.0).normalize(), Vec3::Z),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            StandardView::Top => "Top",
            StandardView::Bottom => "Bottom",
            StandardView::Front => "Front",
            StandardView::Back => "Back",
            StandardView::Left => "Left",
            StandardView::Right => "Right",
            StandardView::Home => "Home",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub target: Vec3,
    /// Rotation taking camera space (x right, y up, z back) to world space.
    pub rotation: Quat,
    pub distance: f32,
    pub projection: Projection,
    pub fov_y: f32,
}

impl Default for Camera {
    fn default() -> Self {
        let mut c = Camera { target: Vec3::ZERO, rotation: Quat::IDENTITY, distance: 300.0, projection: Projection::Orthographic, fov_y: 30f32.to_radians() };
        c.set_view(StandardView::Home);
        c
    }
}

impl Camera {
    pub fn back(&self) -> Vec3 {
        self.rotation * Vec3::Z
    }
    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }
    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::X
    }
    pub fn eye(&self) -> Vec3 {
        self.target + self.back() * self.distance
    }

    pub fn set_view(&mut self, v: StandardView) {
        let (back, up) = v.axes();
        self.set_axes(back, up);
    }

    pub fn set_axes(&mut self, back: Vec3, up: Vec3) {
        let z = back.normalize();
        let x = up.cross(z).normalize();
        let y = z.cross(x);
        self.rotation = Quat::from_mat3(&glam::Mat3::from_cols(x, y, z)).normalize();
    }

    /// Half the visible height at the target (world units).
    pub fn half_height(&self) -> f32 {
        self.distance * (self.fov_y * 0.5).tan()
    }

    pub fn view(&self) -> Mat4 {
        look_at_rh(self.eye(), self.target, self.up())
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        let near = (self.distance * 0.01).max(0.01);
        let far = self.distance * 100.0 + 10_000.0;
        match self.projection {
            Projection::Perspective => perspective_rh(self.fov_y, aspect, near, far),
            Projection::Orthographic => {
                let h = self.half_height();
                let w = h * aspect;
                // keep geometry behind the target visible in orthographic mode
                orthographic_rh(-w, w, -h, h, -far, far)
            }
        }
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.proj(aspect) * self.view()
    }

    /// Free orbit about the target (Shift + middle mouse in Fusion), from a drag in pixels.
    pub fn orbit(&mut self, delta: Vec2) {
        let yaw = Quat::from_axis_angle(Vec3::Z, -delta.x * 0.008);
        let pitch = Quat::from_axis_angle(self.right(), -delta.y * 0.008);
        self.rotation = (yaw * pitch * self.rotation).normalize();
    }

    /// Pan (middle mouse in Fusion) by a drag in pixels, for a viewport `height` pixels tall.
    pub fn pan(&mut self, delta: Vec2, height: f32) {
        let k = 2.0 * self.half_height() / height.max(1.0);
        self.target += (-self.right() * delta.x + self.up() * delta.y) * k;
    }

    /// Zoom (mouse wheel) towards the cursor position given in NDC.
    pub fn zoom(&mut self, factor: f32, cursor_ndc: Vec2, aspect: f32) {
        let factor = factor.clamp(0.2, 5.0);
        let h = self.half_height();
        let before = self.target + self.right() * cursor_ndc.x * h * aspect + self.up() * cursor_ndc.y * h;
        self.distance = (self.distance * factor).clamp(0.05, 1.0e6);
        let h2 = self.half_height();
        let after = self.target + self.right() * cursor_ndc.x * h2 * aspect + self.up() * cursor_ndc.y * h2;
        self.target += before - after;
    }

    /// Frames a bounding sphere.
    pub fn fit(&mut self, center: Vec3, radius: f32, aspect: f32) {
        self.target = center;
        let r = radius.max(1.0);
        let t = (self.fov_y * 0.5).tan();
        let fit_h = r / t;
        let fit_w = r / (t * aspect.max(0.1));
        self.distance = fit_h.max(fit_w) * 1.15;
    }

    /// World-space ray through a point in NDC (-1..1, y up).
    pub fn ray(&self, ndc: Vec2, aspect: f32) -> (Vec3, Vec3) {
        let h = self.half_height();
        match self.projection {
            Projection::Orthographic => {
                let o = self.target + self.right() * ndc.x * h * aspect + self.up() * ndc.y * h + self.back() * self.distance * 50.0;
                (o, -self.back())
            }
            Projection::Perspective => {
                let p = self.target + self.right() * ndc.x * h * aspect + self.up() * ndc.y * h;
                let e = self.eye();
                (e, (p - e).normalize())
            }
        }
    }

    /// Projects a world point to NDC (x, y) and returns `None` behind the camera.
    pub fn project(&self, p: Vec3, aspect: f32) -> Option<Vec2> {
        let c = self.view_proj(aspect) * p.extend(1.0);
        if c.w <= 1e-6 {
            return None;
        }
        Some(Vec2::new(c.x / c.w, c.y / c.w))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_views_match_fusion_probe() {
        let mut c = Camera::default();
        c.set_view(StandardView::Front);
        assert!((c.back() - (-Vec3::Y)).length() < 1e-6 && (c.up() - Vec3::Z).length() < 1e-6);
        c.set_view(StandardView::Top);
        assert!((c.back() - Vec3::Z).length() < 1e-6 && (c.up() - Vec3::Y).length() < 1e-6);
        c.set_view(StandardView::Right);
        assert!((c.back() - Vec3::X).length() < 1e-6);
        assert_eq!(Camera::default().projection, Projection::Orthographic);
    }

    #[test]
    fn ray_and_project_are_consistent() {
        let mut c = Camera::default();
        c.set_view(StandardView::Top);
        c.target = Vec3::new(10.0, 5.0, 0.0);
        let (o, d) = c.ray(Vec2::new(0.25, -0.5), 1.5);
        let t = -o.z / d.z;
        let hit = o + d * t;
        let back = c.project(hit, 1.5).unwrap();
        assert!((back - Vec2::new(0.25, -0.5)).length() < 1e-4, "{back:?}");
    }

    #[test]
    fn zoom_keeps_point_under_cursor() {
        let mut c = Camera::default();
        c.set_view(StandardView::Top);
        let ndc = Vec2::new(0.4, 0.2);
        let (o, d) = c.ray(ndc, 1.0);
        let p = o + d * (-o.z / d.z);
        c.zoom(0.5, ndc, 1.0);
        let q = c.project(p, 1.0).unwrap();
        assert!((q - ndc).length() < 1e-4);
    }
}
