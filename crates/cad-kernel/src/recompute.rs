//! Parametric recompute: evaluates the feature timeline into solid bodies.
//!
//! Features are evaluated in timeline order up to the marker. Each produces a health state like
//! Fusion's timeline (healthy / warning / error / suppressed / rolled back); a failing feature
//! leaves the model as it was before it and evaluation continues downstream, so dependent
//! features report their own problems. Results are cached per feature with a key chained over
//! all upstream state, so editing a late feature does not recompute early ones, and editing an
//! early one recomputes everything after it.

use crate::body::{new_body, Body};
use crate::frame::{add, cross, dist, dot, normalize, scale, sub, Frame, V3};
use crate::occ::{KResult, Seg3, Shape, SOLID};
use crate::refs;
use cad_core::{
    sketch::{self, EntityKind, Profile, Segment, SolveReport, SolveStatus},
    Document, ExtentDirection, Feature, FeatureId, FeatureKind, Operation, OriginAxis, PlaneRef, ProfileRef, RevolveAxis,
};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum Health {
    Healthy,
    Warning(String),
    Error(String),
    Suppressed,
    RolledBack,
}

impl Health {
    pub fn is_error(&self) -> bool {
        matches!(self, Health::Error(_))
    }
    pub fn message(&self) -> Option<&str> {
        match self {
            Health::Warning(m) | Health::Error(m) => Some(m),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SketchResult {
    pub frame: Frame,
    pub solved: sketch::Sketch,
    pub report: SolveReport,
    pub profiles: Vec<Profile>,
}

#[derive(Clone, Default)]
struct State {
    bodies: Vec<Body>,
    planes: HashMap<FeatureId, Frame>,
    sketches: HashMap<FeatureId, Arc<SketchResult>>,
}

#[derive(Clone)]
struct CacheEntry {
    key: u64,
    state: State,
    health: Health,
}

/// Output of a recompute.
#[derive(Clone, Default)]
pub struct Model {
    pub bodies: Vec<Body>,
    pub health: HashMap<FeatureId, Health>,
    pub sketches: HashMap<FeatureId, Arc<SketchResult>>,
    pub planes: HashMap<FeatureId, Frame>,
    pub parameter_errors: HashMap<String, String>,
    /// Index of the first feature that was actually recomputed (not served from cache).
    pub recomputed_from: Option<usize>,
}

impl Model {
    pub fn body(&self, id: &str) -> Option<&Body> {
        self.bodies.iter().find(|b| b.id == id)
    }
    pub fn health_of(&self, id: &str) -> Health {
        self.health.get(id).cloned().unwrap_or(Health::Healthy)
    }
}

#[derive(Default)]
pub struct Kernel {
    cache: Vec<CacheEntry>,
}

fn hash_str(h: &mut DefaultHasher, s: &str) {
    s.hash(h);
}

/// Solved-geometry fingerprint of a sketch (positions quantised to 1e-7 mm).
fn sketch_fingerprint(h: &mut DefaultHasher, s: &sketch::Sketch) {
    for e in &s.entities {
        e.id.hash(h);
        e.construction.hash(h);
        match &e.kind {
            EntityKind::Point { x, y } => ((x * 1e7).round() as i64, (y * 1e7).round() as i64).hash(h),
            EntityKind::Line { p1, p2 } => (p1, p2).hash(h),
            EntityKind::Circle { c, r } => (c, (r * 1e7).round() as i64).hash(h),
            EntityKind::Arc { c, start, end, r } => (c, start, end, (r * 1e7).round() as i64).hash(h),
        }
    }
}

impl Kernel {
    pub fn new() -> Self {
        crate::occ::quiet();
        Kernel::default()
    }

    /// Drops all cached results (e.g. after loading another document).
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn recompute(&mut self, doc: &Document) -> Model {
        let pv = doc.parameter_values();
        let mut model = Model { parameter_errors: pv.errors.clone(), ..Default::default() };
        let mut state = State::default();
        let mut prev_key: u64 = 0;
        let mut cache_valid = true;

        for (i, f) in doc.features.iter().enumerate() {
            let mut h = DefaultHasher::new();
            prev_key.hash(&mut h);
            let active = i < doc.marker && !f.suppressed;
            (i < doc.marker, f.suppressed).hash(&mut h);
            if let FeatureKind::Sketch { plane, .. } = &f.kind {
                hash_str(&mut h, &serde_json::to_string(plane).unwrap_or_default());
                hash_str(&mut h, &f.id);
            } else {
                hash_str(&mut h, &serde_json::to_string(f).unwrap_or_default());
            }
            for p in f.parameter_names() {
                p.hash(&mut h);
                pv.values.get(p).map(|v| v.to_bits()).hash(&mut h);
                pv.errors.get(p).hash(&mut h);
            }

            // Sketches are always re-solved (cheap); their cache key is the solved geometry, so a
            // re-solve that moves nothing does not invalidate downstream features.
            let mut sketch_result: Option<(Health, Option<Arc<SketchResult>>)> = None;
            if active {
                if let FeatureKind::Sketch { .. } = &f.kind {
                    let (health, res) = self.solve_sketch(f, &state, &pv.values, &pv.errors);
                    if let Some(r) = &res {
                        sketch_fingerprint(&mut h, &r.solved);
                        (r.report.status == SolveStatus::Solved).hash(&mut h);
                    }
                    format!("{health:?}").hash(&mut h);
                    sketch_result = Some((health, res));
                }
            }
            let key = h.finish();

            if cache_valid && self.cache.get(i).is_some_and(|c| c.key == key) {
                let c = &self.cache[i];
                state = c.state.clone();
                model.health.insert(f.id.clone(), c.health.clone());
                prev_key = key;
                continue;
            }
            if cache_valid {
                cache_valid = false;
                model.recomputed_from = Some(i);
                self.cache.truncate(i);
            }

            let health = if i >= doc.marker {
                Health::RolledBack
            } else if f.suppressed {
                Health::Suppressed
            } else if let Some((health, res)) = sketch_result {
                if let Some(r) = res {
                    state.sketches.insert(f.id.clone(), r);
                }
                health
            } else if let Some(bad) = f.parameter_names().iter().find(|p| pv.errors.contains_key(**p) || !pv.values.contains_key(**p)) {
                Health::Error(format!("Parameter {bad}: {}", pv.errors.get(*bad).map(String::as_str).unwrap_or("missing")))
            } else {
                let vals = |name: &str| pv.values.get(name).copied().unwrap_or(0.0);
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| evaluate(f, &mut state, &vals))) {
                    Ok(h) => h,
                    Err(_) => Health::Error("Internal error while computing this feature".into()),
                }
            };
            model.health.insert(f.id.clone(), health.clone());
            self.cache.push(CacheEntry { key, state: state.clone(), health });
            prev_key = key;
        }
        self.cache.truncate(doc.features.len());
        model.bodies = state.bodies;
        model.sketches = state.sketches;
        model.planes = state.planes;
        model
    }

    fn solve_sketch(&self, f: &Feature, state: &State, values: &HashMap<String, f64>, errors: &HashMap<String, String>) -> (Health, Option<Arc<SketchResult>>) {
        let FeatureKind::Sketch { plane, sketch: sk, .. } = &f.kind else { return (Health::Healthy, None) };
        let frame = match resolve_plane(plane, state) {
            Ok(fr) => fr,
            Err(e) => return (Health::Error(format!("Sketch plane reference lost: {e}")), None),
        };
        let mut dims = sketch::DimensionValues::new();
        for c in &sk.constraints {
            if let Some(p) = &c.param {
                if c.driven {
                    continue;
                }
                match values.get(p) {
                    Some(v) => {
                        dims.insert(c.id.clone(), *v);
                    }
                    None => return (Health::Error(format!("Dimension {p}: {}", errors.get(p).map(String::as_str).unwrap_or("missing parameter"))), None),
                }
            }
        }
        let mut solved = sk.clone();
        let report = sketch::solve(&mut solved, &dims, &[]);
        let health = match &report.status {
            SolveStatus::Solved => Health::Healthy,
            SolveStatus::Failed { unsatisfied } => Health::Error(format!(
                "Sketch could not be solved: {} constraint(s) cannot be satisfied with the current dimensions",
                unsatisfied.len()
            )),
        };
        let profiles = sketch::find_profiles(&solved);
        (health, Some(Arc::new(SketchResult { frame, solved, report, profiles })))
    }
}

fn resolve_plane(p: &PlaneRef, state: &State) -> Result<Frame, String> {
    match p {
        PlaneRef::Origin { plane } => Ok(Frame::origin_plane(*plane)),
        PlaneRef::Construction { feature } => state.planes.get(feature).copied().ok_or_else(|| "the construction plane is not available".to_string()),
        PlaneRef::Face { face } => {
            let body = state.bodies.iter().find(|b| b.id == face.body).ok_or_else(|| format!("body {} no longer exists", face.body))?;
            let i = refs::resolve_face(body, face)?;
            let fi = &body.topology().face_info[i];
            if fi.surface != cad_core::SurfaceKind::Plane {
                return Err("the face is no longer planar".into());
            }
            Ok(Frame::on_plane(fi.centroid, fi.normal))
        }
    }
}

fn evaluate(f: &Feature, state: &mut State, vals: &dyn Fn(&str) -> f64) -> Health {
    let r: Result<Health, String> = match &f.kind {
        FeatureKind::Sketch { .. } => Ok(Health::Healthy),
        FeatureKind::Extrude { sketch, profiles, distance, direction, through_all, operation } => {
            extrude(f, state, sketch, profiles, vals(distance), *direction, *through_all, *operation)
        }
        FeatureKind::Revolve { sketch, profiles, axis, angle, operation } => revolve(f, state, sketch, profiles, axis, vals(angle), *operation),
        FeatureKind::Fillet { edges, radius } => edge_feature(state, edges, vals(radius), true),
        FeatureKind::Chamfer { edges, distance } => edge_feature(state, edges, vals(distance), false),
        FeatureKind::Hole { face, center, diameter, depth, through_all } => hole(state, face, *center, vals(diameter), vals(depth), *through_all),
        FeatureKind::Plane { base, offset, .. } => resolve_plane(base, state).map(|fr| {
            state.planes.insert(f.id.clone(), fr.offset(vals(offset)));
            Health::Healthy
        }),
        FeatureKind::Import { step, .. } => import(f, state, step),
    };
    r.unwrap_or_else(Health::Error)
}

fn sketch_of<'a>(state: &'a State, id: &str) -> Result<&'a Arc<SketchResult>, String> {
    state.sketches.get(id).ok_or_else(|| "The sketch this feature uses is missing, suppressed or rolled back".to_string())
}

