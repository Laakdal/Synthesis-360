//! cad-geom (pure Rust) runner. Primitives via `cad_geom::primitives`, mass properties via
//! `cad_geom::mass::solid_properties`, validity via `cad_geom::validate`. Every call runs under
//! `catch_unwind`, so a body that is still `unimplemented!()` yields a `NOT_IMPLEMENTED` row (or a
//! note for the missing part) instead of aborting the run. Booleans run through
//! `cad_geom::boolean` (ASM tolerance `BOOLEAN_TOLERANCE`) when every operand is a box, a
//! cylinder or a sphere, folding the tools into the target in catalogue order; other operands
//! (cone, torus) stay `NOT_IMPLEMENTED`. Boolean results are validated at 1e-7 mm first and, if
//! that fails, at the boolean tolerance, with a note saying so.

use crate::cases::{BoolOp, Case, Spec};
use crate::result::{KERNEL_RUST, KernelResult, NOT_IMPLEMENTED, normalize_curve_type};
use cad_geom::topology::Brep;
use cad_geom::{Curve, Frame, GResult, P3, SolidId, Tolerance, V3};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

pub const CAD_GEOM_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Outcome of a guarded call into cad-geom.
enum Guarded<T> {
    Done(T),
    /// The callee panicked; `true` when the panic was `unimplemented!()` / `todo!()`.
    Panicked(String, bool),
}

impl<T> Guarded<T> {
    fn into_option(self) -> Option<T> {
        match self {
            Guarded::Done(v) => Some(v),
            Guarded::Panicked(..) => None,
        }
    }
}

fn guarded<T>(f: impl FnOnce() -> T) -> Guarded<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => Guarded::Done(v),
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".into());
            // `unimplemented!()` panics with "not implemented", `todo!()` with "not yet implemented"
            let unimpl = msg.contains("not implemented") || msg.contains("not yet implemented");
            Guarded::Panicked(msg, unimpl)
        }
    }
}

fn p3(a: [f64; 3]) -> P3 {
    P3::new(a[0], a[1], a[2])
}
fn v3(a: [f64; 3]) -> V3 {
    V3::new(a[0], a[1], a[2])
}

fn build_primitive(spec: &Spec) -> GResult<Brep> {
    use cad_geom::primitives::*;
    match *spec {
        Spec::Box { corner, size } => make_box(p3(corner), size),
        Spec::Cylinder {
            base, axis, r0, h, ..
        } => make_cylinder(Frame::from_origin_z(p3(base), v3(axis)), r0, h),
        Spec::Cone {
            base,
            axis,
            r0,
            r1,
            h,
        } => make_cone(Frame::from_origin_z(p3(base), v3(axis)), r0, r1, h),
        Spec::Sphere { center, r } => make_sphere(p3(center), r),
        Spec::Torus {
            center,
            axis,
            major,
            minor,
        } => make_torus(Frame::from_origin_z(p3(center), v3(axis)), major, minor),
        Spec::Boolean { .. } => unreachable!("booleans are filtered out before"),
    }
}

fn edge_type(c: &Option<Curve>) -> String {
    match c {
        None => "degenerate".into(),
        Some(Curve::Line { .. }) => "line".into(),
        Some(Curve::Circle { .. }) => "circle".into(),
        Some(Curve::Ellipse { .. }) => "ellipse".into(),
        Some(Curve::Nurbs(_)) => normalize_curve_type("bspline"),
    }
}

/// Measures a cad-geom B-rep (topology from the arenas, mass properties and validity guarded).
pub fn measure(case_id: &str, b: &Brep) -> KernelResult {
    measure_with(case_id, b, None)
}

