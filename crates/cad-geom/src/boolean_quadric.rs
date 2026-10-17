//! Regularised Boolean operations on solids bounded by planes, cylinders and spheres (the
//! *quadric pipeline*). [`crate::boolean::boolean`] dispatches here whenever an operand has a
//! cylinder or sphere face or a circle edge; pure polyhedra keep the polyhedral pipeline.
//!
//! Same structure as the polyhedral method (boundary evaluation: Requicha & Voelcker 1985;
//! Mäntylä 1988, ch. 15), with analytic curves throughout:
//! 1. **Extraction.** Each face becomes a canonical surface (plane, cylinder or sphere; faces whose
//!    sampled boundary lies within `tol` of an already registered surface share it, so
//!    coincidence is one decision) with an orientation sign (outward normal = sign × natural
//!    normal), and loops of *half-edges* `(curve, start, end, forward)` on canonical lines and
//!    circles, with all points welded in one pool. A full circle is a half-edge from a point to
//!    itself. ASM's seamless convention is kept: a cylinder band is two closed circles, a whole
//!    sphere has no loop.
//! 2. **Face–face intersection.** Surface pairs give canonical curves (plane–plane line;
//!    plane–cylinder lines or circle; plane–sphere circle; sphere–sphere circle; parallel
//!    cylinders lines; coaxial cylinder–sphere circles; others `Unsupported`), with tangency
//!    decided in distance space by [`crate::intersect`] (a 1e-5 mm overlap at ASM's tolerance is
//!    one tangent line). Each curve is cut at its crossings with both faces' boundaries
//!    (`curve_curve`); every span whose midpoint is in-or-on both faces becomes a cut segment of
//!    both. Faces on the same canonical surface exchange their boundaries.
//! 3. **Splitting**, per face, in 3D: on every curve of the face the covered intervals are split
//!    at the segment ends, at the curve's intersections with the face's other curves and at pool
//!    points lying on it; dangling edges are pruned; cycles are traced with the "first edge
//!    clockwise" rule, the angles being those of the 3D tangents in the face's oriented tangent
//!    plane, ties (tangent curves) broken by geodesic curvature. Cycles are grouped into pieces in
//!    a planar *chart* of the surface that preserves orientation and maps every edge to a
//!    segment or circular arc: the plane's frame; for a cylinder the annulus map
//!    (u, v) ↦ (v − v₀)(cos u, −sin u), which turns a seamless band into an annulus; for a
//!    sphere the stereographic projection from a pole chosen far from every curve (circles stay
//!    circles). Counter-clockwise cycles are outer loops, clockwise ones holes of the smallest
//!    enclosing outer loop; on a sphere the clockwise cycles enclosed by none bound the region
//!    containing the pole. A sample point is taken from exact scan lines across the arcs.
//! 4. **Classification** of each piece's sample point: *on* when inside a face of the other solid
//!    on the same canonical surface (same or opposite orientation); otherwise *in*/*out* by ray
//!    casting against the other solid's analytic faces (`intersect::line_surface`), counting
//!    signed crossings with the faces the hits fall in. A ray that grazes a surface, hits within
//!    `tol` of a face boundary or runs inside a surface is degenerate and replaced by the next of a
//!    fixed set of directions.
//! 5. **Selection** as in the polyhedral pipeline (`boolean::select`).
//! 6. **Sewing:** pool points lying on an edge are inserted into it; faces on the same canonical
//!    surface and orientation that share a two-use edge are merged and re-traced; faces are grouped
//!    into shells by connectivity; vertices of degree 2 between two edges of one canonical curve
//!    are removed (so a full circle ends with one vertex, as ASM's); each vertex is moved to the
//!    least-squares point of its faces' surfaces (Gauss–Newton, at most 4·tol; a contact line
//!    found within tolerance thus lies halfway between the plane and the cylinder); shells of
//!    negative volume become voids of the smallest enclosing outer shell. Result spheres carry a
//!    frame whose axis is the chart pole, whose two ends are far from every loop (the mass
//!    quadrature loses accuracy where a loop passes close to a parametric pole).
//!
//! **Line contact.** An edge with more than two uses (two solids touching along a line, ASM's
//! `bool.union.tangent_cylinder`) is kept as ONE edge in ONE shell, as ASM builds it, and listed in
//! [`Brep::nonmanifold_edges`] so that [`crate::validate`] accepts exactly that edge. Faces are not
//! merged across such an edge (ASM keeps the box face split along the contact line).
//!
//! **Tolerance.** `tol.linear` plus a rounding allowance of 1e-12 × model size: decimal inputs such
//! as 29.99999 are not exact in binary, and an overlap written as exactly the tolerance must
//! behave the same whichever way it rounds (ASM treats a 1e-5 mm overlap as contact).
//!
//! **Not supported** (`GeomError::Unsupported`): cone, torus and NURBS faces; ellipse and NURBS
//! edges; oblique plane–cylinder sections (ellipses); non-parallel cylinder–cylinder and
//! off-axis cylinder–sphere intersections, unless the surfaces are far apart.

use crate::boolean::{select, BoolOp, Class, Pool, Uf};
use crate::intersect::{curve_curve, line_surface, surface_surface, CurveCurveIntersection, CurveSurfaceIntersection, SurfaceIntersection};
use crate::{Brep, Curve, EdgeId, Frame, GResult, GeomError, SolidId, Surface, Tolerance, VertexId, P3, V3};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// Rounding allowance added to `tol.linear`, relative to the model size (see module docs).
const ROUNDING_REL: f64 = 1e-12;
/// Two tangent directions closer than this (radians) are a tie, broken by curvature.
const ANGLE_TIE: f64 = 1e-9;
/// A ray meeting a surface at |cos| below this is grazing (degenerate).
const GRAZING_COS: f64 = 1e-6;
/// Ray directions tried before classification gives up.
const RAY_TRIES: usize = 32;

type P2 = [f64; 2];
/// Undirected edge: (curve, start, end) with start → end along the curve's increasing parameter
/// (for a circle the counter-clockwise arc; start == end is the full circle).
type Key = (u32, u32, u32);

/// A directed use of an edge: from pool point `a` to `b` along curve `c`, `fwd` when that is the
/// curve's increasing parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct He {
    c: u32,
    a: u32,
    b: u32,
    fwd: bool,
}

impl He {
    fn rev(self) -> He {
        He { c: self.c, a: self.b, b: self.a, fwd: !self.fwd }
    }
    fn key(self) -> Key {
        if self.fwd { (self.c, self.a, self.b) } else { (self.c, self.b, self.a) }
    }
    fn of(k: Key, fwd: bool) -> He {
        if fwd { He { c: k.0, a: k.1, b: k.2, fwd } } else { He { c: k.0, a: k.2, b: k.1, fwd } }
    }
}

/// A face: canonical surface, outward normal = `sign` × natural normal, loops of half-edges
/// (outer first, counter-clockwise about the outward normal; none for a whole sphere).
#[derive(Debug, Clone)]
struct QFace {
    surf: usize,
    sign: f64,
    loops: Vec<Vec<He>>,
}

impl QFace {
    fn flip(&mut self) {
        self.sign = -self.sign;
        for l in &mut self.loops {
            l.reverse();
            l.iter_mut().for_each(|h| *h = h.rev());
        }
    }
    fn hes(&self) -> impl Iterator<Item = He> + '_ {
        self.loops.iter().flatten().copied()
    }
}

// ---------------------------------------------------------------------------------------------
// Planar charts and exact 2D regions bounded by segments and circular arcs

/// Orientation-preserving planar chart of a canonical surface (see module docs).
#[derive(Debug, Clone, Copy)]
enum Chart {
    Plane(Frame),
    Cyl { f: Frame, r: f64, v0: f64 },
    Sph { c: P3, r: f64, n: V3, e1: V3, e2: V3 },
}

impl Chart {
    /// Chart coordinates; `None` outside the chart (cylinder below v₀, the sphere's pole).
    fn to2(&self, p: &P3) -> Option<P2> {
        match self {
            Chart::Plane(f) => {
                let d = p - f.origin;
                Some([d.dot(&f.x), d.dot(&f.y)])
            }
            Chart::Cyl { f, v0, .. } => {
                let l = f.to_local(*p);
                let rho = l.z - v0;
                (rho > 0.0).then(|| {
                    let u = l.y.atan2(l.x);
                    [rho * u.cos(), -rho * u.sin()]
                })
            }
            Chart::Sph { c, r, n, e1, e2 } => {
                let w = (p - c).try_normalize(0.0)?;
                let s = 1.0 - w.dot(n);
                (s > 1e-300).then(|| [r * w.dot(e1) / s, r * w.dot(e2) / s])
            }
        }
    }

    fn from2(&self, q: P2) -> P3 {
        match self {
            Chart::Plane(f) => f.origin + f.x * q[0] + f.y * q[1],
            Chart::Cyl { f, r, v0 } => {
                let rho = q[0].hypot(q[1]);
                let u = (-q[1]).atan2(q[0]);
                f.origin + (f.x * u.cos() + f.y * u.sin()) * *r + f.z * (rho + v0)
            }
            Chart::Sph { c, r, n, e1, e2 } => {
                let (a, b) = (q[0] / r, q[1] / r);
                let s2 = a * a + b * b;
                c + (e1 * (2.0 * a) + e2 * (2.0 * b) + n * (s2 - 1.0)) * (*r / (s2 + 1.0))
            }
        }
    }
}

fn sub2(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}

