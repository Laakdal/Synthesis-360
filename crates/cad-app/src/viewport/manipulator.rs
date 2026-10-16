//! On-canvas drag manipulators (Fusion: the blue arrow and the distance box of Extrude, Offset
//! Plane, Fillet, ...). The open feature dialog exposes an [`AxisHandle`] through
//! [`AppState::manipulator_handle`]; dragging the arrow along its axis changes the dialog's
//! primary value.
//!
//! * Value = signed axis parameter of the closest point between the cursor ray and the axis
//!   line, relative to where the arrow was grabbed (no jump on press), times the handle scale.
//! * Snapping (when grid snap is on): 1 mm, coarser 1-2-5 steps when 1 mm is under 4 px on
//!   screen; Shift = a tenth of that step. A value never snaps to 0 (zero extrude is invalid).
//! * Cheap features (Extrude, Offset Plane) recompute the preview while dragging, at most every
//!   [`THROTTLE_S`]; the others update the on-canvas value box live and recompute on release.
//!   Release always commits through [`AppState::dialog_set_primary`], like a typed value.
//!
//! Input is a plain [`PointerInput`] so the app, scenarios and headless tests drive the same
//! state machine; a drag only ends on an explicit release.

use super::camera::Camera;
use crate::state::AppState;
use glam::{Vec2, Vec3};

/// Minimum interval between preview recomputes while dragging (seconds).
pub const THROTTLE_S: f64 = 0.08;
/// Minimum on-screen arrow length (points), so a tiny value stays grabbable.
const MIN_ARROW_PX: f32 = 46.0;
const HEAD_LEN: f32 = 16.0;
const HEAD_HALF_WIDTH: f32 = 8.0;

/// A linear drag handle: an arrow from `origin` along `dir` whose tip sits at `value / scale`.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisHandle {
    pub origin: [f64; 3],
    /// Unit axis direction (positive values point this way).
    pub dir: [f64; 3],
    /// Current dialog value (mm).
    pub value: f64,
    /// Value per unit of axis length (2 for a symmetric extrude, whose arrow shows one half).
    pub scale: f64,
    /// The value must stay > 0 (radius, thickness, symmetric distance).
    pub positive: bool,
    /// Recompute the preview while dragging (cheap features); otherwise only on release.
    pub live: bool,
}

impl AxisHandle {
    fn o(&self) -> Vec3 {
        Vec3::new(self.origin[0] as f32, self.origin[1] as f32, self.origin[2] as f32)
    }
    fn d(&self) -> Vec3 {
        Vec3::new(self.dir[0] as f32, self.dir[1] as f32, self.dir[2] as f32).normalize_or_zero()
    }
    /// World point on the axis that corresponds to `value`.
    pub fn point_at(&self, value: f64) -> Vec3 {
        self.o() + self.d() * (value / self.scale) as f32
    }
}

/// Axis parameter of the point of the line `axis_o + s * axis_d` closest to the ray
/// `ray_o + t * ray_d` (both directions unit). `None` when the two are (nearly) parallel.
pub fn ray_axis_param(axis_o: Vec3, axis_d: Vec3, ray_o: Vec3, ray_d: Vec3) -> Option<f32> {
    let w = axis_o - ray_o;
    let b = axis_d.dot(ray_d);
    let den = 1.0 - b * b;
    if den < 1e-5 {
        return None;
    }
    Some((b * ray_d.dot(w) - axis_d.dot(w)) / den)
}

/// Smallest 1-2-5 × 10^n step that is at least `min` (mm).
pub fn nice_step(min: f64) -> f64 {
    let min = min.max(1e-6);
    let p = 10f64.powf(min.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if m * p >= min - 1e-12 {
            return m * p;
        }
    }
    10.0 * p
}

/// Snap step for the current zoom: 1 mm, or a coarser 1-2-5 step when 1 mm is under 4 px on
/// screen (zoomed out); `fine` (Shift) divides it by 10.
pub fn snap_step(mm_per_px: f64, fine: bool) -> f64 {
    let s = nice_step(4.0 * mm_per_px).max(1.0);
    if fine {
        s / 10.0
    } else {
        s
    }
}

/// Rounds `v` to a multiple of `step` (and strips float noise).
pub fn snap(v: f64, step: f64) -> f64 {
    if step <= 0.0 {
        return v;
    }
    let r = (v / step).round() * step;
    (r * 1e6).round() / 1e6
}

/// Expression text for a value in mm ("25 mm", "12.5 mm"; at most 4 decimals).
pub fn fmt_mm(v: f64) -> String {
    let r = (v * 1e4).round() / 1e4;
    let r = if r == 0.0 { 0.0 } else { r };
    format!("{r} mm")
}

