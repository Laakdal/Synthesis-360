//! Assembly joints (ASSEMBLE > Joint) and the component placement solve.
//!
//! Child module of `recompute` (it works on the private evaluation `State`).
//!
//! Model (Fusion "Joint", reduced to what CAD360 models; Fusion 2705.1.30 probe, ledger F35-F37):
//! * Every component has a placement (rigid transform, local -> world; identity until a joint
//!   moves it). Evaluation state is kept in world (placed) coordinates, so downstream features
//!   reference the placed geometry and body ids never change. Origin planes used by a feature
//!   of a moved component are that component's placed origin planes ([`place_origin`]), so a
//!   body created after a joint in a moved component appears in the moved frame — placement is a
//!   property of the component, not of the bodies that existed when the joint was evaluated.
//! * A joint origin designates a frame on a body: a planar face gives the face centroid with
//!   z = outward normal; a circular edge gives the circle centre with z = circle axis (oriented
//!   like the adjacent planar face's outward normal when there is one). The x axis follows the
//!   sketch-plane rule of [`Frame::on_plane`], so it is deterministic.
//! * Constraint (F35): both joint frames coincide with their z axes ALIGNED in the same
//!   direction (A top face + B bottom face puts B into A, just below A's top face); `flip`
//!   reverses b's z axis (B sits on top). The angle turns origin 1's frame by +angle relative to
//!   origin 2's about the joint z axis, i.e. the moved component B turns by -angle about A's
//!   joint z (reproduces the probe's translation (10.76, 33.995, 20) and rotation row
//!   (0.866, -0.5, 0) for 30 deg); the offset slides b along a's joint z. Rigid, Revolute and
//!   Slider all apply both values; the type tells which value is the joint's motion.
//! * Graph solve: at each joint's timeline position the accepted joints so far form a forest over
//!   components. Each connected cluster has an anchor that never moves: the root component or a
//!   grounded component if the cluster contains one, otherwise the earliest-created component
//!   (F36: with nothing grounded, Component2 moved in both pick orders). Placements propagate
//!   from the anchor through the joints (breadth first); the bodies, sketches and construction
//!   planes of every component whose placement changed are transformed in place.
//! * Over-constraint (F37): a joint whose two components are already connected (directly or
//!   through other joints), or that would connect two fixed components, is refused with
//!   Fusion's message and does not take part in the solve; the earlier joints stay healthy.
//!   Components not connected by any joint stay where they are.

use super::{Health, State, SketchResult};
use crate::body::{new_body_with, Body};
use crate::frame::{cross, dot, len, normalize, scale, sub, Frame, V3};
use crate::refs;
use cad_core::{ComponentId, CurveKind, Document, Feature, FeatureId, FeatureKind, JointGeometry, SurfaceKind};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Fusion's refusal text for a joint that over-constrains the assembly (probe F37).
pub const OVER_CONSTRAINED: &str = "A joint in system exists for the provided input. System will be over constrained with this input.";

/// Row-major 3x4 rigid transform [R | t].
pub type Rigid = [f64; 12];

pub const IDENTITY: Rigid = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// Component key: `None` = root.
type Key = Option<ComponentId>;

fn snap(v: f64) -> f64 {
    if v.abs() < 1e-14 {
        0.0
    } else if (v - 1.0).abs() < 1e-14 {
        1.0
    } else if (v + 1.0).abs() < 1e-14 {
        -1.0
    } else {
        v
    }
}

/// `a · b` (apply b first).
pub fn compose(a: &Rigid, b: &Rigid) -> Rigid {
    let mut m = [0.0; 12];
    for i in 0..3 {
        for j in 0..3 {
            m[i * 4 + j] = snap((0..3).map(|k| a[i * 4 + k] * b[k * 4 + j]).sum());
        }
        m[i * 4 + 3] = (0..3).map(|k| a[i * 4 + k] * b[k * 4 + 3]).sum::<f64>() + a[i * 4 + 3];
    }
    m
}

