//! INSPECT panel tool dialogs without a transaction: Interference and Center of Mass, plus the
//! canvas markers of the inspect tools (Measure's closest points, the centre-of-mass glyph).

use super::{Dialog, Route};
use crate::state::command::CommandId;
use crate::state::{AppState, Pick};
use cad_core::BodyId;
use cad_kernel::frame::V3;
use cad_kernel::inspect::{self, MassProps};

/// Interference volumes at or below this (mm^3) count as touching, not interfering.
pub const INTERFERENCE_TOLERANCE: f64 = 1e-6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InspectKind {
    #[default]
    Interference,
    CenterOfMass,
}

/// One interfering pair of bodies.
#[derive(Clone, Debug, PartialEq)]
pub struct InterferenceRow {
    pub a: BodyId,
    pub b: BodyId,
    pub a_name: String,
    pub b_name: String,
    /// Shared volume (mm^3).
    pub volume: f64,
}

/// Interference / Center of Mass dialog. The checked bodies are the selected ones, or every
/// visible solid body when nothing is selected.
#[derive(Clone, Debug, Default)]
pub struct InspectDialog {
    pub kind: InspectKind,
    pub bodies: Vec<BodyId>,
    pub synced: Vec<Pick>,
    /// Interference: the result of the last Compute (`None` until computed or after the
    /// selection changed).
    pub interference: Option<Vec<InterferenceRow>>,
    /// Number of bodies the last interference check looked at.
    pub checked: usize,
    /// Center of Mass of the checked bodies (recomputed when the selection changes).
    pub mass: Option<MassProps>,
    pub status: Option<String>,
    /// The bodies (shared Arcs, by address) the shown results were evaluated on; a recompute
    /// or undo replaces them, which marks the results stale.
    pub evaluated: Vec<usize>,
}

impl InspectDialog {
    pub fn title(&self) -> &'static str {
        match self.kind {
            InspectKind::Interference => "INTERFERENCE",
            InspectKind::CenterOfMass => "CENTER OF MASS",
        }
    }

    pub fn picks(&self) -> Vec<Pick> {
        self.bodies.iter().map(|b| Pick::Body { body: b.clone() }).collect()
    }

    pub fn clear_inputs(&mut self) {
        self.bodies.clear();
        self.interference = None;
        self.mass = None;
        self.status = None;
        self.evaluated.clear();
    }
}

/// Selection items an inspect dialog takes (bodies, or their faces / edges).
pub(crate) fn is_inspect_pick(p: &Pick) -> bool {
    matches!(p, Pick::Body { .. } | Pick::Face { .. } | Pick::Edge { .. })
}

fn pick_body(p: &Pick) -> Option<&BodyId> {
    match p {
        Pick::Body { body } | Pick::Face { body, .. } | Pick::Edge { body, .. } => Some(body),
        _ => None,
    }
}

/// Something an inspect tool shows in the canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InspectMarker {
    Point(V3),
    Segment(V3, V3),
}

impl AppState {
    pub(crate) fn open_inspect(&mut self, id: CommandId) {
        let kind = if id == CommandId::CenterOfMass { InspectKind::CenterOfMass } else { InspectKind::Interference };
        let mut d = InspectDialog { kind, ..Default::default() };
        for p in self.selection.iter().filter(|p| is_inspect_pick(p)) {
            if let Some(b) = pick_body(p) {
                if self.model.body(b).is_some() && !d.bodies.contains(b) {
                    d.bodies.push(b.clone());
                }
            }
        }
        self.refresh_inspect(&mut d);
        self.dialog = Some(Dialog::Inspect(d));
        self.sync_selection();
    }

    /// Applies a canvas selection change to the dialog's bodies.
    pub(crate) fn route_inspect(&self, x: &mut InspectDialog, r: Route) {
        let before = x.bodies.clone();
        let toggle =|b: &BodyId, x: &mut InspectDialog| match x.bodies.iter().position(|q| q == b) {
            Some(i) => {
                x.bodies.remove(i);
            }
            None => x.bodies.push(b.clone()),
        };
        match r {
            Route::Toggle(p) => {
                if let Some(b) = pick_body(&p) {
                    toggle(b, x);
                }
            }
            Route::Add(v) => {
                for b in v.iter().filter_map(pick_body) {
                    if !x.bodies.contains(b) {
                        x.bodies.push(b.clone());
                    }
                }
            }
            Route::Remove(v) => {
                let gone: Vec<&BodyId> = v.iter().filter_map(pick_body).collect();
                x.bodies.retain(|b| !gone.contains(&b));
            }
            Route::Nothing => {}
        }
        x.bodies.retain(|b| self.model.body(b).is_some());
        if x.bodies != before {
            self.refresh_inspect(x);
        }
    }

