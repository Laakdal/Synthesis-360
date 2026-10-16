//! Sketch Modify operations: Trim, Extend, Sketch Fillet, Offset and Mirror.
//!
//! Each operation is split into a *plan* (pure geometry, also used for the hover preview) and
//! an *apply* step that edits the sketch: points are re-used where curves meet existing points,
//! new end points are tied to the curve they stop at (coincident / point-on-curve), and
//! constraints that no longer describe the edited curve (its length, equal-length, midpoint)
//! are removed. Removing dimension constraints leaves their parameters orphaned; callers working
//! on a document drop those (cad-core's `delete_sketch_*` helpers do the same).

use crate::interact::{add, closest_on_segment, cross, dist, dot, lerp, line_intersection, scale, sub, unit, P2};
use crate::model::{ConstraintKind, Entity, EntityId, EntityKind, Sketch, ORIGIN};
use crate::shapes::add_if_independent;
use crate::solver::DimensionValues;
use std::f64::consts::{PI, TAU};

const EPS: f64 = 1e-7;

/// A curve as an analytic primitive. Parameters run 0..1 from start to end (circles: angle
/// / TAU from the +x axis).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Line { a: P2, b: P2 },
    Circle { c: P2, r: f64 },
    /// Counter-clockwise from angle `a0` over `sweep` radians.
    Arc { c: P2, r: f64, a0: f64, sweep: f64 },
}

fn ang(c: P2, p: P2) -> f64 {
    (p[1] - c[1]).atan2(p[0] - c[0])
}

impl Curve {
    pub fn of(sk: &Sketch, id: &str) -> Option<Curve> {
        Some(match &sk.entity(id)?.kind {
            EntityKind::Line { p1, p2 } => Curve::Line { a: sk.point(p1)?, b: sk.point(p2)? },
            EntityKind::Circle { c, r } => Curve::Circle { c: sk.point(c)?, r: *r },
            EntityKind::Arc { c, start, end, r } => {
                let (cc, s, e) = (sk.point(c)?, sk.point(start)?, sk.point(end)?);
                let a0 = ang(cc, s);
                let mut sweep = (ang(cc, e) - a0).rem_euclid(TAU);
                if sweep < 1e-9 {
                    sweep = TAU;
                }
                Curve::Arc { c: cc, r: *r, a0, sweep }
            }
            EntityKind::Point { .. } => return None,
        })
    }

    pub fn point_at(&self, t: f64) -> P2 {
        match *self {
            Curve::Line { a, b } => lerp(a, b, t),
            Curve::Circle { c, r } => add(c, scale([(TAU * t).cos(), (TAU * t).sin()], r)),
            Curve::Arc { c, r, a0, sweep } => add(c, scale([(a0 + sweep * t).cos(), (a0 + sweep * t).sin()], r)),
        }
    }

    /// Parameter of `p` (projected onto the curve). Lines are unbounded (t < 0 / t > 1 beyond
    /// the ends); arcs give the CCW angle from the start over the sweep (> 1 outside the arc).
    pub fn param_of(&self, p: P2) -> f64 {
        match *self {
            Curve::Line { a, b } => {
                let d = sub(b, a);
                dot(sub(p, a), d) / dot(d, d).max(1e-24)
            }
            Curve::Circle { c, .. } => ang(c, p).rem_euclid(TAU) / TAU,
            Curve::Arc { c, a0, sweep, .. } => (ang(c, p) - a0).rem_euclid(TAU) / sweep,
        }
    }

    /// Polyline of the part between parameters `t0` and `t1` (`t1 < t0` wraps for circles).
    pub fn polyline(&self, t0: f64, t1: f64) -> Vec<P2> {
        match *self {
            Curve::Line { .. } => vec![self.point_at(t0), self.point_at(t1)],
            Curve::Circle { .. } | Curve::Arc { .. } => {
                let (span, total) = match *self {
                    Curve::Circle { .. } => ((t1 - t0).rem_euclid(1.0), TAU),
                    Curve::Arc { sweep, .. } => (t1 - t0, sweep),
                    _ => unreachable!(),
                };
                let span = if span.abs() < 1e-12 && matches!(self, Curve::Circle { .. }) { 1.0 } else { span };
                let n = ((span.abs() * total / (PI / 36.0)).ceil() as usize).max(2);
                (0..=n).map(|i| self.point_at(t0 + span * i as f64 / n as f64)).collect()
            }
        }
    }

    fn circle(&self) -> Option<(P2, f64)> {
        match *self {
            Curve::Circle { c, r } | Curve::Arc { c, r, .. } => Some((c, r)),
            Curve::Line { .. } => None,
        }
    }
    fn line(&self) -> Option<(P2, P2)> {
        match *self {
            Curve::Line { a, b } => Some((a, b)),
            _ => None,
        }
    }

    /// Whether `p` (known to lie on the carrier) lies on the bounded curve.
    pub fn contains(&self, p: P2) -> bool {
        match *self {
            Curve::Line { .. } => {
                let t = self.param_of(p);
                (-EPS..=1.0 + EPS).contains(&t)
            }
            Curve::Circle { .. } => true,
            Curve::Arc { c, r, a0, sweep } => {
                let rel = (ang(c, p) - a0).rem_euclid(TAU);
                rel <= sweep + EPS || TAU - rel < EPS / r.max(1e-9)
            }
        }
    }
}

/// Intersections of the carriers (infinite line / full circle) of two curves.
pub fn carrier_intersections(p: &Curve, q: &Curve) -> Vec<P2> {
    match (p.line(), q.line(), p.circle(), q.circle()) {
        (Some((a1, a2)), Some((b1, b2)), _, _) => line_intersection(a1, a2, b1, b2).into_iter().collect(),
        (Some((a, b)), None, _, Some((c, r))) | (None, Some((a, b)), Some((c, r)), _) => {
            let Some(u) = unit(sub(b, a)) else { return vec![] };
            let f = sub(a, c);
            let t0 = -dot(f, u);
            let h2 = r * r - (dot(f, f) - t0 * t0);
            if h2 < -1e-9 * r.max(1.0) {
                return vec![];
            }
            let h = h2.max(0.0).sqrt();
            if h < 1e-9 {
                return vec![add(a, scale(u, t0))];
            }
            vec![add(a, scale(u, t0 - h)), add(a, scale(u, t0 + h))]
        }
        (None, None, Some((c1, r1)), Some((c2, r2))) => {
            let d = dist(c1, c2);
            if d < 1e-12 || d > r1 + r2 + 1e-9 || d < (r1 - r2).abs() - 1e-9 {
                return vec![];
            }
            let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
            let h = (r1 * r1 - a * a).max(0.0).sqrt();
            let u = scale(sub(c2, c1), 1.0 / d);
            let m = add(c1, scale(u, a));
            let n = [-u[1], u[0]];
            if h < 1e-9 {
                vec![m]
            } else {
                vec![add(m, scale(n, h)), add(m, scale(n, -h))]
            }
        }
        _ => vec![],
    }
}

/// Where another curve meets the edited one.
#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    pub t: f64,
    pub pos: P2,
    pub other: EntityId,
}

/// Every place other curves cross `id`, by parameter. `extended` keeps hits beyond the ends of
/// the edited curve (on its carrier); the other curve is always bounded.
fn cuts(sk: &Sketch, id: &str, extended: bool) -> Vec<Cut> {
    let Some(me) = Curve::of(sk, id) else { return vec![] };
    let mut out: Vec<Cut> = Vec::new();
    for e in sk.entities.iter().filter(|e| e.is_curve() && e.id != id) {
        let Some(other) = Curve::of(sk, &e.id) else { continue };
        for p in carrier_intersections(&me, &other) {
            if !other.contains(p) || (!extended && !me.contains(p)) {
                continue;
            }
            let t = me.param_of(p);
            if !out.iter().any(|c| dist(c.pos, p) < 1e-6) {
                out.push(Cut { t, pos: p, other: e.id.clone() });
            }
        }
    }
    out.sort_by(|a, b| a.t.total_cmp(&b.t));
    out
}

// ---------------------------------------------------------------------------------- trim

/// The segment of a curve Trim removes: the part between the nearest intersections on either
/// side of the picked position (`lo` / `hi` absent = up to the curve's own end). Both absent
/// means the whole curve goes.
#[derive(Clone, Debug)]
pub struct TrimPlan {
    pub curve: EntityId,
    pub lo: Option<Cut>,
    pub hi: Option<Cut>,
    /// Polyline of the removed part (hover preview).
    pub removed: Vec<P2>,
}

