//! Revolve: a planar profile rotated about an axis in its plane into an exact B-rep.
//!
//! Canonical frame. The region (see [`crate::profile`]) is mapped to meridian coordinates (ρ, h)
//! in a frame F: origin = the axis origin (projected into the plane), z = the axis direction
//! times the sign of the angle (so the sweep always runs through α = |angle| > 0 about F.z), x =
//! the in-plane direction perpendicular to the axis on the material's side (ρ ≥ 0), y = z × x.
//! A meridian point (ρ, h) at sweep angle φ is `o + ρ(cos φ·x + sin φ·y) + h·z`. When that map
//! reverses the profile's orientation the loops are reversed, so in (ρ, h) too the outer loop is
//! counter-clockwise and the material lies to the left of every segment. Points within
//! `tol.linear` of the axis are snapped onto it (ρ = 0).
//!
//! Checks: angle finite with 0 < |angle| ≤ 2π (|angle| ≥ 2π − `tol.angular` is a full
//! revolution); axis finite, its direction within `tol.angular` of the plane and its origin
//! within `tol.linear` of it (both are then projected exactly into the plane); the region on one
//! side of the axis (it may touch the axis along segments or at points, but not cross it).
//!
//! Faces. Each profile segment off the axis sweeps one lateral face; a segment on the axis
//! sweeps nothing. Surfaces use frames with x = F.x, y = F.y, z = F.z at the height of the
//! segment's centre / base, so the surface u is the sweep angle φ:
//! line ⟂ axis → `Plane` (disc or annulus; `reversed` when the segment runs away from the axis),
//! line ∥ axis → `Cylinder`, oblique line → `Cone` (frame at the end farther from the axis,
//! signed half angle), both `reversed` when the segment runs towards −h; arc centred on the axis
//! → `Sphere`, other arc → `Torus`, both `reversed` for a clockwise arc (material outside the
//! circle). With (φ, s) — sweep angle, fraction along the segment — the map is positively oriented
//! about the outward normal (∂φ × ∂s = ρ·n_out), so every lateral loop below runs
//! counter-clockwise about the outward normal whatever the surface.
//!
//! Partial revolution (|angle| < 2π). Each profile vertex off the axis gives two vertices (φ = 0
//! and φ = α) and the circular arc it traces (`Circle` on the axial frame at its height, t ∈ [0, α],
//! parameter 0 at the start cap); a vertex on the axis is one vertex and traces nothing. Each
//! segment gives its copy at φ = 0 and at φ = α (a segment on the axis gives one edge, shared by
//! both caps). Lateral loop: [arc of the start vertex forward, copy at α along the profile, arc of
//! the end vertex reversed, copy at 0 against the profile] (arcs of axis vertices omitted). The
//! caps are planes in the meridian planes, frame (o, radial(φ), z, radial(φ) × z) so that their
//! (u, v) are (ρ, h): the start cap (outward −∂φ, `reversed = false`) uses the copies at 0 along
//! the profile, the end cap (`reversed = true`) the copies at α against it. A full-circle loop
//! traces nothing: its torus face is seamless, bounded by the two closed circle copies (loops
//! [copy at α along], [copy at 0 against], each winding once round v), as ASM builds
//! `rev.circle.half` (`research/evidence/asm-v6`, F3 E2 V2 L4); a lateral face
//! touching the axis (cone apex, sphere pole, disc centre) has no edge there — its pcurve jumps
//! in u at the pole, which [`crate::mass`] and [`crate::tess`] handle.
//!
//! Full revolution: ASM's seamless convention, as in [`crate::primitives`]. Each vertex off the
//! axis traces one closed circle (one vertex at φ = 0); a lateral face has the loops [start
//! circle forward] and [end circle reversed]; a cone end on the axis gets a degenerate apex edge
//! (no curve, u ∈ [0, 2π], its own vertex) like `make_cone`; a plane or sphere end on the axis
//! gets nothing (like the cylinder's caps and `make_sphere`); a full circle is a loopless torus.
//! The lateral faces of one loop are connected only through circles, so the boundary splits at
//! the axis into closed pieces: the piece of positive volume (π∮ρ² dh > 0) is the outer shell,
//! every other piece (a hole loop, a pocket of the outer loop between two axis contacts) bounds a
//! void and gets its own shell of the same solid.
//!
//! | profile                                 | F | E | V | L | genus | like             |
//! |-----------------------------------------|---|---|---|---|-------|------------------|
//! | rectangle off the axis, 360°            | 4 | 4 | 4 | 8 | 1     | washer           |
//! | rectangle on the axis, 360°             | 3 | 2 | 2 | 4 | 0     | `make_cylinder`  |
//! | right triangle on the axis, 360°        | 2 | 2 | 2 | 3 | 0     | `make_cone`      |
//! | half disc on the axis, 360°             | 1 | 0 | 0 | 0 | 0     | `make_sphere`    |
//! | circle off the axis, 360°               | 1 | 0 | 0 | 0 | 1     | `make_torus`     |
//! | rectangle off the axis, partial         | 6 | 12| 8 | 6 | 0     | box topology     |
//! | circle off the axis, partial            | 3 | 2 | 2 | 4 | 0     |                  |
//!
//! Not supported (`GeomError::Unsupported`): an arc not centred on the axis that reaches it (its
//! torus would be singular on the face: spindle / horn tori, which mass properties and
//! tessellation do not handle).