/// [`measure`], validating at `fallback` when validation at the size-scaled default tolerance
/// fails (booleans: the tolerance they were computed at).
pub fn measure_with(case_id: &str, b: &Brep, fallback: Option<Tolerance>) -> KernelResult {
    let mut r = KernelResult::new(KERNEL_RUST, case_id);
    r.ok = true;
    let solids = b.solids.len() as u32;
    r.bodies = Some(solids);
    r.lumps = Some(solids);
    r.solid = Some(solids >= 1);
    r.shells = Some(b.shells.len() as u32);
    r.faces = Some(b.faces.len() as u32);
    r.loops = Some(b.loops.len() as u32);
    r.vertices = Some(b.vertices.len() as u32);
    r.edges = Some(b.edges.len() as u32);

    let mut ft: Vec<String> = b
        .faces
        .iter()
        .map(|f| f.surface.type_name().to_string())
        .collect();
    ft.sort();
    r.face_types = Some(ft);
    let mut et: Vec<String> = b
        .edges
        .iter()
        .filter(|e| e.curve.is_some())
        .map(|e| edge_type(&e.curve))
        .collect();
    et.sort();
    r.edge_types = Some(et);
    r.degenerate_edges = Some(b.edges.iter().filter(|e| e.curve.is_none()).count() as u32);
    // literal definition (as the ASM probe): no geometry, or start vertex == end vertex
    r.degenerate_edges_literal = Some(
        b.edges
            .iter()
            .filter(|e| e.curve.is_none() || e.start == e.end || (e.t1 - e.t0).abs() <= 0.0)
            .count() as u32,
    );

    // seam: a non-degenerate edge used twice by one face
    let mut uses: HashMap<usize, Vec<usize>> = HashMap::new();
    for c in &b.coedges {
        let face = b
            .loops
            .get(c.owner.idx())
            .map_or(usize::MAX, |l| l.face.idx());
        uses.entry(c.edge.idx()).or_default().push(face);
    }
    let seams = b
        .edges
        .iter()
        .enumerate()
        .filter(|(i, e)| {
            // some face uses the edge twice (as ASM counts it: this includes a non-manifold
            // contact edge that runs along a seam of one face)
            e.curve.is_some()
                && uses
                    .get(i)
                    .is_some_and(|f| f.iter().any(|x| f.iter().filter(|y| *y == x).count() >= 2))
        })
        .count();
    r.seam_edges = Some(seams as u32);

    if solids >= 1 {
        match guarded(|| cad_geom::mass::solid_properties(b, SolidId(0))) {
            Guarded::Done(Ok(m)) => {
                if solids > 1 {
                    r.notes
                        .push("mass properties of the first solid only".into());
                }
                r.volume = Some(m.volume);
                r.area = Some(m.area);
                r.centroid = Some([m.centroid.x, m.centroid.y, m.centroid.z]);
                r.inertia_centroid = Some(m.inertia);
                r.mass_path = Some(
                    "exact: parametric face integration (cad_geom::mass::solid_properties)".into(),
                );
            }
            Guarded::Done(Err(e)) => r.notes.push(format!("mass properties failed: {e}")),
            Guarded::Panicked(m, true) => r
                .notes
                .push(format!("mass properties: {NOT_IMPLEMENTED} ({m})")),
            Guarded::Panicked(m, false) => r.notes.push(format!("mass properties: PANIC ({m})")),
        }
    }

    // vertex-based extent only bounds polyhedral parts; it is not a bbox for curved faces
    let size = b
        .vertices
        .iter()
        .map(|v| v.point.coords.amax())
        .fold(1.0_f64, f64::max);
    match guarded(|| cad_geom::validate(b, &Tolerance::for_size(size))) {
        Guarded::Done(rep) if !rep.is_valid() && fallback.is_some() => {
            let t = fallback.expect("checked");
            let t = Tolerance {
                linear: t.linear.max(size * 1e-12),
                ..t
            };
            match guarded(|| cad_geom::validate(b, &t)) {
                Guarded::Done(rep2) => {
                    r.valid = Some(rep2.is_valid());
                    r.notes.push(format!(
                        "valid at the boolean tolerance {:e} mm: {} (at {:e} mm: {} issue(s), e.g. {:?})",
                        t.linear,
                        rep2.is_valid(),
                        Tolerance::for_size(size).linear,
                        rep.issues.len(),
                        rep.issues.first()
                    ));
                    if !rep2.is_valid() {
                        r.notes
                            .push(format!("validation issues: {:?}", rep2.issues));
                    }
                }
                Guarded::Panicked(m, _) => r.notes.push(format!("validate: PANIC ({m})")),
            }
        }
        Guarded::Done(rep) => {
            r.valid = Some(rep.is_valid());
            if !rep.is_valid() {
                r.notes.push(format!("validation issues: {:?}", rep.issues));
            }
        }
        Guarded::Panicked(m, true) => r.notes.push(format!("validate: {NOT_IMPLEMENTED} ({m})")),
        Guarded::Panicked(m, false) => r.notes.push(format!("validate: PANIC ({m})")),
    }
    r.notes.push("counts are arena sizes (one entry per vertex/edge/loop/face); seam = edge used twice by one face".into());
    r.notes.push("bbox not exposed by cad-geom".into());
    r
}

