//! Extrude: a planar profile swept along its plane normal into an exact B-rep.
//!
//! The region (see [`crate::profile`]) is swept from offset `z0` to `z1` along `plane.z`. Faces:
//! one side face per profile segment (a plane for a line, a cylinder for an arc or full circle),
//! then the bottom cap (plane at `z0`, `reversed`) and the top cap (plane at `z1`). Edges: each
//! segment at `z0` and at `z1`, plus one straight edge from `z0` to `z1` at every segment start;
//! a full-circle loop has no vertical edge and its side face is seamless, bounded by the two
//! closed circles, exactly like [`crate::primitives::make_cylinder`] (ASM convention,
//! `research/evidence/asm-v5`). Extruding a rectangle gives the box's 6/12/8 and a circle the
//! cylinder's 3/2/2; extruding a rectangle with a circular hole gives 7/14/10 like ASM's
//! `bool.cut.hole`. ASM's own extrude (Fusion ExtrudeFeature, probe v6:
//! `research/evidence/asm-v6`) builds exactly these topologies for all nine `ext.*` cases of
//! `research/feature-cases.json`: one face per segment (tangent line/arc joints stay edges), a
//! seamless side face for a full circle (`tests/extrude.rs::extrude_matches_asm_feature_reference`).
//! Merging consecutive collinear lines or co-circular arcs (see [`crate::profile`]) is a design
//! choice: no recorded case has such neighbours.
//!
//! Orientation: a segment's material lies to its left (profile orientation), so the side face of
//! segment i has the loop [bottom edge along the profile, vertical edge at the segment end
//! upwards, top edge against the profile, vertical edge at the start downwards]; the top cap uses
//! the top edges along the profile, the bottom cap the bottom edges against it. Edge curves:
//! lines run from the segment start to its end; arcs and circles run counter-clockwise about the
//! normal (circle parameter increasing), so a clockwise segment uses its edges reversed. A side
//! face of a clockwise arc is a cylinder face with `reversed = true` (material outside the
//! circle). Cylinder and circle frames put parameter 0 at the counter-clockwise start of the arc,
//! so an arc face spans u ∈ [0, |sweep|].

use crate::profile::{Region, Seg, P2};
use crate::{Brep, Curve, EdgeId, Frame, GResult, GeomError, Profile, Surface, Tolerance, VertexId};
use std::f64::consts::TAU;

/// Extrudes `profile` along its plane normal between offsets `from` and `to` (either order;
/// they must differ by more than `tol.linear`). One solid, one shell.
pub fn extrude(profile: &Profile, from: f64, to: f64, tol: &Tolerance) -> GResult<Brep> {
    if !(from.is_finite() && to.is_finite()) || (to - from).abs() <= tol.linear {
        return Err(GeomError::InvalidInput(format!("extrude extent must be finite and non-zero, got {from} to {to}")));
    }
    let region = profile.region(tol)?;
    Ok(extrude_region(&region, from.min(to), from.max(to)))
}

/// One profile segment at a height, as an edge, plus whether the edge runs along the profile.
struct SegEdge {
    edge: EdgeId,
    along: bool,
}

