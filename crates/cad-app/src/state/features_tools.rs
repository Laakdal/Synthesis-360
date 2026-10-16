//! Tool dialogs without a transaction: Change Parameters, Measure (minimum distance, angle,
//! length / area / volume), Appearance.

use super::{AppearanceDialog, Dialog, MeasureDialog, ParametersDialog, Route};
use crate::state::{AppState, Mode, Pick};
use cad_core::{BodyMeta, Quantity};
use cad_kernel::frame::{dist, sub, V3};
use cad_kernel::inspect::{self, MinDistance};
use cad_kernel::occ::Shape;
use cad_kernel::Body;
use std::cell::RefCell;
use std::sync::Arc;

/// Appearance palette (name, sRGB colour).
pub const APPEARANCES: &[(&str, [u8; 3])] = &[
    ("Aluminum", [176, 180, 186]),
    ("Steel", [132, 138, 146]),
    ("Brass", [196, 160, 72]),
    ("Copper", [184, 104, 64]),
    ("ABS White", [236, 236, 232]),
    ("ABS Black", [38, 38, 40]),
    ("Rubber", [52, 54, 58]),
    ("Glass Blue", [120, 170, 214]),
    ("Wood", [164, 116, 70]),
    ("Red Paint", [190, 36, 36]),
    ("Green Paint", [52, 140, 72]),
    ("Yellow Paint", [230, 190, 40]),
];

/// One measured item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measurement {
    /// "Face", "Edge", "Body", "Profile".
    pub kind: String,
    /// Display name (body name, face type ...).
    pub name: String,
    pub area: Option<f64>,
    pub length: Option<f64>,
    pub radius: Option<f64>,
    pub volume: Option<f64>,
    pub surface_area: Option<f64>,
    /// Bounding-box size of a body.
    pub size: Option<V3>,
    /// Representative point (face centroid, edge midpoint, body bounding-box centre).
    pub point: Option<V3>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasureReport {
    pub items: Vec<Measurement>,
    /// Distance between the first two items: the true minimum distance when both are model
    /// geometry, else between their representative points.
    pub distance: Option<f64>,
    /// From the first item's measured point to the second's.
    pub delta: Option<V3>,
    /// Closest points of the first two items (model geometry only).
    pub min: Option<MinDistance>,
    /// Distance between the representative points (centroids / midpoints).
    pub center_distance: Option<f64>,
    /// Angle (degrees) between two planar faces, two linear edges, or a linear edge and a
    /// planar face.
    pub angle: Option<f64>,
}

thread_local! {
    /// Last minimum-distance evaluation (the dialog redraws every frame). Keyed by the picks
    /// and the bodies they were evaluated on; holding the bodies keeps the key unambiguous.
    static MIN_CACHE: RefCell<Option<(Vec<Pick>, Vec<Body>, Option<MinDistance>)>> = const { RefCell::new(None) };
}

pub(crate) fn route_measure(x: &mut MeasureDialog, route: Route) {
    let add = |x: &mut MeasureDialog, p: Pick| {
        if let Some(i) = x.picks.iter().position(|q| *q == p) {
            x.picks.remove(i);
        } else if x.picks.len() >= 2 {
            // Fusion: a third pick starts a new measurement
            x.picks = vec![p];
        } else {
            x.picks.push(p);
        }
    };
    match route {
        Route::Toggle(p) => add(x, p),
        Route::Add(v) => {
            for p in v {
                if !x.picks.contains(&p) {
                    add(x, p);
                }
            }
        }
        Route::Remove(v) => x.picks.retain(|p| !v.contains(p)),
        Route::Nothing => {}
    }
}

impl AppState {
    // ------------------------------------------------------------------ parameters
    pub(crate) fn open_parameters(&mut self) {
        self.dialog = Some(Dialog::Parameters(ParametersDialog::default()));
    }

