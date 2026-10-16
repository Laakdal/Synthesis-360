//! ViewCube (F5): a shaded cube drawn from the camera rotation with in-plane face labels, an
//! XYZ triad (red X, green Y, blue Z) along the cube edges from the origin corner, hover
//! highlight of the face / edge / corner under the pointer, and click-to-orient:
//! faces -> orthographic standard views, edges and corners -> the view along that direction
//! (corners give the isometric views). A Home button sits above-left, a small menu below-right.
//!
//! The camera projection is overwritten from the settings every frame, so the cube only rotates
//! the camera; its Orthographic / Perspective menu posts a request the nav bar executes.

use crate::ui::icons::{self, Icon};
use crate::ui::navbar::projection_request_id;
use crate::ui::theme;
use crate::viewport::camera::{Camera, Projection, StandardView};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Id, Popup, Pos2, Rect, Sense, Shape, Stroke, Ui};
use glam::{Quat, Vec2, Vec3};

/// One cube face: outward normal, in-plane right (u) and up (v) axes for its label.
pub struct Face {
    pub n: Vec3,
    pub u: Vec3,
    pub v: Vec3,
    pub label: &'static str,
    pub view: StandardView,
}

/// Faces with Fusion's label orientation (labels read upright from the matching standard view).
pub const FACES: [Face; 6] = [
    Face { n: Vec3::Z, u: Vec3::X, v: Vec3::Y, label: "TOP", view: StandardView::Top },
    Face { n: Vec3::NEG_Z, u: Vec3::X, v: Vec3::NEG_Y, label: "BOTTOM", view: StandardView::Bottom },
    Face { n: Vec3::NEG_Y, u: Vec3::X, v: Vec3::Z, label: "FRONT", view: StandardView::Front },
    Face { n: Vec3::Y, u: Vec3::NEG_X, v: Vec3::Z, label: "BACK", view: StandardView::Back },
    Face { n: Vec3::X, u: Vec3::Y, v: Vec3::Z, label: "RIGHT", view: StandardView::Right },
    Face { n: Vec3::NEG_X, u: Vec3::NEG_Y, v: Vec3::Z, label: "LEFT", view: StandardView::Left },
];

/// Region boundary in face coordinates (-1..1): beyond it the pointer is on an edge / corner.
const BAND: f32 = 0.55;

/// A clickable region of the cube: face index plus (i, j) in {-1, 0, 1} along (u, v).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub face: usize,
    pub i: i8,
    pub j: i8,
}

impl Target {
    pub fn is_face(&self) -> bool {
        self.i == 0 && self.j == 0
    }
    /// View direction (target -> eye) for this region.
    pub fn dir(&self) -> Vec3 {
        let f = &FACES[self.face];
        f.n + f.u * self.i as f32 + f.v * self.j as f32
    }
}

/// World direction -> screen offset (pixels, y down) for a camera rotation.
pub fn to_screen(rot: Quat, w: Vec3, scale: f32) -> Vec2 {
    let c = rot.inverse() * w;
    Vec2::new(c.x, -c.y) * scale
}

/// Towards-the-viewer depth of a world direction.
pub fn depth(rot: Quat, w: Vec3) -> f32 {
    (rot.inverse() * w).z
}

/// The cube region under screen offset `p` (relative to the cube centre).
pub fn pick(rot: Quat, p: Vec2, scale: f32) -> Option<Target> {
    for (k, f) in FACES.iter().enumerate() {
        if depth(rot, f.n) <= 0.02 {
            continue;
        }
        let c = to_screen(rot, f.n, scale);
        let su = to_screen(rot, f.u, scale);
        let sv = to_screen(rot, f.v, scale);
        let det = su.x * sv.y - su.y * sv.x;
        if det.abs() < 1e-3 * scale * scale {
            continue;
        }
        let d = p - c;
        let s = (d.x * sv.y - d.y * sv.x) / det;
        let t = (su.x * d.y - su.y * d.x) / det;
        if s.abs() <= 1.0 + 1e-3 && t.abs() <= 1.0 + 1e-3 {
            let r = |x: f32| if x > BAND { 1 } else if x < -BAND { -1 } else { 0 };
            return Some(Target { face: k, i: r(s), j: r(t) });
        }
    }
    None
}

