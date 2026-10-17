//! Loader for the ASM (Autodesk ShapeManager, via Fusion) reference data in
//! `research/evidence/asm-v5/reference.json`. Parsing is lenient: every field is optional, numbers
//! may be integers or floats, type lists may be arrays or `{type: count}` maps, points may be
//! `[x,y,z]` or `{x,y,z}`, and bounding boxes `[[min],[max]]`, `[6 numbers]` or `{min, max}`.

use crate::result::{KERNEL_ASM, KernelResult, normalize_curve_type, normalize_surface_type};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

pub const ASM_UNAVAILABLE: &str = "ASM reference not available";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Global {
    pub point_tolerance_mm: Option<f64>,
    pub vector_angle_tolerance_rad: Option<f64>,
}

/// Raw file layout; `cases` stay JSON values and are normalised by [`case_result`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AsmReference {
    pub kernel: Option<String>,
    pub source: Option<String>,
    pub fusion_version: Option<String>,
    pub units: Option<String>,
    pub global: Option<Global>,
    pub cases: Vec<Value>,
    pub extras: Vec<Value>,
}

impl AsmReference {
    pub fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("invalid ASM reference: {e}"))
    }

    /// `Ok(None)` when the file does not exist.
    pub fn load(path: &Path) -> Result<Option<Self>, String> {
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))?;
        Self::parse(&text).map(Some)
    }

    /// Normalised rows, one per case that has an `id`.
    pub fn results(&self) -> Vec<KernelResult> {
        let units_note = self
            .units
            .as_deref()
            .filter(|u| !u.eq_ignore_ascii_case("mm"))
            .map(|u| format!("units declared as '{u}', not mm: values used as given"));
        self.cases
            .iter()
            .filter_map(|c| {
                let mut r = case_result(c)?;
                if let Some(n) = &units_note {
                    r.notes.push(n.clone());
                }
                Some(r)
            })
            .collect()
    }
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn count(v: Option<&Value>) -> Option<u32> {
    v.and_then(num)
        .filter(|x| *x >= 0.0)
        .map(|x| x.round() as u32)
}