/// The canvas camera and viewport rectangle (points).
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub cam: Camera,
    pub rect: egui::Rect,
}

impl View {
    fn aspect(&self) -> f32 {
        (self.rect.width() / self.rect.height().max(1.0)).max(0.1)
    }
    pub fn to_screen(&self, p: Vec3) -> Option<egui::Pos2> {
        let n = self.cam.project(p, self.aspect())?;
        let r = self.rect;
        Some(egui::pos2(r.left() + (n.x + 1.0) * 0.5 * r.width(), r.top() + (1.0 - n.y) * 0.5 * r.height()))
    }
    pub fn ray(&self, p: egui::Pos2) -> (Vec3, Vec3) {
        let r = self.rect;
        let ndc = Vec2::new((p.x - r.left()) / r.width() * 2.0 - 1.0, 1.0 - (p.y - r.top()) / r.height() * 2.0);
        self.cam.ray(ndc, self.aspect())
    }
    pub fn mm_per_px(&self) -> f64 {
        2.0 * self.cam.half_height() as f64 / self.rect.height().max(1.0) as f64
    }
}

/// One frame of pointer input for the manipulator.
#[derive(Clone, Copy, Debug, Default)]
pub struct PointerInput {
    pub pos: Option<egui::Pos2>,
    /// Primary button went down this frame over the canvas.
    pub pressed: bool,
    /// Primary button went up this frame (anywhere).
    pub released: bool,
    /// The pointer moved this frame.
    pub moved: bool,
    pub shift: bool,
    /// Time in seconds (egui input time) for throttling.
    pub time: f64,
}

#[derive(Clone, Debug)]
struct Drag {
    handle: AxisHandle,
    /// Axis parameter where the arrow was grabbed.
    grab: f64,
    start: f64,
    value: f64,
    /// Last value written into the dialog.
    applied: f64,
    last_apply: f64,
}

/// Screen-space arrow: base (axis origin), tip and unit direction.
struct Arrow {
    base: egui::Pos2,
    tip: egui::Pos2,
    dir: egui::Vec2,
}

fn arrow(view: &View, h: &AxisHandle, value: f64) -> Option<Arrow> {
    let base = view.to_screen(h.o())?;
    let unit = view.to_screen(h.o() + h.d())?;
    let px_per_mm = (unit - base).length();
    let sign = if value < 0.0 { -1.0 } else { 1.0 };
    if px_per_mm < 1e-3 {
        // the axis points at the viewer: a short arrow straight up keeps it grabbable
        return Some(Arrow { base, tip: base - egui::vec2(0.0, MIN_ARROW_PX), dir: egui::vec2(0.0, -1.0) });
    }
    let len = ((value / h.scale).abs() as f32).max(MIN_ARROW_PX / px_per_mm);
    let tip = view.to_screen(h.o() + h.d() * sign * len)?;
    let d = tip - base;
    let dir = if d.length() > 1e-3 { d.normalized() } else { egui::vec2(0.0, -1.0) };
    Some(Arrow { base, tip, dir })
}

fn seg_dist(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() < 1e-9 { 0.0 } else { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) };
    (a + ab * t - p).length()
}

fn hit(view: &View, h: &AxisHandle, pos: egui::Pos2) -> bool {
    arrow(view, h, h.value).is_some_and(|a| seg_dist(pos, a.base, a.tip) <= 8.0 || (pos - a.tip).length() <= HEAD_LEN)
}

/// Drag state of the on-canvas manipulator.
#[derive(Debug, Default)]
pub struct Manipulator {
    hovered: bool,
    drag: Option<Drag>,
}

impl Manipulator {
    pub fn hovered(&self) -> bool {
        self.hovered
    }
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
    /// Live value of the drag in progress.
    pub fn drag_value(&self) -> Option<f64> {
        self.drag.as_ref().map(|d| d.value)
    }
    /// Handle frozen at the start of the drag in progress.
    pub fn drag_handle(&self) -> Option<&AxisHandle> {
        self.drag.as_ref().map(|d| &d.handle)
    }

    fn value_at(d: &Drag, view: &View, pos: egui::Pos2, shift: bool, snap_on: bool) -> Option<f64> {
        let (ro, rd) = view.ray(pos);
        let s = ray_axis_param(d.handle.o(), d.handle.d(), ro, rd)? as f64;
        let raw = d.start + (s - d.grab) * d.handle.scale;
        let step = if snap_on { snap_step(view.mm_per_px(), shift) } else { 0.01 };
        let mut v = snap(raw, step);
        if d.handle.positive {
            v = v.max(step);
        } else if v.abs() < step * 0.5 {
            v = if raw < 0.0 { -step } else { step };
        }
        Some(v)
    }