pub fn trim_plan(sk: &Sketch, id: &str, at: P2) -> Option<TrimPlan> {
    let me = Curve::of(sk, id)?;
    let all = cuts(sk, id, false);
    let tc = me.param_of(at);
    let (lo, hi) = match me {
        Curve::Circle { .. } => {
            if all.len() < 2 {
                (None, None)
            } else {
                let lo = all.iter().rev().find(|c| c.t < tc).or(all.last()).cloned();
                let hi = all.iter().find(|c| c.t > tc).or(all.first()).cloned();
                (lo, hi)
            }
        }
        _ => {
            let tc = tc.clamp(0.0, 1.0);
            let inner: Vec<&Cut> = all.iter().filter(|c| c.t > 1e-6 && c.t < 1.0 - 1e-6).collect();
            (inner.iter().rev().find(|c| c.t < tc).map(|c| (*c).clone()), inner.iter().find(|c| c.t > tc).map(|c| (*c).clone()))
        }
    };
    let removed = match me {
        Curve::Circle { .. } => match (&lo, &hi) {
            (Some(l), Some(h)) => me.polyline(l.t, h.t),
            _ => me.polyline(0.0, 1.0),
        },
        _ => me.polyline(lo.as_ref().map_or(0.0, |c| c.t), hi.as_ref().map_or(1.0, |c| c.t)),
    };
    Some(TrimPlan { curve: id.to_string(), lo, hi, removed })
}

/// Point for a new curve end at `pos`, where the curve stops at `other`: an existing point
/// there is shared; otherwise a new point is tied to `other` (coincident / point-on-curve).
fn end_point(sk: &mut Sketch, vals: &DimensionValues, pos: P2, other: &str, owner: &str, exclude: &[&str]) -> EntityId {
    let existing = sk.entities.iter().find(|e| matches!(e.kind, EntityKind::Point { x, y } if dist([x, y], pos) < 1e-6) && !exclude.contains(&e.id.as_str())).map(|e| e.id.clone());
    if let Some(p) = existing {
        return p;
    }
    let id = sk.add_point(pos[0], pos[1], Some(owner));
    let kind = if dist(pos, [0.0, 0.0]) < 1e-6 { ConstraintKind::Coincident { a: id.clone(), b: ORIGIN.into() } } else { ConstraintKind::PointOnCurve { p: id.clone(), curve: other.to_string() } };
    add_if_independent(sk, vals, kind);
    id
}

/// Deletes a point no curve uses any more (with the constraints on it).
fn drop_if_unused(sk: &mut Sketch, p: &str) {
    if p == ORIGIN {
        return;
    }
    if !sk.entities.iter().any(|e| e.is_curve() && e.points().contains(&p)) {
        sk.delete_entity(p);
    }
}

/// Constraints that describe the extent of a curve and stop being meaningful once its ends
/// move: its length, equal lengths with it, a midpoint on it.
fn drop_extent_constraints(sk: &mut Sketch, id: &str) {
    let is_line = matches!(sk.entity(id).map(|e| &e.kind), Some(EntityKind::Line { .. }));
    sk.constraints.retain(|c| match &c.kind {
        ConstraintKind::Distance { a, b, .. } => !(a == id && b == id),
        ConstraintKind::Equal { a, b } => !(is_line && (a == id || b == id)),
        ConstraintKind::Midpoint { line, .. } => line != id,
        _ => true,
    });
}

/// A point that became an end of the curve it was constrained onto needs no constraint.
fn drop_self_point_on_curve(sk: &mut Sketch) {
    let own: Vec<(String, String)> = sk.entities.iter().filter(|e| e.is_curve()).flat_map(|e| e.points().into_iter().map(|p| (p.to_string(), e.id.clone())).collect::<Vec<_>>()).collect();
    sk.constraints.retain(|c| !matches!(&c.kind, ConstraintKind::PointOnCurve { p, curve } if own.iter().any(|(q, k)| q == p && k == curve)));
}

fn set_end(sk: &mut Sketch, curve: &str, start: bool, p: &str) {
    if let Some(e) = sk.entity_mut(curve) {
        match &mut e.kind {
            EntityKind::Line { p1, p2 } => *(if start { p1 } else { p2 }) = p.to_string(),
            EntityKind::Arc { start: s, end, .. } => *(if start { s } else { end }) = p.to_string(),
            _ => {}
        }
    }
}

fn ends(sk: &Sketch, curve: &str) -> Option<(EntityId, EntityId)> {
    match &sk.entity(curve)?.kind {
        EntityKind::Line { p1, p2 } => Some((p1.clone(), p2.clone())),
        EntityKind::Arc { start, end, .. } => Some((start.clone(), end.clone())),
        _ => None,
    }
}

/// Applies a trim. Returns the ids of curves created by splitting.
pub fn apply_trim(sk: &mut Sketch, vals: &DimensionValues, plan: &TrimPlan) -> Result<Vec<EntityId>, String> {
    let id = plan.curve.as_str();
    let e = sk.entity(id).cloned().ok_or("The curve no longer exists")?;
    if plan.lo.is_none() && plan.hi.is_none() {
        sk.delete_entity(id);
        return Ok(vec![]);
    }
    drop_extent_constraints(sk, id);
    let mut created = Vec::new();
    match e.kind {
        EntityKind::Circle { c, r } => {
            let (lo, hi) = (plan.lo.as_ref().unwrap(), plan.hi.as_ref().unwrap());
            // the remaining arc runs CCW from the far cut back to the near one
            let s = end_point(sk, vals, hi.pos, &hi.other, id, &[]);
            let t = end_point(sk, vals, lo.pos, &lo.other, id, &[&s]);
            if let Some(en) = sk.entity_mut(id) {
                en.kind = EntityKind::Arc { c, start: s, end: t, r };
            }
        }
        EntityKind::Line { .. } | EntityKind::Arc { .. } => {
            let (p1, p2) = ends(sk, id).ok_or("Not a trimmable curve")?;
            match (&plan.lo, &plan.hi) {
                (None, Some(hi)) => {
                    let n = end_point(sk, vals, hi.pos, &hi.other, id, &[&p1, &p2]);
                    set_end(sk, id, true, &n);
                    drop_if_unused(sk, &p1);
                }
                (Some(lo), None) => {
                    let n = end_point(sk, vals, lo.pos, &lo.other, id, &[&p1, &p2]);
                    set_end(sk, id, false, &n);
                    drop_if_unused(sk, &p2);
                }
                (Some(lo), Some(hi)) => {
                    let a = end_point(sk, vals, lo.pos, &lo.other, id, &[&p1, &p2]);
                    let b = end_point(sk, vals, hi.pos, &hi.other, id, &[&p1, &p2, &a]);
                    set_end(sk, id, false, &a);
                    let piece = match &e.kind {
                        EntityKind::Line { .. } => {
                            let l = sk.add_line_between(&b, &p2);
                            add_if_independent(sk, vals, ConstraintKind::Collinear { a: id.to_string(), b: l.clone() });
                            l
                        }
                        EntityKind::Arc { c, .. } => {
                            let a2 = sk.add_arc_between(c, &b, &p2).ok_or("Could not split the arc")?;
                            add_if_independent(sk, vals, ConstraintKind::Equal { a: id.to_string(), b: a2.clone() });
                            a2
                        }
                        _ => unreachable!(),
                    };
                    if let Some(en) = sk.entity_mut(&piece) {
                        en.construction = e.construction;
                    }
                    if let Some(en) = sk.entity_mut(&b) {
                        if en.owner.as_deref() == Some(id) {
                            en.owner = Some(piece.clone());
                        }
                    }
                    created.push(piece);
                }
                (None, None) => unreachable!(),
            }
        }
        EntityKind::Point { .. } => return Err("Select a curve to trim".into()),
    }
    drop_self_point_on_curve(sk);
    Ok(created)
}

// ---------------------------------------------------------------------------------- extend

/// Extension of a line or arc end up to the nearest curve beyond it.
#[derive(Clone, Debug)]
pub struct ExtendPlan {
    pub curve: EntityId,
    /// The start (true) or end of the curve moves.
    pub start: bool,
    pub to: Cut,
    /// Polyline of the added part (hover preview).
    pub added: Vec<P2>,
}