/// Camera rotation after clicking `t`.
pub fn target_rotation(cam: &Camera, t: Target) -> Quat {
    let mut c = *cam;
    if t.is_face() {
        c.set_view(FACES[t.face].view);
    } else {
        // never parallel to Z (an edge/corner always has a horizontal component or is a top/bottom edge)
        c.set_axes(t.dir().normalize(), Vec3::Z);
    }
    c.rotation
}

#[derive(Clone, Copy)]
struct Anim {
    from: Quat,
    to: Quat,
    t0: f64,
}

const ANIM_S: f64 = 0.28;
/// Cube half-size as a fraction of the widget width.
const SCALE: f32 = 0.25;

pub fn show(ui: &mut Ui, rect: Rect, cam: &mut Camera) -> bool {
    let ctx = ui.ctx().clone();
    let anim_id = Id::unique("cad360.viewcube.anim");
    let now = ctx.input(|i| i.time);
    let mut changed = false;

    // running orientation animation
    if let Some(a) = ctx.data_mut(|d| d.get_temp::<Anim>(anim_id)) {
        let k = ((now - a.t0) / ANIM_S).clamp(0.0, 1.0) as f32;
        let e = k * k * (3.0 - 2.0 * k);
        cam.rotation = a.from.slerp(a.to, e).normalize();
        changed = true;
        if k >= 1.0 {
            cam.rotation = a.to;
            ctx.data_mut(|d| d.remove::<Anim>(anim_id));
        } else {
            ctx.request_repaint();
        }
    }

    let painter = ui.painter().clone();
    let resp = ui.interact(rect, ui.scope_id().with("viewcube"), Sense::click());
    let center = rect.center() + vec2(4.0, 6.0);
    let scale = rect.width() * SCALE;
    let rot = cam.rotation;
    let hover = resp.hover_pos().and_then(|p| pick(rot, Vec2::new(p.x - center.x, p.y - center.y), scale));
    let sp = |w: Vec3| -> Pos2 {
        let s = to_screen(rot, w, scale);
        pos2(center.x + s.x, center.y + s.y)
    };

    // triad (hidden parts first, so faces cover them)
    let origin = Vec3::splat(-1.0);
    let axes = [(Vec3::X, Color32::from_rgb(0xe5, 0x48, 0x4d), "X", [Vec3::NEG_Y, Vec3::NEG_Z]), (Vec3::Y, Color32::from_rgb(0x3c, 0xc2, 0x5a), "Y", [Vec3::NEG_X, Vec3::NEG_Z]), (Vec3::Z, Color32::from_rgb(0x4a, 0xa0, 0xff), "Z", [Vec3::NEG_Y, Vec3::NEG_X])];
    for (a, col, _, _) in &axes {
        painter.line_segment([sp(origin), sp(origin + *a * 2.75)], Stroke::new(1.6, *col));
    }

    // faces
    let mut vis: Vec<(usize, f32)> = FACES.iter().enumerate().map(|(k, f)| (k, depth(rot, f.n))).filter(|(_, d)| *d > 0.0).collect();
    vis.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (k, d) in &vis {
        let f = &FACES[*k];
        let corners = [f.n - f.u - f.v, f.n + f.u - f.v, f.n + f.u + f.v, f.n - f.u + f.v].map(sp);
        let shade = 0.74 + 0.26 * d;
        let base = Color32::from_rgb((214.0 * shade) as u8, (218.0 * shade) as u8, (224.0 * shade) as u8);
        painter.add(Shape::convex_polygon(corners.to_vec(), base, Stroke::new(1.0, Color32::from_rgb(0x7a, 0x80, 0x88))));
        if let Some(h) = hover.filter(|h| h.face == *k) {
            let span = |i: i8| match i {
                -1 => (-1.0, -BAND),
                1 => (BAND, 1.0),
                _ => (-BAND, BAND),
            };
            let (s0, s1) = span(h.i);
            let (t0, t1) = span(h.j);
            let q = [f.n + f.u * s0 + f.v * t0, f.n + f.u * s1 + f.v * t0, f.n + f.u * s1 + f.v * t1, f.n + f.u * s0 + f.v * t1].map(sp);
            painter.add(Shape::convex_polygon(q.to_vec(), theme::ACCENT.gamma_multiply(0.6), Stroke::NONE));
        }
        draw_label(&painter, f, *d, &sp);
    }

    // visible triad edges on top, then axis letters
    for (a, col, l, adj) in &axes {
        if adj.iter().any(|n| depth(rot, *n) > 0.0) {
            painter.line_segment([sp(origin), sp(origin + *a * 2.0)], Stroke::new(1.8, *col));
        }
        // a letter behind the cube silhouette is hidden (like the hidden axis segment)
        let lp = origin + *a * 3.05;
        let off = to_screen(rot, lp, scale);
        if depth(rot, lp) > -0.5 || pick(rot, off, scale).is_none() {
            painter.text(sp(lp), Align2::CENTER_CENTER, *l, FontId::proportional(11.0), *col);
        }
    }

    // home button (above-left) and menu (below-right)
    let home = Rect::from_min_size(rect.left_top() + vec2(2.0, 0.0), vec2(20.0, 20.0));
    let hresp = ui.interact(home, ui.scope_id().with("viewcube-home"), Sense::click());
    if hresp.hovered() || resp.hovered() {
        icons::paint_button_bg(&painter, home, &hresp, true, false);
        icons::paint(&painter, home.shrink(3.0), Icon::Home, true);
    }
    let mut target: Option<Quat> = None;
    if hresp.on_hover_text("Home view").clicked() {
        let mut c = *cam;
        c.set_view(StandardView::Home);
        target = Some(c.rotation);
    }
    let menu_r = Rect::from_min_size(rect.right_bottom() - vec2(18.0, 18.0), vec2(16.0, 16.0));
    let mresp = ui.interact(menu_r, ui.scope_id().with("viewcube-menu"), Sense::click());
    if mresp.hovered() || resp.hovered() || Popup::is_id_open(&ctx, Popup::default_response_id(&mresp)) {
        icons::paint_button_bg(&painter, menu_r, &mresp, true, false);
        icons::caret(&painter, menu_r.center(), 7.0, theme::TEXT);
    }
    let persp = cam.projection == Projection::Perspective;
    Popup::menu(&mresp).show(|ui| {
        if icons::menu_row(ui, Some(Icon::Home), "Go Home", None, true, None).clicked() {
            let mut c = *cam;
            c.set_view(StandardView::Home);
            target = Some(c.rotation);
            ui.close();
        }
        icons::menu_separator(ui);
        if icons::menu_row(ui, None, "Orthographic", None, true, Some(!persp)).clicked() {
            ctx.data_mut(|d| d.insert_temp(projection_request_id(), false));
            ui.close();
        }
        if icons::menu_row(ui, None, "Perspective", None, true, Some(persp)).clicked() {
            ctx.data_mut(|d| d.insert_temp(projection_request_id(), true));
            ui.close();
        }
        icons::menu_separator(ui);
        icons::menu_row(ui, None, "Set Current View as Home", None, false, None).on_hover_text(crate::state::command::NOT_SUPPORTED);
    });

    if let Some(h) = hover {
        let tip = if h.is_face() { FACES[h.face].label.to_string() } else { "Rotate to this view".to_string() };
        if resp.clicked() {
            target = Some(target_rotation(cam, h));
        } else {
            resp.on_hover_text(tip);
        }
    }
    if let Some(to) = target {
        ctx.data_mut(|d| d.insert_temp(anim_id, Anim { from: cam.rotation, to, t0: now }));
        ctx.request_repaint();
    }
    let _ = CornerRadius::ZERO;
    changed
}