use crate::profile::{Region, Seg, P2};
use crate::{Brep, Curve, EdgeId, Frame, GResult, GeomError, Profile, ShellId, Surface, Tolerance, VertexId, P3, V3};
use std::f64::consts::{PI, TAU};

/// Revolves `profile` about the line through `axis_origin` along `axis_dir` by `angle` radians
/// (right-hand rule about `axis_dir`, 0 < |angle| ≤ 2π). One solid; a full revolution of a
/// profile with holes (or pockets between two axis contacts) has void shells after the outer one.
pub fn revolve(profile: &Profile, axis_origin: P3, axis_dir: V3, angle: f64, tol: &Tolerance) -> GResult<Brep> {
    let finite = |v: &[f64]| v.iter().all(|x| x.is_finite());
    if !angle.is_finite() || !finite(axis_origin.coords.as_slice()) || !finite(axis_dir.as_slice()) {
        return invalid(format!("revolve: angle and axis must be finite, got {angle}, {axis_origin:?}, {axis_dir:?}"));
    }
    let len = axis_dir.norm();
    if len <= 0.0 || !len.is_finite() {
        return invalid("revolve: axis direction is zero".into());
    }
    if angle.abs() <= tol.angular || angle.abs() > TAU + tol.angular {
        return invalid(format!("revolve: angle must satisfy 0 < |angle| ≤ 2π, got {angle}"));
    }
    let full = angle.abs() >= TAU - tol.angular;
    let alpha = if full { TAU } else { angle.abs() };
    let region = profile.region(tol)?;
    let n = region.plane.z;
    let d = axis_dir / len;
    if d.dot(&n).abs() > tol.angular {
        return invalid(format!("revolve: axis direction is {:.3e} rad out of the profile plane", d.dot(&n).abs().asin()));
    }
    let off = (axis_origin - region.plane.origin).dot(&n);
    if off.abs() > tol.linear {
        return invalid(format!("revolve: axis origin is {:.3e} mm from the profile plane", off.abs()));
    }
    let z = (d - n * d.dot(&n)).normalize() * angle.signum();
    let o = axis_origin - n * off;
    let mut x = z.cross(&n).normalize();
    let mut loops = to_meridian(&region, &o, &x, &z);
    let (lo, hi) = rho_range(&loops);
    if lo < -tol.linear && hi > tol.linear {
        return invalid(format!("revolve: the profile crosses the axis (ρ from {lo:.6} to {hi:.6})"));
    }
    if hi <= tol.linear {
        x = -x;
        loops = to_meridian(&region, &o, &x, &z);
    }
    let rev = Rev { o, x, y: z.cross(&x), z, alpha };
    let loops = loops.into_iter().map(|l| MLoop::new(l, tol.linear)).collect::<GResult<Vec<_>>>()?;
    Ok(if full { build_full(&rev, &loops)? } else { build_partial(&rev, &loops) })
}

