//! Body-level modelling features: Shell, Combine, Mirror (bodies), Rectangular / Circular
//! Pattern (bodies) and Move/Copy.
//!
//! Child module of `recompute` (it works on the private evaluation `State`).
//!
//! Body identity: bodies a feature *creates* get `{feature_id}`, `{feature_id}~1`, ... from one
//! counter over every source body and instance (deterministic, in input order), with
//! `created_by = feature_id`. Bodies a feature *modifies* (shell, combine target, move without
//! copy, pattern with Join) keep their id and `created_by`, and stay at their place in the body
//! list, so downstream references to them keep resolving.

use super::{resolve_plane, Health, State};
use crate::body::{new_body_with, Body, BodyKind};
use crate::frame::{add, scale, V3};
use crate::naming::Named;
use crate::occ::Shape;
use crate::refs;
use cad_core::{BodyId, FaceRef, Feature, FeatureKind, Operation, OriginAxis};
use std::collections::{HashMap, HashSet};

/// Most instances a pattern may create (protects against "1000 x 1000" typos).
const MAX_INSTANCES: usize = 1000;

pub(super) fn evaluate(f: &Feature, state: &mut State, vals: &dyn Fn(&str) -> f64) -> Result<Health, String> {
    match &f.kind {
        FeatureKind::Shell { faces, thickness } => shell(state, &f.id, faces, vals(thickness)),
        FeatureKind::Combine { target, tools, operation, keep_tools } => combine(state, &f.id, target, tools, *operation, *keep_tools),
        FeatureKind::MirrorBodies { bodies, plane } => {
            let frame = resolve_plane(plane, state).map_err(|e| format!("The mirror plane reference is lost: {e}. Edit this feature and select a new plane."))?;
            let fell_back = super::plane_fell_back(plane, state);
            let h = copies(f, state, bodies, &[Xform::Mirror(frame.origin, frame.n)], false)?;
            Ok(if fell_back && h == Health::Healthy { Health::Warning(refs::fallback_warning(1)) } else { h })
        }
        FeatureKind::RectangularPattern { bodies, axis1, quantity1, distance1, axis2, quantity2, distance2, join } => {
            let q1 = quantity(vals(quantity1))?;
            let d1 = vals(distance1);
            let (a2, q2, d2) = match axis2 {
                Some(a) => (Some(*a), quantity(vals(quantity2))?, vals(distance2)),
                None => (None, 1, 0.0),
            };
            if a2 == Some(*axis1) && q2 > 1 {
                return Err("The two pattern directions must be different axes.".into());
            }
            if q1.saturating_mul(q2) > MAX_INSTANCES {
                return Err(format!("Too many instances ({} x {}); at most {MAX_INSTANCES} are allowed.", q1, q2));
            }
            if (q1 > 1 && d1.abs() < 1e-9) || (q2 > 1 && d2.abs() < 1e-9) {
                return Err("Distance must not be zero.".into());
            }
            let u1 = axis_dir(*axis1);
            let u2 = a2.map(axis_dir).unwrap_or([0.0; 3]);
            let mut xf = Vec::new();
            for j in 0..q2 {
                for i in 0..q1 {
                    if i == 0 && j == 0 {
                        continue;
                    }
                    xf.push(Xform::Rigid(translation(add(scale(u1, i as f64 * d1), scale(u2, j as f64 * d2)))));
                }
            }
            copies(f, state, bodies, &xf, *join)
        }
        FeatureKind::CircularPattern { bodies, axis, quantity: q, angle, join } => {
            let n = quantity(vals(q))?;
            let total = vals(angle);
            if n > MAX_INSTANCES {
                return Err(format!("Too many instances ({n}); at most {MAX_INSTANCES} are allowed."));
            }
            if total.abs() > 360.0 + 1e-9 {
                return Err("The total angle must be between -360 deg and 360 deg.".into());
            }
            if n > 1 && total.abs() < 1e-9 {
                return Err("Angle must not be zero.".into());
            }
            // Fusion "Full": 360 deg spaces N instances evenly; a partial angle includes both ends
            let step = if (total.abs() - 360.0).abs() < 1e-9 { total / n as f64 } else { total / (n.max(2) - 1) as f64 };
            let xf: Vec<Xform> = (1..n).map(|k| Xform::Rigid(rotation(*axis, k as f64 * step))).collect();
            copies(f, state, bodies, &xf, *join)
        }
        FeatureKind::MoveBodies { bodies, dx, dy, dz, axis, angle, copy } => {
            let m = compose(translation([vals(dx), vals(dy), vals(dz)]), rotation(*axis, vals(angle)));
            if *copy {
                copies(f, state, bodies, &[Xform::Rigid(m)], false)
            } else {
                move_in_place(state, bodies, &m)
            }
        }
        _ => Ok(Health::Healthy),
    }
}