fn cross2(a: P2, b: P2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// Circle through three points (computed relative to `a` for accuracy).
fn circumcircle(a: P2, b: P2, c: P2) -> Option<(P2, f64)> {
    let (b, c) = (sub2(b, a), sub2(c, a));
    let d = 2.0 * cross2(b, c);
    let scale = (b[0].abs() + b[1].abs()) * (c[0].abs() + c[1].abs());
    if d.abs() <= 1e-14 * scale {
        return None;
    }
    let (b2, c2) = (b[0] * b[0] + b[1] * b[1], c[0] * c[0] + c[1] * c[1]);
    let ux = (c[1] * b2 - b[1] * c2) / d;
    let uy = (b[0] * c2 - c[0] * b2) / d;
    Some(([a[0] + ux, a[1] + uy], ux.hypot(uy)))
}

/// A boundary piece in a chart: a segment, or the arc c + r(cos θ, sin θ) for θ from `a0` over the
/// signed sweep `sw`, with the exact images `s`, `e` of its end points.
#[derive(Debug, Clone, Copy)]
enum Prim {
    Seg(P2, P2),
    Arc { c: P2, r: f64, a0: f64, sw: f64, s: P2, e: P2 },
}

impl Prim {
    /// Contribution to the signed area, ½∮(x dy − y dx).
    fn area(&self) -> f64 {
        match *self {
            Prim::Seg(a, b) => 0.5 * cross2(a, b),
            Prim::Arc { c, r, a0, sw, .. } => {
                let a1 = a0 + sw;
                0.5 * (r * r * sw + r * c[0] * (a1.sin() - a0.sin()) - r * c[1] * (a1.cos() - a0.cos()))
            }
        }
    }

    /// Pieces monotone in y: (θa, θb, ya, yb) for arcs; a segment is one piece.
    fn arc_pieces(c: P2, r: f64, a0: f64, sw: f64, s: P2, e: P2) -> Vec<(f64, f64, f64, f64)> {
        let a1 = a0 + sw;
        let mut th = vec![a0];
        if sw > 0.0 {
            let mut k = ((a0 - FRAC_PI_2) / PI).floor();
            loop {
                let t = FRAC_PI_2 + k * PI;
                if t >= a1 {
                    break;
                }
                if t > a0 {
                    th.push(t);
                }
                k += 1.0;
            }
        } else {
            let mut k = ((a0 - FRAC_PI_2) / PI).ceil();
            loop {
                let t = FRAC_PI_2 + k * PI;
                if t <= a1 {
                    break;
                }
                if t < a0 {
                    th.push(t);
                }
                k -= 1.0;
            }
        }
        th.push(a1);
        let n = th.len();
        (0..n - 1)
            .map(|i| {
                let ya = if i == 0 { s[1] } else { c[1] + r * th[i].sin() };
                let yb = if i + 2 == n { e[1] } else { c[1] + r * th[i + 1].sin() };
                (th[i], th[i + 1], ya, yb)
            })
            .collect()
    }

    /// y values where the scan-line structure changes (ends, arc extremes).
    fn crit_ys(&self, out: &mut Vec<f64>) {
        match *self {
            Prim::Seg(a, b) => out.extend([a[1], b[1]]),
            Prim::Arc { c, r, a0, sw, s, e } => {
                for (_, _, ya, yb) in Self::arc_pieces(c, r, a0, sw, s, e) {
                    out.extend([ya, yb]);
                }
            }
        }
    }

    /// Crossings of the line Y = y: (x, +1 upward / −1 downward), half-open rule on each
    /// y-monotone piece.
    fn crossings(&self, y: f64, out: &mut Vec<(f64, i32)>) {
        match *self {
            Prim::Seg(a, b) => {
                if (a[1] > y) != (b[1] > y) {
                    out.push((a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1]), if b[1] > a[1] { 1 } else { -1 }));
                }
            }
            Prim::Arc { c, r, a0, sw, s, e } => {
                for (ta, tb, ya, yb) in Self::arc_pieces(c, r, a0, sw, s, e) {
                    if (ya > y) != (yb > y) {
                        out.push((arc_x(c, r, ta, tb, y), if yb > ya { 1 } else { -1 }));
                    }
                }
            }
        }
    }
}

/// x of the point at height y on the arc piece between angles ta and tb (y-monotone).
fn arc_x(c: P2, r: f64, ta: f64, tb: f64, y: f64) -> f64 {
    let (lo, hi) = (ta.min(tb), ta.max(tb));
    let mid = 0.5 * (lo + hi);
    let base = ((y - c[1]) / r).clamp(-1.0, 1.0).asin();
    let mut best = (f64::INFINITY, mid);
    for cand in [base, PI - base] {
        let t = cand + TAU * ((mid - cand) / TAU).round();
        let d = (lo - t).max(t - hi).max(0.0);
        if d < best.0 {
            best = (d, t.clamp(lo, hi));
        }
    }
    c[0] + r * best.1.cos()
}

fn region_area(loops: &[Vec<Prim>]) -> f64 {
    loops.iter().flatten().map(Prim::area).sum()
}

/// Winding number of the loops about `q` (crossings of the ray to +x).
fn winding(loops: &[Vec<Prim>], q: P2) -> i64 {
    let mut xs = Vec::new();
    for p in loops.iter().flatten() {
        p.crossings(q[1], &mut xs);
    }
    xs.iter().filter(|x| x.0 > q[0]).map(|x| x.1 as i64).sum()
}

/// A point well inside a bounded region: the midpoint of the widest span of the scan lines
/// halfway between consecutive critical ordinates (score = min(span width, slab height)).
fn sample_2d(loops: &[Vec<Prim>]) -> Option<P2> {
    let mut ys = Vec::new();
    for p in loops.iter().flatten() {
        p.crit_ys(&mut ys);
    }
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    let mut best: Option<(f64, P2)> = None;
    for w in ys.windows(2) {
        let (h, y) = (w[1] - w[0], 0.5 * (w[0] + w[1]));
        if h <= 0.0 {
            continue;
        }
        let mut xs = Vec::new();
        for p in loops.iter().flatten() {
            p.crossings(y, &mut xs);
        }
        let mut xs: Vec<f64> = xs.into_iter().map(|x| x.0).collect();
        xs.sort_by(f64::total_cmp);
        for c in xs.chunks_exact(2) {
            let score = (c[1] - c[0]).min(h);
            if best.is_none_or(|b| score > b.0) {
                best = Some((score, [0.5 * (c[0] + c[1]), y]));
            }
        }
    }
    best.filter(|b| b.0 > 0.0).map(|b| b.1)
}

/// A face's loops in its chart (orientation applied) and their total signed area (negative when
/// the face contains the chart's point at infinity: a sphere region around the pole).
#[derive(Debug, Clone, Default)]
struct FGeo {
    loops: Vec<Vec<Prim>>,
    area: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Loc {
    Inside,
    Boundary,
    Outside,
}

/// Point/normal helpers on canonical surfaces.
fn natural_normal(s: &Surface, p: &P3) -> V3 {
    match s {
        Surface::Plane { frame } => frame.z,
        Surface::Cylinder { frame, .. } => {
            let l = frame.to_local(*p);
            (frame.x * l.x + frame.y * l.y).try_normalize(0.0).unwrap_or(frame.x)
        }
        Surface::Sphere { frame, .. } => (p - frame.origin).try_normalize(0.0).unwrap_or(frame.z),
        _ => V3::z(),
    }
}

/// Signed distance to the surface (positive along the natural normal) and its gradient.
fn signed_distance(s: &Surface, p: &P3) -> (f64, V3) {
    match s {
        Surface::Plane { frame } => ((p - frame.origin).dot(&frame.z), frame.z),
        Surface::Cylinder { frame, radius } => {
            let l = frame.to_local(*p);
            (l.x.hypot(l.y) - radius, natural_normal(s, p))
        }
        Surface::Sphere { frame, radius } => ((p - frame.origin).norm() - radius, natural_normal(s, p)),
        _ => (0.0, V3::zeros()),
    }
}

fn boxes_overlap(a: &(P3, P3), b: &(P3, P3), tol: f64) -> bool {
    (0..3).all(|k| a.0[k] <= b.1[k] + tol && b.0[k] <= a.1[k] + tol)
}

/// Fixed, well-spread ray directions (golden-angle spiral, rotated off the coordinate axes).
fn ray_dirs() -> Vec<V3> {
    let ga = PI * (3.0 - 5f64.sqrt());
    (0..RAY_TRIES)
        .map(|i| {
            let z = 1.0 - (2.0 * i as f64 + 1.0) / RAY_TRIES as f64;
            let r = (1.0 - z * z).sqrt();
            let t = ga * i as f64 + 0.3141;
            V3::new(r * t.cos() + 0.0123, r * t.sin() - 0.0271, z + 0.0177).normalize()
        })
        .collect()
}

/// Largest absolute coordinate of a model (vertices, analytic surface extents).
fn model_size(b: &Brep) -> f64 {
    let mut s: f64 = 0.0;
    for v in &b.vertices {
        s = s.max(v.point.coords.amax());
    }
    for f in &b.faces {
        if let (Some(fr), r) = (f.surface.frame(), match &f.surface {
            Surface::Cylinder { radius, .. } | Surface::Sphere { radius, .. } => *radius,
            _ => 0.0,
        }) {
            s = s.max(fr.origin.coords.amax() + r);
        }
    }
    s
}

// ---------------------------------------------------------------------------------------------

/// Regularised `a op b` for solids bounded by planes, cylinders and spheres (see module docs).
pub(crate) fn boolean_quadric(a: &Brep, b: &Brep, op: BoolOp, tol: &Tolerance) -> GResult<Brep> {
    let size = model_size(a).max(model_size(b)).max(1.0);
    let lin = tol.linear + ROUNDING_REL * size;
    let mut cx = Ctx {
        pool: Pool::new(lin),
        surfs: Vec::new(),
        curves: Vec::new(),
        charts: Vec::new(),
        ssx_cache: HashMap::new(),
        lin,
        size,
        tol: Tolerance { linear: lin, angular: tol.angular },
    };
    let fa = cx.extract(a, "first operand")?;
    let fb = cx.extract(b, "second operand")?;
    // phase 1: charts from the operands' own boundaries (for in-face tests while clipping)
    cx.make_charts(&[(&fa, &[]), (&fb, &[])]);
    let (ga, gb) = (cx.geos(&fa)?, cx.geos(&fb)?);
    let (ba, bb): (Vec<_>, Vec<_>) = (fa.iter().map(|f| cx.face_box(f)).collect(), fb.iter().map(|f| cx.face_box(f)).collect());
    let (mut sa, mut sb) = (vec![Vec::new(); fa.len()], vec![Vec::new(); fb.len()]);
    for i in 0..fa.len() {
        for j in 0..fb.len() {
            if boxes_overlap(&ba[i], &bb[j], lin) {
                cx.intersect_pair(&fa[i], &fb[j], &ga[i], &gb[j], &mut sa[i], &mut sb[j])?;
            }
        }
    }
    // phase 2: charts that also avoid the cut curves (sphere poles), used from here on
    let segs_a: Vec<&[Key]> = sa.iter().map(|v| v.as_slice()).collect();
    let segs_b: Vec<&[Key]> = sb.iter().map(|v| v.as_slice()).collect();
    cx.make_charts(&[(&fa, &segs_a), (&fb, &segs_b)]);
    let (ga, gb) = (cx.geos(&fa)?, cx.geos(&fb)?);
    let mut relevant: BTreeSet<u32> = BTreeSet::new();
    for f in fa.iter().chain(&fb) {
        relevant.extend(f.hes().map(|h| h.a));
    }
    for k in sa.iter().chain(&sb).flatten() {
        relevant.extend([k.1, k.2]);
    }
    let relevant: Vec<u32> = relevant.into_iter().collect();
    let mut kept = Vec::new();
    for (from_a, faces, geos, segs, other, ogeos) in [(true, &fa, &ga, &sa, &fb, &gb), (false, &fb, &gb, &sb, &fa, &ga)] {
        for ((f, g), s) in faces.iter().zip(geos).zip(segs) {
            for (loops, q) in cx.split(f, g, s, &relevant)? {
                let class = cx.classify(&q, f, other, ogeos)?;
                if let Some(flip) = select(op, from_a, class) {
                    let mut piece = QFace { surf: f.surf, sign: f.sign, loops };
                    if flip {
                        piece.flip();
                    }
                    kept.push(piece);
                }
            }
        }
    }
    cx.sew(kept)
}

struct Ctx {
    pool: Pool,
    surfs: Vec<Surface>,
    curves: Vec<Curve>,
    charts: Vec<Chart>,
    ssx_cache: HashMap<(usize, usize), Vec<u32>>,
    lin: f64,
    size: f64,
    tol: Tolerance,
}

impl Ctx {
    fn pt(&self, id: u32) -> P3 {
        self.pool.pts[id as usize]
    }