pub fn inverse(m: &Rigid) -> Rigid {
    let mut r = [0.0; 12];
    for i in 0..3 {
        for j in 0..3 {
            r[i * 4 + j] = m[j * 4 + i];
        }
    }
    for i in 0..3 {
        r[i * 4 + 3] = -(0..3).map(|k| r[i * 4 + k] * m[k * 4 + 3]).sum::<f64>();
    }
    r
}

pub fn apply_point(m: &Rigid, p: V3) -> V3 {
    [0, 1, 2].map(|i| m[i * 4] * p[0] + m[i * 4 + 1] * p[1] + m[i * 4 + 2] * p[2] + m[i * 4 + 3])
}

fn apply_dir(m: &Rigid, d: V3) -> V3 {
    [0, 1, 2].map(|i| snap(m[i * 4] * d[0] + m[i * 4 + 1] * d[1] + m[i * 4 + 2] * d[2]))
}

fn is_identity(m: &Rigid) -> bool {
    m.iter().zip(IDENTITY.iter()).all(|(a, b)| (a - b).abs() < 1e-12)
}

/// Frame as a transform (frame-local coordinates -> world), axes re-orthonormalised.
fn frame_matrix(f: &Frame) -> Rigid {
    let z = normalize(f.n);
    let x = normalize(sub(f.x, scale(z, dot(f.x, z))));
    let y = cross(z, x);
    let o = f.origin;
    [x[0], y[0], z[0], o[0], x[1], y[1], z[1], o[1], x[2], y[2], z[2], o[2]]
}

/// Frame moved by a rigid transform.
pub fn transform_frame(m: &Rigid, f: &Frame) -> Frame {
    Frame { origin: apply_point(m, f.origin), x: apply_dir(m, f.x), y: apply_dir(m, f.y), n: apply_dir(m, f.n) }
}

// ---------------------------------------------------------------------------- joint frames

/// Joint frame of a planar face (origin = centroid, z = outward normal).
pub fn face_joint_frame(body: &Body, face: usize) -> Result<Frame, String> {
    let t = body.topology();
    let fi = t.face_info.get(face).ok_or("the face no longer exists")?;
    if fi.surface != SurfaceKind::Plane {
        return Err("joint origins on faces must be planar faces".into());
    }
    Ok(Frame { origin: fi.centroid, ..Frame::on_plane(fi.centroid, fi.normal) })
}

/// Joint frame of a circular edge (origin = centre, z = circle axis oriented like the adjacent
/// planar face's outward normal when there is one).
pub fn edge_joint_frame(body: &Body, edge: usize) -> Result<Frame, String> {
    let t = body.topology();
    let e = t.edge_info.get(edge).ok_or("the edge no longer exists")?;
    if e.curve != CurveKind::Circle {
        return Err("joint origins on edges must be circular edges".into());
    }
    let mut axis = cross(sub(e.mid, e.center), e.dir);
    if len(axis) < 1e-9 {
        return Err("the circular edge is degenerate".into());
    }
    axis = normalize(axis);
    for &fi in &e.faces {
        if fi < 0 {
            continue;
        }
        if let Some(f) = t.face_info.get(fi as usize) {
            if f.surface == SurfaceKind::Plane && dot(f.normal, axis).abs() > 0.999 {
                axis = f.normal;
                break;
            }
        }
    }
    let axis = [snap(axis[0]), snap(axis[1]), snap(axis[2])];
    Ok(Frame { origin: e.center, ..Frame::on_plane(e.center, axis) })
}

/// Joint frame of a joint origin's geometry on `body` (resolved through the persistent
/// reference).
pub fn joint_frame(body: &Body, g: &JointGeometry) -> Result<Frame, String> {
    match g {
        JointGeometry::Face { face } => face_joint_frame(body, refs::resolve_face(body, face)?),
        JointGeometry::Edge { edge } => edge_joint_frame(body, refs::resolve_edge(body, edge)?),
    }
}

