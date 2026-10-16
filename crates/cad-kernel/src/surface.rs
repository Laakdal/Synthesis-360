//! Surface-modelling primitives over the shim's `surface` section (wires from sketch curves,
//! sewing, offset, thicken, patch filling, mesh faces) and the sketch curve chains the SURFACE
//! tab's Extrude / Revolve work on.

use crate::frame::{Frame, V3};
use crate::occ::{wrap_ffi, KResult, Shape};
use cad_core::sketch::{EntityKind, Sketch};
use opencascade_sys::cad360 as ffi;

pub const COMPOUND: i32 = 0;
pub const SHELL: i32 = 3;

/// One sketch curve in sketch coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve2 {
    Line([f64; 2], [f64; 2]),
    /// Arc through start / mid / end.
    Arc([f64; 2], [f64; 2], [f64; 2]),
    /// Full circle: centre, radius.
    Circle([f64; 2], f64),
}

impl Curve2 {
    fn ends(&self) -> Option<([f64; 2], [f64; 2])> {
        match *self {
            Curve2::Line(a, b) | Curve2::Arc(a, _, b) => Some((a, b)),
            Curve2::Circle(..) => None,
        }
    }
    fn reversed(self) -> Curve2 {
        match self {
            Curve2::Line(a, b) => Curve2::Line(b, a),
            Curve2::Arc(a, m, b) => Curve2::Arc(b, m, a),
            c => c,
        }
    }
}

/// A connected chain of sketch curves (in path order).
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    /// Curve entity ids in path order.
    pub curves: Vec<String>,
    /// Oriented geometry aligned with `curves`.
    pub segs: Vec<Curve2>,
    pub closed: bool,
}

impl Chain {
    pub fn length(&self) -> f64 {
        self.segs
            .iter()
            .map(|s| match *s {
                Curve2::Line(a, b) => d2(a, b),
                Curve2::Arc(a, m, b) => match cad_core::sketch::circumcircle(a, m, b) {
                    Some((c, r)) => {
                        let ang = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
                        let tau = std::f64::consts::TAU;
                        let am = (ang(m) - ang(a)).rem_euclid(tau);
                        let ab = (ang(b) - ang(a)).rem_euclid(tau);
                        r * if am < ab { ab } else { tau - ab }
                    }
                    None => d2(a, b),
                },
                Curve2::Circle(_, r) => std::f64::consts::TAU * r,
            })
            .sum()
    }
}