/// Draws a face label with a small stroke font so it lies in the face plane (like Fusion).
fn draw_label(painter: &egui::Painter, f: &Face, facing: f32, sp: &dyn Fn(Vec3) -> Pos2) {
    if facing < 0.15 {
        return;
    }
    let letters: Vec<(f32, Vec<Vec<(f32, f32)>>)> = f.label.chars().map(glyph).collect();
    let gap = 0.24;
    let raw_w: f32 = letters.iter().map(|(w, _)| *w).sum::<f32>() + gap * (letters.len() as f32 - 1.0);
    let h = (1.55 / raw_w).min(0.42);
    let mut x = -raw_w * h * 0.5;
    let y0 = -h * 0.5;
    let col = Color32::from_rgb(0x52, 0x57, 0x5f).gamma_multiply((facing * 1.6).min(1.0));
    let stroke = Stroke::new(1.25, col);
    for (w, strokes) in letters {
        for s in strokes {
            let pts: Vec<Pos2> = s.iter().map(|(lx, ly)| sp(f.n + f.u * (x + lx * h) + f.v * (y0 + ly * h))).collect();
            painter.add(Shape::line(pts, stroke));
        }
        x += (w + gap) * h;
    }
}

fn ellipse_arc(a0: f32, a1: f32) -> Vec<(f32, f32)> {
    (0..=16).map(|i| {
        let a = (a0 + (a1 - a0) * i as f32 / 16.0).to_radians();
        (0.35 + 0.35 * a.cos(), 0.5 + 0.5 * a.sin())
    }).collect()
}