/// Builds the solid of a checked region between `z0 < z1`.
pub fn extrude_region(region: &Region, z0: f64, z1: f64) -> Brep {
    let mut b = Brep::new();
    let solid = b.add_solid();
    let shell = b.add_shell(Some(solid));
    let loops: Vec<&Vec<Seg>> = region.loops().collect();

    // vertices at each segment start (none for a full circle: its vertex is made with the edge)
    struct LoopBuild {
        bottom: Vec<SegEdge>,
        top: Vec<SegEdge>,
        vertical: Vec<Option<EdgeId>>,
    }
    let mut built = Vec::with_capacity(loops.len());
    for l in &loops {
        let k = l.len();
        let full = k == 1 && l[0].is_full_circle();
        let vb: Vec<VertexId> = if full { Vec::new() } else { l.iter().map(|s| b.add_vertex(region.world(s.start(), z0))).collect() };
        let vt: Vec<VertexId> = if full { Vec::new() } else { l.iter().map(|s| b.add_vertex(region.world(s.start(), z1))).collect() };
        let mut bottom = Vec::with_capacity(k);
        let mut top = Vec::with_capacity(k);
        for (z, out, vs) in [(z0, &mut bottom, &vb), (z1, &mut top, &vt)] {
            for i in 0..k {
                let ends = if full { None } else { Some((vs[i], vs[(i + 1) % k])) };
                out.push(segment_edge(&mut b, region, &l[i], z, ends));
            }
        }
        let vertical = if full { vec![None] } else { (0..k).map(|i| Some(b.add_line_edge(vb[i], vt[i]))).collect() };
        built.push(LoopBuild { bottom, top, vertical });
    }

    // side faces, loop by loop, in segment order
    for (l, lb) in loops.iter().zip(&built) {
        let k = l.len();
        for i in 0..k {
            let s = &l[i];
            let (surface, reversed) = side_surface(region, s);
            let face = b.add_face(shell, surface, reversed);
            let (bot, top) = (&lb.bottom[i], &lb.top[i]);
            match (lb.vertical[i], lb.vertical[(i + 1) % k]) {
                (Some(v_start), Some(v_end)) => {
                    b.add_loop(face, &[(bot.edge, !bot.along), (v_end, false), (top.edge, top.along), (v_start, true)]);
                }
                _ => {
                    // full circle: two seamless loops
                    b.add_loop(face, &[(bot.edge, !bot.along)]);
                    b.add_loop(face, &[(top.edge, top.along)]);
                }
            }
        }
    }

    // caps: outer loop first, then holes
    for (z, reversed) in [(z0, true), (z1, false)] {
        let frame = Frame { origin: region.world(P2::origin(), z), ..region.plane };
        let face = b.add_face(shell, Surface::Plane { frame }, reversed);
        for lb in &built {
            let edges = if reversed { &lb.bottom } else { &lb.top };
            // `add_loop` takes (edge, reversed): the top cap walks along the profile; the bottom
            // cap (seen from −z) walks against it, i.e. in reverse segment order
            let uses: Vec<(EdgeId, bool)> =
                if reversed { edges.iter().rev().map(|e| (e.edge, e.along)).collect() } else { edges.iter().map(|e| (e.edge, !e.along)).collect() };
            b.add_loop(face, &uses);
        }
    }
    b
}

/// The edge of segment `s` at height `z`. `ends` = (vertex at the segment start, at its end);
/// `None` for a full circle (a closed edge with one new vertex at its parameter 0).
fn segment_edge(b: &mut Brep, region: &Region, s: &Seg, z: f64, ends: Option<(VertexId, VertexId)>) -> SegEdge {
    match *s {
        Seg::Line { .. } => {
            let (va, vb) = ends.expect("line segments have vertices");
            SegEdge { edge: b.add_line_edge(va, vb), along: true }
        }
        Seg::Arc { r, sweep, .. } => {
            let frame = arc_frame(region, s, z);
            let span = sweep.abs();
            let ccw = sweep > 0.0;
            let edge = match ends {
                None => {
                    let v = b.add_vertex(frame.origin + frame.x * r);
                    b.add_edge(Some(Curve::Circle { frame, radius: r }), 0.0, TAU, v, v)
                }
                Some((va, vb)) => {
                    let (start, end) = if ccw { (va, vb) } else { (vb, va) };
                    b.add_edge(Some(Curve::Circle { frame, radius: r }), 0.0, span, start, end)
                }
            };
            SegEdge { edge, along: ccw }
        }
    }
}

/// Frame of an arc at height `z`: origin at the centre, z = plane normal, x towards the arc's
/// counter-clockwise start (the segment start for a counter-clockwise arc, its end otherwise).
fn arc_frame(region: &Region, s: &Seg, z: f64) -> Frame {
    let Seg::Arc { c, a0, sweep, .. } = *s else { unreachable!("arc_frame of a line") };
    let start = if sweep > 0.0 { a0 } else { a0 + sweep };
    let pl = &region.plane;
    let x = pl.x * start.cos() + pl.y * start.sin();
    Frame { origin: region.world(c, z), x, y: pl.z.cross(&x), z: pl.z }
}

/// Side surface of a segment and its `reversed` flag (outward normal against the natural one).
fn side_surface(region: &Region, s: &Seg) -> (Surface, bool) {
    let pl = &region.plane;
    match *s {
        Seg::Line { a, b } => {
            // x along the segment, y = normal: z = x × n points to the segment's right, which is
            // outward because the material lies to its left
            let d = b - a;
            let x = (pl.x * d.x + pl.y * d.y).normalize();
            let frame = Frame { origin: region.world(a, 0.0), x, y: pl.z, z: x.cross(&pl.z) };
            (Surface::Plane { frame }, false)
        }
        Seg::Arc { r, sweep, .. } => (Surface::Cylinder { frame: arc_frame(region, s, 0.0), radius: r }, sweep < 0.0),
    }
}