    // ----- registries -----

    /// Canonical surface of a face: (index, sign of the face's outward normal against the
    /// canonical natural normal).
    fn surf_of(&mut self, s: &Surface, reversed: bool, pts: &[P3]) -> GResult<(usize, f64)> {
        let rs = if reversed { -1.0 } else { 1.0 };
        let lin = self.lin;
        let near = |c: &Surface| pts.iter().all(|p| crate::param::surface_distance(c, p).is_some_and(|d| d <= lin));
        for (k, c) in self.surfs.iter().enumerate() {
            match (s, c) {
                (Surface::Plane { frame }, Surface::Plane { frame: cf }) => {
                    let d = cf.z.dot(&frame.z);
                    if d.abs() > 0.5 && near(c) {
                        return Ok((k, rs * d.signum()));
                    }
                }
                (Surface::Cylinder { frame, radius }, Surface::Cylinder { frame: cf, radius: cr }) => {
                    let axis_off = {
                        let l = cf.to_local(frame.origin);
                        l.x.hypot(l.y)
                    };
                    if (radius - cr).abs() <= lin && frame.z.cross(&cf.z).norm() * self.size <= lin && axis_off <= lin && near(c) {
                        return Ok((k, rs));
                    }
                }
                (Surface::Sphere { frame, radius }, Surface::Sphere { frame: cf, radius: cr }) if (radius - cr).abs() <= lin && (frame.origin - cf.origin).norm() <= lin => {
                    return Ok((k, rs));
                }
                _ => {}
            }
        }
        self.surfs.push(s.clone());
        Ok((self.surfs.len() - 1, rs))
    }

    /// Canonical curve id (lines and circles only).
    fn curve_id(&mut self, c: &Curve) -> GResult<u32> {
        let lin = self.lin;
        match c {
            Curve::Line { origin, dir } => {
                let d = dir.normalize();
                let probes = [*origin, origin + d * self.size, origin - d * self.size];
                for (k, cc) in self.curves.iter().enumerate() {
                    if let Curve::Line { origin: o2, dir: d2 } = cc
                        && probes.iter().all(|p| (p - o2 - d2 * (p - o2).dot(d2)).norm() <= lin)
                    {
                        return Ok(k as u32);
                    }
                }
                self.curves.push(Curve::Line { origin: *origin, dir: d });
            }
            Curve::Circle { frame, radius } => {
                for (k, cc) in self.curves.iter().enumerate() {
                    if let Curve::Circle { frame: f2, radius: r2 } = cc
                        && (frame.origin - f2.origin).norm() <= lin
                        && (radius - r2).abs() <= lin
                        && frame.z.cross(&f2.z).norm() * radius <= lin
                    {
                        return Ok(k as u32);
                    }
                }
                self.curves.push(c.clone());
            }
            other => return Err(GeomError::Unsupported(format!("boolean: {} curves (only lines and circles are supported)", other.type_name()))),
        }
        Ok(self.curves.len() as u32 - 1)
    }

    fn radius(&self, c: u32) -> f64 {
        match &self.curves[c as usize] {
            Curve::Circle { radius, .. } => *radius,
            _ => 0.0,
        }
    }

    fn is_circle(&self, c: u32) -> bool {
        matches!(self.curves[c as usize], Curve::Circle { .. })
    }

    /// Parameter of a point on a canonical curve (circle: angle in [0, 2π)).
    fn param(&self, c: u32, p: &P3) -> f64 {
        match &self.curves[c as usize] {
            Curve::Line { origin, dir } => (p - origin).dot(dir),
            Curve::Circle { frame, .. } => {
                let l = frame.to_local(*p);
                l.y.atan2(l.x).rem_euclid(TAU)
            }
            _ => 0.0,
        }
    }

    /// Parameter interval of an edge: (t_start, t_end), t_end > t_start (circles unwrapped).
    fn interval(&self, k: Key) -> (f64, f64) {
        let t0 = self.param(k.0, &self.pt(k.1));
        if self.is_circle(k.0) {
            let sw = if k.1 == k.2 { TAU } else { (self.param(k.0, &self.pt(k.2)) - t0).rem_euclid(TAU) };
            (t0, t0 + sw)
        } else {
            (t0, self.param(k.0, &self.pt(k.2)))
        }
    }

    /// Unwrapped parameter of `p` on the edge's interval, allowing `slack` (length) beyond its
    /// ends; `None` when outside.
    fn param_in(&self, k: Key, p: &P3, slack: f64) -> Option<f64> {
        let (t0, t1) = self.interval(k);
        let t = self.param(k.0, p);
        if self.is_circle(k.0) {
            let r = self.radius(k.0);
            let d = (t - t0).rem_euclid(TAU);
            if d <= t1 - t0 + slack / r {
                Some(t0 + d)
            } else if (TAU - d) * r <= slack {
                Some(t0 + d - TAU)
            } else {
                None
            }
        } else {
            (t >= t0 - slack && t <= t1 + slack).then_some(t)
        }
    }

    /// Distance from `p` to the (unbounded) curve.
    fn curve_dist(&self, c: u32, p: &P3) -> f64 {
        match &self.curves[c as usize] {
            Curve::Line { origin, dir } => {
                let w = p - origin;
                (w - dir * w.dot(dir)).norm()
            }
            Curve::Circle { frame, radius } => {
                let l = frame.to_local(*p);
                (l.x.hypot(l.y) - radius).hypot(l.z)
            }
            _ => f64::INFINITY,
        }
    }

    /// Distance from `p` to the edge (curve restricted to its interval).
    fn key_dist(&self, k: Key, p: &P3) -> f64 {
        let ends = (self.pt(k.1) - p).norm().min((self.pt(k.2) - p).norm());
        let c = &self.curves[k.0 as usize];
        let t = match c {
            Curve::Line { .. } => {
                let (t0, t1) = self.interval(k);
                self.param(k.0, p).clamp(t0, t1)
            }
            _ => match self.param_in(k, p, 0.0) {
                Some(t) => t,
                None => return ends,
            },
        };
        (c.point(t) - p).norm().min(ends)
    }

    // ----- extraction -----

    fn extract(&mut self, b: &Brep, what: &str) -> GResult<Vec<QFace>> {
        let in_solid: usize = b.solids.iter().map(|s| s.shells.iter().map(|&sh| b.shell(sh).faces.len()).sum::<usize>()).sum();
        if in_solid != b.faces.len() {
            return Err(GeomError::InvalidInput(format!("boolean: {what} has faces outside any solid (open shell)")));
        }
        let mut cache: HashMap<EdgeId, u32> = HashMap::new();
        let mut out = Vec::new();
        for s in b.solid_ids() {
            for f in b.solid_faces(s) {
                let face = b.face(f);
                if !matches!(face.surface, Surface::Plane { .. } | Surface::Cylinder { .. } | Surface::Sphere { .. }) {
                    return Err(GeomError::Unsupported(format!(
                        "boolean: {what} face F{} is a {} (planes, cylinders and spheres are supported)",
                        f.0,
                        face.surface.type_name()
                    )));
                }
                let mut loops = Vec::new();
                let mut samples = Vec::new();
                for &l in &face.loops {
                    let mut hes = Vec::new();
                    for &c in &b.lp(l).coedges {
                        let ce = b.coedge(c);
                        let e = b.edge(ce.edge);
                        let Some(curve) = &e.curve else {
                            return Err(GeomError::Unsupported(format!("boolean: {what} face F{} has a degenerate edge", f.0)));
                        };
                        let cid = match cache.get(&ce.edge) {
                            Some(&x) => x,
                            None => {
                                let x = self.curve_id(curve)?;
                                cache.insert(ce.edge, x);
                                x
                            }
                        };
                        let same = match (curve, &self.curves[cid as usize]) {
                            (Curve::Line { dir, .. }, Curve::Line { dir: d2, .. }) => dir.dot(d2) > 0.0,
                            (Curve::Circle { frame, .. }, Curve::Circle { frame: f2, .. }) => frame.z.dot(&f2.z) > 0.0,
                            _ => true,
                        };
                        let (ps, pe) = b.coedge_points(c);
                        samples.push(ps);
                        for k in 1..8 {
                            samples.push(curve.point(e.t0 + (e.t1 - e.t0) * k as f64 / 8.0));
                        }
                        let (ia, ib) = (self.pool.id(ps), self.pool.id(pe));
                        if ia == ib {
                            // a closed circle, or an edge shorter than the tolerance (dropped)
                            let len = match curve {
                                Curve::Circle { radius, .. } => radius * (e.t1 - e.t0),
                                _ => 0.0,
                            };
                            if len <= 2.0 * self.lin {
                                continue;
                            }
                        }
                        hes.push(He { c: cid, a: ia, b: ib, fwd: same != ce.reversed });
                    }
                    if hes.is_empty() {
                        return Err(GeomError::InvalidInput(format!("boolean: {what} face F{} has a loop shorter than the tolerance", f.0)));
                    }
                    loops.push(hes);
                }
                let (surf, sign) = self.surf_of(&face.surface, face.reversed, &samples)?;
                if loops.is_empty() && !matches!(self.surfs[surf], Surface::Sphere { .. }) {
                    return Err(GeomError::InvalidInput(format!("boolean: {what} face F{} has no loop", f.0)));
                }
                out.push(QFace { surf, sign, loops });
            }
        }
        Ok(out)
    }

