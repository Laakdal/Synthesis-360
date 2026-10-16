//! Advanced sketch tools.
//!
//! * Shape tools on the click framework of `sketch_tools.rs`: 3-Point Rectangle, 2-Point
//!   Circle, Center Point Arc, Polygon and Slot (geometry in `cad_sketch::shapes`).
//! * Modify tools that pick existing geometry: Trim, Extend, Sketch Fillet, Offset and Mirror
//!   (geometry in `cad_sketch::modify`). Fillet and Offset create their dimension with a model
//!   parameter and open the value box on it, so the radius / distance is typed like any other
//!   dimension value.

use super::tools::{snap_constraint, HitFilter};
use super::*;
use cad_core::sketch::interact::{circle_polyline, dist, nice_step};
use cad_core::sketch::modify::{self as m, Curve};
use cad_core::sketch::shapes::{polygon_vertices, rectangle_3pt_corners, slot_corners, slot_radius};
use cad_core::sketch::{measure, would_overconstrain};

const CURVES: HitFilter = HitFilter { points: false, origin: false, lines: true, circles: true, annotations: false };
const LINES: HitFilter = HitFilter { points: false, origin: false, lines: true, circles: false, annotations: false };

/// An offset whose distance is being entered: the source chain, the side (+1 left / -1 right)
/// and the created curves in chain order.
#[derive(Clone, Debug)]
pub(crate) struct OffsetEdit {
    pub param: String,
    pub chain: m::Chain,
    pub side: f64,
    pub curves: Vec<String>,
}

/// Re-places the offset curves at the distance their parameter now has, so the solve that
/// follows starts from the answer and the source geometry does not move.
pub(crate) fn reposition_offset(d: &mut Document, fid: &str, o: &OffsetEdit) {
    let Some(v) = d.parameter_values().values.get(&o.param).copied() else { return };
    let Some(sk) = cad_core::ops::sketch_mut(d, fid) else { return };
    let Ok(geo) = m::offset_geometry(sk, &o.chain, o.side * v.abs()) else { return };
    for (id, g) in o.curves.iter().zip(&geo) {
        let Some(kind) = sk.entity(id).map(|e| e.kind.clone()) else { continue };
        match (kind, g) {
            (EntityKind::Line { p1, p2 }, m::OffsetCurve::Line { a, b }) => {
                sk.set_point(&p1, *a);
                sk.set_point(&p2, *b);
            }
            (EntityKind::Arc { start, end, .. }, m::OffsetCurve::Arc { r, from, to, ccw, .. }) => {
                let (s, e) = if *ccw { (*from, *to) } else { (*to, *from) };
                sk.set_point(&start, s);
                sk.set_point(&end, e);
                if let Some(Entity { kind: EntityKind::Arc { r: rr, .. }, .. }) = sk.entity_mut(id) {
                    *rr = *r;
                }
            }
            (EntityKind::Circle { .. }, m::OffsetCurve::Circle { r, .. }) => {
                if let Some(Entity { kind: EntityKind::Circle { r: rr, .. }, .. }) = sk.entity_mut(id) {
                    *rr = *r;
                }
            }
            _ => {}
        }
    }
}

/// Names of the model parameters a sketch's dimensions use.
fn sketch_params(d: &Document, fid: &str) -> Vec<String> {
    d.feature(fid).and_then(|f| f.sketch()).map(|s| s.constraints.iter().filter_map(|c| c.param.clone()).collect()).unwrap_or_default()
}

fn point_of(sk: &Sketch, curve: &str, which: usize) -> Option<String> {
    sk.entity(curve).and_then(|e| e.points().get(which).map(|p| p.to_string()))
}

