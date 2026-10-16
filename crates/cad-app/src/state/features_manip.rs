//! Read-only accessors for the viewport's on-canvas manipulators (Fusion's drag arrows): where
//! the handle of the open feature dialog sits, which way it points and the value it drives.
//! The interaction itself lives in `viewport/manipulator.rs`; values are committed through
//! [`AppState::dialog_set_primary`] like a typed value.

use super::profiles::resolve_profile;
use super::{eval_expr, Dialog, ExprField};
use crate::state::AppState;
use crate::viewport::manipulator::AxisHandle;
use cad_core::{ExtentDirection, FeatureKind, Quantity, SurfaceKind};
use cad_kernel::frame::{add, dot, normalize, scale, sub};
use cad_kernel::{refs, Body};

/// Area and centroid of a polygon (shoelace); area is unsigned.
fn polygon_centroid(p: &[[f64; 2]]) -> (f64, [f64; 2]) {
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for i in 0..p.len() {
        let (p0, p1) = (p[i], p[(i + 1) % p.len()]);
        let c = p0[0] * p1[1] - p1[0] * p0[1];
        a += c;
        cx += (p0[0] + p1[0]) * c;
        cy += (p0[1] + p1[1]) * c;
    }
    if a.abs() < 1e-12 {
        let n = p.len().max(1) as f64;
        return (0.0, [p.iter().map(|q| q[0]).sum::<f64>() / n, p.iter().map(|q| q[1]).sum::<f64>() / n]);
    }
    (a.abs() * 0.5, [cx / (3.0 * a), cy / (3.0 * a)])
}

/// Outward mesh normal of `face` at the vertex closest to `p` (works for curved faces too).
fn face_normal_near(body: &Body, face: usize, p: [f64; 3]) -> Option<[f64; 3]> {
    let m = body.mesh();
    let &(s, e) = m.face_ranges.get(face)?;
    let mut best: Option<(f64, usize)> = None;
    for k in s..e {
        let v = m.indices[k as usize] as usize;
        let q = [m.positions[v * 3] as f64, m.positions[v * 3 + 1] as f64, m.positions[v * 3 + 2] as f64];
        let d = dot(sub(q, p), sub(q, p));
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, v));
        }
    }
    let v = best?.1;
    Some(normalize([m.normals[v * 3] as f64, m.normals[v * 3 + 1] as f64, m.normals[v * 3 + 2] as f64]))
}

impl AppState {
    /// Current value of an expression field (mm), falling back to the feature parameter when the
    /// text is invalid.
    fn manip_value(&self, field: &ExprField, param: Option<&str>) -> Option<f64> {
        eval_expr(&self.doc, &field.text, Quantity::Length).ok().or_else(|| param.and_then(|p| self.doc.parameter_values().values.get(p).copied()))
    }

    /// Drag handle of the open feature dialog, if it has one: Extrude (distance arrow at the
    /// profiles' centroid), Offset Plane (arrow along the plane normal), Fillet / Chamfer
    /// (arrow at the first edge's midpoint along the bisector of the adjacent face normals) and
    /// Shell (arrow into the material at the first removed face).
    pub fn manipulator_handle(&self) -> Option<AxisHandle> {
        if !matches!(self.mode, super::Mode::Model) {
            return None;
        }
        match self.dialog.as_ref()? {
            Dialog::Extrude(x) if !x.through_all && !x.profiles.is_empty() => {
                let sk = x.sketch.as_deref()?;
                let sr = self.model.sketches.get(sk).or_else(|| x.s.base.0.sketches.get(sk))?;
                let (mut area, mut c) = (0.0, [0.0, 0.0]);
                for r in &x.profiles {
                    let Some(i) = resolve_profile(sr, r) else { continue };
                    let p = &sr.profiles[i];
                    let (mut a, oc) = polygon_centroid(&p.outer.polygon());
                    let mut m = [a * oc[0], a * oc[1]];
                    for h in &p.holes {
                        let (ha, hc) = polygon_centroid(&h.polygon());
                        a -= ha;
                        m[0] -= ha * hc[0];
                        m[1] -= ha * hc[1];
                    }
                    area += a;
                    c[0] += m[0];
                    c[1] += m[1];
                }
                if area <= 1e-12 {
                    return None;
                }
                let origin = sr.frame.to_world([c[0] / area, c[1] / area]);
                let param = x.s.feature.as_deref().and_then(|f| self.doc.feature(f)).and_then(|f| match &f.kind {
                    FeatureKind::Extrude { distance, .. } => Some(distance.clone()),
                    _ => None,
                });
                let value = self.manip_value(&x.distance, param.as_deref())?;
                let n = sr.frame.n;
                let (dir, scale_) = match x.direction {
                    ExtentDirection::Normal => (n, 1.0),
                    ExtentDirection::Reverse => (scale(n, -1.0), 1.0),
                    // symmetric: the distance is the total length, the arrow shows one half
                    ExtentDirection::Symmetric => (n, 2.0),
                };
                Some(AxisHandle { origin, dir, value, scale: scale_, positive: x.direction == ExtentDirection::Symmetric, live: true })
            }
            Dialog::OffsetPlane(x) => {
                let fid = x.s.feature.as_deref()?;
                let fr = self.model.planes.get(fid)?;
                let param = match &self.doc.feature(fid)?.kind {
                    FeatureKind::Plane { offset, .. } => Some(offset.clone()),
                    _ => None,
                };
                let value = self.manip_value(&x.offset, param.as_deref())?;
                Some(AxisHandle { origin: sub(fr.origin, scale(fr.n, value)), dir: fr.n, value, scale: 1.0, positive: false, live: true })
            }
            Dialog::Fillet(x) | Dialog::Chamfer(x) => {
                let r = x.edges.first()?;
                let b0 = x.s.base.0.body(&r.body)?;
                let j = refs::resolve_edge(b0, r).ok()?;
                let e = b0.topology().edge_info.get(j)?.clone();
                let mut bis = [0.0; 3];
                for f in e.faces.iter().filter(|&&f| f >= 0) {
                    bis = add(bis, face_normal_near(b0, *f as usize, e.mid)?);
                }
                if dot(bis, bis) < 1e-8 {
                    return None;
                }
                let value = self.manip_value(&x.value, None)?;
                Some(AxisHandle { origin: e.mid, dir: normalize(bis), value, scale: 1.0, positive: true, live: false })
            }
            Dialog::Shell(x) => {
                let r = x.faces.first()?;
                let b0 = x.s.base.0.body(&r.body)?;
                let j = refs::resolve_face(b0, r).ok()?;
                let fi = b0.topology().face_info.get(j)?.clone();
                if fi.surface != SurfaceKind::Plane {
                    return None;
                }
                let value = self.manip_value(&x.thickness, None)?;
                Some(AxisHandle { origin: fi.centroid, dir: scale(normalize(fi.normal), -1.0), value, scale: 1.0, positive: true, live: false })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::polygon_centroid;

    #[test]
    fn centroid_of_rectangle() {
        let (a, c) = polygon_centroid(&[[0.0, 0.0], [50.0, 0.0], [50.0, 30.0], [0.0, 30.0]]);
        assert!((a - 1500.0).abs() < 1e-9 && (c[0] - 25.0).abs() < 1e-9 && (c[1] - 15.0).abs() < 1e-9);
        // clockwise gives the same
        let (a2, c2) = polygon_centroid(&[[0.0, 30.0], [50.0, 30.0], [50.0, 0.0], [0.0, 0.0]]);
        assert!((a2 - a).abs() < 1e-9 && (c2[0] - 25.0).abs() < 1e-9);
    }
}