    /// Bodies the dialog checks: its selection, or every visible body.
    pub fn inspect_bodies(&self, x: &InspectDialog) -> Vec<&cad_kernel::Body> {
        if x.bodies.is_empty() {
            self.visible_bodies()
        } else {
            x.bodies.iter().filter_map(|b| self.model.body(b)).collect()
        }
    }

    fn inspect_key(&self, x: &InspectDialog) -> Vec<usize> {
        self.inspect_bodies(x).into_iter().map(|b| std::sync::Arc::as_ptr(b) as usize).collect()
    }

    /// Called every frame by the panel: after a recompute / undo (or Clear) the centre of mass
    /// is evaluated again and an interference result is dropped.
    pub(crate) fn inspect_refresh_if_stale(&self, x: &mut InspectDialog) {
        x.bodies.retain(|b| self.model.body(b).is_some());
        if self.inspect_key(x) == x.evaluated {
            return;
        }
        match x.kind {
            InspectKind::CenterOfMass => self.refresh_inspect(x),
            InspectKind::Interference if x.interference.is_some() => {
                self.refresh_inspect(x);
                x.status = Some("The model changed; click Compute again.".into());
            }
            InspectKind::Interference => x.evaluated = self.inspect_key(x),
        }
    }

    /// Updates what depends on the selection: the centre of mass is recomputed; a computed
    /// interference result is dropped (Fusion: select, then Compute).
    pub(crate) fn refresh_inspect(&self, x: &mut InspectDialog) {
        x.interference = None;
        x.status = None;
        x.evaluated = self.inspect_key(x);
        if x.kind == InspectKind::CenterOfMass {
            let bodies = self.inspect_bodies(x);
            x.mass = inspect::center_of_mass(&bodies);
            if x.mass.is_none() {
                x.status = Some(if bodies.is_empty() { "There are no bodies to evaluate.".into() } else { "The selection has no solid bodies (surfaces and meshes have no mass).".into() });
            }
        }
    }

    /// Interference: checks every pair of the dialog's bodies (Compute button).
    pub fn inspect_compute(&self, x: &mut InspectDialog) {
        let bodies = self.inspect_bodies(x);
        let solids: Vec<&cad_kernel::Body> = bodies.into_iter().filter(|b| inspect::is_solid(b)).collect();
        x.checked = solids.len();
        x.evaluated = self.inspect_key(x);
        if solids.len() < 2 {
            x.interference = None;
            x.status = Some("Select at least two solid bodies (or show them) to check for interference.".into());
            return;
        }
        let check = inspect::interference_check(&solids, INTERFERENCE_TOLERANCE);
        let rows: Vec<InterferenceRow> = check
            .pairs
            .into_iter()
            .map(|r| {
                let (a, b) = (&solids[r.a].id, &solids[r.b].id);
                InterferenceRow { a: a.clone(), b: b.clone(), a_name: self.body_name(a), b_name: self.body_name(b), volume: r.volume }
            })
            .collect();
        let mut status = match rows.len() {
            0 if check.failed.is_empty() => format!("No interference between {} bodies.", solids.len()),
            0 => "No interference found.".to_string(),
            1 => "1 interference found.".to_string(),
            n => format!("{n} interferences found."),
        };
        for (i, j) in &check.failed {
            status.push_str(&format!(" {} / {} could not be checked.", self.body_name(&solids[*i].id), self.body_name(&solids[*j].id)));
        }
        x.status = Some(status);
        x.interference = Some(rows);
    }

    /// Canvas markers of the open inspect dialog: Measure's minimum-distance segment and its
    /// end points; the centre-of-mass point.
    pub fn inspect_markers(&self) -> Vec<InspectMarker> {
        match &self.dialog {
            Some(Dialog::Measure(m)) => match self.measure_report_for(&m.picks).min {
                Some(d) => vec![InspectMarker::Segment(d.a, d.b), InspectMarker::Point(d.a), InspectMarker::Point(d.b)],
                None => vec![],
            },
            Some(Dialog::Inspect(x)) if x.kind == InspectKind::CenterOfMass => x.mass.iter().map(|p| InspectMarker::Point(p.centroid)).collect(),
            _ => vec![],
        }
    }
}