/// Polyline of an arc around `c` from `s` to `e`, counter-clockwise or clockwise.
fn arc_polyline(c: [f64; 2], s: [f64; 2], e: [f64; 2], ccw: bool) -> Vec<[f64; 2]> {
    let (s, e) = if ccw { (s, e) } else { (e, s) };
    let r = dist(c, s);
    let a0 = (s[1] - c[1]).atan2(s[0] - c[0]);
    let mut sweep = ((e[1] - c[1]).atan2(e[0] - c[0]) - a0).rem_euclid(std::f64::consts::TAU);
    if sweep < 1e-9 {
        sweep = std::f64::consts::TAU;
    }
    Curve::Arc { c, r, a0, sweep }.polyline(0.0, 1.0)
}

fn slot_outline(c1: [f64; 2], c2: [f64; 2], r: f64) -> Option<Vec<[f64; 2]>> {
    let k = slot_corners(c1, c2, r)?;
    let mut out = vec![k[0], k[1]];
    out.extend(arc_polyline(c2, k[1], k[2], true));
    out.push(k[3]);
    out.extend(arc_polyline(c1, k[3], k[0], true));
    Some(out)
}

fn closed(mut pts: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    if let Some(f) = pts.first().copied() {
        pts.push(f);
    }
    pts
}

impl AppState {
    // ------------------------------------------------------------------ shape tools
    pub(crate) fn finish_more_shape(&mut self, tool: SketchTool, c: Vec<[f64; 2]>, snaps: Vec<Snap>, ccw: bool) -> Result<(), String> {
        let (sides, inscribed) = self.active_sketch().map(|s| (s.polygon_sides, s.polygon_inscribed)).unwrap_or((6, true));
        let tie = |sk: &mut Sketch, vals: &DimensionValues, id: &Option<String>, snap: &Snap| {
            if let Some(k) = id.as_ref().and_then(|id| snap_constraint(id, snap)) {
                add_if_independent(sk, vals, k);
            }
        };
        match tool {
            SketchTool::Circle2Pt => {
                let r = dist(c[0], c[1]) / 2.0;
                if r < 1e-6 {
                    return Err("The circle has no size".into());
                }
                self.edit_active_sketch(|sk, vals| {
                    let id = sk.add_circle(interact::lerp(c[0], c[1], 0.5), r);
                    // both picked points lie on the circle
                    for snap in &snaps {
                        if let Snap::Point(p) = snap {
                            add_if_independent(sk, vals, ConstraintKind::PointOnCurve { p: p.clone(), curve: id.clone() });
                        }
                    }
                    Ok(())
                })
            }
            SketchTool::Rectangle3Pt => {
                rectangle_3pt_corners(c[0], c[1], c[2]).ok_or("The rectangle has no area")?;
                self.edit_active_sketch(|sk, vals| {
                    let lines = sk.add_rectangle_3pt(vals, c[0], c[1], c[2]).ok_or("The rectangle has no area")?;
                    let (a, b) = (point_of(sk, &lines[0], 0), point_of(sk, &lines[0], 1));
                    tie(sk, vals, &a, &snaps[0]);
                    tie(sk, vals, &b, &snaps[1]);
                    Ok(())
                })
            }
            SketchTool::ArcCenter => self.edit_active_sketch(|sk, vals| {
                let (arc, sp, ep) = sk.add_arc_center(c[0], c[1], c[2], ccw).ok_or("The arc has no size")?;
                tie(sk, vals, &point_of(sk, &arc, 0), &snaps[0]);
                tie(sk, vals, &Some(sp), &snaps[1]);
                // the end lies on the circle: a snapped point is only joined when it is there
                if let Snap::Point(p) = &snaps[2] {
                    if sk.point(p).zip(sk.point(&ep)).is_some_and(|(a, b)| dist(a, b) < 1e-6) {
                        add_if_independent(sk, vals, ConstraintKind::Coincident { a: ep, b: p.clone() });
                    }
                }
                Ok(())
            }),
            SketchTool::Polygon => self.edit_active_sketch(|sk, vals| {
                let shape = sk.add_polygon(vals, c[0], c[1], sides, inscribed).ok_or("The polygon has no size")?;
                tie(sk, vals, &Some(shape.center), &snaps[0]);
                Ok(())
            }),
            SketchTool::Slot => {
                let r = slot_radius(c[0], c[1], c[2]);
                if dist(c[0], c[1]) < 1e-6 || r < 1e-6 {
                    return Err("The slot has no size".into());
                }
                self.edit_active_sketch(|sk, vals| {
                    let slot = sk.add_slot(vals, c[0], c[1], r).ok_or("The slot has no size")?;
                    tie(sk, vals, &Some(slot.centers[0].clone()), &snaps[0]);
                    tie(sk, vals, &Some(slot.centers[1].clone()), &snaps[1]);
                    Ok(())
                })
            }
            _ => Ok(()),
        }
    }

