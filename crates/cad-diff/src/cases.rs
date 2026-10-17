//! The shared case catalogues: `research/cases.json` (primitives, booleans) and
//! `research/feature-cases.json` (extrude, revolve, fillet, chamfer, shell). Units: mm, degrees.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Solid specification, tagged by `kind` exactly as in `research/cases.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Spec {
    /// Axis-aligned box: corner + edge lengths.
    Box {
        corner: [f64; 3],
        size: [f64; 3],
    },
    /// Right circular cylinder (r0 == r1).
    Cylinder {
        base: [f64; 3],
        axis: [f64; 3],
        r0: f64,
        r1: f64,
        h: f64,
    },
    /// Cone (r1 == 0) or frustum.
    Cone {
        base: [f64; 3],
        axis: [f64; 3],
        r0: f64,
        r1: f64,
        h: f64,
    },
    Sphere {
        center: [f64; 3],
        r: f64,
    },
    Torus {
        center: [f64; 3],
        axis: [f64; 3],
        major: f64,
        minor: f64,
    },
    /// Planar profile swept along its plane normal between offsets `from` and `to`
    /// (`research/feature-cases.json`).
    Extrude {
        profile: ProfileSpec,
        from: f64,
        to: f64,
    },
    /// Planar profile revolved about `axis` by `angle` degrees (right-hand rule).
    Revolve {
        profile: ProfileSpec,
        axis: AxisSpec,
        angle: f64,
    },
    /// Constant-radius fillet of the edges through the given world points of `base`'s body.
    Fillet {
        base: BaseRef,
        radius: f64,
        edges: Vec<[f64; 3]>,
    },
    /// Equal-distance chamfer of the edges through the given points.
    Chamfer {
        base: BaseRef,
        distance: f64,
        edges: Vec<[f64; 3]>,
    },
    /// Hollow `base` to wall `thickness`, removing the faces through the given points.
    Shell {
        base: BaseRef,
        thickness: f64,
        direction: String,
        faces: Vec<[f64; 3]>,
    },
    /// `tools` are applied to `target` in order with `op` (union, cut, intersect).
    Boolean {
        op: BoolOp,
        target: Box<Spec>,
        tools: Vec<Spec>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoolOp {
    Union,
    Cut,
    Intersect,
}

/// Profile plane: origin and unit x, y (normal = x × y).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaneSpec {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
}

/// One profile segment in plane coordinates (mm; angles in degrees).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegSpec {
    Line { a: [f64; 2], b: [f64; 2] },
    Arc { c: [f64; 2], r: f64, a0: f64, sweep: f64 },
    Circle { c: [f64; 2], r: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileSpec {
    pub plane: PlaneSpec,
    pub loops: Vec<Vec<SegSpec>>,
}

impl ProfileSpec {
    /// World point of plane coordinates `p` at offset `z` along the normal.
    pub fn world(&self, p: [f64; 2], z: f64) -> [f64; 3] {
        let n = self.normal();
        let (o, x, y) = (self.plane.origin, self.plane.x, self.plane.y);
        [0, 1, 2].map(|i| o[i] + x[i] * p[0] + y[i] * p[1] + n[i] * z)
    }

    pub fn normal(&self) -> [f64; 3] {
        let (x, y) = (self.plane.x, self.plane.y);
        [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AxisSpec {
    pub origin: [f64; 3],
    pub dir: [f64; 3],
}

/// The body a fillet/chamfer/shell acts on: a case id in the file, resolved to its spec by
/// [`Catalogue::resolve_bases`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BaseRef {
    Spec(Box<Spec>),
    Id(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Case {
    pub id: String,
    #[serde(flatten)]
    pub spec: Spec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalogue {
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub version: u32,
    pub cases: Vec<Case>,
}

impl Catalogue {
    pub fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("invalid case catalogue: {e}"))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))?;
        Self::parse(&text)
    }

    /// `research/cases.json` followed by the feature cases of `research/feature-cases.json`,
    /// with every `base` reference resolved.
    pub fn load_all(root: &Path) -> Result<Self, String> {
        let research = root.join("research");
        let mut cat = Self::load(&research.join("cases.json"))?;
        let features = Self::load(&research.join("feature-cases.json"))?;
        cat.cases.extend(features.cases);
        cat.resolve_bases()?;
        Ok(cat)
    }

    /// Replaces each `base` case id by that case's spec (bases must be listed earlier).
    pub fn resolve_bases(&mut self) -> Result<(), String> {
        for i in 0..self.cases.len() {
            let id = match &self.cases[i].spec {
                Spec::Fillet { base: BaseRef::Id(id), .. }
                | Spec::Chamfer { base: BaseRef::Id(id), .. }
                | Spec::Shell { base: BaseRef::Id(id), .. } => id.clone(),
                _ => continue,
            };
            let spec = self.cases[..i]
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.spec.clone())
                .ok_or_else(|| format!("case {}: base {id} is not an earlier case", self.cases[i].id))?;
            if let Spec::Fillet { base, .. } | Spec::Chamfer { base, .. } | Spec::Shell { base, .. } =
                &mut self.cases[i].spec
            {
                *base = BaseRef::Spec(Box::new(spec));
            }
        }
        Ok(())
    }
}

impl Spec {
    pub fn is_boolean(&self) -> bool {
        matches!(self, Spec::Boolean { .. })
    }

    /// Extrude, revolve, fillet, chamfer or shell (`research/feature-cases.json`).
    pub fn is_feature(&self) -> bool {
        matches!(
            self,
            Spec::Extrude { .. }
                | Spec::Revolve { .. }
                | Spec::Fillet { .. }
                | Spec::Chamfer { .. }
                | Spec::Shell { .. }
        )
    }

    /// The resolved base body of a fillet / chamfer / shell.
    pub fn base(&self) -> Option<&Spec> {
        match self {
            Spec::Fillet { base, .. } | Spec::Chamfer { base, .. } | Spec::Shell { base, .. } => {
                match base {
                    BaseRef::Spec(s) => Some(s),
                    BaseRef::Id(_) => None,
                }
            }
            _ => None,
        }
    }

    /// Closed-form volume for primitives (None for booleans and features).
    pub fn closed_form_volume(&self) -> Option<f64> {
        use std::f64::consts::PI;
        match *self {
            Spec::Box { size, .. } => Some(size[0] * size[1] * size[2]),
            Spec::Cylinder { r0, h, .. } => Some(PI * r0 * r0 * h),
            Spec::Cone { r0, r1, h, .. } => Some(PI * h * (r0 * r0 + r0 * r1 + r1 * r1) / 3.0),
            Spec::Sphere { r, .. } => Some(4.0 / 3.0 * PI * r * r * r),
            Spec::Torus { major, minor, .. } => Some(2.0 * PI * PI * major * minor * minor),
            _ => None,
        }
    }
}

/// Repository root (two levels above this crate's manifest).
pub fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}
