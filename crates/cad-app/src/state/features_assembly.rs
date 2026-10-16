//! ASSEMBLE: components (New Component, activate, rename, show/hide, delete) and the Joint
//! dialog.
//!
//! Components follow Fusion's "Hybrid Design" model, reduced to one level: the root component
//! is implicit (`None`), components are listed in `doc.components`, and every feature records the
//! component that was active when it was created. Bodies belong to the component of the feature
//! that created them. "New Component" with bodies selected moves those bodies into the new
//! component by reassigning their creating features (all bodies of that feature move along).
//!
//! The Joint dialog uses the same session model as the other feature dialogs (live preview in a
//! transaction, OK = one undo step, Cancel restores, `edit_feature` reopens it). Its two
//! selection inputs take a planar face or a circular edge each. Which component moves is decided
//! by the kernel's assembly solve, not by pick order (Fusion F36): root / grounded components
//! never move, otherwise the earlier-created one stays; the status line names the moving one. A
//! joint that over-constrains the assembly (a second joint between already connected
//! components, F37) gets Fusion's error, which blocks OK. Until both inputs are set the joint
//! feature stays suppressed, so picks are never made on a moved preview.

use super::{apply_expr, param_expr, BodyInput, Dialog, ExprField, Route, Session};
use crate::state::{AppState, Pick};
use cad_core::{ops, ComponentId, CurveKind, FeatureKind, JointGeometry, JointKind, JointOrigin, SurfaceKind};
use cad_kernel::{refs, Model};

#[derive(Clone, Debug)]
pub struct JointDialog {
    pub s: Session,
    /// Joint origin 1 (Component 1 geometry).
    pub a: Option<JointOrigin>,
    /// Joint origin 2 (Component 2 geometry).
    pub b: Option<JointOrigin>,
    pub kind: JointKind,
    /// Rotation about the joint axis.
    pub angle: ExprField,
    /// Translation along the joint axis.
    pub offset: ExprField,
    pub flip: bool,
    /// Which input receives canvas clicks.
    pub active: BodyInput,
    /// The edited joint was suppressed by the user (kept suppressed on OK).
    pub user_suppressed: bool,
}

fn route_picks(route: Route) -> (Vec<Pick>, u8) {
    match route {
        Route::Toggle(p) => (vec![p], 0),
        Route::Add(v) => (v, 1),
        Route::Remove(v) => (v, 2),
        Route::Nothing => (Vec::new(), 3),
    }
}

pub(crate) fn is_joint_pick(p: &Pick) -> bool {
    matches!(p, Pick::Face { .. } | Pick::Edge { .. })
}

impl AppState {
    // ------------------------------------------------------------------ components
    /// Component a body of the displayed model belongs to (`None` = root).
    #[allow(dead_code)]
    pub fn component_of_body(&self, body: &str) -> Option<ComponentId> {
        self.model.component_of(&self.doc, body).map(String::from)
    }

    /// ASSEMBLE > New Component: creates "ComponentN" and activates it. Bodies in the current
    /// selection are moved into it ("create from selected bodies").
    pub(crate) fn new_component_command(&mut self) {
        let mut bodies: Vec<String> = Vec::new();
        for p in &self.selection {
            if let Pick::Body { body } | Pick::Face { body, .. } | Pick::Edge { body, .. } = p {
                if self.model.body(body).is_some() && !bodies.contains(body) {
                    bodies.push(body.clone());
                }
            }
        }
        self.new_component(&bodies);
    }

    /// Creates a component (moving `bodies` into it) and activates it; one undo step.
    pub fn new_component(&mut self, bodies: &[String]) -> Option<ComponentId> {
        // move bodies by their creating feature (the kernel's `created_by`)
        let owners: Vec<String> = bodies.iter().filter_map(|b| self.model.body(b).map(|x| x.created_by.clone())).collect();
        let mut id = None;
        let ok = self.apply("New Component", |d| {
            id = Some(ops::new_component(d, &owners));
            Ok(())
        });
        let id = id.filter(|_| ok)?;
        self.selection.clear();
        let name = self.doc.component_name(Some(&id));
        if bodies.is_empty() {
            self.info(format!("Created {name} (active)"));
        } else {
            self.info(format!("Created {name} from {} bod{} (active)", bodies.len(), if bodies.len() == 1 { "y" } else { "ies" }));
        }
        Some(id)
    }

    /// Activates a component (`None` = root): new features are created in it (Fusion's
    /// activation radio button).
    pub fn activate_component(&mut self, id: Option<&str>) {
        if self.doc.active_component.as_deref() == id {
            return;
        }
        if let Some(c) = id {
            if self.doc.component(c).is_none() {
                self.error(format!("Unknown component \"{c}\""));
                return;
            }
        }
        let id = id.map(String::from);
        if self.apply("Activate Component", |d| {
            d.active_component = id;
            Ok(())
        }) {
            let name = self.doc.component_name(self.doc.active_component.as_deref());
            self.info(format!("Activated {name}"));
        }
    }

