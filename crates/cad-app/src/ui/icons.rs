//! Original vector icons, drawn with `egui::Painter` (no bitmap assets, nothing copied from
//! Autodesk). Every command in the registry has its own glyph; chrome-only glyphs (eye, folder,
//! gear, playback controls ...) live in [`Icon`] as well.
//!
//! Icons are designed in a unit square (0..1, y down) and scale to any rect. Colour language
//! follows Fusion: grey/white solids with a blue "operated" face, blue sketch curves, red-salmon
//! constraint marks, orange construction geometry, green for create / finish.
//!
//! The module also holds the small custom widgets the chrome shares (icon buttons, menu rows).

use crate::state::command::CommandId;
use crate::ui::theme;
use cad_core::FeatureKind;
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui};

const BLUE: Color32 = Color32::from_rgb(0x4a, 0xa0, 0xff);
const BLUE_DEEP: Color32 = Color32::from_rgb(0x2f, 0x7a, 0xe0);
const LIGHT: Color32 = Color32::from_rgb(0xe4, 0xe7, 0xeb);
const GREY: Color32 = Color32::from_rgb(0xa6, 0xac, 0xb4);
const DARK: Color32 = Color32::from_rgb(0x5f, 0x65, 0x6e);
const FACE_T: Color32 = Color32::from_rgb(0xdc, 0xdf, 0xe4);
const FACE_L: Color32 = Color32::from_rgb(0x9a, 0xa1, 0xaa);
const FACE_R: Color32 = Color32::from_rgb(0xbc, 0xc2, 0xca);
const GREEN: Color32 = Color32::from_rgb(0x3c, 0xc2, 0x5a);
const ORANGE: Color32 = Color32::from_rgb(0xf2, 0x9d, 0x38);
const YELLOW: Color32 = Color32::from_rgb(0xf5, 0xc5, 0x42);
const RED: Color32 = Color32::from_rgb(0xe5, 0x48, 0x4d);
const CONSTRAINT: Color32 = Color32::from_rgb(0xe8, 0x73, 0x5a);
const PURPLE: Color32 = Color32::from_rgb(0xa7, 0x8b, 0xfa);
/// Surface bodies / sheets.
const SURF: Color32 = Color32::from_rgb(0x4f, 0xc3, 0xb8);

/// Every glyph the chrome can draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Cmd(CommandId),
    Eye,
    EyeOff,
    Folder,
    Gear,
    Lock,
    Home,
    DataPanel,
    Help,
    FileMenu,
    /// Root component / document.
    Component,
    Body,
    Origin,
    Axis,
    Plane,
    Units,
    Info,
    Warning,
    Error,
    Log,
    ToStart,
    StepBack,
    Play,
    StepForward,
    ToEnd,
    Viewports,
    DisplaySettings,
    Select,
    CaretDown,
    CaretRight,
    Close,
    Workspace,
    /// Circular pattern (feature / command without a dedicated registry arm yet).
    CircularPattern,
    /// Generic timeline feature (unknown feature kind).
    Feature,
}

/// Icon for a timeline / browser feature.
///
/// Chosen from the kernel's `type_label` (not an exhaustive match), so new feature kinds never
/// break this module; unknown kinds get the generic feature glyph.
pub fn feature_icon(kind: &FeatureKind) -> Icon {
    icon_for_type_label(cad_core::document::type_label(kind))
}

/// Icon for a feature type label ("Sketch", "Extrude", "Circular Pattern", "MoveBodies" ...).
pub fn icon_for_type_label(label: &str) -> Icon {
    let l: String = label.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    let c = |id| Icon::Cmd(id);
    match l.as_str() {
        "sketch" => c(CommandId::CreateSketch),
        "extrude" => c(CommandId::Extrude),
        "revolve" => c(CommandId::Revolve),
        "fillet" => c(CommandId::Fillet),
        "chamfer" => c(CommandId::Chamfer),
        "hole" => c(CommandId::Hole),
        "plane" | "offsetplane" => c(CommandId::OffsetPlane),
        "import" | "importstep" => c(CommandId::ImportStep),
        "shell" => c(CommandId::Shell),
        "combine" => c(CommandId::Combine),
        "joint" | "rigidjoint" | "revolutejoint" | "sliderjoint" => c(CommandId::Joint),
        "component" | "newcomponent" => c(CommandId::NewComponent),
        "surfaceextrude" => c(CommandId::SurfaceExtrude),
        "surfacerevolve" => c(CommandId::SurfaceRevolve),
        "patch" => c(CommandId::Patch),
        "stitch" => c(CommandId::Stitch),
        "offsetsurface" => c(CommandId::OffsetSurface),
        "thicken" => c(CommandId::Thicken),
        "mesh" | "insertmesh" => c(CommandId::InsertMesh),
        _ if l.contains("circular") => Icon::CircularPattern,
        _ if l.contains("rectangular") || l == "pattern" => c(CommandId::RectangularPattern),
        _ if l.contains("mirror") => c(CommandId::MirrorFeature),
        _ if l.contains("move") || l.contains("copy") => c(CommandId::Move),
        _ => Icon::Feature,
    }
}

/// Paints `icon` into `rect`. Disabled icons are drawn desaturated and faded.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, enabled: bool) {
    let pen = Pen { p: painter, r: rect, dim: !enabled, k: rect.width().min(rect.height()) / 24.0 };
    match icon {
        Icon::Cmd(id) => {
            draw_command(&pen, id);
        }
        other => draw_chrome(&pen, other),
    }
}

// ---------------------------------------------------------------------------------- pen

struct Pen<'a> {
    p: &'a Painter,
    r: Rect,
    dim: bool,
    /// Stroke-width scale (1.0 at 24 px).
    k: f32,
}

type P = (f32, f32);