    /// Feeds one frame of pointer input. Returns true when the manipulator owns the pointer
    /// (hovered or dragging), so the canvas must skip picking and selection this frame.
    pub fn input(&mut self, state: &mut AppState, view: &View, p: &PointerInput) -> bool {
        if state.dialog.is_none() {
            self.drag = None;
            self.hovered = false;
            return false;
        }
        if let Some(mut d) = self.drag.take() {
            if p.moved || p.pressed || p.released {
                if let Some(v) = p.pos.and_then(|pos| Self::value_at(&d, view, pos, p.shift, state.settings.snap)) {
                    d.value = v;
                }
            }
            if p.released {
                if d.value != d.applied {
                    state.dialog_set_primary(&fmt_mm(d.value));
                }
                self.hovered = p.pos.is_some_and(|pos| state.manipulator_handle().is_some_and(|h| hit(view, &h, pos)));
                return true;
            }
            if d.handle.live && d.value != d.applied && p.time - d.last_apply >= THROTTLE_S {
                state.dialog_set_primary(&fmt_mm(d.value));
                d.applied = d.value;
                d.last_apply = p.time;
            }
            self.drag = Some(d);
            return true;
        }
        let Some(h) = state.manipulator_handle() else {
            self.hovered = false;
            return false;
        };
        self.hovered = p.pos.is_some_and(|pos| hit(view, &h, pos));
        if self.hovered && p.pressed {
            let pos = p.pos.expect("hovered implies a position");
            let (ro, rd) = view.ray(pos);
            let grab = ray_axis_param(h.o(), h.d(), ro, rd).map(|s| s as f64).unwrap_or(h.value / h.scale);
            self.drag = Some(Drag { grab, start: h.value, value: h.value, applied: h.value, last_apply: p.time, handle: h });
        }
        self.hovered
    }