/// Stroke-font glyph (width, polylines) in a unit-height box, y up.
fn glyph(c: char) -> (f32, Vec<Vec<(f32, f32)>>) {
    let p = vec![(0.0, 0.0), (0.0, 1.0), (0.5, 1.0), (0.7, 0.86), (0.7, 0.64), (0.5, 0.5), (0.0, 0.5)];
    match c {
        'T' => (0.7, vec![vec![(0.0, 1.0), (0.7, 1.0)], vec![(0.35, 1.0), (0.35, 0.0)]]),
        'O' => (0.7, vec![ellipse_arc(0.0, 360.0)]),
        'P' => (0.7, vec![p]),
        'R' => (0.7, vec![p, vec![(0.38, 0.5), (0.7, 0.0)]]),
        'F' => (0.62, vec![vec![(0.62, 1.0), (0.0, 1.0), (0.0, 0.0)], vec![(0.0, 0.52), (0.5, 0.52)]]),
        'N' => (0.7, vec![vec![(0.0, 0.0), (0.0, 1.0), (0.7, 0.0), (0.7, 1.0)]]),
        'B' => (0.7, vec![vec![(0.0, 0.0), (0.0, 1.0), (0.48, 1.0), (0.64, 0.88), (0.64, 0.64), (0.48, 0.53), (0.0, 0.53)], vec![(0.48, 0.53), (0.7, 0.4), (0.7, 0.13), (0.52, 0.0), (0.0, 0.0)]]),
        'A' => (0.72, vec![vec![(0.0, 0.0), (0.36, 1.0), (0.72, 0.0)], vec![(0.13, 0.36), (0.59, 0.36)]]),
        'C' => (0.7, vec![ellipse_arc(45.0, 315.0)]),
        'K' => (0.68, vec![vec![(0.0, 0.0), (0.0, 1.0)], vec![(0.68, 1.0), (0.0, 0.38)], vec![(0.24, 0.6), (0.68, 0.0)]]),
        'L' => (0.6, vec![vec![(0.0, 1.0), (0.0, 0.0), (0.6, 0.0)]]),
        'E' => (0.62, vec![vec![(0.62, 1.0), (0.0, 1.0), (0.0, 0.0), (0.62, 0.0)], vec![(0.0, 0.52), (0.5, 0.52)]]),
        'I' => (0.12, vec![vec![(0.06, 0.0), (0.06, 1.0)]]),
        'G' => {
            let mut a = ellipse_arc(45.0, 340.0);
            a.push((0.7, 0.45));
            a.push((0.4, 0.45));
            (0.7, vec![a])
        }
        'H' => (0.7, vec![vec![(0.0, 0.0), (0.0, 1.0)], vec![(0.7, 0.0), (0.7, 1.0)], vec![(0.0, 0.52), (0.7, 0.52)]]),
        'M' => (0.82, vec![vec![(0.0, 0.0), (0.0, 1.0), (0.41, 0.3), (0.82, 1.0), (0.82, 0.0)]]),
        _ => (0.5, vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam(v: StandardView) -> Camera {
        let mut c = Camera::default();
        c.set_view(v);
        c
    }

    #[test]
    fn centre_of_top_view_picks_top_face() {
        let c = cam(StandardView::Top);
        let t = pick(c.rotation, Vec2::ZERO, 30.0).expect("hit");
        assert_eq!(FACES[t.face].label, "TOP");
        assert!(t.is_face());
    }

    #[test]
    fn front_view_regions() {
        let c = cam(StandardView::Front);
        // screen right of centre on the front face is +X (towards RIGHT); near the edge -> edge region
        let t = pick(c.rotation, Vec2::new(27.0, 0.0), 30.0).unwrap();
        assert_eq!(FACES[t.face].label, "FRONT");
        assert_eq!((t.i, t.j), (1, 0));
        assert_eq!(t.dir(), Vec3::new(1.0, -1.0, 0.0));
        // top-left corner of the front face (screen y up = -y)
        let t = pick(c.rotation, Vec2::new(-27.0, -27.0), 30.0).unwrap();
        assert_eq!((t.i, t.j), (-1, 1));
        assert_eq!(t.dir(), Vec3::new(-1.0, -1.0, 1.0));
        assert!(pick(c.rotation, Vec2::new(80.0, 0.0), 30.0).is_none(), "outside the cube");
    }

    #[test]
    fn home_view_shows_top_front_right() {
        let c = cam(StandardView::Home);
        let labels: Vec<&str> = FACES.iter().filter(|f| depth(c.rotation, f.n) > 0.0).map(|f| f.label).collect();
        assert_eq!(labels, ["TOP", "FRONT", "RIGHT"]);
        // the centre of the iso view is the front-right-top corner
        let t = pick(c.rotation, Vec2::ZERO, 30.0).unwrap();
        assert_eq!(t.dir().normalize(), Vec3::new(1.0, -1.0, 1.0).normalize());
    }

    #[test]
    fn clicking_targets_orients_camera() {
        let c = cam(StandardView::Home);
        let q = target_rotation(&c, Target { face: 4, i: 0, j: 0 }); // RIGHT face
        let mut want = c;
        want.set_view(StandardView::Right);
        assert!(q.dot(want.rotation).abs() > 0.9999);
        // corner -> iso direction with Z up
        let q = target_rotation(&c, Target { face: 0, i: -1, j: 1 }); // top, -X, +Y corner
        let back = q * Vec3::Z;
        assert!((back - Vec3::new(-1.0, 1.0, 1.0).normalize()).length() < 1e-4);
        assert!((q * Vec3::Y).z > 0.0, "up stays towards +Z");
        assert!(q.is_finite());
    }

    #[test]
    fn label_orientation_matches_standard_views() {
        // from each face's standard view, the label's u axis points screen-right and v screen-up
        for f in &FACES {
            let c = cam(f.view);
            let u = to_screen(c.rotation, f.u, 1.0);
            let v = to_screen(c.rotation, f.v, 1.0);
            assert!(u.x > 0.99, "{}: u -> {u:?}", f.label);
            assert!(v.y < -0.99, "{}: v -> {v:?}", f.label);
        }
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use crate::ui::icons::testkit::Kit;

    #[test]
    fn clicking_front_face_animates_to_front_view() {
        let mut cam = Camera::default();
        let rect = Rect::from_min_size(pos2(1400.0, 100.0), vec2(116.0, 116.0));
        let center = rect.center() + vec2(4.0, 6.0);
        let scale = rect.width() * SCALE;
        let off = to_screen(cam.rotation, Vec3::NEG_Y, scale);
        let target = pos2(center.x + off.x, center.y + off.y);
        let mut kit = Kit::new();
        let mut changed = false;
        {
            let mut f = |ui: &mut Ui| {
                egui::CentralPanel::default().show(ui, |ui| changed |= show(ui, rect, &mut cam));
            };
            kit.idle(2, &mut f);
            kit.click(target, &mut f);
            kit.idle(30, &mut f);
        }
        assert!(changed);
        let mut want = Camera::default();
        want.set_view(StandardView::Front);
        assert!(cam.rotation.dot(want.rotation).abs() > 0.9999, "{:?}", cam.rotation);
    }
}