pub fn extend_plan(sk: &Sketch, id: &str, at: P2) -> Option<ExtendPlan> {
    let me = Curve::of(sk, id)?;
    let start = match me {
        Curve::Line { a, b } => dist(at, a) < dist(at, b),
        Curve::Arc { .. } => dist(at, me.point_at(0.0)) < dist(at, me.point_at(1.0)),
        Curve::Circle { .. } => return None,
    };
    let all = cuts(sk, id, true);
    let to = match me {
        Curve::Line { .. } => {
            if start {
                all.iter().rev().find(|c| c.t < -1e-6).cloned()
            } else {
                all.iter().find(|c| c.t > 1.0 + 1e-6).cloned()
            }
        }
        Curve::Arc { c, a0, sweep, .. } => {
            // angle travelled beyond the end (CCW) / before the start (CW), within the gap
            let gap = TAU - sweep;
            let beyond = |p: P2| if start { (a0 - ang(c, p)).rem_euclid(TAU) } else { (ang(c, p) - (a0 + sweep)).rem_euclid(TAU) };
            all.iter().map(|k| (beyond(k.pos), k)).filter(|(d, _)| *d > 1e-6 && *d < gap - 1e-6).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, k)| k.clone())
        }
        Curve::Circle { .. } => None,
    }?;
    let added = match me {
        Curve::Line { a, b } => vec![if start { a } else { b }, to.pos],
        Curve::Arc { c, r, a0, sweep } => {
            let (from, span) = if start { (a0 - (a0 - ang(c, to.pos)).rem_euclid(TAU), (a0 - ang(c, to.pos)).rem_euclid(TAU)) } else { (a0 + sweep, (ang(c, to.pos) - a0 - sweep).rem_euclid(TAU)) };
            Curve::Arc { c, r, a0: from, sweep: span.max(1e-9) }.polyline(0.0, 1.0)
        }
        Curve::Circle { .. } => return None,
    };
    Some(ExtendPlan { curve: id.to_string(), start, to, added })
}

pub fn apply_extend(sk: &mut Sketch, vals: &DimensionValues, plan: &ExtendPlan) -> Result<(), String> {
    let id = plan.curve.as_str();
    let (p1, p2) = ends(sk, id).ok_or("Only lines and arcs can be extended")?;
    let end = if plan.start { p1.clone() } else { p2.clone() };
    let shared = sk.entities.iter().any(|e| e.is_curve() && e.id != id && e.points().contains(&end.as_str()));
    if shared {
        return Err("This end is connected to other geometry and cannot be extended".into());
    }
    drop_extent_constraints(sk, id);
    let n = end_point(sk, vals, plan.to.pos, &plan.to.other, id, &[&p1, &p2]);
    set_end(sk, id, plan.start, &n);
    drop_if_unused(sk, &end);
    drop_self_point_on_curve(sk);
    Ok(())
}

// ---------------------------------------------------------------------------------- fillet

/// Tangent arc rounding the corner between two curves (lines or arcs).
#[derive(Clone, Debug)]
pub struct FilletPlan {
    /// The two filleted curves (lines or arcs).
    pub lines: [EntityId; 2],
    pub radius: f64,
    pub center: P2,
    /// Tangent points on `lines[0]` / `lines[1]`.
    pub tangent: [P2; 2],
    /// Which end of each curve is the corner end (true = p1 / arc start).
    pub corner_start: [bool; 2],
    /// Largest radius that still fits on both curves.
    pub max_radius: f64,
    /// The fillet arc runs counter-clockwise from `tangent[0]` to `tangent[1]`.
    pub ccw: bool,
    pub arc: Vec<P2>,
}

fn is_line_or_arc(e: &Entity) -> bool {
    matches!(e.kind, EntityKind::Line { .. } | EntityKind::Arc { .. })
}

/// The two curves (lines or arcs) meeting at point `p` (exactly two curves use it).
pub fn corner_curves(sk: &Sketch, p: &str) -> Option<[EntityId; 2]> {
    let users: Vec<&Entity> = sk.entities.iter().filter(|e| e.is_curve() && e.points().contains(&p)).collect();
    match users.as_slice() {
        [a, b] if is_line_or_arc(a) && is_line_or_arc(b) => Some([a.id.clone(), b.id.clone()]),
        _ => None,
    }
}

/// Whether `id` is a line or an arc (the curves Sketch Fillet works on).
pub fn fillettable(sk: &Sketch, id: &str) -> bool {
    sk.entity(id).is_some_and(is_line_or_arc)
}

pub fn fillet_plan(sk: &Sketch, l1: &str, l2: &str, radius: f64) -> Result<FilletPlan, String> {
    fillet_plan_at(sk, l1, l2, None, radius)
}

/// Fillet plan at the corner point `corner` (the clicked point) when the curves meet at more
/// than one point (a line and an arc forming a D, two arcs forming a lens).
pub fn fillet_plan_at(sk: &Sketch, l1: &str, l2: &str, corner: Option<&str>, radius: f64) -> Result<FilletPlan, String> {
    if !fillettable(sk, l1) || !fillettable(sk, l2) || l1 == l2 {
        return Err("Sketch Fillet: select two lines or arcs".into());
    }
    let (Some((a1, b1)), Some((a2, b2))) = (sk.line_points(l1), sk.line_points(l2)) else {
        return curve_fillet_plan(sk, [l1, l2], corner, radius);
    };
    let p = line_intersection(a1, b1, a2, b2).ok_or("Sketch Fillet: the lines are parallel")?;
    let corner_start = [dist(a1, p) < dist(b1, p), dist(a2, p) < dist(b2, p)];
    let far = [if corner_start[0] { b1 } else { a1 }, if corner_start[1] { b2 } else { a2 }];
    let (u1, u2) = (unit(sub(far[0], p)).ok_or("Sketch Fillet: degenerate line")?, unit(sub(far[1], p)).ok_or("Sketch Fillet: degenerate line")?);
    let theta = cross(u1, u2).abs().atan2(dot(u1, u2));
    if theta < 1e-6 || theta > PI - 1e-6 {
        return Err("Sketch Fillet: the lines are collinear".into());
    }
    let k = (theta / 2.0).tan();
    let max_radius = dist(far[0], p).min(dist(far[1], p)) * k;
    if radius <= 0.0 || radius > max_radius + 1e-9 {
        return Err(format!("Sketch Fillet: the radius must be between 0 and {max_radius:.3} mm"));
    }
    let d = radius / k;
    let tangent = [add(p, scale(u1, d)), add(p, scale(u2, d))];
    let w = unit(add(u1, u2)).ok_or("Sketch Fillet: the lines are collinear")?;
    let center = add(p, scale(w, radius / (theta / 2.0).sin()));
    let (s0, s1) = (ang(center, tangent[0]), ang(center, tangent[1]));
    let sweep01 = (s1 - s0).rem_euclid(TAU);
    let (a0, sweep) = if sweep01 <= PI { (s0, sweep01) } else { (s1, TAU - sweep01) };
    let arc = Curve::Arc { c: center, r: radius, a0, sweep }.polyline(0.0, 1.0);
    Ok(FilletPlan { lines: [l1.to_string(), l2.to_string()], radius, center, tangent, corner_start, max_radius, ccw: sweep01 <= PI, arc })
}

/// Lines offset by `r` to both sides / circles of radius R + r and |R - r|: the loci of the
/// centres of radius-`r` circles tangent to the curve.
fn offset_carriers(k: &Curve, r: f64) -> Vec<Curve> {
    match *k {
        Curve::Line { a, b } => {
            let Some(u) = unit(sub(b, a)) else { return vec![] };
            let n = [-u[1], u[0]];
            [r, -r].iter().map(|s| Curve::Line { a: add(a, scale(n, *s)), b: add(b, scale(n, *s)) }).collect()
        }
        Curve::Circle { c, r: rr } | Curve::Arc { c, r: rr, .. } => {
            let mut v = vec![Curve::Circle { c, r: rr + r }];
            if (rr - r).abs() > EPS {
                v.push(Curve::Circle { c, r: (rr - r).abs() });
            }
            v
        }
    }
}

/// Point of `k`'s carrier where a radius-`r` circle centred at `center` touches it.
fn touch_point(k: &Curve, center: P2, r: f64) -> Option<P2> {
    let t = match *k {
        Curve::Line { a, b } => crate::interact::foot_on_line(center, a, b),
        Curve::Circle { c, r: rr } | Curve::Arc { c, r: rr, .. } => {
            let u = unit(sub(center, c))?;
            let (p, q) = (add(c, scale(u, rr)), sub(c, scale(u, rr)));
            if (dist(p, center) - r).abs() <= (dist(q, center) - r).abs() {
                p
            } else {
                q
            }
        }
    };
    ((dist(t, center) - r).abs() < 1e-6 * r.max(1.0)).then_some(t)
}