fn flag(v: Option<&Value>) -> Option<bool> {
    match v? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_f64().map(|x| x != 0.0),
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => Some(true),
            "false" | "no" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn point(v: &Value) -> Option<[f64; 3]> {
    match v {
        Value::Array(a) if a.len() == 3 => Some([num(&a[0])?, num(&a[1])?, num(&a[2])?]),
        Value::Object(o) => Some([num(o.get("x")?)?, num(o.get("y")?)?, num(o.get("z")?)?]),
        _ => None,
    }
}

fn bbox(v: &Value) -> Option<[[f64; 3]; 2]> {
    match v {
        Value::Array(a) if a.len() == 2 => Some([point(&a[0])?, point(&a[1])?]),
        Value::Array(a) if a.len() == 6 => {
            let n: Option<Vec<f64>> = a.iter().map(num).collect();
            let n = n?;
            Some([[n[0], n[1], n[2]], [n[3], n[4], n[5]]])
        }
        Value::Object(o) => {
            let lo = o.get("min").or_else(|| o.get("minPoint"))?;
            let hi = o.get("max").or_else(|| o.get("maxPoint"))?;
            Some([point(lo)?, point(hi)?])
        }
        _ => None,
    }
}

/// Inertia about the centroid: 3x3 array, `[xx, yy, zz, xy, yz, xz]`, or an object with those keys.
fn inertia(v: &Value) -> Option<[[f64; 3]; 3]> {
    let from6 = |xx: f64, yy: f64, zz: f64, xy: f64, yz: f64, xz: f64| {
        [[xx, xy, xz], [xy, yy, yz], [xz, yz, zz]]
    };
    match v {
        Value::Array(a) if a.len() == 3 => Some([point(&a[0])?, point(&a[1])?, point(&a[2])?]),
        Value::Array(a) if a.len() == 6 => {
            let n: Vec<f64> = a.iter().map(num).collect::<Option<_>>()?;
            Some(from6(n[0], n[1], n[2], n[3], n[4], n[5]))
        }
        Value::Object(o) => {
            let g = |k: &str| o.get(k).and_then(num);
            Some(from6(
                g("xx")?,
                g("yy")?,
                g("zz")?,
                g("xy")?,
                g("yz")?,
                g("xz")?,
            ))
        }
        _ => None,
    }
}

/// Type multiset from an array of names or a `{name: count}` map, normalised and sorted.
fn types(v: Option<&Value>, norm: fn(&str) -> String) -> Option<(Vec<String>, Vec<String>)> {
    let mut raw = Vec::new();
    match v? {
        Value::Array(a) => raw.extend(a.iter().map(|x| {
            x.as_str()
                .map_or_else(|| "none".to_string(), str::to_string)
        })),
        Value::Object(o) => {
            for (k, n) in o {
                for _ in 0..count(Some(n)).unwrap_or(0) {
                    raw.push(k.clone());
                }
            }
        }
        _ => return None,
    }
    let mut out: Vec<String> = raw.iter().map(|s| norm(s)).collect();
    out.sort();
    Some((out, raw))
}

/// "… + N truly degenerate edge(s)" in the probe's notes → N.
fn truly_degenerate(notes: &str) -> Option<u32> {
    let i = notes.find(" truly degenerate edge")?;
    let digits: String = notes[..i]
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().ok()
}

/// One reference case → normalised row (None without an `id`).
pub fn case_result(c: &Value) -> Option<KernelResult> {
    let o = c.as_object()?;
    let id = o.get("id")?.as_str()?;
    let mut r = KernelResult::new(KERNEL_ASM, id);
    r.ok = flag(o.get("ok")).unwrap_or(o.get("error").is_none_or(Value::is_null));
    r.error = o.get("error").and_then(|e| {
        if e.is_null() {
            None
        } else {
            Some(e.as_str().map_or_else(|| e.to_string(), str::to_string))
        }
    });
    r.bodies = count(o.get("bodies"));
    r.solid = flag(o.get("solid"));
    r.lumps = count(o.get("lumps"));
    r.shells = count(o.get("shells"));
    r.faces = count(o.get("faces"));
    r.edges = count(o.get("edges"));
    r.vertices = count(o.get("vertices"));
    r.loops = count(o.get("loops"));
    if let Some((t, raw)) = types(o.get("face_types"), normalize_surface_type) {
        r.face_types = Some(t);
        r.notes.push(format!("raw face_types: {raw:?}"));
    }
    if let Some((t, raw)) = types(o.get("edge_types"), normalize_curve_type) {
        // degenerate edges are counted separately, as in the other runners
        r.edge_types = Some(t.into_iter().filter(|x| x != "degenerate").collect());
        r.notes.push(format!("raw edge_types: {raw:?}"));
    }
    // The probe's `degenerate_edges` uses the literal definition (start == end vertex, zero
    // length or no geometry: closed circles count); the truly degenerate count is in its notes.
    r.degenerate_edges_literal = count(o.get("degenerate_edges"));
    r.seam_edges = count(o.get("seam_edges"));
    if let Some(Value::String(n)) = o.get("notes") {
        r.degenerate_edges = truly_degenerate(n);
    }
    r.volume = o.get("volume").and_then(num);
    r.area = o.get("area").and_then(num);
    r.centroid = o.get("centroid").and_then(point);
    r.inertia_centroid = o.get("inertia_centroid").and_then(inertia);
    r.bbox = o.get("bbox").and_then(bbox);
    r.valid = flag(o.get("valid"));
    r.max_edge_tolerance = o.get("max_edge_tolerance").and_then(num);
    r.max_vertex_tolerance = o.get("max_vertex_tolerance").and_then(num);
    r.mass_path = o
        .get("mass_path")
        .and_then(|v| v.as_str().map(str::to_string));
    match o.get("notes") {
        Some(Value::String(s)) => r.notes.push(s.clone()),
        Some(Value::Array(a)) => r.notes.extend(
            a.iter()
                .map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_string)),
        ),
        _ => {}
    }
    Some(r)
}