/// Profile reference resolution: same bounding curves first, then the sample point.
fn resolve_profiles<'a>(sr: &'a SketchResult, refs: &[ProfileRef]) -> Result<Vec<&'a Profile>, String> {
    if refs.is_empty() {
        return Err("No profile selected".into());
    }
    let mut out = Vec::new();
    for (k, r) in refs.iter().enumerate() {
        let p = sr.profiles.iter().find(|p| p.curves == r.curves).or_else(|| sr.profiles.iter().find(|p| p.contains(r.sample)));
        match p {
            Some(p) => out.push(p),
            None => return Err(format!("Profile {} missing: the profile reference is lost. Edit this feature and select new profile references.", k + 1)),
        }
    }
    Ok(out)
}

fn seg3(frame: &Frame, s: &Segment) -> Seg3 {
    match s {
        Segment::Line { a, b } => Seg3::Line(frame.to_world(*a), frame.to_world(*b)),
        Segment::Arc { .. } => Seg3::Arc(frame.to_world(s.start()), frame.to_world(s.mid()), frame.to_world(s.end())),
    }
}

fn profile_face(frame: &Frame, p: &Profile) -> KResult<Shape> {
    let mut loops = vec![p.outer.segments.iter().map(|(s, _)| seg3(frame, s)).collect::<Vec<_>>()];
    for h in &p.holes {
        loops.push(h.segments.iter().map(|(s, _)| seg3(frame, s)).collect());
    }
    Shape::face_from_loops(&loops)
}

