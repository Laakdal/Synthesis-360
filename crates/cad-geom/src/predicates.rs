//! Robust orientation predicates with a tolerance band and an explicit three-way result.
//!
//! The question "is `p` above, on or below the plane through `o` with normal `n`, where *on*
//! means within `tol`?" reduces to the sign of |d| − tol for the signed distance
//! d = Σᵢ nᵢ (pᵢ − oᵢ) (exact when `n` is a unit vector; for any other `n` the band is
//! `tol·|n|`, as the caller chose). The inputs are taken as exact floating-point numbers: the
//! predicate decides the question for the given `n`, `p` and `o`, not for the real numbers
//! they were rounded from.
//!
//! **Filter.** d is first evaluated in plain floating point. By the standard error analysis of
//! summation and dot products (Higham, *Accuracy and Stability of Numerical Algorithms*, §3.1,
//! with u = 2⁻⁵³ and γₖ = ku/(1 − ku)), one rounding per subtraction and a K-term dot product
//! give |fl(d) − d| ≤ γ_{K+1} Σ |nᵢ| |pᵢ − oᵢ|. The bound used is
//! `FILTER_COEF · Σ |nᵢ| |fl(pᵢ − oᵢ)|` with `FILTER_COEF` = 8u ≥ γ₄(1 + u)² (the extra
//! factors cover the rounding of the bound itself and of fl(pᵢ − oᵢ)). The answer is certain
//! when |fl(d)| − bound > tol (Above/Below) or |fl(d)| + bound ≤ tol (On).
//!
//! **Fallback.** Inside the uncertain band d is recomputed with the compensated dot product
//! Dot2 of Ogita, Rump & Oishi ("Accurate sum and dot product", SIAM J. Sci. Comput. 26, 2005):
//! the differences pᵢ − oᵢ are split exactly by TwoSum, the products by TwoProduct (via fused
//! multiply–add), and the result is as accurate as if computed in twice the working precision
//! and then rounded (error ≤ u|d| + γ²_{2K} Σ|nᵢ||pᵢ − oᵢ|). The tolerance comparison is then
//! made on that value; a tie at exactly ±tol counts as On. Exact (arbitrary-precision)
//! arithmetic is not used: with a tolerance band of 1e-5 mm the residual error (~1e-32 relative)
//! cannot change a decision except for distances within that error of ±tol.

use crate::{P3, V3};
use serde::{Deserialize, Serialize};

/// Three-way result of an orientation test with a tolerance band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    /// Signed distance > tol (on the side the normal points to).
    Above,
    /// |signed distance| ≤ tol.
    On,
    /// Signed distance < −tol.
    Below,
}

/// u = 2⁻⁵³, the unit round-off of f64.
const U: f64 = f64::EPSILON / 2.0;
/// Coefficient of the forward error bound of the filter (see module docs).
pub const FILTER_COEF: f64 = 8.0 * U;

fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    (p, a.mul_add(b, -p))
}

/// Σᵢ nᵢ (pᵢ − oᵢ) by Dot2 (compensated: as accurate as twice the working precision).
pub fn dot_diff_accurate<const K: usize>(n: &[f64; K], p: &[f64; K], o: &[f64; K]) -> f64 {
    let (mut s, mut c) = (0.0, 0.0);
    for i in 0..K {
        let (hi, lo) = two_sum(p[i], -o[i]);
        for x in [hi, lo] {
            let (prod, e1) = two_prod(n[i], x);
            let (s2, e2) = two_sum(s, prod);
            s = s2;
            c += e1 + e2;
        }
    }
    s + c
}

/// Plain floating-point Σ nᵢ (pᵢ − oᵢ) and its forward error bound.
pub fn dot_diff_filtered<const K: usize>(n: &[f64; K], p: &[f64; K], o: &[f64; K]) -> (f64, f64) {
    let (mut d, mut mag) = (0.0, 0.0);
    for i in 0..K {
        let x = p[i] - o[i];
        d += n[i] * x;
        mag += (n[i] * x).abs();
    }
    (d, FILTER_COEF * mag)
}