impl Pen<'_> {
    fn pt(&self, (x, y): P) -> Pos2 {
        pos2(self.r.left() + x * self.r.width(), self.r.top() + y * self.r.height())
    }
    fn c(&self, c: Color32) -> Color32 {
        if self.dim {
            let l = (0.30 * c.r() as f32 + 0.59 * c.g() as f32 + 0.11 * c.b() as f32) * 0.62;
            let l = l as u8;
            Color32::from_rgba_unmultiplied(l, l, l.saturating_add(4), (c.a() as f32 * 0.75) as u8)
        } else {
            c
        }
    }
    fn s(&self, col: Color32, w: f32) -> Stroke {
        Stroke::new((w * self.k).max(0.8), self.c(col))
    }
    fn line(&self, pts: &[P], col: Color32, w: f32) {
        let v: Vec<Pos2> = pts.iter().map(|&q| self.pt(q)).collect();
        self.p.add(Shape::line(v, self.s(col, w)));
    }
    fn seg(&self, a: P, b: P, col: Color32, w: f32) {
        self.p.line_segment([self.pt(a), self.pt(b)], self.s(col, w));
    }
    fn dashed(&self, a: P, b: P, col: Color32, w: f32) {
        let (a, b) = (self.pt(a), self.pt(b));
        let len = (b - a).length().max(1.0);
        let dash = 2.6 * self.k.max(0.6);
        self.p.extend(Shape::dashed_line(&[a, b], self.s(col, w), dash, dash * 0.8));
        let _ = len;
    }
    fn closed(&self, pts: &[P], col: Color32, w: f32) {
        let v: Vec<Pos2> = pts.iter().map(|&q| self.pt(q)).collect();
        self.p.add(Shape::closed_line(v, self.s(col, w)));
    }
    /// Convex polygon with optional outline.
    fn poly(&self, pts: &[P], fill: Color32, outline: Option<(Color32, f32)>) {
        let v: Vec<Pos2> = pts.iter().map(|&q| self.pt(q)).collect();
        let st = outline.map(|(c, w)| self.s(c, w)).unwrap_or(Stroke::NONE);
        self.p.add(Shape::convex_polygon(v, self.c(fill), st));
    }
    fn rect(&self, a: P, b: P, fill: Option<Color32>, outline: Option<(Color32, f32)>) {
        let r = Rect::from_two_pos(self.pt(a), self.pt(b));
        if let Some(f) = fill {
            self.p.rect_filled(r, CornerRadius::same((self.k * 1.0) as u8), self.c(f));
        }
        if let Some((c, w)) = outline {
            self.p.rect_stroke(r, CornerRadius::same((self.k * 1.0) as u8), self.s(c, w), StrokeKind::Middle);
        }
    }
    /// Circle with radius as a fraction of the icon width.
    fn circle(&self, c: P, rad: f32, fill: Option<Color32>, outline: Option<(Color32, f32)>) {
        let r = rad * self.r.width();
        if let Some(f) = fill {
            self.p.circle_filled(self.pt(c), r, self.c(f));
        }
        if let Some((col, w)) = outline {
            self.p.circle_stroke(self.pt(c), r, self.s(col, w));
        }
    }
    fn dot(&self, c: P, col: Color32) {
        self.p.circle_filled(self.pt(c), 1.7 * self.k.max(0.7), self.c(col));
    }
    fn ring_dot(&self, c: P) {
        self.p.circle_filled(self.pt(c), 2.0 * self.k.max(0.7), self.c(LIGHT));
        self.p.circle_stroke(self.pt(c), 2.0 * self.k.max(0.7), self.s(DARK, 0.7));
    }
    /// Arc (angles in degrees, counter-clockwise as seen on screen, 0 = +x).
    fn arc_pts(&self, c: P, rx: f32, ry: f32, a0: f32, a1: f32) -> Vec<P> {
        let n = 24;
        (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                (c.0 + rx * a.cos(), c.1 - ry * a.sin())
            })
            .collect()
    }
    fn arc(&self, c: P, rad: f32, a0: f32, a1: f32, col: Color32, w: f32) {
        let pts = self.arc_pts(c, rad, rad, a0, a1);
        self.line(&pts, col, w);
    }
    fn ellipse(&self, c: P, rx: f32, ry: f32, col: Color32, w: f32) {
        let pts = self.arc_pts(c, rx, ry, 0.0, 360.0);
        self.closed(&pts, col, w);
    }
    fn arrow(&self, a: P, b: P, col: Color32, w: f32) {
        self.seg(a, b, col, w);
        self.head(a, b, col, 0.13);
    }
    /// Filled arrow head at `b` pointing away from `a` (size as icon fraction).
    fn head(&self, a: P, b: P, col: Color32, size: f32) {
        let (pa, pb) = (self.pt(a), self.pt(b));
        let d = (pb - pa).normalized();
        let n = vec2(-d.y, d.x);
        let s = size * self.r.width();
        let tip = pb + d * s * 0.25;
        let base = pb - d * s * 0.75;
        self.p.add(Shape::convex_polygon(vec![tip, base + n * s * 0.5, base - n * s * 0.5], self.c(col), Stroke::NONE));
    }
    fn text(&self, c: P, t: &str, size: f32, col: Color32) {
        self.p.text(self.pt(c), Align2::CENTER_CENTER, t, FontId::proportional(size * self.r.height()), self.c(col));
    }
    /// Isometric cube centred at `c` with "radius" `s`; returns the 7 key points
    /// (top, top-right, bottom-right, bottom, bottom-left, top-left, centre).
    fn cube(&self, c: P, s: f32, top: Color32, left: Color32, right: Color32) -> [P; 7] {
        let h = 0.866 * s;
        let t = (c.0, c.1 - s);
        let tr = (c.0 + h, c.1 - s * 0.5);
        let br = (c.0 + h, c.1 + s * 0.5);
        let b = (c.0, c.1 + s);
        let bl = (c.0 - h, c.1 + s * 0.5);
        let tl = (c.0 - h, c.1 - s * 0.5);
        let o = c;
        let edge = Some((DARK, 0.6));
        self.poly(&[t, tr, o, tl], top, edge);
        self.poly(&[tl, o, b, bl], left, edge);
        self.poly(&[o, tr, br, b], right, edge);
        [t, tr, br, b, bl, tl, o]
    }
    fn plus_badge(&self, c: P, col: Color32) {
        let s = 0.13;
        self.seg((c.0 - s, c.1), (c.0 + s, c.1), col, 2.4);
        self.seg((c.0, c.1 - s), (c.0, c.1 + s), col, 2.4);
    }
    fn plane(&self, c: P, w: f32, h: f32, fill: Color32, edge: Color32) {
        let sk = w * 0.35;
        let pts = [(c.0 - w * 0.5 + sk, c.1 - h * 0.5), (c.0 + w * 0.5 + sk * 0.0, c.1 - h * 0.5), (c.0 + w * 0.5 - sk, c.1 + h * 0.5), (c.0 - w * 0.5 - sk * 0.0, c.1 + h * 0.5)];
        self.poly(&pts, fill.gamma_multiply(0.55), Some((edge, 1.2)));
    }
    fn lock(&self, c: P, s: f32, col: Color32) {
        self.arc((c.0, c.1 - s * 0.15), s * 0.42, 0.0, 180.0, col, 2.0);
        self.seg((c.0 - s * 0.42, c.1 - s * 0.15), (c.0 - s * 0.42, c.1 + 0.0), col, 2.0);
        self.seg((c.0 + s * 0.42, c.1 - s * 0.15), (c.0 + s * 0.42, c.1 + 0.0), col, 2.0);
        self.rect((c.0 - s * 0.62, c.1), (c.0 + s * 0.62, c.1 + s * 0.85), Some(col), None);
    }
    fn eye(&self, open: bool) {
        let up = self.arc_pts((0.5, 0.78), 0.42, 0.42, 145.0, 35.0);
        let lo = self.arc_pts((0.5, 0.22), 0.42, 0.42, 215.0, 325.0);
        let col = if open { LIGHT } else { GREY };
        self.line(&up, col, 1.5);
        self.line(&lo, col, 1.5);
        if open {
            self.circle((0.5, 0.5), 0.13, Some(col), None);
            self.circle((0.5, 0.5), 0.05, Some(theme::PANEL_BG), None);
        } else {
            self.circle((0.5, 0.5), 0.1, None, Some((col, 1.2)));
            self.seg((0.18, 0.82), (0.82, 0.18), col, 1.6);
        }
    }
}

// ---------------------------------------------------------------------------------- commands