fn union_bbox(bodies: &[Body]) -> Option<[f64; 6]> {
    let mut acc: Option<[f64; 6]> = None;
    for b in bodies {
        let bb = b.topology().bbox;
        acc = Some(match acc {
            None => bb,
            Some(a) => [a[0].min(bb[0]), a[1].min(bb[1]), a[2].min(bb[2]), a[3].max(bb[3]), a[4].max(bb[4]), a[5].max(bb[5])],
        });
    }
    acc
}

fn fuse_all(mut shapes: Vec<Shape>) -> KResult<Shape> {
    let mut acc = shapes.remove(0);
    for s in shapes {
        acc = acc.fuse(&s)?;
    }
    Ok(acc)
}

#[allow(clippy::too_many_arguments)]
fn extrude(f: &Feature, state: &mut State, sketch: &str, prefs: &[ProfileRef], d: f64, dir: ExtentDirection, through_all: bool, op: Operation) -> Result<Health, String> {
    let sr = sketch_of(state, sketch)?.clone();
    let profiles = resolve_profiles(&sr, prefs)?;
    let (start, length) = if through_all {
        let bb = union_bbox(&state.bodies).unwrap_or([-1.0, -1.0, -1.0, 1.0, 1.0, 1.0]);
        let reach = dist([bb[0], bb[1], bb[2]], [bb[3], bb[4], bb[5]]) + dist(sr.frame.origin, [(bb[0] + bb[3]) / 2.0, (bb[1] + bb[4]) / 2.0, (bb[2] + bb[5]) / 2.0]) + 10.0;
        match dir {
            ExtentDirection::Normal => (0.0, reach),
            ExtentDirection::Reverse => (0.0, -reach),
            ExtentDirection::Symmetric => (-reach, 2.0 * reach),
        }
    } else {
        if d.abs() < 1e-9 {
            return Err("Distance must not be zero.".into());
        }
        match dir {
            ExtentDirection::Normal => (0.0, d),
            ExtentDirection::Reverse => (0.0, -d),
            ExtentDirection::Symmetric => (-d.abs() / 2.0, d.abs()),
        }
    };
    let frame = sr.frame.offset(start);
    let mut tools = Vec::new();
    for p in profiles {
        let face = profile_face(&frame, p).map_err(|e| e.0)?;
        tools.push(face.prism(scale(frame.n, length)).map_err(|e| e.0)?);
    }
    let tool = fuse_all(tools).map_err(|e| e.0)?;
    apply_operation(state, &f.id, tool, op)
}