    pub fn rename_component(&mut self, id: &str, name: &str) {
        let (id, name) = (id.to_string(), name.trim().to_string());
        if name.is_empty() || self.doc.component(&id).is_some_and(|c| c.name == name) {
            return;
        }
        self.apply("Rename Component", |d| {
            d.component_mut(&id).ok_or_else(|| format!("Unknown component \"{id}\""))?.name = name;
            Ok(())
        });
    }

    /// Shows or hides a component (all of its bodies).
    pub fn set_component_visible(&mut self, id: &str, visible: bool) {
        let id = id.to_string();
        self.apply("Toggle Visibility", |d| {
            d.component_mut(&id).ok_or_else(|| format!("Unknown component \"{id}\""))?.visible = visible;
            Ok(())
        });
    }

    /// Deletes a component that owns no features (otherwise refused with a message).
    pub fn delete_component(&mut self, id: &str) -> bool {
        let id = id.to_string();
        let name = self.doc.component_name(Some(&id));
        let ok = self.apply(&format!("Delete {name}"), |d| d.delete_component(&id).map_err(|e| e.to_string()));
        if ok {
            self.info(format!("Deleted {name}"));
        }
        ok
    }

    // ------------------------------------------------------------------ joint dialog
    pub(crate) fn open_joint(&mut self, editing: Option<&str>) {
        let initial: Vec<Pick> = self.selection.iter().filter(|p| is_joint_pick(p)).cloned().collect();
        let s = self.open_session("Joint", editing);
        let mut d = JointDialog { s, a: None, b: None, kind: JointKind::Rigid, angle: ExprField::new("0 deg"), offset: ExprField::new("0 mm"), flip: false, active: BodyInput::First, user_suppressed: false };
        match editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            Some(FeatureKind::Joint { a, b, kind, angle, offset, flip }) => {
                d.a = Some(a);
                d.b = Some(b);
                d.kind = kind;
                d.flip = flip;
                d.angle = ExprField::new(&param_expr(&self.doc, &angle));
                d.offset = ExprField::new(&param_expr(&self.doc, &offset));
                d.active = BodyInput::Second;
                d.user_suppressed = editing.and_then(|id| self.doc.feature(id)).is_some_and(|f| f.suppressed);
            }
            _ => {
                self.route_joint(&mut d, Route::Add(initial));
            }
        }
        self.dialog = Some(Dialog::Joint(d));
        self.dialog_changed();
    }

    /// Joint origin of a displayed-model pick, resolved against the base model.
    fn joint_origin(&self, base: &Model, p: &Pick) -> Result<JointOrigin, String> {
        // a component moved by the preview: its world-space references would map to the wrong
        // base geometry, so it must be re-picked unmoved (clear the input first)
        if let Pick::Face { body, .. } | Pick::Edge { body, .. } = p {
            if let (Some(b0), Some(cur)) = (base.body(body), self.model.body(body)) {
                if !std::sync::Arc::ptr_eq(b0, cur) {
                    return Err("This component is moved by the joint preview. Clear the input on it (\u{00d7}) to pick new geometry on it.".into());
                }
            }
        }
        let (body, geometry) = match p {
            Pick::Face { body, face } => {
                let (b, j) = self.base_face(base, body, *face).ok_or("Select geometry of an existing body (not of the preview).")?;
                if b.topology().face_info[j].surface != SurfaceKind::Plane {
                    return Err("Select a planar face or a circular edge.".into());
                }
                (b.clone(), JointGeometry::Face { face: refs::face_ref(&b, j).ok_or("The face cannot be referenced.")? })
            }
            Pick::Edge { body, edge } => {
                let (b, j) = self.base_edge(base, body, *edge).ok_or("Select geometry of an existing body (not of the preview).")?;
                if b.topology().edge_info[j].curve != CurveKind::Circle {
                    return Err("Select a circular edge (or a planar face).".into());
                }
                cad_kernel::edge_joint_frame(&b, j)?;
                (b.clone(), JointGeometry::Edge { edge: refs::edge_ref(&b, j).ok_or("The edge cannot be referenced.")? })
            }
            _ => return Err("Select a planar face or a circular edge.".into()),
        };
        Ok(JointOrigin { geometry, component: base.component_of(&self.doc, &body.id).map(String::from) })
    }

    /// Displayed-model pick of a joint origin (for highlighting).
    fn joint_pick(&self, base: &Model, o: &JointOrigin) -> Option<Pick> {
        match &o.geometry {
            JointGeometry::Face { face } => self.face_pick(base, face),
            JointGeometry::Edge { edge } => self.edge_pick(base, edge),
        }
    }

    pub(crate) fn route_joint(&self, x: &mut JointDialog, route: Route) -> bool {
        let (picks, mode) = route_picks(route);
        let base = x.s.base.0.clone();
        let mut changed = false;
        for p in picks.iter().filter(|p| is_joint_pick(p)) {
            // clicking a selected origin again (or deselecting it) clears that input
            if mode == 0 || mode == 2 {
                if x.a.as_ref().and_then(|o| self.joint_pick(&base, o)).as_ref() == Some(p) {
                    x.a = None;
                    x.active = BodyInput::First;
                    changed = true;
                    continue;
                }
                if x.b.as_ref().and_then(|o| self.joint_pick(&base, o)).as_ref() == Some(p) {
                    x.b = None;
                    x.active = BodyInput::Second;
                    changed = true;
                    continue;
                }
                if mode == 2 {
                    continue;
                }
            }
            let o = match self.joint_origin(&base, p) {
                Ok(o) => o,
                Err(e) => {
                    x.s.notice = Some(e);
                    continue;
                }
            };
            let first = x.active == BodyInput::First || x.a.is_none();
            let other = if first { &x.b } else { &x.a };
            if other.as_ref().is_some_and(|q| q.component == o.component) {
                x.s.notice = Some(format!("{} is already used by the other joint origin. Select geometry on a different component.", self.doc.component_name(o.component.as_deref())));
                continue;
            }
            if first {
                x.a = Some(o);
                // Fusion advances to the second component input
                if x.b.is_none() {
                    x.active = BodyInput::Second;
                }
            } else {
                x.b = Some(o);
            }
            x.s.notice = None;
            changed = true;
        }
        changed
    }

    pub(crate) fn update_joint(&mut self, x: &mut JointDialog) {
        if x.s.feature.is_none() {
            let (Some(a), Some(b)) = (x.a.clone(), x.b.clone()) else { return };
            x.s.feature = Some(ops::new_joint(&mut self.doc, a, b, x.kind, "0 deg", "0 mm", x.flip));
        }
        let fid = x.s.feature.clone().unwrap_or_default();
        let names: Vec<String> = self.doc.feature(&fid).map(|f| f.parameter_names().iter().map(|s| s.to_string()).collect()).unwrap_or_default();
        if let [pa, po] = names.as_slice() {
            apply_expr(&mut self.doc, pa, &mut x.angle);
            apply_expr(&mut self.doc, po, &mut x.offset);
        }
        if let Some(f) = self.doc.feature_mut(&fid) {
            // no preview while an input is missing (picks must not land on a moved preview)
            f.suppressed = x.user_suppressed || x.a.is_none() || x.b.is_none();
            if let FeatureKind::Joint { a, b, kind, flip, .. } = &mut f.kind {
                if let Some(o) = &x.a {
                    *a = o.clone();
                }
                if let Some(o) = &x.b {
                    *b = o.clone();
                }
                *kind = x.kind;
                *flip = x.flip;
            }
        }
        self.recompute();
    }

    pub(crate) fn joint_picks(&self, x: &JointDialog) -> Vec<Pick> {
        let base = &x.s.base.0;
        x.a.iter().chain(x.b.iter()).filter_map(|o| self.joint_pick(base, o)).collect()
    }

    pub(crate) fn joint_status(&self, x: &JointDialog) -> Option<String> {
        let (Some(a), Some(b)) = (&x.a, &x.b) else { return None };
        let (ca, cb) = (a.component.as_deref(), b.component.as_deref());
        let (na, nb) = (self.doc.component_name(ca), self.doc.component_name(cb));
        // same rule as the kernel's assembly solve (the joint being edited is left out)
        match cad_kernel::joint_moves_b(&self.doc, x.s.feature.as_deref(), ca, cb) {
            Ok(true) => Some(format!("{na} (fixed) <- {nb} (moves)")),
            Ok(false) => Some(format!("{nb} (fixed) <- {na} (moves)")),
            Err(e) => Some(e),
        }
    }

    /// Grounds or frees a component (grounded components never move when jointed); one undo
    /// step.
    pub fn set_component_grounded(&mut self, id: &str, grounded: bool) {
        let id = id.to_string();
        if self.doc.component(&id).is_some_and(|c| c.grounded == grounded) {
            return;
        }
        self.apply(if grounded { "Ground" } else { "Unground" }, |d| {
            d.component_mut(&id).ok_or_else(|| format!("Unknown component \"{id}\""))?.grounded = grounded;
            Ok(())
        });
    }

    /// Name of the component a joint origin input designates ("Select" when empty).
    pub fn joint_input_text(&self, o: &Option<JointOrigin>) -> String {
        match o {
            None => "Select".into(),
            Some(o) => {
                let what = match o.geometry {
                    JointGeometry::Face { .. } => "Face",
                    JointGeometry::Edge { .. } => "Circular edge",
                };
                format!("{what} \u{00b7} {}", self.doc.component_name(o.component.as_deref()))
            }
        }
    }
}