    pub fn polygon_options(&self) -> Option<(usize, bool)> {
        self.active_sketch().map(|s| (s.polygon_sides, s.polygon_inscribed))
    }

    /// Polygon tool: number of sides (3..=64) and inscribed (vertex at the click) or
    /// circumscribed (edge midpoint at the click).
    pub fn set_polygon_options(&mut self, sides: usize, inscribed: bool) {
        if let Some(s) = self.session_mut() {
            s.polygon_sides = sides.clamp(3, 64);
            s.polygon_inscribed = inscribed;
        }
    }

    // ------------------------------------------------------------------ modify tools
    /// Fillet / Offset / Mirror: a valid pre-selection is used right away (Fusion).
    pub(crate) fn start_modify_tool(&mut self, tool: SketchTool) {
        let pre: Vec<String> = self.selection.iter().filter_map(|p| if let Pick::SketchEntity(e) = p { Some(e.clone()) } else { None }).collect();
        self.set_sketch_tool(tool);
        let Some(sk) = self.doc_sketch().cloned() else { return };
        let curves: Vec<String> = pre.iter().filter(|e| sk.entity(e).is_some_and(|en| en.is_curve())).cloned().collect();
        match tool {
            SketchTool::Mirror if !curves.is_empty() => {
                if let Some(s) = self.session_mut() {
                    s.picks = curves;
                    s.stage = 1;
                }
                self.selection.clear();
            }
            SketchTool::Offset if !curves.is_empty() => {
                if let Some(s) = self.session_mut() {
                    s.picks = vec![curves[0].clone()];
                }
                self.selection.clear();
            }
            SketchTool::Fillet => {
                let lines: Vec<String> = curves.iter().filter(|c| m::fillettable(&sk, c)).cloned().collect();
                let corner = pre.iter().find_map(|p| m::corner_curves(&sk, p).map(|c| (c, p.clone())));
                let pair = match (lines.as_slice(), corner) {
                    ([a, b], _) => Some(([a.clone(), b.clone()], None)),
                    (_, Some((c, p))) => Some((c, Some(p))),
                    _ => None,
                };
                if let Some(([a, b], p)) = pair {
                    self.selection.clear();
                    self.fillet(&a, &b, p.as_deref());
                }
            }
            _ => {}
        }
    }

    /// Applies a Modify edit to the active sketch and drops the parameters of dimensions it
    /// removed.
    fn edit_pruned<R>(&mut self, f: impl FnOnce(&mut Sketch, &DimensionValues) -> Result<R, String>) -> Result<R, String> {
        self.edit_doc(|d, fid, vals| {
            let before = sketch_params(d, fid);
            let sk = cad_core::ops::sketch_mut(d, fid).ok_or("The sketch no longer exists")?;
            let r = f(sk, vals)?;
            let after = sketch_params(d, fid);
            d.parameters.retain(|p| !(before.contains(&p.name) && !after.contains(&p.name)));
            Ok(r)
        })
    }

    fn hit_id(&self, pos: [f64; 2], tol: f64, f: HitFilter) -> Option<String> {
        match self.sketch_hit(pos, tol, f) {
            Some(Pick::SketchEntity(id)) => Some(id),
            _ => None,
        }
    }

