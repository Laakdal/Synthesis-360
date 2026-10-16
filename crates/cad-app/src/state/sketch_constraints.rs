//! Geometric constraint tools (Coincident, Collinear, Concentric, Midpoint, Fix/UnFix,
//! Parallel, Perpendicular, Horizontal/Vertical, Tangent, Equal, Symmetry). Each uses a valid
//! pre-selection directly, otherwise collects clicks; over-constraining constraints are
//! rejected with Fusion's "Sketch geometry is over constrained" (F18).

use super::tools::{classify, Class, HitFilter};
use super::*;
use cad_core::sketch::would_overconstrain;

/// Result of interpreting the picked entities for a constraint tool.
#[derive(Debug)]
pub(crate) enum Build {
    Ready(Vec<ConstraintKind>),
    /// Fix/UnFix on a point that is already fixed: remove these constraints.
    Remove(Vec<String>),
    NeedMore(&'static str),
    Invalid(String),
}

pub(crate) fn hint(t: ConstraintTool, picked: usize) -> &'static str {
    use ConstraintTool::*;
    match (t, picked) {
        (Coincident, 0) => "Coincident: select a point.",
        (Coincident, _) => "Coincident: select a second point or a curve.",
        (Collinear, 0) => "Collinear: select a line.",
        (Collinear, _) => "Collinear: select a second line.",
        (Concentric, 0) => "Concentric: select a circle or arc.",
        (Concentric, _) => "Concentric: select a second circle or arc.",
        (Midpoint, 0) => "Midpoint: select a point and a line.",
        (Midpoint, _) => "Midpoint: select the line (or point).",
        (Fix, _) => "Fix/UnFix: select a point or curve.",
        (Parallel, 0) => "Parallel: select a line.",
        (Parallel, _) => "Parallel: select a second line.",
        (Perpendicular, 0) => "Perpendicular: select a line.",
        (Perpendicular, _) => "Perpendicular: select a second line.",
        (HorizontalVertical, _) => "Horizontal/Vertical: select a line.",
        (Tangent, 0) => "Tangent: select a line, circle or arc.",
        (Tangent, _) => "Tangent: select a circle or arc (or a line).",
        (Equal, 0) => "Equal: select a line, circle or arc.",
        (Equal, _) => "Equal: select a second entity of the same type.",
        (Symmetry, 0) => "Symmetry: select the first point.",
        (Symmetry, 1) => "Symmetry: select the second point.",
        (Symmetry, _) => "Symmetry: select the symmetry line.",
    }
}

/// Which entities the tool accepts as its next pick (drives hover highlighting / picking).
pub(crate) fn filter(t: ConstraintTool, sk: &Sketch, picks: &[String]) -> HitFilter {
    use ConstraintTool::*;
    let f = |points: bool, lines: bool, circles: bool| HitFilter { points, origin: points, lines, circles, annotations: false };
    match t {
        Coincident | Fix => f(true, true, true),
        Collinear | Parallel | Perpendicular | HorizontalVertical => f(false, true, false),
        Concentric => f(false, false, true),
        Equal | Tangent => f(false, true, true),
        Midpoint => match picks.first().and_then(|p| classify(sk, p)) {
            Some(Class::Point) => f(false, true, false),
            Some(Class::Line) => f(true, false, false),
            _ => f(true, true, false),
        },
        Symmetry => {
            if picks.len() < 2 {
                f(true, false, false)
            } else {
                f(false, true, false)
            }
        }
    }
}

