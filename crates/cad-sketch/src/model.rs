//! Sketch entities and constraints, in sketch-plane coordinates (millimetres).
//!
//! Every sketch has an implicit fixed point with id [`ORIGIN`]: Fusion projects the origin into
//! each sketch, and a rectangle corner made coincident with it fixes the sketch's position
//! (observed: H/V + origin-coincident + 2 dimensions => `isFullyConstrained == true`).

use serde::{Deserialize, Serialize};

pub const ORIGIN: &str = "origin";

pub type EntityId = String;
pub type ConstraintId = String;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EntityKind {
    Point { x: f64, y: f64 },
    Line { p1: EntityId, p2: EntityId },
    Circle { c: EntityId, r: f64 },
    /// Counter-clockwise arc around `c` from `start` to `end`.
    Arc { c: EntityId, start: EntityId, end: EntityId, r: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub id: EntityId,
    #[serde(flatten)]
    pub kind: EntityKind,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub construction: bool,
    /// Curve that owns this point (endpoint / centre); free points have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<EntityId>,
}

impl Entity {
    pub fn is_curve(&self) -> bool {
        !matches!(self.kind, EntityKind::Point { .. })
    }
    /// Point ids that define this entity.
    pub fn points(&self) -> Vec<&str> {
        match &self.kind {
            EntityKind::Point { .. } => vec![self.id.as_str()],
            EntityKind::Line { p1, p2 } => vec![p1, p2],
            EntityKind::Circle { c, .. } => vec![c],
            EntityKind::Arc { c, start, end, .. } => vec![c, start, end],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DistanceOrientation {
    Aligned,
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ConstraintKind {
    Coincident { a: EntityId, b: EntityId },
    PointOnCurve { p: EntityId, curve: EntityId },
    Horizontal { line: EntityId },
    Vertical { line: EntityId },
    Parallel { a: EntityId, b: EntityId },
    Perpendicular { a: EntityId, b: EntityId },
    Collinear { a: EntityId, b: EntityId },
    Equal { a: EntityId, b: EntityId },
    Tangent { a: EntityId, b: EntityId },
    Concentric { a: EntityId, b: EntityId },
    Midpoint { p: EntityId, line: EntityId },
    Fix { p: EntityId, x: f64, y: f64 },
    Symmetric { p1: EntityId, p2: EntityId, line: EntityId },
    // ---- dimensions: the value lives in a document parameter
    Distance { a: EntityId, b: EntityId, orientation: DistanceOrientation },
    PointLineDistance { p: EntityId, line: EntityId },
    Radius { curve: EntityId },
    Diameter { curve: EntityId },
    Angle { a: EntityId, b: EntityId },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Constraint {
    pub id: ConstraintId,
    #[serde(flatten)]
    pub kind: ConstraintKind,
    /// Dimensions only: name of the parameter holding the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    /// Dimensions only: driven (reference) dimensions measure but do not constrain.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub driven: bool,
    /// Dimensions only: label position in sketch coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<[f64; 2]>,
}

impl ConstraintKind {
    pub fn is_dimension(&self) -> bool {
        matches!(
            self,
            ConstraintKind::Distance { .. }
                | ConstraintKind::PointLineDistance { .. }
                | ConstraintKind::Radius { .. }
                | ConstraintKind::Diameter { .. }
                | ConstraintKind::Angle { .. }
        )
    }
    pub fn is_angle(&self) -> bool {
        matches!(self, ConstraintKind::Angle { .. })
    }
    pub fn label(&self) -> &'static str {
        match self {
            ConstraintKind::Coincident { .. } | ConstraintKind::PointOnCurve { .. } => "Coincident",
            ConstraintKind::Horizontal { .. } => "Horizontal",
            ConstraintKind::Vertical { .. } => "Vertical",
            ConstraintKind::Parallel { .. } => "Parallel",
            ConstraintKind::Perpendicular { .. } => "Perpendicular",
            ConstraintKind::Collinear { .. } => "Collinear",
            ConstraintKind::Equal { .. } => "Equal",
            ConstraintKind::Tangent { .. } => "Tangent",
            ConstraintKind::Concentric { .. } => "Concentric",
            ConstraintKind::Midpoint { .. } => "Midpoint",
            ConstraintKind::Fix { .. } => "Fix",
            ConstraintKind::Symmetric { .. } => "Symmetry",
            ConstraintKind::Distance { .. } | ConstraintKind::PointLineDistance { .. } => "Distance",
            ConstraintKind::Radius { .. } => "Radius",
            ConstraintKind::Diameter { .. } => "Diameter",
            ConstraintKind::Angle { .. } => "Angle",
        }
    }
    /// Entity ids the constraint refers to.
    pub fn refs(&self) -> Vec<&str> {
        use ConstraintKind::*;
        match self {
            Coincident { a, b } | Parallel { a, b } | Perpendicular { a, b } | Collinear { a, b } | Equal { a, b }
            | Tangent { a, b } | Concentric { a, b } | Distance { a, b, .. } | Angle { a, b } => vec![a, b],
            PointOnCurve { p, curve } => vec![p, curve],
            Horizontal { line } | Vertical { line } => vec![line],
            Midpoint { p, line } | PointLineDistance { p, line } => vec![p, line],
            Fix { p, .. } => vec![p],
            Symmetric { p1, p2, line } => vec![p1, p2, line],
            Radius { curve } | Diameter { curve } => vec![curve],
        }
    }
}

/// A 2D sketch: entities plus constraints. Positions stored here are the last solved state and
/// serve as the initial guess for the next solve, so solutions do not flip between solves.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    pub entities: Vec<Entity>,
    pub constraints: Vec<Constraint>,
    #[serde(default)]
    pub next_id: u64,
}

impl Sketch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn alloc_id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}{}", self.next_id)
    }

    pub fn entity(&self, id: &str) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id == id)
    }
    pub fn entity_mut(&mut self, id: &str) -> Option<&mut Entity> {
        self.entities.iter_mut().find(|e| e.id == id)
    }
    pub fn constraint(&self, id: &str) -> Option<&Constraint> {
        self.constraints.iter().find(|c| c.id == id)
    }

    /// Position of a point entity (the origin is always (0, 0)).
    pub fn point(&self, id: &str) -> Option<[f64; 2]> {
        if id == ORIGIN {
            return Some([0.0, 0.0]);
        }
        match self.entity(id)?.kind {
            EntityKind::Point { x, y } => Some([x, y]),
            _ => None,
        }
    }
    pub fn set_point(&mut self, id: &str, p: [f64; 2]) {
        if let Some(Entity { kind: EntityKind::Point { x, y }, .. }) = self.entity_mut(id) {
            *x = p[0];
            *y = p[1];
        }
    }

    pub fn add_point(&mut self, x: f64, y: f64, owner: Option<&str>) -> EntityId {
        let id = self.alloc_id("p");
        self.entities.push(Entity { id: id.clone(), kind: EntityKind::Point { x, y }, construction: false, owner: owner.map(str::to_string) });
        id
    }

    /// Adds a line between two existing point ids.
    pub fn add_line_between(&mut self, p1: &str, p2: &str) -> EntityId {
        let id = self.alloc_id("l");
        self.entities.push(Entity { id: id.clone(), kind: EntityKind::Line { p1: p1.into(), p2: p2.into() }, construction: false, owner: None });
        id
    }

    pub fn add_line(&mut self, a: [f64; 2], b: [f64; 2]) -> EntityId {
        let id = self.alloc_id("l");
        let p1 = self.add_point(a[0], a[1], Some(&id));
        let p2 = self.add_point(b[0], b[1], Some(&id));
        self.entities.push(Entity { id: id.clone(), kind: EntityKind::Line { p1, p2 }, construction: false, owner: None });
        id
    }

    pub fn add_circle(&mut self, c: [f64; 2], r: f64) -> EntityId {
        let id = self.alloc_id("c");
        let cp = self.add_point(c[0], c[1], Some(&id));
        self.entities.push(Entity { id: id.clone(), kind: EntityKind::Circle { c: cp, r }, construction: false, owner: None });
        id
    }

    /// Arc through three points (start, a point on the arc, end).
    pub fn add_arc_3pt(&mut self, a: [f64; 2], m: [f64; 2], b: [f64; 2]) -> Option<EntityId> {
        let (c, r) = circumcircle(a, m, b)?;
        // orient counter-clockwise: if the mid point is not on the CCW path a->b, swap ends
        let ang = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
        let ccw = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
        let (s, e) = if ccw(ang(a), ang(m)) < ccw(ang(a), ang(b)) { (a, b) } else { (b, a) };
        let id = self.alloc_id("a");
        let cp = self.add_point(c[0], c[1], Some(&id));
        let sp = self.add_point(s[0], s[1], Some(&id));
        let ep = self.add_point(e[0], e[1], Some(&id));
        self.entities.push(Entity { id: id.clone(), kind: EntityKind::Arc { c: cp, start: sp, end: ep, r }, construction: false, owner: None });
        Some(id)
    }

    /// Two-point rectangle: four lines sharing corner points, with the horizontal/vertical
    /// constraints a rectangle tool implies. Returns the line ids [bottom, right, top, left].
    pub fn add_rectangle(&mut self, a: [f64; 2], b: [f64; 2]) -> [EntityId; 4] {
        let corners = [[a[0], a[1]], [b[0], a[1]], [b[0], b[1]], [a[0], b[1]]];
        let ids: Vec<EntityId> = corners.iter().map(|c| self.add_point(c[0], c[1], None)).collect();
        let lines: Vec<EntityId> = (0..4).map(|i| self.add_line_between(&ids[i].clone(), &ids[(i + 1) % 4].clone())).collect();
        for (i, l) in lines.iter().enumerate() {
            for p in [&ids[i], &ids[(i + 1) % 4]] {
                if let Some(e) = self.entity_mut(p) {
                    e.owner.get_or_insert_with(|| l.clone());
                }
            }
        }
        for (i, l) in lines.iter().enumerate() {
            let kind = if i % 2 == 0 { ConstraintKind::Horizontal { line: l.clone() } } else { ConstraintKind::Vertical { line: l.clone() } };
            self.add_constraint(kind);
        }
        [lines[0].clone(), lines[1].clone(), lines[2].clone(), lines[3].clone()]
    }

    pub fn add_constraint(&mut self, kind: ConstraintKind) -> ConstraintId {
        let id = self.alloc_id("k");
        self.constraints.push(Constraint { id: id.clone(), kind, param: None, driven: false, label: None });
        id
    }

    pub fn add_dimension(&mut self, kind: ConstraintKind, param: &str, label: [f64; 2], driven: bool) -> ConstraintId {
        let id = self.alloc_id("k");
        self.constraints.push(Constraint { id: id.clone(), kind, param: Some(param.into()), driven, label: Some(label) });
        id
    }

    /// Line endpoints.
    pub fn line_points(&self, id: &str) -> Option<([f64; 2], [f64; 2])> {
        match &self.entity(id)?.kind {
            EntityKind::Line { p1, p2 } => Some((self.point(p1)?, self.point(p2)?)),
            _ => None,
        }
    }

    /// Removes an entity, its owned points (unless shared) and every constraint referring to them.
    pub fn delete_entity(&mut self, id: &str) {
        let mut doomed: Vec<String> = vec![id.to_string()];
        if let Some(e) = self.entity(id) {
            if e.is_curve() {
                for p in e.points() {
                    let shared = self.entities.iter().any(|o| o.id != id && o.is_curve() && o.points().contains(&p));
                    if !shared {
                        doomed.push(p.to_string());
                    }
                }
            } else {
                // deleting a point deletes the curves built on it
                let users: Vec<String> = self.entities.iter().filter(|o| o.is_curve() && o.points().contains(&id)).map(|o| o.id.clone()).collect();
                for u in users {
                    self.delete_entity(&u);
                }
            }
        }
        self.entities.retain(|e| !doomed.contains(&e.id));
        self.constraints.retain(|c| !c.kind.refs().iter().any(|r| doomed.iter().any(|d| d == r)));
    }

    pub fn delete_constraint(&mut self, id: &str) {
        self.constraints.retain(|c| c.id != id);
    }
}

/// Centre and radius of the circle through three points; `None` when collinear.
pub fn circumcircle(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<([f64; 2], f64)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return None;
    }
    let sq = |p: [f64; 2]| p[0] * p[0] + p[1] * p[1];
    let ux = (sq(a) * (b[1] - c[1]) + sq(b) * (c[1] - a[1]) + sq(c) * (a[1] - b[1])) / d;
    let uy = (sq(a) * (c[0] - b[0]) + sq(b) * (a[0] - c[0]) + sq(c) * (b[0] - a[0])) / d;
    let r = ((a[0] - ux).powi(2) + (a[1] - uy).powi(2)).sqrt();
    Some(([ux, uy], r))
}
