//! Tolerance policy. Kernels differ here, so every comparison names the tolerance it used:
//!
//! | preset              | linear (mm) | angular (rad) | source                                                         |
//! |---------------------|-------------|---------------|----------------------------------------------------------------|
//! | [`Tolerance::occt`] | 1e-7        | 1e-12         | OCCT `Precision::Confusion()` / `Precision::Angular()`         |
//! | [`Tolerance::asm`]  | 1e-5        | 1e-10         | ASM, measured: `research/evidence/asm-v5/reference.json` `global` (Fusion `pointTolerance` 1e-6 cm, `vectorAngleTolerance`) |
//!
//! `Tolerance::default()` is the OCCT preset: the curve/surface intersection routines and their
//! tests (`tests/geometry.rs`, e.g. "a 1e-6 mm cut of a sphere gives two points") are specified
//! at 1e-7 mm, and switching the default to 1e-5 mm breaks that recorded behaviour. Booleans,
//! whose parity target is ASM, use [`Tolerance::asm`] instead (see [`crate::boolean::BOOLEAN_TOLERANCE`]):
//! ASM treats a 1e-5 mm overlap as contact (`bool.union.near_tangent_cylinder`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tolerance {
    /// Two points closer than this are the same point (mm).
    pub linear: f64,
    /// Two directions closer than this are parallel (radians).
    pub angular: f64,
}

impl Default for Tolerance {
    /// The OCCT preset (see the module docs for why the default is not ASM's).
    fn default() -> Self {
        Self::occt()
    }
}

impl Tolerance {
    /// ASM's measured global tolerances: point 1e-5 mm, vector angle 1e-10 rad
    /// (`research/evidence/asm-v5/reference.json`, `global`; `research/REFERENCE_BEHAVIOR.md`,
    /// "Size limits and tolerances"). The default for booleans.
    pub const fn asm() -> Self {
        Self { linear: 1e-5, angular: 1e-10 }
    }

    /// OCCT's `Precision::Confusion()` (1e-7 mm) and `Precision::Angular()` (1e-12 rad).
    pub const fn occt() -> Self {
        Self { linear: 1e-7, angular: 1e-12 }
    }

    /// Linear tolerance scaled to a model of the given characteristic size, never below the
    /// absolute default: `max(linear, size * 1e-12)`.
    pub fn for_size(size: f64) -> Self {
        let d = Self::default();
        Self { linear: d.linear.max(size.abs() * 1e-12), ..d }
    }

    pub fn same_point(&self, a: &crate::P3, b: &crate::P3) -> bool {
        (a - b).norm() <= self.linear
    }
}
