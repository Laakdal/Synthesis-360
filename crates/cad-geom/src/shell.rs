//! Shell (hollow): a solid turned into a wall of constant thickness, with chosen faces removed.
//!
//! Scope: solids bounded by planes only (every face a plane, every edge a line), one solid with
//! one shell. Method (offset by plane shifting, then a boolean):
//! 1. Every face plane `n·x = h` (n the outward normal) is shifted: kept faces inward to
//!    `n·x = h − t`, removed faces by `+s` (s ≥ 0, see below). The topology is kept: each inner
//!    vertex is the unique intersection point of the shifted planes of the faces around the base
//!    vertex (least squares over the k ≥ 3 planes, accepted only when the normals have full rank
//!    — smallest singular value ≥ [`MIN_PLANE_SINGULAR_VALUE`] — and every plane passes within
//!    `tol.linear` of the point). A vertex where the planes do not meet in one well-conditioned
//!    point gives `Unsupported`; nothing is approximated.
//! 2. The *cavity* (s = 0: removed faces stay in place) must be a valid solid of the same
//!    topology: every edge keeps its direction and a length above `tol.linear`, every loop keeps
//!    its orientation about its face normal, no edge pierces a face it does not touch, the model
//!    validates and its volume is positive. Otherwise the thickness is too large for the part
//!    (a face vanishes or inverts): `InvalidInput`.
//! 3. The result is `base − tool` by the planar boolean ([`crate::boolean`], ASM tolerance). The
//!    tool is first the cavity with removed faces pushed out by s = t (no coplanar faces, the
//!    opening is cut cleanly), then — if that tool is invalid or its result does not check out —
//!    the cavity itself (removed faces coplanar with the base faces; the boolean's coplanar
//!    classification opens them). The extended tool is wrong when it reaches through kept
//!    material (a removed face next to a reflex edge), which the volume check below detects.
//! 4. A result is accepted only when it is one valid solid (one shell when faces are removed,
//!    outer shell plus one void shell when none are) whose volume equals
//!    `vol(base) − vol(cavity)` to 1e-9 relative, i.e. the tool removed exactly the cavity.
//!    Otherwise `Unsupported`.
//!
//! With `remove` empty the result is a closed hollow solid: the outer shell and an inward-facing
//! void shell (the cavity).

use crate::boolean::{boolean, BoolOp, BOOLEAN_TOLERANCE};
use crate::mass::solid_properties;
use crate::validate::validate;
use crate::{Brep, Curve, FaceId, Frame, GResult, GeomError, SolidId, Surface, Tolerance, VertexId, P3, V3};

/// Smallest singular value of the stacked unit normals at a vertex for its offset point to count
/// as well conditioned (planes closer to parallel than ~1e-6 rad give `Unsupported`).
pub const MIN_PLANE_SINGULAR_VALUE: f64 = 1e-6;

/// Relative agreement required between `vol(result)` and `vol(base) − vol(cavity)`.
const VOLUME_CHECK: f64 = 1e-9;