    // ----- charts -----

    /// Builds one chart per canonical surface from the curves and points of the given faces and
    /// their cut segments (cylinder: v range; sphere: pole far from every circle).
    fn make_charts(&mut self, sets: &[(&[QFace], &[&[Key]])]) {
        let n = self.surfs.len();
        let mut curves: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
        let mut pts: Vec<Vec<u32>> = vec![Vec::new(); n];
        for (faces, segs) in sets {
            for (i, f) in faces.iter().enumerate() {
                for h in f.hes() {
                    curves[f.surf].insert(h.c);
                    pts[f.surf].push(h.a);
                }
                for k in segs.get(i).copied().unwrap_or(&[]) {
                    curves[f.surf].insert(k.0);
                    pts[f.surf].extend([k.1, k.2]);
                }
            }
        }
        let charts = (0..n)
            .map(|s| match &self.surfs[s] {
                Surface::Cylinder { frame, radius } => {
                    let vs = pts[s].iter().map(|&i| frame.to_local(self.pt(i)).z).chain(curves[s].iter().filter_map(|&c| match &self.curves[c as usize] {
                        Curve::Circle { frame: cf, .. } => Some(frame.to_local(cf.origin).z),
                        _ => None,
                    }));
                    let (lo, hi) = vs.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
                    let (lo, hi) = if lo <= hi { (lo, hi) } else { (0.0, 0.0) };
                    Chart::Cyl { f: *frame, r: *radius, v0: lo - (radius + (hi - lo)) }
                }
                Surface::Sphere { frame, radius } => {
                    let n = self.choose_pole(&frame.origin, *radius, &curves[s]);
                    let e1 = Frame::from_origin_z(P3::origin(), n).x;
                    Chart::Sph { c: frame.origin, r: *radius, n, e1, e2: e1.cross(&n) }
                }
                Surface::Plane { frame } => Chart::Plane(*frame),
                _ => Chart::Plane(Frame::world()),
            })
            .collect();
        self.charts = charts;
    }

    /// The axis direction (from a fixed well-spread set) whose two ends are farthest in angle from
    /// every circle on the sphere: the stereographic pole, and the frame axis of result spheres
    /// (so no loop runs near a parametric pole, where the mass quadrature loses accuracy).
    fn choose_pole(&self, c: &P3, r: f64, curves: &BTreeSet<u32>) -> V3 {
        let circles: Vec<(V3, f64)> = curves
            .iter()
            .filter_map(|&k| match &self.curves[k as usize] {
                Curve::Circle { frame, radius } => {
                    let off = frame.origin - c;
                    let axis = if off.norm() > 1e-9 * r { off.normalize() } else { frame.z };
                    Some((axis, radius.atan2(off.norm())))
                }
                _ => None,
            })
            .collect();
        let n = 256;
        let ga = PI * (3.0 - 5f64.sqrt());
        let mut best = (f64::NEG_INFINITY, V3::new(0.267, 0.534, 0.802).normalize());
        for i in 0..n {
            let z = 1.0 - (2.0 * i as f64 + 1.0) / n as f64;
            let rr = (1.0 - z * z).sqrt();
            let t = ga * i as f64 + 0.123;
            let d = V3::new(rr * t.cos(), rr * t.sin(), z);
            let m = circles
                .iter()
                .map(|(a, al)| {
                    let t = d.dot(a).clamp(-1.0, 1.0).acos();
                    (t - al).abs().min((PI - t - al).abs())
                })
                .fold(f64::INFINITY, f64::min);
            if m > best.0 {
                best = (m, d);
            }
        }
        best.1
    }

    fn to2f(&self, surf: usize, sign: f64, p: &P3) -> Option<P2> {
        self.charts[surf].to2(p).map(|q| [q[0], sign * q[1]])
    }

    fn from2f(&self, surf: usize, sign: f64, q: P2) -> P3 {
        self.charts[surf].from2([q[0], sign * q[1]])
    }

    fn pole_point(&self, surf: usize) -> Option<P3> {
        match self.charts[surf] {
            Chart::Sph { c, r, n, .. } => Some(c + n * r),
            _ => None,
        }
    }

    /// Signed sweep (in the canonical parameter) of a half-edge, starting at its start point.
    fn sweep(&self, h: He) -> (f64, f64) {
        let t0 = self.param(h.c, &self.pt(h.a));
        if h.a == h.b {
            return (t0, if h.fwd { TAU } else { -TAU });
        }
        let t1 = self.param(h.c, &self.pt(h.b));
        (t0, if h.fwd { (t1 - t0).rem_euclid(TAU) } else { -(t0 - t1).rem_euclid(TAU) })
    }

    /// The half-edge in the chart of (surf, sign).
    fn prim(&self, h: He, surf: usize, sign: f64) -> GResult<Prim> {
        let bad = || GeomError::Numerical("boolean: point outside the surface chart".into());
        let to = |p: &P3| self.to2f(surf, sign, p).ok_or_else(bad);
        let (s, e) = (to(&self.pt(h.a))?, to(&self.pt(h.b))?);
        let curve = &self.curves[h.c as usize];
        match curve {
            Curve::Line { .. } => {
                if matches!(self.charts[surf], Chart::Sph { .. }) {
                    return Err(GeomError::Numerical("boolean: a line edge on a sphere".into()));
                }
                Ok(Prim::Seg(s, e))
            }
            _ => {
                // image circle and its orientation from three well-spread points of the full circle
                let q: Vec<P2> = (0..3).map(|i| to(&curve.point(TAU * i as f64 / 3.0))).collect::<GResult<_>>()?;
                let (c, r) = circumcircle(q[0], q[1], q[2]).ok_or_else(bad)?;
                let dcan = cross2(sub2(q[1], q[0]), sub2(q[2], q[0])).signum();
                let dir = if h.fwd { dcan } else { -dcan };
                let a0 = (s[1] - c[1]).atan2(s[0] - c[0]);
                let a1 = (e[1] - c[1]).atan2(e[0] - c[0]);
                let sw = if h.a == h.b {
                    dir * TAU
                } else if dir > 0.0 {
                    (a1 - a0).rem_euclid(TAU)
                } else {
                    -(a0 - a1).rem_euclid(TAU)
                };
                Ok(Prim::Arc { c, r, a0, sw, s, e })
            }
        }
    }

    fn loop_prims(&self, l: &[He], surf: usize, sign: f64) -> GResult<Vec<Prim>> {
        l.iter().map(|&h| self.prim(h, surf, sign)).collect()
    }

    fn geo(&self, f: &QFace) -> GResult<FGeo> {
        let loops: Vec<Vec<Prim>> = f.loops.iter().map(|l| self.loop_prims(l, f.surf, f.sign)).collect::<GResult<_>>()?;
        let area = region_area(&loops);
        Ok(FGeo { loops, area })
    }

    fn geos(&self, faces: &[QFace]) -> GResult<Vec<FGeo>> {
        faces.iter().map(|f| self.geo(f)).collect()
    }

    /// Strict point-in-face test in the chart (no tolerance).
    fn inside_strict(&self, f: &QFace, g: &FGeo, p: &P3) -> bool {
        if f.loops.is_empty() {
            return true;
        }
        let contains_inf = g.area < 0.0;
        match self.to2f(f.surf, f.sign, p) {
            Some(q) => winding(&g.loops, q) + i64::from(contains_inf) == 1,
            None => matches!(self.charts[f.surf], Chart::Sph { .. }) && contains_inf,
        }
    }

    /// In / on (within tol of a boundary edge, in 3D) / out, for a point on the face's surface.
    fn locate(&self, f: &QFace, g: &FGeo, p: &P3) -> Loc {
        if f.hes().any(|h| self.key_dist(h.key(), p) <= self.lin) {
            Loc::Boundary
        } else if self.inside_strict(f, g, p) {
            Loc::Inside
        } else {
            Loc::Outside
        }
    }

    /// A point inside the face (the pole for a sphere region containing it).
    fn face_sample(&self, f: &QFace, g: &FGeo) -> GResult<P3> {
        if f.loops.is_empty() || g.area < 0.0 {
            return self.pole_point(f.surf).ok_or_else(|| GeomError::Numerical("boolean: unbounded face off a sphere".into()));
        }
        let q = sample_2d(&g.loops).ok_or_else(|| GeomError::Numerical("boolean: degenerate face (no interior point)".into()))?;
        Ok(self.from2f(f.surf, f.sign, q))
    }

    fn face_box(&self, f: &QFace) -> (P3, P3) {
        if let Surface::Sphere { frame, radius } = &self.surfs[f.surf] {
            let r = V3::repeat(*radius);
            return (frame.origin - r, frame.origin + r);
        }
        let mut lo = P3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut hi = -lo;
        let mut add = |a: P3, b: P3| {
            lo = lo.inf(&a);
            hi = hi.sup(&b);
        };
        for h in f.hes() {
            match &self.curves[h.c as usize] {
                Curve::Circle { frame, radius } => {
                    // bounds of the whole circle
                    let ext = V3::from_fn(|i, _| radius * (1.0 - frame.z[i] * frame.z[i]).max(0.0).sqrt());
                    add(frame.origin - ext, frame.origin + ext);
                }
                _ => {
                    let p = self.pt(h.a);
                    add(p, p);
                }
            }
        }
        (lo, hi)
    }

    // ----- intersection -----