// ---------------------------------------------------------------------------------- transforms

/// Row-major 3x4 rigid transform [R | t].
type Rigid = [f64; 12];

#[derive(Clone, Copy)]
enum Xform {
    Rigid(Rigid),
    /// Mirror across the plane through the point with the normal.
    Mirror(V3, V3),
}

fn axis_dir(a: OriginAxis) -> V3 {
    match a {
        OriginAxis::X => [1.0, 0.0, 0.0],
        OriginAxis::Y => [0.0, 1.0, 0.0],
        OriginAxis::Z => [0.0, 0.0, 1.0],
    }
}

fn translation(t: V3) -> Rigid {
    [1.0, 0.0, 0.0, t[0], 0.0, 1.0, 0.0, t[1], 0.0, 0.0, 1.0, t[2]]
}

/// Rotation by `deg` about the origin axis (right-hand rule), through the origin.
fn rotation(axis: OriginAxis, deg: f64) -> Rigid {
    let u = axis_dir(axis);
    let (s, c) = deg.to_radians().sin_cos();
    // snap the exact quarter turns so 90/180 deg patterns are exact
    let snap = |v: f64| if v.abs() < 1e-15 { 0.0 } else { v };
    let (s, c) = (snap(s), snap(c));
    let k = 1.0 - c;
    let (x, y, z) = (u[0], u[1], u[2]);
    [
        c + x * x * k,
        x * y * k - z * s,
        x * z * k + y * s,
        0.0,
        y * x * k + z * s,
        c + y * y * k,
        y * z * k - x * s,
        0.0,
        z * x * k - y * s,
        z * y * k + x * s,
        c + z * z * k,
        0.0,
    ]
}

/// `a` after `b` (x -> a(b(x))).
fn compose(a: Rigid, b: Rigid) -> Rigid {
    let mut out = [0.0; 12];
    for r in 0..3 {
        for col in 0..4 {
            let mut v = 0.0;
            for k in 0..3 {
                v += a[r * 4 + k] * b[k * 4 + col];
            }
            if col == 3 {
                v += a[r * 4 + 3];
            }
            out[r * 4 + col] = v;
        }
    }
    out
}

/// Transformed copy; each face name goes through `rename` (pattern instances get distinct names
/// so references to the original stay unambiguous after a joined pattern).
fn apply(shape: &Named, x: &Xform, rename: impl Fn(&str) -> String) -> Result<Named, String> {
    match x {
        Xform::Rigid(m) => shape.transformed(0, m, rename),
        Xform::Mirror(p, n) => shape.transformed(1, &[p[0], p[1], p[2], n[0], n[1], n[2]], rename),
    }
    .map_err(|e| e.0)
}

// ---------------------------------------------------------------------------------- helpers