/// Hollows `brep` inwards to wall `thickness`, opening the faces in `remove` (see the module docs
/// for scope, method and error cases).
pub fn shell(brep: &Brep, remove: &[FaceId], thickness: f64, tol: &Tolerance) -> GResult<Brep> {
    let invalid = |m: String| Err(GeomError::InvalidInput(format!("shell: {m}")));
    let unsupported = |m: String| Err(GeomError::Unsupported(format!("shell: {m}")));
    if !(tol.linear.is_finite() && tol.linear > 0.0) {
        return invalid(format!("invalid tolerance {tol:?}"));
    }
    if !(thickness.is_finite() && thickness > tol.linear) {
        return invalid(format!("thickness {thickness} must be finite and greater than {}", tol.linear));
    }
    let nf = brep.faces.len();
    let mut removed = vec![false; nf];
    for &f in remove {
        if f.idx() >= nf {
            return invalid(format!("face F{} does not exist", f.0));
        }
        removed[f.idx()] = true;
    }
    if nf > 0 && removed.iter().all(|&r| r) {
        return invalid("removing every face leaves nothing".into());
    }
    if brep.solids.len() != 1 || brep.solids[0].shells.len() != 1 {
        return unsupported(format!(
            "only a single solid with a single shell is supported ({} solids, {} shells)",
            brep.solids.len(),
            brep.shells.len()
        ));
    }
    let shell_faces = &brep.shell(brep.solids[0].shells[0]).faces;
    if shell_faces.len() != nf || brep.shells.len() != 1 {
        return unsupported("model has faces or shells outside its solid".into());
    }
    for f in brep.face_ids() {
        let face = brep.face(f);
        if !matches!(face.surface, Surface::Plane { .. }) {
            return unsupported(format!("face F{} is a {} (only planar solids are supported)", f.0, face.surface.type_name()));
        }
        if face.loops.is_empty() {
            return invalid(format!("face F{} has no loop", f.0));
        }
    }
    for (i, e) in brep.edges.iter().enumerate() {
        if !matches!(e.curve, Some(Curve::Line { .. })) {
            let t = e.curve.as_ref().map_or("degenerate", |c| c.type_name());
            return unsupported(format!("edge E{i} is a {t} (only line edges are supported)"));
        }
    }
    let base_vol = solid_properties(brep, SolidId(0))?.volume;

    // the exact cavity: kept faces inward by t, removed faces in place
    let cavity_shift: Vec<f64> = removed.iter().map(|&r| if r { 0.0 } else { -thickness }).collect();
    let cavity = offset_solid(brep, &cavity_shift, tol)?;
    check_offset(brep, &cavity, tol).map_err(|m| GeomError::InvalidInput(format!("shell: thickness {thickness} too large: {m}")))?;
    // positive: check_offset rejected a non-positive cavity volume
    let cavity_vol = solid_properties(&cavity, SolidId(0))?.volume;

    let mut tools: Vec<Brep> = Vec::new();
    if !remove.is_empty() {
        // removed faces pushed outwards so the cut crosses them with no coplanar faces
        let shift: Vec<f64> = removed.iter().map(|&r| if r { thickness } else { -thickness }).collect();
        if let Ok(t) = offset_solid(brep, &shift, tol)
            && check_offset(brep, &t, tol).is_ok()
        {
            tools.push(t);
        }
    }
    tools.push(cavity);
    let want = base_vol - cavity_vol;
    let shells_wanted = if remove.is_empty() { 2 } else { 1 };
    let mut why = Vec::new();
    for tool in &tools {
        match boolean(brep, tool, BoolOp::Cut, &BOOLEAN_TOLERANCE) {
            Ok(r) => match accept(&r, want, shells_wanted, tol) {
                Ok(()) => return Ok(r),
                Err(m) => why.push(m),
            },
            Err(e) => why.push(e.to_string()),
        }
    }
    unsupported(format!("the cut does not give the expected wall ({})", why.join("; ")))
}

/// Checks a candidate result: one valid solid with the expected number of shells and volume.
fn accept(r: &Brep, want: f64, shells: usize, tol: &Tolerance) -> Result<(), String> {
    if r.solids.len() != 1 || r.solids[0].shells.len() != shells {
        return Err(format!("{} solids / {} shells, expected one solid with {shells} shells", r.solids.len(), r.shells.len()));
    }
    let rep = validate(r, tol);
    if !rep.is_valid() {
        return Err(format!("result does not validate: {:?}", rep.issues));
    }
    let v = solid_properties(r, SolidId(0)).map_err(|e| e.to_string())?.volume;
    if (v - want).abs() > VOLUME_CHECK * want.abs().max(1.0) {
        return Err(format!("result volume {v} differs from base − cavity = {want}"));
    }
    Ok(())
}

/// Outward unit normal and plane frame of a planar face.
fn plane(brep: &Brep, f: FaceId) -> (V3, Frame) {
    let face = brep.face(f);
    let Surface::Plane { frame } = &face.surface else { unreachable!("checked planar") };
    (if face.reversed { -frame.z } else { frame.z }, *frame)
}