    fn params_dialog(&mut self) -> Option<&mut ParametersDialog> {
        match self.dialog.as_mut() {
            Some(Dialog::Parameters(p)) => Some(p),
            _ => None,
        }
    }

    /// Change Parameters: sets a parameter expression as one undoable operation. A rejected
    /// expression leaves the parameter unchanged and records the error for its row.
    pub fn parameters_set(&mut self, name: &str, expression: &str) -> bool {
        let mut trial = self.doc.clone();
        let res = trial.set_expression(name, expression);
        let current = self.doc.parameter(name).map(|p| p.expression.clone());
        let ok = match res {
            Err(e) => {
                let msg = e.to_string();
                if let Some(p) = self.params_dialog() {
                    p.errors.insert(name.to_string(), msg);
                }
                return false;
            }
            Ok(_) if current.as_deref() == Some(expression.trim()) => true,
            Ok(_) => self.set_parameter(name, expression),
        };
        if let Some(p) = self.params_dialog() {
            p.errors.remove(name);
            p.edits.remove(name);
        }
        ok
    }

    /// Adds the user parameter described by the dialog's "new parameter" fields.
    pub fn parameters_add_user(&mut self) -> bool {
        let Some(p) = self.params_dialog() else { return false };
        let (name, expression, q, comment) = (p.new_name.trim().to_string(), p.new_expression.trim().to_string(), p.new_quantity, p.new_comment.clone());
        let mut trial = self.doc.clone();
        if let Err(e) = trial.add_user_parameter(&name, &expression, q, &comment) {
            let msg = e.to_string();
            if let Some(p) = self.params_dialog() {
                p.new_error = Some(msg);
            }
            return false;
        }
        let ok = self.apply(&format!("Add Parameter {name}"), |d| d.add_user_parameter(&name, &expression, q, &comment).map_err(|e| e.to_string()));
        if ok {
            if let Some(p) = self.params_dialog() {
                *p = ParametersDialog { new_quantity: p.new_quantity, ..Default::default() };
            }
        }
        ok
    }

    pub fn parameters_delete(&mut self, name: &str) -> bool {
        let mut trial = self.doc.clone();
        if let Err(e) = trial.delete_user_parameter(name) {
            let msg = e.to_string();
            if let Some(p) = self.params_dialog() {
                p.errors.insert(name.to_string(), msg);
            }
            return false;
        }
        let n = name.to_string();
        self.apply(&format!("Delete Parameter {name}"), |d| d.delete_user_parameter(&n).map_err(|e| e.to_string()))
    }

    // ------------------------------------------------------------------ measure
    pub(crate) fn open_measure(&mut self) {
        let mut d = MeasureDialog::default();
        let initial: Vec<Pick> = self.selection.iter().filter(|p| matches!(p, Pick::Face { .. } | Pick::Edge { .. } | Pick::Body { .. } | Pick::Profile { .. })).cloned().collect();
        for p in initial.into_iter().rev().take(2).collect::<Vec<_>>().into_iter().rev() {
            d.picks.push(p);
        }
        self.dialog = Some(Dialog::Measure(d));
        self.sync_selection();
    }

    pub(crate) fn body_name(&self, id: &str) -> String {
        if let Some(n) = self.doc.bodies.get(id).and_then(|m| m.name.clone()) {
            return n;
        }
        let i = self.model.bodies.iter().position(|b| b.id == id).unwrap_or(0);
        format!("Body{}", i + 1)
    }

