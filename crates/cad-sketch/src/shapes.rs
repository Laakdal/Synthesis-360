//! Builders for the compound sketch shapes of the Create panel: 3-point rectangle, centre-point
//! arc, regular polygon and slot. Each builds shared-point geometry plus the constraints the
//! matching Fusion tool implies; automatic constraints are only added when they contribute an
//! independent equation, so a shape never over-constrains the sketch it is drawn into.

use crate::interact::{add, cross, dist, scale, sub, unit, P2};
use crate::model::{ConstraintKind, Entity, EntityId, EntityKind, Sketch};
use crate::solver::{would_overconstrain, DimensionValues};
use std::f64::consts::{PI, TAU};

/// Adds `kind` unless it would over-constrain the sketch.
pub fn add_if_independent(sk: &mut Sketch, vals: &DimensionValues, kind: ConstraintKind) -> bool {
    if would_overconstrain(sk, &kind, None, vals) {
        return false;
    }
    sk.add_constraint(kind);
    true
}

/// Regular polygon: the construction circle it is inscribed in / circumscribed about, its
/// centre point, the vertices (CCW) and the edges (`lines[i]` joins `vertices[i]` and
/// `vertices[i + 1]`).
#[derive(Clone, Debug)]
pub struct PolygonShape {
    pub circle: EntityId,
    pub center: EntityId,
    pub vertices: Vec<EntityId>,
    pub lines: Vec<EntityId>,
}

/// Slot: arc centres, the two end arcs, the two side lines and the construction centre line.
#[derive(Clone, Debug)]
pub struct SlotShape {
    pub centers: [EntityId; 2],
    pub arcs: [EntityId; 2],
    pub lines: [EntityId; 2],
    pub axis: EntityId,
}

/// Vertex positions of a regular polygon (CCW). Inscribed: `at` is a vertex; circumscribed:
/// `at` is the midpoint of an edge.
pub fn polygon_vertices(c: P2, at: P2, sides: usize, inscribed: bool) -> Option<Vec<P2>> {
    let d = dist(c, at);
    if sides < 3 || d < 1e-9 {
        return None;
    }
    let half = PI / sides as f64;
    let phi = (at[1] - c[1]).atan2(at[0] - c[0]);
    let (radius, phi0) = if inscribed { (d, phi) } else { (d / half.cos(), phi - half) };
    Some((0..sides).map(|i| {
        let a = phi0 + TAU * i as f64 / sides as f64;
        add(c, scale([a.cos(), a.sin()], radius))
    }).collect())
}

/// Corner positions of a 3-point rectangle: edge `a`-`b`, height from the signed distance of
/// `h` to that edge (positive to the left of `a`->`b`).
pub fn rectangle_3pt_corners(a: P2, b: P2, h: P2) -> Option<[P2; 4]> {
    let u = unit(sub(b, a))?;
    let n = [-u[1], u[0]];
    let height = crate::interact::dot(sub(h, a), n);
    if height.abs() < 1e-9 {
        return None;
    }
    let off = scale(n, height);
    Some([a, b, add(b, off), add(a, off)])
}

/// Outline of a slot between the centres `c1`, `c2` with radius `r`: (corner points
/// [c1-side right, c2-side right, c2-side left, c1-side left] relative to `c1`->`c2`).
pub fn slot_corners(c1: P2, c2: P2, r: f64) -> Option<[P2; 4]> {
    let u = unit(sub(c2, c1))?;
    if r < 1e-9 {
        return None;
    }
    let n = scale([-u[1], u[0]], r);
    Some([sub(c1, n), sub(c2, n), add(c2, n), add(c1, n)])
}

/// Width point -> slot radius: distance from `p` to the centre segment.
pub fn slot_radius(c1: P2, c2: P2, p: P2) -> f64 {
    dist(p, crate::interact::closest_on_segment(p, c1, c2))
}

impl Sketch {
    fn own(&mut self, point: &str, owner: &str) {
        if let Some(e) = self.entity_mut(point) {
            e.owner.get_or_insert_with(|| owner.to_string());
        }
    }

    /// Counter-clockwise arc around the existing point `c` from the existing point `start` to
    /// the existing point `end` (radius from `c`-`start`).
    pub fn add_arc_between(&mut self, c: &str, start: &str, end: &str) -> Option<EntityId> {
        let r = dist(self.point(c)?, self.point(start)?);
        if r < 1e-9 || start == end {
            return None;
        }
        let id = self.alloc_id("a");
        self.entities.push(Entity { id: id.clone(), kind: EntityKind::Arc { c: c.into(), start: start.into(), end: end.into(), r }, construction: false, owner: None });
        for p in [c, start, end] {
            self.own(p, &id);
        }
        Some(id)
    }