/// Pattern quantity from a unitless parameter value (counts the original).
fn quantity(v: f64) -> Result<usize, String> {
    if !v.is_finite() || v < 1.0 - 1e-9 {
        return Err("Quantity must be at least 1.".into());
    }
    if (v - v.round()).abs() > 1e-6 {
        return Err(format!("Quantity must be a whole number (got {v}).",));
    }
    Ok(v.round() as usize)
}

/// Sequential ids for bodies created by one feature: `fid`, `fid~1`, `fid~2`, ...
struct IdAlloc<'a> {
    fid: &'a str,
    next: usize,
}

impl IdAlloc<'_> {
    fn next(&mut self) -> BodyId {
        let k = self.next;
        self.next += 1;
        if k == 0 { self.fid.to_string() } else { format!("{}~{k}", self.fid) }
    }
}

/// Positions of the referenced bodies that exist (deduplicated, input order) and the number of
/// lost references.
fn find_bodies(state: &State, ids: &[BodyId]) -> (Vec<usize>, usize) {
    let mut found: Vec<usize> = Vec::new();
    let mut lost = 0;
    for id in ids {
        match state.bodies.iter().position(|b| b.id == *id) {
            Some(p) => {
                if !found.contains(&p) {
                    found.push(p)
                }
            }
            None => lost += 1,
        }
    }
    (found, lost)
}

fn lost_bodies_error(lost: usize) -> String {
    format!("{lost} reference failure(s): the selected bodies no longer exist. Edit this feature and reselect the bodies.")
}

fn partial_loss(lost: usize, what: &str) -> Health {
    if lost > 0 {
        Health::Warning(format!("{lost} reference failure(s): some selected {what} no longer exist."))
    } else {
        Health::Healthy
    }
}

fn used_ids(state: &State) -> HashSet<BodyId> {
    state.bodies.iter().map(|b| b.id.clone()).collect()
}

/// Splits the result of modifying `body` into its solids: the first keeps the body's id and
/// `created_by`; further lumps get the next `{id}~k` not in `taken` (so a split never reuses the
/// id of another existing body, e.g. a pattern instance `{id}~1`). Deterministic.
/// Each solid keeps the generative names of its faces.
fn split_keep_id(taken: &mut HashSet<BodyId>, body: &Body, shape: Named) -> Vec<Body> {
    let mut out = Vec::new();
    let mut k = 0;
    for (i, solid) in shape.solids().into_iter().enumerate() {
        let id = if i == 0 {
            body.id.clone()
        } else {
            loop {
                k += 1;
                let c = format!("{}~{k}", body.id);
                if !taken.contains(&c) {
                    break c;
                }
            }
        };
        taken.insert(id.clone());
        out.push(new_body_with(id, body.created_by.clone(), solid.shape, BodyKind::Solid, solid.names));
    }
    out
}

/// Replaces bodies at the given positions (each by zero or more bodies), keeping list order.
fn replace_at(state: &mut State, mut repl: HashMap<usize, Vec<Body>>, remove: &[usize]) {
    let old = std::mem::take(&mut state.bodies);
    for (i, b) in old.into_iter().enumerate() {
        if let Some(r) = repl.remove(&i) {
            state.bodies.extend(r);
        } else if !remove.contains(&i) {
            state.bodies.push(b);
        }
    }
}

// ---------------------------------------------------------------------------------- features