    pub(crate) fn modify_pointer(&mut self, tool: SketchTool, ev: SketchPointer) {
        let tol = ev.tolerance;
        let stage = self.active_sketch().map(|s| s.stage).unwrap_or(0);
        let picks = self.active_sketch().map(|s| s.picks.clone()).unwrap_or_default();
        let filter = match tool {
            SketchTool::Fillet => HitFilter { points: picks.is_empty(), origin: false, lines: true, circles: true, annotations: false },
            SketchTool::Mirror if stage == 1 => LINES,
            SketchTool::Mirror => HitFilter { points: true, origin: false, lines: true, circles: true, annotations: false },
            SketchTool::Offset if !picks.is_empty() => HitFilter { points: false, origin: false, lines: false, circles: false, annotations: false },
            _ => CURVES,
        };
        let hit = self.hit_id(ev.pos, tol, filter);
        match ev.kind {
            PointerKind::Move | PointerKind::Drag => {
                self.hover = hit.map(Pick::SketchEntity);
                if let Some(s) = self.session_mut() {
                    s.cursor = Some(ev.pos);
                }
            }
            PointerKind::Click => {
                let res = match tool {
                    SketchTool::Trim => self.trim_at(hit, ev.pos),
                    SketchTool::Extend => self.extend_at(hit, ev.pos),
                    SketchTool::Fillet => self.fillet_click(hit),
                    SketchTool::Offset => self.offset_click(hit, ev.pos, tol),
                    SketchTool::Mirror => self.mirror_click(hit, ev.shift || ev.ctrl),
                    _ => Ok(()),
                };
                if let Err(e) = res {
                    self.warn(e);
                }
            }
            _ => {}
        }
    }

    fn trim_at(&mut self, hit: Option<String>, pos: [f64; 2]) -> Result<(), String> {
        let id = hit.ok_or("Trim: click on a curve")?;
        let plan = self.doc_sketch().and_then(|sk| m::trim_plan(sk, &id, pos)).ok_or("Trim: this curve cannot be trimmed")?;
        self.edit_pruned(|sk, vals| m::apply_trim(sk, vals, &plan))?;
        self.hover = None;
        self.selection.retain(|p| !matches!(p, Pick::SketchEntity(e) if *e == id));
        Ok(())
    }

    fn extend_at(&mut self, hit: Option<String>, pos: [f64; 2]) -> Result<(), String> {
        let id = hit.ok_or("Extend: click near the end of a line or arc")?;
        let plan = self.doc_sketch().and_then(|sk| m::extend_plan(sk, &id, pos)).ok_or("Extend: no curve to extend to")?;
        self.edit_pruned(|sk, vals| m::apply_extend(sk, vals, &plan))
    }

    fn fillet_click(&mut self, hit: Option<String>) -> Result<(), String> {
        let id = hit.ok_or("Sketch Fillet: click a corner, a line or an arc")?;
        let sk = self.doc_sketch().cloned().ok_or("No sketch is being edited")?;
        if sk.point(&id).is_some() {
            let [a, b] = m::corner_curves(&sk, &id).ok_or("Sketch Fillet: the point must be the corner of exactly two lines or arcs")?;
            self.fillet(&a, &b, Some(&id));
            return Ok(());
        }
        let first = self.active_sketch().and_then(|s| s.picks.first().cloned());
        match first {
            Some(a) if a != id => {
                if let Some(s) = self.session_mut() {
                    s.picks.clear();
                }
                self.fillet(&a, &id, None);
            }
            _ => {
                if let Some(s) = self.session_mut() {
                    s.picks = vec![id];
                }
            }
        }
        Ok(())
    }