/// Joint offset J in a's joint frame: b's frame = a's frame · J. Slide `offset` along z, turn by
/// -`angle_deg` about z (origin 1 turns +angle relative to origin 2), `flip` reverses b's z
/// (half turn about x).
fn joint_offset(angle_deg: f64, offset: f64, flip: bool) -> Rigid {
    let (s, c) = (-angle_deg).to_radians().sin_cos();
    let (s, c) = (snap(s), snap(c));
    let rz: Rigid = [c, -s, 0.0, 0.0, s, c, 0.0, 0.0, 0.0, 0.0, 1.0, offset];
    if flip {
        compose(&rz, &[1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0])
    } else {
        rz
    }
}

/// Rigid transform that moves frame `b` onto frame `a` per the joint constraint: origins
/// coincide, z axes aligned (opposed with `flip`), then b turns by -`angle_deg` about and slides
/// by `offset` along a's z axis. This is also b's placement relative to a when both frames are
/// given in their components' local coordinates.
pub fn mate_transform(a: &Frame, b: &Frame, angle_deg: f64, offset: f64, flip: bool) -> Rigid {
    let m = compose(&compose(&frame_matrix(a), &joint_offset(angle_deg, offset, flip)), &inverse(&frame_matrix(b)));
    let mut m = m;
    for i in 0..3 {
        for j in 0..3 {
            m[i * 4 + j] = snap(m[i * 4 + j]);
        }
    }
    m
}

// ---------------------------------------------------------------------------- components

/// Per-recompute view of the document's components: creating feature -> component, creation
/// order and grounded flags.
pub(super) struct ComponentMap {
    owner: HashMap<FeatureId, Key>,
    rank: HashMap<ComponentId, usize>,
    grounded: HashSet<ComponentId>,
}

impl ComponentMap {
    pub(super) fn of_feature(&self, feature: &str) -> Key {
        self.owner.get(feature).cloned().flatten()
    }
    fn fixed(&self, c: &Key) -> bool {
        c.as_ref().is_none_or(|c| self.grounded.contains(c))
    }
    /// Anchor preference among non-fixed components: earliest created first.
    fn rank(&self, c: &Key) -> (usize, String) {
        match c {
            None => (0, String::new()),
            Some(c) => (self.rank.get(c).map_or(usize::MAX, |r| r + 1), c.clone()),
        }
    }
}

pub(super) fn component_map(doc: &Document) -> ComponentMap {
    ComponentMap {
        owner: doc.features.iter().map(|f| (f.id.clone(), f.component.clone())).collect(),
        rank: doc.components.iter().enumerate().map(|(i, c)| (c.id.clone(), i)).collect(),
        grounded: doc.components.iter().filter(|c| c.grounded).map(|c| c.id.clone()).collect(),
    }
}

/// Hash of the component properties the solve depends on that are not part of any feature
/// (creation order, grounded flags); seeds the recompute cache key.
pub(super) fn graph_key(comps: &ComponentMap) -> u64 {
    let mut h = DefaultHasher::new();
    let mut r: Vec<_> = comps.rank.iter().collect();
    r.sort();
    r.hash(&mut h);
    let mut g: Vec<_> = comps.grounded.iter().collect();
    g.sort();
    g.hash(&mut h);
    h.finish()
}

/// A joint accepted into the assembly graph: P_b = P_a · rel.
#[derive(Clone, Debug)]
struct Accepted {
    a: Key,
    b: Key,
    rel: Rigid,
}

/// Assembly part of the evaluation state (cached with it per feature).
#[derive(Clone, Default)]
pub(super) struct Assembly {
    /// Placements of moved components (missing = identity; the root never moves).
    placements: HashMap<ComponentId, Rigid>,
    joints: Vec<Accepted>,
    /// Placement of the component of the feature being evaluated (for its origin planes).
    current: Option<Rigid>,
}