fn invalid<T>(m: String) -> GResult<T> {
    Err(GeomError::InvalidInput(m))
}

/// The revolve frame (see the module docs) and sweep angle α ∈ (0, 2π].
struct Rev {
    o: P3,
    x: V3,
    y: V3,
    z: V3,
    alpha: f64,
}

impl Rev {
    fn radial(&self, phi: f64) -> V3 {
        if phi == 0.0 { self.x } else { self.x * phi.cos() + self.y * phi.sin() }
    }

    /// World point of meridian coordinates `p` = (ρ, h) at sweep angle `phi`.
    fn world(&self, p: P2, phi: f64) -> P3 {
        self.o + self.radial(phi) * p.x + self.z * p.y
    }

    /// Frame on the axis at height `h` with the revolve axes (u of a revolved surface = φ).
    fn axial(&self, h: f64) -> Frame {
        Frame { origin: self.o + self.z * h, x: self.x, y: self.y, z: self.z }
    }

    /// Meridian plane at `phi`: local (x, y) = (ρ, h), normal radial × z = −∂φ direction.
    fn meridian(&self, phi: f64) -> Frame {
        let r = self.radial(phi);
        Frame { origin: self.o, x: r, y: self.z, z: r.cross(&self.z) }
    }
}

/// The region's loops in meridian coordinates (ρ along `x`, h along `z`), outer loop
/// counter-clockwise in (ρ, h).
fn to_meridian(region: &Region, o: &P3, x: &V3, z: &V3) -> Vec<Vec<Seg>> {
    let pl = &region.plane;
    let map = |p: P2| {
        let w = region.world(p, 0.0) - o;
        P2::new(w.dot(x), w.dot(z))
    };
    let dir = |a: f64| {
        let w = pl.x * a.cos() + pl.y * a.sin();
        w.dot(z).atan2(w.dot(x))
    };
    let flip = pl.x.dot(x) * pl.y.dot(z) - pl.y.dot(x) * pl.x.dot(z) < 0.0;
    region
        .loops()
        .map(|l| {
            let segs: Vec<Seg> = l
                .iter()
                .map(|s| match *s {
                    Seg::Line { a, b } => Seg::Line { a: map(a), b: map(b) },
                    Seg::Arc { c, r, a0, sweep } => Seg::Arc { c: map(c), r, a0: dir(a0), sweep: if flip { -sweep } else { sweep } },
                })
                .collect();
            if flip { segs.iter().rev().map(Seg::reversed).collect() } else { segs }
        })
        .collect()
}

/// Does the arc pass angle `ang` (endpoints included)?
fn arc_passes(a0: f64, sweep: f64, ang: f64) -> bool {
    let d = if sweep >= 0.0 { (ang - a0).rem_euclid(TAU) } else { (a0 - ang).rem_euclid(TAU) };
    d <= sweep.abs() || TAU - d <= 1e-15
}

/// Smallest and largest ρ of a segment (exact for arcs).
fn seg_rho_range(s: &Seg) -> (f64, f64) {
    let (p, q) = (s.start(), s.end());
    let (mut lo, mut hi) = (p.x.min(q.x), p.x.max(q.x));
    if let Seg::Arc { c, r, a0, sweep } = *s {
        if arc_passes(a0, sweep, 0.0) {
            hi = hi.max(c.x + r);
        }
        if arc_passes(a0, sweep, PI) {
            lo = lo.min(c.x - r);
        }
    }
    (lo, hi)
}

fn rho_range(loops: &[Vec<Seg>]) -> (f64, f64) {
    loops.iter().flatten().map(seg_rho_range).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (lo, hi)| (a.min(lo), b.max(hi)))
}

/// What a segment sweeps.
#[derive(Debug, Clone, Copy)]
enum Kind {
    /// On the axis: nothing.
    Axis,
    Plane { h: f64 },
    Cylinder { r: f64 },
    /// `base` = the end farther from the axis; tan α = dρ/dh.
    Cone { base: P2, half_angle: f64 },
    Sphere { c: P2, r: f64 },
    Torus { c: P2, r: f64 },
}