/// Draws the command glyph; returns false for the generic fallback (no dedicated icon).
fn draw_command(p: &Pen, id: CommandId) -> bool {
    use CommandId::*;
    #[allow(unreachable_patterns)]
    match id {
        // ---- file / edit
        NewDesign => {
            page(p);
            p.plus_badge((0.74, 0.74), GREEN);
        }
        Open => {
            p.poly(&[(0.1, 0.3), (0.38, 0.3), (0.45, 0.38), (0.82, 0.38), (0.82, 0.8), (0.1, 0.8)], DARK, Some((GREY, 1.0)));
            p.poly(&[(0.22, 0.48), (0.95, 0.48), (0.82, 0.8), (0.1, 0.8)], GREY, Some((LIGHT, 1.0)));
        }
        Save => floppy(p),
        SaveAs => {
            floppy(p);
            p.seg((0.62, 0.92), (0.95, 0.58), ORANGE, 2.6);
        }
        ImportStep => {
            p.cube((0.58, 0.6), 0.3, FACE_T, FACE_L, FACE_R);
            p.arrow((0.08, 0.12), (0.4, 0.42), GREEN, 2.0);
        }
        ExportStep => {
            p.cube((0.42, 0.6), 0.3, FACE_T, FACE_L, FACE_R);
            p.arrow((0.6, 0.4), (0.92, 0.1), BLUE, 2.0);
        }
        ExportStl => {
            mesh(p, (0.42, 0.6), 0.3);
            p.arrow((0.6, 0.4), (0.92, 0.1), BLUE, 2.0);
        }
        Undo => {
            p.arc((0.52, 0.56), 0.28, 160.0, -30.0, LIGHT, 2.0);
            p.head((0.3, 0.6), (0.24, 0.5), LIGHT, 0.3);
        }
        Redo => {
            p.arc((0.48, 0.56), 0.28, 20.0, 210.0, LIGHT, 2.0);
            p.head((0.7, 0.6), (0.76, 0.5), LIGHT, 0.3);
        }
        Delete => {
            p.seg((0.25, 0.25), (0.75, 0.75), RED, 2.6);
            p.seg((0.75, 0.25), (0.25, 0.75), RED, 2.6);
        }
        ComputeAll => {
            p.arc((0.5, 0.5), 0.3, 20.0, 160.0, BLUE, 2.0);
            p.head((0.78, 0.45), (0.22, 0.38), BLUE, 0.0);
            p.head((0.25, 0.3), (0.22, 0.42), BLUE, 0.25);
            p.arc((0.5, 0.5), 0.3, 200.0, 340.0, BLUE, 2.0);
            p.head((0.75, 0.7), (0.78, 0.58), BLUE, 0.25);
        }

        // ---- sketch create
        CreateSketch => {
            p.rect((0.12, 0.3), (0.72, 0.85), None, Some((BLUE, 1.6)));
            p.dot((0.12, 0.85), LIGHT);
            p.dot((0.72, 0.3), LIGHT);
            p.seg((0.4, 0.62), (0.82, 0.2), ORANGE, 2.6);
            p.seg((0.4, 0.62), (0.36, 0.68), LIGHT, 1.6);
            p.plus_badge((0.84, 0.78), GREEN);
        }
        FinishSketch => {
            p.circle((0.5, 0.5), 0.42, Some(GREEN), None);
            p.line(&[(0.3, 0.52), (0.45, 0.67), (0.72, 0.36)], Color32::WHITE, 2.6);
        }
        Line => {
            p.seg((0.15, 0.8), (0.85, 0.22), BLUE, 1.8);
            p.ring_dot((0.15, 0.8));
            p.ring_dot((0.85, 0.22));
        }
        Rectangle2Pt => {
            p.rect((0.14, 0.26), (0.86, 0.76), None, Some((BLUE, 1.6)));
            p.ring_dot((0.14, 0.76));
            p.ring_dot((0.86, 0.26));
        }
        RectangleCenter => {
            p.rect((0.14, 0.26), (0.86, 0.76), None, Some((BLUE, 1.6)));
            p.ring_dot((0.5, 0.51));
            p.ring_dot((0.86, 0.26));
        }
        Rectangle3Pt => {
            p.closed(&[(0.12, 0.6), (0.48, 0.2), (0.88, 0.48), (0.52, 0.88)], BLUE, 1.6);
            p.ring_dot((0.12, 0.6));
            p.ring_dot((0.48, 0.2));
            p.ring_dot((0.88, 0.48));
        }
        CircleCenter => {
            p.circle((0.5, 0.5), 0.36, None, Some((BLUE, 1.6)));
            p.dashed((0.5, 0.5), (0.76, 0.25), GREY, 1.0);
            p.ring_dot((0.5, 0.5));
        }
        Circle2Pt => {
            p.circle((0.5, 0.5), 0.36, None, Some((BLUE, 1.6)));
            p.ring_dot((0.14, 0.5));
            p.ring_dot((0.86, 0.5));
        }
        Arc3Pt => {
            p.arc((0.5, 0.66), 0.36, 180.0, 0.0, BLUE, 1.8);
            p.ring_dot((0.14, 0.66));
            p.ring_dot((0.5, 0.3));
            p.ring_dot((0.86, 0.66));
        }
        ArcCenter => {
            p.arc((0.35, 0.65), 0.45, 80.0, -5.0, BLUE, 1.8);
            p.dashed((0.35, 0.65), (0.42, 0.21), GREY, 1.0);
            p.dashed((0.35, 0.65), (0.8, 0.69), GREY, 1.0);
            p.ring_dot((0.35, 0.65));
        }
        Point => {
            p.seg((0.3, 0.5), (0.7, 0.5), GREY, 1.0);
            p.seg((0.5, 0.3), (0.5, 0.7), GREY, 1.0);
            p.circle((0.5, 0.5), 0.1, Some(BLUE), Some((LIGHT, 1.0)));
        }
        Polygon => {
            let pts: Vec<P> = (0..6).map(|i| (0.5 + 0.38 * (i as f32 * 60.0f32).to_radians().cos(), 0.5 + 0.38 * (i as f32 * 60.0f32).to_radians().sin())).collect();
            p.closed(&pts, BLUE, 1.6);
            p.ring_dot((0.5, 0.5));
        }
        Spline => {
            let pts: Vec<P> = (0..=24).map(|i| {
                let t = i as f32 / 24.0;
                (0.12 + 0.76 * t, 0.5 - 0.28 * (t * std::f32::consts::TAU).sin())
            }).collect();
            p.line(&pts, BLUE, 1.8);
            p.ring_dot((0.12, 0.5));
            p.ring_dot((0.31, 0.22));
            p.ring_dot((0.69, 0.78));
            p.ring_dot((0.88, 0.5));
        }
        Slot => {
            p.arc((0.3, 0.5), 0.2, 90.0, 270.0, BLUE, 1.6);
            p.arc((0.7, 0.5), 0.2, -90.0, 90.0, BLUE, 1.6);
            p.seg((0.3, 0.3), (0.7, 0.3), BLUE, 1.6);
            p.seg((0.3, 0.7), (0.7, 0.7), BLUE, 1.6);
            p.dashed((0.3, 0.5), (0.7, 0.5), GREY, 1.0);
        }
        // ---- sketch modify
        SketchFillet => {
            p.seg((0.2, 0.85), (0.2, 0.5), GREY, 1.6);
            p.seg((0.5, 0.2), (0.85, 0.2), GREY, 1.6);
            p.arc((0.5, 0.5), 0.3, 90.0, 180.0, BLUE, 2.2);
            p.dashed((0.2, 0.5), (0.2, 0.2), GREY, 1.0);
            p.dashed((0.2, 0.2), (0.5, 0.2), GREY, 1.0);
        }
        Trim => {
            p.dashed((0.08, 0.3), (0.92, 0.3), BLUE, 1.2);
            p.seg((0.3, 0.12), (0.62, 0.66), LIGHT, 1.8);
            p.seg((0.7, 0.12), (0.38, 0.66), LIGHT, 1.8);
            p.circle((0.32, 0.78), 0.11, None, Some((GREY, 1.6)));
            p.circle((0.68, 0.78), 0.11, None, Some((GREY, 1.6)));
        }
        Extend => {
            p.seg((0.1, 0.55), (0.5, 0.55), BLUE, 1.8);
            p.dashed((0.5, 0.55), (0.78, 0.55), BLUE, 1.2);
            p.head((0.5, 0.55), (0.76, 0.55), LIGHT, 0.2);
            p.seg((0.86, 0.2), (0.86, 0.88), GREY, 1.8);
        }
        SketchOffset => {
            p.arc((0.52, 0.5), 0.34, 90.0, 270.0, GREY, 1.6);
            p.seg((0.52, 0.16), (0.9, 0.16), GREY, 1.6);
            p.seg((0.52, 0.84), (0.9, 0.84), GREY, 1.6);
            p.arc((0.52, 0.5), 0.17, 90.0, 270.0, BLUE, 1.8);
            p.seg((0.52, 0.33), (0.9, 0.33), BLUE, 1.8);
            p.seg((0.52, 0.67), (0.9, 0.67), BLUE, 1.8);
        }
        SketchMirror => {
            p.dashed((0.5, 0.08), (0.5, 0.92), ORANGE, 1.2);
            p.closed(&[(0.42, 0.25), (0.42, 0.75), (0.12, 0.75)], BLUE, 1.6);
            p.closed(&[(0.58, 0.25), (0.58, 0.75), (0.88, 0.75)], GREY, 1.2);
        }
        Project => {
            p.cube((0.5, 0.3), 0.2, FACE_T, FACE_L, FACE_R);
            p.dashed((0.5, 0.52), (0.5, 0.74), BLUE, 1.2);
            p.head((0.5, 0.6), (0.5, 0.76), BLUE, 0.18);
            p.plane((0.5, 0.85), 0.75, 0.16, BLUE, BLUE);
        }
        SketchDimension => {
            p.seg((0.16, 0.25), (0.16, 0.85), GREY, 1.2);
            p.seg((0.84, 0.25), (0.84, 0.85), GREY, 1.2);
            p.seg((0.2, 0.45), (0.8, 0.45), LIGHT, 1.4);
            p.head((0.5, 0.45), (0.2, 0.45), LIGHT, 0.2);
            p.head((0.5, 0.45), (0.8, 0.45), LIGHT, 0.2);
            p.seg((0.16, 0.85), (0.84, 0.85), BLUE, 1.8);
        }
        ToggleConstruction => {
            p.dashed((0.15, 0.82), (0.85, 0.2), ORANGE, 1.8);
            p.ring_dot((0.15, 0.82));
            p.ring_dot((0.85, 0.2));
        }
        // ---- constraints
        ConstraintCoincident => {
            p.seg((0.15, 0.82), (0.48, 0.48), GREY, 1.6);
            p.seg((0.48, 0.48), (0.88, 0.62), GREY, 1.6);
            p.circle((0.48, 0.48), 0.12, None, Some((CONSTRAINT, 2.0)));
        }
        ConstraintCollinear => {
            p.seg((0.1, 0.88), (0.44, 0.54), CONSTRAINT, 2.2);
            p.seg((0.56, 0.46), (0.9, 0.12), CONSTRAINT, 2.2);
        }
        ConstraintConcentric => {
            p.circle((0.5, 0.5), 0.38, None, Some((CONSTRAINT, 1.8)));
            p.circle((0.5, 0.5), 0.2, None, Some((CONSTRAINT, 1.8)));
        }
        ConstraintMidpoint => {
            p.seg((0.08, 0.7), (0.92, 0.7), GREY, 1.6);
            p.poly(&[(0.5, 0.36), (0.66, 0.62), (0.34, 0.62)], CONSTRAINT, None);
        }
        ConstraintFix => p.lock((0.5, 0.45), 0.5, CONSTRAINT),
        ConstraintParallel => {
            p.seg((0.15, 0.85), (0.55, 0.15), CONSTRAINT, 2.2);
            p.seg((0.45, 0.85), (0.85, 0.15), CONSTRAINT, 2.2);
        }
        ConstraintPerpendicular => {
            p.seg((0.12, 0.82), (0.88, 0.82), CONSTRAINT, 2.2);
            p.seg((0.5, 0.82), (0.5, 0.15), CONSTRAINT, 2.2);
        }
        ConstraintHorizontalVertical => {
            p.seg((0.1, 0.82), (0.62, 0.82), CONSTRAINT, 2.4);
            p.seg((0.82, 0.12), (0.82, 0.64), CONSTRAINT, 2.4);
        }
        ConstraintTangent => {
            p.circle((0.5, 0.62), 0.27, None, Some((CONSTRAINT, 1.8)));
            p.seg((0.08, 0.35), (0.92, 0.35), CONSTRAINT, 2.0);
        }
        ConstraintSmooth => {
            p.arc((0.3, 0.75), 0.28, 90.0, 0.0, CONSTRAINT, 2.0);
            p.arc((0.86, 0.75), 0.28, 180.0, 90.0, CONSTRAINT, 2.0);
            p.text((0.5, 0.25), "G2", 0.34, LIGHT);
        }
        ConstraintEqual => {
            p.seg((0.18, 0.38), (0.82, 0.38), CONSTRAINT, 3.0);
            p.seg((0.18, 0.62), (0.82, 0.62), CONSTRAINT, 3.0);
        }
        ConstraintSymmetry => {
            p.dashed((0.5, 0.1), (0.5, 0.9), GREY, 1.2);
            p.line(&[(0.36, 0.22), (0.18, 0.22), (0.18, 0.78), (0.36, 0.78)], CONSTRAINT, 2.0);
            p.line(&[(0.64, 0.22), (0.82, 0.22), (0.82, 0.78), (0.64, 0.78)], CONSTRAINT, 2.0);
        }

        // ---- solid create
        Extrude => {
            p.cube((0.46, 0.6), 0.36, BLUE, FACE_L, FACE_R);
            p.arrow((0.46, 0.42), (0.46, 0.06), GREEN, 2.4);
        }
        Revolve => {
            let back = p.arc_pts((0.5, 0.58), 0.38, 0.15, 20.0, 160.0);
            p.line(&back, GREY, 1.2);
            p.rect((0.56, 0.3), (0.74, 0.72), Some(BLUE), Some((LIGHT, 0.6)));
            p.dashed((0.5, 0.06), (0.5, 0.94), ORANGE, 1.4);
            let a = p.arc_pts((0.5, 0.58), 0.38, 0.15, 200.0, 340.0);
            p.line(&a, LIGHT, 2.0);
            p.head(a[a.len() - 3], a[a.len() - 1], LIGHT, 0.26);
        }
        Sweep => {
            let path = p.arc_pts((0.15, 0.85), 0.65, 0.65, 90.0, 0.0);
            p.line(&path, GREY, 4.0);
            p.line(&path, BLUE, 1.2);
            p.ellipse((0.15, 0.2), 0.08, 0.13, LIGHT, 1.6);
        }
        Loft => {
            p.ellipse((0.5, 0.22), 0.28, 0.08, LIGHT, 1.6);
            p.closed(&[(0.25, 0.78), (0.5, 0.88), (0.75, 0.78), (0.5, 0.68)], BLUE, 1.6);
            p.seg((0.22, 0.22), (0.25, 0.78), GREY, 1.0);
            p.seg((0.78, 0.22), (0.75, 0.78), GREY, 1.0);
        }
        Rib => {
            p.rect((0.1, 0.65), (0.9, 0.85), Some(FACE_L), Some((DARK, 0.6)));
            p.rect((0.1, 0.15), (0.26, 0.65), Some(FACE_R), Some((DARK, 0.6)));
            p.poly(&[(0.26, 0.25), (0.8, 0.65), (0.26, 0.65)], BLUE, Some((DARK, 0.6)));
        }
        Hole => {
            let k = p.cube((0.5, 0.56), 0.36, FACE_T, FACE_L, FACE_R);
            let c = ((k[0].0 + k[6].0) * 0.5, (k[0].1 + k[6].1) * 0.5);
            let ell = p.arc_pts(c, 0.16, 0.08, 0.0, 360.0);
            p.poly(&ell, Color32::from_rgb(0x2a, 0x2d, 0x33), Some((BLUE, 1.6)));
        }
        Thread => {
            p.rect((0.3, 0.18), (0.7, 0.82), Some(FACE_R), Some((DARK, 0.6)));
            for i in 0..5 {
                let y = 0.24 + i as f32 * 0.12;
                p.seg((0.3, y + 0.06), (0.7, y), BLUE, 1.4);
            }
        }
        BoxPrimitive => {
            p.cube((0.5, 0.52), 0.38, FACE_T, FACE_L, FACE_R);
        }
        RectangularPattern => {
            for (x, y) in [(0.3, 0.3), (0.72, 0.3), (0.3, 0.72), (0.72, 0.72)] {
                p.cube((x, y), 0.17, if (x, y) == (0.3, 0.3) { BLUE } else { FACE_T }, FACE_L, FACE_R);
            }
        }
        MirrorFeature => {
            p.cube((0.24, 0.55), 0.18, BLUE, FACE_L, FACE_R);
            p.cube((0.76, 0.55), 0.18, FACE_T, FACE_L, FACE_R);
            p.poly(&[(0.44, 0.12), (0.56, 0.22), (0.56, 0.92), (0.44, 0.82)], ORANGE.gamma_multiply(0.6), Some((ORANGE, 1.0)));
        }

        // ---- modify
        PressPull => {
            p.cube((0.42, 0.6), 0.3, FACE_T, BLUE, FACE_R);
            p.arrow((0.3, 0.62), (0.06, 0.76), GREEN, 2.0);
            p.arrow((0.72, 0.3), (0.92, 0.16), LIGHT, 1.6);
        }
        Fillet => {
            let mut pts = vec![(0.15, 0.88), (0.15, 0.18)];
            pts.extend(p.arc_pts((0.5, 0.53), 0.35, 0.35, 90.0, 0.0));
            pts.push((0.85, 0.88));
            p.poly(&pts, FACE_R, Some((DARK, 0.6)));
            let a = p.arc_pts((0.5, 0.53), 0.35, 0.35, 90.0, 0.0);
            p.line(&a, BLUE, 3.0);
        }
        Chamfer => {
            p.poly(&[(0.15, 0.88), (0.15, 0.18), (0.55, 0.18), (0.85, 0.48), (0.85, 0.88)], FACE_R, Some((DARK, 0.6)));
            p.seg((0.55, 0.18), (0.85, 0.48), BLUE, 3.0);
        }
        Shell => {
            let k = p.cube((0.5, 0.56), 0.38, FACE_T, FACE_L, FACE_R);
            let inset = |q: P| (k[6].0 + (q.0 - k[6].0) * 0.6, (k[0].1 + k[6].1) * 0.5 + (q.1 - (k[0].1 + k[6].1) * 0.5) * 0.6);
            p.poly(&[inset(k[0]), inset(k[1]), inset(k[6]), inset(k[5])], Color32::from_rgb(0x50, 0x55, 0x5d), Some((BLUE, 1.4)));
        }
        Draft => {
            p.poly(&[(0.28, 0.18), (0.72, 0.18), (0.88, 0.85), (0.12, 0.85)], FACE_R, Some((DARK, 0.6)));
            p.seg((0.72, 0.18), (0.88, 0.85), BLUE, 3.0);
            p.dashed((0.72, 0.18), (0.72, 0.85), GREY, 1.0);
        }
        Combine => {
            p.rect((0.1, 0.1), (0.62, 0.62), Some(FACE_R), Some((DARK, 0.6)));
            p.rect((0.38, 0.38), (0.9, 0.9), Some(BLUE.gamma_multiply(0.85)), Some((LIGHT, 0.8)));
        }
        Move => {
            p.arrow((0.5, 0.5), (0.5, 0.1), LIGHT, 1.8);
            p.arrow((0.5, 0.5), (0.5, 0.9), LIGHT, 1.8);
            p.arrow((0.5, 0.5), (0.1, 0.5), LIGHT, 1.8);
            p.arrow((0.5, 0.5), (0.9, 0.5), LIGHT, 1.8);
            p.circle((0.5, 0.5), 0.08, Some(BLUE), None);
        }
        Appearance => {
            p.circle((0.48, 0.52), 0.36, Some(ORANGE), None);
            p.circle((0.42, 0.44), 0.22, Some(Color32::from_rgb(0xf7, 0xb8, 0x6a)), None);
            p.circle((0.36, 0.36), 0.08, Some(Color32::from_rgb(0xfd, 0xe6, 0xc8)), None);
            p.circle((0.8, 0.8), 0.12, Some(BLUE), Some((LIGHT, 1.0)));
        }
        ChangeParameters => {
            p.rect((0.1, 0.16), (0.9, 0.84), Some(Color32::from_rgb(0x3a, 0x3e, 0x46)), Some((GREY, 1.0)));
            p.text((0.38, 0.5), "ƒx", 0.5, LIGHT);
            p.seg((0.66, 0.36), (0.82, 0.36), BLUE, 2.0);
            p.seg((0.66, 0.5), (0.82, 0.5), BLUE, 2.0);
            p.seg((0.66, 0.64), (0.82, 0.64), BLUE, 2.0);
        }

        // ---- assemble
        NewComponent => {
            p.cube((0.44, 0.56), 0.32, FACE_T, FACE_L, FACE_R);
            p.plus_badge((0.82, 0.22), GREEN);
        }
        Joint => {
            p.circle((0.32, 0.62), 0.18, None, Some((LIGHT, 2.0)));
            p.circle((0.68, 0.38), 0.18, None, Some((BLUE, 2.0)));
            p.dot((0.32, 0.62), LIGHT);
            p.dot((0.68, 0.38), BLUE);
        }

        // ---- construct
        OffsetPlane => {
            p.plane((0.48, 0.74), 0.7, 0.24, GREY, GREY);
            p.plane((0.52, 0.32), 0.7, 0.24, ORANGE, ORANGE);
            p.arrow((0.5, 0.7), (0.5, 0.38), LIGHT, 1.4);
        }
        PlaneAtAngle => {
            p.plane((0.5, 0.74), 0.76, 0.24, GREY, GREY);
            p.poly(&[(0.22, 0.62), (0.78, 0.62), (0.92, 0.16), (0.36, 0.16)], ORANGE.gamma_multiply(0.55), Some((ORANGE, 1.2)));
            p.seg((0.22, 0.62), (0.78, 0.62), BLUE, 2.0);
        }
        MidPlane => {
            p.plane((0.5, 0.2), 0.7, 0.18, GREY, GREY);
            p.plane((0.5, 0.5), 0.7, 0.18, ORANGE, ORANGE);
            p.plane((0.5, 0.8), 0.7, 0.18, GREY, GREY);
        }
        AxisThroughCylinder => {
            p.rect((0.28, 0.24), (0.72, 0.8), Some(FACE_R), None);
            p.ellipse((0.5, 0.24), 0.22, 0.07, DARK, 1.0);
            p.arc((0.5, 0.8), 0.22, 180.0, 360.0, DARK, 1.0);
            p.seg((0.5, 0.04), (0.5, 0.96), ORANGE, 2.0);
        }

        // ---- inspect
        Measure => {
            p.poly(&[(0.08, 0.62), (0.62, 0.08), (0.92, 0.38), (0.38, 0.92)], YELLOW, Some((DARK, 0.6)));
            for i in 1..6 {
                let t = i as f32 / 6.0;
                let a = (0.08 + 0.54 * t, 0.62 - 0.54 * t);
                let l = if i % 2 == 0 { 0.16 } else { 0.1 };
                p.seg(a, (a.0 + l, a.1 + l), DARK, 1.2);
            }
        }
        SectionAnalysis => {
            p.cube((0.5, 0.56), 0.36, FACE_T, FACE_L, FACE_R);
            p.poly(&[(0.12, 0.3), (0.62, 0.05), (0.88, 0.7), (0.38, 0.95)], BLUE.gamma_multiply(0.45), Some((BLUE, 1.2)));
        }
        Interference => {
            p.rect((0.1, 0.1), (0.62, 0.62), Some(FACE_R), Some((DARK, 0.6)));
            p.rect((0.38, 0.38), (0.9, 0.9), Some(FACE_L), Some((DARK, 0.6)));
            p.rect((0.38, 0.38), (0.62, 0.62), Some(RED), None);
        }

        // ---- insert
        InsertMesh => mesh(p, (0.5, 0.52), 0.38),
        InsertSvg => {
            p.text((0.5, 0.3), "SVG", 0.34, LIGHT);
            let pts: Vec<P> = (0..=20).map(|i| {
                let t = i as f32 / 20.0;
                (0.12 + 0.76 * t, 0.72 - 0.12 * (t * std::f32::consts::TAU).sin())
            }).collect();
            p.line(&pts, PURPLE, 2.0);
        }

        // ---- view
        Fit => {
            for (a, b, c) in [((0.1, 0.32), (0.1, 0.1), (0.32, 0.1)), ((0.68, 0.1), (0.9, 0.1), (0.9, 0.32)), ((0.9, 0.68), (0.9, 0.9), (0.68, 0.9)), ((0.32, 0.9), (0.1, 0.9), (0.1, 0.68))] {
                p.line(&[a, b, c], LIGHT, 1.6);
            }
            p.cube((0.5, 0.5), 0.2, FACE_T, FACE_L, FACE_R);
        }
        ViewHome => house(p, LIGHT),
        ViewTop | ViewFront | ViewRight | ViewBack | ViewLeft | ViewBottom => {
            let (t, l, r) = match id {
                ViewTop => (BLUE, FACE_L, FACE_R),
                ViewFront => (FACE_T, BLUE, FACE_R),
                ViewRight => (FACE_T, FACE_L, BLUE),
                _ => (FACE_T, FACE_L, FACE_R),
            };
            p.cube((0.45, 0.5), 0.36, t, l, r);
            let letter = match id {
                ViewBack => Some("Bk"),
                ViewLeft => Some("L"),
                ViewBottom => Some("Bt"),
                _ => None,
            };
            if let Some(s) = letter {
                p.circle((0.78, 0.78), 0.2, Some(BLUE_DEEP), None);
                p.text((0.78, 0.78), s, 0.26, Color32::WHITE);
            }
        }
        ToggleProjection => {
            p.closed(&[(0.12, 0.38), (0.88, 0.14), (0.88, 0.86), (0.12, 0.62)], LIGHT, 1.4);
            p.dashed((0.12, 0.5), (0.88, 0.5), GREY, 1.0);
            p.circle((0.12, 0.5), 0.06, Some(BLUE), None);
        }
        LookAt => {
            p.plane((0.6, 0.6), 0.6, 0.4, BLUE, BLUE);
            p.rect((0.1, 0.12), (0.42, 0.36), Some(GREY), Some((LIGHT, 0.8)));
            p.poly(&[(0.42, 0.18), (0.54, 0.1), (0.54, 0.38), (0.42, 0.3)], GREY, None);
            p.arrow((0.3, 0.4), (0.5, 0.56), LIGHT, 1.2);
        }
        ToggleGrid => {
            for i in 0..4 {
                let t = 0.14 + i as f32 * 0.24;
                p.seg((t, 0.14), (t, 0.86), LIGHT, 1.2);
                p.seg((0.14, t), (0.86, t), LIGHT, 1.2);
            }
        }
        OrbitMode => {
            let a = p.arc_pts((0.5, 0.5), 0.4, 0.2, 200.0, 520.0);
            p.line(&a[..a.len() - 2], LIGHT, 1.6);
            p.head(a[a.len() - 4], a[a.len() - 2], LIGHT, 0.24);
            p.circle((0.5, 0.5), 0.12, Some(BLUE), None);
            p.seg((0.5, 0.12), (0.5, 0.3), GREY, 1.0);
        }
        PanMode => {
            p.rect((0.28, 0.44), (0.76, 0.88), Some(LIGHT), None);
            for (i, top) in [0.16f32, 0.1, 0.12, 0.2].iter().enumerate() {
                let x = 0.32 + i as f32 * 0.13;
                p.seg((x, *top), (x, 0.55), LIGHT, 3.2);
            }
            p.seg((0.3, 0.66), (0.12, 0.46), LIGHT, 3.2);
        }
        ZoomMode => {
            p.circle((0.42, 0.42), 0.26, None, Some((LIGHT, 1.8)));
            p.seg((0.61, 0.61), (0.88, 0.88), LIGHT, 3.2);
            p.seg((0.3, 0.42), (0.54, 0.42), BLUE, 1.8);
            p.seg((0.42, 0.3), (0.42, 0.54), BLUE, 1.8);
        }
        CenterOfMass => {
            p.cube((0.5, 0.56), 0.36, FACE_T, FACE_L, FACE_R);
            p.circle((0.5, 0.56), 0.13, Some(Color32::WHITE), Some((DARK, 0.8)));
            p.poly(&[(0.5, 0.43), (0.63, 0.56), (0.5, 0.56)], Color32::from_rgb(0x20, 0x22, 0x26), None);
            p.poly(&[(0.37, 0.56), (0.5, 0.56), (0.5, 0.69)], Color32::from_rgb(0x20, 0x22, 0x26), None);
        }
        // ===== surface ===== (SURFACE / MESH / UTILITIES tabs)
        CreateForm => {
            sheet(p, (0.5, 0.62), 0.7, 0.36, SURF);
            p.circle((0.52, 0.34), 0.2, Some(PURPLE.gamma_multiply(0.7)), Some((PURPLE, 1.4)));
            for c in [(0.4, 0.3), (0.62, 0.28), (0.52, 0.44)] {
                p.dot(c, LIGHT);
            }
        }
        SurfaceExtrude => {
            sheet(p, (0.5, 0.5), 0.62, 0.5, SURF);
            p.line(&[(0.16, 0.86), (0.4, 0.74), (0.62, 0.84), (0.84, 0.72)], BLUE, 2.2);
            p.arrow((0.5, 0.44), (0.5, 0.06), GREEN, 2.2);
        }
        SurfaceRevolve => {
            let back = p.arc_pts((0.5, 0.56), 0.36, 0.14, 20.0, 160.0);
            p.line(&back, GREY, 1.2);
            p.poly(&[(0.6, 0.26), (0.76, 0.3), (0.76, 0.74), (0.6, 0.7)], SURF.gamma_multiply(0.75), Some((SURF, 1.0)));
            p.dashed((0.5, 0.06), (0.5, 0.94), ORANGE, 1.4);
            let a = p.arc_pts((0.5, 0.56), 0.36, 0.14, 200.0, 340.0);
            p.line(&a, SURF, 2.0);
            p.head(a[a.len() - 3], a[a.len() - 1], SURF, 0.24);
        }
        SurfaceSweep => {
            let path = p.arc_pts((0.15, 0.85), 0.65, 0.65, 90.0, 0.0);
            p.line(&path, SURF.gamma_multiply(0.6), 5.0);
            p.line(&path, GREY, 1.0);
            p.seg((0.08, 0.14), (0.22, 0.26), BLUE, 2.0);
        }
        SurfaceLoft => {
            p.poly(&[(0.22, 0.24), (0.78, 0.24), (0.72, 0.76), (0.28, 0.76)], SURF.gamma_multiply(0.55), None);
            p.arc((0.5, 0.36), 0.28, 20.0, 160.0, BLUE, 2.0);
            p.arc((0.5, 0.9), 0.22, 30.0, 150.0, BLUE, 2.0);
        }
        Patch => {
            let pts = [(0.2, 0.3), (0.5, 0.14), (0.84, 0.32), (0.78, 0.76), (0.4, 0.86), (0.14, 0.62)];
            p.poly(&pts, SURF.gamma_multiply(0.7), None);
            p.closed(&pts, BLUE, 2.0);
        }
        Ruled => {
            p.line(&[(0.12, 0.2), (0.4, 0.14), (0.88, 0.22)], BLUE, 2.0);
            for i in 0..5 {
                let t = 0.14 + i as f32 * 0.18;
                p.seg((t, 0.2), (t - 0.06, 0.82), SURF, 1.2);
            }
            p.line(&[(0.06, 0.82), (0.5, 0.88), (0.82, 0.8)], GREY, 1.6);
        }
        OffsetSurface => {
            sheet(p, (0.44, 0.7), 0.6, 0.3, GREY);
            sheet(p, (0.56, 0.32), 0.6, 0.3, SURF);
            p.arrow((0.44, 0.66), (0.56, 0.38), LIGHT, 1.4);
        }
        Thicken => {
            p.poly(&[(0.2, 0.42), (0.44, 0.24), (0.86, 0.24), (0.62, 0.42)], SURF, Some((DARK, 0.6)));
            p.poly(&[(0.2, 0.42), (0.62, 0.42), (0.62, 0.64), (0.2, 0.64)], FACE_L, Some((DARK, 0.6)));
            p.poly(&[(0.62, 0.42), (0.86, 0.24), (0.86, 0.46), (0.62, 0.64)], FACE_R, Some((DARK, 0.6)));
            p.arrow((0.5, 0.92), (0.5, 0.7), GREEN, 1.8);
        }
        BoundaryFill => {
            sheet(p, (0.5, 0.36), 0.74, 0.3, SURF);
            sheet(p, (0.5, 0.7), 0.74, 0.3, GREY);
            p.rect((0.36, 0.42), (0.6, 0.64), Some(BLUE), Some((LIGHT, 0.8)));
        }
        SurfaceTrim => {
            p.poly(&[(0.14, 0.3), (0.62, 0.3), (0.86, 0.7), (0.38, 0.7)], SURF.gamma_multiply(0.75), Some((SURF, 1.0)));
            p.poly(&[(0.5, 0.3), (0.62, 0.3), (0.86, 0.7), (0.5, 0.7)], Color32::from_rgb(0x30, 0x33, 0x39), None);
            p.dashed((0.5, 0.1), (0.5, 0.92), RED, 1.6);
        }
        Untrim => {
            sheet(p, (0.5, 0.5), 0.76, 0.56, SURF);
            p.ellipse((0.5, 0.5), 0.16, 0.1, LIGHT, 1.2);
            p.plus_badge((0.82, 0.2), GREEN);
        }
        SurfaceExtend => {
            sheet(p, (0.38, 0.5), 0.5, 0.5, SURF);
            p.dashed((0.64, 0.28), (0.9, 0.28), SURF, 1.2);
            p.dashed((0.56, 0.74), (0.84, 0.74), SURF, 1.2);
            p.arrow((0.58, 0.5), (0.9, 0.5), GREEN, 2.0);
        }
        Stitch => {
            p.poly(&[(0.08, 0.3), (0.48, 0.3), (0.48, 0.74), (0.08, 0.74)], SURF.gamma_multiply(0.75), None);
            p.poly(&[(0.52, 0.26), (0.92, 0.3), (0.92, 0.74), (0.52, 0.7)], SURF.gamma_multiply(0.55), None);
            for i in 0..4 {
                let y = 0.34 + i as f32 * 0.11;
                p.seg((0.42, y), (0.58, y + 0.05), LIGHT, 1.6);
            }
        }
        Unstitch => {
            p.poly(&[(0.06, 0.3), (0.4, 0.3), (0.4, 0.74), (0.06, 0.74)], SURF.gamma_multiply(0.75), None);
            p.poly(&[(0.6, 0.3), (0.94, 0.3), (0.94, 0.74), (0.6, 0.74)], SURF.gamma_multiply(0.55), None);
            p.arrow((0.5, 0.52), (0.42, 0.52), LIGHT, 1.4);
            p.arrow((0.5, 0.52), (0.58, 0.52), LIGHT, 1.4);
        }
        ReverseNormal => {
            sheet(p, (0.5, 0.56), 0.76, 0.3, SURF);
            p.arrow((0.42, 0.56), (0.42, 0.12), GREEN, 1.8);
            p.arrow((0.6, 0.56), (0.6, 0.94), RED, 1.8);
        }
        Tessellate => {
            p.cube((0.3, 0.4), 0.2, FACE_T, FACE_L, FACE_R);
            p.arrow((0.42, 0.66), (0.56, 0.66), LIGHT, 1.4);
            mesh(p, (0.74, 0.62), 0.2);
        }
        MeshSectionSketch => {
            mesh(p, (0.5, 0.56), 0.36);
            p.line(&[(0.1, 0.5), (0.5, 0.68), (0.9, 0.5)], BLUE, 2.2);
        }
        BaseMeshFeature => {
            mesh(p, (0.44, 0.58), 0.32);
            p.plus_badge((0.82, 0.2), GREEN);
        }
        MeshRepair => {
            mesh(p, (0.42, 0.58), 0.32);
            p.line(&[(0.64, 0.22), (0.74, 0.32), (0.92, 0.1)], GREEN, 2.4);
        }
        GenerateFaceGroups => {
            let k = p.cube((0.5, 0.54), 0.38, BLUE, ORANGE, GREEN);
            p.seg(k[0], k[6], DARK, 0.9);
            p.seg(k[6], k[4], DARK, 0.9);
        }
        CombineFaceGroups => {
            p.cube((0.46, 0.58), 0.34, BLUE, BLUE.gamma_multiply(0.8), FACE_R);
            p.plus_badge((0.84, 0.2), GREEN);
        }
        MeshDirectEdit => {
            mesh(p, (0.42, 0.6), 0.32);
            p.seg((0.6, 0.42), (0.9, 0.12), YELLOW, 3.0);
            p.seg((0.56, 0.46), (0.6, 0.42), LIGHT, 3.0);
        }
        Remesh => tri_grid(p, 4, SURF),
        MeshReduce => {
            tri_grid(p, 2, GREY);
            p.seg((0.66, 0.08), (0.9, 0.08), RED, 2.4);
        }
        MeshPlaneCut => {
            mesh(p, (0.5, 0.58), 0.34);
            p.poly(&[(0.08, 0.5), (0.7, 0.36), (0.92, 0.5), (0.3, 0.64)], ORANGE.gamma_multiply(0.55), Some((ORANGE, 1.2)));
        }
        MeshShell => {
            let k = p.cube((0.5, 0.56), 0.38, FACE_T, FACE_L, FACE_R);
            let inset = |q: P| (k[6].0 + (q.0 - k[6].0) * 0.6, (k[0].1 + k[6].1) * 0.5 + (q.1 - (k[0].1 + k[6].1) * 0.5) * 0.6);
            p.poly(&[inset(k[0]), inset(k[1]), inset(k[6]), inset(k[5])], Color32::from_rgb(0x50, 0x55, 0x5d), Some((SURF, 1.4)));
            p.seg(k[5], k[3], DARK, 0.9);
            p.seg(k[6], k[2], DARK, 0.9);
        }
        MeshCombine => {
            mesh(p, (0.36, 0.42), 0.24);
            mesh(p, (0.62, 0.62), 0.24);
        }
        MeshSmooth => {
            p.line(&[(0.08, 0.7), (0.24, 0.42), (0.4, 0.66), (0.56, 0.38), (0.72, 0.62), (0.92, 0.36)], GREY, 1.4);
            let pts: Vec<P> = (0..=20)
                .map(|i| {
                    let t = i as f32 / 20.0;
                    (0.08 + 0.84 * t, 0.46 - 0.06 * (t * std::f32::consts::TAU).sin())
                })
                .collect();
            p.line(&pts, SURF, 2.2);
        }
        MeshReverseNormal => {
            mesh(p, (0.38, 0.6), 0.28);
            p.arrow((0.76, 0.5), (0.76, 0.12), GREEN, 1.8);
            p.arrow((0.76, 0.54), (0.76, 0.92), RED, 1.8);
        }
        MeshSeparate => {
            mesh(p, (0.28, 0.4), 0.2);
            mesh(p, (0.72, 0.66), 0.2);
            p.dashed((0.2, 0.86), (0.84, 0.18), GREY, 1.0);
        }
        MeshScale => {
            mesh(p, (0.34, 0.66), 0.24);
            p.arrow((0.56, 0.44), (0.88, 0.12), LIGHT, 1.6);
        }
        ConvertMesh => {
            mesh(p, (0.28, 0.4), 0.22);
            p.arrow((0.42, 0.62), (0.56, 0.62), LIGHT, 1.4);
            p.cube((0.74, 0.62), 0.2, BLUE, FACE_L, FACE_R);
        }
        ThreeDPrint => {
            p.rect((0.12, 0.08), (0.88, 0.18), Some(GREY), None);
            p.poly(&[(0.42, 0.18), (0.58, 0.18), (0.52, 0.34), (0.48, 0.34)], LIGHT, None);
            for i in 0..4 {
                let y = 0.5 + i as f32 * 0.1;
                let w = 0.16 + i as f32 * 0.04;
                p.rect((0.5 - w, y), (0.5 + w, y + 0.08), Some(if i == 0 { ORANGE } else { BLUE }), None);
            }
            p.seg((0.08, 0.94), (0.92, 0.94), GREY, 1.6);
        }
        NestPreparation => {
            p.rect((0.1, 0.12), (0.9, 0.88), None, Some((GREY, 1.2)));
            p.rect((0.18, 0.2), (0.48, 0.5), Some(BLUE), None);
            p.circle((0.68, 0.36), 0.14, Some(SURF), None);
            p.poly(&[(0.18, 0.8), (0.48, 0.8), (0.33, 0.58)], ORANGE, None);
            p.rect((0.56, 0.58), (0.82, 0.8), Some(FACE_R), None);
        }
        ScriptsAddIns => {
            p.rect((0.1, 0.16), (0.9, 0.84), Some(Color32::from_rgb(0x3a, 0x3e, 0x46)), Some((GREY, 1.0)));
            p.text((0.5, 0.5), "</>", 0.36, GREEN);
        }
        ManageMaterials => {
            p.circle((0.5, 0.5), 0.36, Some(FACE_R), Some((DARK, 0.8)));
            p.ellipse((0.5, 0.5), 0.36, 0.12, DARK, 1.0);
            p.circle((0.38, 0.36), 0.08, Some(LIGHT), None);
            p.circle((0.76, 0.76), 0.12, Some(ORANGE), Some((LIGHT, 0.8)));
        }
        _ => {
            // commands added to the registry later: recognise them by label first
            let label = crate::state::command::spec(id).label.to_lowercase();
            if label.contains("circular") {
                circular_pattern(p);
                return true;
            }
            // generic fallback: grey rounded square with a dot
            p.rect((0.2, 0.2), (0.8, 0.8), None, Some((GREY, 1.2)));
            p.dot((0.5, 0.5), GREY);
            return false;
        }
    }
    true
}

