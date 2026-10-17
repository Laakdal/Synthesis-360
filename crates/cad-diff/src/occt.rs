//! OCCT runner. Builds every case through Open CASCADE 7.8.1 using only existing public APIs:
//!
//! * **Primitives** come from the vendored high-level `opencascade` crate, which wraps
//!   `BRepPrimAPI_MakeBox/Cylinder/Cone/Sphere/Torus` (true primitives, the same topology OCCT
//!   itself produces for a primitive). That crate keeps its `TopoDS_Shape` private, and
//!   `cad-kernel` has no constructor from a raw shape, so the shape is handed over through a
//!   STEP file (`opencascade::Shape::write_step` → `cad_kernel::occ::Shape::read_step`). The STEP
//!   reader rebuilds the B-rep (it may re-create degenerate and seam edges); the harness checks
//!   for box and cylinder that the bridged shape is identical in counts, types and volume to one
//!   built natively in cad-kernel ([`bridge_fidelity`]). That check is not possible for cone,
//!   sphere or torus, which cad-kernel cannot build as primitives.
//! * **Booleans** fold the bridged operands with `cad_kernel::occ::Shape::fuse/cut/common`
//!   (`BRepAlgoAPI_*` behind the exception-safe `cad360` shim, the path the application uses).
//! * **Measurements** use cad-kernel only: `subshapes` (TopExp::MapShapes, i.e. unique TopoDS
//!   entities), `face_info`/`edge_info` (GeomAbs types), `edge_faces` (seams), `bbox`
//!   (BRepBndLib::Add, enlarged by tolerances), `is_valid` (BRepCheck_Analyzer) and
//!   `inspect::mass_props` (BRepGProp). OCCT inertia is not exposed by cad-kernel, so
//!   `inertia_centroid` is always None.

use crate::cases::{BoolOp, Case, Spec};
use crate::result::{KERNEL_OCCT, KernelResult, occt_curve_name, occt_surface_name};
use cad_kernel::inspect::mass_props;
use cad_kernel::occ::{Seg3, Shape};
use glam::DVec3;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// OCCT version linked by `opencascade-sys` (occt-sys 7.8.1, static build).
pub const OCCT_VERSION: &str = "7.8.1";

// TopAbs_ShapeEnum ordinals accepted by `Shape::subshapes`.
const SOLID: i32 = 2;
const SHELL: i32 = 3;
const FACE: i32 = 4;
const WIRE: i32 = 5;
const EDGE: i32 = 6;
const VERTEX: i32 = 7;

/// OCCT's STEP layer keeps process-global state; the `opencascade` crate's writer is not under
/// cad-kernel's own exchange lock, so every bridge write+read is serialised here.
static BRIDGE: Mutex<()> = Mutex::new(());
static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn dv(a: [f64; 3]) -> DVec3 {
    DVec3::new(a[0], a[1], a[2])
}

fn is_plus_z(axis: [f64; 3]) -> bool {
    let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    n > 0.0 && axis[0].abs() / n < 1e-12 && axis[1].abs() / n < 1e-12 && axis[2] > 0.0
}

/// Builds a primitive with the `opencascade` crate (BRepPrimAPI_*).
fn primitive_oc(spec: &Spec) -> Result<opencascade::primitives::Shape, String> {
    use opencascade::primitives::Shape as Oc;
    Ok(match *spec {
        Spec::Box { corner, size } => {
            if size.iter().any(|s| *s <= 0.0) {
                return Err("box size must be positive".into());
            }
            Oc::box_from_corners(dv(corner), dv(corner) + dv(size))
        }
        Spec::Cylinder {
            base,
            axis,
            r0,
            r1,
            h,
        } => {
            if r0 != r1 {
                return Err("cylinder with r0 != r1".into());
            }
            Oc::cylinder(dv(base), r0, dv(axis).normalize(), h)
        }
        Spec::Cone {
            base,
            axis,
            r0,
            r1,
            h,
        } => {
            // opencascade's ConeBuilder has a fixed +Z axis; all catalogue cones are +Z.
            if !is_plus_z(axis) {
                return Err(
                    "HARNESS_LIMITATION: opencascade ConeBuilder only builds +Z cones".into(),
                );
            }
            Oc::cone()
                .at(dv(base))
                .bottom_radius(r0)
                .top_radius(r1)
                .height(h)
                .build()
        }
        Spec::Sphere { center, r } => Oc::sphere(r).at(dv(center)).build(),
        Spec::Torus {
            center,
            axis,
            major,
            minor,
        } => Oc::torus()
            .at(dv(center))
            .z_axis(dv(axis).normalize())
            .radius_1(major)
            .radius_2(minor)
            .build(),
        Spec::Boolean { .. } => return Err("not a primitive".into()),
    })
}