    /// Centre-point arc: radius from `c`-`start`, ending in the direction of `toward`;
    /// `ccw` gives the direction of travel from `start`. Returns (arc, start point, end point);
    /// the arc's own start/end are swapped for a clockwise arc (arcs are stored CCW).
    pub fn add_arc_center(&mut self, c: P2, start: P2, toward: P2, ccw: bool) -> Option<(EntityId, EntityId, EntityId)> {
        let r = dist(c, start);
        let u = unit(sub(toward, c))?;
        let end = add(c, scale(u, r));
        if r < 1e-9 || dist(start, end) < 1e-9 {
            return None;
        }
        let cp = self.add_point(c[0], c[1], None);
        let sp = self.add_point(start[0], start[1], None);
        let ep = self.add_point(end[0], end[1], None);
        let id = if ccw { self.add_arc_between(&cp, &sp, &ep)? } else { self.add_arc_between(&cp, &ep, &sp)? };
        Some((id, sp, ep))
    }

    /// Closed polyline through existing points (line `i` joins `pts[i]` and `pts[i + 1]`).
    fn add_closed_polyline(&mut self, pts: &[EntityId]) -> Vec<EntityId> {
        let n = pts.len();
        let lines: Vec<EntityId> = (0..n).map(|i| self.add_line_between(&pts[i].clone(), &pts[(i + 1) % n].clone())).collect();
        for (i, l) in lines.iter().enumerate() {
            self.own(&pts[i].clone(), l);
            self.own(&pts[(i + 1) % n].clone(), l);
        }
        lines
    }

    /// 3-point rectangle (first edge `a`-`b`, height through `h`). Lines [first edge, then
    /// CCW/CW around]; adjacent edges perpendicular, opposite edges parallel.
    pub fn add_rectangle_3pt(&mut self, vals: &DimensionValues, a: P2, b: P2, h: P2) -> Option<[EntityId; 4]> {
        let corners = rectangle_3pt_corners(a, b, h)?;
        let pts: Vec<EntityId> = corners.iter().map(|c| self.add_point(c[0], c[1], None)).collect();
        let l = self.add_closed_polyline(&pts);
        add_if_independent(self, vals, ConstraintKind::Perpendicular { a: l[0].clone(), b: l[1].clone() });
        add_if_independent(self, vals, ConstraintKind::Parallel { a: l[0].clone(), b: l[2].clone() });
        add_if_independent(self, vals, ConstraintKind::Parallel { a: l[1].clone(), b: l[3].clone() });
        Some([l[0].clone(), l[1].clone(), l[2].clone(), l[3].clone()])
    }

    /// Regular polygon with a construction circle: vertices on the circle (inscribed) or edges
    /// tangent to it (circumscribed), and equal edges.
    pub fn add_polygon(&mut self, vals: &DimensionValues, c: P2, at: P2, sides: usize, inscribed: bool) -> Option<PolygonShape> {
        let verts = polygon_vertices(c, at, sides, inscribed)?;
        let r = dist(c, at);
        let circle = self.add_circle(c, r);
        let center = match &self.entity(&circle)?.kind {
            EntityKind::Circle { c, .. } => c.clone(),
            _ => return None,
        };
        if let Some(e) = self.entity_mut(&circle) {
            e.construction = true;
        }
        let vertices: Vec<EntityId> = verts.iter().map(|v| self.add_point(v[0], v[1], None)).collect();
        let lines = self.add_closed_polyline(&vertices);
        if inscribed {
            for v in &vertices {
                add_if_independent(self, vals, ConstraintKind::PointOnCurve { p: v.clone(), curve: circle.clone() });
            }
        } else {
            for l in &lines {
                add_if_independent(self, vals, ConstraintKind::Tangent { a: l.clone(), b: circle.clone() });
            }
        }
        for l in &lines[1..] {
            add_if_independent(self, vals, ConstraintKind::Equal { a: lines[0].clone(), b: l.clone() });
        }
        Some(PolygonShape { circle, center, vertices, lines })
    }