    /// Radius a new fillet starts with: the last fillet's value when it fits, else a round
    /// value that does.
    fn fillet_radius(&self, sk: &Sketch, a: &str, b: &str, corner: Option<&str>) -> Option<f64> {
        let max = m::fillet_plan_at(sk, a, b, corner, 1e-6).ok()?.max_radius;
        let last = self.active_sketch().and_then(|s| s.fillet_param.clone()).and_then(|p| self.doc.parameter_values().values.get(&p).copied());
        match last {
            Some(r) if r > 0.0 && r < max => Some(r),
            _ => m::default_fillet_radius(sk, a, b, corner),
        }
    }

    /// Fillets the corner of two curves (lines or arcs) and opens the value box on the new radius dimension.
    pub(crate) fn fillet(&mut self, a: &str, b: &str, corner: Option<&str>) {
        let Some(sk) = self.doc_sketch().cloned() else { return };
        let plan = self.fillet_radius(&sk, a, b, corner).ok_or_else(|| "Sketch Fillet: the curves do not form a corner".to_string()).and_then(|r| m::fillet_plan_at(&sk, a, b, corner, r));
        let plan = match plan {
            Ok(p) => p,
            Err(e) => return self.warn(e),
        };
        let expr = value_text(plan.radius, Quantity::Length);
        let label = interact::add(plan.center, [plan.radius * 0.7 + 3.0, plan.radius * 0.7 + 3.0]);
        let res = self.edit_doc(|d, fid, vals| {
            let before = sketch_params(d, fid);
            let sk = cad_core::ops::sketch_mut(d, fid).ok_or("The sketch no longer exists")?;
            let arc = m::apply_fillet(sk, vals, &plan)?;
            let after = sketch_params(d, fid);
            d.parameters.retain(|p| !(before.contains(&p.name) && !after.contains(&p.name)));
            cad_core::ops::add_dimension(d, fid, ConstraintKind::Radius { curve: arc }, &expr, label, false).ok_or_else(|| "Could not add the fillet radius".to_string())
        });
        match res {
            Ok((cid, param)) => {
                if let Some(s) = self.session_mut() {
                    s.fillet_param = Some(param.clone());
                    s.prompt = Some(SketchPrompt::Value { constraint: cid, param, text: expr, error: None, focus: true });
                }
            }
            Err(e) => self.warn(e),
        }
    }

    /// Offset distance at the cursor: signed (left of the chain positive), rounded to the grid
    /// step when snapping.
    fn offset_distance(&self, sk: &Sketch, chain: &m::Chain, pos: [f64; 2], tol: f64) -> Option<f64> {
        let d = m::offset_side(sk, chain, pos)?;
        if self.settings.snap {
            let step = nice_step(tol);
            let r = (d.abs() / step).round().max(1.0) * step;
            return Some(r.copysign(d));
        }
        Some(d)
    }