/// Classifies Σ nᵢ (pᵢ − oᵢ) against the band [−tol, tol] (filter, then Dot2).
pub fn classify<const K: usize>(n: &[f64; K], p: &[f64; K], o: &[f64; K], tol: f64) -> Side {
    let (d, err) = dot_diff_filtered(n, p, o);
    let side = |d: f64| if d > tol { Side::Above } else if d < -tol { Side::Below } else { Side::On };
    if d.abs() - err > tol || d.abs() + err <= tol {
        return side(d);
    }
    side(dot_diff_accurate(n, p, o))
}

/// Position of `p` relative to the plane through `origin` with normal `normal` (unit for a
/// distance band in mm), with the tolerance band `tol`.
pub fn orient_plane(p: &P3, origin: &P3, normal: &V3, tol: f64) -> Side {
    classify(&[normal.x, normal.y, normal.z], &[p.x, p.y, p.z], &[origin.x, origin.y, origin.z], tol)
}

/// Position of the 2D point `p` relative to the line through `a` with direction `dir` (unit for
/// a distance band): `Above` = to the left of `dir` (counter-clockwise side).
pub fn orient_line_2d(p: [f64; 2], a: [f64; 2], dir: [f64; 2], tol: f64) -> Side {
    classify(&[-dir[1], dir[0]], &p, &a, tol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_way_with_band() {
        let (o, n) = (P3::new(1.0, 2.0, 3.0), V3::z());
        assert_eq!(orient_plane(&P3::new(5.0, 5.0, 3.0 + 2e-5), &o, &n, 1e-5), Side::Above);
        assert_eq!(orient_plane(&P3::new(5.0, 5.0, 3.0 - 2e-5), &o, &n, 1e-5), Side::Below);
        assert_eq!(orient_plane(&P3::new(5.0, 5.0, 3.0 + 5e-6), &o, &n, 1e-5), Side::On);
        assert_eq!(orient_plane(&P3::new(5.0, 5.0, 3.0), &o, &n, 0.0), Side::On);
        assert_eq!(orient_line_2d([0.0, 1.0], [0.0, 0.0], [1.0, 0.0], 0.5), Side::Above);
        assert_eq!(orient_line_2d([0.0, -1.0], [0.0, 0.0], [1.0, 0.0], 0.5), Side::Below);
    }

    #[test]
    fn fallback_decides_when_naive_rounding_cancels() {
        // exact value (1e16 + 1) + (1 − 1e16) = 2, but fl(1e16 + 1) = 1e16 and fl(1 − 1e16) = −1e16
        let (n, p, o) = ([1.0, 1.0], [1e16, 1.0], [-1.0, 1e16]);
        assert_eq!(dot_diff_filtered(&n, &p, &o).0, 0.0);
        assert_eq!(dot_diff_accurate(&n, &p, &o), 2.0);
        assert_eq!(classify(&n, &p, &o, 1.0), Side::Above);
        assert_eq!(classify(&[-1.0, -1.0], &p, &o, 1.0), Side::Below);
        assert_eq!(classify(&n, &p, &o, 2.0), Side::On);
    }

    #[test]
    fn antisymmetric_and_translation_consistent() {
        let n = V3::new(0.3, -0.4, 0.5).normalize();
        for k in 0..200 {
            let t = k as f64 * 0.37;
            let p = P3::new(1e4 + t, -3e4 + t * t, 7.0 - t);
            let o = P3::new(1e4, -3e4, 7.0);
            let a = orient_plane(&p, &o, &n, 1e-5);
            let b = orient_plane(&p, &o, &-n, 1e-5);
            let flip = match a { Side::Above => Side::Below, Side::Below => Side::Above, Side::On => Side::On };
            assert_eq!(b, flip);
        }
    }
}