    pub fn measure_item(&self, p: &Pick) -> Option<Measurement> {
        match p {
            Pick::Face { body, face } => {
                let b = self.model.body(body)?;
                let t = b.topology();
                let f = t.face_info.get(*face)?;
                let name = match f.surface {
                    cad_core::SurfaceKind::Plane => "Planar face",
                    cad_core::SurfaceKind::Cylinder => "Cylindrical face",
                    cad_core::SurfaceKind::Other => "Face",
                };
                Some(Measurement {
                    kind: "Face".into(),
                    name: format!("{name} ({})", self.body_name(body)),
                    area: Some(f.area),
                    radius: (f.surface == cad_core::SurfaceKind::Cylinder).then_some(f.radius),
                    point: Some(f.centroid),
                    ..Default::default()
                })
            }
            Pick::Edge { body, edge } => {
                let b = self.model.body(body)?;
                let t = b.topology();
                let e = t.edge_info.get(*edge)?;
                let name = match e.curve {
                    cad_core::CurveKind::Line => "Line",
                    cad_core::CurveKind::Circle => "Circular edge",
                    cad_core::CurveKind::Other => "Edge",
                };
                Some(Measurement {
                    kind: "Edge".into(),
                    name: format!("{name} ({})", self.body_name(body)),
                    length: Some(e.length),
                    radius: (e.curve == cad_core::CurveKind::Circle).then_some(e.radius),
                    point: Some(e.mid),
                    ..Default::default()
                })
            }
            Pick::Body { body } => {
                let b = self.model.body(body)?;
                let t = b.topology();
                let bb = t.bbox;
                Some(Measurement {
                    kind: "Body".into(),
                    name: self.body_name(body),
                    volume: Some(b.volume()),
                    surface_area: Some(t.face_info.iter().map(|f| f.area).sum()),
                    size: Some([bb[3] - bb[0], bb[4] - bb[1], bb[5] - bb[2]]),
                    point: Some([(bb[0] + bb[3]) / 2.0, (bb[1] + bb[4]) / 2.0, (bb[2] + bb[5]) / 2.0]),
                    ..Default::default()
                })
            }
            Pick::Profile { sketch, index } => {
                let sr = self.model.sketches.get(sketch)?;
                let pr = sr.profiles.get(*index)?;
                let name = self.doc.feature(sketch).map(|f| f.name.clone()).unwrap_or_default();
                Some(Measurement { kind: "Profile".into(), name, area: Some(pr.area.abs()), point: Some(sr.frame.to_world(pr.interior_point())), ..Default::default() })
            }
            _ => None,
        }
    }

    /// Measurements of the Measure dialog's picks (or of the selection when it is closed).
    #[allow(dead_code)]
    pub fn measure_report(&self) -> MeasureReport {
        let picks: &[Pick] = match &self.dialog {
            Some(Dialog::Measure(m)) => &m.picks,
            _ => &self.selection,
        };
        self.measure_report_for(picks)
    }

    /// Measurements of the given picks.
    pub fn measure_report_for(&self, picks: &[Pick]) -> MeasureReport {
        let items: Vec<Measurement> = picks.iter().filter_map(|p| self.measure_item(p)).collect();
        let (center_distance, center_delta) = match (items.first().and_then(|m| m.point), items.get(1).and_then(|m| m.point)) {
            (Some(a), Some(b)) => (Some(dist(a, b)), Some(sub(b, a))),
            _ => (None, None),
        };
        let (min, angle) = match picks {
            [a, b, ..] if items.len() >= 2 => (self.min_distance_between(a, b), self.angle_between(a, b)),
            _ => (None, None),
        };
        let (distance, delta) = match min {
            Some(m) => (Some(m.distance), Some(sub(m.b, m.a))),
            None => (center_distance, center_delta),
        };
        MeasureReport { items, distance, delta, min, center_distance, angle }
    }

    /// The exact shape of a picked face, edge or body.
    fn pick_shape(&self, p: &Pick) -> Option<(Body, Shape)> {
        let (Pick::Face { body, .. } | Pick::Edge { body, .. } | Pick::Body { body }) = p else { return None };
        let b = self.model.body(body)?;
        let s = match p {
            Pick::Face { face, .. } => b.topology().faces.get(*face)?.clone(),
            Pick::Edge { edge, .. } => b.topology().edges.get(*edge)?.clone(),
            _ => b.shape.clone(),
        };
        Some((b.clone(), s))
    }