impl Assembly {
    fn placement(&self, c: &Key) -> Rigid {
        c.as_ref().and_then(|c| self.placements.get(c)).copied().unwrap_or(IDENTITY)
    }
}

/// Called before each feature is evaluated: its origin planes become its component's.
pub(super) fn enter(state: &mut State, f: &Feature) {
    let p = state.asm.placement(&f.component);
    state.asm.current = (!is_identity(&p)).then_some(p);
}

/// Origin-geometry frame (origin plane, origin axis) of the component being evaluated.
pub(super) fn place_origin(state: &State, f: Frame) -> Frame {
    match &state.asm.current {
        Some(m) => transform_frame(m, &f),
        None => f,
    }
}

/// Origin point + direction of the component being evaluated.
pub(super) fn place_axis(state: &State, o: V3, d: V3) -> (V3, V3) {
    match &state.asm.current {
        Some(m) => (apply_point(m, o), apply_dir(m, d)),
        None => (o, d),
    }
}

/// Union-find over components with "contains a fixed component" per cluster.
struct Clusters<'a> {
    comps: &'a ComponentMap,
    parent: HashMap<Key, Key>,
}

impl<'a> Clusters<'a> {
    fn new(comps: &'a ComponentMap) -> Self {
        Clusters { comps, parent: HashMap::new() }
    }
    fn find(&self, c: &Key) -> Key {
        let mut c = c.clone();
        while let Some(p) = self.parent.get(&c) {
            if *p == c {
                break;
            }
            c = p.clone();
        }
        c
    }
    fn union(&mut self, a: &Key, b: &Key) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent.insert(rb, ra);
        }
    }
    fn members(&self, c: &Key) -> Vec<Key> {
        let r = self.find(c);
        // the tree root has no parent entry: add it (and `c`, possibly a singleton) explicitly
        let mut out: Vec<Key> = self.parent.keys().filter(|k| self.find(k) == r).cloned().collect();
        for k in [&r, c] {
            if !out.contains(k) {
                out.push(k.clone());
            }
        }
        out
    }
    fn has_fixed(&self, c: &Key) -> bool {
        self.members(c).iter().any(|m| self.comps.fixed(m))
    }
    /// The cluster member that never moves.
    fn anchor(&self, c: &Key) -> Key {
        let m = self.members(c);
        if let Some(f) = m.iter().find(|k| self.comps.fixed(k)) {
            return f.clone();
        }
        m.into_iter().min_by_key(|k| self.comps.rank(k)).unwrap_or(None)
    }
    /// Whether a joint between `a` and `b` would over-constrain the assembly.
    fn over_constrains(&self, a: &Key, b: &Key) -> bool {
        self.find(a) == self.find(b) || (self.has_fixed(a) && self.has_fixed(b))
    }
}

/// Which side of a prospective joint moves (doc-only, for the Joint dialog's status): `Ok(true)`
/// when the component of origin 2 (`b`) moves, `Ok(false)` when origin 1's does, `Err` with the
/// refusal message when the joint over-constrains the assembly. `skip` is a joint to leave out
/// (the one being edited).
pub fn joint_moves_b(doc: &Document, skip: Option<&str>, a: Option<&str>, b: Option<&str>) -> Result<bool, String> {
    let comps = component_map(doc);
    let mut cl = Clusters::new(&comps);
    let body_comp = |o: &cad_core::JointOrigin| -> Key { doc.body_component(o.body(), None).map(String::from) };
    for (i, f) in doc.features.iter().enumerate() {
        if i >= doc.marker || f.suppressed || Some(f.id.as_str()) == skip {
            continue;
        }
        if let FeatureKind::Joint { a, b, .. } = &f.kind {
            let (ca, cb) = (body_comp(a), body_comp(b));
            if ca != cb && !cl.over_constrains(&ca, &cb) {
                cl.union(&ca, &cb);
            }
        }
    }
    let (a, b): (Key, Key) = (a.map(String::from), b.map(String::from));
    if a == b {
        return Err("Both joint origins are on the same component. Select geometry on two different components.".into());
    }
    if cl.over_constrains(&a, &b) {
        return Err(OVER_CONSTRAINED.into());
    }
    Ok(moves_b(&cl, &a, &b))
}