fn circular_pattern(p: &Pen) {
    p.circle((0.5, 0.5), 0.3, None, Some((GREY, 1.0)));
    p.circle((0.5, 0.5), 0.05, Some(ORANGE), None);
    for k in 0..6 {
        let a = (k as f32 * 60.0 - 90.0).to_radians();
        let c = (0.5 + 0.3 * a.cos(), 0.5 + 0.3 * a.sin());
        p.circle(c, 0.09, Some(if k == 0 { BLUE } else { FACE_T }), Some((DARK, 0.6)));
    }
}

fn page(p: &Pen) {
    p.poly(&[(0.2, 0.08), (0.62, 0.08), (0.8, 0.26), (0.8, 0.92), (0.2, 0.92)], Color32::from_rgb(0x55, 0x5a, 0x63), Some((LIGHT, 1.2)));
    p.line(&[(0.62, 0.08), (0.62, 0.26), (0.8, 0.26)], LIGHT, 1.0);
}

fn floppy(p: &Pen) {
    p.poly(&[(0.12, 0.12), (0.74, 0.12), (0.88, 0.26), (0.88, 0.88), (0.12, 0.88)], Color32::from_rgb(0x5d, 0x86, 0xc9), Some((LIGHT, 1.0)));
    p.rect((0.28, 0.12), (0.66, 0.38), Some(LIGHT), None);
    p.rect((0.26, 0.56), (0.74, 0.88), Some(Color32::from_rgb(0x2b, 0x2f, 0x36)), None);
}