fn d2(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

const JOIN_TOL: f64 = 1e-6;

/// Geometry of a (solved) sketch curve.
pub fn curve_geometry(sk: &Sketch, id: &str) -> Option<Curve2> {
    let e = sk.entity(id)?;
    match &e.kind {
        EntityKind::Line { p1, p2 } => Some(Curve2::Line(sk.point(p1)?, sk.point(p2)?)),
        EntityKind::Circle { c, r } => Some(Curve2::Circle(sk.point(c)?, *r)),
        EntityKind::Arc { c, start, end, r } => {
            let (c, s, e) = (sk.point(c)?, sk.point(start)?, sk.point(end)?);
            let a0 = (s[1] - c[1]).atan2(s[0] - c[0]);
            let a1 = (e[1] - c[1]).atan2(e[0] - c[0]);
            let sweep = (a1 - a0).rem_euclid(std::f64::consts::TAU);
            let sweep = if sweep < 1e-12 { std::f64::consts::TAU } else { sweep };
            let am = a0 + sweep / 2.0;
            Some(Curve2::Arc(s, [c[0] + r * am.cos(), c[1] + r * am.sin()], e))
        }
        EntityKind::Point { .. } => None,
    }
}

/// Orders the given curves into connected chains (paths through shared end points; branches
/// start new chains). Unknown ids are skipped.
pub fn chains_of(sk: &Sketch, ids: &[String]) -> Vec<Chain> {
    let mut items: Vec<(String, Curve2)> = Vec::new();
    for id in ids {
        if items.iter().any(|(i, _)| i == id) {
            continue;
        }
        if let Some(g) = curve_geometry(sk, id) {
            items.push((id.clone(), g));
        }
    }
    let mut used = vec![false; items.len()];
    let mut out = Vec::new();
    // closed circles are chains of their own
    for (k, (id, g)) in items.iter().enumerate() {
        if let Curve2::Circle(..) = g {
            used[k] = true;
            out.push(Chain { curves: vec![id.clone()], segs: vec![*g], closed: true });
        }
    }
    let degree = |p: [f64; 2], items: &[(String, Curve2)]| -> usize {
        items.iter().filter_map(|(_, g)| g.ends()).map(|(a, b)| (d2(a, p) < JOIN_TOL) as usize + (d2(b, p) < JOIN_TOL) as usize).sum()
    };
    loop {
        // start at a free end (degree 1) if possible so open chains are walked end to end
        let start = (0..items.len())
            .filter(|&k| !used[k])
            .find(|&k| items[k].1.ends().is_some_and(|(a, b)| degree(a, &items) == 1 || degree(b, &items) == 1))
            .or_else(|| (0..items.len()).find(|&k| !used[k]));
        let Some(k0) = start else { break };
        used[k0] = true;
        let mut g0 = items[k0].1;
        if let Some((a, _)) = g0.ends() {
            if degree(a, &items) != 1 {
                // walk away from a free end at the far side
                if let Some((_, b)) = g0.ends() {
                    if degree(b, &items) == 1 {
                        g0 = g0.reversed();
                    }
                }
            }
        }
        let mut chain = Chain { curves: vec![items[k0].0.clone()], segs: vec![g0], closed: false };
        loop {
            let tail = chain.segs.last().and_then(|s| s.ends()).map(|(_, b)| b);
            let Some(tail) = tail else { break };
            let next = (0..items.len()).filter(|&k| !used[k]).find_map(|k| {
                let (a, b) = items[k].1.ends()?;
                if d2(a, tail) < JOIN_TOL {
                    Some((k, items[k].1))
                } else if d2(b, tail) < JOIN_TOL {
                    Some((k, items[k].1.reversed()))
                } else {
                    None
                }
            });
            match next {
                Some((k, g)) => {
                    used[k] = true;
                    chain.curves.push(items[k].0.clone());
                    chain.segs.push(g);
                }
                None => break,
            }
        }
        let first = chain.segs.first().and_then(|s| s.ends()).map(|(a, _)| a);
        let last = chain.segs.last().and_then(|s| s.ends()).map(|(_, b)| b);
        chain.closed = matches!((first, last), (Some(a), Some(b)) if d2(a, b) < JOIN_TOL);
        out.push(chain);
    }
    out
}

/// Every chain of the non-construction curves of a sketch (what Surface Extrude offers).
pub fn sketch_chains(sk: &Sketch) -> Vec<Chain> {
    let ids: Vec<String> = sk.entities.iter().filter(|e| e.is_curve() && !e.construction).map(|e| e.id.clone()).collect();
    chains_of(sk, &ids)
}

fn push3(data: &mut Vec<f64>, kind: f64, a: V3, m: V3, b: V3) {
    data.push(kind);
    data.extend_from_slice(&a);
    data.extend_from_slice(&m);
    data.extend_from_slice(&b);
}

/// One wire per chain, in world coordinates (`frame` maps sketch to world), as a compound.
pub fn chain_wires(frame: &Frame, chains: &[Chain]) -> KResult<Shape> {
    let mut data = vec![chains.len() as f64];
    for c in chains {
        data.push(c.segs.len() as f64);
        for s in &c.segs {
            match *s {
                Curve2::Line(a, b) => {
                    let (a, b) = (frame.to_world(a), frame.to_world(b));
                    push3(&mut data, 0.0, a, a, b);
                }
                Curve2::Arc(a, m, b) => push3(&mut data, 1.0, frame.to_world(a), frame.to_world(m), frame.to_world(b)),
                Curve2::Circle(c, r) => push3(&mut data, 2.0, frame.to_world(c), frame.to_world([c[0] + r, c[1]]), frame.n),
            }
        }
    }
    wrap_ffi(ffi::cad_make_wires(&data), "The curves could not be converted to wires")
}

/// Direct children of a compound (or the shape itself when it is not a compound).
pub fn children(s: &Shape) -> Vec<Shape> {
    if s.shape_type() != COMPOUND {
        return vec![s.clone()];
    }
    let v = ffi::cad_children(s.inner());
    v.iter().filter_map(|c| wrap_ffi(opencascade_sys::topo_ds::TopoDS_Shape_to_owned(c), "").ok()).collect()
}

/// Sews faces into shells; with `make_solid`, closed shells become solids. Returns the pieces.
pub fn sew(s: &Shape, tolerance: f64, make_solid: bool) -> KResult<Vec<Shape>> {
    let r = wrap_ffi(ffi::cad_sew(s.inner(), tolerance, make_solid), "Stitch failed")?;
    Ok(children(&r))
}

pub fn offset_surface(s: &Shape, distance: f64) -> KResult<Shape> {
    wrap_ffi(ffi::cad_offset_surface(s.inner(), distance), "Offset failed")
}

pub fn thicken(s: &Shape, thickness: f64) -> KResult<Shape> {
    wrap_ffi(ffi::cad_thicken(s.inner(), thickness), "Thicken failed")
}

/// Face filling the closed loop of `edges` (planar face, or an N-sided filling surface).
pub fn fill(edges: &[&Shape]) -> KResult<Shape> {
    let c = Shape::compound(edges)?;
    wrap_ffi(ffi::cad_fill(c.inner()), "Patch failed")
}

/// Triangulation-only face (mesh body); `tris` holds 9 coordinates per triangle.
pub fn mesh_face(tris: &[f64]) -> KResult<Shape> {
    wrap_ffi(ffi::cad_mesh_face(tris), "The mesh could not be created")
}

/// Mesh converted to B-rep pieces (closed shells become solids, coplanar triangles merged).
pub fn mesh_to_brep(tris: &[f64], tolerance: f64) -> KResult<Vec<Shape>> {
    let r = wrap_ffi(ffi::cad_mesh_to_brep(tris, tolerance), "The mesh could not be converted to B-rep")?;
    Ok(children(&r))
}