fn moves_b(cl: &Clusters, a: &Key, b: &Key) -> bool {
    if cl.has_fixed(a) {
        return true;
    }
    if cl.has_fixed(b) {
        return false;
    }
    cl.comps.rank(&cl.anchor(a)) <= cl.comps.rank(&cl.anchor(b))
}

/// Placements of all jointed components (anchors keep the identity).
fn solve(joints: &[Accepted], comps: &ComponentMap) -> HashMap<Key, Rigid> {
    let mut cl = Clusters::new(comps);
    for j in joints {
        cl.union(&j.a, &j.b);
    }
    let mut placed: HashMap<Key, Rigid> = HashMap::new();
    let mut nodes: Vec<Key> = Vec::new();
    for j in joints {
        for k in [&j.a, &j.b] {
            if !nodes.contains(k) {
                nodes.push(k.clone());
            }
        }
    }
    for n in &nodes {
        if placed.contains_key(n) {
            continue;
        }
        let anchor = cl.anchor(n);
        placed.insert(anchor.clone(), IDENTITY);
        let mut queue = VecDeque::from([anchor]);
        while let Some(u) = queue.pop_front() {
            let pu = placed[&u];
            for j in joints {
                let (v, p) = if j.a == u && !placed.contains_key(&j.b) {
                    (j.b.clone(), compose(&pu, &j.rel))
                } else if j.b == u && !placed.contains_key(&j.a) {
                    (j.a.clone(), compose(&pu, &inverse(&j.rel)))
                } else {
                    continue;
                };
                placed.insert(v.clone(), p);
                queue.push_back(v);
            }
        }
    }
    placed
}

/// Moves component `c`'s bodies, sketches and construction planes by `delta`.
fn move_component(state: &mut State, comps: &ComponentMap, c: &Key, delta: &Rigid) -> Result<(), String> {
    let mut bodies = Vec::with_capacity(state.bodies.len());
    for body in &state.bodies {
        if comps.of_feature(&body.created_by) == *c {
            let s = body.shape.transformed(delta).map_err(|e| e.0)?;
            // same id, creating feature, kind and generative face names (the transform keeps the
            // face order)
            bodies.push(new_body_with(body.id.clone(), body.created_by.clone(), s, body.kind, body.face_names.clone()));
        } else {
            bodies.push(body.clone());
        }
    }
    state.bodies = bodies;
    for (id, r) in state.sketches.iter_mut() {
        if comps.of_feature(id) == *c {
            *r = Arc::new(SketchResult { frame: transform_frame(delta, &r.frame), ..(**r).clone() });
        }
    }
    for (id, fr) in state.planes.iter_mut() {
        if comps.of_feature(id) == *c {
            *fr = transform_frame(delta, fr);
        }
    }
    Ok(())
}