/// Unit tangent of `k` at `p` in its parameter direction (lines a -> b, arcs CCW).
fn forward(k: &Curve, p: P2) -> Option<P2> {
    match *k {
        Curve::Line { a, b } => unit(sub(b, a)),
        Curve::Circle { c, .. } | Curve::Arc { c, .. } => {
            let v = unit(sub(p, c))?;
            Some([-v[1], v[0]])
        }
    }
}

/// The corner of two curves: the given shared end point, else their (first) shared end point,
/// else the carrier intersection nearest to their closest pair of ends.
fn fillet_corner(sk: &Sketch, ids: [&str; 2], k: &[Curve; 2], hint: Option<&str>) -> Option<P2> {
    let (e0, e1) = (ends(sk, ids[0])?, ends(sk, ids[1])?);
    if let Some(h) = hint.filter(|h| [&e0.0, &e0.1].contains(&&h.to_string()) && [&e1.0, &e1.1].contains(&&h.to_string())) {
        return sk.point(h);
    }
    for p in [&e0.0, &e0.1] {
        if p == &e1.0 || p == &e1.1 {
            return sk.point(p);
        }
    }
    let (p0, p1) = (end_positions(sk, ids[0])?, end_positions(sk, ids[1])?);
    let (mut best, mut mid) = (f64::MAX, p0.0);
    for a in [p0.0, p0.1] {
        for b in [p1.0, p1.1] {
            if dist(a, b) < best {
                best = dist(a, b);
                mid = lerp(a, b, 0.5);
            }
        }
    }
    carrier_intersections(&k[0], &k[1]).into_iter().min_by(|a, b| dist(*a, mid).total_cmp(&dist(*b, mid)))
}

/// Line / arc fillet geometry for one radius: (centre, tangent points, ccw). The fillet keeps
/// each curve from its far end to the tangent point and joins them smoothly (no cusp).
///
/// `crossings` are the other points where the two curves meet: the part cut off a curve (its
/// tangent point to the corner) must not contain one, or the fillet would not be at this corner.
fn curve_fillet_solve(k: &[Curve; 2], ends: &[(P2, P2); 2], corner: P2, corner_start: [bool; 2], crossings: &[P2], r: f64) -> Option<(P2, [P2; 2], bool)> {
    let mut best: Option<(f64, P2, [P2; 2], bool)> = None;
    let cut_crosses = |i: usize, t: P2, to: P2| -> bool {
        crossings.iter().any(|x| match k[i] {
            Curve::Line { .. } => {
                let d = sub(to, t);
                let s = dot(sub(*x, t), d) / dot(d, d).max(1e-24);
                s > 1e-9 && s < 1.0 - 1e-9
            }
            _ => {
                let (pt, pe, px) = (k[i].param_of(t), if corner_start[i] { 0.0 } else { 1.0 }, k[i].param_of(*x));
                px > pt.min(pe) + 1e-9 && px < pt.max(pe) - 1e-9
            }
        })
    };
    for o0 in offset_carriers(&k[0], r) {
        for o1 in offset_carriers(&k[1], r) {
            for c in carrier_intersections(&o0, &o1) {
                let (Some(t0), Some(t1)) = (touch_point(&k[0], c, r), touch_point(&k[1], c, r)) else { continue };
                let t = [t0, t1];
                let mut ok = true;
                let mut head = [[0.0; 2]; 2];
                for i in 0..2 {
                    let (corner_end, far) = if corner_start[i] { (ends[i].0, ends[i].1) } else { (ends[i].1, ends[i].0) };
                    // the kept part runs from the far end to the tangent point (lines may extend to the corner)
                    let to = if matches!(k[i], Curve::Line { .. }) && dist(far, corner) > dist(far, corner_end) { corner } else { corner_end };
                    let on = match k[i] {
                        Curve::Line { .. } => {
                            let d = sub(to, far);
                            let s = dot(sub(t[i], far), d) / dot(d, d).max(1e-24);
                            s > EPS && s <= 1.0 + EPS
                        }
                        _ => k[i].contains(t[i]) && dist(t[i], far) > EPS,
                    } && !cut_crosses(i, t[i], to);
                    let fwd = forward(&k[i], t[i]);
                    // heading along the curve towards its corner end
                    let towards_end = if corner_start[i] { -1.0 } else { 1.0 };
                    match (on, fwd) {
                        (true, Some(f)) => head[i] = scale(f, towards_end),
                        _ => ok = false,
                    }
                }
                if !ok {
                    continue;
                }
                // sense of the fillet arc leaving t0 along curve 0's heading
                let s = cross(sub(t0, c), head[0]);
                if s.abs() < 1e-9 {
                    continue;
                }
                let ccw = s > 0.0;
                let r1 = unit(sub(t1, c))?;
                let arrive = if ccw { [-r1[1], r1[0]] } else { [r1[1], -r1[0]] };
                // arriving on curve 1 must continue away from its corner (tangent, not a cusp)
                if dot(arrive, head[1]) > -1.0 + 1e-6 {
                    continue;
                }
                let score = dist(t0, corner) + dist(t1, corner);
                if best.as_ref().is_none_or(|b| score < b.0) {
                    best = Some((score, c, t, ccw));
                }
            }
        }
    }
    best.map(|(_, c, t, ccw)| (c, t, ccw))
}

/// Fillet between a line and an arc or two arcs.
fn curve_fillet_plan(sk: &Sketch, ids: [&str; 2], hint: Option<&str>, radius: f64) -> Result<FilletPlan, String> {
    let err = || "Sketch Fillet: select two lines or arcs".to_string();
    let k = [Curve::of(sk, ids[0]).ok_or_else(err)?, Curve::of(sk, ids[1]).ok_or_else(err)?];
    let ends = [end_positions(sk, ids[0]).ok_or_else(err)?, end_positions(sk, ids[1]).ok_or_else(err)?];
    let corner = fillet_corner(sk, ids, &k, hint).ok_or("Sketch Fillet: the curves do not meet")?;
    let corner_start = [dist(ends[0].0, corner) < dist(ends[0].1, corner), dist(ends[1].0, corner) < dist(ends[1].1, corner)];
    let crossings: Vec<P2> = carrier_intersections(&k[0], &k[1]).into_iter().filter(|x| dist(*x, corner) > 1e-7 && k[0].contains(*x) && k[1].contains(*x)).collect();
    // largest radius with a solution (a radius that fits means every smaller one does)
    let size = ends.iter().map(|e| dist(e.0, e.1)).fold(0.0, f64::max)
        + [k[0], k[1]].iter().map(|c| c.circle().map(|(_, r)| 2.0 * r).unwrap_or(0.0)).fold(0.0, f64::max);
    let fits = |r: f64| curve_fillet_solve(&k, &ends, corner, corner_start, &crossings, r).is_some();
    let mut lo = size * 1e-6;
    if !fits(lo) {
        return Err("Sketch Fillet: these curves cannot be filleted (are they tangent already?)".into());
    }
    let mut hi = size.max(lo * 2.0);
    for _ in 0..20 {
        if !fits(hi) {
            break;
        }
        lo = hi;
        hi *= 2.0;
    }
    if !fits(hi) {
        for _ in 0..48 {
            let m = 0.5 * (lo + hi);
            if fits(m) {
                lo = m;
            } else {
                hi = m;
            }
        }
    }
    let max_radius = lo;
    let (center, tangent, ccw) = curve_fillet_solve(&k, &ends, corner, corner_start, &crossings, radius)
        .filter(|_| radius > 0.0)
        .ok_or_else(|| format!("Sketch Fillet: the radius must be between 0 and {max_radius:.3} mm"))?;
    let (s0, s1) = (ang(center, tangent[0]), ang(center, tangent[1]));
    let (a0, sweep) = if ccw { (s0, (s1 - s0).rem_euclid(TAU)) } else { (s1, (s0 - s1).rem_euclid(TAU)) };
    let arc = Curve::Arc { c: center, r: radius, a0, sweep }.polyline(0.0, 1.0);
    Ok(FilletPlan { lines: [ids[0].to_string(), ids[1].to_string()], radius, center, tangent, corner_start, max_radius, ccw, arc })
}

/// Default fillet radius for a corner: a round value well inside what fits.
pub fn default_fillet_radius(sk: &Sketch, l1: &str, l2: &str, corner: Option<&str>) -> Option<f64> {
    let max = fillet_plan_at(sk, l1, l2, corner, 1e-6).ok()?.max_radius;
    let target = max * 0.25;
    let step = crate::interact::nice_step(target / 2.0);
    Some(((target / step).floor() * step).max(step.min(max * 0.5)))
}