    /// Draws the arrow and the value box (Fusion-style, on top of the model).
    pub fn paint(&self, painter: &egui::Painter, state: &AppState, view: &View) {
        let (h, value) = match &self.drag {
            Some(d) => (d.handle.clone(), d.value),
            None => match state.manipulator_handle() {
                Some(h) => {
                    let v = h.value;
                    (h, v)
                }
                None => return,
            },
        };
        let Some(a) = arrow(view, &h, value) else { return };
        let hot = self.hovered || self.drag.is_some();
        let fill = if hot { egui::Color32::from_rgb(150, 210, 255) } else { egui::Color32::from_rgb(70, 160, 245) };
        let dark = egui::Color32::from_rgb(12, 34, 64);
        let neck = a.tip - a.dir * HEAD_LEN;
        painter.line_segment([a.base, neck], egui::Stroke::new(5.0, dark));
        painter.line_segment([a.base, neck], egui::Stroke::new(3.0, fill));
        let perp = egui::vec2(-a.dir.y, a.dir.x);
        let head = vec![a.tip, neck + perp * HEAD_HALF_WIDTH, neck - perp * HEAD_HALF_WIDTH];
        painter.add(egui::Shape::convex_polygon(head, fill, egui::Stroke::new(1.5, dark)));
        painter.circle(a.base, 3.5, fill, egui::Stroke::new(1.0, dark));

        // value box beside the arrow head (Fusion: "0.00 mm" next to the manipulator)
        let text = format!("{value:.2} mm");
        let font = egui::FontId::proportional(13.0);
        let galley = painter.layout_no_wrap(text, font, egui::Color32::from_rgb(235, 238, 242));
        let side = if perp.x >= 0.0 { perp } else { -perp };
        let size = galley.size() + egui::vec2(18.0, 8.0);
        let center = a.tip + side * (size.x * 0.5 + 18.0) + a.dir * 4.0;
        let rect = egui::Rect::from_center_size(center, size);
        painter.rect(rect, 3.0, egui::Color32::from_rgba_unmultiplied(28, 30, 34, 235), egui::Stroke::new(1.0, if hot { fill } else { egui::Color32::from_gray(90) }), egui::StrokeKind::Inside);
        painter.galley(rect.center() - galley.size() * 0.5, galley, egui::Color32::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::command::CommandId;
    use crate::state::{Dialog, Pick};
    use crate::viewport::camera::StandardView;
    use cad_core::sketch::{ConstraintKind as K, DistanceOrientation as O, EntityKind, ORIGIN};
    use cad_core::{ops, OriginPlane, PlaneRef};

    #[test]
    fn ray_to_axis_closest_point() {
        // axis = Z through the origin; a ray along -X at height 5 passes over it
        let s = ray_axis_param(Vec3::ZERO, Vec3::Z, Vec3::new(10.0, 0.0, 5.0), -Vec3::X).unwrap();
        assert!((s - 5.0).abs() < 1e-5, "{s}");
        // skew ray: along -Y at x = 3, z = -7 -> closest axis point z = -7
        let s = ray_axis_param(Vec3::new(0.0, 0.0, 1.0), Vec3::Z, Vec3::new(3.0, 10.0, -7.0), -Vec3::Y).unwrap();
        assert!((s + 8.0).abs() < 1e-5, "{s}");
        // oblique ray towards the point (0, 0, 12) of the axis
        let target = Vec3::new(0.0, 0.0, 12.0);
        let o = Vec3::new(40.0, -30.0, 50.0);
        let s = ray_axis_param(Vec3::ZERO, Vec3::Z, o, (target - o).normalize()).unwrap();
        assert!((s - 12.0).abs() < 1e-3, "{s}");
        // parallel: undefined
        assert!(ray_axis_param(Vec3::ZERO, Vec3::Z, Vec3::new(1.0, 0.0, 0.0), Vec3::Z).is_none());
    }

    #[test]
    fn snapping_steps_and_formatting() {
        assert_eq!(nice_step(0.92), 1.0);
        assert_eq!(nice_step(1.2), 2.0);
        assert_eq!(nice_step(3.0), 5.0);
        assert_eq!(nice_step(7.0), 10.0);
        assert!((nice_step(0.03) - 0.05).abs() < 1e-12);
        assert_eq!(snap_step(0.23, false), 1.0);
        assert!((snap_step(0.23, true) - 0.1).abs() < 1e-12);
        assert_eq!(snap_step(0.02, false), 1.0, "never coarser than needed, never finer than 1 mm");
        assert_eq!(snap_step(3.0, false), 20.0);
        assert_eq!(snap(12.34, 1.0), 12.0);
        assert_eq!(snap(12.5, 5.0), 15.0);
        assert!((snap(12.34, 0.1) - 12.3).abs() < 1e-9);
        assert_eq!(snap(-2.6, 1.0), -3.0);
        assert_eq!(fmt_mm(25.0), "25 mm");
        assert_eq!(fmt_mm(12.5), "12.5 mm");
        assert_eq!(fmt_mm(0.1 + 0.2), "0.3 mm");
        assert_eq!(fmt_mm(-0.00001), "0 mm");
    }

    static OCC_INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 50 × 30 rectangle on XY with its corner on the origin.
    fn rectangle_state() -> (AppState, String) {
        let mut st = {
            let _guard = OCC_INIT.lock().unwrap_or_else(|e| e.into_inner());
            AppState::new()
        };
        let doc = &mut st.doc;
        let sk = ops::new_sketch(doc, PlaneRef::Origin { plane: OriginPlane::XY });
        let s = ops::sketch_mut(doc, &sk).unwrap();
        let lines = s.add_rectangle([1.0, 1.0], [40.0, 20.0]);
        let corner = match &s.entity(&lines[0]).unwrap().kind {
            EntityKind::Line { p1, .. } => p1.clone(),
            _ => unreachable!(),
        };
        s.add_constraint(K::Coincident { a: corner, b: ORIGIN.into() });
        ops::add_dimension(doc, &sk, K::Distance { a: lines[0].clone(), b: lines[0].clone(), orientation: O::Horizontal }, "50 mm", [20.0, -8.0], false).unwrap();
        ops::add_dimension(doc, &sk, K::Distance { a: lines[1].clone(), b: lines[1].clone(), orientation: O::Vertical }, "30 mm", [48.0, 10.0], false).unwrap();
        st.recompute();
        (st, sk)
    }

    fn view() -> View {
        let mut cam = Camera::default();
        cam.set_view(StandardView::Home);
        cam.fit(Vec3::new(25.0, 15.0, 10.0), 40.0, 1.4);
        View { cam, rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1120.0, 800.0)) }
    }