fn revolve(f: &Feature, state: &mut State, sketch: &str, prefs: &[ProfileRef], axis: &RevolveAxis, angle_deg: f64, op: Operation) -> Result<Health, String> {
    let sr = sketch_of(state, sketch)?.clone();
    let profiles = resolve_profiles(&sr, prefs)?;
    if angle_deg.abs() < 1e-9 {
        return Err("Angle must not be zero.".into());
    }
    let (o, a): (V3, V3) = match axis {
        RevolveAxis::SketchLine { entity } => {
            let (p, q) = sr.solved.line_points(entity).ok_or("The revolve axis line no longer exists in the sketch")?;
            let (p3, q3) = (sr.frame.to_world(p), sr.frame.to_world(q));
            (p3, normalize(sub(q3, p3)))
        }
        RevolveAxis::Origin { axis } => (
            [0.0; 3],
            match axis {
                OriginAxis::X => [1.0, 0.0, 0.0],
                OriginAxis::Y => [0.0, 1.0, 0.0],
                OriginAxis::Z => [0.0, 0.0, 1.0],
            },
        ),
    };
    if dot(a, sr.frame.n).abs() > 1e-6 || dot(sub(o, sr.frame.origin), sr.frame.n).abs() > 1e-6 {
        return Err("The revolve axis must lie in the sketch plane.".into());
    }
    // the profile must lie on one side of the axis
    let a2 = sr.frame.to_local(o);
    let b2 = sr.frame.to_local(add(o, a));
    let side = |p: [f64; 2]| (b2[0] - a2[0]) * (p[1] - a2[1]) - (b2[1] - a2[1]) * (p[0] - a2[0]);
    for p in &profiles {
        let pts = p.outer.polygon();
        let (mn, mx) = pts.iter().map(|q| side(*q)).fold((f64::MAX, f64::MIN), |(lo, hi), s| (lo.min(s), hi.max(s)));
        if mn < -1e-6 && mx > 1e-6 {
            return Err("The profile crosses the revolve axis.".into());
        }
    }
    let mut tools = Vec::new();
    for p in profiles {
        let face = profile_face(&sr.frame, p).map_err(|e| e.0)?;
        tools.push(face.revolve(o, a, angle_deg.to_radians().clamp(-std::f64::consts::TAU, std::f64::consts::TAU)).map_err(|e| e.0)?);
    }
    let tool = fuse_all(tools).map_err(|e| e.0)?;
    apply_operation(state, &f.id, tool, op)
}

fn bbox_overlap(a: [f64; 6], b: [f64; 6]) -> bool {
    let t = 1e-6;
    (0..3).all(|k| a[k] <= b[k + 3] + t && b[k] <= a[k + 3] + t)
}

