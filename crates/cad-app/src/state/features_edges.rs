//! Fillet / Chamfer (edge inputs), Hole (planar face input) and Offset Plane (plane input).
//!
//! Picks arrive as indices into the *displayed* model, which shows the preview. They are mapped
//! to the base model (before the feature) through the persistent-reference resolver, so an edge
//! clicked on the filleted preview still designates the original edge.

use super::{apply_expr, eval_expr, param_expr, Dialog, EdgeDialog, ExprField, HoleDialog, PlaneDialog, Route};
use crate::state::{AppState, Pick};
use cad_core::expr::Display;
use cad_core::{ops, EdgeRef, FaceRef, FeatureKind, PlaneRef, Quantity, SurfaceKind};
use cad_kernel::{refs, Body, Frame, Model};
use std::sync::Arc;

fn route_picks(route: Route) -> (Vec<Pick>, u8) {
    match route {
        Route::Toggle(p) => (vec![p], 0),
        Route::Add(v) => (v, 1),
        Route::Remove(v) => (v, 2),
        Route::Nothing => (Vec::new(), 3),
    }
}

/// Face-frame coordinates of a face centroid (the frame the kernel places holes in).
fn face_centroid_local(body: &Body, face: usize) -> [f64; 2] {
    let fi = &body.topology().face_info[face];
    Frame::on_plane(fi.centroid, fi.normal).to_local(fi.centroid)
}

impl AppState {
    // ------------------------------------------------------------------ pick mapping
    /// Maps an edge of the displayed model to the same edge of the base model.
    pub(crate) fn base_edge(&self, base: &Model, body: &str, edge: usize) -> Option<(Body, usize)> {
        let b0 = base.body(body)?;
        let cur = self.model.body(body)?;
        if Arc::ptr_eq(b0, cur) {
            return (edge < b0.topology().edge_info.len()).then(|| (b0.clone(), edge));
        }
        let r = refs::edge_ref(cur, edge)?;
        refs::resolve_edge(b0, &r).ok().map(|j| (b0.clone(), j))
    }

    /// Maps a face of the displayed model to the same face of the base model.
    pub(crate) fn base_face(&self, base: &Model, body: &str, face: usize) -> Option<(Body, usize)> {
        let b0 = base.body(body)?;
        let cur = self.model.body(body)?;
        if Arc::ptr_eq(b0, cur) {
            return (face < b0.topology().face_info.len()).then(|| (b0.clone(), face));
        }
        let r = refs::face_ref(cur, face)?;
        refs::resolve_face(b0, &r).ok().map(|j| (b0.clone(), j))
    }

    /// Displayed-model pick of an edge reference (for highlighting).
    pub(crate) fn edge_pick(&self, base: &Model, r: &EdgeRef) -> Option<Pick> {
        let cur = self.model.body(&r.body)?;
        let i = match base.body(&r.body) {
            Some(b0) if Arc::ptr_eq(b0, cur) => refs::resolve_edge(b0, r).ok()?,
            _ => refs::resolve_edge(cur, r).ok()?,
        };
        Some(Pick::Edge { body: r.body.clone(), edge: i })
    }

    pub(crate) fn face_pick(&self, base: &Model, r: &FaceRef) -> Option<Pick> {
        let cur = self.model.body(&r.body)?;
        let i = match base.body(&r.body) {
            Some(b0) if Arc::ptr_eq(b0, cur) => refs::resolve_face(b0, r).ok()?,
            _ => refs::resolve_face(cur, r).ok()?,
        };
        Some(Pick::Face { body: r.body.clone(), face: i })
    }

    pub(crate) fn plane_pick(&self, base: &Model, p: &PlaneRef) -> Option<Pick> {
        match p {
            PlaneRef::Origin { plane } => Some(Pick::OriginPlane(*plane)),
            PlaneRef::Construction { feature } => Some(Pick::ConstructionPlane(feature.clone())),
            PlaneRef::Face { face } => self.face_pick(base, face),
        }
    }

