//! Pointer handling of the sketch tools: snapping, creation tools (Line, rectangles, circle,
//! arc, point), the Select tool (hover, selection, dragging), Delete, Normal/Construction and
//! the Sketch Dimension tool with its inline value entry.

use super::*;
use cad_core::sketch::interact::{dist, infer_axis, nearest_curve, nearest_point, nice_step, project_to_axis, snap_to_grid};
use cad_core::sketch::{measure, solve, would_overconstrain, DistanceOrientation};

/// Which kinds of sketch objects a hit test considers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HitFilter {
    pub points: bool,
    pub origin: bool,
    pub lines: bool,
    pub circles: bool,
    /// Dimension labels and constraint glyphs.
    pub annotations: bool,
}

impl HitFilter {
    pub const ALL: HitFilter = HitFilter { points: true, origin: true, lines: true, circles: true, annotations: true };
    pub const GEOMETRY: HitFilter = HitFilter { points: true, origin: true, lines: true, circles: true, annotations: false };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Point,
    Line,
    Circle,
    Arc,
}

pub(crate) fn classify(sk: &Sketch, id: &str) -> Option<Class> {
    if id == ORIGIN {
        return Some(Class::Point);
    }
    Some(match sk.entity(id)?.kind {
        EntityKind::Point { .. } => Class::Point,
        EntityKind::Line { .. } => Class::Line,
        EntityKind::Circle { .. } => Class::Circle,
        EntityKind::Arc { .. } => Class::Arc,
    })
}

/// Constraint tying a newly created point `id` to what its click snapped to.
pub(super) fn snap_constraint(id: &str, snap: &Snap) -> Option<ConstraintKind> {
    match snap {
        Snap::Point(p) if p != id => Some(ConstraintKind::Coincident { a: id.to_string(), b: p.clone() }),
        Snap::Curve(c) => Some(ConstraintKind::PointOnCurve { p: id.to_string(), curve: c.clone() }),
        _ => None,
    }
}

/// Point id for a line-tool click: an existing point is shared; otherwise a new point is made
/// (coincident with the origin / on the snapped curve, F16). Returns (id, newly created).
fn resolve_point(sk: &mut Sketch, vals: &DimensionValues, pos: [f64; 2], snap: &Snap) -> (String, bool) {
    if let Snap::Point(p) = snap {
        if p != ORIGIN && sk.point(p).is_some() {
            return (p.clone(), false);
        }
    }
    let id = sk.add_point(pos[0], pos[1], None);
    if let Some(k) = snap_constraint(&id, snap) {
        add_if_independent(sk, vals, k);
    }
    (id, true)
}

fn set_owner(sk: &mut Sketch, point: &str, owner: &str) {
    if let Some(e) = sk.entity_mut(point) {
        e.owner.get_or_insert_with(|| owner.to_string());
    }
}

/// Line endpoint / circle centre ids of a curve.
fn line_ends(sk: &Sketch, id: &str) -> Option<(String, String)> {
    match &sk.entity(id)?.kind {
        EntityKind::Line { p1, p2 } => Some((p1.clone(), p2.clone())),
        _ => None,
    }
}

impl AppState {
    // ------------------------------------------------------------------ hit testing
    /// What is under `pos` (sketch mm): points first, then dimension labels and constraint
    /// glyphs, then curves.
    pub(crate) fn sketch_hit(&self, pos: [f64; 2], tol: f64, f: HitFilter) -> Option<Pick> {
        let s = self.active_sketch()?;
        let r = self.model.sketches.get(&s.feature)?;
        let sk = &r.solved;
        if f.points {
            let ex: &[&str] = if f.origin { &[] } else { &[ORIGIN] };
            if let Some((id, _)) = nearest_point(sk, pos, tol, ex) {
                return Some(Pick::SketchEntity(id));
            }
        }
        if f.annotations {
            if self.settings.show_dimensions {
                let best = sk
                    .constraints
                    .iter()
                    .filter(|c| c.kind.is_dimension())
                    .map(|c| (c.id.clone(), dist(pos, c.label.unwrap_or_else(|| default_label(sk, &c.kind)))))
                    .filter(|(_, d)| *d <= 2.0 * tol)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((id, _)) = best {
                    return Some(Pick::SketchConstraint(id));
                }
            }
            if self.settings.show_constraints {
                let best = constraint_glyphs(sk, s.tolerance).into_iter().map(|(id, p, _)| (id, dist(pos, p))).filter(|(_, d)| *d <= tol).min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((id, _)) = best {
                    return Some(Pick::SketchConstraint(id));
                }
            }
        }
        let accept = |e: &Entity| match e.kind {
            EntityKind::Line { .. } => f.lines,
            EntityKind::Circle { .. } | EntityKind::Arc { .. } => f.circles,
            EntityKind::Point { .. } => false,
        };
        nearest_curve(sk, pos, tol, accept).map(|(id, _, _)| Pick::SketchEntity(id))
    }