/// Splits a result into its solids; the first keeps `id`, the rest get `id~k`.
fn bodies_from(id: &str, created_by: &str, shape: Shape) -> Vec<Body> {
    let solids = shape.subshapes(SOLID);
    match solids.len() {
        0 => Vec::new(),
        1 => vec![new_body(id.into(), created_by.into(), solids.into_iter().next().unwrap())],
        _ => solids.into_iter().enumerate().map(|(k, s)| new_body(if k == 0 { id.into() } else { format!("{id}~{k}") }, created_by.into(), s)).collect(),
    }
}

fn apply_operation(state: &mut State, fid: &str, tool: Shape, op: Operation) -> Result<Health, String> {
    if tool.volume().abs() < 1e-9 {
        return Err("The operation produced no volume.".into());
    }
    let tool_bb = tool.bbox().unwrap_or([0.0; 6]);
    let targets: Vec<usize> = state.bodies.iter().enumerate().filter(|(_, b)| bbox_overlap(b.topology().bbox, tool_bb)).map(|(i, _)| i).collect();
    let mut warn = None;
    match op {
        Operation::New => {
            state.bodies.extend(bodies_from(fid, fid, tool));
        }
        Operation::Join => {
            if targets.is_empty() {
                state.bodies.extend(bodies_from(fid, fid, tool));
                warn = Some("No target body to join; a new body was created.".to_string());
            } else {
                let first = state.bodies[targets[0]].clone();
                let mut acc = first.shape.fuse(&tool).map_err(|e| e.0)?;
                for &t in &targets[1..] {
                    acc = acc.fuse(&state.bodies[t].shape).map_err(|e| e.0)?;
                }
                if !acc.is_valid() {
                    warn = Some("The joined geometry is not valid.".into());
                }
                let replacement = bodies_from(&first.id, &first.created_by, acc);
                let mut next: Vec<Body> = Vec::new();
                for (i, b) in state.bodies.iter().enumerate() {
                    if i == targets[0] {
                        next.extend(replacement.iter().cloned());
                    } else if !targets.contains(&i) {
                        next.push(b.clone());
                    }
                }
                state.bodies = next;
            }
        }
        Operation::Cut | Operation::Intersect => {
            let mut changed = 0;
            let mut next: Vec<Body> = Vec::new();
            for (i, b) in state.bodies.iter().enumerate() {
                if !targets.contains(&i) {
                    next.push(b.clone());
                    continue;
                }
                let r = if op == Operation::Cut { b.shape.cut(&tool) } else { b.shape.common(&tool) }.map_err(|e| e.0)?;
                let v0 = b.volume();
                let v1 = r.volume();
                if (v1 - v0).abs() <= 1e-9 * v0.abs().max(1.0) {
                    next.push(b.clone());
                    continue;
                }
                changed += 1;
                if !r.is_valid() {
                    warn = Some("The resulting geometry is not valid.".into());
                }
                if v1.abs() > 1e-9 {
                    next.extend(bodies_from(&b.id, &b.created_by, r));
                }
            }
            state.bodies = next;
            if changed == 0 {
                // observed Fusion text when a cut's tool misses every body
                return Ok(Health::Warning("No target body! Compute Failed".into()));
            }
        }
    }
    Ok(warn.map(Health::Warning).unwrap_or(Health::Healthy))
}

fn edge_feature(state: &mut State, edges: &[cad_core::EdgeRef], value: f64, fillet: bool) -> Result<Health, String> {
    let what = if fillet { "Radius" } else { "Distance" };
    if value <= 0.0 {
        return Err(format!("{what} must be greater than zero."));
    }
    if edges.is_empty() {
        return Err("No edges selected.".into());
    }
    let mut by_body: Vec<(String, Vec<&cad_core::EdgeRef>)> = Vec::new();
    for e in edges {
        match by_body.iter_mut().find(|(b, _)| *b == e.body) {
            Some((_, v)) => v.push(e),
            None => by_body.push((e.body.clone(), vec![e])),
        }
    }
    let mut lost = 0;
    for (bid, refs_) in by_body {
        let Some(pos) = state.bodies.iter().position(|b| b.id == bid) else {
            lost += refs_.len();
            continue;
        };
        let body = state.bodies[pos].clone();
        let topo = body.topology();
        let mut sel: Vec<&Shape> = Vec::new();
        for r in refs_ {
            match refs::resolve_edge(&body, r) {
                Ok(i) => {
                    if !sel.iter().any(|s| std::ptr::eq(*s, &topo.edges[i])) {
                        sel.push(&topo.edges[i]);
                    }
                }
                Err(_) => lost += 1,
            }
        }
        if sel.is_empty() {
            continue;
        }
        let r = if fillet { body.shape.fillet(&sel, value) } else { body.shape.chamfer(&sel, value) }.map_err(|e| e.0)?;
        if !r.is_valid() {
            return Err(format!("{} produced invalid geometry with this {}.", if fillet { "Fillet" } else { "Chamfer" }, what.to_lowercase()));
        }
        let replacement = bodies_from(&body.id, &body.created_by, r);
        state.bodies.splice(pos..pos + 1, replacement);
    }
    if lost == edges.len() {
        return Err(format!("{lost} reference failure(s): the selected edges no longer exist. Edit this feature and reselect the edges."));
    }
    Ok(if lost > 0 { Health::Warning(format!("{lost} reference failure(s): some selected edges no longer exist.")) } else { Health::Healthy })
}

