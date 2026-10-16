//! Dialogs of the body-level modelling features: Shell, Combine, Mirror (bodies), Rectangular
//! Pattern, Circular Pattern and Move/Copy. Same session model as the other feature dialogs
//! (see features.rs): live preview inside a transaction, OK = one undo step, Cancel restores.
//!
//! Body inputs are body ids of the *base* model (the model before the feature). A canvas click
//! on a face or edge selects its body (Fusion's body selection filter); the dialog mirrors its
//! body inputs into `state.selection` as `Pick::Body`, which the canvas highlights.

use super::{apply_expr, param_expr, Dialog, ExprField, Route, Session};
use crate::state::{AppState, Pick};
use cad_core::{ops, BodyId, FaceRef, FeatureKind, Operation, OriginAxis, PlaneRef, SurfaceKind};
use cad_kernel::{refs, Model};

#[derive(Clone, Debug)]
pub struct ShellDialog {
    pub s: Session,
    /// Faces to remove.
    pub faces: Vec<FaceRef>,
    /// Inside thickness.
    pub thickness: ExprField,
}

/// Which selection input of a two-input dialog receives canvas clicks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyInput {
    /// Combine: target body / Mirror: objects.
    First,
    /// Combine: tool bodies / Mirror: mirror plane.
    Second,
}

#[derive(Clone, Debug)]
pub struct CombineDialog {
    pub s: Session,
    pub target: Option<BodyId>,
    pub tools: Vec<BodyId>,
    /// Join, Cut or Intersect.
    pub operation: Operation,
    pub keep_tools: bool,
    pub active: BodyInput,
}

#[derive(Clone, Debug)]
pub struct MirrorDialog {
    pub s: Session,
    pub bodies: Vec<BodyId>,
    pub plane: Option<PlaneRef>,
    pub active: BodyInput,
}

#[derive(Clone, Debug)]
pub struct RectPatternDialog {
    pub s: Session,
    pub bodies: Vec<BodyId>,
    pub axis1: OriginAxis,
    pub quantity1: ExprField,
    pub distance1: ExprField,
    /// Second direction (None = one direction).
    pub axis2: Option<OriginAxis>,
    pub quantity2: ExprField,
    pub distance2: ExprField,
    pub join: bool,
}

#[derive(Clone, Debug)]
pub struct CircPatternDialog {
    pub s: Session,
    pub bodies: Vec<BodyId>,
    pub axis: OriginAxis,
    pub quantity: ExprField,
    /// Total angle.
    pub angle: ExprField,
    pub join: bool,
}

#[derive(Clone, Debug)]
pub struct MoveDialog {
    pub s: Session,
    pub bodies: Vec<BodyId>,
    pub dx: ExprField,
    pub dy: ExprField,
    pub dz: ExprField,
    pub axis: OriginAxis,
    pub angle: ExprField,
    /// Create Copy.
    pub copy: bool,
}

/// Fusion's Combine operations (no "New Body").
pub const COMBINE_OPERATIONS: [Operation; 3] = [Operation::Join, Operation::Cut, Operation::Intersect];

const DEFAULT_SPACING: &str = "10 mm";

/// (picks, mode) with mode 0 toggle, 1 add, 2 remove, 3 nothing.
fn route_picks(route: Route) -> (Vec<Pick>, u8) {
    match route {
        Route::Toggle(p) => (vec![p], 0),
        Route::Add(v) => (v, 1),
        Route::Remove(v) => (v, 2),
        Route::Nothing => (Vec::new(), 3),
    }
}

pub(crate) fn is_body_pick(p: &Pick) -> bool {
    matches!(p, Pick::Body { .. } | Pick::Face { .. } | Pick::Edge { .. })
}

/// Toggles / adds / removes `id` in `list` per `mode`. Returns whether the list changed.
fn edit_list(list: &mut Vec<BodyId>, id: BodyId, mode: u8) -> bool {
    let at = list.iter().position(|b| *b == id);
    match (at, mode) {
        (Some(i), 0 | 2) => {
            list.remove(i);
            true
        }
        (None, 0 | 1) => {
            list.push(id);
            true
        }
        _ => false,
    }
}