    /// Canonical intersection curves of two canonical surfaces (cached).
    fn ssx(&mut self, i: usize, j: usize) -> GResult<Vec<u32>> {
        let key = (i.min(j), i.max(j));
        if let Some(v) = self.ssx_cache.get(&key) {
            return Ok(v.clone());
        }
        let (a, b) = (self.surfs[key.0].clone(), self.surfs[key.1].clone());
        let lin = self.lin;
        let curves: Vec<Curve> = match (&a, &b) {
            (Surface::Plane { .. }, _) | (_, Surface::Plane { .. }) => match surface_surface(&a, &b, &self.tol)? {
                SurfaceIntersection::Curves { curves, .. } => {
                    if let Some(c) = curves.iter().find(|c| !matches!(c, Curve::Line { .. } | Curve::Circle { .. })) {
                        return Err(GeomError::Unsupported(format!(
                            "boolean: {}–{} intersection is a {} (oblique plane sections of cylinders are not supported)",
                            a.type_name(),
                            b.type_name(),
                            c.type_name()
                        )));
                    }
                    curves
                }
                _ => Vec::new(),
            },
            (Surface::Sphere { frame: f1, radius: r1 }, Surface::Sphere { frame: f2, radius: r2 }) => sphere_sphere(&f1.origin, *r1, &f2.origin, *r2, lin),
            (Surface::Cylinder { frame: f1, radius: r1 }, Surface::Cylinder { frame: f2, radius: r2 }) => cyl_cyl(f1, *r1, f2, *r2, lin, self.tol.angular)?,
            (Surface::Cylinder { frame, radius }, Surface::Sphere { frame: sf, radius: sr }) | (Surface::Sphere { frame: sf, radius: sr }, Surface::Cylinder { frame, radius }) => {
                cyl_sphere(frame, *radius, &sf.origin, *sr, lin)?
            }
            _ => return Err(GeomError::Unsupported(format!("boolean: {}–{} intersection", a.type_name(), b.type_name()))),
        };
        let ids = curves.iter().map(|c| self.curve_id(c)).collect::<GResult<Vec<_>>>()?;
        self.ssx_cache.insert(key, ids.clone());
        Ok(ids)
    }

    /// Cut segments between two faces, appended to both faces' lists.
    fn intersect_pair(&mut self, fa: &QFace, fb: &QFace, ga: &FGeo, gb: &FGeo, sa: &mut Vec<Key>, sb: &mut Vec<Key>) -> GResult<()> {
        if fa.surf == fb.surf {
            sa.extend(fb.hes().map(He::key));
            sb.extend(fa.hes().map(He::key));
            return Ok(());
        }
        for c in self.ssx(fa.surf, fb.surf)? {
            for (t0, t1) in self.clip(c, fa, fb, ga, gb)? {
                let curve = self.curves[c as usize].clone();
                let i0 = self.pool.id(curve.point(t0));
                let full = self.is_circle(c) && t1 - t0 >= TAU - 1e-12;
                let k = if full {
                    (c, i0, i0)
                } else {
                    let i1 = self.pool.id(curve.point(t1));
                    if i0 == i1 {
                        continue;
                    }
                    (c, i0, i1)
                };
                sa.push(k);
                sb.push(k);
            }
        }
        Ok(())
    }

    /// Parameter spans of curve `c` lying in-or-on both faces (circle spans may exceed 2π at
    /// their end; a full circle is (t, t + 2π)).
    fn clip(&mut self, c: u32, fa: &QFace, fb: &QFace, ga: &FGeo, gb: &FGeo) -> GResult<Vec<(f64, f64)>> {
        let curve = self.curves[c as usize].clone();
        let circle = self.is_circle(c);
        let mut bp: Vec<f64> = Vec::new();
        for f in [fa, fb] {
            for h in f.hes() {
                let k = h.key();
                if h.c == c {
                    bp.extend([self.param(c, &self.pt(k.1)), self.param(c, &self.pt(k.2))]);
                    continue;
                }
                match curve_curve(&curve, &self.curves[h.c as usize], &self.tol)? {
                    CurveCurveIntersection::Points(hits) => {
                        for x in hits {
                            if self.param_in(k, &x.point, self.lin).is_some() {
                                bp.push(self.param(c, &x.point));
                            }
                        }
                    }
                    CurveCurveIntersection::Coincident => bp.extend([self.param(c, &self.pt(k.1)), self.param(c, &self.pt(k.2))]),
                    CurveCurveIntersection::None => {}
                }
            }
        }
        let scale = if circle { self.radius(c) } else { 1.0 };
        bp.sort_by(f64::total_cmp);
        bp.dedup_by(|b, a| (*b - *a) * scale <= self.lin);
        if circle && bp.len() > 1 && (bp[0] + TAU - bp[bp.len() - 1]) * scale <= self.lin {
            bp.pop();
        }
        let both = |cx: &Self, t: f64| {
            let p = curve.point(t);
            cx.locate(fa, ga, &p) != Loc::Outside && cx.locate(fb, gb, &p) != Loc::Outside
        };
        let mut spans: Vec<(f64, f64, bool)> = Vec::new();
        if circle {
            if bp.is_empty() {
                return Ok(if both(self, 0.0) { vec![(0.0, TAU)] } else { Vec::new() });
            }
            let mut cyc = Vec::new();
            for i in 0..bp.len() {
                let (t0, t1) = (bp[i], if i + 1 < bp.len() { bp[i + 1] } else { bp[0] + TAU });
                cyc.push((t0, t1, both(self, 0.5 * (t0 + t1))));
            }
            if cyc.iter().all(|s| s.2) {
                return Ok(vec![(bp[0], bp[0] + TAU)]);
            }
            // start after a rejected span (unwrapping by 2π) so kept runs do not straddle the start
            let s = cyc.iter().position(|s| !s.2).expect("some span rejected") + 1;
            let n = cyc.len();
            for k in 0..n {
                let i = (s + k) % n;
                let off = if i < s { TAU } else { 0.0 };
                spans.push((cyc[i].0 + off, cyc[i].1 + off, cyc[i].2));
            }
        } else {
            for w in bp.windows(2) {
                if w[1] - w[0] > self.lin {
                    spans.push((w[0], w[1], both(self, 0.5 * (w[0] + w[1]))));
                }
            }
        }
        let mut out: Vec<(f64, f64)> = Vec::new();
        for (t0, t1, keep) in spans {
            if !keep {
                continue;
            }
            match out.last_mut() {
                Some(last) if (t0 - last.1).abs() * scale <= self.lin => last.1 = t1,
                _ => out.push((t0, t1)),
            }
        }
        Ok(out.into_iter().filter(|s| (s.1 - s.0) * scale > self.lin).collect())
    }

    // ----- splitting -----

    /// Splits a face by its cut segments: pieces as (loops, sample point inside the piece).
    fn split(&mut self, f: &QFace, g0: &FGeo, extra: &[Key], relevant: &[u32]) -> GResult<Vec<(Vec<Vec<He>>, P3)>> {
        let own: BTreeSet<Key> = f.hes().map(He::key).collect();
        if extra.iter().all(|k| own.contains(k)) {
            return Ok(vec![(f.loops.clone(), self.face_sample(f, g0)?)]);
        }
        let mut by_curve: BTreeMap<u32, Vec<Key>> = BTreeMap::new();
        for &k in own.iter().chain(extra) {
            let v = by_curve.entry(k.0).or_default();
            if !v.contains(&k) {
                v.push(k);
            }
        }
        let curves: Vec<u32> = by_curve.keys().copied().collect();
        let mut splits: BTreeMap<u32, BTreeSet<u32>> = curves.iter().map(|&c| (c, BTreeSet::new())).collect();
        for (c, ks) in &by_curve {
            for k in ks {
                splits.get_mut(c).unwrap().extend([k.1, k.2]);
            }
        }
        // crossings between the face's curves
        for i in 0..curves.len() {
            for j in i + 1..curves.len() {
                let (ci, cj) = (curves[i], curves[j]);
                let hits = match curve_curve(&self.curves[ci as usize], &self.curves[cj as usize], &self.tol)? {
                    CurveCurveIntersection::Points(h) => h,
                    _ => continue,
                };
                for x in hits {
                    let on = |cx: &Self, c: u32| by_curve[&c].iter().any(|&k| cx.param_in(k, &x.point, cx.lin).is_some());
                    if on(self, ci) && on(self, cj) {
                        let id = self.pool.id(x.point);
                        splits.get_mut(&ci).unwrap().insert(id);
                        splits.get_mut(&cj).unwrap().insert(id);
                    }
                }
            }
        }
        // pool points lying on a covered interval (T-junctions with the rest of the model)
        for &c in &curves {
            for &v in relevant {
                let p = self.pt(v);
                if self.curve_dist(c, &p) <= self.lin && by_curve[&c].iter().any(|&k| self.param_in(k, &p, self.lin).is_some()) {
                    splits.get_mut(&c).unwrap().insert(v);
                }
            }
        }
        // elementary edges
        let mut edges: BTreeSet<Key> = BTreeSet::new();
        for &c in &curves {
            let covered = |cx: &Self, t: f64| {
                by_curve[&c].iter().any(|&k| {
                    let (t0, t1) = cx.interval(k);
                    if cx.is_circle(c) { (t - t0).rem_euclid(TAU) < t1 - t0 } else { t > t0 && t < t1 }
                })
            };
            let mut ids: Vec<(f64, u32)> = splits[&c].iter().map(|&v| (self.param(c, &self.pt(v)), v)).collect();
            ids.sort_by(|a, b| a.0.total_cmp(&b.0));
            if self.is_circle(c) {
                if ids.len() == 1 {
                    if covered(self, ids[0].0 + PI) {
                        edges.insert((c, ids[0].1, ids[0].1));
                    }
                    continue;
                }
                for i in 0..ids.len() {
                    let (t0, v0) = ids[i];
                    let (t1, v1) = if i + 1 < ids.len() { ids[i + 1] } else { (ids[0].0 + TAU, ids[0].1) };
                    if covered(self, 0.5 * (t0 + t1)) {
                        edges.insert((c, v0, v1));
                    }
                }
            } else {
                for w in ids.windows(2) {
                    if w[0].1 != w[1].1 && covered(self, 0.5 * (w[0].0 + w[1].0)) {
                        edges.insert((c, w[0].1, w[1].1));
                    }
                }
            }
        }
        // prune dangling edges
        loop {
            let mut deg: HashMap<u32, usize> = HashMap::new();
            for &(_, s, e) in &edges {
                *deg.entry(s).or_default() += 1;
                *deg.entry(e).or_default() += 1;
            }
            let n = edges.len();
            edges.retain(|&(_, s, e)| deg[&s] > 1 && deg[&e] > 1);
            if edges.len() == n {
                break;
            }
        }
        let directed: Vec<He> = edges.iter().flat_map(|&k| [He::of(k, true), He::of(k, false)]).collect();
        let cycles = self.trace(&directed, f.surf, f.sign)?;
        let mut out = Vec::new();
        for (loops, infinite) in self.assemble(cycles, f.surf, f.sign, true)? {
            let q = if infinite {
                match self.pole_point(f.surf) {
                    Some(p) if f.loops.is_empty() || g0.area < 0.0 => p,
                    _ => continue,
                }
            } else {
                let l2: Vec<Vec<Prim>> = loops.iter().map(|l| self.loop_prims(l, f.surf, f.sign)).collect::<GResult<_>>()?;
                let Some(q) = sample_2d(&l2) else { continue };
                let q = self.from2f(f.surf, f.sign, q);
                // the face boundary is part of the arrangement, so pieces are strictly in or out
                if !self.inside_strict(f, g0, &q) {
                    continue;
                }
                q
            };
            out.push((loops, q));
        }
        Ok(out)
    }