/// Applies a fillet; returns the arc id.
///
/// Like Fusion's virtual sharp, a corner point that constraints refer to (a coincidence with
/// the origin, dimensions) is kept as a construction point held on both lines, so what was
/// anchored to the corner stays put. Length dimensions of the two lines become the distance from
/// their far end to that corner point, which keeps their value exactly.
pub fn apply_fillet(sk: &mut Sketch, vals: &DimensionValues, plan: &FilletPlan) -> Result<EntityId, String> {
    let mut corners = Vec::new();
    let mut new_ends = Vec::new();
    for i in 0..2 {
        let l = &plan.lines[i];
        let (p1, p2) = ends(sk, l).ok_or("Sketch Fillet: select two lines")?;
        let (corner, far) = if plan.corner_start[i] { (p1, p2) } else { (p2, p1) };
        for c in sk.constraints.iter_mut() {
            if let ConstraintKind::Distance { a, b, orientation } = &c.kind {
                if a == l && b == l {
                    c.kind = ConstraintKind::Distance { a: far.clone(), b: corner.clone(), orientation: *orientation };
                }
            }
        }
        drop_extent_constraints(sk, l);
        let t = sk.add_point(plan.tangent[i][0], plan.tangent[i][1], Some(l));
        set_end(sk, l, plan.corner_start[i], &t);
        corners.push(corner);
        new_ends.push(t);
    }
    corners.dedup();
    for c in corners {
        let used = sk.entities.iter().any(|e| e.is_curve() && e.points().contains(&c.as_str())) || c == ORIGIN;
        let referenced = sk.constraints.iter().any(|k| k.kind.refs().contains(&c.as_str()));
        if used {
            continue;
        }
        if !referenced {
            sk.delete_entity(&c);
            continue;
        }
        if let Some(e) = sk.entity_mut(&c) {
            e.construction = true;
            e.owner = None;
        }
        for l in &plan.lines {
            add_if_independent(sk, vals, ConstraintKind::PointOnCurve { p: c.clone(), curve: l.clone() });
        }
    }
    let cp = sk.add_point(plan.center[0], plan.center[1], None);
    let arc = if plan.ccw { sk.add_arc_between(&cp, &new_ends[0], &new_ends[1]) } else { sk.add_arc_between(&cp, &new_ends[1], &new_ends[0]) }.ok_or("Sketch Fillet: could not create the arc")?;
    for l in &plan.lines {
        add_if_independent(sk, vals, ConstraintKind::Tangent { a: l.clone(), b: arc.clone() });
    }
    Ok(arc)
}

// ---------------------------------------------------------------------------------- offset

/// Connected chain of curves through shared (or coincident) end points, in travel order.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    /// (curve, travelled from its end to its start)
    pub items: Vec<(EntityId, bool)>,
    pub closed: bool,
}

fn end_positions(sk: &Sketch, id: &str) -> Option<(P2, P2)> {
    let (a, b) = ends(sk, id)?;
    Some((sk.point(&a)?, sk.point(&b)?))
}

/// The chain of lines / arcs connected end-to-end with `id` (a circle is a chain by itself).
/// Branch points (more than two curves meeting) end the chain.
pub fn chain_of(sk: &Sketch, id: &str) -> Option<Chain> {
    let e = sk.entity(id)?;
    if matches!(e.kind, EntityKind::Circle { .. }) {
        return Some(Chain { items: vec![(id.to_string(), false)], closed: true });
    }
    let construction = e.construction;
    let candidates: Vec<&Entity> = sk.entities.iter().filter(|o| matches!(o.kind, EntityKind::Line { .. } | EntityKind::Arc { .. }) && o.construction == construction).collect();
    let at = |p: P2, skip: &str| -> Vec<(EntityId, bool)> {
        candidates
            .iter()
            .filter(|o| o.id != skip)
            .filter_map(|o| {
                let (a, b) = end_positions(sk, &o.id)?;
                if dist(a, p) < 1e-6 {
                    Some((o.id.clone(), false))
                } else if dist(b, p) < 1e-6 {
                    Some((o.id.clone(), true))
                } else {
                    None
                }
            })
            .collect()
    };
    let (s0, e0) = end_positions(sk, id)?;
    let mut items = vec![(id.to_string(), false)];
    let mut closed = false;
    // forward from the end
    let mut cur = (id.to_string(), e0);
    loop {
        let next = at(cur.1, &cur.0);
        if next.len() != 1 {
            break;
        }
        let (n, rev) = next[0].clone();
        if n == id {
            closed = true;
            break;
        }
        if items.iter().any(|(i, _)| *i == n) {
            break;
        }
        let (a, b) = end_positions(sk, &n)?;
        cur = (n.clone(), if rev { a } else { b });
        items.push((n, rev));
    }
    if !closed {
        let mut cur = (id.to_string(), s0);
        loop {
            let prev = at(cur.1, &cur.0);
            if prev.len() != 1 {
                break;
            }
            let (n, starts_here) = prev[0].clone();
            // the previous curve must end here: travelled forward if its end is here
            let rev = !starts_here;
            if items.iter().any(|(i, _)| *i == n) {
                break;
            }
            let (a, b) = end_positions(sk, &n)?;
            cur = (n.clone(), if rev { b } else { a });
            items.insert(0, (n, rev));
        }
    }
    Some(Chain { items, closed })
}

/// One offset curve: a line, or an arc/circle around `center` (its source's centre point).
#[derive(Clone, Debug)]
pub enum OffsetCurve {
    Line { a: P2, b: P2 },
    Arc { center: P2, r: f64, from: P2, to: P2, ccw: bool },
    Circle { center: P2, r: f64 },
}

/// Signed distance of `p` from the chain: positive on the left of the travel direction.
pub fn offset_side(sk: &Sketch, chain: &Chain, p: P2) -> Option<f64> {
    let mut best: Option<(f64, f64)> = None;
    for (id, rev) in &chain.items {
        let (d, signed) = match Curve::of(sk, id)? {
            Curve::Line { a, b } => {
                let (a, b) = if *rev { (b, a) } else { (a, b) };
                let q = closest_on_segment(p, a, b);
                (dist(p, q), cross(sub(b, a), sub(p, a)) / dist(a, b).max(1e-12))
            }
            Curve::Circle { c, r } => ((dist(p, c) - r).abs(), r - dist(p, c)),
            Curve::Arc { c, r, .. } => {
                let (_, d) = crate::interact::closest_on_curve(sk, sk.entity(id)?, p)?;
                let left = r - dist(p, c);
                (d, if *rev { -left } else { left })
            }
        };
        if best.is_none_or(|b| d < b.0) {
            best = Some((d, signed));
        }
    }
    best.map(|b| b.1)
}