/// Folds a boolean case with `cad_geom::boolean` at `BOOLEAN_TOLERANCE`; `None` when an operand
/// is not a box, a cylinder or a sphere (cones and tori are `Unsupported` in cad-geom booleans).
fn build_boolean(spec: &Spec) -> Option<GResult<Brep>> {
    let Spec::Boolean { op, target, tools } = spec else {
        return None;
    };
    let supported = std::iter::once(target.as_ref()).chain(tools).all(|s| {
        matches!(s, Spec::Box { .. } | Spec::Sphere { .. })
            || matches!(s, Spec::Cylinder { r0, r1, .. } if r0 == r1)
    });
    if !supported {
        return None;
    }
    let op = match op {
        BoolOp::Union => cad_geom::BoolOp::Union,
        BoolOp::Cut => cad_geom::BoolOp::Cut,
        BoolOp::Intersect => cad_geom::BoolOp::Intersect,
    };
    let fold = || -> GResult<Brep> {
        let mut acc = build_primitive(target)?;
        for t in tools {
            acc = cad_geom::boolean_default(&acc, &build_primitive(t)?, op)?;
        }
        Ok(acc)
    };
    Some(fold())
}

/// Runs one catalogue case through cad-geom.
pub fn run_case(case: &Case) -> KernelResult {
    if case.spec.is_boolean() {
        let Some(built) = guarded(|| build_boolean(&case.spec)).into_option() else {
            return KernelResult::failed(KERNEL_RUST, &case.id, "PANIC in cad_geom::boolean");
        };
        return match built {
            Some(Ok(b)) => {
                let mut r = measure_with(&case.id, &b, Some(cad_geom::boolean::BOOLEAN_TOLERANCE));
                r.notes.push(format!(
                    "cad_geom::boolean, tools folded in order, tolerance {:?} (ASM)",
                    cad_geom::boolean::BOOLEAN_TOLERANCE
                ));
                if !b.nonmanifold_edges.is_empty() {
                    r.notes.push(format!(
                        "{} edge(s) flagged non-manifold (line contact, as ASM: one edge with 4 coedges)",
                        b.nonmanifold_edges.len()
                    ));
                }
                r
            }
            Some(Err(e)) => KernelResult::failed(KERNEL_RUST, &case.id, e.to_string()),
            None => KernelResult::not_implemented(
                KERNEL_RUST,
                &case.id,
                "cad-geom booleans support boxes, cylinders and spheres; this case has another operand (cad_geom::boolean returns Unsupported)",
            ),
        };
    }
    match guarded(|| build_primitive(&case.spec)) {
        Guarded::Done(Ok(b)) => measure(&case.id, &b),
        Guarded::Done(Err(e)) => KernelResult::failed(KERNEL_RUST, &case.id, e.to_string()),
        Guarded::Panicked(m, true) => {
            KernelResult::not_implemented(KERNEL_RUST, &case.id, format!("primitive: {m}"))
        }
        Guarded::Panicked(m, false) => {
            KernelResult::failed(KERNEL_RUST, &case.id, format!("PANIC: {m}"))
        }
    }
}