/// A segment in meridian coordinates with its swept surface and orientation.
struct MSeg {
    seg: Seg,
    kind: Kind,
    /// Outward normal against the surface's natural normal.
    reversed: bool,
}

impl MSeg {
    fn surface(&self, rev: &Rev) -> Surface {
        match self.kind {
            Kind::Axis => unreachable!("axis segments sweep no face"),
            Kind::Plane { h } => Surface::Plane { frame: rev.axial(h) },
            Kind::Cylinder { r } => Surface::Cylinder { frame: rev.axial(0.0), radius: r },
            Kind::Cone { base, half_angle } => Surface::Cone { frame: rev.axial(base.y), radius: base.x, half_angle },
            Kind::Sphere { c, r } => Surface::Sphere { frame: rev.axial(c.y), radius: r },
            Kind::Torus { c, r } => Surface::Torus { frame: rev.axial(c.y), major: c.x, minor: r },
        }
    }

    /// π∫ρ² dh along the segment: summed over a piece closed along the axis, its signed volume
    /// swept through 2π (positive when the material lies inside).
    fn volume_term(&self) -> f64 {
        match self.seg {
            Seg::Line { a, b } => PI * (b.y - a.y) * (a.x * a.x + a.x * b.x + b.x * b.x) / 3.0,
            Seg::Arc { c, r, a0, sweep } => {
                let f = |t: f64| {
                    let (s, co) = t.sin_cos();
                    c.x * c.x * r * s + c.x * r * r * (t + s * co) + r * r * r * (s - s * s * s / 3.0)
                };
                PI * (f(a0 + sweep) - f(a0))
            }
        }
    }
}

/// A loop in meridian coordinates: its vertices (segment starts, snapped onto the axis within
/// tolerance) and segments.
struct MLoop {
    verts: Vec<P2>,
    segs: Vec<MSeg>,
    /// A single full circle (no vertex in a full revolution).
    circle: bool,
}

impl MLoop {
    fn new(segs: Vec<Seg>, lin: f64) -> GResult<Self> {
        let snap = |p: P2| if p.x.abs() <= lin { P2::new(0.0, p.y) } else { p };
        let verts: Vec<P2> = segs.iter().map(|s| snap(s.start())).collect();
        let k = segs.len();
        let circle = k == 1 && segs[0].is_full_circle();
        let mut out = Vec::with_capacity(k);
        for (i, s) in segs.iter().enumerate() {
            let (p, q) = (verts[i], verts[(i + 1) % k]);
            let (seg, kind, reversed) = match *s {
                Seg::Line { .. } => {
                    let (dr, dh) = (q.x - p.x, q.y - p.y);
                    let kind = if p.x == 0.0 && q.x == 0.0 {
                        Kind::Axis
                    } else if dh.abs() <= lin {
                        Kind::Plane { h: 0.5 * (p.y + q.y) }
                    } else if dr.abs() <= lin {
                        Kind::Cylinder { r: 0.5 * (p.x + q.x) }
                    } else {
                        Kind::Cone { base: if p.x >= q.x { p } else { q }, half_angle: (dr / dh).atan() }
                    };
                    let reversed = if matches!(kind, Kind::Plane { .. }) { dr > 0.0 } else { dh < 0.0 };
                    (Seg::Line { a: p, b: q }, kind, reversed)
                }
                Seg::Arc { c, r, a0, sweep } => {
                    let c = snap(c);
                    let kind = if c.x == 0.0 {
                        Kind::Sphere { c, r }
                    } else {
                        if seg_rho_range(s).0 <= lin {
                            return Err(GeomError::Unsupported(format!(
                                "revolve: an arc centred off the axis (ρ = {:.6}, r = {r:.6}) reaches the axis (spindle / horn torus face)",
                                c.x
                            )));
                        }
                        Kind::Torus { c, r }
                    };
                    (Seg::Arc { c, r, a0, sweep }, kind, sweep < 0.0)
                }
            };
            out.push(MSeg { seg, kind, reversed });
        }
        Ok(Self { verts, segs: out, circle })
    }
}

