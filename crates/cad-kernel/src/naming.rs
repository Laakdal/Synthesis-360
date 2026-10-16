//! History-based (generative) persistent naming.
//!
//! Every face the kernel produces gets a name derived from *how* it was made, not where it is:
//!
//! * extrude / revolve: lateral faces `{feature}/side:{curve}#{k}` (sketch curve id and the
//!   ordinal of the segment among that curve's segments in the profile), caps `{feature}/start`
//!   and `{feature}/end`;
//! * booleans, transforms, fillets, chamfers, shells: faces that survive (unchanged, trimmed or
//!   split) keep the name of the input face they come from, traced with OCCT's
//!   `BRepBuilderAPI_MakeShape::Modified / Generated` history;
//! * new faces: fillet/chamfer `{feature}/blend:{edge}`, corner blends `{feature}/corner`, shell
//!   walls `{feature}/inner:{face}`, hole `{feature}/hole:wall|floor|top`, pattern instances
//!   `{feature}/i{n}:{face}`, imported faces `{feature}/face{i}`.
//!
//! An edge is named by the sorted names of its two faces (`refs::edge_ref`). Faces split into
//! several pieces share a name; `refs` picks among same-name candidates geometrically.

use crate::body::Body;
use crate::occ::{wrap_ffi, KResult, Seg3, Shape, EDGE, FACE, SOLID};
use opencascade_sys::cad360 as ffi;

/// A shape with generative names for its faces, aligned with `shape.subshapes(FACE)`.
#[derive(Clone, Debug)]
pub struct Named {
    pub shape: Shape,
    pub names: Vec<String>,
}

/// How a result face came from an input sub-shape (see the shim's `naming` section).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The same face, or a trimmed / split / moved version of it.
    Same,
    /// A face generated from a face (e.g. an offset wall).
    FromFace,
    /// A face generated from an edge (sweep side, fillet / chamfer blend).
    FromEdge,
    /// A face generated from a vertex (corner blend).
    FromVertex,
    /// Sweep start cap.
    Start,
    /// Sweep end cap.
    End,
}

