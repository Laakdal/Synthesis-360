//! Reader for `research/feature-cases.json` (mm, degrees) into cad-geom inputs (mm, radians).
//! Shared by the extrude / revolve / feature tests; not public API.
#![allow(dead_code)]

use cad_geom::{Frame, Profile, Seg, P2, P3, V3};
use serde_json::Value;
use std::path::PathBuf;

pub fn load(rel: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

/// All cases of `research/feature-cases.json`.
pub fn cases() -> Vec<Value> {
    load("research/feature-cases.json")["cases"].as_array().unwrap().clone()
}

pub fn case(id: &str) -> Value {
    cases().into_iter().find(|c| c["id"] == id).unwrap_or_else(|| panic!("no case {id}"))
}

pub fn v3(v: &Value) -> V3 {
    let a = v.as_array().unwrap();
    V3::new(a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap())
}

pub fn p3(v: &Value) -> P3 {
    P3::from(v3(v))
}

fn p2(v: &Value) -> P2 {
    let a = v.as_array().unwrap();
    P2::new(a[0].as_f64().unwrap(), a[1].as_f64().unwrap())
}

fn num(v: &Value, k: &str) -> f64 {
    v[k].as_f64().unwrap_or_else(|| panic!("missing {k} in {v}"))
}

pub fn seg(s: &Value) -> Seg {
    if let Some(l) = s.get("line") {
        Seg::Line { a: p2(&l["a"]), b: p2(&l["b"]) }
    } else if let Some(a) = s.get("arc") {
        Seg::Arc { c: p2(&a["c"]), r: num(a, "r"), a0: num(a, "a0").to_radians(), sweep: num(a, "sweep").to_radians() }
    } else if let Some(c) = s.get("circle") {
        Seg::circle(p2(&c["c"]), num(c, "r"))
    } else {
        panic!("unknown segment {s}")
    }
}

pub fn profile(c: &Value) -> Profile {
    let pl = &c["profile"]["plane"];
    let (x, y) = (v3(&pl["x"]).normalize(), v3(&pl["y"]).normalize());
    let plane = Frame { origin: p3(&pl["origin"]), x, y, z: x.cross(&y) };
    let loops = c["profile"]["loops"].as_array().unwrap().iter().map(|l| l.as_array().unwrap().iter().map(seg).collect()).collect();
    Profile { plane, loops }
}

/// Closed and consistently oriented mesh: after welding positions closer than 1e-9·size, every
/// directed edge occurs exactly once and so does its reverse (same rule as `tests/tess.rs`).
pub fn assert_watertight(id: &str, m: &cad_geom::tess::Mesh) {
    use std::collections::HashMap;
    let size = m.positions.iter().map(|p| V3::from(*p).amax()).fold(1.0, f64::max);
    let tol = 1e-9 * size;
    let cell = |p: &[f64; 3]| [(p[0] / tol).floor() as i64, (p[1] / tol).floor() as i64, (p[2] / tol).floor() as i64];
    let mut grid: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
    let mut rep: Vec<usize> = Vec::with_capacity(m.positions.len());
    for (i, p) in m.positions.iter().enumerate() {
        let c = cell(p);
        let mut found = None;
        'search: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for &j in grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]).map(Vec::as_slice).unwrap_or(&[]) {
                        if (V3::from(m.positions[j]) - V3::from(*p)).norm() <= tol {
                            found = Some(rep[j]);
                            break 'search;
                        }
                    }
                }
            }
        }
        rep.push(found.unwrap_or(i));
        grid.entry(c).or_default().push(i);
    }
    let mut edges: HashMap<(usize, usize), usize> = HashMap::new();
    for t in &m.triangles {
        let v = [rep[t[0] as usize], rep[t[1] as usize], rep[t[2] as usize]];
        assert!(v[0] != v[1] && v[1] != v[2] && v[2] != v[0], "{id}: triangle {t:?} collapses after welding");
        for k in 0..3 {
            *edges.entry((v[k], v[(k + 1) % 3])).or_insert(0) += 1;
        }
    }
    for (&(a, c), &n) in &edges {
        assert_eq!(n, 1, "{id}: directed edge {a}->{c} used {n} times");
        assert_eq!(edges.get(&(c, a)).copied().unwrap_or(0), 1, "{id}: edge {a}->{c} has no twin");
    }
}
