//! Pure geometry helpers for interactive sketching: hit testing, snapping, horizontal/vertical
//! inference, preview polylines and dimension annotation geometry. No UI or document
//! dependency, so everything here is unit-testable.

use crate::model::{ConstraintKind, DistanceOrientation, Entity, EntityKind, Sketch, ORIGIN};
use std::f64::consts::{PI, TAU};

pub type P2 = [f64; 2];

pub fn sub(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}
pub fn add(a: P2, b: P2) -> P2 {
    [a[0] + b[0], a[1] + b[1]]
}
pub fn scale(a: P2, s: f64) -> P2 {
    [a[0] * s, a[1] * s]
}
pub fn dot(a: P2, b: P2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
pub fn cross(a: P2, b: P2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
pub fn len(a: P2) -> f64 {
    dot(a, a).sqrt()
}
pub fn dist(a: P2, b: P2) -> f64 {
    len(sub(a, b))
}
pub fn unit(a: P2) -> Option<P2> {
    let l = len(a);
    (l > 1e-12).then(|| scale(a, 1.0 / l))
}
pub fn lerp(a: P2, b: P2, t: f64) -> P2 {
    add(a, scale(sub(b, a), t))
}

/// Closest point to `p` on the segment `a`-`b`.
pub fn closest_on_segment(p: P2, a: P2, b: P2) -> P2 {
    let d = sub(b, a);
    let l2 = dot(d, d);
    if l2 < 1e-24 {
        return a;
    }
    lerp(a, b, (dot(sub(p, a), d) / l2).clamp(0.0, 1.0))
}

/// Foot of the perpendicular from `p` onto the infinite line through `a`, `b`.
pub fn foot_on_line(p: P2, a: P2, b: P2) -> P2 {
    let d = sub(b, a);
    let l2 = dot(d, d);
    if l2 < 1e-24 {
        return a;
    }
    lerp(a, b, dot(sub(p, a), d) / l2)
}

/// Counter-clockwise sweep of an arc from `start` to `end` around `c` (full turn when equal).
fn arc_sweep(c: P2, s: P2, e: P2) -> (f64, f64) {
    let a0 = (s[1] - c[1]).atan2(s[0] - c[0]);
    let mut sweep = ((e[1] - c[1]).atan2(e[0] - c[0]) - a0).rem_euclid(TAU);
    if sweep < 1e-9 {
        sweep = TAU;
    }
    (a0, sweep)
}

/// Closest position on a curve entity and its distance from `p`.
pub fn closest_on_curve(s: &Sketch, e: &Entity, p: P2) -> Option<(P2, f64)> {
    let q = match &e.kind {
        EntityKind::Line { p1, p2 } => closest_on_segment(p, s.point(p1)?, s.point(p2)?),
        EntityKind::Circle { c, r } => {
            let c = s.point(c)?;
            add(c, scale(unit(sub(p, c)).unwrap_or([1.0, 0.0]), *r))
        }
        EntityKind::Arc { c, start, end, r } => {
            let (c, sp, ep) = (s.point(c)?, s.point(start)?, s.point(end)?);
            let (a0, sweep) = arc_sweep(c, sp, ep);
            let a = (p[1] - c[1]).atan2(p[0] - c[0]);
            if (a - a0).rem_euclid(TAU) <= sweep {
                add(c, scale([a.cos(), a.sin()], *r))
            } else if dist(p, sp) < dist(p, ep) {
                sp
            } else {
                ep
            }
        }
        EntityKind::Point { .. } => return None,
    };
    Some((q, dist(p, q)))
}

/// Nearest point entity (the sketch origin included) within `tol`.
pub fn nearest_point(s: &Sketch, p: P2, tol: f64, exclude: &[&str]) -> Option<(String, f64)> {
    let mut best: Option<(String, f64)> = None;
    let origin = (ORIGIN.to_string(), dist(p, [0.0, 0.0]));
    let candidates = s.entities.iter().filter_map(|e| match e.kind {
        EntityKind::Point { x, y } => Some((e.id.clone(), dist(p, [x, y]))),
        _ => None,
    });
    for (id, d) in candidates.chain(std::iter::once(origin)) {
        if d <= tol && !exclude.contains(&id.as_str()) && best.as_ref().is_none_or(|b| d < b.1) {
            best = Some((id, d));
        }
    }
    best
}

/// Nearest curve accepted by `accept` within `tol`: (id, closest position, distance).
pub fn nearest_curve(s: &Sketch, p: P2, tol: f64, accept: impl Fn(&Entity) -> bool) -> Option<(String, P2, f64)> {
    let mut best: Option<(String, P2, f64)> = None;
    for e in s.entities.iter().filter(|e| e.is_curve() && accept(e)) {
        if let Some((q, d)) = closest_on_curve(s, e, p) {
            if d <= tol && best.as_ref().is_none_or(|b| d < b.2) {
                best = Some((e.id.clone(), q, d));
            }
        }
    }
    best
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// Horizontal / vertical inference: the axis the segment `a`-`b` is within `max_deg` of.
pub fn infer_axis(a: P2, b: P2, max_deg: f64) -> Option<Axis> {
    let d = sub(b, a);
    if len(d) < 1e-9 {
        return None;
    }
    let ang = d[1].abs().atan2(d[0].abs()).to_degrees();
    if ang <= max_deg {
        Some(Axis::Horizontal)
    } else if 90.0 - ang <= max_deg {
        Some(Axis::Vertical)
    } else {
        None
    }
}

/// Moves `b` so that `a`-`b` lies exactly on `axis`.
pub fn project_to_axis(a: P2, b: P2, axis: Axis) -> P2 {
    match axis {
        Axis::Horizontal => [b[0], a[1]],
        Axis::Vertical => [a[0], b[1]],
    }
}

/// Smallest 1-2-5 × 10^n step that is at least `x`.
pub fn nice_step(x: f64) -> f64 {
    let x = x.max(1e-6);
    let p = 10f64.powf(x.log10().floor());
    let m = x / p;
    p * if m <= 1.0 + 1e-9 {
        1.0
    } else if m <= 2.0 + 1e-9 {
        2.0
    } else if m <= 5.0 + 1e-9 {
        5.0
    } else {
        10.0
    }
}

pub fn snap_to_grid(p: P2, step: f64) -> P2 {
    [(p[0] / step).round() * step, (p[1] / step).round() * step]
}

pub fn circle_polyline(c: P2, r: f64) -> Vec<P2> {
    (0..=72).map(|i| add(c, scale([(TAU * i as f64 / 72.0).cos(), (TAU * i as f64 / 72.0).sin()], r))).collect()
}

/// Preview polyline of the arc from `a` through `m` to `b` (a straight line when collinear).
pub fn arc_3pt_polyline(a: P2, m: P2, b: P2) -> Vec<P2> {
    let Some((c, r)) = crate::model::circumcircle(a, m, b) else { return vec![a, b] };
    let ang = |p: P2| (p[1] - c[1]).atan2(p[0] - c[0]);
    let ccw = |from: f64, to: f64| (to - from).rem_euclid(TAU);
    let (s, e) = if ccw(ang(a), ang(m)) < ccw(ang(a), ang(b)) { (a, b) } else { (b, a) };
    let (a0, sweep) = arc_sweep(c, s, e);
    let n = ((sweep / (PI / 36.0)).ceil() as usize).max(2);
    (0..=n).map(|i| add(c, scale([(a0 + sweep * i as f64 / n as f64).cos(), (a0 + sweep * i as f64 / n as f64).sin()], r))).collect()
}

/// Orientation of a distance dimension from where its label is placed (Fusion: dragging the
/// label above/below gives a horizontal dimension, left/right a vertical one, along the
/// perpendicular an aligned one).
pub fn distance_orientation(pa: P2, pb: P2, label: P2) -> DistanceOrientation {
    let (x0, x1) = (pa[0].min(pb[0]), pa[0].max(pb[0]));
    let (y0, y1) = (pa[1].min(pb[1]), pa[1].max(pb[1]));
    let in_x = label[0] >= x0 && label[0] <= x1;
    let in_y = label[1] >= y0 && label[1] <= y1;
    let degenerate_x = x1 - x0 < 1e-9;
    let degenerate_y = y1 - y0 < 1e-9;
    if degenerate_x || degenerate_y {
        return DistanceOrientation::Aligned;
    }
    match (in_x, in_y) {
        (true, false) => DistanceOrientation::Horizontal,
        (false, true) => DistanceOrientation::Vertical,
        _ => DistanceOrientation::Aligned,
    }
}

/// The two points a distance dimension measures (a line id given twice stands for its ends).
pub fn distance_points(s: &Sketch, a: &str, b: &str) -> Option<(P2, P2)> {
    if a == b {
        if let Some(l) = s.line_points(a) {
            return Some(l);
        }
    }
    let pa = s.line_points(a).map(|(p, _)| p).or_else(|| s.point(a))?;
    let pb = s.line_points(b).map(|(_, q)| q).or_else(|| s.point(b))?;
    Some((pa, pb))
}

fn circle_of(s: &Sketch, id: &str) -> Option<(P2, f64)> {
    match &s.entity(id)?.kind {
        EntityKind::Circle { c, r } | EntityKind::Arc { c, r, .. } => Some((s.point(c)?, *r)),
        _ => None,
    }
}

/// Intersection of the infinite lines through (a1, a2) and (b1, b2).
pub fn line_intersection(a1: P2, a2: P2, b1: P2, b2: P2) -> Option<P2> {
    let (da, db) = (sub(a2, a1), sub(b2, b1));
    let den = cross(da, db);
    if den.abs() < 1e-12 * len(da).max(1e-12) * len(db).max(1e-12) {
        return None;
    }
    let t = cross(sub(b1, a1), db) / den;
    Some(add(a1, scale(da, t)))
}

/// Extension / dimension lines of a dimension constraint with its label at `label`.
pub fn dimension_graphics(s: &Sketch, kind: &ConstraintKind, label: P2) -> Vec<(P2, P2)> {
    let mut out = Vec::new();
    match kind {
        ConstraintKind::Distance { a, b, orientation } => {
            let Some((pa, pb)) = distance_points(s, a, b) else { return out };
            let (qa, qb) = match orientation {
                DistanceOrientation::Horizontal => ([pa[0], label[1]], [pb[0], label[1]]),
                DistanceOrientation::Vertical => ([label[0], pa[1]], [label[0], pb[1]]),
                DistanceOrientation::Aligned => match unit(sub(pb, pa)) {
                    Some(u) => {
                        let n = [-u[1], u[0]];
                        let o = dot(sub(label, pa), n);
                        (add(pa, scale(n, o)), add(pb, scale(n, o)))
                    }
                    None => (pa, pb),
                },
            };
            out.push((pa, qa));
            out.push((pb, qb));
            out.push((qa, qb));
            // leader from the dimension line to a label placed beyond its ends
            let near = closest_on_segment(label, qa, qb);
            if dist(near, label) > 1e-9 {
                out.push((near, label));
            }
        }
        ConstraintKind::PointLineDistance { p, line } => {
            let (Some(pp), Some((a, b))) = (s.point(p), s.line_points(line)) else { return out };
            let f = foot_on_line(pp, a, b);
            out.push((pp, f));
            let near = closest_on_segment(f, a, b);
            if dist(near, f) > 1e-9 {
                out.push((near, f));
            }
            let mid = closest_on_segment(label, pp, f);
            if dist(mid, label) > 1e-9 {
                out.push((mid, label));
            }
        }
        ConstraintKind::Radius { curve } | ConstraintKind::Diameter { curve } => {
            let Some((c, r)) = circle_of(s, curve) else { return out };
            let u = unit(sub(label, c)).unwrap_or([1.0, 0.0]);
            let rim = add(c, scale(u, r));
            if matches!(kind, ConstraintKind::Diameter { .. }) {
                out.push((add(c, scale(u, -r)), rim));
            } else {
                out.push((c, rim));
            }
            if dist(label, c) > r {
                out.push((rim, label));
            }
        }
        ConstraintKind::Angle { a, b } => {
            let (Some((a1, a2)), Some((b1, b2))) = (s.line_points(a), s.line_points(b)) else { return out };
            let Some(x) = line_intersection(a1, a2, b1, b2) else {
                out.push((lerp(a1, a2, 0.5), lerp(b1, b2, 0.5)));
                return out;
            };
            // rays from the intersection towards the far ends of each line
            let far = |p: P2, q: P2| if dist(p, x) > dist(q, x) { p } else { q };
            let (Some(ua), Some(ub)) = (unit(sub(far(a1, a2), x)), unit(sub(far(b1, b2), x))) else { return out };
            let rad = dist(label, x).max(1e-6);
            let (t0, t1) = (ua[1].atan2(ua[0]), ub[1].atan2(ub[0]));
            let mut sweep = (t1 - t0).rem_euclid(TAU);
            let mut start = t0;
            if sweep > PI {
                sweep = TAU - sweep;
                start = t1;
            }
            let n = ((sweep / (PI / 36.0)).ceil() as usize).max(2);
            let pts: Vec<P2> = (0..=n).map(|i| add(x, scale([(start + sweep * i as f64 / n as f64).cos(), (start + sweep * i as f64 / n as f64).sin()], rad))).collect();
            for w in pts.windows(2) {
                out.push((w[0], w[1]));
            }
            // extension lines when the arc lies beyond the drawn lines
            for (u, p, q) in [(ua, a1, a2), (ub, b1, b2)] {
                let reach = dist(far(p, q), x);
                if rad > reach {
                    out.push((add(x, scale(u, reach)), add(x, scale(u, rad))));
                }
            }
        }
        _ => {}
    }
    out
}

/// Where a constraint glyph for `entity` sits: next to the curve's midpoint (lines), on the rim
/// (circles / arcs) or beside the point, offset by `off`; `k` stacks several glyphs.
pub fn glyph_anchor(s: &Sketch, entity: &str, off: f64, k: usize) -> Option<P2> {
    let step = off * 1.4 * k as f64;
    if let Some((a, b)) = s.line_points(entity) {
        let u = unit(sub(b, a)).unwrap_or([1.0, 0.0]);
        let n = [-u[1], u[0]];
        let m = lerp(a, b, 0.5);
        return Some(add(add(m, scale(n, off)), scale(u, step)));
    }
    if let Some(e) = s.entity(entity) {
        match &e.kind {
            EntityKind::Circle { .. } | EntityKind::Arc { .. } => {
                let pts = crate::profiles::entity_polyline(s, e);
                let (c, r) = circle_of(s, entity)?;
                let m = pts.get(pts.len() / 3).copied().unwrap_or(add(c, [r, 0.0]));
                let u = unit(sub(m, c)).unwrap_or([1.0, 0.0]);
                let t = [-u[1], u[0]];
                return Some(add(add(m, scale(u, off)), scale(t, step)));
            }
            _ => {}
        }
    }
    let p = s.point(entity)?;
    Some(add(p, [off + step, off]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_horizontal_and_vertical_within_tolerance() {
        assert_eq!(infer_axis([0.0, 0.0], [40.0, 0.7], 3.0), Some(Axis::Horizontal));
        assert_eq!(infer_axis([0.0, 0.0], [-0.5, -30.0], 3.0), Some(Axis::Vertical));
        assert_eq!(infer_axis([0.0, 0.0], [10.0, 10.0], 3.0), None);
        assert_eq!(project_to_axis([1.0, 2.0], [9.0, 2.3], Axis::Horizontal), [9.0, 2.0]);
    }

    #[test]
    fn nice_steps() {
        assert_eq!(nice_step(0.5), 0.5);
        assert_eq!(nice_step(0.7), 1.0);
        assert_eq!(nice_step(1.3), 2.0);
        assert_eq!(nice_step(3.0), 5.0);
        assert!((nice_step(0.03) - 0.05).abs() < 1e-12);
        assert_eq!(snap_to_grid([1.26, -0.74], 0.5), [1.5, -0.5]);
    }

    #[test]
    fn point_and_curve_hits() {
        let mut s = Sketch::new();
        let l = s.add_line([5.0, 5.0], [25.0, 5.0]);
        let c = s.add_circle([50.0, 0.0], 10.0);
        let (id, _) = nearest_point(&s, [25.2, 5.1], 0.5, &[]).unwrap();
        assert_eq!(s.line_points(&l).unwrap().1, s.point(&id).unwrap());
        assert_eq!(nearest_point(&s, [0.1, -0.1], 0.5, &[]).unwrap().0, ORIGIN);
        assert!(nearest_point(&s, [0.1, -0.1], 0.5, &[ORIGIN]).is_none());
        let (cid, q, _) = nearest_curve(&s, [15.0, 5.3], 0.5, |_| true).unwrap();
        assert_eq!(cid, l);
        assert!((q[1] - 5.0).abs() < 1e-12);
        let (cid, q, _) = nearest_curve(&s, [60.2, 0.0], 0.5, |_| true).unwrap();
        assert_eq!(cid, c);
        assert!((q[0] - 60.0).abs() < 1e-12);
        assert!(nearest_curve(&s, [30.0, 30.0], 0.5, |_| true).is_none());
    }

    #[test]
    fn arc_hit_respects_sweep() {
        let mut s = Sketch::new();
        let a = s.add_arc_3pt([10.0, 0.0], [0.0, 10.0], [-10.0, 0.0]).unwrap();
        let e = s.entity(&a).unwrap().clone();
        let (q, d) = closest_on_curve(&s, &e, [0.0, 11.0]).unwrap();
        assert!(d < 1.0 + 1e-9 && (q[1] - 10.0).abs() < 1e-9);
        // below the centre: outside the upper half arc, nearest is an endpoint
        let (_, d) = closest_on_curve(&s, &e, [0.0, -10.0]).unwrap();
        assert!(d > 10.0);
    }

    #[test]
    fn orientation_follows_label_placement() {
        let (a, b) = ([0.0, 0.0], [40.0, 20.0]);
        assert_eq!(distance_orientation(a, b, [20.0, -8.0]), DistanceOrientation::Horizontal);
        assert_eq!(distance_orientation(a, b, [48.0, 10.0]), DistanceOrientation::Vertical);
        assert_eq!(distance_orientation(a, b, [15.0, 12.0]), DistanceOrientation::Aligned);
        assert_eq!(distance_orientation([0.0, 0.0], [40.0, 0.0], [20.0, -8.0]), DistanceOrientation::Aligned);
    }

    #[test]
    fn dimension_lines_for_each_kind() {
        let mut s = Sketch::new();
        let lines = s.add_rectangle([0.0, 0.0], [40.0, 20.0]);
        let g = dimension_graphics(&s, &ConstraintKind::Distance { a: lines[0].clone(), b: lines[0].clone(), orientation: DistanceOrientation::Horizontal }, [20.0, -8.0]);
        assert!(g.iter().any(|(p, q)| (p[1] + 8.0).abs() < 1e-9 && (q[1] + 8.0).abs() < 1e-9 && (dist(*p, *q) - 40.0).abs() < 1e-9));
        let c = s.add_circle([80.0, 0.0], 5.0);
        let g = dimension_graphics(&s, &ConstraintKind::Diameter { curve: c }, [80.0, 20.0]);
        assert!((dist(g[0].0, g[0].1) - 10.0).abs() < 1e-9);
        let g = dimension_graphics(&s, &ConstraintKind::Angle { a: lines[0].clone(), b: lines[1].clone() }, [35.0, 5.0]);
        assert!(g.len() >= 2);
        let p = s.add_point(10.0, 30.0, None);
        let g = dimension_graphics(&s, &ConstraintKind::PointLineDistance { p, line: lines[0].clone() }, [12.0, 15.0]);
        assert!((dist(g[0].0, g[0].1) - 30.0).abs() < 1e-9);
    }

    #[test]
    fn arc_preview_passes_through_points() {
        let pts = arc_3pt_polyline([10.0, 0.0], [0.0, 10.0], [-10.0, 0.0]);
        assert!(pts.iter().all(|p| (len(*p) - 10.0).abs() < 1e-9));
        assert!(pts.iter().all(|p| p[1] >= -1e-9));
        assert_eq!(arc_3pt_polyline([0.0, 0.0], [1.0, 0.0], [2.0, 0.0]).len(), 2);
    }
}