    /// Snapped cursor position: existing point (incl. origin) > curve > grid.
    pub(crate) fn snap_position(&self, p: [f64; 2], tol: f64, exclude: &[String]) -> ([f64; 2], Snap) {
        let Some(r) = self.sketch_result() else { return (p, Snap::None) };
        let sk = &r.solved;
        let ex: Vec<&str> = exclude.iter().map(String::as_str).collect();
        if let Some((id, _)) = nearest_point(sk, p, tol, &ex) {
            return (sk.point(&id).unwrap_or(p), Snap::Point(id));
        }
        if let Some((id, q, _)) = nearest_curve(sk, p, tol, |e| !e.points().iter().any(|q| ex.contains(q))) {
            return (q, Snap::Curve(id));
        }
        if self.settings.snap && self.settings.grid {
            return (snap_to_grid(p, nice_step(tol)), Snap::None);
        }
        (p, Snap::None)
    }

    fn chain_last_pos(&self) -> Option<[f64; 2]> {
        let s = self.active_sketch()?;
        let ch = s.chain.as_ref()?;
        match &ch.last {
            Some(id) => self.doc_sketch()?.point(id),
            None => Some(ch.first.0),
        }
    }

    // ------------------------------------------------------------------ creation tools
    pub(crate) fn create_pointer(&mut self, tool: SketchTool, ev: SketchPointer) {
        let tol = ev.tolerance;
        let exclude: Vec<String> = match self.active_sketch().and_then(|s| s.chain.as_ref()) {
            Some(LineChain { last: Some(l), .. }) => vec![l.clone()],
            _ => Vec::new(),
        };
        let (mut pos, snap) = self.snap_position(ev.pos, tol, &exclude);
        let mut inferred = None;
        if tool == SketchTool::Line {
            if let Some(from) = self.chain_last_pos() {
                inferred = infer_axis(from, pos, HV_INFERENCE_DEG);
                if let (Some(ax), Snap::None) = (inferred, &snap) {
                    pos = project_to_axis(from, pos, ax);
                }
            }
        }
        self.hover = match &snap {
            Snap::Point(id) | Snap::Curve(id) => Some(Pick::SketchEntity(id.clone())),
            Snap::None => None,
        };
        let Some(s) = self.session_mut() else { return };
        s.cursor = Some(pos);
        s.snap = snap.clone();
        s.inferred = inferred;
        // Center Point Arc: follow the cursor around the centre so the arc runs the way it moved
        if tool == SketchTool::ArcCenter && s.clicks.len() == 2 {
            let c = s.clicks[0];
            let angle_of = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
            let last = s.arc_last.unwrap_or_else(|| angle_of(s.clicks[1]));
            let a = angle_of(pos);
            if dist(pos, c) > 1e-9 {
                s.arc_turn += (a - last + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
                s.arc_last = Some(a);
            }
        }
        match ev.kind {
            PointerKind::Move => {}
            PointerKind::Press => {
                s.press = Some((ev.pos, None));
                s.dragged = false;
            }
            PointerKind::Drag => s.dragged = true,
            PointerKind::Release => {
                let press = s.press.take().map(|p| p.0);
                let dragged = std::mem::replace(&mut s.dragged, false);
                let idle = s.clicks.is_empty() && s.chain.is_none();
                // press-drag-release draws like two clicks
                if let (true, Some(p0)) = (dragged, press) {
                    if dist(p0, ev.pos) > tol {
                        if idle {
                            let (pp, ps) = self.snap_position(p0, tol, &exclude);
                            self.creation_click(tool, pp, ps);
                        }
                        self.creation_click(tool, pos, snap);
                    }
                }
            }
            PointerKind::Click => self.creation_click(tool, pos, snap),
            PointerKind::DoubleClick => {
                if tool == SketchTool::Line {
                    // the first click of the double-click already placed the point
                    let at_last = self.chain_last_pos().is_some_and(|l| dist(l, pos) <= tol);
                    if !at_last {
                        self.creation_click(tool, pos, snap);
                    }
                    if let Some(s) = self.session_mut() {
                        s.chain = None;
                        s.inferred = None;
                    }
                }
            }
        }
    }

    fn creation_click(&mut self, tool: SketchTool, pos: [f64; 2], snap: Snap) {
        let result = match tool {
            SketchTool::Line => self.line_click(pos, snap),
            SketchTool::Point => self.edit_active_sketch(|sk, vals| {
                let id = sk.add_point(pos[0], pos[1], None);
                if let Some(k) = snap_constraint(&id, &snap) {
                    add_if_independent(sk, vals, k);
                }
                Ok(())
            }),
            t if t.clicks_needed().is_some() => {
                let needed = t.clicks_needed().unwrap_or(2);
                let Some(s) = self.session_mut() else { return };
                if s.clicks.last().is_some_and(|l| dist(*l, pos) < 1e-9) {
                    return;
                }
                s.clicks.push(pos);
                s.click_snaps.push(snap);
                if s.clicks.len() < needed {
                    return;
                }
                let clicks = std::mem::take(&mut s.clicks);
                let snaps = std::mem::take(&mut s.click_snaps);
                let ccw = s.arc_turn >= 0.0;
                s.arc_turn = 0.0;
                s.arc_last = None;
                match tool {
                    SketchTool::Rectangle2Pt | SketchTool::RectangleCenter | SketchTool::Circle | SketchTool::Arc3Pt => self.finish_shape(tool, clicks, snaps),
                    _ => self.finish_more_shape(tool, clicks, snaps, ccw),
                }
            }
            _ => Ok(()),
        };
        if let Err(e) = result {
            self.warn(e);
        }
    }

    fn line_click(&mut self, pos: [f64; 2], snap: Snap) -> Result<(), String> {
        let Some(chain) = self.active_sketch().map(|s| s.chain.clone()) else { return Ok(()) };
        let Some(chain) = chain else {
            if let Some(s) = self.session_mut() {
                s.chain = Some(LineChain { first: (pos, snap), start: None, last: None });
            }
            return Ok(());
        };
        let last_pos = self.chain_last_pos().unwrap_or(chain.first.0);
        if dist(pos, last_pos) < 1e-9 || (chain.last.is_some() && matches!(&snap, Snap::Point(p) if Some(p) == chain.last.as_ref())) {
            return Ok(());
        }
        let (start, end, closed) = self.edit_active_sketch(|sk, vals| {
            let (last, last_new) = match &chain.last {
                Some(id) => (id.clone(), false),
                None => resolve_point(sk, vals, chain.first.0, &chain.first.1),
            };
            let start = chain.start.clone().unwrap_or_else(|| last.clone());
            let closed = matches!(&snap, Snap::Point(p) if *p == start);
            let (end, end_new) = resolve_point(sk, vals, pos, &snap);
            let line = sk.add_line_between(&last, &end);
            if last_new {
                set_owner(sk, &last, &line);
            }
            if end_new {
                set_owner(sk, &end, &line);
            }
            // F15: horizontal / vertical inference
            if let (Some(a), Some(b)) = (sk.point(&last), sk.point(&end)) {
                if let Some(ax) = infer_axis(a, b, HV_INFERENCE_DEG) {
                    let k = match ax {
                        Axis::Horizontal => ConstraintKind::Horizontal { line: line.clone() },
                        Axis::Vertical => ConstraintKind::Vertical { line: line.clone() },
                    };
                    add_if_independent(sk, vals, k);
                }
            }
            Ok((start, end, closed))
        })?;
        if let Some(s) = self.session_mut() {
            s.chain = if closed { None } else { Some(LineChain { first: (pos, Snap::None), start: Some(start), last: Some(end) }) };
            if closed {
                s.inferred = None;
            }
        }
        Ok(())
    }

    fn finish_shape(&mut self, tool: SketchTool, c: Vec<[f64; 2]>, snaps: Vec<Snap>) -> Result<(), String> {
        match tool {
            SketchTool::Rectangle2Pt | SketchTool::RectangleCenter => {
                let center = tool == SketchTool::RectangleCenter;
                let b = c[1];
                let a = if center { [2.0 * c[0][0] - b[0], 2.0 * c[0][1] - b[1]] } else { c[0] };
                if (b[0] - a[0]).abs() < 1e-6 || (b[1] - a[1]).abs() < 1e-6 {
                    return Err("The rectangle has no area".into());
                }
                self.edit_active_sketch(|sk, vals| {
                    // F13: the rectangle tool adds 2 horizontal + 2 vertical constraints
                    let lines = sk.add_rectangle(a, b);
                    let ca = line_ends(sk, &lines[0]).map(|e| e.0).unwrap_or_default();
                    let cb = line_ends(sk, &lines[2]).map(|e| e.0).unwrap_or_default();
                    if center {
                        let diag = sk.add_line_between(&ca, &cb);
                        if let Some(e) = sk.entity_mut(&diag) {
                            e.construction = true;
                        }
                        match &snaps[0] {
                            Snap::Point(p) => {
                                add_if_independent(sk, vals, ConstraintKind::Midpoint { p: p.clone(), line: diag.clone() });
                            }
                            other => {
                                let m = sk.add_point(c[0][0], c[0][1], Some(&diag));
                                sk.add_constraint(ConstraintKind::Midpoint { p: m.clone(), line: diag.clone() });
                                if let Some(k) = snap_constraint(&m, other) {
                                    add_if_independent(sk, vals, k);
                                }
                            }
                        }
                    } else if let Some(k) = snap_constraint(&ca, &snaps[0]) {
                        add_if_independent(sk, vals, k);
                    }
                    if let Some(k) = snap_constraint(&cb, &snaps[1]) {
                        add_if_independent(sk, vals, k);
                    }
                    Ok(())
                })
            }
            SketchTool::Circle => {
                let r = dist(c[0], c[1]);
                if r < 1e-6 {
                    return Err("The circle has no size".into());
                }
                self.edit_active_sketch(|sk, vals| {
                    let id = sk.add_circle(c[0], r);
                    let cp = match &sk.entity(&id).map(|e| e.kind.clone()) {
                        Some(EntityKind::Circle { c, .. }) => c.clone(),
                        _ => String::new(),
                    };
                    if let Some(k) = snap_constraint(&cp, &snaps[0]) {
                        add_if_independent(sk, vals, k);
                    }
                    // F16: a radius point landing on a point puts that point on the circle
                    if let Snap::Point(p) = &snaps[1] {
                        add_if_independent(sk, vals, ConstraintKind::PointOnCurve { p: p.clone(), curve: id.clone() });
                    }
                    Ok(())
                })
            }
            SketchTool::Arc3Pt => self.edit_active_sketch(|sk, vals| {
                let (a, b, m) = (c[0], c[1], c[2]);
                let id = sk.add_arc_3pt(a, m, b).ok_or("The three points lie on a line; no arc can pass through them")?;
                let (sp, ep) = match &sk.entity(&id).map(|e| e.kind.clone()) {
                    Some(EntityKind::Arc { start, end, .. }) => (start.clone(), end.clone()),
                    _ => return Ok(()),
                };
                // add_arc_3pt may swap the ends to keep the arc counter-clockwise
                for (pos, snap) in [(a, &snaps[0]), (b, &snaps[1])] {
                    let pid = if sk.point(&sp).is_some_and(|q| dist(q, pos) < 1e-6) { sp.clone() } else { ep.clone() };
                    if let Some(k) = snap_constraint(&pid, snap) {
                        add_if_independent(sk, vals, k);
                    }
                }
                Ok(())
            }),
            _ => Ok(()),
        }
    }

    // ------------------------------------------------------------------ select tool
    pub(crate) fn select_pointer(&mut self, ev: SketchPointer) {
        let tol = ev.tolerance;
        match ev.kind {
            PointerKind::Move => {
                if self.active_sketch().is_some_and(|s| s.drag.is_none()) {
                    self.hover = self.sketch_hit(ev.pos, tol, HitFilter::ALL);
                }
            }
            PointerKind::Press => {
                let hit = self.sketch_hit(ev.pos, tol, HitFilter::ALL);
                if let Some(s) = self.session_mut() {
                    s.press = Some((ev.pos, hit));
                    s.dragged = false;
                    s.drag = None;
                }
            }
            PointerKind::Drag => {
                let Some(s) = self.active_sketch() else { return };
                if !s.dragged {
                    let press = s.press.clone();
                    if let Some(s) = self.session_mut() {
                        s.dragged = true;
                    }
                    match press {
                        Some((p0, Some(Pick::SketchEntity(id)))) => self.begin_drag(&id, p0),
                        Some((_, Some(Pick::SketchConstraint(c)))) if self.doc_sketch().and_then(|k| k.constraint(&c)).is_some_and(|k| k.kind.is_dimension()) => {
                            if let Some(s) = self.session_mut() {
                                s.label_drag = Some(c);
                            }
                        }
                        _ => {}
                    }
                }
                match self.active_sketch().and_then(|s| s.label_drag.clone()) {
                    Some(c) => self.move_label(&c, ev.pos),
                    None => self.drag_to(ev.pos),
                }
            }
            PointerKind::Release => {
                let mut moved_label = false;
                if let Some(s) = self.session_mut() {
                    s.drag = None;
                    s.press = None;
                    s.dragged = false;
                    moved_label = s.label_drag.take().is_some();
                }
                if moved_label {
                    self.recompute_sketch_edit();
                }
            }
            PointerKind::Click => match self.sketch_hit(ev.pos, tol, HitFilter::ALL) {
                Some(p) => self.select(p, ev.shift || ev.ctrl),
                None => {
                    if !(ev.shift || ev.ctrl) {
                        self.clear_selection();
                    }
                }
            },
            PointerKind::DoubleClick => {
                if let Some(Pick::SketchConstraint(c)) = self.sketch_hit(ev.pos, tol, HitFilter::ALL) {
                    self.open_value_edit(&c);
                }
            }
        }
    }

    fn begin_drag(&mut self, id: &str, press: [f64; 2]) {
        if id == ORIGIN {
            return;
        }
        // fully constrained geometry cannot be dragged
        if self.sketch_result().is_some_and(|r| r.report.fully_constrained.iter().any(|f| f == id)) {
            return;
        }
        let Some(sk) = self.doc_sketch() else { return };
        let Some(e) = sk.entity(id) else { return };
        let (ids, radius_of): (Vec<String>, Option<String>) = match &e.kind {
            EntityKind::Point { .. } => (vec![id.to_string()], None),
            EntityKind::Line { p1, p2 } => (vec![p1.clone(), p2.clone()], None),
            EntityKind::Circle { c, .. } => (vec![c.clone()], Some(id.to_string())),
            EntityKind::Arc { c, start, end, .. } => (vec![c.clone(), start.clone(), end.clone()], None),
        };
        let ids: Vec<String> = ids.into_iter().filter(|p| p != ORIGIN).collect();
        let start: Vec<[f64; 2]> = ids.iter().map(|p| sk.point(p).unwrap_or_default()).collect();
        if let Some(s) = self.session_mut() {
            s.drag = Some(DragState { ids, start, press, radius_of });
        }
    }

    /// Moves the dragged geometry to follow the cursor: solve with the dragged points held;
    /// when that is impossible (e.g. a point on a horizontal line), solve from the moved
    /// position without holding so the geometry follows as closely as the constraints allow.
    fn drag_to(&mut self, pos: [f64; 2]) {
        let Some(d) = self.active_sketch().and_then(|s| s.drag.clone()) else { return };
        let Some(fid) = self.sketch_id() else { return };
        let Some(base) = self.doc_sketch().cloned() else { return };
        let vals = self.dimension_values();
        let delta = [pos[0] - d.press[0], pos[1] - d.press[1]];
        let mut trial = base.clone();
        if let Some(cid) = &d.radius_of {
            let c = trial.point(&d.ids[0]).unwrap_or_default();
            if let Some(Entity { kind: EntityKind::Circle { r, .. }, .. }) = trial.entity_mut(cid) {
                *r = dist(pos, c).max(1e-3);
            }
        } else {
            for (id, p0) in d.ids.iter().zip(&d.start) {
                trial.set_point(id, [p0[0] + delta[0], p0[1] + delta[1]]);
            }
        }
        let hold: Vec<&str> = d.ids.iter().map(String::as_str).collect();
        let mut held = trial.clone();
        let solved = if solve(&mut held, &vals, &hold).status == SolveStatus::Solved {
            Some(held)
        } else {
            let mut free = trial;
            (solve(&mut free, &vals, &[]).status == SolveStatus::Solved).then_some(free)
        };
        if let Some(new) = solved {
            if new != base {
                if let Some(sk) = cad_core::ops::sketch_mut(&mut self.doc, &fid) {
                    *sk = new;
                }
                self.dirty = true;
                self.recompute();
            }
        }
    }

    /// Moves a dimension label while dragging. Only the label changes, so the document and the
    /// displayed solved copy are updated in place; the release recomputes once.
    fn move_label(&mut self, constraint: &str, pos: [f64; 2]) {
        let Some(fid) = self.sketch_id() else { return };
        let set = |sk: &mut Sketch| {
            if let Some(c) = sk.constraints.iter_mut().find(|c| c.id == constraint) {
                c.label = Some(pos);
            }
        };
        if let Some(sk) = cad_core::ops::sketch_mut(&mut self.doc, &fid) {
            set(sk);
        }
        if let Some(r) = self.model.sketches.get_mut(&fid) {
            set(&mut Arc::make_mut(r).solved);
        }
        self.dirty = true;
    }

    /// Deletes the selected sketch entities, dimensions (with their parameters) and constraints.
    pub fn sketch_delete_selection(&mut self) {
        let Some(fid) = self.sketch_id() else { return };
        let mut entities = Vec::new();
        let mut cons = Vec::new();
        for p in &self.selection {
            match p {
                Pick::SketchEntity(e) if e != ORIGIN => entities.push(e.clone()),
                Pick::SketchConstraint(c) => cons.push(c.clone()),
                _ => {}
            }
        }
        if entities.is_empty() && cons.is_empty() {
            self.info("Select sketch geometry, dimensions or constraints to delete.");
            return;
        }
        let n = entities.len() + cons.len();
        let res = self.edit_doc(|d, _, _| {
            for c in &cons {
                cad_core::ops::delete_sketch_constraint(d, &fid, c);
            }
            for e in &entities {
                cad_core::ops::delete_sketch_entity(d, &fid, e);
            }
            Ok(())
        });
        self.selection.retain(|p| !matches!(p, Pick::SketchEntity(_) | Pick::SketchConstraint(_)));
        self.hover = None;
        if let Some(s) = self.session_mut() {
            s.prompt = None;
            s.picks.clear();
            s.pending_dim = None;
        }
        match res {
            Ok(()) => self.info(format!("Deleted {n} sketch item(s)")),
            Err(e) => self.warn(e),
        }
    }

    /// Normal / Construction toggle of the selected curves (shortcut X).
    pub fn toggle_construction(&mut self) {
        let Some(sk) = self.doc_sketch() else { return };
        let ids: Vec<String> = self
            .selection
            .iter()
            .filter_map(|p| match p {
                Pick::SketchEntity(e) if sk.entity(e).is_some_and(|en| en.is_curve()) => Some(e.clone()),
                _ => None,
            })
            .collect();
        if ids.is_empty() {
            self.info("Select sketch curves to switch between normal and construction geometry.");
            return;
        }
        let res = self.edit_active_sketch(|sk, _| {
            for id in &ids {
                if let Some(e) = sk.entity_mut(id) {
                    e.construction = !e.construction;
                }
            }
            Ok(())
        });
        if let Err(e) = res {
            self.warn(e);
        }
    }

    // ------------------------------------------------------------------ dimension tool
    pub(crate) fn start_dimension_tool(&mut self) {
        self.set_sketch_tool(SketchTool::Dimension);
        // Fusion: a preselected entity starts the dimension right away
        let pre: Vec<String> = self.selection.iter().filter_map(|p| if let Pick::SketchEntity(e) = p { Some(e.clone()) } else { None }).collect();
        if pre.is_empty() || pre.len() > 2 {
            return;
        }
        let Some(r) = self.sketch_result() else { return };
        let label = default_label(&r.solved, &ConstraintKind::Horizontal { line: pre[0].clone() });
        let kind = dimension_kind(&r.solved, &pre, label);
        if let Some(s) = self.session_mut() {
            match kind {
                Some(kind) => s.pending_dim = Some(PendingDimension { picks: pre, kind, label }),
                None => s.picks = pre,
            }
        }
        self.selection.clear();
    }

    pub(crate) fn dimension_pointer(&mut self, ev: SketchPointer) {
        let tol = ev.tolerance;
        let Some(r) = self.sketch_result() else { return };
        let sk = &r.solved;
        match ev.kind {
            PointerKind::Move | PointerKind::Drag => {
                let pending = self.active_sketch().is_some_and(|s| s.pending_dim.is_some());
                self.hover = self.sketch_hit(ev.pos, tol, if pending { HitFilter::GEOMETRY } else { HitFilter::ALL });
                if let Some(s) = self.session_mut() {
                    s.cursor = Some(ev.pos);
                    if let Some(pd) = &mut s.pending_dim {
                        pd.label = ev.pos;
                        if let Some(k) = dimension_kind(sk, &pd.picks, ev.pos) {
                            pd.kind = k;
                        }
                    }
                }
            }
            PointerKind::Click => {
                let Some(s) = self.active_sketch() else { return };
                let pending = s.pending_dim.clone();
                let picks = pending.as_ref().map(|p| p.picks.clone()).unwrap_or_else(|| s.picks.clone());
                let hit = match self.sketch_hit(ev.pos, tol, HitFilter::GEOMETRY) {
                    Some(Pick::SketchEntity(id)) if !picks.contains(&id) => Some(id),
                    _ => None,
                };
                if let Some(id) = hit {
                    let mut next = picks.clone();
                    next.push(id);
                    match dimension_kind(sk, &next, ev.pos) {
                        Some(kind) => {
                            if let Some(s) = self.session_mut() {
                                s.pending_dim = Some(PendingDimension { picks: next, kind, label: ev.pos });
                                s.picks.clear();
                            }
                            return;
                        }
                        None if next.len() == 1 => {
                            if let Some(s) = self.session_mut() {
                                s.picks = next;
                            }
                            return;
                        }
                        None if pending.is_none() => {
                            if let Some(s) = self.session_mut() {
                                s.picks.clear();
                            }
                            self.info("These entities cannot be dimensioned together.");
                            return;
                        }
                        None => {}
                    }
                }
                match pending {
                    Some(pd) => {
                        let kind = dimension_kind(sk, &pd.picks, ev.pos).unwrap_or(pd.kind);
                        self.place_dimension(kind, ev.pos);
                    }
                    None => {
                        if !matches!(self.sketch_hit(ev.pos, tol, HitFilter::ALL), Some(Pick::SketchConstraint(_))) {
                            if let Some(s) = self.session_mut() {
                                s.picks.clear();
                            }
                        }
                    }
                }
            }
            PointerKind::DoubleClick => {
                if let Some(Pick::SketchConstraint(c)) = self.sketch_hit(ev.pos, tol, HitFilter::ALL) {
                    self.open_value_edit(&c);
                }
            }
            _ => {}
        }
    }

    /// Places a dimension: over-constraining ones are offered as driven (F18); otherwise the
    /// dimension is created with its measured value and the value box opens.
    pub(crate) fn place_dimension(&mut self, kind: ConstraintKind, label: [f64; 2]) {
        let Some(sk) = self.doc_sketch() else { return };
        let vals = self.dimension_values();
        let measured = measure(sk, &kind).unwrap_or(0.0);
        let over = would_overconstrain(sk, &kind, Some(measured), &vals);
        if let Some(s) = self.session_mut() {
            s.pending_dim = None;
            s.picks.clear();
        }
        if over {
            self.warn(OVER_CONSTRAINED);
            if let Some(s) = self.session_mut() {
                s.prompt = Some(SketchPrompt::Driven { kind, label, measured });
            }
            return;
        }
        let expr = value_text(measured, quantity_of(&kind));
        let res = self.edit_doc(|d, fid, _| cad_core::ops::add_dimension(d, fid, kind, &expr, label, false).ok_or_else(|| "Could not add the dimension".to_string()));
        match res {
            Ok((cid, param)) => {
                if let Some(s) = self.session_mut() {
                    s.prompt = Some(SketchPrompt::Value { constraint: cid, param, text: expr, error: None, focus: true });
                }
            }
            Err(e) => self.warn(e),
        }
    }

    /// Opens the value box of an existing dimension (double-click on its label).
    pub fn open_value_edit(&mut self, constraint: &str) {
        let Some(c) = self.doc_sketch().and_then(|s| s.constraint(constraint)).cloned() else { return };
        if !c.kind.is_dimension() {
            return;
        }
        if c.driven {
            self.info("Driven dimensions show a measured value and cannot be edited.");
            return;
        }
        let Some(param) = c.param.clone() else { return };
        let text = self.doc.parameter(&param).map(|p| p.expression.clone()).unwrap_or_default();
        if let Some(s) = self.session_mut() {
            s.prompt = Some(SketchPrompt::Value { constraint: c.id.clone(), param, text, error: None, focus: true });
        }
    }

    pub fn sketch_prompt(&self) -> Option<&SketchPrompt> {
        self.active_sketch()?.prompt.as_ref()
    }

    /// Text of the open value box (edited by the UI's text field).
    pub fn sketch_value_text_mut(&mut self) -> Option<&mut String> {
        match self.session_mut()?.prompt.as_mut()? {
            SketchPrompt::Value { text, .. } => Some(text),
            SketchPrompt::Driven { .. } => None,
        }
    }

    /// True once after a value box opens (or a value was rejected): the UI focuses the field.
    pub fn sketch_prompt_take_focus(&mut self) -> bool {
        match self.session_mut().and_then(|s| s.prompt.as_mut()) {
            Some(SketchPrompt::Value { focus, .. }) => std::mem::replace(focus, false),
            _ => false,
        }
    }

    pub fn sketch_set_value_text(&mut self, t: &str) {
        if let Some(text) = self.sketch_value_text_mut() {
            *text = t.to_string();
        }
    }

    /// Enter in the value box: sets the dimension's parameter expression. Invalid expressions
    /// ("Expression is invalid", F21) or values the sketch cannot satisfy keep the box open.
    pub fn sketch_value_commit(&mut self) {
        let Some(prompt) = self.sketch_prompt().cloned() else { return };
        let SketchPrompt::Value { param, text, .. } = prompt else {
            self.sketch_accept_driven();
            return;
        };
        let offset = self.active_sketch().and_then(|s| s.offset_edit.clone()).filter(|o| o.param == param);
        let res = self.edit_doc(|d, fid, _| {
            d.set_expression(&param, &text).map(|_| ()).map_err(|e| format!("{param}: {e}"))?;
            if let Some(o) = &offset {
                super::modify::reposition_offset(d, fid, o);
            }
            Ok(())
        });
        match res {
            Ok(()) => {
                if let Some(s) = self.session_mut() {
                    s.prompt = None;
                    s.offset_edit = None;
                }
            }
            Err(e) => {
                if let Some(Some(SketchPrompt::Value { error, focus, .. })) = self.session_mut().map(|s| s.prompt.as_mut()) {
                    *error = Some(e.clone());
                    *focus = true;
                }
                self.error(e);
            }
        }
    }

    /// Esc in the value box / declining the driven offer. A placed dimension keeps its
    /// measured value (Fusion creates the dimension when it is placed).
    pub fn sketch_value_cancel(&mut self) {
        if let Some(s) = self.session_mut() {
            s.prompt = None;
            s.offset_edit = None;
        }
    }

    pub(crate) fn close_prompt(&mut self) {
        self.sketch_value_cancel();
    }

    /// Accepts the offer to add an over-constraining dimension as a driven dimension.
    pub fn sketch_accept_driven(&mut self) {
        let Some(SketchPrompt::Driven { kind, label, measured }) = self.sketch_prompt().cloned() else { return };
        if let Some(s) = self.session_mut() {
            s.prompt = None;
        }
        let expr = value_text(measured, quantity_of(&kind));
        match self.edit_doc(|d, fid, _| cad_core::ops::add_dimension(d, fid, kind, &expr, label, true).ok_or_else(|| "Could not add the dimension".to_string())) {
            Ok(_) => self.info("Created a driven dimension"),
            Err(e) => self.warn(e),
        }
    }

    /// World position of the open prompt (for the UI).
    pub fn sketch_prompt_anchor(&self) -> Option<[f64; 2]> {
        let s = self.active_sketch()?;
        match s.prompt.as_ref()? {
            SketchPrompt::Driven { label, .. } => Some(*label),
            SketchPrompt::Value { constraint, .. } => {
                let sk = self.doc_sketch()?;
                let c = sk.constraint(constraint)?;
                Some(c.label.unwrap_or_else(|| default_label(sk, &c.kind)))
            }
        }
    }
}

/// Dimension type from the picked entities and label position (Fusion's Sketch Dimension:
/// line -> length, two points -> distance, circle -> diameter, arc -> radius, two lines ->
/// angle (distance when parallel), point + line -> point-line distance).
pub(crate) fn dimension_kind(sk: &Sketch, picks: &[String], label: [f64; 2]) -> Option<ConstraintKind> {
    use ConstraintKind::*;
    let cls: Vec<Class> = picks.iter().map(|p| classify(sk, p)).collect::<Option<_>>()?;
    let center = |id: &str| match &sk.entity(id)?.kind {
        EntityKind::Circle { c, .. } | EntityKind::Arc { c, .. } => Some(c.clone()),
        _ => None,
    };
    let orient = |a: &str, b: &str| {
        interact::distance_points(sk, a, b).map(|(pa, pb)| interact::distance_orientation(pa, pb, label)).unwrap_or(DistanceOrientation::Aligned)
    };
    let p2p = |a: String, b: String| {
        let orientation = orient(&a, &b);
        Some(Distance { a, b, orientation })
    };
    let pl = |p: &str, l: &str| {
        let (e1, e2) = line_ends(sk, l)?;
        if p == e1 || p == e2 {
            return None;
        }
        Some(PointLineDistance { p: p.to_string(), line: l.to_string() })
    };
    match (picks, cls.as_slice()) {
        ([l], [Class::Line]) => {
            let orientation = orient(l, l);
            Some(Distance { a: l.clone(), b: l.clone(), orientation })
        }
        ([c], [Class::Circle]) => Some(Diameter { curve: c.clone() }),
        ([c], [Class::Arc]) => Some(Radius { curve: c.clone() }),
        ([_], [Class::Point]) => None,
        ([p, q], [Class::Point, Class::Point]) => p2p(p.clone(), q.clone()),
        ([p, l], [Class::Point, Class::Line]) | ([l, p], [Class::Line, Class::Point]) => pl(p, l),
        ([a, b], [Class::Line, Class::Line]) => {
            let (a1, a2) = sk.line_points(a)?;
            let (b1, b2) = sk.line_points(b)?;
            let (ua, ub) = (interact::unit(interact::sub(a2, a1))?, interact::unit(interact::sub(b2, b1))?);
            if interact::cross(ua, ub).abs() < 1e-6 {
                let (q, _) = line_ends(sk, b)?;
                Some(PointLineDistance { p: q, line: a.clone() })
            } else {
                Some(Angle { a: a.clone(), b: b.clone() })
            }
        }
        ([p, c], [Class::Point, Class::Circle | Class::Arc]) | ([c, p], [Class::Circle | Class::Arc, Class::Point]) => p2p(p.clone(), center(c)?),
        ([a, b], [Class::Circle | Class::Arc, Class::Circle | Class::Arc]) => p2p(center(a)?, center(b)?),
        ([l, c], [Class::Line, Class::Circle | Class::Arc]) | ([c, l], [Class::Circle | Class::Arc, Class::Line]) => pl(&center(c)?, l),
        _ => None,
    }
}