/// The solid with every face plane moved by `shift[f]` along its outward normal and every vertex
/// at the intersection of its faces' moved planes (same topology, same face order).
fn offset_solid(brep: &Brep, shift: &[f64], tol: &Tolerance) -> GResult<Brep> {
    let unsupported = |m: String| GeomError::Unsupported(format!("shell: {m}"));
    let planes: Vec<(V3, Frame)> = brep.face_ids().map(|f| plane(brep, f)).collect();
    let mut around: Vec<Vec<usize>> = vec![Vec::new(); brep.vertices.len()];
    for f in brep.face_ids() {
        for c in brep.face_coedges(f) {
            let v = brep.coedge_start(c).idx();
            if !around[v].contains(&f.idx()) {
                around[v].push(f.idx());
            }
        }
    }
    let mut pts = Vec::with_capacity(brep.vertices.len());
    for (v, fs) in around.iter().enumerate() {
        if fs.len() < 3 {
            return Err(unsupported(format!("vertex V{v} lies on {} faces (need at least 3)", fs.len())));
        }
        let m = nalgebra::DMatrix::from_fn(fs.len(), 3, |i, j| planes[fs[i]].0[j]);
        let rhs = nalgebra::DVector::from_fn(fs.len(), |i, _| {
            let (n, fr) = &planes[fs[i]];
            n.dot(&fr.origin.coords) + shift[fs[i]]
        });
        let svd = m.svd(true, true);
        let smin = svd.singular_values.iter().copied().fold(f64::INFINITY, f64::min);
        if smin < MIN_PLANE_SINGULAR_VALUE {
            return Err(unsupported(format!(
                "the offset planes at vertex V{v} are near-parallel (smallest singular value {smin:e}): no unique offset point"
            )));
        }
        let x = svd.solve(&rhs, 0.0).map_err(|e| GeomError::Numerical(format!("shell: vertex V{v}: {e}")))?;
        let p = P3::new(x[0], x[1], x[2]);
        for (i, &f) in fs.iter().enumerate() {
            let r = (planes[f].0.dot(&p.coords) - rhs[i]).abs();
            if r > tol.linear {
                return Err(unsupported(format!(
                    "the {} offset planes at vertex V{v} do not meet in one point (F{f} misses by {r:e})",
                    fs.len()
                )));
            }
        }
        pts.push(p);
    }
    let mut out = Brep::new();
    let s = out.add_solid();
    let sh = out.add_shell(Some(s));
    for p in pts {
        out.add_vertex(p);
    }
    for e in &brep.edges {
        let (a, b) = (out.vertex(e.start).point, out.vertex(e.end).point);
        if (b - a).norm() <= tol.linear {
            // a zero-length offset edge: a face has vanished (too thick for the part)
            return Err(GeomError::InvalidInput(format!(
                "shell: thickness too large: an offset edge collapses ({:?} → {:?})",
                brep.vertex(e.start).point,
                brep.vertex(e.end).point
            )));
        }
        out.add_line_edge(e.start, e.end);
    }
    for f in brep.face_ids() {
        let (n, fr) = planes[f.idx()];
        let frame = Frame { origin: fr.origin + n * shift[f.idx()], ..fr };
        let nfid = out.add_face(sh, Surface::Plane { frame }, brep.face(f).reversed);
        for &l in &brep.face(f).loops {
            let uses: Vec<_> = brep.lp(l).coedges.iter().map(|&c| (brep.coedge(c).edge, brep.coedge(c).reversed)).collect();
            out.add_loop(nfid, &uses);
        }
    }
    Ok(out)
}

/// Vertex positions of a loop in order.
fn loop_points(b: &Brep, l: crate::LoopId) -> Vec<P3> {
    b.lp(l).coedges.iter().map(|&c| b.vertex(b.coedge_start(c)).point).collect()
}

/// Vector area (Newell) of a closed polygon.
fn vector_area(p: &[P3]) -> V3 {
    (0..p.len()).fold(V3::zeros(), |s, i| s + p[i].coords.cross(&p[(i + 1) % p.len()].coords)) / 2.0
}