fn bodies_text(n: usize) -> String {
    format!("{n} {}", if n == 1 { "Body" } else { "Bodies" })
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

impl AppState {
    // ------------------------------------------------------------------ helpers
    /// Body of the base model a canvas / browser pick designates.
    fn pick_body(base: &Model, p: &Pick) -> Option<BodyId> {
        let id = match p {
            Pick::Body { body } | Pick::Face { body, .. } | Pick::Edge { body, .. } => body,
            _ => return None,
        };
        base.body(id).map(|_| id.clone())
    }

    /// Applies picks to a body-list input.
    fn route_body_list(s: &mut Session, list: &mut Vec<BodyId>, picks: &[Pick], mode: u8) -> bool {
        let mut changed = false;
        for p in picks.iter().filter(|p| is_body_pick(p)) {
            match Self::pick_body(&s.base.0, p) {
                Some(id) => changed |= edit_list(list, id, mode),
                None if mode != 2 => s.notice = Some("Select an existing body (not a preview copy).".into()),
                None => {}
            }
        }
        if changed {
            s.notice = None;
        }
        changed
    }

    /// Bodies the current selection designates (for pre-selection when a command starts).
    fn initial_body_picks(&self) -> Vec<Pick> {
        self.selection.iter().filter(|p| is_body_pick(p)).cloned().collect()
    }

    /// `Pick::Body` for the bodies that exist in the displayed model.
    fn body_picks(&self, ids: &[BodyId]) -> Vec<Pick> {
        ids.iter().filter(|b| self.model.body(b).is_some()).map(|b| Pick::Body { body: b.clone() }).collect()
    }

    /// Writes an expression field into the n-th parameter of the feature.
    fn apply_param(&mut self, fid: &str, n: usize, field: &mut ExprField) {
        let Some(name) = self.doc.feature(fid).and_then(|f| f.parameter_names().get(n).map(|s| s.to_string())) else { return };
        apply_expr(&mut self.doc, &name, field);
    }

    fn kind_mut(&mut self, fid: &str) -> Option<&mut FeatureKind> {
        self.doc.feature_mut(fid).map(|f| &mut f.kind)
    }

    fn field_of(&self, fid: &str, n: usize) -> ExprField {
        let name = self.doc.feature(fid).and_then(|f| f.parameter_names().get(n).map(|s| s.to_string())).unwrap_or_default();
        ExprField::new(&param_expr(&self.doc, &name))
    }

    // ------------------------------------------------------------------ opening
    pub(crate) fn open_shell(&mut self, editing: Option<&str>) {
        let initial: Vec<Pick> = self.selection.iter().filter(|p| matches!(p, Pick::Face { .. })).cloned().collect();
        let s = self.open_session("Shell", editing);
        let mut d = ShellDialog { s, faces: Vec::new(), thickness: ExprField::new("2 mm") };
        match editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            Some(FeatureKind::Shell { faces, thickness }) => {
                d.faces = faces;
                d.thickness = ExprField::new(&param_expr(&self.doc, &thickness));
            }
            _ => {
                self.route_shell(&mut d, Route::Add(initial));
            }
        }
        self.dialog = Some(Dialog::Shell(d));
        self.dialog_changed();
    }

    pub(crate) fn open_combine(&mut self, editing: Option<&str>) {
        let initial = self.initial_body_picks();
        let s = self.open_session("Combine", editing);
        let mut d = CombineDialog { s, target: None, tools: Vec::new(), operation: Operation::Join, keep_tools: false, active: BodyInput::First };
        match editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            Some(FeatureKind::Combine { target, tools, operation, keep_tools }) => {
                d.target = Some(target);
                d.tools = tools;
                d.operation = operation;
                d.keep_tools = keep_tools;
                d.active = BodyInput::Second;
            }
            _ => {
                self.route_combine(&mut d, Route::Add(initial));
            }
        }
        self.dialog = Some(Dialog::Combine(d));
        self.dialog_changed();
    }

    pub(crate) fn open_mirror(&mut self, editing: Option<&str>) {
        let initial: Vec<Pick> = self.selection.iter().filter(|p| is_body_pick(p) || matches!(p, Pick::OriginPlane(_) | Pick::ConstructionPlane(_))).cloned().collect();
        let s = self.open_session("Mirror", editing);
        let mut d = MirrorDialog { s, bodies: Vec::new(), plane: None, active: BodyInput::First };
        match editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            Some(FeatureKind::MirrorBodies { bodies, plane }) => {
                d.bodies = bodies;
                d.plane = Some(plane);
            }
            _ => {
                self.route_mirror(&mut d, Route::Add(initial));
            }
        }
        self.dialog = Some(Dialog::Mirror(d));
        self.dialog_changed();
    }

    pub(crate) fn open_rect_pattern(&mut self, editing: Option<&str>) {
        let initial = self.initial_body_picks();
        let s = self.open_session("Rectangular Pattern", editing);
        let mut d = RectPatternDialog {
            s,
            bodies: Vec::new(),
            axis1: OriginAxis::X,
            quantity1: ExprField::new("3"),
            distance1: ExprField::new(DEFAULT_SPACING),
            axis2: None,
            quantity2: ExprField::new("2"),
            distance2: ExprField::new(DEFAULT_SPACING),
            join: false,
        };
        match editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            Some(FeatureKind::RectangularPattern { bodies, axis1, axis2, join, .. }) => {
                let id = editing.unwrap_or_default();
                d.bodies = bodies;
                d.axis1 = axis1;
                d.axis2 = axis2;
                d.join = join;
                d.quantity1 = self.field_of(id, 0);
                d.distance1 = self.field_of(id, 1);
                d.quantity2 = self.field_of(id, 2);
                d.distance2 = self.field_of(id, 3);
            }
            _ => {
                let (picks, _) = route_picks(Route::Add(initial));
                Self::route_body_list(&mut d.s, &mut d.bodies, &picks, 1);
            }
        }
        self.dialog = Some(Dialog::RectangularPattern(d));
        self.dialog_changed();
    }

    pub(crate) fn open_circ_pattern(&mut self, editing: Option<&str>) {
        let initial = self.initial_body_picks();
        let s = self.open_session("Circular Pattern", editing);
        let mut d = CircPatternDialog { s, bodies: Vec::new(), axis: OriginAxis::Z, quantity: ExprField::new("3"), angle: ExprField::new("360 deg"), join: false };
        match editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            Some(FeatureKind::CircularPattern { bodies, axis, join, .. }) => {
                let id = editing.unwrap_or_default();
                d.bodies = bodies;
                d.axis = axis;
                d.join = join;
                d.quantity = self.field_of(id, 0);
                d.angle = self.field_of(id, 1);
            }
            _ => {
                Self::route_body_list(&mut d.s, &mut d.bodies, &initial, 1);
            }
        }
        self.dialog = Some(Dialog::CircularPattern(d));
        self.dialog_changed();
    }

    pub(crate) fn open_move(&mut self, editing: Option<&str>) {
        let initial = self.initial_body_picks();
        let s = self.open_session("Move/Copy", editing);
        let mut d = MoveDialog {
            s,
            bodies: Vec::new(),
            dx: ExprField::new("0 mm"),
            dy: ExprField::new("0 mm"),
            dz: ExprField::new("0 mm"),
            axis: OriginAxis::Z,
            angle: ExprField::new("0 deg"),
            copy: false,
        };
        match editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            Some(FeatureKind::MoveBodies { bodies, axis, copy, .. }) => {
                let id = editing.unwrap_or_default();
                d.bodies = bodies;
                d.axis = axis;
                d.copy = copy;
                d.dx = self.field_of(id, 0);
                d.dy = self.field_of(id, 1);
                d.dz = self.field_of(id, 2);
                d.angle = self.field_of(id, 3);
            }
            _ => {
                Self::route_body_list(&mut d.s, &mut d.bodies, &initial, 1);
            }
        }
        self.dialog = Some(Dialog::Move(d));
        self.dialog_changed();
    }

    // ------------------------------------------------------------------ routing
    /// Selection-change routing for the dialogs of this module (`None`: not one of them).
    pub(crate) fn route_more(&self, d: &mut Dialog) -> Option<bool> {
        Some(match d {
            Dialog::Shell(x) => {
                let r = self.route(&x.s.synced, |p| matches!(p, Pick::Face { .. }));
                self.route_shell(x, r)
            }
            Dialog::Combine(x) => {
                let r = self.route(&x.s.synced, is_body_pick);
                self.route_combine(x, r)
            }
            Dialog::Mirror(x) => {
                let r = self.route(&x.s.synced, |p| is_body_pick(p) || matches!(p, Pick::OriginPlane(_) | Pick::ConstructionPlane(_)));
                self.route_mirror(x, r)
            }
            Dialog::RectangularPattern(x) => {
                let (picks, mode) = route_picks(self.route(&x.s.synced, is_body_pick));
                Self::route_body_list(&mut x.s, &mut x.bodies, &picks, mode)
            }
            Dialog::CircularPattern(x) => {
                let (picks, mode) = route_picks(self.route(&x.s.synced, is_body_pick));
                Self::route_body_list(&mut x.s, &mut x.bodies, &picks, mode)
            }
            Dialog::Move(x) => {
                let (picks, mode) = route_picks(self.route(&x.s.synced, is_body_pick));
                Self::route_body_list(&mut x.s, &mut x.bodies, &picks, mode)
            }
            _ => return None,
        })
    }

    pub(crate) fn route_shell(&self, x: &mut ShellDialog, route: Route) -> bool {
        let (picks, mode) = route_picks(route);
        let base = x.s.base.0.clone();
        let mut changed = false;
        for p in picks {
            let Pick::Face { body, face } = &p else { continue };
            let Some((b, j)) = self.base_face(&base, body, *face) else {
                if mode != 2 {
                    x.s.notice = Some("Select faces of an existing body (not of the preview).".into());
                }
                continue;
            };
            if b.topology().face_info[j].surface != SurfaceKind::Plane {
                if mode != 2 {
                    x.s.notice = Some("Select planar faces to remove.".into());
                }
                continue;
            }
            let at = x.faces.iter().position(|r| r.body == b.id && refs::resolve_face(&b, r).ok() == Some(j));
            match (at, mode) {
                (Some(i), 0 | 2) => {
                    x.faces.remove(i);
                    changed = true;
                }
                (None, 0 | 1) => {
                    if let Some(r) = refs::face_ref(&b, j) {
                        x.faces.push(r);
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        if changed {
            x.s.notice = None;
        }
        changed
    }

    pub(crate) fn route_combine(&self, x: &mut CombineDialog, route: Route) -> bool {
        let (picks, mode) = route_picks(route);
        let mut changed = false;
        for p in picks.iter().filter(|p| is_body_pick(p)) {
            let Some(id) = Self::pick_body(&x.s.base.0, p) else {
                if mode != 2 {
                    x.s.notice = Some("Select an existing body (not a preview copy).".into());
                }
                continue;
            };
            if mode == 2 {
                if x.target.as_ref() == Some(&id) {
                    x.target = None;
                    changed = true;
                }
                changed |= edit_list(&mut x.tools, id, 2);
                continue;
            }
            if x.active == BodyInput::First || x.target.is_none() {
                if x.target.as_ref() == Some(&id) {
                    if mode == 0 {
                        x.target = None;
                        changed = true;
                    }
                    continue;
                }
                x.tools.retain(|t| *t != id);
                x.target = Some(id);
                // Fusion advances to the Tool Bodies input once the target is chosen
                x.active = BodyInput::Second;
                changed = true;
            } else if x.target.as_ref() == Some(&id) {
                x.s.notice = Some("The target body cannot also be a tool body.".into());
            } else {
                changed |= edit_list(&mut x.tools, id, mode);
            }
        }
        if changed {
            x.s.notice = None;
        }
        changed
    }

    pub(crate) fn route_mirror(&self, x: &mut MirrorDialog, route: Route) -> bool {
        let (picks, mode) = route_picks(route);
        let base = x.s.base.0.clone();
        let mut changed = false;
        for p in &picks {
            let plane = match p {
                Pick::OriginPlane(plane) => Some(PlaneRef::Origin { plane: *plane }),
                Pick::ConstructionPlane(id) if base.planes.contains_key(id) => Some(PlaneRef::Construction { feature: id.clone() }),
                Pick::Face { body, face } if x.active == BodyInput::Second => match self.base_face(&base, body, *face) {
                    Some((b, j)) if b.topology().face_info[j].surface == SurfaceKind::Plane => refs::face_ref(&b, j).map(|face| PlaneRef::Face { face }),
                    _ => {
                        if mode != 2 {
                            x.s.notice = Some("Select a planar face or a plane as the mirror plane.".into());
                        }
                        continue;
                    }
                },
                _ => None,
            };
            match plane {
                Some(r) if mode == 2 => {
                    if x.plane.as_ref() == Some(&r) {
                        x.plane = None;
                        changed = true;
                    }
                }
                Some(r) => {
                    if x.plane.as_ref() != Some(&r) {
                        x.plane = Some(r);
                        changed = true;
                    }
                }
                None if is_body_pick(p) => {
                    // a removed face pick may be the selected mirror face
                    if mode == 2 {
                        if let (Pick::Face { .. }, Some(PlaneRef::Face { face })) = (p, &x.plane) {
                            if self.face_pick(&base, face).as_ref() == Some(p) {
                                x.plane = None;
                                changed = true;
                                continue;
                            }
                        }
                    }
                    changed |= Self::route_body_list(&mut x.s, &mut x.bodies, std::slice::from_ref(p), mode);
                    // Fusion advances to the Mirror Plane input after the first object
                    if changed && mode != 2 && x.plane.is_none() && x.active == BodyInput::First && !x.bodies.is_empty() {
                        x.active = BodyInput::Second;
                    }
                }
                None => {}
            }
        }
        if changed {
            x.s.notice = None;
        }
        changed
    }

    /// Selection mirrored into `state.selection` for the dialogs of this module.
    pub(crate) fn more_picks(&self, d: &Dialog) -> Option<Vec<Pick>> {
        Some(match d {
            Dialog::Shell(x) => x.faces.iter().filter_map(|r| self.face_pick(&x.s.base.0, r)).collect(),
            Dialog::Combine(x) => {
                let mut v: Vec<BodyId> = x.target.iter().cloned().collect();
                v.extend(x.tools.iter().cloned());
                self.body_picks(&v)
            }
            Dialog::Mirror(x) => {
                let mut v = self.body_picks(&x.bodies);
                v.extend(x.plane.iter().filter_map(|p| self.plane_pick(&x.s.base.0, p)));
                v
            }
            Dialog::RectangularPattern(x) => self.body_picks(&x.bodies),
            Dialog::CircularPattern(x) => self.body_picks(&x.bodies),
            Dialog::Move(x) => self.body_picks(&x.bodies),
            _ => return None,
        })
    }

    // ------------------------------------------------------------------ updates
    /// Writes the dialog payload of this module's dialogs into the document and recomputes.
    pub(crate) fn update_more(&mut self, d: &mut Dialog) {
        match d {
            Dialog::Shell(x) => self.update_shell(x),
            Dialog::Combine(x) => self.update_combine(x),
            Dialog::Mirror(x) => self.update_mirror(x),
            Dialog::RectangularPattern(x) => self.update_rect_pattern(x),
            Dialog::CircularPattern(x) => self.update_circ_pattern(x),
            Dialog::Move(x) => self.update_move(x),
            _ => {}
        }
    }

    fn update_shell(&mut self, x: &mut ShellDialog) {
        if x.s.feature.is_none() {
            if x.faces.is_empty() {
                return;
            }
            x.s.feature = Some(ops::new_shell(&mut self.doc, x.faces.clone(), "2 mm"));
        }
        let fid = x.s.feature.clone().unwrap_or_default();
        self.apply_param(&fid, 0, &mut x.thickness);
        if let Some(FeatureKind::Shell { faces, .. }) = self.kind_mut(&fid) {
            *faces = x.faces.clone();
        }
        self.recompute();
    }

    fn update_combine(&mut self, x: &mut CombineDialog) {
        if x.s.feature.is_none() {
            let Some(t) = x.target.clone() else { return };
            if x.tools.is_empty() {
                return;
            }
            x.s.feature = Some(ops::new_combine(&mut self.doc, t, x.tools.clone(), x.operation, x.keep_tools));
        }
        let fid = x.s.feature.clone().unwrap_or_default();
        if let Some(FeatureKind::Combine { target, tools, operation, keep_tools }) = self.kind_mut(&fid) {
            *target = x.target.clone().unwrap_or_default();
            *tools = x.tools.clone();
            *operation = x.operation;
            *keep_tools = x.keep_tools;
        }
        self.recompute();
    }

    fn update_mirror(&mut self, x: &mut MirrorDialog) {
        if x.s.feature.is_none() {
            let Some(p) = x.plane.clone() else { return };
            if x.bodies.is_empty() {
                return;
            }
            x.s.feature = Some(ops::new_mirror(&mut self.doc, x.bodies.clone(), p));
        }
        let fid = x.s.feature.clone().unwrap_or_default();
        if let Some(FeatureKind::MirrorBodies { bodies, plane }) = self.kind_mut(&fid) {
            *bodies = x.bodies.clone();
            if let Some(p) = &x.plane {
                *plane = p.clone();
            }
        }
        self.recompute();
    }

    /// Default spacing along `axis`: the extent of the bodies plus 10 mm, so instances do not
    /// overlap (whole millimetres).
    fn default_spacing(base: &Model, bodies: &[BodyId], axis: OriginAxis) -> String {
        let k = match axis {
            OriginAxis::X => 0,
            OriginAxis::Y => 1,
            OriginAxis::Z => 2,
        };
        let ext = bodies.iter().filter_map(|b| base.body(b)).map(|b| b.topology().bbox).fold(None::<(f64, f64)>, |acc, bb| match acc {
            None => Some((bb[k], bb[k + 3])),
            Some((lo, hi)) => Some((lo.min(bb[k]), hi.max(bb[k + 3]))),
        });
        match ext {
            // (bounding boxes carry a ~1e-7 tolerance gap: round it away before ceil)
            Some((lo, hi)) => format!("{} mm", ((((hi - lo) + 10.0) * 1e4).round() / 1e4).ceil()),
            None => DEFAULT_SPACING.into(),
        }
    }

    fn update_rect_pattern(&mut self, x: &mut RectPatternDialog) {
        if x.s.feature.is_none() {
            if x.bodies.is_empty() {
                return;
            }
            if x.distance1.text == DEFAULT_SPACING {
                x.distance1.text = Self::default_spacing(&x.s.base.0, &x.bodies, x.axis1);
            }
            if x.distance2.text == DEFAULT_SPACING {
                x.distance2.text = Self::default_spacing(&x.s.base.0, &x.bodies, x.axis2.unwrap_or(if x.axis1 == OriginAxis::Y { OriginAxis::X } else { OriginAxis::Y }));
            }
            x.s.feature = Some(ops::new_rectangular_pattern(&mut self.doc, x.bodies.clone(), x.axis1, "3", DEFAULT_SPACING, None, x.join));
        }
        let fid = x.s.feature.clone().unwrap_or_default();
        self.apply_param(&fid, 0, &mut x.quantity1);
        self.apply_param(&fid, 1, &mut x.distance1);
        self.apply_param(&fid, 2, &mut x.quantity2);
        self.apply_param(&fid, 3, &mut x.distance2);
        if let Some(FeatureKind::RectangularPattern { bodies, axis1, axis2, join, .. }) = self.kind_mut(&fid) {
            *bodies = x.bodies.clone();
            *axis1 = x.axis1;
            *axis2 = x.axis2;
            *join = x.join;
        }
        self.recompute();
    }

    fn update_circ_pattern(&mut self, x: &mut CircPatternDialog) {
        if x.s.feature.is_none() {
            if x.bodies.is_empty() {
                return;
            }
            x.s.feature = Some(ops::new_circular_pattern(&mut self.doc, x.bodies.clone(), x.axis, "3", "360 deg", x.join));
        }
        let fid = x.s.feature.clone().unwrap_or_default();
        self.apply_param(&fid, 0, &mut x.quantity);
        self.apply_param(&fid, 1, &mut x.angle);
        if let Some(FeatureKind::CircularPattern { bodies, axis, join, .. }) = self.kind_mut(&fid) {
            *bodies = x.bodies.clone();
            *axis = x.axis;
            *join = x.join;
        }
        self.recompute();
    }

    fn update_move(&mut self, x: &mut MoveDialog) {
        if x.s.feature.is_none() {
            if x.bodies.is_empty() {
                return;
            }
            x.s.feature = Some(ops::new_move(&mut self.doc, x.bodies.clone(), "0 mm", "0 mm", "0 mm", x.axis, "0 deg", x.copy));
        }
        let fid = x.s.feature.clone().unwrap_or_default();
        self.apply_param(&fid, 0, &mut x.dx);
        self.apply_param(&fid, 1, &mut x.dy);
        self.apply_param(&fid, 2, &mut x.dz);
        self.apply_param(&fid, 3, &mut x.angle);
        if let Some(FeatureKind::MoveBodies { bodies, axis, copy, .. }) = self.kind_mut(&fid) {
            *bodies = x.bodies.clone();
            *axis = x.axis;
            *copy = x.copy;
        }
        self.recompute();
    }

    // ------------------------------------------------------------------ status / inputs
    pub(crate) fn more_status(&self, d: &Dialog) -> Option<String> {
        match d {
            Dialog::Shell(x) if !x.faces.is_empty() => Some(plural(x.faces.len(), "Face")),
            Dialog::Combine(x) if x.target.is_some() => Some(format!("Target + {} Tool {}", x.tools.len(), if x.tools.len() == 1 { "Body" } else { "Bodies" })),
            Dialog::Mirror(x) if !x.bodies.is_empty() => Some(bodies_text(x.bodies.len())),
            Dialog::RectangularPattern(x) if !x.bodies.is_empty() => Some(bodies_text(x.bodies.len())),
            Dialog::CircularPattern(x) if !x.bodies.is_empty() => Some(bodies_text(x.bodies.len())),
            Dialog::Move(x) if !x.bodies.is_empty() => Some(bodies_text(x.bodies.len())),
            _ => None,
        }
    }

    /// Clears the active selection input (the "x" of the selection row).
    pub(crate) fn clear_more_inputs(d: &mut Dialog) {
        match d {
            Dialog::Shell(x) => x.faces.clear(),
            Dialog::Combine(x) => match x.active {
                BodyInput::First => x.target = None,
                BodyInput::Second => x.tools.clear(),
            },
            Dialog::Mirror(x) => match x.active {
                BodyInput::First => x.bodies.clear(),
                BodyInput::Second => x.plane = None,
            },
            Dialog::RectangularPattern(x) => x.bodies.clear(),
            Dialog::CircularPattern(x) => x.bodies.clear(),
            Dialog::Move(x) => x.bodies.clear(),
            _ => {}
        }
    }
}