    fn offset_click(&mut self, hit: Option<String>, pos: [f64; 2], tol: f64) -> Result<(), String> {
        let source = self.active_sketch().and_then(|s| s.picks.first().cloned());
        let Some(source) = source else {
            let id = hit.ok_or("Offset: select a curve")?;
            if let Some(s) = self.session_mut() {
                s.picks = vec![id];
            }
            return Ok(());
        };
        let sk = self.doc_sketch().cloned().ok_or("No sketch is being edited")?;
        let chain = m::chain_of(&sk, &source).ok_or("Offset: the curve no longer exists")?;
        let d = self.offset_distance(&sk, &chain, pos, tol).ok_or("Offset: move to the side to offset to")?;
        let res = self.edit_doc(|doc, fid, vals| {
            let sk = cad_core::ops::sketch_mut(doc, fid).ok_or("The sketch no longer exists")?;
            let made = m::apply_offset(sk, vals, &chain, d)?;
            // one offset distance: the first line's dimension drives the others by expression
            let snapshot = sk.clone();
            let mut master: Option<(String, String)> = None;
            for (new, src) in &made.pairs {
                let Some(p) = point_of(&snapshot, new, 0) else { continue };
                if snapshot.line_points(new).is_none() {
                    continue;
                }
                let kind = ConstraintKind::PointLineDistance { p: p.clone(), line: src.clone() };
                let cur = doc.feature(fid).and_then(|f| f.sketch()).cloned().unwrap_or_default();
                if would_overconstrain(&cur, &kind, measure(&cur, &kind), vals) {
                    continue;
                }
                let label = cur.point(&p).map(|q| interact::add(q, [2.0, 2.0])).unwrap_or_default();
                let expr = match &master {
                    Some((_, param)) => param.clone(),
                    None => value_text(d.abs(), Quantity::Length),
                };
                let added = cad_core::ops::add_dimension(doc, fid, kind, &expr, label, false).ok_or("Could not add the offset dimension")?;
                if master.is_none() {
                    master = Some(added);
                }
            }
            Ok(master.map(|m| (m, made.pairs.iter().map(|p| p.0.clone()).collect::<Vec<_>>())))
        });
        if let Some(s) = self.session_mut() {
            s.picks.clear();
        }
        match res? {
            Some(((cid, param), curves)) => {
                let text = value_text(d.abs(), Quantity::Length);
                if let Some(s) = self.session_mut() {
                    s.offset_edit = Some(OffsetEdit { param: param.clone(), chain, side: d.signum(), curves });
                    s.prompt = Some(SketchPrompt::Value { constraint: cid, param, text, error: None, focus: true });
                }
            }
            None => self.info("Offset created (concentric curves; dimension them to fix the distance)"),
        }
        Ok(())
    }

    /// Mirror clicks. Picking objects: a click toggles a curve; once objects are picked, a click
    /// on a line that is not one of them is the mirror line (Shift / Ctrl + click adds the line
    /// to the objects instead). Enter also ends the object selection.
    fn mirror_click(&mut self, hit: Option<String>, add: bool) -> Result<(), String> {
        let stage = self.active_sketch().map(|s| s.stage).unwrap_or(0);
        let id = hit.ok_or(if stage == 0 { "Mirror: select curves to mirror" } else { "Mirror: select a line as the mirror line" })?;
        let picks = self.active_sketch().map(|s| s.picks.clone()).unwrap_or_default();
        let is_line = self.doc_sketch().is_some_and(|sk| sk.line_points(&id).is_some());
        let axis_click = stage == 0 && !add && is_line && !picks.is_empty() && !picks.contains(&id);
        if stage == 0 && !axis_click {
            if let Some(s) = self.session_mut() {
                match s.picks.iter().position(|p| *p == id) {
                    Some(i) => {
                        s.picks.remove(i);
                    }
                    None => s.picks.push(id),
                }
            }
            return Ok(());
        }
        let objects = self.active_sketch().map(|s| s.picks.clone()).unwrap_or_default();
        let made = self.edit_active_sketch(|sk, vals| m::apply_mirror(sk, vals, &objects, &id))?;
        if let Some(s) = self.session_mut() {
            s.picks.clear();
            s.stage = 0;
        }
        self.info(format!("Mirrored {} object(s)", made.iter().filter(|e| !e.starts_with('p')).count()));
        Ok(())
    }

