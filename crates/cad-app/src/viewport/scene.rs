//! Builds GPU scene data (meshes + line instances) from the application state.

use super::camera::Camera;
use super::renderer::{LineInst, MeshData, CAP_COLOR};
use crate::state::sketch::{CurveState, SketchOverlay};
use crate::state::{AppState, Pick};
use cad_core::{FeatureKind, OriginPlane};
use cad_kernel::Frame;

pub const BODY: [f32; 4] = [0.64, 0.66, 0.70, 1.0];
pub const FACE_HOVER: [f32; 4] = [0.55, 0.74, 1.0, 1.0];
pub const FACE_SELECTED: [f32; 4] = [0.30, 0.56, 1.0, 1.0];
pub const EDGE: [f32; 4] = [0.04, 0.04, 0.05, 1.0];
pub const EDGE_HOVER: [f32; 4] = [0.45, 0.75, 1.0, 1.0];
pub const EDGE_SELECTED: [f32; 4] = [0.20, 0.55, 1.0, 1.0];
pub const SKETCH_FREE: [f32; 4] = [0.30, 0.62, 1.0, 1.0];
/// Fusion draws fully constrained curves black on its light canvas; on the dark canvas CAD360
/// uses near-white for the same "fully constrained" state.
pub const SKETCH_FIXED: [f32; 4] = [0.93, 0.93, 0.96, 1.0];
pub const SKETCH_CONSTRUCTION: [f32; 4] = [1.0, 0.56, 0.16, 1.0];
pub const SKETCH_SELECTED: [f32; 4] = [0.38, 0.88, 1.0, 1.0];
pub const SKETCH_HOVER: [f32; 4] = [0.65, 0.85, 1.0, 1.0];
pub const SKETCH_PREVIEW: [f32; 4] = [0.55, 0.78, 1.0, 0.9];
pub const SKETCH_ERROR: [f32; 4] = [1.0, 0.30, 0.25, 1.0];
pub const SKETCH_IDLE: [f32; 4] = [0.55, 0.62, 0.74, 1.0];
/// Projected geometry (Fusion: purple).
pub const SKETCH_PROJECTED: [f32; 4] = [0.74, 0.45, 0.95, 1.0];
pub const PROFILE_FILL: [f32; 4] = [0.36, 0.56, 0.92, 0.26];
pub const PROFILE_HOVER: [f32; 4] = [0.45, 0.70, 1.0, 0.45];
pub const PROFILE_SELECTED: [f32; 4] = [0.30, 0.60, 1.0, 0.60];

fn f3(v: [f64; 3]) -> [f32; 3] {
    [v[0] as f32, v[1] as f32, v[2] as f32]
}

/// One mesh per visible body; returns meshes and the body id of each.
pub fn build_meshes(state: &AppState) -> (Vec<MeshData>, Vec<String>) {
    let mut out = Vec::new();
    let mut ids = Vec::new();
    for b in state.visible_bodies() {
        let m = b.mesh();
        let colors = face_colors(state, &b.id, &m);
        let cap_color = cap_color(body_color(state, &b.id));
        out.push(MeshData { positions: m.positions.clone(), normals: m.normals.clone(), colors, indices: m.indices.clone(), transparent: false, cap_color });
        ids.push(b.id.clone());
    }
    (out, ids)
}

/// Section cap colour of a body: its colour pulled towards the warm cap tint.
pub fn cap_color(body: [f32; 4]) -> [f32; 4] {
    let t = 0.65;
    [body[0] + (CAP_COLOR[0] - body[0]) * t, body[1] + (CAP_COLOR[1] - body[1]) * t, body[2] + (CAP_COLOR[2] - body[2]) * t, 1.0]
}

pub fn body_color(state: &AppState, body: &str) -> [f32; 4] {
    match state.doc.bodies.get(body).and_then(|m| m.color) {
        Some(c) => [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0],
        None => BODY,
    }
}