    /// Tangent angle (in the face's oriented tangent plane at the start vertex) and geodesic
    /// curvature of a half-edge leaving its start point.
    fn out_key(&self, h: He, surf: usize, sign: f64) -> (f64, f64) {
        let p = self.pt(h.a);
        let n = natural_normal(&self.surfs[surf], &p) * sign;
        let e1 = Frame::from_origin_z(P3::origin(), n).x;
        let e2 = n.cross(&e1);
        let (t, k) = match &self.curves[h.c as usize] {
            Curve::Line { dir, .. } => (if h.fwd { *dir } else { -dir }, V3::zeros()),
            Curve::Circle { frame, radius } => {
                let l = frame.to_local(p);
                let th = l.y.atan2(l.x);
                let tan = frame.y * th.cos() - frame.x * th.sin();
                ((if h.fwd { tan } else { -tan }), -(frame.x * th.cos() + frame.y * th.sin()) / *radius)
            }
            _ => (V3::x(), V3::zeros()),
        };
        (t.dot(&e2).atan2(t.dot(&e1)).rem_euclid(TAU), n.cross(&t).dot(&k))
    }

    /// Cycles of a set of directed half-edges: after arriving at v, leave by the half-edge first
    /// clockwise from the direction back along the arrival (faces on the left).
    fn trace(&self, directed: &[He], surf: usize, sign: f64) -> GResult<Vec<Vec<He>>> {
        let mut outgoing: HashMap<u32, Vec<He>> = HashMap::new();
        for &h in directed {
            outgoing.entry(h.a).or_default().push(h);
        }
        let mut keys: HashMap<He, (f64, f64)> = HashMap::new();
        let mut key = |h: He| *keys.entry(h).or_insert_with(|| self.out_key(h, surf, sign));
        let mut seen: BTreeSet<He> = BTreeSet::new();
        let mut cycles = Vec::new();
        for &start in directed {
            if seen.contains(&start) {
                continue;
            }
            let mut cyc = Vec::new();
            let mut h = start;
            loop {
                if !seen.insert(h) {
                    return Err(GeomError::Numerical("boolean: edge cycles do not close (inconsistent arrangement)".into()));
                }
                cyc.push(h);
                let back = h.rev();
                let kb = key(back);
                let cands = outgoing.get(&h.b).ok_or_else(|| GeomError::Numerical("boolean: open edge chain".into()))?;
                // clockwise turn from `back` with effective angles θ + ε·κ (κ = geodesic
                // curvature): ties in θ are decided by κ, the curve bending further left lying
                // further counter-clockwise; going straight back is the last resort
                let mut best: Option<(f64, f64, He)> = None;
                for &w in cands {
                    let kw = key(w);
                    let cw = if w == back {
                        TAU + 1.0
                    } else {
                        let d = (kb.0 - kw.0).rem_euclid(TAU);
                        if !(ANGLE_TIE..=TAU - ANGLE_TIE).contains(&d) {
                            if kw.1 < kb.1 { 0.0 } else { TAU }
                        } else {
                            d
                        }
                    };
                    let better = best.is_none_or(|b| if (cw - b.0).abs() < ANGLE_TIE { kw.1 > b.1 } else { cw < b.0 });
                    if better {
                        best = Some((cw, kw.1, w));
                    }
                }
                h = best.expect("non-empty").2;
                if h == start {
                    break;
                }
            }
            cycles.push(cyc);
        }
        Ok(cycles)
    }

    /// Groups cycles into faces: counter-clockwise cycles (in the chart) are outer loops; each
    /// clockwise cycle becomes a hole of the smallest outer loop containing it. Clockwise cycles
    /// in none: on a sphere, the boundary of the region containing the chart pole, returned as one
    /// piece flagged `true`; elsewhere dropped (`allow_free`) or an error.
    fn assemble(&self, cycles: Vec<Vec<He>>, surf: usize, sign: f64, allow_free: bool) -> GResult<Vec<(Vec<Vec<He>>, bool)>> {
        let prims: Vec<Vec<Prim>> = cycles.iter().map(|c| self.loop_prims(c, surf, sign)).collect::<GResult<_>>()?;
        let area: Vec<f64> = prims.iter().map(|p| region_area(std::slice::from_ref(p))).collect();
        let outers: Vec<usize> = (0..cycles.len()).filter(|&i| area[i] > 0.0).collect();
        let mut holes: Vec<Vec<Vec<He>>> = vec![Vec::new(); outers.len()];
        let mut free: Vec<Vec<He>> = Vec::new();
        for h in (0..cycles.len()).filter(|&i| area[i] <= 0.0) {
            let host = (0..outers.len())
                .filter(|&k| self.cycle_inside(&cycles[h], &cycles[outers[k]], &prims[outers[k]], surf, sign))
                .min_by(|&x, &y| area[outers[x]].total_cmp(&area[outers[y]]));
            match host {
                Some(k) => holes[k].push(cycles[h].clone()),
                None => free.push(cycles[h].clone()),
            }
        }
        let mut out: Vec<(Vec<Vec<He>>, bool)> = outers.iter().zip(holes).map(|(&o, h)| (std::iter::once(cycles[o].clone()).chain(h).collect(), false)).collect();
        if !free.is_empty() {
            if matches!(self.charts[surf], Chart::Sph { .. }) {
                out.push((free, true));
            } else if !allow_free {
                return Err(GeomError::Numerical("boolean: a merged face has a hole outside its outer loop".into()));
            }
        }
        Ok(out)
    }

    /// True when cycle `inner` lies inside cycle `outer` (tested at a vertex of `inner` not on
    /// `outer`, else at the midpoint of an edge `outer` does not use).
    fn cycle_inside(&self, inner: &[He], outer: &[He], outer_prims: &[Prim], surf: usize, sign: f64) -> bool {
        let test = |p: &P3| self.to2f(surf, sign, p).is_some_and(|q| winding(std::slice::from_ref(&outer_prims.to_vec()), q) != 0);
        let ov: BTreeSet<u32> = outer.iter().map(|h| h.a).collect();
        if let Some(h) = inner.iter().find(|h| !ov.contains(&h.a)) {
            return test(&self.pt(h.a));
        }
        let ok: BTreeSet<Key> = outer.iter().map(|h| h.key()).collect();
        for &h in inner {
            if !ok.contains(&h.key()) {
                return test(&self.he_mid(h));
            }
        }
        false
    }

    fn he_mid(&self, h: He) -> P3 {
        match &self.curves[h.c as usize] {
            Curve::Line { .. } => nalgebra::center(&self.pt(h.a), &self.pt(h.b)),
            c => {
                let (t0, sw) = self.sweep(h);
                c.point(t0 + 0.5 * sw)
            }
        }
    }

    // ----- classification -----

    fn classify(&self, q: &P3, f: &QFace, other: &[QFace], og: &[FGeo]) -> GResult<Class> {
        for (g, geo) in other.iter().zip(og) {
            if g.surf == f.surf && self.inside_strict(g, geo, q) {
                return Ok(if g.sign == f.sign { Class::OnSame } else { Class::OnOpp });
            }
        }
        Ok(if self.ray_inside(q, Some(f.surf), other, og)? { Class::In } else { Class::Out })
    }

    /// Point-in-solid by ray casting (faces on `skip` are ignored: the point lies on that surface
    /// outside them).
    fn ray_inside(&self, q: &P3, skip: Option<usize>, faces: &[QFace], geos: &[FGeo]) -> GResult<bool> {
        for d in ray_dirs() {
            if let Some(w) = self.cast(q, &d, skip, faces, geos) {
                return Ok(w > 0);
            }
        }
        Err(GeomError::Numerical(format!("boolean: every ray from {q:?} is degenerate")))
    }

    /// Signed crossing count (exits − entries) along the ray, `None` when degenerate.
    fn cast(&self, q: &P3, d: &V3, skip: Option<usize>, faces: &[QFace], geos: &[FGeo]) -> Option<i64> {
        let mut w = 0;
        for (g, geo) in faces.iter().zip(geos) {
            if Some(g.surf) == skip {
                continue;
            }
            match line_surface(q, d, &self.surfs[g.surf], &self.tol).ok()? {
                CurveSurfaceIntersection::Coincident => return None,
                CurveSurfaceIntersection::None => {}
                CurveSurfaceIntersection::Points(hits) => {
                    for x in hits {
                        if x.t < -self.lin {
                            continue;
                        }
                        let loc = self.locate(g, geo, &x.point);
                        if loc == Loc::Outside {
                            continue;
                        }
                        if x.t <= self.lin || loc == Loc::Boundary || x.tangent {
                            return None;
                        }
                        let c = (natural_normal(&self.surfs[g.surf], &x.point) * g.sign).dot(d);
                        if c.abs() < GRAZING_COS {
                            return None;
                        }
                        w += if c > 0.0 { 1 } else { -1 };
                    }
                }
            }
        }
        Some(w)
    }