    // ------------------------------------------------------------------ previews
    /// Rubber-band / hover preview of the new tools.
    pub(crate) fn tool_preview(&self, s: &SketchSession, sk: &Sketch, cur: [f64; 2]) -> Vec<OverlayCurve> {
        let mut out = Vec::new();
        let mut push = |poly: Vec<[f64; 2]>, state: CurveState, dashed: bool| {
            if poly.len() >= 2 {
                out.push(OverlayCurve { id: PREVIEW_ID.into(), polyline: poly, state, dashed });
            }
        };
        let hovered = match &self.hover {
            Some(Pick::SketchEntity(e)) => Some(e.clone()),
            _ => None,
        };
        let c = &s.clicks;
        match (s.tool, c.len()) {
            (SketchTool::Rectangle3Pt | SketchTool::Slot, 1) => push(vec![c[0], cur], CurveState::Preview, s.tool == SketchTool::Slot),
            (SketchTool::Rectangle3Pt, 2) => {
                if let Some(k) = rectangle_3pt_corners(c[0], c[1], cur) {
                    push(closed(k.to_vec()), CurveState::Preview, false);
                }
            }
            (SketchTool::Circle2Pt, 1) => {
                push(circle_polyline(interact::lerp(c[0], cur, 0.5), dist(c[0], cur) / 2.0), CurveState::Preview, false);
                push(vec![c[0], cur], CurveState::Preview, true);
            }
            (SketchTool::ArcCenter, 1) => push(vec![c[0], cur], CurveState::Preview, true),
            (SketchTool::ArcCenter, 2) => {
                let r = dist(c[0], c[1]);
                if let Some(u) = interact::unit(interact::sub(cur, c[0])) {
                    let end = interact::add(c[0], interact::scale(u, r));
                    push(arc_polyline(c[0], c[1], end, s.arc_turn >= 0.0), CurveState::Preview, false);
                    push(vec![c[0], end], CurveState::Preview, true);
                }
            }
            (SketchTool::Polygon, 1) => {
                if let Some(v) = polygon_vertices(c[0], cur, s.polygon_sides, s.polygon_inscribed) {
                    push(closed(v), CurveState::Preview, false);
                    push(circle_polyline(c[0], dist(c[0], cur)), CurveState::Preview, true);
                }
            }
            (SketchTool::Slot, 2) => {
                if let Some(o) = slot_outline(c[0], c[1], slot_radius(c[0], c[1], cur)) {
                    push(o, CurveState::Preview, false);
                    push(vec![c[0], c[1]], CurveState::Preview, true);
                }
            }
            (SketchTool::Trim, _) => {
                if let Some(p) = hovered.and_then(|h| m::trim_plan(sk, &h, cur)) {
                    push(p.removed, CurveState::Error, true);
                }
            }
            (SketchTool::Extend, _) => {
                if let Some(p) = hovered.and_then(|h| m::extend_plan(sk, &h, cur)) {
                    push(p.added, CurveState::Preview, false);
                }
            }
            (SketchTool::Fillet, _) => {
                let pair = match (s.picks.first(), hovered) {
                    (Some(a), Some(h)) if *a != h && m::fillettable(sk, &h) => Some(([a.clone(), h], None)),
                    (None, Some(h)) if sk.point(&h).is_some() => m::corner_curves(sk, &h).map(|c| (c, Some(h))),
                    _ => None,
                };
                if let Some(([a, b], corner)) = pair {
                    let corner = corner.as_deref();
                    if let Some(p) = self.fillet_radius(sk, &a, &b, corner).and_then(|r| m::fillet_plan_at(sk, &a, &b, corner, r).ok()) {
                        push(p.arc, CurveState::Preview, false);
                    }
                }
            }
            (SketchTool::Offset, _) => {
                if let Some(chain) = s.picks.first().and_then(|p| m::chain_of(sk, p)) {
                    if let Some(g) = self.offset_distance(sk, &chain, cur, s.tolerance).and_then(|d| m::offset_geometry(sk, &chain, d).ok()) {
                        for poly in m::offset_polylines(&g) {
                            push(poly, CurveState::Preview, false);
                        }
                    }
                }
            }
            (SketchTool::Mirror, _) if s.stage == 1 || (!s.picks.is_empty() && hovered.as_ref().is_some_and(|h| !s.picks.contains(h))) => {
                if let Some((a, b)) = hovered.and_then(|h| sk.line_points(&h)) {
                    for id in &s.picks {
                        if let Some(e) = sk.entity(id) {
                            let poly = cad_core::sketch::profiles::entity_polyline(sk, e);
                            push(poly.into_iter().map(|p| m::reflect(p, a, b)).collect(), CurveState::Preview, e.construction);
                        }
                    }
                }
            }
            _ => {}
        }
        out
    }
}