fn shell(state: &mut State, fid: &str, faces: &[FaceRef], thickness: f64) -> Result<Health, String> {
    if thickness <= 0.0 {
        return Err("Thickness must be greater than zero.".into());
    }
    if faces.is_empty() {
        return Err("No faces selected. Select the faces to remove.".into());
    }
    let mut by_body: Vec<(String, Vec<&FaceRef>)> = Vec::new();
    for r in faces {
        match by_body.iter_mut().find(|(b, _)| *b == r.body) {
            Some((_, v)) => v.push(r),
            None => by_body.push((r.body.clone(), vec![r])),
        }
    }
    let mut lost = 0;
    let mut fallback = 0;
    let mut repl: HashMap<usize, Vec<Body>> = HashMap::new();
    let mut taken = used_ids(state);
    for (bid, refs_) in by_body {
        let Some(pos) = state.bodies.iter().position(|b| b.id == bid) else {
            lost += refs_.len();
            continue;
        };
        let body = state.bodies[pos].clone();
        let topo = body.topology();
        let mut idx: Vec<usize> = Vec::new();
        for r in refs_ {
            match refs::resolve_face_ex(&body, r) {
                Ok(res) => {
                    fallback += res.fell_back() as usize;
                    if !idx.contains(&res.index) {
                        idx.push(res.index);
                    }
                }
                Err(_) => lost += 1,
            }
        }
        if idx.is_empty() {
            continue;
        }
        let sel: Vec<&Shape> = idx.iter().map(|&i| &topo.faces[i]).collect();
        let named = Named::of_body(&body).shell(&sel, thickness, fid).map_err(|e| format!("{} (the thickness may be too large for this body).", e.0))?;
        let r = &named.shape;
        let (v0, v1) = (body.volume(), r.volume());
        // OCCT may return a shape instead of failing when the walls collide: validate it (an
        // inward shell can neither grow the body nor fail to remove material)
        let (b0, b1) = (topo.bbox, r.bbox().unwrap_or([0.0; 6]));
        let grew = (0..3).any(|i| b1[i] < b0[i] - 1e-6 || b1[i + 3] > b0[i + 3] + 1e-6);
        let why = if !r.is_valid() {
            Some("the offset geometry is invalid")
        } else if r.subshapes(crate::occ::SOLID).is_empty() || v1 <= 1e-9 {
            Some("no solid remains")
        } else if v1 >= v0 - 1e-9 || grew {
            Some("the offset walls are inverted")
        } else {
            None
        };
        if let Some(why) = why {
            return Err(format!("Shell failed: a {thickness} mm wall is too thick for this body ({why}; the inner walls would intersect)."));
        }
        repl.insert(pos, split_keep_id(&mut taken, &body, named));
    }
    if lost == faces.len() {
        return Err(format!("{lost} reference failure(s): the selected faces no longer exist. Edit this feature and reselect the faces."));
    }
    replace_at(state, repl, &[]);
    Ok(if lost == 0 && fallback > 0 { Health::Warning(refs::fallback_warning(fallback)) } else { partial_loss(lost, "faces") })
}

fn combine(state: &mut State, fid: &str, target: &str, tools: &[BodyId], op: Operation, keep_tools: bool) -> Result<Health, String> {
    let tpos = state
        .bodies
        .iter()
        .position(|b| b.id == target)
        .ok_or("The target body reference is lost: the body no longer exists. Edit this feature and reselect the target body.")?;
    let tools: Vec<BodyId> = tools.iter().filter(|t| *t != target).cloned().collect();
    if tools.is_empty() {
        return Err("No tool bodies selected.".into());
    }
    let (tpos_list, lost) = find_bodies(state, &tools);
    if tpos_list.is_empty() {
        return Err(lost_bodies_error(lost));
    }
    let body = state.bodies[tpos].clone();
    let mut acc = Named::of_body(&body);
    for &p in &tpos_list {
        let tool = Named::of_body(&state.bodies[p]);
        acc = match op {
            Operation::Join | Operation::New => acc.fuse(&tool, fid),
            Operation::Cut => acc.cut(&tool, fid),
            Operation::Intersect => acc.common(&tool, fid),
        }
        .map_err(|e| e.0)?;
    }
    let (v0, v1) = (body.volume(), acc.shape.volume());
    if v1.abs() < 1e-9 {
        return Err(match op {
            Operation::Intersect => "The bodies do not intersect: the result is empty.".into(),
            _ => "The combine produced no volume.".into(),
        });
    }
    let mut health = partial_loss(lost, "tool bodies");
    if op == Operation::Cut && (v1 - v0).abs() <= 1e-9 * v0.abs().max(1.0) {
        health = Health::Warning("The tool bodies do not intersect the target body.".into());
    }
    if !acc.shape.is_valid() {
        health = Health::Warning("The resulting geometry is not valid.".into());
    }
    let repl = HashMap::from([(tpos, split_keep_id(&mut used_ids(state), &body, acc))]);
    let remove: Vec<usize> = if keep_tools { Vec::new() } else { tpos_list };
    replace_at(state, repl, &remove);
    Ok(health)
}