/// One segment copy at a sweep angle, as an edge, plus whether the edge runs along the profile.
#[derive(Clone, Copy)]
struct SegEdge {
    edge: EdgeId,
    along: bool,
}

/// The copy of segment `s` in the meridian plane at `phi`; `ends` = (vertex at the segment start,
/// at its end), the same vertex for a full circle (closed edge at its parameter 0). Arcs run
/// counter-clockwise in (ρ, h) with parameter 0 at their counter-clockwise start.
fn seg_edge(b: &mut Brep, rev: &Rev, s: &Seg, phi: f64, ends: (VertexId, VertexId)) -> SegEdge {
    match *s {
        Seg::Line { .. } => SegEdge { edge: b.add_line_edge(ends.0, ends.1), along: true },
        Seg::Arc { c, r, a0, sweep } => {
            let m = rev.meridian(phi);
            let start = if sweep > 0.0 { a0 } else { a0 + sweep };
            let x = m.x * start.cos() + m.y * start.sin();
            let frame = Frame { origin: rev.world(c, phi), x, y: m.z.cross(&x), z: m.z };
            let ccw = sweep > 0.0;
            let edge = if s.is_full_circle() {
                b.add_edge(Some(Curve::Circle { frame, radius: r }), 0.0, TAU, ends.0, ends.0)
            } else {
                let (va, vb) = if ccw { ends } else { (ends.1, ends.0) };
                b.add_edge(Some(Curve::Circle { frame, radius: r }), 0.0, sweep.abs(), va, vb)
            };
            SegEdge { edge, along: ccw }
        }
    }
}

/// Partial revolution through α < 2π: one shell (the caps join every loop).
fn build_partial(rev: &Rev, loops: &[MLoop]) -> Brep {
    let mut b = Brep::new();
    let solid = b.add_solid();
    let shell = b.add_shell(Some(solid));
    struct Built {
        e0: Vec<SegEdge>,
        e1: Vec<SegEdge>,
        arcs: Vec<Option<EdgeId>>,
    }
    let mut built = Vec::with_capacity(loops.len());
    for l in loops {
        let k = l.segs.len();
        let mut v0 = Vec::with_capacity(k);
        let mut v1 = Vec::with_capacity(k);
        let mut arcs = Vec::with_capacity(k);
        for &p in &l.verts {
            let a = b.add_vertex(rev.world(p, 0.0));
            if p.x == 0.0 {
                v0.push(a);
                v1.push(a);
                arcs.push(None);
            } else {
                let e = b.add_vertex(rev.world(p, rev.alpha));
                v0.push(a);
                v1.push(e);
                // a lone full circle traces nothing: its torus face is bounded by the two copies
                arcs.push((!l.circle).then(|| b.add_edge(Some(Curve::Circle { frame: rev.axial(p.y), radius: p.x }), 0.0, rev.alpha, a, e)));
            }
        }
        let mut e0 = Vec::with_capacity(k);
        let mut e1 = Vec::with_capacity(k);
        for (i, s) in l.segs.iter().enumerate() {
            let j = (i + 1) % k;
            let first = seg_edge(&mut b, rev, &s.seg, 0.0, (v0[i], v0[j]));
            e0.push(first);
            e1.push(if matches!(s.kind, Kind::Axis) { first } else { seg_edge(&mut b, rev, &s.seg, rev.alpha, (v1[i], v1[j])) });
        }
        built.push(Built { e0, e1, arcs });
    }
    for (l, lb) in loops.iter().zip(&built) {
        let k = l.segs.len();
        for (i, s) in l.segs.iter().enumerate() {
            if matches!(s.kind, Kind::Axis) {
                continue;
            }
            let j = (i + 1) % k;
            let face = b.add_face(shell, s.surface(rev), s.reversed);
            if l.circle {
                // seamless torus band: two closed circles winding once round the v period
                b.add_loop(face, &[(lb.e1[i].edge, !lb.e1[i].along)]);
                b.add_loop(face, &[(lb.e0[i].edge, lb.e0[i].along)]);
                continue;
            }
            let mut uses = Vec::with_capacity(4);
            if let Some(a) = lb.arcs[i] {
                uses.push((a, false));
            }
            uses.push((lb.e1[i].edge, !lb.e1[i].along));
            if let Some(a) = lb.arcs[j] {
                uses.push((a, true));
            }
            uses.push((lb.e0[i].edge, lb.e0[i].along));
            b.add_loop(face, &uses);
        }
    }
    // caps: start cap along the profile, end cap against it (outer loop first, then holes)
    let start = b.add_face(shell, Surface::Plane { frame: rev.meridian(0.0) }, false);
    for lb in &built {
        let uses: Vec<(EdgeId, bool)> = lb.e0.iter().map(|e| (e.edge, !e.along)).collect();
        b.add_loop(start, &uses);
    }
    let end = b.add_face(shell, Surface::Plane { frame: rev.meridian(rev.alpha) }, true);
    for lb in &built {
        let uses: Vec<(EdgeId, bool)> = lb.e1.iter().rev().map(|e| (e.edge, e.along)).collect();
        b.add_loop(end, &uses);
    }
    b
}

