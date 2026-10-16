//! Extrude and Revolve: profile-based feature dialogs.

use super::{apply_expr, param_expr, Dialog, ExprField, ExtrudeDialog, RevolveDialog, Route, Session};
use crate::state::{AppState, Pick};
use cad_core::{ops, ExtentDirection, FeatureKind, Operation, OriginAxis, ProfileRef, RevolveAxis};
use cad_kernel::frame::{dot, sub};
use cad_kernel::{Model, SketchResult};

/// Index of the profile a reference designates (same rule as the kernel: same bounding curves
/// first, then the region containing the sample point).
pub(crate) fn resolve_profile(sr: &SketchResult, r: &ProfileRef) -> Option<usize> {
    sr.profiles.iter().position(|p| p.curves == r.curves).or_else(|| sr.profiles.iter().position(|p| p.contains(r.sample)))
}

fn profile_ref(sr: &SketchResult, index: usize) -> Option<ProfileRef> {
    sr.profiles.get(index).map(|p| ProfileRef { curves: p.curves.clone(), sample: p.interior_point() })
}

/// Display name of a revolve axis choice.
pub fn axis_label(a: &RevolveAxis) -> String {
    match a {
        RevolveAxis::Origin { axis } => format!("{axis:?} Axis"),
        RevolveAxis::SketchLine { entity } => format!("Line {entity}"),
    }
}

impl AppState {
    /// Profile picks of the current selection (all from the first sketch picked).
    fn selected_profiles(&self) -> (Option<String>, Vec<ProfileRef>) {
        let mut sketch: Option<String> = None;
        let mut out = Vec::new();
        for p in &self.selection {
            if let Pick::Profile { sketch: sk, index } = p {
                if sketch.is_none() {
                    sketch = Some(sk.clone());
                }
                if sketch.as_deref() == Some(sk.as_str()) {
                    if let Some(r) = self.model.sketches.get(sk).and_then(|sr| profile_ref(sr, *index)) {
                        out.push(r);
                    }
                }
            }
        }
        (sketch, out)
    }