    /// Centre-to-centre slot: two tangent side lines, two end arcs of equal radius and a
    /// construction line between the arc centres.
    pub fn add_slot(&mut self, vals: &DimensionValues, c1: P2, c2: P2, r: f64) -> Option<SlotShape> {
        let k = slot_corners(c1, c2, r)?;
        let cp1 = self.add_point(c1[0], c1[1], None);
        let cp2 = self.add_point(c2[0], c2[1], None);
        let p: Vec<EntityId> = k.iter().map(|q| self.add_point(q[0], q[1], None)).collect();
        // right side c1 -> c2, arc around c2 (CCW from right to left), left side c2 -> c1,
        // arc around c1 (CCW from left to right)
        let l1 = self.add_line_between(&p[0], &p[1]);
        let a2 = self.add_arc_between(&cp2, &p[1], &p[2])?;
        let l2 = self.add_line_between(&p[2], &p[3]);
        let a1 = self.add_arc_between(&cp1, &p[3], &p[0])?;
        let axis = self.add_line_between(&cp1, &cp2);
        if let Some(e) = self.entity_mut(&axis) {
            e.construction = true;
        }
        for (pt, l) in [(&p[0], &l1), (&p[1], &l1), (&p[2], &l2), (&p[3], &l2)] {
            if let Some(e) = self.entity_mut(pt) {
                e.owner = Some(l.clone());
            }
        }
        for (l, a) in [(&l1, &a1), (&l1, &a2), (&l2, &a1), (&l2, &a2)] {
            add_if_independent(self, vals, ConstraintKind::Tangent { a: l.clone(), b: a.clone() });
        }
        add_if_independent(self, vals, ConstraintKind::Equal { a: a1.clone(), b: a2.clone() });
        Some(SlotShape { centers: [cp1, cp2], arcs: [a1, a2], lines: [l1, l2], axis })
    }
}

/// Sign of the turn from `a` to `b` around `c` (positive = counter-clockwise), in (-PI, PI].
pub fn turn(c: P2, a: P2, b: P2) -> f64 {
    let (u, v) = (sub(a, c), sub(b, c));
    cross(u, v).atan2(crate::interact::dot(u, v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{find_profiles, solve};

    fn solved_area(s: &mut Sketch) -> Vec<f64> {
        let r = solve(s, &DimensionValues::new(), &[]);
        assert_eq!(r.status, crate::SolveStatus::Solved);
        let mut a: Vec<f64> = find_profiles(s).iter().map(|p| p.area).collect();
        a.sort_by(f64::total_cmp);
        a
    }

    #[test]
    fn hexagon_area_and_dof() {
        let mut s = Sketch::new();
        let shape = s.add_polygon(&DimensionValues::new(), [10.0, 5.0], [30.0, 5.0], 6, true).unwrap();
        assert_eq!(shape.lines.len(), 6);
        let r = solve(&mut s, &DimensionValues::new(), &[]);
        // centre (2) + size (1) + rotation (1)
        assert_eq!(r.dof, 4);
        let a = solved_area(&mut s);
        assert_eq!(a.len(), 1);
        assert!((a[0] - 1.5 * 3f64.sqrt() * 400.0).abs() < 1e-6, "{a:?}");
    }

    #[test]
    fn circumscribed_square_has_apothem_at_click() {
        let mut s = Sketch::new();
        s.add_polygon(&DimensionValues::new(), [0.0, 0.0], [10.0, 0.0], 4, false).unwrap();
        let a = solved_area(&mut s);
        assert!((a[0] - 400.0).abs() < 1e-6, "{a:?}");
    }

    #[test]
    fn slot_area_and_dof() {
        let mut s = Sketch::new();
        s.add_slot(&DimensionValues::new(), [0.0, 0.0], [30.0, 10.0], 4.0).unwrap();
        let l = dist([0.0, 0.0], [30.0, 10.0]);
        let a = solved_area(&mut s);
        assert_eq!(a.len(), 1);
        assert!((a[0] - (PI * 16.0 + 8.0 * l)).abs() < 1e-6, "{a:?}");
        let r = solve(&mut s, &DimensionValues::new(), &[]);
        assert_eq!(r.dof, 5, "two centres + radius");
    }

    #[test]
    fn three_point_rectangle() {
        let mut s = Sketch::new();
        s.add_rectangle_3pt(&DimensionValues::new(), [0.0, 0.0], [30.0, 40.0], [-40.0, 30.0]).unwrap();
        let a = solved_area(&mut s);
        assert!((a[0] - 50.0 * 50.0).abs() < 1e-6, "{a:?}");
        assert_eq!(solve(&mut s, &DimensionValues::new(), &[]).dof, 5);
    }

    #[test]
    fn center_arc_direction() {
        let mut s = Sketch::new();
        let (ccw, _, _) = s.add_arc_center([0.0, 0.0], [10.0, 0.0], [0.0, 5.0], true).unwrap();
        let (cw, _, _) = s.add_arc_center([0.0, 0.0], [10.0, 0.0], [0.0, 5.0], false).unwrap();
        let sweep = |id: &str| {
            let e = s.entity(id).unwrap();
            let pts = crate::profiles::entity_polyline(&s, e);
            pts.iter().all(|p| p[1] >= -1e-9)
        };
        assert!(sweep(&ccw), "CCW quarter stays in the upper half");
        assert!(!sweep(&cw), "CW three-quarter arc goes below");
        assert!((turn([0.0, 0.0], [1.0, 0.0], [0.0, 1.0]) - PI / 2.0).abs() < 1e-12);
    }
}