/// Geometry of the chain offset by `d` to the left of its travel direction.
pub fn offset_geometry(sk: &Sketch, chain: &Chain, d: f64) -> Result<Vec<OffsetCurve>, String> {
    if d.abs() < 1e-9 {
        return Err("Offset: the distance must not be zero".into());
    }
    // per item: travel-oriented start/end and the naive offset curve
    struct It {
        curve: Curve,
        rev: bool,
        start: P2,
        end: P2,
        /// offset carrier: line through (a, b) / circle (c, r)
        off: Curve,
    }
    let mut its = Vec::new();
    for (id, rev) in &chain.items {
        let c = Curve::of(sk, id).ok_or("Offset: the curve no longer exists")?;
        let it = match c {
            Curve::Line { a, b } => {
                let (s, e) = if *rev { (b, a) } else { (a, b) };
                let u = unit(sub(e, s)).ok_or("Offset: degenerate line")?;
                let n = scale([-u[1], u[0]], d);
                It { curve: c, rev: *rev, start: s, end: e, off: Curve::Line { a: add(s, n), b: add(e, n) } }
            }
            Curve::Circle { c: cc, r } => {
                let r2 = r - d;
                if r2 <= 1e-9 {
                    return Err("Offset: the distance is larger than the circle's radius".into());
                }
                return Ok(vec![OffsetCurve::Circle { center: cc, r: r2 }]);
            }
            Curve::Arc { c: cc, r, .. } => {
                let (s, e) = if *rev { (c.point_at(1.0), c.point_at(0.0)) } else { (c.point_at(0.0), c.point_at(1.0)) };
                let r2 = if *rev { r + d } else { r - d };
                if r2 <= 1e-9 {
                    return Err("Offset: the distance is larger than an arc's radius".into());
                }
                It { curve: c, rev: *rev, start: s, end: e, off: Curve::Circle { c: cc, r: r2 } }
            }
        };
        its.push(it);
    }
    let naive = |it: &It, p: P2| -> P2 {
        match it.off {
            Curve::Line { a, b } => crate::interact::foot_on_line(p, a, b),
            Curve::Circle { c, r } => add(c, scale(unit(sub(p, c)).unwrap_or([1.0, 0.0]), r)),
            _ => p,
        }
    };
    let n = its.len();
    // junction j sits between item j-1 and item j (closed: junction 0 joins last and first)
    let mut starts = vec![[0.0; 2]; n];
    let mut ends_ = vec![[0.0; 2]; n];
    for j in 0..n {
        if j == 0 && !chain.closed {
            starts[0] = naive(&its[0], its[0].start);
            continue;
        }
        let (pi, ni) = ((j + n - 1) % n, j);
        let (p, q) = (&its[pi], &its[ni]);
        let (np, nq) = (naive(p, p.end), naive(q, q.start));
        let joint = if dist(np, nq) < 1e-6 {
            np
        } else {
            let hint = lerp(np, nq, 0.5);
            carrier_intersections(&p.off, &q.off).into_iter().min_by(|a, b| dist(*a, hint).total_cmp(&dist(*b, hint))).ok_or("Offset: adjacent offset curves do not meet")?
        };
        ends_[pi] = joint;
        starts[ni] = joint;
    }
    if !chain.closed {
        ends_[n - 1] = naive(&its[n - 1], its[n - 1].end);
    }
    let mut out = Vec::new();
    for (k, it) in its.iter().enumerate() {
        let (s, e) = (starts[k], ends_[k]);
        match (it.curve, it.off) {
            (Curve::Line { .. }, _) => {
                if dot(sub(e, s), sub(it.end, it.start)) <= 1e-9 {
                    return Err("Offset: the distance is too large for this geometry".into());
                }
                out.push(OffsetCurve::Line { a: s, b: e });
            }
            (Curve::Arc { .. }, Curve::Circle { c, r }) => out.push(OffsetCurve::Arc { center: c, r, from: s, to: e, ccw: !it.rev }),
            _ => {}
        }
    }
    Ok(out)
}

pub fn offset_polylines(curves: &[OffsetCurve]) -> Vec<Vec<P2>> {
    curves
        .iter()
        .map(|c| match *c {
            OffsetCurve::Line { a, b } => vec![a, b],
            OffsetCurve::Circle { center, r } => crate::interact::circle_polyline(center, r),
            OffsetCurve::Arc { center, r, from, to, ccw } => {
                let (s, e) = if ccw { (from, to) } else { (to, from) };
                let a0 = ang(center, s);
                let mut sweep = (ang(center, e) - a0).rem_euclid(TAU);
                if sweep < 1e-9 {
                    sweep = TAU;
                }
                Curve::Arc { c: center, r, a0, sweep }.polyline(0.0, 1.0)
            }
        })
        .collect()
}

/// Result of an offset: the new curves paired with their sources.
#[derive(Clone, Debug)]
pub struct OffsetResult {
    pub pairs: Vec<(EntityId, EntityId)>,
}

/// Creates the offset chain: lines parallel to their sources, arcs/circles sharing their
/// source's centre point, consecutive offset curves sharing end points. Distance dimensions
/// are left to the caller (they need document parameters).
pub fn apply_offset(sk: &mut Sketch, vals: &DimensionValues, chain: &Chain, d: f64) -> Result<OffsetResult, String> {
    let geo = offset_geometry(sk, chain, d)?;
    let construction = chain.items.first().and_then(|(i, _)| sk.entity(i)).is_some_and(|e| e.construction);
    let center_of = |sk: &Sketch, id: &str| match &sk.entity(id)?.kind {
        EntityKind::Circle { c, .. } | EntityKind::Arc { c, .. } => Some(c.clone()),
        _ => None,
    };
    if let [OffsetCurve::Circle { r, .. }] = geo.as_slice() {
        let src = chain.items[0].0.clone();
        let c = center_of(sk, &src).ok_or("Offset: not a circle")?;
        let id = sk.alloc_id("c");
        sk.entities.push(Entity { id: id.clone(), kind: EntityKind::Circle { c, r: *r }, construction, owner: None });
        return Ok(OffsetResult { pairs: vec![(id, src)] });
    }
    let n = geo.len();
    // junction points: start of each curve; the end of curve k is the start of k+1
    let mut pts: Vec<EntityId> = Vec::new();
    for g in &geo {
        let s = match *g {
            OffsetCurve::Line { a, .. } => a,
            OffsetCurve::Arc { from, .. } => from,
            OffsetCurve::Circle { .. } => unreachable!(),
        };
        pts.push(sk.add_point(s[0], s[1], None));
    }
    let last_end = match *geo.last().unwrap() {
        OffsetCurve::Line { b, .. } => b,
        OffsetCurve::Arc { to, .. } => to,
        OffsetCurve::Circle { .. } => unreachable!(),
    };
    let tail = if chain.closed { pts[0].clone() } else { sk.add_point(last_end[0], last_end[1], None) };
    let mut pairs = Vec::new();
    for (k, g) in geo.iter().enumerate() {
        let (s, e) = (pts[k].clone(), if k + 1 < n { pts[k + 1].clone() } else { tail.clone() });
        let src = chain.items[k].0.clone();
        let id = match *g {
            OffsetCurve::Line { .. } => {
                let l = sk.add_line_between(&s, &e);
                add_if_independent(sk, vals, ConstraintKind::Parallel { a: src.clone(), b: l.clone() });
                l
            }
            OffsetCurve::Arc { ccw, .. } => {
                let c = center_of(sk, &src).ok_or("Offset: not an arc")?;
                if ccw { sk.add_arc_between(&c, &s, &e) } else { sk.add_arc_between(&c, &e, &s) }.ok_or("Offset: could not create an arc")?
            }
            OffsetCurve::Circle { .. } => unreachable!(),
        };
        for p in [&s, &e] {
            if let Some(en) = sk.entity_mut(p) {
                en.owner.get_or_insert_with(|| id.clone());
            }
        }
        if let Some(en) = sk.entity_mut(&id) {
            en.construction = construction;
        }
        pairs.push((id, src));
    }
    Ok(OffsetResult { pairs })
}

// ---------------------------------------------------------------------------------- mirror

/// Mirror image of `p` in the line through `a`, `b`.
pub fn reflect(p: P2, a: P2, b: P2) -> P2 {
    let f = crate::interact::foot_on_line(p, a, b);
    sub(scale(f, 2.0), p)
}