/// Hands a shape built by the `opencascade` crate to cad-kernel through a temporary STEP file.
fn bridge(shape: &opencascade::primitives::Shape) -> Result<Shape, String> {
    let _g = BRIDGE.lock().unwrap_or_else(|p| p.into_inner());
    let path = std::env::temp_dir().join(format!(
        "cad-diff-{}-{}.step",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    shape
        .write_step(&path)
        .map_err(|e| format!("STEP bridge write failed: {e}"))?;
    let read = Shape::read_step(&path.to_string_lossy())
        .map_err(|e| format!("STEP bridge read failed: {e}"));
    let _ = std::fs::remove_file(&path);
    read
}

/// Builds `spec` in OCCT (primitives via BRepPrimAPI + STEP bridge, booleans via cad-kernel).
pub fn build(spec: &Spec) -> Result<Shape, String> {
    cad_kernel::occ::quiet();
    match spec {
        Spec::Boolean { op, target, tools } => {
            let mut acc = build(target)?;
            for t in tools {
                let tool = build(t)?;
                acc = match op {
                    BoolOp::Union => acc.fuse(&tool),
                    BoolOp::Cut => acc.cut(&tool),
                    BoolOp::Intersect => acc.common(&tool),
                }
                .map_err(|e| e.0)?;
            }
            Ok(acc)
        }
        _ => bridge(&primitive_oc(spec)?),
    }
}

/// Box built natively in cad-kernel (rectangle face + prism), for the bridge fidelity check.
pub fn native_box(corner: [f64; 3], size: [f64; 3]) -> Result<Shape, String> {
    let [x, y, z] = corner;
    let p = [
        [x, y, z],
        [x + size[0], y, z],
        [x + size[0], y + size[1], z],
        [x, y + size[1], z],
    ];
    let lp: Vec<Seg3> = (0..4).map(|i| Seg3::Line(p[i], p[(i + 1) % 4])).collect();
    Shape::face_from_loops(&[lp])
        .and_then(|f| f.prism([0.0, 0.0, size[2]]))
        .map_err(|e| e.0)
}

/// Cylinder built natively in cad-kernel (`Shape::cylinder` = BRepPrimAPI_MakeCylinder).
pub fn native_cylinder(base: [f64; 3], axis: [f64; 3], r: f64, h: f64) -> Result<Shape, String> {
    let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    Shape::cylinder(base, [axis[0] / n, axis[1] / n, axis[2] / n], r, h).map_err(|e| e.0)
}

/// Measures an OCCT shape into a normalised result.
pub fn measure(case_id: &str, s: &Shape) -> KernelResult {
    let mut r = KernelResult::new(KERNEL_OCCT, case_id);
    r.ok = true;
    let count = |k: i32| s.subshapes(k).len() as u32;
    let solids = count(SOLID);
    r.bodies = Some(solids);
    r.lumps = Some(solids);
    r.solid = Some(solids >= 1);
    r.shells = Some(count(SHELL));
    r.faces = Some(count(FACE));
    r.loops = Some(count(WIRE));
    r.vertices = Some(count(VERTEX));

    let mut faces: Vec<String> = s
        .subshapes(FACE)
        .iter()
        .map(|f| {
            f.face_info()
                .map_or("other", |o| occt_surface_name(o[0] as i32))
                .to_string()
        })
        .collect();
    faces.sort();
    r.face_types = Some(faces);

    let edges = s.subshapes(EDGE);
    let infos: Vec<Option<[f64; 19]>> = edges.iter().map(|e| e.edge_info()).collect();
    let kinds: Vec<i32> = infos
        .iter()
        .map(|o| o.map_or(99, |o| o[0] as i32))
        .collect();
    // literal definition: no geometry, zero length, or start == end (closed flag or coincident ends)
    let literal = infos
        .iter()
        .filter(|o| match o {
            None => false,
            Some(o) => {
                let gap =
                    ((o[2] - o[8]).powi(2) + (o[3] - o[9]).powi(2) + (o[4] - o[10]).powi(2)).sqrt();
                o[0] < 0.0 || o[1] <= 1e-7 || o[18] > 0.5 || gap <= 1e-7
            }
        })
        .count() as u32;
    r.degenerate_edges_literal = Some(literal);
    let adjacency = s.edge_faces();
    let degenerate = kinds.iter().filter(|k| **k == -1).count() as u32;
    let seams = kinds
        .iter()
        .zip(&adjacency)
        .filter(|(k, f)| **k != -1 && f[0] >= 0 && f[1] == -1)
        .count() as u32;
    let mut et: Vec<String> = kinds
        .iter()
        .filter(|k| **k != -1)
        .map(|k| occt_curve_name(*k).to_string())
        .collect();
    et.sort();
    r.edges = Some(edges.len() as u32);
    r.edge_types = Some(et);
    r.degenerate_edges = Some(degenerate);
    r.seam_edges = Some(seams);

    match mass_props(s) {
        Some(m) => {
            r.volume = Some(m.volume);
            r.area = Some(m.area);
            r.centroid = Some(m.centroid);
        }
        None => r.notes.push("mass_props failed".into()),
    }
    r.mass_path = Some(
        "exact: BRepGProp volume/surface integration (cad_kernel::inspect::mass_props)".into(),
    );
    r.bbox = s.bbox().map(|b| [[b[0], b[1], b[2]], [b[3], b[4], b[5]]]);
    r.valid = Some(s.is_valid());
    r.notes.push("counts are unique TopoDS entities (TopExp::MapShapes); loops = unique wires; lumps = solids".into());
    r.notes.push(
        "seam = non-degenerate edge with one distinct adjacent face (shim edge_faces)".into(),
    );
    r.notes.push("inertia not exposed by cad-kernel (HARNESS_LIMITATION); bbox from BRepBndLib::Add (tolerance-enlarged)".into());
    r
}

/// Runs one catalogue case through OCCT.
pub fn run_case(case: &Case) -> KernelResult {
    match build(&case.spec) {
        Ok(s) => {
            let mut r = measure(&case.id, &s);
            if case.spec.is_boolean() {
                r.notes.push("operands: BRepPrimAPI primitives via STEP bridge; boolean: cad-kernel fuse/cut/common (BRepAlgoAPI)".into());
            } else {
                r.notes.push("built by BRepPrimAPI (opencascade crate), handed to cad-kernel through a STEP file".into());
            }
            r
        }
        Err(e) => KernelResult::failed(KERNEL_OCCT, &case.id, e),
    }
}

/// Kernel name of the ASM geometry re-measured by OCCT.
pub const KERNEL_ASM_STEP: &str = "asm-step(occt)";

/// Imports the ASM probe's STEP export of a case (`research/evidence/asm-v5/step/<id>.step`) with
/// cad-kernel's STEP reader and measures it with OCCT: a cross-check of the exported ASM geometry
/// against the numbers ASM itself reported. `None` when the file does not exist.
pub fn remeasure_asm_step(dir: &std::path::Path, case_id: &str) -> Option<KernelResult> {
    let path = dir.join(format!("{case_id}.step"));
    if !path.exists() {
        return None;
    }
    cad_kernel::occ::quiet();
    let read = {
        let _g = BRIDGE.lock().unwrap_or_else(|p| p.into_inner());
        Shape::read_step(&path.to_string_lossy())
    };
    let mut r = match read {
        Ok(s) => measure(case_id, &s),
        Err(e) => {
            KernelResult::failed(KERNEL_OCCT, case_id, format!("STEP import failed: {}", e.0))
        }
    };
    r.kernel = KERNEL_ASM_STEP.into();
    r.notes.push(format!(
        "ASM result exported as STEP by the probe, imported and measured by OCCT {OCCT_VERSION}"
    ));
    Some(r)
}

/// Result of comparing a STEP-bridged primitive with the same primitive built natively.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BridgeCheck {
    pub primitive: String,
    pub identical: bool,
    pub differences: Vec<String>,
}

/// Checks that the STEP bridge does not change topology, types or volume, for the primitives
/// cad-kernel can also build natively (box via face+prism, cylinder via MakeCylinder).
pub fn bridge_fidelity() -> Vec<BridgeCheck> {
    let mut out = Vec::new();
    let cases: Vec<(&str, Spec, Result<Shape, String>)> = vec![
        (
            "box",
            Spec::Box {
                corner: [0.0; 3],
                size: [50.0, 30.0, 10.0],
            },
            native_box([0.0; 3], [50.0, 30.0, 10.0]),
        ),
        (
            "cylinder",
            Spec::Cylinder {
                base: [1.0, 2.0, 3.0],
                axis: [1.0, 1.0, 1.0],
                r0: 4.0,
                r1: 4.0,
                h: 15.0,
            },
            native_cylinder([1.0, 2.0, 3.0], [1.0, 1.0, 1.0], 4.0, 15.0),
        ),
    ];
    for (name, spec, native) in cases {
        let mut diffs = Vec::new();
        match (build(&spec), native) {
            (Ok(b), Ok(n)) => {
                let (a, c) = (measure(name, &b), measure(name, &n));
                macro_rules! eq {
                    ($f:ident) => {
                        if a.$f != c.$f {
                            diffs.push(format!(
                                "{}: bridged {:?} vs native {:?}",
                                stringify!($f),
                                a.$f,
                                c.$f
                            ));
                        }
                    };
                }
                eq!(solid);
                eq!(shells);
                eq!(faces);
                eq!(edges);
                eq!(vertices);
                eq!(loops);
                eq!(face_types);
                eq!(edge_types);
                eq!(degenerate_edges);
                eq!(seam_edges);
                eq!(valid);
                let (va, vc) = (a.volume.unwrap_or(f64::NAN), c.volume.unwrap_or(f64::NAN));
                // NaN (a missing volume) must count as a difference
                let close = (va - vc).abs() <= 1e-9 * va.abs().max(vc.abs());
                if !close {
                    diffs.push(format!("volume: bridged {va} vs native {vc}"));
                }
            }
            (b, n) => diffs.push(format!(
                "build failed: bridged {:?}, native {:?}",
                b.err(),
                n.err()
            )),
        }
        out.push(BridgeCheck {
            primitive: name.into(),
            identical: diffs.is_empty(),
            differences: diffs,
        });
    }
    out
}