    pub(crate) fn open_extrude(&mut self, editing: Option<&str>) {
        let (sel_sketch, sel_profiles) = self.selected_profiles();
        let s = self.open_session("Extrude", editing);
        let d = match editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            Some(FeatureKind::Extrude { sketch, profiles, distance, direction, through_all, operation }) => ExtrudeDialog {
                distance: ExprField::new(&param_expr(&self.doc, &distance)),
                s,
                sketch: Some(sketch),
                profiles,
                direction,
                through_all,
                operation,
            },
            _ => {
                // Fusion: New Body when there is nothing to join, otherwise Join
                let operation = if s.base.0.bodies.is_empty() { Operation::New } else { Operation::Join };
                ExtrudeDialog { s, sketch: sel_sketch, profiles: sel_profiles, direction: ExtentDirection::Normal, through_all: false, distance: ExprField::new("10 mm"), operation }
            }
        };
        self.dialog = Some(Dialog::Extrude(d));
        self.dialog_changed();
    }

    pub(crate) fn open_revolve(&mut self, editing: Option<&str>) {
        let (sel_sketch, sel_profiles) = self.selected_profiles();
        let s = self.open_session("Revolve", editing);
        let d = match editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone()) {
            Some(FeatureKind::Revolve { sketch, profiles, axis, angle, operation }) => {
                RevolveDialog { angle: ExprField::new(&param_expr(&self.doc, &angle)), s, sketch: Some(sketch), profiles, axis: Some(axis), operation }
            }
            _ => {
                let operation = if s.base.0.bodies.is_empty() { Operation::New } else { Operation::Join };
                let axis = self.default_axis(sel_sketch.as_deref(), &s.base.0);
                RevolveDialog { s, sketch: sel_sketch, profiles: sel_profiles, axis, angle: ExprField::new("360 deg"), operation }
            }
        };
        self.dialog = Some(Dialog::Revolve(d));
        self.dialog_changed();
    }

    pub(crate) fn update_extrude(&mut self, x: &mut ExtrudeDialog) {
        let Some(sketch) = x.sketch.clone() else { return };
        if x.s.feature.is_none() {
            if x.profiles.is_empty() {
                return;
            }
            x.s.feature = Some(ops::new_extrude(&mut self.doc, &sketch, x.profiles.clone(), "10 mm", x.direction, x.operation));
        }
        let fid = x.s.feature.clone().unwrap_or_default();
        let param = match self.doc.feature(&fid).map(|f| &f.kind) {
            Some(FeatureKind::Extrude { distance, .. }) => distance.clone(),
            _ => return,
        };
        apply_expr(&mut self.doc, &param, &mut x.distance);
        if let Some(FeatureKind::Extrude { sketch: sk, profiles, direction, through_all, operation, .. }) = self.doc.feature_mut(&fid).map(|f| &mut f.kind) {
            *sk = sketch;
            *profiles = x.profiles.clone();
            *direction = x.direction;
            *through_all = x.through_all;
            *operation = x.operation;
        }
        self.recompute();
    }

    pub(crate) fn update_revolve(&mut self, x: &mut RevolveDialog) {
        let Some(sketch) = x.sketch.clone() else { return };
        if x.s.feature.is_none() {
            let Some(axis) = x.axis.clone() else { return };
            if x.profiles.is_empty() {
                return;
            }
            x.s.feature = Some(ops::new_revolve(&mut self.doc, &sketch, x.profiles.clone(), axis, "360 deg", x.operation));
        }
        let fid = x.s.feature.clone().unwrap_or_default();
        let param = match self.doc.feature(&fid).map(|f| &f.kind) {
            Some(FeatureKind::Revolve { angle, .. }) => angle.clone(),
            _ => return,
        };
        apply_expr(&mut self.doc, &param, &mut x.angle);
        if let Some(FeatureKind::Revolve { sketch: sk, profiles, axis, operation, .. }) = self.doc.feature_mut(&fid).map(|f| &mut f.kind) {
            *sk = sketch;
            *profiles = x.profiles.clone();
            if let Some(a) = &x.axis {
                *axis = a.clone();
            }
            *operation = x.operation;
        }
        self.recompute();
    }

    /// Applies a selection route to a profile input. Returns whether the inputs changed.
    pub(crate) fn route_profiles(&self, s: &mut Session, sketch: &mut Option<String>, profiles: &mut Vec<ProfileRef>, route: Route) -> bool {
        let (picks, mode) = match route {
            Route::Toggle(p) => (vec![p], 0),
            Route::Add(v) => (v, 1),
            Route::Remove(v) => (v, 2),
            Route::Nothing => return false,
        };
        let mut changed = false;
        for p in picks {
            let Pick::Profile { sketch: sk, index } = p else { continue };
            let Some(sr) = s.base.0.sketches.get(&sk).or_else(|| self.model.sketches.get(&sk)) else { continue };
            if sketch.as_deref() != Some(sk.as_str()) {
                if mode == 2 {
                    continue;
                }
                if !profiles.is_empty() {
                    let name = self.doc.feature(&sk).map(|f| f.name.clone()).unwrap_or_default();
                    s.notice = Some(format!("Profiles must come from one sketch; now using {name}."));
                }
                profiles.clear();
                *sketch = Some(sk.clone());
            }
            let pos = profiles.iter().position(|r| resolve_profile(sr, r) == Some(index));
            match (pos, mode) {
                (Some(i), 0 | 2) => {
                    profiles.remove(i);
                    changed = true;
                }
                (None, 0 | 1) => {
                    if let Some(r) = profile_ref(sr, index) {
                        profiles.push(r);
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        changed
    }

    /// Canvas picks for the selected profiles (for highlighting).
    pub(crate) fn profile_picks(&self, sketch: Option<&str>, profiles: &[ProfileRef]) -> Vec<Pick> {
        let Some(sk) = sketch else { return Vec::new() };
        let Some(sr) = self.model.sketches.get(sk) else { return Vec::new() };
        let mut out: Vec<Pick> = Vec::new();
        for r in profiles {
            if let Some(i) = resolve_profile(sr, r) {
                let p = Pick::Profile { sketch: sk.to_string(), index: i };
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// "1 Profile | Area : 1500.00 mm^2" (Fusion's status text for profile selections).
    pub(crate) fn profile_status(&self, s: &Session, sketch: Option<&str>, profiles: &[ProfileRef]) -> Option<String> {
        let sr = s.base.0.sketches.get(sketch?).or_else(|| self.model.sketches.get(sketch?))?;
        if profiles.is_empty() {
            return None;
        }
        let area: f64 = profiles.iter().filter_map(|r| resolve_profile(sr, r)).map(|i| sr.profiles[i].area.abs()).sum();
        let n = profiles.len();
        Some(format!("{n} Profile{} | Area : {area:.2} mm^2", if n == 1 { "" } else { "s" }))
    }

    /// Axis choices for a revolve: origin axes, then the sketch's lines.
    pub fn revolve_axis_choices(&self, sketch: Option<&str>, base: &Model) -> Vec<RevolveAxis> {
        let mut out: Vec<RevolveAxis> = [OriginAxis::X, OriginAxis::Y, OriginAxis::Z].into_iter().map(|axis| RevolveAxis::Origin { axis }).collect();
        if let Some(sr) = sketch.and_then(|sk| base.sketches.get(sk).or_else(|| self.model.sketches.get(sk))) {
            for e in &sr.solved.entities {
                if matches!(e.kind, cad_core::sketch::EntityKind::Line { .. }) {
                    out.push(RevolveAxis::SketchLine { entity: e.id.clone() });
                }
            }
        }
        out
    }

    /// Default revolve axis: a construction line of the sketch, else an origin axis in the
    /// sketch plane, preferring one the profiles do not cross.
    pub(crate) fn default_axis(&self, sketch: Option<&str>, base: &Model) -> Option<RevolveAxis> {
        let sk = sketch?;
        let sr = base.sketches.get(sk).or_else(|| self.model.sketches.get(sk))?;
        let frame = sr.frame;
        let in_plane = |o: [f64; 3], a: [f64; 3]| dot(a, frame.n).abs() < 1e-6 && dot(sub(o, frame.origin), frame.n).abs() < 1e-6;
        let mut candidates: Vec<(RevolveAxis, [f64; 2], [f64; 2])> = Vec::new();
        for e in &sr.solved.entities {
            if e.construction {
                if let Some((p, q)) = sr.solved.line_points(&e.id) {
                    candidates.push((RevolveAxis::SketchLine { entity: e.id.clone() }, p, q));
                }
            }
        }
        for (axis, a) in [(OriginAxis::X, [1.0, 0.0, 0.0]), (OriginAxis::Y, [0.0, 1.0, 0.0]), (OriginAxis::Z, [0.0, 0.0, 1.0])] {
            if in_plane([0.0; 3], a) {
                candidates.push((RevolveAxis::Origin { axis }, frame.to_local([0.0; 3]), frame.to_local(a)));
            }
        }
        let clean = |a: [f64; 2], b: [f64; 2]| {
            sr.profiles.iter().all(|p| {
                let side = |q: [f64; 2]| (b[0] - a[0]) * (q[1] - a[1]) - (b[1] - a[1]) * (q[0] - a[0]);
                let (mn, mx) = p.outer.polygon().iter().map(|q| side(*q)).fold((f64::MAX, f64::MIN), |(lo, hi), s| (lo.min(s), hi.max(s)));
                !(mn < -1e-6 && mx > 1e-6)
            })
        };
        candidates.iter().find(|(_, a, b)| clean(*a, *b)).or(candidates.first()).map(|c| c.0.clone())
    }
}