fn hole(state: &mut State, face: &cad_core::FaceRef, center: [f64; 2], diameter: f64, depth: f64, through_all: bool) -> Result<Health, String> {
    if diameter <= 0.0 {
        return Err("Diameter must be greater than zero.".into());
    }
    if !through_all && depth <= 0.0 {
        return Err("Depth must be greater than zero.".into());
    }
    let pos = state.bodies.iter().position(|b| b.id == face.body).ok_or("The face reference is lost: the body no longer exists.")?;
    let body = state.bodies[pos].clone();
    let fi = refs::resolve_face(&body, face).map_err(|e| format!("The face reference is lost: {e}"))?;
    let info = body.topology().face_info[fi].clone();
    if info.surface != cad_core::SurfaceKind::Plane {
        return Err("Holes must be placed on a planar face.".into());
    }
    let frame = Frame::on_plane(info.centroid, info.normal);
    let c = frame.to_world(center);
    let bb = body.topology().bbox;
    let h = if through_all { dist([bb[0], bb[1], bb[2]], [bb[3], bb[4], bb[5]]) + 2.0 } else { depth };
    let tool = Shape::cylinder(add(c, scale(frame.n, 0.01)), scale(frame.n, -1.0), diameter / 2.0, h + 0.01).map_err(|e| e.0)?;
    let r = body.shape.cut(&tool).map_err(|e| e.0)?;
    if (r.volume() - body.volume()).abs() < 1e-9 {
        return Ok(Health::Warning("The hole does not intersect the body.".into()));
    }
    state.bodies.splice(pos..pos + 1, bodies_from(&body.id, &body.created_by, r));
    Ok(Health::Healthy)
}

fn import(f: &Feature, state: &mut State, step: &str) -> Result<Health, String> {
    let mut h = DefaultHasher::new();
    step.hash(&mut h);
    let path = std::env::temp_dir().join(format!("cad360-import-{:016x}.step", h.finish()));
    std::fs::write(&path, step).map_err(|e| format!("Could not stage the imported STEP data: {e}"))?;
    let shape = Shape::read_step(&path.to_string_lossy()).map_err(|e| e.0)?;
    let _ = std::fs::remove_file(&path);
    let bodies = bodies_from(&f.id, &f.id, shape);
    if bodies.is_empty() {
        return Err("The STEP data contains no solid bodies.".into());
    }
    state.bodies.extend(bodies);
    Ok(Health::Healthy)
}

/// Copies solved sketch geometry back into the document (solved positions are the next initial
/// guess, as in Fusion where sketch geometry is stored in its solved state).
pub fn store_solved_sketches(doc: &mut Document, model: &Model) {
    for f in doc.features.iter_mut() {
        if let (FeatureKind::Sketch { sketch, .. }, Some(r)) = (&mut f.kind, model.sketches.get(&f.id)) {
            if r.report.status == SolveStatus::Solved && r.solved.entities.len() == sketch.entities.len() {
                *sketch = r.solved.clone();
            }
        }
    }
}

/// Normal of a face of a body (for UI).
pub fn face_normal(body: &Body, face: usize) -> Option<V3> {
    body.topology().face_info.get(face).map(|f| f.normal)
}

/// Perpendicular helper (for UI construction of default sketch x axes).
pub fn any_perpendicular(n: V3) -> V3 {
    let c = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    normalize(cross(n, c))
}