/// Thin surface sheet (a skewed quad).
fn sheet(p: &Pen, c: P, w: f32, h: f32, col: Color32) {
    let sk = w * 0.22;
    let pts = [(c.0 - w * 0.5 + sk, c.1 - h * 0.5), (c.0 + w * 0.5, c.1 - h * 0.5), (c.0 + w * 0.5 - sk, c.1 + h * 0.5), (c.0 - w * 0.5, c.1 + h * 0.5)];
    p.poly(&pts, col.gamma_multiply(0.6), Some((col, 1.2)));
}

/// Flat triangle grid (n x n cells, each split into two triangles).
fn tri_grid(p: &Pen, n: usize, col: Color32) {
    let (x0, y0, s) = (0.12, 0.14, 0.76 / n as f32);
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (x0 + i as f32 * s, y0 + j as f32 * s);
            p.closed(&[(x, y), (x + s, y), (x + s, y + s), (x, y + s)], col, 1.0);
            p.seg((x, y + s), (x + s, y), col, 1.0);
        }
    }
}

fn mesh(p: &Pen, c: P, s: f32) {
    let k = p.cube(c, s, FACE_T, FACE_L, FACE_R);
    p.seg(k[0], k[6], DARK, 0.9);
    p.seg(k[5], k[1], DARK, 0.9);
    p.seg(k[5], k[3], DARK, 0.9);
    p.seg(k[6], k[2], DARK, 0.9);
    p.seg(k[6], k[4], DARK, 0.9);
}