impl Kind {
    fn from_code(c: i32) -> Option<Kind> {
        Some(match c {
            0 => Kind::Same,
            1 => Kind::FromFace,
            2 => Kind::FromEdge,
            3 => Kind::FromVertex,
            4 => Kind::Start,
            5 => Kind::End,
            _ => return None,
        })
    }
    /// Precedence when a face has several origins: surviving faces keep their identity first.
    fn rank(self) -> u8 {
        match self {
            Kind::Same => 0,
            Kind::Start | Kind::End => 1,
            Kind::FromEdge => 2,
            Kind::FromFace => 3,
            Kind::FromVertex => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Origin {
    pub input: usize,
    pub kind: Kind,
    pub index: usize,
}

/// Decodes the shim's history vector into per-result-face origins.
pub fn parse_history(h: &[i32]) -> Vec<Vec<Origin>> {
    let mut it = h.iter().copied();
    let n = it.next().unwrap_or(0).max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let c = it.next().unwrap_or(0).max(0) as usize;
        let mut o = Vec::with_capacity(c);
        for _ in 0..c {
            let (i, k, x) = (it.next().unwrap_or(-1), it.next().unwrap_or(-1), it.next().unwrap_or(-1));
            if let (true, Some(kind), true) = (i >= 0, Kind::from_code(k), x >= 0) {
                o.push(Origin { input: i as usize, kind, index: x as usize });
            }
        }
        o.sort_by_key(|o| (o.kind.rank(), o.input, o.index));
        out.push(o);
    }
    out
}

/// Names for the result faces: the first origin (by precedence) the rule can name wins; faces the
/// rule cannot name get `fallback`.
fn derive(origins: &[Vec<Origin>], nfaces: usize, rule: impl Fn(&Origin) -> Option<String>, fallback: &str) -> Vec<String> {
    (0..nfaces).map(|j| origins.get(j).and_then(|o| o.iter().find_map(&rule)).unwrap_or_else(|| fallback.to_string())).collect()
}

fn get(names: &[String], i: usize) -> Option<String> {
    names.get(i).filter(|n| !n.is_empty()).cloned()
}

/// Edge names (sorted adjacent face names joined by `|`) for the edges of a named shape, aligned
/// with `shape.subshapes(EDGE)`.
pub fn edge_names(shape: &Shape, face_names: &[String]) -> Vec<String> {
    shape
        .edge_faces()
        .iter()
        .map(|f| {
            let mut n: Vec<&str> = f.iter().filter(|&&i| i >= 0).filter_map(|&i| face_names.get(i as usize)).map(String::as_str).filter(|s| !s.is_empty()).collect();
            n.sort();
            n.dedup();
            n.join("|")
        })
        .collect()
}

fn run(context: &str, f: impl FnOnce(&mut Vec<i32>) -> cxx::UniquePtr<opencascade_sys::topo_ds::TopoDS_Shape>) -> KResult<(Shape, Vec<Vec<Origin>>)> {
    let mut h = Vec::new();
    let s = wrap_ffi(f(&mut h), context)?;
    Ok((s, parse_history(&h)))
}

fn nfaces(s: &Shape) -> usize {
    s.subshapes(FACE).len()
}

impl Named {
    pub fn new(shape: Shape, names: Vec<String>) -> Named {
        Named { shape, names }
    }

    /// The body's shape with its names (unnamed faces get empty names).
    pub fn of_body(b: &Body) -> Named {
        let n = b.topology().faces.len();
        let mut names = b.face_names.clone();
        names.resize(n, String::new());
        Named { shape: b.shape.clone(), names }
    }

    /// Names every face by its ordinal (`{prefix}{i}`): for shapes without history (imports).
    pub fn by_ordinal(shape: Shape, prefix: &str) -> Named {
        let names = (0..nfaces(&shape)).map(|i| format!("{prefix}{i}")).collect();
        Named { shape, names }
    }

    pub fn fuse(&self, other: &Named, fid: &str) -> KResult<Named> {
        self.boolean(other, 0, fid, "Join failed")
    }
    pub fn cut(&self, other: &Named, fid: &str) -> KResult<Named> {
        self.boolean(other, 1, fid, "Cut failed")
    }
    pub fn common(&self, other: &Named, fid: &str) -> KResult<Named> {
        self.boolean(other, 2, fid, "Intersect failed")
    }

    fn boolean(&self, other: &Named, op: i32, fid: &str, context: &str) -> KResult<Named> {
        let (s, o) = run(context, |h| ffi::cad_boolean_named(self.shape.inner(), other.shape.inner(), op, h))?;
        let inputs = [&self.names, &other.names];
        let names = derive(
            &o,
            nfaces(&s),
            |o| match o.kind {
                Kind::Same | Kind::FromFace => inputs.get(o.input).and_then(|n| get(n, o.index)),
                _ => None,
            },
            &format!("{fid}/face"),
        );
        Ok(Named { shape: s, names })
    }

    /// Fillet (`fillet = true`) or chamfer of `edges` (edges of this shape).
    pub fn blend(&self, edges: &[&Shape], value: f64, fillet: bool, fid: &str) -> KResult<Named> {
        let c = Shape::compound(edges)?;
        let context = if fillet { "Fillet failed" } else { "Chamfer failed" };
        let (s, o) = run(context, |h| ffi::cad_blend_named(self.shape.inner(), c.inner(), value, fillet, h))?;
        let en = edge_names(&self.shape, &self.names);
        let names = derive(
            &o,
            nfaces(&s),
            |o| match o.kind {
                Kind::Same | Kind::FromFace => get(&self.names, o.index),
                Kind::FromEdge => Some(format!("{fid}/blend:{}", en.get(o.index).map(String::as_str).unwrap_or("?"))),
                Kind::FromVertex => Some(format!("{fid}/corner")),
                _ => None,
            },
            &format!("{fid}/face"),
        );
        Ok(Named { shape: s, names })
    }

    /// Inward shell removing `faces` (faces of this shape).
    pub fn shell(&self, faces: &[&Shape], thickness: f64, fid: &str) -> KResult<Named> {
        let c = Shape::compound(faces)?;
        let (s, o) = run("Shell failed", |h| ffi::cad_shell_named(self.shape.inner(), c.inner(), thickness, h))?;
        let en = edge_names(&self.shape, &self.names);
        let names = derive(
            &o,
            nfaces(&s),
            |o| match o.kind {
                Kind::Same => get(&self.names, o.index),
                Kind::FromFace => get(&self.names, o.index).map(|n| format!("{fid}/inner:{n}")),
                Kind::FromEdge => Some(format!("{fid}/blend:{}", en.get(o.index).map(String::as_str).unwrap_or("?"))),
                Kind::FromVertex => Some(format!("{fid}/corner")),
                _ => None,
            },
            &format!("{fid}/face"),
        );
        Ok(Named { shape: s, names })
    }

    /// Transformed copy (kind 0: row-major 3x4 rigid matrix; kind 1: mirror point + normal); each
    /// face's name is passed through `rename`.
    pub fn transformed(&self, kind: i32, m: &[f64], rename: impl Fn(&str) -> String) -> KResult<Named> {
        let (s, o) = run("Transform failed", |h| ffi::cad_transform_named(self.shape.inner(), kind, m, h))?;
        let names = derive(&o, nfaces(&s), |o| if o.kind == Kind::Same { get(&self.names, o.index).map(|n| rename(&n)) } else { None }, "");
        Ok(Named { shape: s, names })
    }

    /// The solids of this shape, each with its own (re-indexed) names.
    pub fn solids(&self) -> Vec<Named> {
        let solids = self.shape.subshapes(SOLID);
        solids
            .into_iter()
            .map(|solid| {
                let mut idx = Vec::new();
                ffi::cad_subshape_indices(self.shape.inner(), solid.inner(), FACE, &mut idx);
                let names = idx.iter().map(|&i| if i >= 0 { self.names.get(i as usize).cloned().unwrap_or_default() } else { String::new() }).collect();
                Named { shape: solid, names }
            })
            .collect()
    }
}

/// One profile segment in 3D with its generative name (`{curve}#{k}`).
#[derive(Clone, Debug)]
pub struct NamedSeg {
    pub seg: Seg3,
    pub name: String,
}

fn seg_points(s: &Seg3) -> ([f64; 3], [f64; 3], [f64; 3]) {
    match *s {
        Seg3::Line(a, b) => (a, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0], b),
        Seg3::Arc(a, m, b) => (a, m, b),
    }
}

fn d3(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// A planar profile face with the names of its edges (aligned with `face.subshapes(EDGE)`).
pub struct ProfileFace {
    pub face: Shape,
    pub edge_names: Vec<String>,
}

/// Builds the profile face and maps each of its edges back to the sketch segment it came from.
/// ShapeFix_Face may reorder or reorient edges, so the mapping is geometric: endpoints (in either
/// order) plus the midpoint, which separates the two arcs of a split circle.
pub fn profile_face(loops: &[Vec<NamedSeg>]) -> KResult<ProfileFace> {
    let plain: Vec<Vec<Seg3>> = loops.iter().map(|l| l.iter().map(|s| s.seg).collect()).collect();
    let face = Shape::face_from_loops(&plain)?;
    let segs: Vec<&NamedSeg> = loops.iter().flatten().collect();
    let edge_names = face
        .subshapes(EDGE)
        .iter()
        .map(|e| {
            let Some(o) = e.edge_info() else { return String::new() };
            let (s, m, t) = ([o[2], o[3], o[4]], [o[5], o[6], o[7]], [o[8], o[9], o[10]]);
            segs.iter()
                .map(|ns| {
                    let (a, mm, b) = seg_points(&ns.seg);
                    ((d3(s, a) + d3(t, b)).min(d3(s, b) + d3(t, a)) + d3(m, mm), ns)
                })
                .min_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(_, ns)| ns.name.clone())
                .unwrap_or_default()
        })
        .collect();
    Ok(ProfileFace { face, edge_names })
}

fn sweep_names(fid: &str, pf: &ProfileFace, s: &Shape, o: &[Vec<Origin>]) -> Vec<String> {
    derive(
        o,
        nfaces(s),
        |o| match o.kind {
            Kind::Same | Kind::Start => Some(format!("{fid}/start")),
            Kind::End => Some(format!("{fid}/end")),
            Kind::FromEdge => Some(format!("{fid}/side:{}", pf.edge_names.get(o.index).map(String::as_str).unwrap_or("?"))),
            _ => None,
        },
        &format!("{fid}/face"),
    )
}

/// Extrusion of a profile face along `v`.
pub fn prism(fid: &str, pf: &ProfileFace, v: [f64; 3]) -> KResult<Named> {
    let (s, o) = run("Extrusion failed", |h| ffi::cad_prism_named(pf.face.inner(), v[0], v[1], v[2], h))?;
    let names = sweep_names(fid, pf, &s, &o);
    Ok(Named { shape: s, names })
}

/// Revolution of a profile face about the axis through `origin` along `axis`.
pub fn revolve(fid: &str, pf: &ProfileFace, origin: [f64; 3], axis: [f64; 3], angle_rad: f64) -> KResult<Named> {
    let (s, o) = run("Revolve failed", |h| ffi::cad_revol_named(pf.face.inner(), origin[0], origin[1], origin[2], axis[0], axis[1], axis[2], angle_rad, h))?;
    let names = sweep_names(fid, pf, &s, &o);
    Ok(Named { shape: s, names })
}

/// Names a primitive's faces from their geometry (`name(face_info)`); for tool solids without a
/// sketch (hole cylinders).
pub fn by_geometry(shape: Shape, name: impl Fn(&[f64; 15]) -> String) -> Named {
    let names = shape.subshapes(FACE).iter().map(|f| f.face_info().map(|i| name(&i)).unwrap_or_default()).collect();
    Named { shape, names }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_decoding() {
        let h = [2, 1, 0, 0, 3, 2, 1, 2, 4, 0, 0, 0];
        let o = parse_history(&h);
        assert_eq!(o.len(), 2);
        assert_eq!(o[0], vec![Origin { input: 0, kind: Kind::Same, index: 3 }]);
        // sorted by precedence: same, cap, edge
        assert_eq!(o[1].iter().map(|o| o.kind).collect::<Vec<_>>(), vec![Kind::Same, Kind::FromEdge]);
    }
}