/// Mirrors curves (and points) about the line `axis`. Points on the axis are shared with the
/// copies; every other mirrored point gets a Symmetry constraint with its source, and copied
/// circles / arcs an Equal radius. Returns the new entity ids.
pub fn apply_mirror(sk: &mut Sketch, vals: &DimensionValues, ids: &[String], axis: &str) -> Result<Vec<EntityId>, String> {
    let (a, b) = sk.line_points(axis).ok_or("Mirror: the mirror line must be a sketch line")?;
    let ids: Vec<String> = ids.iter().filter(|i| *i != axis && sk.entity(i).is_some()).cloned().collect();
    if ids.is_empty() {
        return Err("Mirror: select the objects to mirror".into());
    }
    let mut map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut created = Vec::new();
    let mut map_point = |sk: &mut Sketch, p: &str, created: &mut Vec<EntityId>| -> Option<String> {
        if let Some(m) = map.get(p) {
            return Some(m.clone());
        }
        let pos = sk.point(p)?;
        let q = reflect(pos, a, b);
        let m = if dist(q, pos) < 1e-6 {
            p.to_string()
        } else {
            let id = sk.add_point(q[0], q[1], None);
            created.push(id.clone());
            id
        };
        map.insert(p.to_string(), m.clone());
        Some(m)
    };
    for id in &ids {
        let e = sk.entity(id).cloned().ok_or("Mirror: missing entity")?;
        let kind = match &e.kind {
            EntityKind::Point { .. } => {
                map_point(sk, id, &mut created);
                continue;
            }
            EntityKind::Line { p1, p2 } => EntityKind::Line { p1: map_point(sk, p1, &mut created).ok_or("Mirror: bad line")?, p2: map_point(sk, p2, &mut created).ok_or("Mirror: bad line")? },
            EntityKind::Circle { c, r } => EntityKind::Circle { c: map_point(sk, c, &mut created).ok_or("Mirror: bad circle")?, r: *r },
            // mirroring reverses the direction of travel: swap the ends to stay CCW
            EntityKind::Arc { c, start, end, r } => EntityKind::Arc {
                c: map_point(sk, c, &mut created).ok_or("Mirror: bad arc")?,
                start: map_point(sk, end, &mut created).ok_or("Mirror: bad arc")?,
                end: map_point(sk, start, &mut created).ok_or("Mirror: bad arc")?,
                r: *r,
            },
        };
        let prefix = match kind {
            EntityKind::Line { .. } => "l",
            EntityKind::Circle { .. } => "c",
            _ => "a",
        };
        let nid = sk.alloc_id(prefix);
        let pts: Vec<String> = Entity { id: nid.clone(), kind: kind.clone(), construction: false, owner: None }.points().iter().map(|s| s.to_string()).collect();
        sk.entities.push(Entity { id: nid.clone(), kind, construction: e.construction, owner: None });
        for p in pts {
            if created.contains(&p) {
                if let Some(en) = sk.entity_mut(&p) {
                    en.owner.get_or_insert_with(|| nid.clone());
                }
            }
        }
        if matches!(e.kind, EntityKind::Circle { .. } | EntityKind::Arc { .. }) {
            add_if_independent(sk, vals, ConstraintKind::Equal { a: id.clone(), b: nid.clone() });
        }
        created.push(nid);
    }
    let mut pairs: Vec<(String, String)> = map.into_iter().filter(|(k, v)| k != v).collect();
    pairs.sort();
    for (src, copy) in pairs {
        add_if_independent(sk, vals, ConstraintKind::Symmetric { p1: src, p2: copy, line: axis.to_string() });
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{find_profiles, solve, SolveStatus};

    fn areas(s: &mut Sketch) -> Vec<f64> {
        assert_eq!(solve(s, &DimensionValues::new(), &[]).status, SolveStatus::Solved);
        let mut a: Vec<f64> = find_profiles(s).iter().map(|p| p.area).collect();
        a.sort_by(f64::total_cmp);
        a
    }

    fn v() -> DimensionValues {
        DimensionValues::new()
    }

    #[test]
    fn trim_line_crossing_rectangle() {
        let mut s = Sketch::new();
        s.add_rectangle([0.0, 0.0], [40.0, 20.0]);
        let l = s.add_line([20.0, -10.0], [20.0, 30.0]);
        let plan = trim_plan(&s, &l, [20.0, -5.0]).unwrap();
        assert!(plan.lo.is_none() && plan.hi.as_ref().is_some_and(|h| dist(h.pos, [20.0, 0.0]) < 1e-9));
        apply_trim(&mut s, &v(), &plan).unwrap();
        let plan = trim_plan(&s, &l, [20.0, 25.0]).unwrap();
        apply_trim(&mut s, &v(), &plan).unwrap();
        let (a, b) = s.line_points(&l).unwrap();
        assert!(dist(a, [20.0, 0.0]) < 1e-9 && dist(b, [20.0, 20.0]) < 1e-9);
        assert_eq!(areas(&mut s), vec![400.0, 400.0]);
        // middle trim of the bottom edge splits it in two
        let bottom = s.entities.iter().find(|e| s.line_points(&e.id).is_some_and(|(p, q)| p[1].abs() < 1e-9 && q[1].abs() < 1e-9)).unwrap().id.clone();
        let mid = s.add_line([30.0, -5.0], [30.0, 5.0]);
        let plan = trim_plan(&s, &bottom, [25.0, 0.0]).unwrap();
        let made = apply_trim(&mut s, &v(), &plan).unwrap();
        assert_eq!(made.len(), 1);
        let _ = mid;
        assert_eq!(solve(&mut s, &v(), &[]).status, SolveStatus::Solved);
    }

    #[test]
    fn trim_circle_leaves_arc() {
        let mut s = Sketch::new();
        let c = s.add_circle([0.0, 0.0], 10.0);
        s.add_line([-20.0, 0.0], [20.0, 0.0]);
        let plan = trim_plan(&s, &c, [0.0, -10.0]).unwrap();
        apply_trim(&mut s, &v(), &plan).unwrap();
        assert!(matches!(s.entity(&c).unwrap().kind, EntityKind::Arc { .. }));
        let a = areas(&mut s);
        assert_eq!(a.len(), 1);
        assert!((a[0] - PI * 50.0).abs() < 1e-6, "{a:?}");
    }

    #[test]
    fn extend_line_to_edge() {
        let mut s = Sketch::new();
        s.add_rectangle([0.0, 0.0], [40.0, 20.0]);
        let l = s.add_line([10.0, 5.0], [20.0, 5.0]);
        let plan = extend_plan(&s, &l, [19.0, 5.0]).unwrap();
        apply_extend(&mut s, &v(), &plan).unwrap();
        assert!(dist(s.line_points(&l).unwrap().1, [40.0, 5.0]) < 1e-9);
        let plan = extend_plan(&s, &l, [11.0, 5.0]).unwrap();
        apply_extend(&mut s, &v(), &plan).unwrap();
        assert_eq!(areas(&mut s).len(), 2);
    }

    #[test]
    fn fillet_rectangle_corner() {
        let mut s = Sketch::new();
        let r = s.add_rectangle([0.0, 0.0], [40.0, 20.0]);
        let plan = fillet_plan(&s, &r[1], &r[2], 5.0).unwrap();
        apply_fillet(&mut s, &v(), &plan).unwrap();
        let a = areas(&mut s);
        assert!((a[0] - (800.0 - (25.0 - PI * 25.0 / 4.0))).abs() < 1e-6, "{a:?}");
        assert!(fillet_plan(&s, &r[0], &r[3], 30.0).is_err(), "too large");
    }

    /// Fillets `a`-`b` at radius `r`, adds the radius dimension and checks that the result
    /// solves in place with the same degrees of freedom as before.
    fn fillet_keeps_dof(s: &mut Sketch, a: &str, b: &str, r: f64) -> FilletPlan {
        let before = solve(s, &v(), &[]).dof;
        let plan = fillet_plan(s, a, b, r).unwrap();
        let arc = apply_fillet(s, &v(), &plan).unwrap();
        assert_eq!(s.constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::Tangent { .. })).count(), 2);
        let mut vals = v();
        let k = s.add_dimension(ConstraintKind::Radius { curve: arc.clone() }, "r", [0.0, 0.0], false);
        vals.insert(k, r);
        let rep = solve(s, &vals, &[]);
        assert_eq!(rep.status, SolveStatus::Solved, "{rep:?}");
        assert_eq!(rep.dof, before, "the fillet adds no freedom besides its radius: {rep:?}");
        let Some(Curve::Arc { c, r: rr, .. }) = Curve::of(s, &arc) else { panic!() };
        assert!(dist(c, plan.center) < 1e-6 && (rr - r).abs() < 1e-9, "solved in place: {c:?} {rr}");
        plan
    }

    #[test]
    fn fillet_line_and_arc() {
        // quarter arc (0,0) r 20 from (20,0) to (0,20); a line from its start to (50,0)
        let mut s = Sketch::new();
        let (c, p, e, f) = (s.add_point(0.0, 0.0, None), s.add_point(20.0, 0.0, None), s.add_point(0.0, 20.0, None), s.add_point(50.0, 0.0, None));
        let arc = s.add_arc_between(&c, &p, &e).unwrap();
        let line = s.add_line_between(&p, &f);
        assert_eq!(corner_curves(&s, &p), Some([arc.clone(), line.clone()]));
        let plan = fillet_keeps_dof(&mut s, &line, &arc, 5.0);
        // outside the arc, above the line: |centre| = 25, y = 5
        let x = 600f64.sqrt();
        assert!(dist(plan.center, [x, 5.0]) < 1e-9, "{:?}", plan.center);
        assert!(dist(plan.tangent[0], [x, 0.0]) < 1e-9 && dist(plan.tangent[1], scale([x, 5.0], 20.0 / 25.0)) < 1e-9, "{:?}", plan.tangent);
        assert!(s.point(&p).is_none(), "the unused corner point is removed");
        // the arc now starts at the tangent point; the line too
        assert!(matches!(Curve::of(&s, &arc), Some(Curve::Arc { a0, .. }) if (a0 - (4.0f64).atan2(x * 0.8)).abs() < 1e-6));
        assert!(dist(s.line_points(&line).unwrap().0, [x, 0.0]) < 1e-6);
        // too large: the line is only 30 long
        let mut t = Sketch::new();
        let (c, p, e, f) = (t.add_point(0.0, 0.0, None), t.add_point(20.0, 0.0, None), t.add_point(0.0, 20.0, None), t.add_point(30.0, 0.0, None));
        let arc = t.add_arc_between(&c, &p, &e).unwrap();
        let line = t.add_line_between(&p, &f);
        let plan = fillet_plan(&t, &arc, &line, 1.0).unwrap();
        assert!(plan.max_radius > 1.0 && plan.max_radius < 30.0, "{}", plan.max_radius);
        assert!(fillet_plan(&t, &arc, &line, plan.max_radius * 1.5).is_err());
    }

    #[test]
    fn fillet_lines_drawn_from_the_same_point() {
        // both lines start at the corner (p1 shared), then both end there
        for flip in [false, true] {
            let mut s = Sketch::new();
            let (o, a, b) = (s.add_point(0.0, 0.0, None), s.add_point(20.0, 0.0, None), s.add_point(0.0, 20.0, None));
            let (l1, l2) = if flip { (s.add_line_between(&a, &o), s.add_line_between(&b, &o)) } else { (s.add_line_between(&o, &a), s.add_line_between(&o, &b)) };
            let plan = fillet_plan(&s, &l1, &l2, 5.0).unwrap();
            assert!(dist(plan.center, [5.0, 5.0]) < 1e-9, "{:?}", plan.center);
            assert!(dist(plan.tangent[0], [5.0, 0.0]) < 1e-9 && dist(plan.tangent[1], [0.0, 5.0]) < 1e-9);
            assert!((plan.max_radius - 20.0).abs() < 1e-9);
        }
    }

    #[test]
    fn fillet_stays_at_the_corner_when_the_curves_cross_again() {
        // the arc (-1,-1) r sqrt2 from (0,0) CCW to (-2,-2) crosses the line again at (-2,0):
        // the corner fillet only fits up to (sqrt2 - 1) / 2
        let mut s = Sketch::new();
        let (p, f, c, e) = (s.add_point(0.0, 0.0, None), s.add_point(-10.0, 0.0, None), s.add_point(-1.0, -1.0, None), s.add_point(-2.0, -2.0, None));
        let line = s.add_line_between(&f, &p);
        let arc = s.add_arc_between(&c, &p, &e).unwrap();
        let plan = fillet_plan(&s, &line, &arc, 0.1).unwrap();
        assert!((plan.max_radius - (2f64.sqrt() - 1.0) / 2.0).abs() < 1e-6, "{}", plan.max_radius);
        assert!(plan.tangent[0][0] > -2.0, "trimmed between the crossing and the corner: {:?}", plan.tangent);
        assert!(fillet_plan(&s, &line, &arc, 0.5).is_err(), "no jump past the crossing");
    }

    #[test]
    fn fillet_at_the_clicked_corner_of_a_d_shape() {
        // a line and a half circle share both ends
        let mut s = Sketch::new();
        let (a, b, c) = (s.add_point(-10.0, 0.0, None), s.add_point(10.0, 0.0, None), s.add_point(0.0, 0.0, None));
        let line = s.add_line_between(&a, &b);
        let arc = s.add_arc_between(&c, &b, &a).unwrap();
        for (corner, side) in [(&a, -1.0), (&b, 1.0)] {
            let plan = fillet_plan_at(&s, &line, &arc, Some(corner), 2.0).unwrap();
            assert!(plan.tangent.iter().all(|t| t[0] * side > 5.0), "fillet at the clicked corner: {:?}", plan.tangent);
        }
        let mut t = s.clone();
        let plan = fillet_plan_at(&t, &line, &arc, Some(&b), 2.0).unwrap();
        apply_fillet(&mut t, &v(), &plan).unwrap();
        assert!(t.point(&a).is_some() && t.point(&b).is_none(), "only the clicked corner is rounded");
    }

    #[test]
    fn fillet_two_arcs() {
        // A: (0,0) r 20 from (20,0) up to (0,20); B: (20,20) r 20 from (20,0) right to (40,20)
        let mut s = Sketch::new();
        let (ca, p, ea) = (s.add_point(0.0, 0.0, None), s.add_point(20.0, 0.0, None), s.add_point(0.0, 20.0, None));
        let (cb, eb) = (s.add_point(20.0, 20.0, None), s.add_point(40.0, 20.0, None));
        let a = s.add_arc_between(&ca, &p, &ea).unwrap();
        let b = s.add_arc_between(&cb, &p, &eb).unwrap();
        let plan = fillet_keeps_dof(&mut s, &a, &b, 3.0);
        // outside A (external tangency), inside B (internal tangency)
        assert!((dist(plan.center, [0.0, 0.0]) - 23.0).abs() < 1e-9, "{:?}", plan.center);
        assert!((dist(plan.center, [20.0, 20.0]) - 17.0).abs() < 1e-9, "{:?}", plan.center);
        assert!((dist(plan.tangent[0], [0.0, 0.0]) - 20.0).abs() < 1e-9 && (dist(plan.tangent[1], [20.0, 20.0]) - 20.0).abs() < 1e-9);
        assert!(plan.tangent.iter().all(|t| (dist(*t, plan.center) - 3.0).abs() < 1e-9));
    }

    #[test]
    fn offset_closed_and_open_chains() {
        let mut s = Sketch::new();
        let r = s.add_rectangle([0.0, 0.0], [40.0, 20.0]);
        let ch = chain_of(&s, &r[2]).unwrap();
        assert!(ch.closed && ch.items.len() == 4);
        // outside of a CCW loop is to the right: negative
        assert!(offset_side(&s, &ch, [20.0, 25.0]).unwrap() < 0.0);
        apply_offset(&mut s, &v(), &ch, -5.0).unwrap();
        assert_eq!(areas(&mut s), vec![700.0, 800.0]);
        // open L: offset keeps a mitred corner
        let mut s = Sketch::new();
        let a = s.add_line([0.0, 0.0], [10.0, 0.0]);
        let p = match &s.entity(&a).unwrap().kind {
            EntityKind::Line { p2, .. } => p2.clone(),
            _ => unreachable!(),
        };
        let q = s.add_point(10.0, 10.0, None);
        s.add_line_between(&p, &q);
        let ch = chain_of(&s, &a).unwrap();
        assert!(!ch.closed && ch.items.len() == 2);
        let g = offset_geometry(&s, &ch, 2.0).unwrap();
        assert!(matches!(g[0], OffsetCurve::Line { b, .. } if dist(b, [8.0, 2.0]) < 1e-9));
    }

    #[test]
    fn offset_slot_concentric_arcs() {
        let mut s = Sketch::new();
        let slot = s.add_slot(&v(), [0.0, 0.0], [20.0, 0.0], 5.0).unwrap();
        let ch = chain_of(&s, &slot.lines[0]).unwrap();
        assert!(ch.closed && ch.items.len() == 4);
        apply_offset(&mut s, &v(), &ch, 2.0).unwrap();
        let a = areas(&mut s);
        let inner = PI * 9.0 + 2.0 * 3.0 * 20.0;
        let outer = PI * 25.0 + 2.0 * 5.0 * 20.0;
        assert_eq!(a.len(), 2);
        assert!(a.iter().any(|x| (x - inner).abs() < 1e-6) && a.iter().any(|x| (x - (outer - inner)).abs() < 1e-6), "{a:?}");
    }

    #[test]
    fn mirror_triangle() {
        let mut s = Sketch::new();
        let axis = s.add_line([0.0, -10.0], [0.0, 30.0]);
        s.entity_mut(&axis).unwrap().construction = true;
        let p0 = s.add_point(0.0, 0.0, None);
        let p1 = s.add_point(10.0, 0.0, None);
        let p2 = s.add_point(0.0, 20.0, None);
        let ls = [s.add_line_between(&p0, &p1), s.add_line_between(&p1, &p2), s.add_line_between(&p2, &p0)];
        let made = apply_mirror(&mut s, &v(), &ls[..2], &axis).unwrap();
        assert_eq!(made.iter().filter(|i| i.starts_with('l')).count(), 2);
        assert!(s.entities.iter().any(|e| matches!(e.kind, EntityKind::Point { x, y } if (x + 10.0).abs() < 1e-9 && y.abs() < 1e-9)));
        assert_eq!(s.constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::Symmetric { .. })).count(), 1);
        assert_eq!(areas(&mut s), vec![100.0, 100.0]);
    }
}