    /// True minimum distance between two picked faces / edges / bodies (cached).
    fn min_distance_between(&self, a: &Pick, b: &Pick) -> Option<MinDistance> {
        let ((ba, sa), (bb, sb)) = (self.pick_shape(a)?, self.pick_shape(b)?);
        let key = vec![a.clone(), b.clone()];
        let bodies = vec![ba, bb];
        let hit = MIN_CACHE.with(|c| c.borrow().as_ref().filter(|(k, bs, _)| *k == key && bs.iter().zip(&bodies).all(|(x, y)| Arc::ptr_eq(x, y))).map(|e| e.2));
        if let Some(r) = hit {
            return r;
        }
        let r = inspect::min_distance(&sa, &sb).ok();
        MIN_CACHE.with(|c| *c.borrow_mut() = Some((key, bodies, r)));
        r
    }

    /// Planar-face normal (`true`) or linear-edge direction (`false`) of a pick.
    fn pick_direction(&self, p: &Pick) -> Option<(bool, V3)> {
        match p {
            Pick::Face { body, face } => {
                let t = self.model.body(body)?.topology();
                let f = t.face_info.get(*face)?;
                (f.surface == cad_core::SurfaceKind::Plane).then_some((true, f.normal))
            }
            Pick::Edge { body, edge } => {
                let t = self.model.body(body)?.topology();
                let e = t.edge_info.get(*edge)?;
                (e.curve == cad_core::CurveKind::Line).then_some((false, e.dir))
            }
            _ => None,
        }
    }

    fn angle_between(&self, a: &Pick, b: &Pick) -> Option<f64> {
        Some(match (self.pick_direction(a)?, self.pick_direction(b)?) {
            ((true, n1), (true, n2)) => inspect::plane_plane_angle(n1, n2),
            ((false, u), (false, v)) => inspect::line_line_angle(u, v),
            ((false, u), (true, n)) | ((true, n), (false, u)) => inspect::line_plane_angle(u, n),
        })
    }

    // ------------------------------------------------------------------ appearance
    pub(crate) fn open_appearance(&mut self) {
        self.dialog = Some(Dialog::Appearance(AppearanceDialog::default()));
    }

    /// Bodies the selection designates (bodies, or the bodies of picked faces / edges).
    pub fn selected_bodies(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in &self.selection {
            let b = match p {
                Pick::Body { body } | Pick::Face { body, .. } | Pick::Edge { body, .. } => body,
                _ => continue,
            };
            if !out.contains(b) {
                out.push(b.clone());
            }
        }
        out
    }

    /// Applies palette entry `index` (or `None` = default appearance) to the selected bodies as
    /// one undoable operation.
    pub fn apply_appearance(&mut self, index: Option<usize>) -> bool {
        let bodies = self.selected_bodies();
        let set_status = |s: &mut AppState, t: String| {
            if let Some(Dialog::Appearance(a)) = s.dialog.as_mut() {
                a.status = Some(t);
            }
        };
        if bodies.is_empty() || matches!(self.mode, Mode::Sketch(_)) {
            set_status(self, "Select bodies (or faces of them) in the canvas first.".into());
            return false;
        }
        let color = index.and_then(|i| APPEARANCES.get(i)).map(|a| a.1);
        let name = index.and_then(|i| APPEARANCES.get(i)).map(|a| a.0).unwrap_or("Default");
        let ok = self.apply("Appearance", |d| {
            for b in &bodies {
                d.bodies.entry(b.clone()).or_insert_with(|| BodyMeta { visible: true, ..Default::default() }).color = color;
            }
            Ok(())
        });
        let n = bodies.len();
        set_status(self, format!("{name} applied to {n} bod{}", if n == 1 { "y" } else { "ies" }));
        ok
    }
}

/// Unit label for a parameter quantity in the parameters table.
pub fn quantity_unit(q: Quantity) -> &'static str {
    match q {
        Quantity::Length => "mm",
        Quantity::Angle => "deg",
        Quantity::Unitless => "",
    }
}