fn house(p: &Pen, col: Color32) {
    p.poly(&[(0.5, 0.12), (0.9, 0.48), (0.1, 0.48)], col, None);
    p.rect((0.22, 0.46), (0.78, 0.88), Some(col), None);
    p.rect((0.42, 0.62), (0.58, 0.88), Some(theme::APPBAR_BG), None);
}

// ---------------------------------------------------------------------------------- chrome glyphs

fn draw_chrome(p: &Pen, icon: Icon) {
    match icon {
        Icon::Cmd(_) => {}
        Icon::Eye => p.eye(true),
        Icon::EyeOff => p.eye(false),
        Icon::Folder => {
            p.poly(&[(0.08, 0.22), (0.4, 0.22), (0.48, 0.32), (0.48, 0.4), (0.08, 0.4)], Color32::from_rgb(0x8b, 0x91, 0x9a), None);
            p.rect((0.08, 0.32), (0.92, 0.84), Some(Color32::from_rgb(0xa4, 0xaa, 0xb2)), None);
        }
        Icon::Gear => {
            for i in 0..8 {
                let a = (i as f32 * 45.0f32).to_radians();
                p.seg((0.5 + 0.22 * a.cos(), 0.5 + 0.22 * a.sin()), (0.5 + 0.4 * a.cos(), 0.5 + 0.4 * a.sin()), LIGHT, 3.6);
            }
            p.circle((0.5, 0.5), 0.28, Some(LIGHT), None);
            p.circle((0.5, 0.5), 0.12, Some(theme::TIMELINE_BG), None);
        }
        Icon::Lock => p.lock((0.5, 0.42), 0.6, RED),
        Icon::Home => house(p, LIGHT),
        Icon::DataPanel => {
            for i in 0..3 {
                for j in 0..3 {
                    let (x, y) = (0.14 + i as f32 * 0.27, 0.14 + j as f32 * 0.27);
                    p.rect((x, y), (x + 0.19, y + 0.19), Some(LIGHT), None);
                }
            }
        }
        Icon::Help => {
            p.circle((0.5, 0.5), 0.4, Some(LIGHT), None);
            p.text((0.5, 0.52), "?", 0.62, theme::APPBAR_BG);
        }
        Icon::FileMenu => page(p),
        Icon::Component => {
            p.cube((0.5, 0.52), 0.36, Color32::from_rgb(0xf2, 0xf3, 0xf5), Color32::from_rgb(0xb0, 0xb6, 0xbe), Color32::from_rgb(0xd0, 0xd4, 0xda));
        }
        Icon::Body => {
            p.cube((0.5, 0.52), 0.36, FACE_T, FACE_L, FACE_R);
        }
        Icon::Origin => {
            p.circle((0.5, 0.5), 0.2, None, Some((LIGHT, 1.4)));
            p.seg((0.5, 0.15), (0.5, 0.85), GREY, 1.0);
            p.seg((0.15, 0.5), (0.85, 0.5), GREY, 1.0);
        }
        Icon::Axis => {
            p.arrow((0.15, 0.85), (0.85, 0.15), ORANGE, 1.6);
        }
        Icon::Plane => p.plane((0.5, 0.5), 0.72, 0.5, ORANGE, ORANGE),
        Icon::Units => {
            p.rect((0.12, 0.3), (0.88, 0.7), Some(Color32::from_rgb(0x8b, 0x91, 0x9a)), None);
            for i in 1..6 {
                let x = 0.12 + i as f32 * 0.127;
                p.seg((x, 0.3), (x, if i % 2 == 0 { 0.52 } else { 0.44 }), theme::PANEL_BG, 1.2);
            }
        }
        Icon::Info => {
            p.circle((0.5, 0.5), 0.42, Some(theme::ACCENT), None);
            p.rect((0.45, 0.42), (0.55, 0.76), Some(Color32::WHITE), None);
            p.circle((0.5, 0.28), 0.06, Some(Color32::WHITE), None);
        }
        Icon::Warning => {
            p.poly(&[(0.5, 0.1), (0.92, 0.86), (0.08, 0.86)], theme::WARNING, None);
            p.rect((0.46, 0.36), (0.54, 0.64), Some(theme::APPBAR_BG), None);
            p.circle((0.5, 0.74), 0.045, Some(theme::APPBAR_BG), None);
        }
        Icon::Error => {
            p.circle((0.5, 0.5), 0.42, Some(theme::ERROR), None);
            p.seg((0.34, 0.34), (0.66, 0.66), Color32::WHITE, 2.2);
            p.seg((0.66, 0.34), (0.34, 0.66), Color32::WHITE, 2.2);
        }
        Icon::Log => {
            for i in 0..4 {
                let y = 0.22 + i as f32 * 0.19;
                p.dot((0.16, y), LIGHT);
                p.seg((0.3, y), (0.86, y), LIGHT, 1.6);
            }
        }
        Icon::ToStart => {
            p.rect((0.22, 0.24), (0.3, 0.76), Some(LIGHT), None);
            p.poly(&[(0.78, 0.24), (0.78, 0.76), (0.34, 0.5)], LIGHT, None);
        }
        Icon::StepBack => {
            p.poly(&[(0.7, 0.24), (0.7, 0.76), (0.3, 0.5)], LIGHT, None);
            p.rect((0.72, 0.24), (0.78, 0.76), None, None);
        }
        Icon::Play => p.poly(&[(0.3, 0.2), (0.78, 0.5), (0.3, 0.8)], LIGHT, None),
        Icon::StepForward => {
            p.poly(&[(0.3, 0.24), (0.7, 0.5), (0.3, 0.76)], LIGHT, None);
        }
        Icon::ToEnd => {
            p.poly(&[(0.22, 0.24), (0.66, 0.5), (0.22, 0.76)], LIGHT, None);
            p.rect((0.7, 0.24), (0.78, 0.76), Some(LIGHT), None);
        }
        Icon::Viewports => {
            for (x, y) in [(0.12, 0.16), (0.52, 0.16), (0.12, 0.52), (0.52, 0.52)] {
                p.rect((x, y), (x + 0.34, y + 0.3), Some(LIGHT), None);
            }
        }
        Icon::DisplaySettings => {
            p.rect((0.1, 0.16), (0.9, 0.7), Some(LIGHT), None);
            p.rect((0.16, 0.22), (0.84, 0.64), Some(Color32::from_rgb(0x4d, 0x7f, 0xc4)), None);
            p.rect((0.4, 0.7), (0.6, 0.82), Some(LIGHT), None);
            p.rect((0.28, 0.82), (0.72, 0.88), Some(LIGHT), None);
        }
        Icon::Select => {
            p.dashed((0.1, 0.12), (0.8, 0.12), GREEN, 1.4);
            p.dashed((0.1, 0.12), (0.1, 0.72), GREEN, 1.4);
            p.dashed((0.8, 0.12), (0.8, 0.4), GREEN, 1.4);
            p.dashed((0.1, 0.72), (0.4, 0.72), GREEN, 1.4);
            p.poly(&[(0.5, 0.42), (0.92, 0.7), (0.72, 0.74), (0.6, 0.94)], Color32::WHITE, Some((DARK, 0.8)));
        }
        Icon::CaretDown => p.poly(&[(0.2, 0.35), (0.8, 0.35), (0.5, 0.7)], LIGHT, None),
        Icon::CaretRight => p.poly(&[(0.35, 0.2), (0.7, 0.5), (0.35, 0.8)], LIGHT, None),
        Icon::Close => {
            p.seg((0.25, 0.25), (0.75, 0.75), LIGHT, 1.6);
            p.seg((0.75, 0.25), (0.25, 0.75), LIGHT, 1.6);
        }
        Icon::Workspace => {
            p.cube((0.5, 0.52), 0.36, BLUE, FACE_L, FACE_R);
        }
        Icon::CircularPattern => circular_pattern(p),
        Icon::Feature => {
            p.cube((0.5, 0.54), 0.34, FACE_T, FACE_L, FACE_R);
            p.circle((0.78, 0.22), 0.12, Some(BLUE), None);
        }
    }
}