/// Per-vertex colours with hover / selection highlighting of faces and bodies.
pub fn face_colors(state: &AppState, body: &str, m: &cad_kernel::BodyMesh) -> Vec<[f32; 4]> {
    let base = body_color(state, body);
    let n = m.positions.len() / 3;
    let mut colors = vec![base; n];
    let body_sel = state.selection.iter().any(|p| matches!(p, Pick::Body { body: b } if b == body));
    let body_hover = matches!(&state.hover, Some(Pick::Body { body: b }) if b == body);
    if body_sel || body_hover {
        colors.iter_mut().for_each(|c| *c = if body_sel { FACE_SELECTED } else { FACE_HOVER });
    }
    let mut paint = |face: usize, c: [f32; 4]| {
        if let Some(&(s, e)) = m.face_ranges.get(face) {
            for k in s..e {
                colors[m.indices[k as usize] as usize] = c;
            }
        }
    };
    if let Some(Pick::Face { body: b, face }) = &state.hover {
        if b == body {
            paint(*face, FACE_HOVER);
        }
    }
    for p in &state.selection {
        if let Pick::Face { body: b, face } = p {
            if b == body {
                paint(*face, FACE_SELECTED);
            }
        }
    }
    colors
}

/// Adaptive grid step: 1, 2 or 5 × 10^n so roughly 10-20 cells span the half-height.
pub fn grid_step(half_height: f32) -> f32 {
    let raw = (half_height / 8.0).max(1e-3);
    let p = 10f32.powf(raw.log10().floor());
    let m = raw / p;
    p * if m < 2.0 { 1.0 } else if m < 5.0 { 2.0 } else { 5.0 }
}

fn grid_on(frame: &Frame, center: [f64; 2], step: f32, extent: f32, lines: &mut Vec<LineInst>) {
    let n = (extent / step).ceil() as i32;
    let cx = (center[0] as f32 / step).round() * step;
    let cy = (center[1] as f32 / step).round() * step;
    for i in -n..=n {
        let major = ((cx / step).round() as i32 + i) % 5 == 0;
        let col = if major { [0.42, 0.44, 0.50, 0.55] } else { [0.34, 0.36, 0.41, 0.30] };
        let x = cx + i as f32 * step;
        let y = cy + i as f32 * step;
        let a = frame.to_world([x as f64, (cy - extent) as f64]);
        let b = frame.to_world([x as f64, (cy + extent) as f64]);
        lines.push(LineInst::new(f3(a), f3(b), col, 1.0));
        let a = frame.to_world([(cx - extent) as f64, y as f64]);
        let b = frame.to_world([(cx + extent) as f64, y as f64]);
        lines.push(LineInst::new(f3(a), f3(b), col, 1.0));
    }
}

fn polyline(frame: &Frame, pts: &[[f64; 2]], color: [f32; 4], width: f32, dashed: bool, on_top: bool, lines: &mut Vec<LineInst>) {
    for w in pts.windows(2) {
        let mut l = LineInst::new(f3(frame.to_world(w[0])), f3(frame.to_world(w[1])), color, width);
        if dashed {
            l = l.dashed(10.0);
        }
        if on_top {
            l = l.on_top();
        }
        lines.push(l);
    }
}

fn curve_color(s: CurveState) -> ([f32; 4], f32) {
    match s {
        CurveState::Free => (SKETCH_FREE, 2.0),
        CurveState::Fixed => (SKETCH_FIXED, 2.0),
        CurveState::Construction => (SKETCH_CONSTRUCTION, 1.6),
        CurveState::Hovered => (SKETCH_HOVER, 3.0),
        CurveState::Selected => (SKETCH_SELECTED, 3.2),
        CurveState::Preview => (SKETCH_PREVIEW, 1.8),
        CurveState::Error => (SKETCH_ERROR, 2.4),
    }
}

/// Triangulates polygons-with-holes into a transparent mesh.
pub fn fill_mesh(frame: &Frame, regions: &[(Vec<Vec<[f64; 2]>>, [f32; 4])]) -> Option<MeshData> {
    let mut m = MeshData::new(true);
    for (loops, color) in regions {
        let mut flat: Vec<f64> = Vec::new();
        let mut holes = Vec::new();
        for (k, l) in loops.iter().enumerate() {
            if k > 0 {
                holes.push(flat.len() / 2);
            }
            for p in l {
                flat.push(p[0]);
                flat.push(p[1]);
            }
        }
        let Ok(tris) = earcutr::earcut(&flat, &holes, 2) else { continue };
        let base = (m.positions.len() / 3) as u32;
        for p in flat.chunks(2) {
            let w = frame.to_world([p[0], p[1]]);
            // lift fills a hair off the plane towards the normal to avoid z-fighting with faces
            let w = [w[0] + frame.n[0] * 0.01, w[1] + frame.n[1] * 0.01, w[2] + frame.n[2] * 0.01];
            m.positions.extend_from_slice(&f3(w));
            m.normals.extend_from_slice(&f3(frame.n));
            m.colors.push(*color);
        }
        m.indices.extend(tris.iter().map(|&i| base + i as u32));
    }
    (!m.indices.is_empty()).then_some(m)
}