    // ----- sewing -----

    fn sew(&mut self, faces: Vec<QFace>) -> GResult<Brep> {
        if faces.is_empty() {
            return Ok(Brep::new());
        }
        let faces = self.fix_t_junctions(faces);
        let faces = self.merge_same_surface(faces)?;
        // shells by connectivity through edges (any number of uses)
        let uses = edge_uses(&faces);
        let mut uf = Uf::new(faces.len());
        for (k, us) in &uses {
            let fwd = us.iter().filter(|u| u.1).count();
            if fwd * 2 != us.len() {
                return Err(GeomError::Numerical(format!("boolean: edge {k:?} has {} uses, {fwd} forward (result not closed)", us.len())));
            }
            for u in us {
                uf.union(us[0].0, u.0);
            }
        }
        let mut shells: Vec<Vec<QFace>> = Vec::new();
        for g in uf.groups(faces.len()) {
            let mut sf: Vec<QFace> = g.iter().map(|&i| faces[i].clone()).collect();
            self.remove_degree2(&mut sf);
            shells.push(sf);
        }
        let snapped: Vec<HashMap<u32, P3>> = shells.iter().map(|s| self.snap(s)).collect();
        let vols: Vec<f64> = shells
            .iter()
            .zip(&snapped)
            .map(|(s, p)| {
                let mut tmp = Brep::new();
                let so = tmp.add_solid();
                self.build_shell(&mut tmp, so, s, p);
                crate::mass::solid_integrals(&tmp, SolidId(0)).map(|(_, m)| m[0])
            })
            .collect::<GResult<_>>()?;
        let outers: Vec<usize> = (0..shells.len()).filter(|&i| vols[i] > 0.0).collect();
        let mut solids: Vec<(usize, Vec<usize>)> = outers.iter().map(|&i| (i, Vec::new())).collect();
        for v in (0..shells.len()).filter(|&i| vols[i] <= 0.0) {
            let g = self.geo(&shells[v][0])?;
            let q = self.face_sample(&shells[v][0], &g)?;
            let mut host = None;
            for k in 0..solids.len() {
                let outer = &shells[solids[k].0];
                let og = self.geos(outer)?;
                if self.ray_inside(&q, None, outer, &og)? && host.is_none_or(|h: usize| vols[solids[k].0] < vols[solids[h].0]) {
                    host = Some(k);
                }
            }
            let host = host.ok_or_else(|| GeomError::Numerical("boolean: an inward-facing shell lies in no outer shell".into()))?;
            solids[host].1.push(v);
        }
        let mut out = Brep::new();
        for (outer, voids) in solids {
            let s = out.add_solid();
            for sh in std::iter::once(outer).chain(voids) {
                self.build_shell(&mut out, s, &shells[sh], &snapped[sh]);
            }
        }
        Ok(out)
    }

    /// Inserts every result vertex lying on the interior of a loop edge into that edge.
    fn fix_t_junctions(&self, mut faces: Vec<QFace>) -> Vec<QFace> {
        let used: BTreeSet<u32> = faces.iter().flat_map(|f| f.hes().map(|h| h.a)).collect();
        for f in &mut faces {
            for l in &mut f.loops {
                let mut nl = Vec::with_capacity(l.len());
                for &h in l.iter() {
                    let k = h.key();
                    let (t0, t1) = self.interval(k);
                    let scale = if self.is_circle(h.c) { self.radius(h.c) } else { 1.0 };
                    let mut ins: Vec<(f64, u32)> = used
                        .iter()
                        .filter(|&&v| v != k.1 && v != k.2)
                        .filter_map(|&v| {
                            let p = self.pt(v);
                            if self.curve_dist(h.c, &p) > self.lin {
                                return None;
                            }
                            let t = self.param_in(k, &p, 0.0)?;
                            ((t - t0) * scale > self.lin && (t1 - t) * scale > self.lin).then_some((t, v))
                        })
                        .collect();
                    if ins.is_empty() {
                        nl.push(h);
                        continue;
                    }
                    ins.sort_by(|x, y| x.0.total_cmp(&y.0));
                    if !h.fwd {
                        ins.reverse();
                    }
                    let mut prev = h.a;
                    for (_, v) in ins {
                        nl.push(He { c: h.c, a: prev, b: v, fwd: h.fwd });
                        prev = v;
                    }
                    nl.push(He { c: h.c, a: prev, b: h.b, fwd: h.fwd });
                }
                *l = nl;
            }
        }
        faces
    }

    /// Merges faces on the same canonical surface with the same orientation that share an edge
    /// used exactly twice (never across a non-manifold edge); merged faces are re-traced.
    fn merge_same_surface(&self, faces: Vec<QFace>) -> GResult<Vec<QFace>> {
        let uses = edge_uses(&faces);
        let mut uf = Uf::new(faces.len());
        let mut slit = vec![false; faces.len()];
        for us in uses.values() {
            if us.len() == 2 && us[0].1 != us[1].1 {
                let (i, j) = (us[0].0, us[1].0);
                if i == j {
                    slit[i] = true;
                } else if faces[i].surf == faces[j].surf && faces[i].sign == faces[j].sign {
                    uf.union(i, j);
                }
            }
        }
        let mut out = Vec::new();
        for g in uf.groups(faces.len()) {
            if g.len() == 1 && !slit[g[0]] {
                out.push(faces[g[0]].clone());
                continue;
            }
            let (surf, sign) = (faces[g[0]].surf, faces[g[0]].sign);
            let inside: BTreeSet<usize> = g.iter().copied().collect();
            // cancel edges whose two uses both lie in the group
            let mut directed = Vec::new();
            for &i in &g {
                for h in faces[i].hes() {
                    let us = &uses[&h.key()];
                    let internal = us.len() == 2 && us.iter().all(|u| inside.contains(&u.0)) && us[0].1 != us[1].1;
                    if !internal {
                        directed.push(h);
                    }
                }
            }
            if directed.is_empty() {
                if !matches!(self.surfs[surf], Surface::Sphere { .. }) {
                    return Err(GeomError::Numerical("boolean: a merged face has no boundary".into()));
                }
                out.push(QFace { surf, sign, loops: Vec::new() });
                continue;
            }
            let cycles = self.trace(&directed, surf, sign)?;
            for (loops, _) in self.assemble(cycles, surf, sign, false)? {
                out.push(QFace { surf, sign, loops });
            }
        }
        Ok(out)
    }

    /// Removes vertices joining exactly two edges of the same canonical curve (both manifold).
    fn remove_degree2(&self, faces: &mut [QFace]) {
        loop {
            let uses = edge_uses(faces);
            let mut inc: BTreeMap<u32, BTreeSet<Key>> = BTreeMap::new();
            for k in uses.keys() {
                inc.entry(k.1).or_default().insert(*k);
                inc.entry(k.2).or_default().insert(*k);
            }
            let victim = inc.iter().find_map(|(&v, ks)| {
                let [k1, k2] = <[Key; 2]>::try_from(ks.iter().copied().collect::<Vec<_>>()).ok()?;
                let (k1, k2) = if k1.2 == v { (k1, k2) } else { (k2, k1) };
                (k1.0 == k2.0 && k1.2 == v && k2.1 == v && k1.1 != v && k2.2 != v && uses[&k1].len() == 2 && uses[&k2].len() == 2).then_some((v, k1.0))
            });
            let Some((v, c)) = victim else { return };
            for f in faces.iter_mut() {
                for l in &mut f.loops {
                    let n = l.len();
                    let Some(i) = (0..n).find(|&i| l[i].c == c && l[i].b == v) else { continue };
                    let j = (i + 1) % n;
                    let merged = He { c, a: l[i].a, b: l[j].b, fwd: l[i].fwd };
                    if n == 2 {
                        *l = vec![merged];
                    } else {
                        l[i] = merged;
                        l.remove(j);
                    }
                }
            }
        }
    }

    /// Vertex positions moved to the least-squares point of their faces' surfaces (Gauss–Newton,
    /// minimum-norm steps; never more than 4·tol).
    fn snap(&self, faces: &[QFace]) -> HashMap<u32, P3> {
        let mut surfs: BTreeMap<u32, BTreeSet<usize>> = BTreeMap::new();
        for f in faces {
            for h in f.hes() {
                surfs.entry(h.a).or_default().insert(f.surf);
            }
        }
        surfs
            .into_iter()
            .map(|(v, ss)| {
                let p0 = self.pt(v);
                let mut p = p0;
                let ss: Vec<&Surface> = ss.iter().map(|&k| &self.surfs[k]).collect();
                for _ in 0..4 {
                    let rows: Vec<(f64, V3)> = ss.iter().map(|s| signed_distance(s, &p)).collect();
                    let m = nalgebra::DMatrix::from_fn(rows.len(), 3, |i, j| rows[i].1[j]);
                    let r = nalgebra::DVector::from_fn(rows.len(), |i, _| -rows[i].0);
                    let Ok(dx) = m.svd(true, true).solve(&r, 1e-9) else { break };
                    p += V3::new(dx[0], dx[1], dx[2]);
                }
                let q = if (p - p0).norm() <= 4.0 * self.lin { p } else { p0 };
                (v, q)
            })
            .collect()
    }