    /// Base-model edge index an edge reference designates.
    fn edge_key(base: &Model, r: &EdgeRef) -> Option<(String, usize)> {
        let b = base.body(&r.body)?;
        refs::resolve_edge(b, r).ok().map(|j| (r.body.clone(), j))
    }

    // ------------------------------------------------------------------ fillet / chamfer
    pub(crate) fn open_edge_dialog(&mut self, chamfer: bool, editing: Option<&str>) {
        let label = if chamfer { "Chamfer" } else { "Fillet" };
        let initial: Vec<Pick> = self.selection.iter().filter(|p| matches!(p, Pick::Edge { .. } | Pick::Face { .. })).cloned().collect();
        let s = self.open_session(label, editing);
        let existing = editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone());
        let mut d = match existing {
            Some(FeatureKind::Fillet { edges, radius: p }) | Some(FeatureKind::Chamfer { edges, distance: p }) => {
                EdgeDialog { value: ExprField::new(&param_expr(&self.doc, &p)), s, edges }
            }
            _ => EdgeDialog { s, edges: Vec::new(), value: ExprField::new("1 mm") },
        };
        if editing.is_none() {
            self.route_edges(&mut d, Route::Add(initial));
        }
        self.dialog = Some(if chamfer { Dialog::Chamfer(d) } else { Dialog::Fillet(d) });
        self.dialog_changed();
    }

    /// Base-model edge keys of an edge or face pick (a face stands for its boundary edges, as
    /// in Fusion's fillet face selection).
    fn edge_keys_of(&self, base: &Model, p: &Pick) -> Vec<(Body, usize)> {
        match p {
            Pick::Edge { body, edge } => self.base_edge(base, body, *edge).into_iter().collect(),
            Pick::Face { body, face } => match self.base_face(base, body, *face) {
                Some((b, f)) => {
                    let t = b.topology();
                    (0..t.edge_info.len()).filter(|&e| t.edge_info[e].faces.contains(&(f as i32)) && t.edge_info[e].length > 1e-9).map(|e| (b.clone(), e)).collect()
                }
                None => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    pub(crate) fn route_edges(&self, x: &mut EdgeDialog, route: Route) -> bool {
        let (picks, mode) = route_picks(route);
        let base = x.s.base.0.clone();
        let mut changed = false;
        for p in picks {
            let keys = self.edge_keys_of(&base, &p);
            if keys.is_empty() {
                if mode != 2 {
                    x.s.notice = Some("Select edges of an existing body (not of the preview).".into());
                }
                continue;
            }
            let pos = |edges: &Vec<EdgeRef>, b: &Body, j: usize| edges.iter().position(|r| Self::edge_key(&base, r) == Some((b.id.clone(), j)));
            let all_in = keys.iter().all(|(b, j)| pos(&x.edges, b, *j).is_some());
            let remove = mode == 2 || (mode == 0 && all_in);
            for (b, j) in keys {
                match (pos(&x.edges, &b, j), remove) {
                    (Some(i), true) => {
                        x.edges.remove(i);
                        changed = true;
                    }
                    (None, false) => {
                        if let Some(r) = refs::edge_ref(&b, j) {
                            x.edges.push(r);
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
            if changed {
                x.s.notice = None;
            }
        }
        changed
    }

    pub(crate) fn update_edges(&mut self, x: &mut EdgeDialog, chamfer: bool) {
        if x.s.feature.is_none() {
            if x.edges.is_empty() {
                return;
            }
            x.s.feature = Some(if chamfer { ops::new_chamfer(&mut self.doc, x.edges.clone(), "1 mm") } else { ops::new_fillet(&mut self.doc, x.edges.clone(), "1 mm") });
        }
        let fid = x.s.feature.clone().unwrap_or_default();
        let param = match self.doc.feature(&fid).map(|f| &f.kind) {
            Some(FeatureKind::Fillet { radius: p, .. }) | Some(FeatureKind::Chamfer { distance: p, .. }) => p.clone(),
            _ => return,
        };
        apply_expr(&mut self.doc, &param, &mut x.value);
        if let Some(FeatureKind::Fillet { edges, .. }) | Some(FeatureKind::Chamfer { edges, .. }) = self.doc.feature_mut(&fid).map(|f| &mut f.kind) {
            *edges = x.edges.clone();
        }
        self.recompute();
    }

    /// Removes one edge input (the "x" next to it in the dialog).
    pub fn dialog_remove_edge(&mut self, index: usize) {
        self.dialog_edit(|d| {
            if let Dialog::Fillet(x) | Dialog::Chamfer(x) = d {
                if index < x.edges.len() {
                    x.edges.remove(index);
                }
            }
        });
    }

    // ------------------------------------------------------------------ hole
    pub(crate) fn open_hole(&mut self, editing: Option<&str>) {
        let initial: Vec<Pick> = self.selection.iter().filter(|p| matches!(p, Pick::Face { .. })).cloned().collect();
        let s = self.open_session("Hole", editing);
        let existing = editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone());
        let mut d = HoleDialog {
            s,
            face: None,
            centroid: [0.0, 0.0],
            offset_x: ExprField::new("0 mm"),
            offset_y: ExprField::new("0 mm"),
            diameter: ExprField::new("5 mm"),
            depth: ExprField::new("10 mm"),
            through_all: false,
        };
        if let Some(FeatureKind::Hole { face, center, diameter, depth, through_all }) = existing {
            let base = d.s.base.0.clone();
            if let Some(b0) = base.body(&face.body) {
                if let Ok(j) = refs::resolve_face(b0, &face) {
                    d.centroid = face_centroid_local(b0, j);
                }
            }
            d.offset_x = ExprField::new(&Display(center[0] - d.centroid[0], Quantity::Length).to_string());
            d.offset_y = ExprField::new(&Display(center[1] - d.centroid[1], Quantity::Length).to_string());
            d.diameter = ExprField::new(&param_expr(&self.doc, &diameter));
            d.depth = ExprField::new(&param_expr(&self.doc, &depth));
            d.through_all = through_all;
            d.face = Some(face);
        } else {
            self.route_hole(&mut d, Route::Add(initial));
        }
        self.dialog = Some(Dialog::Hole(d));
        self.dialog_changed();
    }

    pub(crate) fn route_hole(&self, x: &mut HoleDialog, route: Route) -> bool {
        let (picks, mode) = route_picks(route);
        if mode >= 2 {
            return false;
        }
        let base = x.s.base.0.clone();
        // single input: the last planar face picked replaces the previous one
        for p in picks.iter().rev() {
            let Pick::Face { body, face } = p else { continue };
            let Some((b, j)) = self.base_face(&base, body, *face) else {
                x.s.notice = Some("Select a face of an existing body.".into());
                continue;
            };
            if b.topology().face_info[j].surface != SurfaceKind::Plane {
                x.s.notice = Some("Holes must be placed on a planar face.".into());
                continue;
            }
            let r = refs::face_ref(&b, j);
            if r.is_some() && r != x.face {
                x.face = r;
                x.centroid = face_centroid_local(&b, j);
                x.s.notice = None;
                return true;
            }
            return false;
        }
        false
    }

    pub(crate) fn update_hole(&mut self, x: &mut HoleDialog) {
        let Some(face) = x.face.clone() else { return };
        let mut off = [0.0; 2];
        let mut ok = true;
        for (k, f) in [&mut x.offset_x, &mut x.offset_y].into_iter().enumerate() {
            match eval_expr(&self.doc, &f.text, Quantity::Length) {
                Ok(v) => {
                    off[k] = v;
                    f.error = None;
                }
                Err(e) => {
                    f.error = Some(e);
                    ok = false;
                }
            }
        }
        let center = [x.centroid[0] + off[0], x.centroid[1] + off[1]];
        if x.s.feature.is_none() {
            x.s.feature = Some(ops::new_hole(&mut self.doc, face.clone(), center, "5 mm", "10 mm", x.through_all));
        }
        let fid = x.s.feature.clone().unwrap_or_default();
        let (pd, ph) = match self.doc.feature(&fid).map(|f| &f.kind) {
            Some(FeatureKind::Hole { diameter, depth, .. }) => (diameter.clone(), depth.clone()),
            _ => return,
        };
        apply_expr(&mut self.doc, &pd, &mut x.diameter);
        apply_expr(&mut self.doc, &ph, &mut x.depth);
        if let Some(FeatureKind::Hole { face: f, center: c, through_all, .. }) = self.doc.feature_mut(&fid).map(|f| &mut f.kind) {
            *f = face;
            if ok {
                *c = center;
            }
            *through_all = x.through_all;
        }
        self.recompute();
    }

    // ------------------------------------------------------------------ offset plane
    pub(crate) fn open_plane(&mut self, editing: Option<&str>) {
        let initial: Vec<Pick> = self.selection.iter().filter(|p| matches!(p, Pick::Face { .. } | Pick::OriginPlane(_) | Pick::ConstructionPlane(_))).cloned().collect();
        let s = self.open_session("Offset Plane", editing);
        let existing = editing.and_then(|id| self.doc.feature(id)).map(|f| f.kind.clone());
        let mut d = PlaneDialog { s, base: None, offset: ExprField::new("10 mm") };
        if let Some(FeatureKind::Plane { base, offset, .. }) = existing {
            d.base = Some(base);
            d.offset = ExprField::new(&param_expr(&self.doc, &offset));
        } else {
            self.route_plane(&mut d, Route::Add(initial));
        }
        self.dialog = Some(Dialog::OffsetPlane(d));
        self.dialog_changed();
    }

    pub(crate) fn route_plane(&self, x: &mut PlaneDialog, route: Route) -> bool {
        let (picks, mode) = route_picks(route);
        if mode >= 2 {
            return false;
        }
        let base = x.s.base.0.clone();
        for p in picks.iter().rev() {
            let r = match p {
                Pick::OriginPlane(plane) => Some(PlaneRef::Origin { plane: *plane }),
                Pick::ConstructionPlane(id) if Some(id) != x.s.feature.as_ref() && base.planes.contains_key(id) => Some(PlaneRef::Construction { feature: id.clone() }),
                Pick::Face { body, face } => match self.base_face(&base, body, *face) {
                    Some((b, j)) if b.topology().face_info[j].surface == SurfaceKind::Plane => refs::face_ref(&b, j).map(|face| PlaneRef::Face { face }),
                    _ => {
                        x.s.notice = Some("Select a planar face or a plane.".into());
                        None
                    }
                },
                _ => None,
            };
            if let Some(r) = r {
                if Some(&r) != x.base.as_ref() {
                    x.base = Some(r);
                    x.s.notice = None;
                    return true;
                }
                return false;
            }
        }
        false
    }

    pub(crate) fn update_plane(&mut self, x: &mut PlaneDialog) {
        let Some(base) = x.base.clone() else { return };
        if x.s.feature.is_none() {
            x.s.feature = Some(ops::new_plane(&mut self.doc, base.clone(), "10 mm"));
        }
        let fid = x.s.feature.clone().unwrap_or_default();
        let param = match self.doc.feature(&fid).map(|f| &f.kind) {
            Some(FeatureKind::Plane { offset, .. }) => offset.clone(),
            _ => return,
        };
        apply_expr(&mut self.doc, &param, &mut x.offset);
        if let Some(FeatureKind::Plane { base: b, .. }) = self.doc.feature_mut(&fid).map(|f| &mut f.kind) {
            *b = base;
        }
        self.recompute();
    }
}