pub(crate) fn build(t: ConstraintTool, sk: &Sketch, picks: &[String]) -> Build {
    use Class::*;
    use ConstraintKind as K;
    use ConstraintTool as T;
    if picks.is_empty() {
        return Build::NeedMore(hint(t, 0));
    }
    for (i, p) in picks.iter().enumerate() {
        if picks[..i].contains(p) {
            return Build::Invalid("Select two different entities.".into());
        }
    }
    let Some(cls) = picks.iter().map(|p| classify(sk, p)).collect::<Option<Vec<Class>>>() else {
        return Build::Invalid("The selected geometry no longer exists.".into());
    };
    let p = |i: usize| picks[i].clone();
    let round = |c: Class| matches!(c, Circle | Arc);
    let bad = |what: &str| Build::Invalid(format!("{}: {what}", t.label()));
    match t {
        T::Coincident => match cls.as_slice() {
            [_] => Build::NeedMore(hint(t, 1)),
            [Point, Point] => Build::Ready(vec![K::Coincident { a: p(0), b: p(1) }]),
            [Point, c] if *c != Point => Build::Ready(vec![K::PointOnCurve { p: p(0), curve: p(1) }]),
            [c, Point] if *c != Point => Build::Ready(vec![K::PointOnCurve { p: p(1), curve: p(0) }]),
            _ => bad("select at least one point (point + point or point + curve)."),
        },
        T::Collinear | T::Parallel | T::Perpendicular => match cls.as_slice() {
            [Line] => Build::NeedMore(hint(t, 1)),
            [Line, Line] => Build::Ready(vec![match t {
                T::Collinear => K::Collinear { a: p(0), b: p(1) },
                T::Parallel => K::Parallel { a: p(0), b: p(1) },
                _ => K::Perpendicular { a: p(0), b: p(1) },
            }]),
            _ => bad("select two lines."),
        },
        T::Concentric => match cls.as_slice() {
            [c] if round(*c) => Build::NeedMore(hint(t, 1)),
            [a, b] if round(*a) && round(*b) => Build::Ready(vec![K::Concentric { a: p(0), b: p(1) }]),
            _ => bad("select two circles or arcs."),
        },
        T::Midpoint => match cls.as_slice() {
            [Point] | [Line] => Build::NeedMore(hint(t, 1)),
            [Point, Line] => midpoint(sk, p(0), p(1)),
            [Line, Point] => midpoint(sk, p(1), p(0)),
            _ => bad("select a point and a line."),
        },
        T::Fix => {
            let mut points: Vec<String> = Vec::new();
            for (id, c) in picks.iter().zip(&cls) {
                if id == ORIGIN {
                    return Build::Invalid("The sketch origin is always fixed.".into());
                }
                match c {
                    Point => points.push(id.clone()),
                    _ => points.extend(sk.entity(id).map(|e| e.points().iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap_or_default()),
                }
            }
            points.retain(|q| q != ORIGIN);
            let fixes: Vec<String> = sk.constraints.iter().filter(|c| matches!(&c.kind, K::Fix { p, .. } if points.contains(p))).map(|c| c.id.clone()).collect();
            if !fixes.is_empty() {
                return Build::Remove(fixes);
            }
            Build::Ready(points.iter().filter_map(|q| sk.point(q).map(|xy| K::Fix { p: q.clone(), x: xy[0], y: xy[1] })).collect())
        }
        T::HorizontalVertical => match cls.as_slice() {
            [Line] => {
                let Some((a, b)) = sk.line_points(&picks[0]) else { return bad("select a line.") };
                if (b[0] - a[0]).abs() >= (b[1] - a[1]).abs() {
                    Build::Ready(vec![K::Horizontal { line: p(0) }])
                } else {
                    Build::Ready(vec![K::Vertical { line: p(0) }])
                }
            }
            _ => bad("select a line."),
        },
        T::Tangent => match cls.as_slice() {
            [c] if *c != Point => Build::NeedMore(hint(t, 1)),
            [Line, c] | [c, Line] if round(*c) => Build::Ready(vec![K::Tangent { a: p(0), b: p(1) }]),
            [a, b] if round(*a) && round(*b) => Build::Ready(vec![K::Tangent { a: p(0), b: p(1) }]),
            _ => bad("select a line and a circle/arc, or two circles/arcs."),
        },
        T::Equal => match cls.as_slice() {
            [c] if *c != Point => Build::NeedMore(hint(t, 1)),
            [Line, Line] => Build::Ready(vec![K::Equal { a: p(0), b: p(1) }]),
            [a, b] if round(*a) && round(*b) => Build::Ready(vec![K::Equal { a: p(0), b: p(1) }]),
            _ => bad("select two lines, or two circles/arcs."),
        },
        T::Symmetry => match cls.as_slice() {
            [Point] => Build::NeedMore(hint(t, 1)),
            [Point, Point] => Build::NeedMore(hint(t, 2)),
            [Point, Point, Line] => {
                if line_uses(sk, &picks[2], &picks[0]) || line_uses(sk, &picks[2], &picks[1]) {
                    return bad("the symmetry line cannot pass through its own end points here.");
                }
                Build::Ready(vec![K::Symmetric { p1: p(0), p2: p(1), line: p(2) }])
            }
            _ => bad("select two points, then the symmetry line."),
        },
    }
}

fn line_uses(sk: &Sketch, line: &str, point: &str) -> bool {
    sk.entity(line).is_some_and(|e| e.points().contains(&point))
}

fn midpoint(sk: &Sketch, p: String, line: String) -> Build {
    if line_uses(sk, &line, &p) {
        return Build::Invalid("Midpoint: the point is an end of that line.".into());
    }
    Build::Ready(vec![ConstraintKind::Midpoint { p, line }])
}

impl AppState {
    /// Starts a constraint command: a valid pre-selection is constrained immediately (and the
    /// tool ends), otherwise the tool collects picks on the canvas.
    pub(crate) fn start_constraint(&mut self, t: ConstraintTool) {
        let pre: Vec<String> = self.selection.iter().filter_map(|p| if let Pick::SketchEntity(e) = p { Some(e.clone()) } else { None }).collect();
        self.set_sketch_tool(SketchTool::Constraint(t));
        if pre.is_empty() {
            self.info(hint(t, 0));
            return;
        }
        let Some(r) = self.sketch_result() else { return };
        match build(t, &r.solved, &pre) {
            Build::Ready(kinds) => {
                self.apply_constraints(t, kinds);
                self.selection.clear();
                self.set_sketch_tool(SketchTool::Select);
            }
            Build::Remove(ids) => {
                self.remove_constraints(&ids);
                self.selection.clear();
                self.set_sketch_tool(SketchTool::Select);
            }
            Build::NeedMore(msg) => {
                if let Some(s) = self.session_mut() {
                    s.picks = pre;
                }
                self.selection.clear();
                self.info(msg);
            }
            Build::Invalid(msg) => self.info(msg),
        }
    }

    pub(crate) fn constraint_pointer(&mut self, t: ConstraintTool, ev: SketchPointer) {
        let Some(r) = self.sketch_result() else { return };
        let picks = self.active_sketch().map(|s| s.picks.clone()).unwrap_or_default();
        let f = filter(t, &r.solved, &picks);
        match ev.kind {
            PointerKind::Move | PointerKind::Drag => {
                self.hover = self.sketch_hit(ev.pos, ev.tolerance, f);
                if let Some(s) = self.session_mut() {
                    s.cursor = Some(ev.pos);
                }
            }
            PointerKind::Click => {
                let Some(Pick::SketchEntity(id)) = self.sketch_hit(ev.pos, ev.tolerance, f) else {
                    self.info(hint(t, picks.len()));
                    return;
                };
                let mut next = picks;
                next.push(id);
                self.run_constraint(t, next);
            }
            _ => {}
        }
    }

    fn run_constraint(&mut self, t: ConstraintTool, picks: Vec<String>) {
        let Some(r) = self.sketch_result() else { return };
        let b = build(t, &r.solved, &picks);
        let keep = matches!(b, Build::NeedMore(_));
        if let Some(s) = self.session_mut() {
            s.picks = if keep { picks } else { Vec::new() };
        }
        match b {
            Build::Ready(kinds) => {
                self.apply_constraints(t, kinds);
            }
            Build::Remove(ids) => self.remove_constraints(&ids),
            Build::NeedMore(msg) => self.info(msg),
            Build::Invalid(msg) => self.info(msg),
        }
    }

    /// Adds constraints atomically; rejects them all when one would over-constrain.
    pub(crate) fn apply_constraints(&mut self, t: ConstraintTool, kinds: Vec<ConstraintKind>) -> bool {
        if kinds.is_empty() {
            return false;
        }
        let res = self.edit_active_sketch(|sk, vals| {
            for k in kinds {
                if would_overconstrain(sk, &k, None, vals) {
                    return Err(OVER_CONSTRAINED.to_string());
                }
                sk.add_constraint(k);
            }
            Ok(())
        });
        match res {
            Ok(()) => {
                self.info(format!("Added {} constraint", t.label()));
                true
            }
            Err(e) => {
                self.warn(e);
                false
            }
        }
    }

    fn remove_constraints(&mut self, ids: &[String]) {
        let res = self.edit_doc(|d, fid, _| {
            for c in ids {
                cad_core::ops::delete_sketch_constraint(d, fid, c);
            }
            Ok(())
        });
        match res {
            Ok(()) => self.info("Removed Fix constraint (UnFix)"),
            Err(e) => self.warn(e),
        }
    }
}