/// Mirror / pattern / copy: transformed instances of the bodies, as new bodies or joined to
/// their original.
fn copies(f: &Feature, state: &mut State, bodies: &[BodyId], xf: &[Xform], join: bool) -> Result<Health, String> {
    if bodies.is_empty() {
        return Err("No bodies selected.".into());
    }
    let (pos, lost) = find_bodies(state, bodies);
    if pos.is_empty() {
        return Err(lost_bodies_error(lost));
    }
    if join {
        let mut repl: HashMap<usize, Vec<Body>> = HashMap::new();
        let mut taken = used_ids(state);
        for &p in &pos {
            let b = state.bodies[p].clone();
            let orig = Named::of_body(&b);
            let mut acc = orig.clone();
            for (k, x) in xf.iter().enumerate() {
                // instance faces get their own names; the original keeps its names
                let inst = apply(&orig, x, |n| format!("{}/i{}:{n}", f.id, k + 1))?;
                acc = acc.fuse(&inst, &f.id).map_err(|e| e.0)?;
            }
            repl.insert(p, split_keep_id(&mut taken, &b, acc));
        }
        replace_at(state, repl, &[]);
    } else {
        let mut ids = IdAlloc { fid: &f.id, next: 0 };
        let mut created = Vec::new();
        for &p in &pos {
            let b = state.bodies[p].clone();
            let orig = Named::of_body(&b);
            for (k, x) in xf.iter().enumerate() {
                // instance names also for new bodies, so combining a copy with its original
                // never yields two faces with the same name
                let s = apply(&orig, x, |n| format!("{}/i{}:{n}", f.id, k + 1))?;
                for solid in s.solids() {
                    created.push(new_body_with(ids.next(), f.id.clone(), solid.shape, BodyKind::Solid, solid.names));
                }
            }
        }
        state.bodies.extend(created);
    }
    Ok(partial_loss(lost, "bodies"))
}

fn move_in_place(state: &mut State, bodies: &[BodyId], m: &Rigid) -> Result<Health, String> {
    if bodies.is_empty() {
        return Err("No bodies selected.".into());
    }
    let (pos, lost) = find_bodies(state, bodies);
    if pos.is_empty() {
        return Err(lost_bodies_error(lost));
    }
    let mut repl: HashMap<usize, Vec<Body>> = HashMap::new();
    let mut taken = used_ids(state);
    for &p in &pos {
        let b = state.bodies[p].clone();
        let s = Named::of_body(&b).transformed(0, m, str::to_string).map_err(|e| e.0)?;
        repl.insert(p, split_keep_id(&mut taken, &b, s));
    }
    replace_at(state, repl, &[]);
    Ok(partial_loss(lost, "bodies"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rigid_algebra() {
        let r = rotation(OriginAxis::Z, 90.0);
        // (1,0,0) -> (0,1,0)
        assert_eq!([r[0], r[4], r[8]], [0.0, 1.0, 0.0]);
        let m = compose(translation([10.0, 0.0, 0.0]), r);
        // (1,0,0) -> (10,1,0)
        let p = [m[0] + m[3], m[4] + m[7], m[8] + m[11]];
        assert_eq!(p, [10.0, 1.0, 0.0]);
        assert_eq!(quantity(3.0), Ok(3));
        assert!(quantity(0.0).is_err() && quantity(2.5).is_err());
    }
}