// ---------------------------------------------------------------------------------- widgets

/// Small caret (▾) painted at `center` — the egui default fonts lack some arrow glyphs.
pub fn caret(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let s = size * 0.5;
    painter.add(Shape::convex_polygon(vec![center + vec2(-s, -s * 0.5), center + vec2(s, -s * 0.5), center + vec2(0.0, s * 0.6)], color, Stroke::NONE));
}

/// Hover / pressed / active background shared by all chrome buttons.
pub fn paint_button_bg(painter: &Painter, rect: Rect, resp: &Response, enabled: bool, active: bool) {
    let r = CornerRadius::same(theme::RADIUS);
    if active {
        painter.rect_filled(rect, r, theme::ACCENT_DIM.gamma_multiply(0.7));
        painter.rect_stroke(rect, r, Stroke::new(1.0, theme::ACCENT), StrokeKind::Inside);
    } else if resp.is_pointer_button_down_on() && enabled {
        painter.rect_filled(rect, r, theme::PRESSED_BG);
    } else if resp.hovered() {
        painter.rect_filled(rect, r, if enabled { theme::HOVER_BG } else { theme::HOVER_BG.gamma_multiply(0.5) });
    }
}

/// Width of the custom menu rows.
pub const MENU_WIDTH: f32 = 250.0;