/// Full revolution (seamless convention): the outer piece's shell, then one void shell per
/// other piece.
fn build_full(rev: &Rev, loops: &[MLoop]) -> GResult<Brep> {
    // pieces: maximal runs of non-axis segments joined at vertices off the axis
    let mut piece_of: Vec<Vec<Option<usize>>> = Vec::with_capacity(loops.len());
    let mut volume: Vec<f64> = Vec::new();
    for l in loops {
        let k = l.segs.len();
        let mut ids = vec![None; k];
        let first_on = (0..k).find(|&i| l.verts[i].x == 0.0 && !l.circle);
        let start = first_on.unwrap_or(0);
        let mut cur: Option<usize> = None;
        for step in 0..k {
            let i = (start + step) % k;
            if l.verts[i].x == 0.0 && !l.circle {
                cur = None;
            }
            if matches!(l.segs[i].kind, Kind::Axis) {
                continue;
            }
            let p = *cur.get_or_insert_with(|| {
                volume.push(0.0);
                volume.len() - 1
            });
            ids[i] = Some(p);
            volume[p] += l.segs[i].volume_term();
        }
        piece_of.push(ids);
    }
    let outer: Vec<usize> = (0..volume.len()).filter(|&p| volume[p] > 0.0).collect();
    if outer.len() != 1 {
        return Err(GeomError::Numerical(format!("revolve: {} pieces of positive volume (expected one outer boundary): {volume:?}", outer.len())));
    }
    let mut b = Brep::new();
    let solid = b.add_solid();
    let mut shells: Vec<Option<ShellId>> = vec![None; volume.len()];
    shells[outer[0]] = Some(b.add_shell(Some(solid)));
    for s in shells.iter_mut().filter(|s| s.is_none()) {
        *s = Some(b.add_shell(Some(solid)));
    }
    for (l, ids) in loops.iter().zip(&piece_of) {
        let k = l.segs.len();
        // one closed circle per vertex off the axis (none for a lone full circle)
        let circles: Vec<Option<EdgeId>> = l
            .verts
            .iter()
            .map(|&p| {
                (p.x != 0.0 && !l.circle).then(|| {
                    let v = b.add_vertex(rev.world(p, 0.0));
                    b.add_edge(Some(Curve::Circle { frame: rev.axial(p.y), radius: p.x }), 0.0, TAU, v, v)
                })
            })
            .collect();
        for (i, s) in l.segs.iter().enumerate() {
            let Some(p) = ids[i] else { continue };
            let face = b.add_face(shells[p].expect("every piece has a shell"), s.surface(rev), s.reversed);
            for (vi, reversed) in [(i, false), ((i + 1) % k, true)] {
                if let Some(c) = circles[vi] {
                    b.add_loop(face, &[(c, reversed)]);
                } else if matches!(s.kind, Kind::Cone { .. }) && !l.circle {
                    // apex: degenerate edge with its own vertex, like `make_cone`
                    let v = b.add_vertex(rev.world(l.verts[vi], 0.0));
                    let e = b.add_degenerate_edge(v, 0.0, TAU);
                    b.add_loop(face, &[(e, reversed)]);
                }
            }
        }
    }
    Ok(b)
}