pub(super) fn evaluate(f: &Feature, state: &mut State, vals: &dyn Fn(&str) -> f64, comps: &ComponentMap) -> Result<Health, String> {
    let FeatureKind::Joint { a, b, angle, offset, flip, .. } = &f.kind else { return Ok(Health::Healthy) };
    let find = |id: &str, which: usize| -> Result<Body, String> {
        state
            .bodies
            .iter()
            .find(|x| x.id == id)
            .cloned()
            .ok_or_else(|| format!("Joint origin {which}: the reference is lost (the body no longer exists). Edit this joint and select new geometry."))
    };
    let ba = find(a.body(), 1)?;
    let bb = find(b.body(), 2)?;
    let (ca, cb) = (comps.of_feature(&ba.created_by), comps.of_feature(&bb.created_by));
    if ca == cb {
        return Err("Both joint origins are on the same component. Select geometry on two different components.".into());
    }
    let fa = joint_frame(&ba, &a.geometry).map_err(|e| format!("Joint origin 1: the reference is lost ({e}). Edit this joint and select new geometry."))?;
    let fb = joint_frame(&bb, &b.geometry).map_err(|e| format!("Joint origin 2: the reference is lost ({e}). Edit this joint and select new geometry."))?;
    let mut cl = Clusters::new(comps);
    for j in &state.asm.joints {
        cl.union(&j.a, &j.b);
    }
    if cl.over_constrains(&ca, &cb) {
        return Err(OVER_CONSTRAINED.into());
    }
    // frames in component-local coordinates -> b's placement relative to a's
    let fa = transform_frame(&inverse(&state.asm.placement(&ca)), &fa);
    let fb = transform_frame(&inverse(&state.asm.placement(&cb)), &fb);
    let rel = mate_transform(&fa, &fb, vals(angle), vals(offset), *flip);
    state.asm.joints.push(Accepted { a: ca, b: cb, rel });

    let placed = solve(&state.asm.joints, comps);
    let mut moves: Vec<(Key, Rigid)> = placed.into_iter().collect();
    moves.sort_by_key(|(k, _)| comps.rank(k));
    for (c, p) in moves {
        let Some(id) = c.clone() else { continue };
        let old = state.asm.placement(&c);
        let delta = compose(&p, &inverse(&old));
        if !is_identity(&delta) {
            move_component(state, comps, &c, &delta)?;
        }
        if is_identity(&p) {
            state.asm.placements.remove(&id);
        } else {
            state.asm.placements.insert(id, p);
        }
    }
    Ok(Health::Healthy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mate_aligns_z_axes_like_fusion() {
        // F35: A top face (+Z at (20,20,20)), B bottom face (-Z at (115,7.5,0))
        let a = Frame { origin: [20.0, 20.0, 20.0], ..Frame::on_plane([20.0, 20.0, 20.0], [0.0, 0.0, 1.0]) };
        let b = Frame { origin: [115.0, 7.5, 0.0], ..Frame::on_plane([115.0, 7.5, 0.0], [0.0, 0.0, -1.0]) };
        let m = mate_transform(&a, &b, 0.0, 0.0, false);
        assert_eq!(apply_point(&m, b.origin), a.origin);
        // half turn about X: B's -Z axis lands on +Z (B goes into A)
        assert_eq!([m[0], m[5], m[10]], [1.0, -1.0, -1.0]);
        // flip: pure translation, B sits on A
        let m = mate_transform(&a, &b, 0.0, 0.0, true);
        assert_eq!([m[0], m[5], m[10]], [1.0, 1.0, 1.0]);
        assert_eq!(apply_point(&m, b.origin), a.origin);
        let m = mate_transform(&a, &b, 0.0, 15.0, false);
        assert_eq!(apply_point(&m, b.origin), [20.0, 20.0, 35.0]);
        // 30 deg: the probe's rotation row and component-origin translation
        let m = mate_transform(&a, &b, 30.0, 0.0, false);
        assert!((m[0] - 0.866).abs() < 1e-3 && (m[1] + 0.5).abs() < 1e-3 && m[2].abs() < 1e-12, "{m:?}");
        let p = apply_point(&m, [100.0, 0.0, 0.0]);
        assert!(len(sub(p, [10.76, 33.995, 20.0])) < 1e-3, "{p:?}");
        // proper rotation
        let det = m[0] * (m[5] * m[10] - m[6] * m[9]) - m[1] * (m[4] * m[10] - m[6] * m[8]) + m[2] * (m[4] * m[9] - m[5] * m[8]);
        assert!((det - 1.0).abs() < 1e-12);
        // inverse / compose round trip
        let r = compose(&m, &inverse(&m));
        assert!(is_identity(&r), "{r:?}");
    }
}