    #[test]
    fn dragging_the_extrude_arrow_sets_the_distance() {
        let (mut st, sk) = rectangle_state();
        st.select(Pick::Profile { sketch: sk, index: 0 }, false);
        st.execute(CommandId::Extrude);
        let h = st.manipulator_handle().expect("extrude handle");
        assert!((h.value - 10.0).abs() < 1e-9 && (h.dir[2] - 1.0).abs() < 1e-9, "{h:?}");
        assert!((h.origin[0] - 25.0).abs() < 1e-6 && (h.origin[1] - 15.0).abs() < 1e-6 && h.origin[2].abs() < 1e-6, "{h:?}");

        let v = view();
        let mut m = Manipulator::default();
        let at = |value: f64| v.to_screen(h.point_at(value)).unwrap();
        // hover, then press on the arrow at its current length
        let over = PointerInput { pos: Some(at(10.0)), moved: true, time: 0.0, ..Default::default() };
        assert!(m.input(&mut st, &v, &over), "the arrow is hovered");
        assert!(m.input(&mut st, &v, &PointerInput { pressed: true, ..over }));
        assert!(m.dragging());
        // drag to the 25.3 mm point: snapped to the zoom step, preview throttled
        let step = snap_step(v.mm_per_px(), false);
        assert!((0.1..=2.0).contains(&step), "{step}");
        let to = |value: f64, time: f64| PointerInput { pos: Some(at(value)), moved: true, time, ..Default::default() };
        assert!(m.input(&mut st, &v, &to(25.3, 0.01)));
        let coarse = snap(25.3, step);
        assert!((m.drag_value().unwrap() - coarse).abs() < 1e-9, "{:?} vs {coarse}", m.drag_value());
        let Some(Dialog::Extrude(x)) = &st.dialog else { panic!() };
        assert_eq!(x.distance.text, "10 mm", "throttled: no recompute 10 ms into the drag");
        // a later frame applies the live preview
        assert!(m.input(&mut st, &v, &to(25.3, 0.2)));
        let Some(Dialog::Extrude(x)) = &st.dialog else { panic!() };
        assert_eq!(x.distance.text, fmt_mm(coarse));
        // Shift = fine snapping
        assert!(m.input(&mut st, &v, &PointerInput { shift: true, ..to(31.26, 0.25) }));
        let fine = snap(31.26, step / 10.0);
        assert!((m.drag_value().unwrap() - fine).abs() < 1e-9, "{:?} vs {fine}", m.drag_value());
        // release commits
        assert!(m.input(&mut st, &v, &PointerInput { released: true, shift: true, ..to(31.26, 0.3) }));
        assert!(!m.dragging());
        let Some(Dialog::Extrude(x)) = &st.dialog else { panic!() };
        assert_eq!(x.distance.text, fmt_mm(fine));
        st.dialog_ok();
        assert!(st.dialog.is_none(), "{:?}", st.messages.last());
        let vol: f64 = st.model.bodies.iter().map(|b| b.volume()).sum();
        assert!((vol - 50.0 * 30.0 * fine).abs() < 1e-3, "{vol}");
    }

    #[test]
    fn dragging_the_offset_plane_arrow_moves_the_plane() {
        let (mut st, _) = rectangle_state();
        st.select(Pick::OriginPlane(OriginPlane::XY), false);
        st.execute(CommandId::OffsetPlane);
        let h = st.manipulator_handle().expect("offset plane handle");
        assert!(h.origin[2].abs() < 1e-9 && (h.dir[2] - 1.0).abs() < 1e-9 && (h.value - 10.0).abs() < 1e-9, "{h:?}");
        let v = view();
        let mut m = Manipulator::default();
        let at = |value: f64| PointerInput { pos: v.to_screen(h.point_at(value)), moved: true, time: 1.0, ..Default::default() };
        assert!(m.input(&mut st, &v, &PointerInput { pressed: true, ..at(10.0) }));
        assert!(m.input(&mut st, &v, &at(20.2)));
        assert!(m.input(&mut st, &v, &PointerInput { released: true, ..at(20.2) }));
        let Some(Dialog::OffsetPlane(x)) = &st.dialog else { panic!() };
        assert_eq!(x.offset.text, "20 mm");
        let fid = x.s.feature.clone().unwrap();
        assert!((st.model.planes[&fid].origin[2] - 20.0).abs() < 1e-6, "{:?}", st.model.planes[&fid]);
    }

    #[test]
    fn manipulator_ignores_the_pointer_away_from_the_arrow() {
        let (mut st, sk) = rectangle_state();
        st.select(Pick::Profile { sketch: sk, index: 0 }, false);
        st.execute(CommandId::Extrude);
        let v = view();
        let mut m = Manipulator::default();
        let far = PointerInput { pos: Some(egui::pos2(5.0, 5.0)), pressed: true, moved: true, ..Default::default() };
        assert!(!m.input(&mut st, &v, &far));
        assert!(!m.dragging());
        // no dialog: no handle
        st.dialog_cancel();
        assert!(st.manipulator_handle().is_none());
    }
}