    fn build_shell(&self, out: &mut Brep, solid: SolidId, faces: &[QFace], pos: &HashMap<u32, P3>) {
        let sh = out.add_shell(Some(solid));
        let uses = edge_uses(faces);
        let mut verts: HashMap<u32, VertexId> = HashMap::new();
        let mut edges: HashMap<Key, EdgeId> = HashMap::new();
        for f in faces {
            let surface = match (&self.surfs[f.surf], self.charts[f.surf]) {
                (Surface::Sphere { radius, .. }, Chart::Sph { c, n, .. }) => Surface::Sphere { frame: Frame::from_origin_z(c, n), radius: *radius },
                (s, _) => s.clone(),
            };
            let fid = out.add_face(sh, surface, f.sign < 0.0);
            for l in &f.loops {
                let mut hs = Vec::with_capacity(l.len());
                for &h in l {
                    let k = h.key();
                    let eid = match edges.get(&k) {
                        Some(&e) => e,
                        None => {
                            let mut vid = |i: u32| *verts.entry(i).or_insert_with(|| out.add_vertex(pos[&i]));
                            let (vs, ve) = (vid(k.1), vid(k.2));
                            let e = match &self.curves[k.0 as usize] {
                                Curve::Circle { frame, radius } => {
                                    let (_, sw) = self.sweep(He::of(k, true));
                                    let l0 = frame.to_local(pos[&k.1]);
                                    let t0 = l0.y.atan2(l0.x).rem_euclid(TAU);
                                    out.add_edge(Some(Curve::Circle { frame: *frame, radius: *radius }), t0, t0 + sw, vs, ve)
                                }
                                _ => out.add_line_edge(vs, ve),
                            };
                            if uses[&k].len() > 2 {
                                out.nonmanifold_edges.push(e);
                            }
                            edges.insert(k, e);
                            e
                        }
                    };
                    hs.push((eid, !h.fwd));
                }
                out.add_loop(fid, &hs);
            }
        }
    }
}

/// Uses of every edge: key → [(face, forward)].
fn edge_uses(faces: &[QFace]) -> BTreeMap<Key, Vec<(usize, bool)>> {
    let mut m: BTreeMap<Key, Vec<(usize, bool)>> = BTreeMap::new();
    for (i, f) in faces.iter().enumerate() {
        for h in f.hes() {
            m.entry(h.key()).or_default().push((i, h.fwd));
        }
    }
    m
}

// ---------------------------------------------------------------------------------------------
// Quadric–quadric intersections not covered by `intersect::surface_surface`

/// Two spheres: their circle of intersection (none when concentric, disjoint, nested or
/// tangent within `lin`).
fn sphere_sphere(c1: &P3, r1: f64, c2: &P3, r2: f64, lin: f64) -> Vec<Curve> {
    let dv = c2 - c1;
    let d = dv.norm();
    if d <= lin || d >= r1 + r2 - lin || d <= (r1 - r2).abs() + lin {
        return Vec::new();
    }
    let n = dv / d;
    let a = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
    let rc = (r1 * r1 - a * a).max(0.0).sqrt();
    vec![Curve::Circle { frame: Frame::from_origin_z(c1 + n * a, n), radius: rc }]
}

/// Two cylinders with parallel axes: 0, 1 (tangent) or 2 lines parallel to the axes. Other
/// configurations are `Unsupported` unless the cylinders are certainly apart.
fn cyl_cyl(f1: &Frame, r1: f64, f2: &Frame, r2: f64, lin: f64, ang: f64) -> GResult<Vec<Curve>> {
    if f1.z.cross(&f2.z).norm() > ang {
        // closest distance between the axes
        let n = f1.z.cross(&f2.z).normalize();
        if (f2.origin - f1.origin).dot(&n).abs() > r1 + r2 + lin {
            return Ok(Vec::new());
        }
        return Err(GeomError::Unsupported("boolean: intersection of cylinders with non-parallel axes".into()));
    }
    let l = f1.to_local(f2.origin);
    let dd = l.x.hypot(l.y);
    if dd <= lin || dd > r1 + r2 + lin || dd < (r1 - r2).abs() - lin {
        return Ok(Vec::new());
    }
    let e = [l.x / dd, l.y / dd];
    let line = |x: f64, y: f64| Curve::Line { origin: f1.origin + f1.x * x + f1.y * y, dir: f1.z };
    if (dd - (r1 + r2)).abs() <= lin || (dd - (r1 - r2).abs()).abs() <= lin {
        let s = if dd * dd + r1 * r1 - r2 * r2 >= 0.0 { r1 } else { -r1 };
        return Ok(vec![line(e[0] * s, e[1] * s)]);
    }
    let a = (dd * dd + r1 * r1 - r2 * r2) / (2.0 * dd);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let (bx, by) = (e[0] * a, e[1] * a);
    Ok(vec![line(bx - e[1] * h, by + e[0] * h), line(bx + e[1] * h, by - e[0] * h)])
}

/// Cylinder and sphere: coaxial (centre on the axis) gives 0 or 2 circles; other configurations
/// are `Unsupported` unless certainly apart.
fn cyl_sphere(f: &Frame, r: f64, c: &P3, rs: f64, lin: f64) -> GResult<Vec<Curve>> {
    let l = f.to_local(*c);
    let rho = l.x.hypot(l.y);
    if rho - rs > r + lin || rho + rs < r - lin {
        return Ok(Vec::new());
    }
    if rho > lin {
        return Err(GeomError::Unsupported("boolean: intersection of a cylinder and a sphere off its axis".into()));
    }
    if r >= rs - lin {
        // the sphere lies inside the cylinder (or touches it along a circle)
        return Ok(Vec::new());
    }
    let h = (rs * rs - r * r).sqrt();
    Ok([l.z - h, l.z + h].iter().map(|&z| Curve::Circle { frame: Frame { origin: f.origin + f.z * z, ..*f }, radius: r }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each chart preserves orientation: the images of S_u and S_v (natural normal S_u × S_v)
    /// form a counter-clockwise pair, and from2 inverts to2.
    #[test]
    fn charts_preserve_orientation_and_invert() {
        let f = Frame::from_origin_z(P3::new(1.0, -2.0, 0.5), V3::new(0.3, 0.4, 0.8));
        let cases = [
            (Surface::Plane { frame: f }, Chart::Plane(f)),
            (Surface::Cylinder { frame: f, radius: 3.0 }, Chart::Cyl { f, r: 3.0, v0: -20.0 }),
            (Surface::Sphere { frame: f, radius: 3.0 }, {
                let n = V3::new(0.6, 0.0, 0.8);
                let e1 = Frame::from_origin_z(P3::origin(), n).x;
                Chart::Sph { c: f.origin, r: 3.0, n, e1, e2: e1.cross(&n) }
            }),
        ];
        for (s, ch) in cases {
            for (u, v) in [(0.3, 0.2), (2.0, -0.7), (4.5, 1.1)] {
                let p = s.point(u, v);
                let q = ch.to2(&p).unwrap();
                assert!((ch.from2(q) - p).norm() < 1e-12, "{s:?}");
                let h = 1e-6;
                let qu = ch.to2(&s.point(u + h, v)).unwrap();
                let qv = ch.to2(&s.point(u, v + h)).unwrap();
                assert!(cross2(sub2(qu, q), sub2(qv, q)) > 0.0, "{} at ({u}, {v})", s.type_name());
            }
        }
    }

    /// The quadric pipeline is general: fed the recorded box cases directly (the public entry
    /// sends polyhedra to the polyhedral pipeline), it reproduces ASM's counts and volumes.
    #[test]
    fn polyhedra_through_the_quadric_pipeline() {
        use crate::primitives::make_box;
        let b = |c: [f64; 3], s: [f64; 3]| make_box(P3::from(c), s).unwrap();
        let cases = [
            (b([0.0; 3], [20.0; 3]), b([10.0; 3], [20.0; 3]), BoolOp::Union, [12, 30, 20], 15000.0),
            (b([0.0; 3], [20.0; 3]), b([20.0, 0.0, 0.0], [20.0; 3]), BoolOp::Union, [6, 12, 8], 16000.0),
            (b([0.0; 3], [20.0; 3]), b([20.0, 5.0, 5.0], [20.0, 10.0, 10.0]), BoolOp::Union, [11, 24, 16], 10000.0),
            (b([0.0; 3], [20.0; 3]), b([5.0, 5.0, 0.0], [10.0, 10.0, 20.0]), BoolOp::Cut, [10, 24, 16], 6000.0),
            (b([0.0; 3], [10.0; 3]), b([20.0; 3], [5.0; 3]), BoolOp::Cut, [6, 12, 8], 1000.0),
            (b([0.0; 3], [20.0; 3]), b([10.0; 3], [20.0; 3]), BoolOp::Intersect, [6, 12, 8], 1000.0),
        ];
        for (x, y, op, fev, v) in cases {
            let r = boolean_quadric(&x, &y, op, &Tolerance::asm()).unwrap();
            assert_eq!([r.faces.len(), r.edges.len(), r.vertices.len()], fev, "{op:?} {fev:?}");
            let rep = crate::validate(&r, &Tolerance::default());
            assert!(rep.is_valid(), "{:?}", rep.issues);
            let m = crate::mass::solid_properties(&r, SolidId(0)).unwrap();
            assert!((m.volume - v).abs() < 1e-9 * v, "{} vs {v}", m.volume);
        }
    }

    #[test]
    fn arc_area_and_winding() {
        // unit circle counter-clockwise from angle 1.0
        let s = [1f64.cos(), 1f64.sin()];
        let full = Prim::Arc { c: [0.0, 0.0], r: 1.0, a0: 1.0, sw: TAU, s, e: s };
        assert!((full.area() - PI).abs() < 1e-14);
        let l = vec![vec![full]];
        assert_eq!(winding(&l, [0.2, -0.3]), 1);
        assert_eq!(winding(&l, [1.2, 0.0]), 0);
        // half disc: arc from (1,0) to (−1,0) through (0,1), then the diameter back
        let half = vec![vec![Prim::Arc { c: [0.0, 0.0], r: 1.0, a0: 0.0, sw: PI, s: [1.0, 0.0], e: [-1.0, 0.0] }, Prim::Seg([-1.0, 0.0], [1.0, 0.0])]];
        assert!((region_area(&half) - PI / 2.0).abs() < 1e-14);
        assert_eq!(winding(&half, [0.0, 0.5]), 1);
        assert_eq!(winding(&half, [0.0, -0.5]), 0);
        // the vertex ordinate itself: half-open rule counts once
        assert_eq!(winding(&half, [-2.0, 0.0]), 0);
        let q = sample_2d(&half).unwrap();
        assert_eq!(winding(&half, q), 1);
        // clockwise arc
        let cw = vec![vec![Prim::Arc { c: [0.0, 0.0], r: 2.0, a0: 0.5, sw: -TAU, s: [2.0 * 0.5f64.cos(), 2.0 * 0.5f64.sin()], e: [2.0 * 0.5f64.cos(), 2.0 * 0.5f64.sin()] }]];
        assert!((region_area(&cw) + 4.0 * PI).abs() < 1e-13);
        assert_eq!(winding(&cw, [0.0, 0.0]), -1);
    }
}