pub struct LineContext<'a> {
    pub cam: &'a Camera,
    pub plane_prompt: bool,
    pub overlay: Option<&'a SketchOverlay>,
}

/// Size of the origin-plane squares (also used for picking).
pub fn origin_plane_half(cam: &Camera) -> f32 {
    (cam.half_height() * 0.35).max(5.0)
}

/// Returns the line instances and the transparent fill meshes (profiles, plane squares).
pub fn build_lines(state: &AppState, ctx: &LineContext) -> (Vec<LineInst>, Vec<MeshData>) {
    let mut lines = Vec::new();
    let mut fill_meshes: Vec<MeshData> = Vec::new();
    let hh = ctx.cam.half_height();
    let step = grid_step(hh);

    // grid (model mode: XY ground plane; sketch mode: the sketch plane)
    if state.settings.grid {
        match ctx.overlay {
            Some(o) => {
                let t = ctx.cam.target;
                let c = o.frame.to_local([t.x as f64, t.y as f64, t.z as f64]);
                grid_on(&o.frame, c, step, hh * 2.5, &mut lines);
            }
            None => {
                let f = Frame::origin_plane(OriginPlane::XY);
                let t = ctx.cam.target;
                grid_on(&f, [t.x as f64, t.y as f64], step, hh * 2.5, &mut lines);
            }
        }
    }

    // origin axes
    let ax = hh * 3.0;
    lines.push(LineInst::new([0.0, 0.0, 0.0], [ax, 0.0, 0.0], [0.86, 0.27, 0.27, 0.9], 1.4));
    lines.push(LineInst::new([0.0, 0.0, 0.0], [0.0, ax, 0.0], [0.33, 0.76, 0.33, 0.9], 1.4));
    lines.push(LineInst::new([0.0, 0.0, 0.0], [0.0, 0.0, ax * 0.5], [0.35, 0.48, 0.95, 0.9], 1.4));

    // origin planes (Create Sketch prompt or Origin made visible in the browser)
    if ctx.plane_prompt || state.doc.origin_visible {
        let h = origin_plane_half(ctx.cam) as f64;
        for p in [OriginPlane::XY, OriginPlane::XZ, OriginPlane::YZ] {
            let f = Frame::origin_plane(p);
            let hovered = matches!(state.hover, Some(Pick::OriginPlane(q)) if q == p);
            let col = if hovered { [0.55, 0.78, 1.0, 1.0] } else { [0.85, 0.65, 0.25, 0.9] };
            let sq = [[-h, -h], [h, -h], [h, h], [-h, h], [-h, -h]];
            polyline(&f, &sq, col, if hovered { 2.5 } else { 1.5 }, false, false, &mut lines);
            let fill = [(vec![sq[..4].to_vec()], if hovered { [0.45, 0.70, 1.0, 0.30] } else { [0.85, 0.65, 0.25, 0.10] })];
            fill_meshes.extend(fill_mesh(&f, &fill));
        }
    }

    // construction planes
    for (fid, fr) in &state.model.planes {
        let visible = state.doc.feature(fid).is_some_and(|f| matches!(f.kind, FeatureKind::Plane { visible: true, .. }));
        if !visible {
            continue;
        }
        let h = origin_plane_half(ctx.cam) as f64;
        let sel = state.selection.iter().any(|p| matches!(p, Pick::ConstructionPlane(q) if q == fid)) || matches!(&state.hover, Some(Pick::ConstructionPlane(q)) if q == fid);
        let sq = [[-h, -h], [h, -h], [h, h], [-h, h], [-h, -h]];
        polyline(fr, &sq, if sel { [0.45, 0.75, 1.0, 1.0] } else { [0.95, 0.75, 0.30, 0.9] }, 1.6, false, false, &mut lines);
    }

    // body edges
    for b in state.visible_bodies() {
        let m = b.mesh();
        for (ei, poly) in m.edge_lines.iter().enumerate() {
            let sel = state.selection.iter().any(|p| matches!(p, Pick::Edge { body, edge } if body == &b.id && *edge == ei));
            let hov = matches!(&state.hover, Some(Pick::Edge { body, edge }) if body == &b.id && *edge == ei);
            let (c, w) = if sel { (EDGE_SELECTED, 3.0) } else if hov { (EDGE_HOVER, 3.0) } else { (EDGE, 1.3) };
            for wn in poly.windows(2) {
                lines.push(LineInst::new(wn[0], wn[1], c, w));
            }
        }
    }

    // finished, visible sketches (model mode) with selectable profiles
    let active = state.active_sketch().map(|s| s.feature.clone());
    for f in &state.doc.features {
        let FeatureKind::Sketch { visible, .. } = &f.kind else { continue };
        // hidden components hide their sketches too
        if !*visible || Some(&f.id) == active.as_ref() || !state.doc.is_active(&f.id) || !state.doc.component_visible(f.component.as_deref()) {
            continue;
        }
        let Some(r) = state.model.sketches.get(&f.id) else { continue };
        for pl in &r.projected {
            polyline(&r.frame, pl, SKETCH_PROJECTED, 1.2, false, false, &mut lines);
        }
        for e in &r.solved.entities {
            let poly = cad_core::sketch::profiles::entity_polyline(&r.solved, e);
            if poly.len() >= 2 {
                polyline(&r.frame, &poly, if e.construction { SKETCH_CONSTRUCTION } else { SKETCH_IDLE }, 1.5, e.construction, false, &mut lines);
            }
        }
        let mut regions = Vec::new();
        for (i, p) in r.profiles.iter().enumerate() {
            let sel = state.selection.iter().any(|q| matches!(q, Pick::Profile { sketch, index } if sketch == &f.id && *index == i));
            let hov = matches!(&state.hover, Some(Pick::Profile { sketch, index }) if sketch == &f.id && *index == i);
            if !(sel || hov || state.settings.show_profiles) {
                continue;
            }
            let loops = std::iter::once(p.outer.polygon()).chain(p.holes.iter().map(|h| h.polygon())).collect();
            regions.push((loops, if sel { PROFILE_SELECTED } else if hov { PROFILE_HOVER } else { PROFILE_FILL }));
        }
        if let Some(m) = fill_mesh(&r.frame, &regions) {
            fill_meshes.push(m);
        }
    }

    // inspect markers (Measure closest points, centre of mass)
    for m in state.inspect_markers() {
        match m {
            crate::state::features::InspectMarker::Point(p) => lines.push(LineInst::point(f3(p), [1.0, 0.82, 0.25, 1.0], 9.0).on_top()),
            crate::state::features::InspectMarker::Segment(a, b) => lines.push(LineInst::new(f3(a), f3(b), [1.0, 0.82, 0.25, 1.0], 2.0).on_top()),
        }
    }

    // active sketch overlay
    if let Some(o) = ctx.overlay {
        if let Some(r) = state.active_sketch().and_then(|a| state.model.sketches.get(&a.feature)) {
            for pl in &r.projected {
                polyline(&o.frame, pl, SKETCH_PROJECTED, 1.6, false, false, &mut lines);
            }
        }
        for c in &o.curves {
            let (col, w) = curve_color(c.state);
            polyline(&o.frame, &c.polyline, col, w, c.dashed, c.state == CurveState::Preview, &mut lines);
        }
        if state.settings.show_points {
            for p in &o.points {
                let (col, _) = curve_color(p.state);
                lines.push(LineInst::point(f3(o.frame.to_world(p.pos)), col, if matches!(p.state, CurveState::Hovered | CurveState::Selected) { 8.0 } else { 5.0 }).on_top());
            }
        }
        if state.settings.show_dimensions {
            for d in &o.dimensions {
                for (a, b) in &d.lines {
                    let col = if d.selected { SKETCH_SELECTED } else { [0.80, 0.82, 0.86, 0.9] };
                    lines.push(LineInst::new(f3(o.frame.to_world(*a)), f3(o.frame.to_world(*b)), col, 1.0).on_top());
                }
            }
        }
        if state.settings.show_profiles {
            let regions: Vec<_> = o.profiles.iter().map(|p| (p.clone(), PROFILE_FILL)).collect();
            if let Some(m) = fill_mesh(&o.frame, &regions) {
                fill_meshes.push(m);
            }
        }
    }
    (lines, fill_meshes)
}