/// A menu row: icon, label, right-aligned shortcut, optional check mark. Returns the response;
/// callers act on `clicked()` only when `enabled`.
pub fn menu_row(ui: &mut Ui, icon: Option<Icon>, label: &str, shortcut: Option<&str>, enabled: bool, checked: Option<bool>) -> Response {
    let h = 24.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(MENU_WIDTH, h), Sense::click());
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(theme::RADIUS), if enabled { theme::ACCENT_DIM } else { theme::HOVER_BG.gamma_multiply(0.6) });
    }
    let mut x = rect.left() + 6.0;
    if let Some(c) = checked {
        if c {
            let m = pos2(x + 6.0, rect.center().y);
            painter.add(Shape::line(vec![m + vec2(-4.0, 0.0), m + vec2(-1.0, 3.5), m + vec2(5.0, -4.0)], Stroke::new(1.6, if enabled { theme::ACCENT } else { theme::TEXT_DISABLED })));
        }
        x += 16.0;
    }
    if let Some(i) = icon {
        paint(painter, Rect::from_min_size(pos2(x, rect.center().y - 9.0), vec2(18.0, 18.0)), i, enabled);
        x += 24.0;
    }
    let col = if enabled { theme::TEXT } else { theme::TEXT_DISABLED };
    painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, theme::body_font(), col);
    if let Some(s) = shortcut {
        painter.text(pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, s, FontId::proportional(11.0), theme::TEXT_WEAK);
    }
    resp
}

/// Thin horizontal separator for custom menus.
pub fn menu_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(MENU_WIDTH, 7.0), Sense::hover());
    ui.painter().hline(rect.x_range().shrink(4.0), rect.center().y, Stroke::new(1.0, theme::SEPARATOR));
}

/// Small caption row inside a menu ("SKETCH", "VIEW" ...).
pub fn menu_caption(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(MENU_WIDTH, 18.0), Sense::hover());
    ui.painter().text(pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, text, theme::caption_font(), theme::TEXT_WEAK);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::command::COMMANDS;

    fn with_painter(f: impl FnOnce(&Painter)) {
        let ctx = egui::Context::default();
        let mut f = Some(f);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(f) = f.take() {
                f(ui.painter());
            }
        });
        out.textures_delta.clear();
    }

    #[test]
    fn every_registered_command_has_a_dedicated_icon() {
        with_painter(|painter| {
            let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(24.0, 24.0));
            for c in COMMANDS {
                let pen = Pen { p: painter, r: rect, dim: false, k: 1.0 };
                assert!(draw_command(&pen, c.id), "{:?} falls back to the generic icon", c.id);
            }
        });
    }

    #[test]
    fn chrome_icons_paint_enabled_and_disabled() {
        with_painter(|painter| {
            let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(16.0, 16.0));
            for i in [Icon::Eye, Icon::EyeOff, Icon::Folder, Icon::Gear, Icon::Lock, Icon::Home, Icon::DataPanel, Icon::Help, Icon::Play, Icon::Select, Icon::Warning] {
                paint(painter, rect, i, true);
                paint(painter, rect, i, false);
            }
        });
    }

    #[test]
    fn feature_icons_follow_type_labels() {
        assert_eq!(icon_for_type_label("Sketch"), Icon::Cmd(CommandId::CreateSketch));
        assert_eq!(icon_for_type_label("Plane"), Icon::Cmd(CommandId::OffsetPlane));
        assert_eq!(icon_for_type_label("Shell"), Icon::Cmd(CommandId::Shell));
        assert_eq!(icon_for_type_label("Circular Pattern"), Icon::CircularPattern);
        assert_eq!(icon_for_type_label("RectangularPattern"), Icon::Cmd(CommandId::RectangularPattern));
        assert_eq!(icon_for_type_label("MirrorBodies"), Icon::Cmd(CommandId::MirrorFeature));
        assert_eq!(icon_for_type_label("Move/Copy"), Icon::Cmd(CommandId::Move));
        assert_eq!(icon_for_type_label("Joint"), Icon::Cmd(CommandId::Joint));
        assert_eq!(icon_for_type_label("New Component"), Icon::Cmd(CommandId::NewComponent));
        assert_eq!(icon_for_type_label("Something New"), Icon::Feature);
    }

    #[test]
    fn disabled_colours_are_grey() {
        with_painter(|painter| {
            let pen = Pen { p: painter, r: Rect::ZERO, dim: true, k: 1.0 };
            let c = pen.c(BLUE);
            assert!((c.r() as i32 - c.g() as i32).abs() <= 6, "desaturated: {c:?}");
        });
    }
}

/// Headless input driver for interaction tests: runs frames on a plain `egui::Context` with
/// synthetic pointer events (no window, no GPU).
#[cfg(test)]
pub(crate) mod testkit {
    use egui::{Event, Modifiers, PointerButton, Pos2, Rect, Ui};

    pub struct Kit {
        pub ctx: egui::Context,
        pub time: f64,
    }

    impl Kit {
        pub fn new() -> Self {
            let ctx = egui::Context::default();
            crate::ui::theme::apply(&ctx);
            Kit { ctx, time: 0.0 }
        }
        pub fn frame(&mut self, events: Vec<Event>, f: &mut dyn FnMut(&mut Ui)) {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 960.0))), time: Some(self.time), events, ..Default::default() };
            let mut out = self.ctx.run_ui(input, |ui| f(ui));
            out.textures_delta.clear();
        }
        pub fn idle(&mut self, n: usize, f: &mut dyn FnMut(&mut Ui)) {
            for _ in 0..n {
                self.frame(vec![], f);
            }
        }
        fn button(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
            Event::PointerButton { pos, button, pressed, modifiers: Modifiers::default() }
        }
        pub fn click_with(&mut self, pos: Pos2, b: PointerButton, f: &mut dyn FnMut(&mut Ui)) {
            self.frame(vec![Event::PointerMoved(pos)], f);
            self.frame(vec![Self::button(pos, b, true)], f);
            self.frame(vec![Self::button(pos, b, false)], f);
            self.frame(vec![], f);
        }
        pub fn click(&mut self, pos: Pos2, f: &mut dyn FnMut(&mut Ui)) {
            self.click_with(pos, PointerButton::Primary, f);
        }
        pub fn drag(&mut self, from: Pos2, to: Pos2, f: &mut dyn FnMut(&mut Ui)) {
            self.frame(vec![Event::PointerMoved(from)], f);
            self.frame(vec![Self::button(from, PointerButton::Primary, true)], f);
            for k in 1..=8 {
                let p = from + (to - from) * (k as f32 / 8.0);
                self.frame(vec![Event::PointerMoved(p)], f);
            }
            self.frame(vec![Self::button(to, PointerButton::Primary, false)], f);
            self.frame(vec![], f);
        }
    }
}