/// Verifies that the offset `inner` has the same shape class as `base`: no edge reverses or
/// collapses, no loop flips, no edge pierces a face it does not touch, valid, positive volume.
fn check_offset(base: &Brep, inner: &Brep, tol: &Tolerance) -> Result<(), String> {
    for (i, (e0, e1)) in base.edges.iter().zip(&inner.edges).enumerate() {
        let d0 = base.vertex(e0.end).point - base.vertex(e0.start).point;
        let d1 = inner.vertex(e1.end).point - inner.vertex(e1.start).point;
        if d1.norm() <= tol.linear || d0.dot(&d1) <= 0.0 {
            return Err(format!("edge E{i} collapses or reverses"));
        }
    }
    for f in base.face_ids() {
        let (n, _) = plane(base, f);
        for (&l0, &l1) in base.face(f).loops.iter().zip(&inner.face(f).loops) {
            let (a0, p1) = (vector_area(&loop_points(base, l0)).dot(&n), loop_points(inner, l1));
            let a1 = vector_area(&p1).dot(&n);
            let perim: f64 = (0..p1.len()).map(|i| (p1[(i + 1) % p1.len()] - p1[i]).norm()).sum();
            if a0.signum() != a1.signum() || a1.abs() <= tol.linear * perim {
                return Err(format!("face F{} vanishes or inverts", f.0));
            }
        }
    }
    if let Some((e, f)) = pierce(inner, tol.linear) {
        return Err(format!("edge E{e} passes through face F{f} (the offset intersects itself)"));
    }
    let rep = validate(inner, tol);
    if !rep.is_valid() {
        return Err(format!("offset solid does not validate: {:?}", rep.issues));
    }
    match solid_properties(inner, SolidId(0)) {
        Ok(m) if m.volume > 0.0 => Ok(()),
        Ok(m) => Err(format!("offset volume {}", m.volume)),
        Err(e) => Err(e.to_string()),
    }
}

/// A face prepared for the piercing test: id, outward normal, plane frame, vertices, loops in
/// the frame's (x, y) coordinates.
type PlanarFace = (FaceId, V3, Frame, Vec<VertexId>, Vec<Vec<[f64; 2]>>);

/// First (edge, face) pair where the edge meets the closed face without sharing a vertex with it.
fn pierce(b: &Brep, tol: f64) -> Option<(usize, u32)> {
    let faces: Vec<PlanarFace> = b
        .face_ids()
        .map(|f| {
            let (n, fr) = plane(b, f);
            let verts: Vec<VertexId> = b.face_coedges(f).map(|c| b.coedge_start(c)).collect();
            let to2 = |p: P3| {
                let d = p - fr.origin;
                [d.dot(&fr.x), d.dot(&fr.y)]
            };
            let loops = b.face(f).loops.iter().map(|&l| loop_points(b, l).into_iter().map(to2).collect()).collect();
            (f, n, fr, verts, loops)
        })
        .collect();
    for (i, e) in b.edges.iter().enumerate() {
        let (p, q) = (b.vertex(e.start).point, b.vertex(e.end).point);
        for (f, n, fr, verts, loops) in &faces {
            if verts.contains(&e.start) || verts.contains(&e.end) {
                continue;
            }
            let (dp, dq) = (n.dot(&(p - fr.origin)), n.dot(&(q - fr.origin)));
            if (dp > tol && dq > tol) || (dp < -tol && dq < -tol) {
                continue;
            }
            let probes: Vec<P3> = if dp.abs() <= tol && dq.abs() <= tol {
                vec![p, q, nalgebra::center(&p, &q)]
            } else if dp.abs() <= tol {
                vec![p]
            } else if dq.abs() <= tol {
                vec![q]
            } else {
                vec![p + (q - p) * (dp / (dp - dq))]
            };
            let hit = probes.iter().any(|x| {
                let d = x - fr.origin;
                inside_or_on(loops, [d.dot(&fr.x), d.dot(&fr.y)], tol)
            });
            if hit {
                return Some((i, f.0));
            }
        }
    }
    None
}

/// Even-odd point-in-polygon over all loops, with points within `tol` of a boundary counted in.
fn inside_or_on(loops: &[Vec<[f64; 2]>], q: [f64; 2], tol: f64) -> bool {
    let mut inside = false;
    for l in loops {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            let (ab, aq) = ([b[0] - a[0], b[1] - a[1]], [q[0] - a[0], q[1] - a[1]]);
            let len2 = ab[0] * ab[0] + ab[1] * ab[1];
            let t = if len2 > 0.0 { ((aq[0] * ab[0] + aq[1] * ab[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let (dx, dy) = (aq[0] - t * ab[0], aq[1] - t * ab[1]);
            if dx * dx + dy * dy <= tol * tol {
                return true;
            }
            if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < a[0] + (q[1] - a[1]) / (b[1] - a[1]) * ab[0] {
                inside = !inside;
            }
        }
    }
    inside
}
