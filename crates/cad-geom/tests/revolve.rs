//! Revolve: every `rev.*` case of `research/feature-cases.json` against closed forms (Pappus
//! moments of the profile region), validation at the ASM and OCCT tolerances, the primitives it
//! must reproduce exactly (cylinder, cone, sphere, torus), partial sweeps, holes and pockets,
//! negative angles, tilted axes, scale, watertight tessellation and invalid input.
//!
//! Closed forms. For a region R of the meridian half-plane (ρ ≥ 0, h) swept through φ ∈ [0, α]
//! about z (starting on +x towards +y), with M_pq = ∫∫_R ρ^p h^q dA:
//!   V = α·M10;  ∫x dV = sin α·M20,  ∫y dV = (1 − cos α)·M20,  ∫z dV = α·M11;
//!   ∫x² dV = M30(α/2 + sin 2α/4),  ∫y² dV = M30(α/2 − sin 2α/4),  ∫xy dV = M30·sin²α/2,
//!   ∫z² dV = α·M12,  ∫xz dV = sin α·M21,  ∫yz dV = (1 − cos α)·M21;
//!   lateral area = α·∫ρ ds over the boundary (Pappus), plus 2·M00 for the caps when α < 2π.
//! M_pq of a polygon is ∮ ρ^{p+1} h^q dh / (p+1) (Green), whose integrand along an edge is a
//! polynomial of degree ≤ 5 in the edge parameter, integrated exactly by Boole's rule; disc and
//! half-disc moments are the textbook integrals written out below.

#[path = "support/features.rs"]
mod features;

use cad_geom::mass::solid_properties;
use cad_geom::primitives::{make_cone, make_cylinder, make_sphere, make_torus};
use cad_geom::tess::tessellate_solid;
use cad_geom::{revolve, validate, Brep, Frame, GeomError, MassProperties, Profile, Seg, SolidId, Tolerance, P2, P3, V3};
use nalgebra::{Matrix3, Rotation3, Unit};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

fn case_input(id: &str) -> (Profile, P3, V3, f64) {
    let c = features::case(id);
    let p = features::profile(&c);
    (p, features::p3(&c["axis"]["origin"]), features::v3(&c["axis"]["dir"]), c["angle"].as_f64().unwrap().to_radians())
}

fn build(id: &str) -> Brep {
    let (p, o, d, a) = case_input(id);
    revolve(&p, o, d, a, &Tolerance::asm()).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn mass(b: &Brep) -> MassProperties {
    solid_properties(b, SolidId(0)).unwrap()
}

fn counts(b: &Brep) -> (usize, usize, usize, usize) {
    (b.faces.len(), b.edges.len(), b.vertices.len(), b.loops.len())
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-300)
}

fn sorted_types(b: &Brep) -> Vec<&'static str> {
    let mut t: Vec<_> = b.faces.iter().map(|f| f.surface.type_name()).collect();
    t.sort_unstable();
    t
}

fn assert_valid(id: &str, b: &Brep, genus: i64) {
    for tol in [Tolerance::asm(), Tolerance::occt()] {
        let r = validate(b, &tol);
        assert!(r.is_valid(), "{id} at {tol:?}: {:?}", r.issues);
        assert_eq!(r.genus, genus, "{id}: genus");
    }
}

fn inertia_scale(i: &[[f64; 3]; 3]) -> f64 {
    i.iter().flatten().fold(0.0f64, |s, x| s.max(x.abs()))
}

fn assert_inertia(id: &str, got: &[[f64; 3]; 3], want: &[[f64; 3]; 3], tol: f64) {
    let s = inertia_scale(want);
    for i in 0..3 {
        for j in 0..3 {
            assert!((got[i][j] - want[i][j]).abs() <= tol * s, "{id}: inertia {i}{j} {} vs {} (scale {s})", got[i][j], want[i][j]);
        }
    }
}

fn assert_mass_eq(id: &str, a: &MassProperties, b: &MassProperties, tol: f64) {
    assert!(rel(a.volume, b.volume) < tol, "{id}: volume {} vs {}", a.volume, b.volume);
    assert!(rel(a.area, b.area) < tol, "{id}: area {} vs {}", a.area, b.area);
    let size = b.volume.cbrt();
    assert!((a.centroid - b.centroid).norm() <= tol * size, "{id}: centroid {:?} vs {:?}", a.centroid, b.centroid);
    assert_inertia(id, &a.inertia, &b.inertia, tol);
}

/// Profile in the world XZ plane (plane x = X, y = Z; normal −Y), as the `rev.*` cases.
fn xz_profile(loops: Vec<Vec<Seg>>) -> Profile {
    Profile { plane: Frame { origin: P3::origin(), x: V3::x(), y: V3::z(), z: V3::x().cross(&V3::z()) }, loops }
}

fn poly(pts: &[(f64, f64)]) -> Vec<Seg> {
    let p: Vec<P2> = pts.iter().map(|&(x, y)| P2::new(x, y)).collect();
    (0..p.len()).map(|i| Seg::Line { a: p[i], b: p[(i + 1) % p.len()] }).collect()
}

// ---------------------------------------------------------------------------------------------
// closed forms
// ---------------------------------------------------------------------------------------------

/// Region moments M00, M10, M20, M30, M11, M21, M12 and the boundary integral ∫ρ ds.
#[derive(Debug, Clone, Copy, Default)]
struct Moments {
    m00: f64,
    m10: f64,
    m20: f64,
    m30: f64,
    m11: f64,
    m21: f64,
    m12: f64,
    rho_ds: f64,
}

impl std::ops::Sub for Moments {
    type Output = Moments;
    /// A hole: region moments subtract, boundary lengths add.
    fn sub(self, o: Moments) -> Moments {
        Moments {
            m00: self.m00 - o.m00,
            m10: self.m10 - o.m10,
            m20: self.m20 - o.m20,
            m30: self.m30 - o.m30,
            m11: self.m11 - o.m11,
            m21: self.m21 - o.m21,
            m12: self.m12 - o.m12,
            rho_ds: self.rho_ds + o.rho_ds,
        }
    }
}

/// Counter-clockwise polygon in (ρ, h): Green + Boole's rule (exact for degree ≤ 5).
fn poly_moments(pts: &[(f64, f64)]) -> Moments {
    let boole = |f: &dyn Fn(f64) -> f64| (7.0 * f(0.0) + 32.0 * f(0.25) + 12.0 * f(0.5) + 32.0 * f(0.75) + 7.0 * f(1.0)) / 90.0;
    let mut m = Moments::default();
    for i in 0..pts.len() {
        let (r0, h0) = pts[i];
        let (r1, h1) = pts[(i + 1) % pts.len()];
        let (dr, dh) = (r1 - r0, h1 - h0);
        let g = |p: i32, q: i32| boole(&|t: f64| (r0 + t * dr).powi(p + 1) * (h0 + t * dh).powi(q) * dh / (p + 1) as f64);
        m.m00 += g(0, 0);
        m.m10 += g(1, 0);
        m.m20 += g(2, 0);
        m.m30 += g(3, 0);
        m.m11 += g(1, 1);
        m.m21 += g(2, 1);
        m.m12 += g(1, 2);
        m.rho_ds += dr.hypot(dh) * (r0 + r1) / 2.0;
    }
    m
}

/// Disc of radius `a` centred at (ρ, h) = (r, hc), r > a.
fn disc_moments(r: f64, hc: f64, a: f64) -> Moments {
    let area = PI * a * a;
    // ∫x² = ∫y² = πa⁴/4 about the centre, odd moments vanish
    let i2 = PI * a.powi(4) / 4.0;
    Moments {
        m00: area,
        m10: area * r,
        m20: area * r * r + i2,
        m30: area * r.powi(3) + 3.0 * r * i2,
        m11: area * r * hc,
        m21: hc * (area * r * r + i2),
        m12: r * (area * hc * hc + i2),
        rho_ds: TAU * a * r,
    }
}

/// Half disc {ρ ≥ 0, ρ² + h² ≤ a²} (polar: ∫ρⁿ⁺¹dρ ∫cosᵖt sinᵠt dt over t ∈ [−π/2, π/2]).
fn half_disc_moments(a: f64) -> Moments {
    Moments {
        m00: PI * a * a / 2.0,
        m10: 2.0 * a.powi(3) / 3.0,
        m20: PI * a.powi(4) / 8.0,
        m30: 4.0 * a.powi(5) / 15.0,
        m11: 0.0,
        m21: 0.0,
        m12: 2.0 * a.powi(5) / 15.0,
        rho_ds: 2.0 * a * a,
    }
}

impl std::ops::Add for Moments {
    type Output = Moments;
    /// Union of two regions meeting along a common boundary; `rho_ds` is meaningless here and is
    /// set from the actual profile by [`with_profile_boundary`].
    fn add(self, o: Moments) -> Moments {
        Moments {
            m00: self.m00 + o.m00,
            m10: self.m10 + o.m10,
            m20: self.m20 + o.m20,
            m30: self.m30 + o.m30,
            m11: self.m11 + o.m11,
            m21: self.m21 + o.m21,
            m12: self.m12 + o.m12,
            rho_ds: f64::NAN,
        }
    }
}

/// I(i, j) = ∫_{t0}^{t1} cosⁱt sinʲt dt by the standard reduction formulas (exact closed form).
fn trig_integral(i: u32, j: u32, t0: f64, t1: f64) -> f64 {
    let at = |f: &dyn Fn(f64) -> f64| f(t1) - f(t0);
    match (i, j) {
        (0, 0) => t1 - t0,
        (1, 0) => at(&|t| t.sin()),
        (0, 1) => at(&|t| -t.cos()),
        (1, 1) => at(&|t| t.sin().powi(2) / 2.0),
        (i, j) if i >= 2 => {
            let n = (i + j) as f64;
            at(&|t| t.cos().powi(i as i32 - 1) * t.sin().powi(j as i32 + 1)) / n + (i - 1) as f64 / n * trig_integral(i - 2, j, t0, t1)
        }
        (i, j) => {
            let n = (i + j) as f64;
            -at(&|t| t.cos().powi(i as i32 + 1) * t.sin().powi(j as i32 - 1)) / n + (j - 1) as f64 / n * trig_integral(i, j - 2, t0, t1)
        }
    }
}

/// Circular sector of radius `a` about `c` = (ρc, hc) between polar angles t0 < t1: local
/// moments L_ij = ∫∫ xⁱ yʲ = a^(i+j+2)/(i+j+2)·I(i, j), shifted binomially to
/// M_pq = ∫∫ (ρc + x)^p (hc + y)^q. `rho_ds` is left unset (see [`with_profile_boundary`]).
fn sector_moments(c: (f64, f64), a: f64, t0: f64, t1: f64) -> Moments {
    let l = |i: u32, j: u32| a.powi((i + j + 2) as i32) / (i + j + 2) as f64 * trig_integral(i, j, t0, t1);
    let binom = |n: u32, k: u32| (1..=k).fold(1.0, |acc, m| acc * (n + 1 - m) as f64 / m as f64);
    let m = |p: u32, q: u32| {
        let mut s = 0.0;
        for i in 0..=p {
            for j in 0..=q {
                s += binom(p, i) * binom(q, j) * c.0.powi((p - i) as i32) * c.1.powi((q - j) as i32) * l(i, j);
            }
        }
        s
    };
    Moments { m00: m(0, 0), m10: m(1, 0), m20: m(2, 0), m30: m(3, 0), m11: m(1, 1), m21: m(2, 1), m12: m(1, 2), rho_ds: f64::NAN }
}

/// ∫ρ ds over a profile drawn in the XZ plane (plane coordinates = (ρ, h)), segment by segment:
/// a line gives L·(ρ0 + ρ1)/2, an arc a·|ρc·sweep + a(sin(a0 + sweep) − sin a0)|.
fn with_profile_boundary(m: Moments, p: &Profile) -> Moments {
    let rho_ds = p
        .loops
        .iter()
        .flatten()
        .map(|s| match *s {
            Seg::Line { a, b } => (b - a).norm() * (a.x + b.x) / 2.0,
            Seg::Arc { c, r, a0, sweep } => r * (c.x * sweep + r * ((a0 + sweep).sin() - a0.sin())).abs(),
        })
        .sum();
    Moments { rho_ds, ..m }
}

/// Exact mass properties of the region swept through α about world z from +x towards +y.
fn swept(m: &Moments, alpha: f64) -> MassProperties {
    let (s, c) = alpha.sin_cos();
    let v = alpha * m.m10;
    let first = V3::new(s * m.m20, (1.0 - c) * m.m20, alpha * m.m11);
    let sxx = m.m30 * (alpha / 2.0 + (2.0 * alpha).sin() / 4.0);
    let syy = m.m30 * (alpha / 2.0 - (2.0 * alpha).sin() / 4.0);
    let sxy = m.m30 * s * s / 2.0;
    let second = Matrix3::new(sxx, sxy, s * m.m21, sxy, syy, (1.0 - c) * m.m21, s * m.m21, (1.0 - c) * m.m21, alpha * m.m12);
    let g = first / v;
    let central = second - g * g.transpose() * v;
    let tr = central.trace();
    let mut inertia = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            inertia[i][j] = if i == j { tr - central[(i, i)] } else { -central[(i, j)] };
        }
    }
    let caps = if alpha < TAU { 2.0 * m.m00 } else { 0.0 };
    MassProperties { volume: v, area: alpha * m.rho_ds + caps, centroid: P3::from(g), inertia }
}

/// Expected F/E/V/L, shells, genus and region moments of each `rev.*` case.
struct Expect {
    id: &'static str,
    fevl: (usize, usize, usize, usize),
    shells: usize,
    genus: i64,
    moments: Moments,
    /// Smallest radius of curvature of a curved face (tessellation bound).
    rmin: f64,
}

/// F/E/V/L derivations (full revolution: one closed circle + one vertex per profile vertex off
/// the axis, one face per segment off the axis, two loops per face, one per end off the axis;
/// partial: two vertices per vertex off the axis, one arc per vertex, two copies per segment,
/// one loop per lateral face, plus two caps):
/// * rect.full: 4 faces (2 planes, 2 cylinders), 4 circles, 4 vertices, 8 loops; V−E+F−(L−F) =
///   4−4+4−4 = 0 → genus 1 (a washer is a solid torus);
/// * rect.quarter: 4 lateral + 2 caps; 4 arcs + 8 copies; 8 vertices; 6 loops → 8−12+6−0 = 2;
/// * rect.on_axis: axis side sweeps nothing; bottom disc, cylinder, top disc; 2 circles,
///   2 vertices, loops 1 + 2 + 1 → make_cylinder's 3/2/2/4;
/// * triangle.on_axis: disc + cone; base circle + degenerate apex edge (own vertex); loops
///   1 + 2 → make_cone's 2/2/2/3 (Euler excludes the degenerate edge: 2−1+2−1 = 2);
/// * trapezoid: like rect.full with the outer cylinder a cone: 4/4/4/8, genus 1;
/// * semicircle: the arc's both ends lie on the axis, the line on it: one loopless sphere face;
/// * circle.full: a lone full circle: one loopless torus face (genus 1);
/// * circle.half: torus face bounded by the two circle copies (no traced arc: seamless in v),
///   two disc caps; E 2, V 2 (one per copy), L 2 + 1 + 1 = 4 → 2−2+3−1 = 2.
fn expectations() -> Vec<Expect> {
    let rect = poly_moments(&[(10.0, 0.0), (20.0, 0.0), (20.0, 10.0), (10.0, 10.0)]);
    vec![
        Expect { id: "rev.rect.full", fevl: (4, 4, 4, 8), shells: 1, genus: 1, moments: rect, rmin: 10.0 },
        Expect { id: "rev.rect.quarter", fevl: (6, 12, 8, 6), shells: 1, genus: 0, moments: rect, rmin: 10.0 },
        Expect {
            id: "rev.rect.on_axis",
            fevl: (3, 2, 2, 4),
            shells: 1,
            genus: 0,
            moments: poly_moments(&[(0.0, 0.0), (10.0, 0.0), (10.0, 20.0), (0.0, 20.0)]),
            rmin: 10.0,
        },
        Expect {
            id: "rev.triangle.on_axis",
            fevl: (2, 2, 2, 3),
            shells: 1,
            genus: 0,
            moments: poly_moments(&[(0.0, 0.0), (10.0, 0.0), (0.0, 20.0)]),
            rmin: 10.0,
        },
        Expect {
            id: "rev.trapezoid",
            fevl: (4, 4, 4, 8),
            shells: 1,
            genus: 1,
            moments: poly_moments(&[(10.0, 0.0), (20.0, 0.0), (15.0, 10.0), (10.0, 10.0)]),
            rmin: 10.0,
        },
        Expect { id: "rev.semicircle", fevl: (1, 0, 0, 0), shells: 1, genus: 0, moments: half_disc_moments(10.0), rmin: 10.0 },
        Expect { id: "rev.circle.full", fevl: (1, 0, 0, 0), shells: 1, genus: 1, moments: disc_moments(20.0, 0.0, 5.0), rmin: 5.0 },
        Expect { id: "rev.circle.half", fevl: (3, 2, 2, 4), shells: 1, genus: 0, moments: disc_moments(20.0, 0.0, 5.0), rmin: 5.0 },
    ]
}

fn check_against_closed_form(id: &str, b: &Brep, want: &MassProperties) {
    let m = mass(b);
    assert_mass_eq(id, &m, want, 1e-12);
}

// ---------------------------------------------------------------------------------------------
// the catalogue cases
// ---------------------------------------------------------------------------------------------

#[test]
fn revolve_cases_match_closed_forms() {
    let all = features::cases();
    let ids: Vec<&str> = all.iter().filter_map(|c| c["id"].as_str()).filter(|i| i.starts_with("rev.")).collect();
    let ex = expectations();
    assert_eq!(ids.len(), ex.len(), "every rev.* case has an expectation");
    for e in &ex {
        let b = build(e.id);
        assert_eq!(counts(&b), e.fevl, "{} F/E/V/L", e.id);
        assert_eq!(b.shells.len(), e.shells, "{} shells", e.id);
        assert_valid(e.id, &b, e.genus);
        let (_, _, _, alpha) = case_input(e.id);
        check_against_closed_form(e.id, &b, &swept(&e.moments, alpha));
    }
}

/// The four primitives a full revolution must reproduce: counts, sorted face types, degenerate
/// edges and mass properties (inertia included) to rounding.
#[test]
fn full_revolutions_equal_the_primitives() {
    let pairs = [
        ("rev.rect.on_axis", make_cylinder(Frame::world(), 10.0, 20.0).unwrap()),
        ("rev.triangle.on_axis", make_cone(Frame::world(), 10.0, 0.0, 20.0).unwrap()),
        ("rev.semicircle", make_sphere(P3::origin(), 10.0).unwrap()),
        ("rev.circle.full", make_torus(Frame::world(), 20.0, 5.0).unwrap()),
    ];
    for (id, prim) in pairs {
        let b = build(id);
        assert_eq!(counts(&b), counts(&prim), "{id}");
        assert_eq!(sorted_types(&b), sorted_types(&prim), "{id}");
        let (rb, rp) = (validate(&b, &Tolerance::asm()), validate(&prim, &Tolerance::asm()));
        assert_eq!((rb.degenerate_edges, rb.genus, rb.shells), (rp.degenerate_edges, rp.genus, rp.shells), "{id}");
        assert_mass_eq(id, &mass(&b), &mass(&prim), 1e-12);
    }
}

/// Textbook inertia (about the centroid, unit density): washer I_zz = M(a² + b²)/2,
/// I_xx = M(3(a² + b²) + h²)/12; cylinder the same with a = 0; ball 2MR²/5; solid torus
/// I_zz = M(R² + 3r²/4), I_xx = M(R²/2 + 5r²/8).
#[test]
fn inertia_matches_textbook_formulas() {
    let diag = |a: f64, c: f64| [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, c]];
    let washer = {
        let m = PI * (400.0 - 100.0) * 10.0;
        diag(m * (3.0 * 500.0 + 100.0) / 12.0, m * 500.0 / 2.0)
    };
    let cylinder = {
        let m = PI * 100.0 * 20.0;
        diag(m * (300.0 + 400.0) / 12.0, m * 100.0 / 2.0)
    };
    let ball = {
        let m = 4.0 / 3.0 * PI * 1000.0;
        diag(0.4 * m * 100.0, 0.4 * m * 100.0)
    };
    let torus = {
        let m = 2.0 * PI * PI * 20.0 * 25.0;
        diag(m * (200.0 + 5.0 * 25.0 / 8.0), m * (400.0 + 0.75 * 25.0))
    };
    for (id, want) in [("rev.rect.full", washer), ("rev.rect.on_axis", cylinder), ("rev.semicircle", ball), ("rev.circle.full", torus)] {
        assert_inertia(id, &mass(&build(id)).inertia, &want, 1e-12);
    }
    // solid hemisphere (z ≥ 0) of radius r: V = 2πr³/3, z̄ = 3r/8, I_zz = 2Mr²/5,
    // I_xx = I_yy = 83Mr²/320 about the centroid
    let e = extras().into_iter().find(|e| e.id == "hemisphere.360").unwrap();
    let m = mass(&build_extra(&e));
    let v = 2.0 * PI * 1000.0 / 3.0;
    assert!(rel(m.volume, v) < 1e-13 && (m.centroid - P3::new(0.0, 0.0, 3.75)).norm() < 1e-12 * 10.0, "{m:?}");
    assert_inertia("hemisphere", &m.inertia, &diag(83.0 / 320.0 * v * 100.0, 0.4 * v * 100.0), 1e-12);
}

#[test]
fn partial_revolutions_are_fractions_of_the_full_solid() {
    let full = mass(&build("rev.rect.full"));
    let quarter = mass(&build("rev.rect.quarter"));
    assert!(rel(quarter.volume, full.volume / 4.0) < 1e-13);
    // centroid of the quarter washer: ρ̄ = M20/M10 = (8000 − 1000)/3 / 150, at 45°
    let k = (7000.0 / 3.0) / 150.0;
    let want = P3::new(k * 2.0 / PI, k * 2.0 / PI, 5.0);
    assert!((quarter.centroid - want).norm() < 1e-12 * 20.0, "{:?} vs {want:?}", quarter.centroid);

    let torus = mass(&build("rev.circle.full"));
    let half = mass(&build("rev.circle.half"));
    assert!(rel(half.volume, torus.volume / 2.0) < 1e-13);
    // ȳ = (2/π)(R² + r²/4)/R for the half torus over φ ∈ [0, π]
    let want = P3::new(0.0, 2.0 / PI * (400.0 + 6.25) / 20.0, 0.0);
    assert!((half.centroid - want).norm() < 1e-12 * 20.0, "{:?} vs {want:?}", half.centroid);
}

/// Provisional ASM record (`research/evidence/asm-v6`, Fusion probe v6): same counts, face types,
/// volume, area, centroid and inertia.
#[test]
fn revolve_matches_asm_feature_records() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../research/evidence/asm-v6/reference.json");
    if !path.exists() {
        eprintln!("asm-v6 reference not present; skipped");
        return;
    }
    let asm = features::load("research/evidence/asm-v6/reference.json");
    for e in expectations() {
        let Some(a) = asm["cases"].as_array().unwrap().iter().find(|x| x["id"] == e.id) else { continue };
        let b = build(e.id);
        let u = |k: &str| a[k].as_u64().unwrap() as usize;
        assert_eq!((b.faces.len(), b.edges.len(), b.vertices.len(), b.loops.len(), b.shells.len()), (u("faces"), u("edges"), u("vertices"), u("loops"), u("shells")), "{}", e.id);
        let mut at: Vec<String> = a["face_types"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
        at.sort();
        assert_eq!(sorted_types(&b), at, "{}", e.id);
        // ASM names a circle edge short of 2π an "arc" and a degenerate edge "none"
        let mut et: Vec<String> = b
            .edges
            .iter()
            .map(|x| match &x.curve {
                None => "none".to_string(),
                Some(c) if c.type_name() == "circle" && x.t1 - x.t0 < TAU - 1e-12 => "arc".to_string(),
                Some(c) => c.type_name().to_string(),
            })
            .collect();
        et.sort();
        let mut aet: Vec<String> = a["edge_types"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
        aet.sort();
        assert_eq!(et, aet, "{} edge types", e.id);
        let m = mass(&b);
        let num = |k: &str| a[k].as_f64().unwrap();
        assert!(rel(m.volume, num("volume")) < 1e-9 && rel(m.area, num("area")) < 1e-9, "{}: {} {} vs {} {}", e.id, m.volume, m.area, num("volume"), num("area"));
        assert!((m.centroid - features::p3(&a["centroid"])).norm() < 1e-9 * 25.0, "{}", e.id);
        let ai: Vec<V3> = a["inertia_centroid"].as_array().unwrap().iter().map(features::v3).collect();
        let want = [[ai[0][0], ai[0][1], ai[0][2]], [ai[1][0], ai[1][1], ai[1][2]], [ai[2][0], ai[2][1], ai[2][2]]];
        // ASM integrates numerically: its rev.rect.full I_xx = I_yy is 1.2e-8 (relative) above the
        // exact M(3(a² + b²) + h²)/12 that this kernel matches to 1e-12 (see
        // `inertia_matches_textbook_formulas`), 6.5e-9 of the tensor scale; every other case agrees
        // to 1e-13. That one case is held just above ASM's own error, the others to 1e-9 as in
        // `tests/extrude.rs`.
        let bound = if e.id == "rev.rect.full" { 1e-8 } else { 1e-9 };
        assert_inertia(e.id, &m.inertia, &want, bound);
    }
}

// ---------------------------------------------------------------------------------------------
// further profiles: partial sweeps touching the axis, holes, pockets
// ---------------------------------------------------------------------------------------------

struct Extra {
    id: &'static str,
    profile: Profile,
    angle: f64,
    fevl: (usize, usize, usize, usize),
    shells: usize,
    genus: i64,
    moments: Moments,
    rmin: f64,
}

/// Partial sweeps of faces that touch the axis (cone apex, sphere poles, disc centres) at angles
/// below and above π — the pcurve then jumps in u at the pole with no edge there — and profiles
/// with holes and pockets.
/// * cone 270°: disc sector (loop: copy, arc, copy), cone sector (base arc, copy, copy), 2 caps;
///   edges: 1 arc + 2·2 copies + 1 axis edge shared by the caps = 6; vertices: apex, origin
///   (axis, one each), base vertex at 0 and α = 4; loops 4 → 4−6+4−0 = 2;
/// * sphere wedge (half disc): sphere face [copy at α, copy at 0], caps share the axis edge:
///   F3 E3 V2 L3 → 2−3+3−0 = 2;
/// * cylinder 270° (rect on the axis): 3 lateral + 2 caps = 5; 2 arcs + 3·2 copies + 1 axis = 9;
///   2 axis vertices + 2·2 = 6; L5 → 6−9+5−0 = 2;
/// * plate with a round hole, 360°: washer shell (4/4/4/8, genus 1) plus a loopless torus void
///   shell (genus 1): F5 E4 V4 L8, two shells, genus 2;
/// * the same through 90°: 4 + 1 + 2 faces; 4 arcs + 8 copies + 2 circle copies = 14 edges;
///   8 + 2 vertices; loops 4 + 2 + 2·2 = 10 → 10−14+7−3 = 0: genus 1 (a tunnel through the block);
/// * pocket (cylinder with an inner cylindrical void touching the axis at two discs), 360°:
///   outer 3/2/2/4 + void 3/2/2/4, two shells, genus 0;
/// * the same pocket through 90°: one shell; lateral 3 + 3, caps 2 = 8 faces; vertices: 4 on the
///   axis + 4·2 = 12; edges: 4 arcs + 6·2 copies + 2 axis = 18; loops 6 + 2 = 8 → 12−18+8−0 = 2;
/// * diamond touching the axis at one point, 360°: four cones joined by 3 circles; both cones at
///   the contact get their own apex vertex and degenerate edge (a pinch point counted once would
///   give V−E+F−(L−F) = 1, no integer genus): F4, E 3 + 2 degenerate, V 3 + 2, L8 → 5−3+4−4 = 2;
/// * the same diamond through 270°: one axis vertex + 3·2; 3 arcs + 4·2 copies; F 4 + 2; L6 →
///   7−11+6−0 = 2;
/// * dimple (triangle (0,0), (10,0), (5,10)), 360°: disc (no edge at its centre), frustum, and an
///   inward cone whose apex touches the disc centre: F3, E 2 + 1 degenerate, V 2 + 1, L 1 + 2 + 2
///   → 3−2+3−2 = 2;
/// * hemisphere (quarter disc on the axis), 360°: disc + sphere cap bounded by the equator only
///   (the pole is not an edge): F2 E1 V1 L2 → 1−1+2−0 = 2; 270°: axis vertices (0,0), (0,10) +
///   2 for (10,0); 1 arc + 2·2 copies + 1 axis edge; F 2 + 2; L4 → 4−6+4−0 = 2;
/// * bump (rectangle whose outer side is a counter-clockwise half circle: a torus band) and bite
///   (the half circle clockwise into the rectangle: a reversed torus band), 360°: like the washer
///   4/4/4/8, genus 1; 270°: like the quarter washer 6/12/8/6;
/// * sphere dimple (cylinder whose top has a clockwise quarter circle down to the axis: a reversed
///   sphere cap with its pole on the axis), 360°: bottom disc, cylinder, top annulus, sphere cap;
///   3 circles, 3 vertices, loops 1 + 2 + 2 + 1 → 3−3+4−2 = 2; 270°: (0,0), (0,15) on the axis +
///   3·2; 3 arcs + 4·2 copies + 1 axis edge = 12; F 4 + 2; L6 → 8−12+6−0 = 2.
fn extras() -> Vec<Extra> {
    let tri = || xz_profile(vec![poly(&[(0.0, 0.0), (10.0, 0.0), (0.0, 20.0)])]);
    let half = || xz_profile(vec![vec![Seg::Arc { c: P2::new(0.0, 0.0), r: 10.0, a0: -FRAC_PI_2, sweep: PI }, Seg::Line { a: P2::new(0.0, 10.0), b: P2::new(0.0, -10.0) }]]);
    let plate = || xz_profile(vec![poly(&[(10.0, 0.0), (30.0, 0.0), (30.0, 20.0), (10.0, 20.0)]), vec![Seg::circle(P2::new(20.0, 10.0), 5.0)]]);
    let pocket_pts = [(0.0, 0.0), (10.0, 0.0), (10.0, 30.0), (0.0, 30.0), (0.0, 20.0), (5.0, 20.0), (5.0, 10.0), (0.0, 10.0)];
    let pocket = || xz_profile(vec![poly(&pocket_pts)]);
    let tri_m = poly_moments(&[(0.0, 0.0), (10.0, 0.0), (0.0, 20.0)]);
    let plate_m = poly_moments(&[(10.0, 0.0), (30.0, 0.0), (30.0, 20.0), (10.0, 20.0)]) - disc_moments(20.0, 10.0, 5.0);
    let pocket_m = poly_moments(&pocket_pts);
    let diamond_pts = [(0.0, 0.0), (5.0, -10.0), (10.0, 0.0), (5.0, 10.0)];
    let diamond = || xz_profile(vec![poly(&diamond_pts)]);
    let diamond_m = poly_moments(&diamond_pts);
    let dimple_pts = [(0.0, 0.0), (10.0, 0.0), (5.0, 10.0)];
    let hemi = || xz_profile(vec![vec![Seg::Line { a: P2::new(0.0, 0.0), b: P2::new(10.0, 0.0) }, Seg::Arc { c: P2::new(0.0, 0.0), r: 10.0, a0: 0.0, sweep: FRAC_PI_2 }, Seg::Line { a: P2::new(0.0, 10.0), b: P2::new(0.0, 0.0) }]]);
    let hemi_m = with_profile_boundary(sector_moments((0.0, 0.0), 10.0, 0.0, FRAC_PI_2), &hemi());
    let rect_m = poly_moments(&[(10.0, 0.0), (20.0, 0.0), (20.0, 10.0), (10.0, 10.0)]);
    let band = |sweep: f64| {
        xz_profile(vec![vec![
            Seg::Line { a: P2::new(10.0, 0.0), b: P2::new(20.0, 0.0) },
            Seg::Arc { c: P2::new(20.0, 5.0), r: 5.0, a0: -FRAC_PI_2, sweep },
            Seg::Line { a: P2::new(20.0, 10.0), b: P2::new(10.0, 10.0) },
            Seg::Line { a: P2::new(10.0, 10.0), b: P2::new(10.0, 0.0) },
        ]])
    };
    let bump_m = with_profile_boundary(rect_m + sector_moments((20.0, 5.0), 5.0, -FRAC_PI_2, FRAC_PI_2), &band(PI));
    let bite_m = with_profile_boundary(rect_m - sector_moments((20.0, 5.0), 5.0, FRAC_PI_2, 1.5 * PI), &band(-PI));
    let sdimple = || {
        xz_profile(vec![vec![
            Seg::Line { a: P2::new(0.0, 0.0), b: P2::new(10.0, 0.0) },
            Seg::Line { a: P2::new(10.0, 0.0), b: P2::new(10.0, 20.0) },
            Seg::Line { a: P2::new(10.0, 20.0), b: P2::new(5.0, 20.0) },
            Seg::Arc { c: P2::new(0.0, 20.0), r: 5.0, a0: 0.0, sweep: -FRAC_PI_2 },
            Seg::Line { a: P2::new(0.0, 15.0), b: P2::new(0.0, 0.0) },
        ]])
    };
    let sdimple_m = with_profile_boundary(poly_moments(&[(0.0, 0.0), (10.0, 0.0), (10.0, 20.0), (0.0, 20.0)]) - sector_moments((0.0, 20.0), 5.0, -FRAC_PI_2, 0.0), &sdimple());
    let x = |id, profile, angle: f64, fevl, shells, genus, moments, rmin| Extra { id, profile, angle, fevl, shells, genus, moments, rmin };
    vec![
        x("cone.90", tri(), FRAC_PI_2, (4, 6, 4, 4), 1, 0, tri_m, 10.0),
        x("cone.270", tri(), 1.5 * PI, (4, 6, 4, 4), 1, 0, tri_m, 10.0),
        x("sphere.180", half(), PI, (3, 3, 2, 3), 1, 0, half_disc_moments(10.0), 10.0),
        x("sphere.270", half(), 1.5 * PI, (3, 3, 2, 3), 1, 0, half_disc_moments(10.0), 10.0),
        x("cylinder.270", xz_profile(vec![poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 20.0), (0.0, 20.0)])]), 1.5 * PI, (5, 9, 6, 5), 1, 0, poly_moments(&[(0.0, 0.0), (10.0, 0.0), (10.0, 20.0), (0.0, 20.0)]), 10.0),
        x("plate_hole.360", plate(), TAU, (5, 4, 4, 8), 2, 2, plate_m, 5.0),
        x("plate_hole.90", plate(), FRAC_PI_2, (7, 14, 10, 10), 1, 1, plate_m, 5.0),
        x("pocket.360", pocket(), TAU, (6, 4, 4, 8), 2, 0, pocket_m, 5.0),
        x("pocket.90", pocket(), FRAC_PI_2, (8, 18, 12, 8), 1, 0, pocket_m, 5.0),
        x("diamond.360", diamond(), TAU, (4, 5, 5, 8), 1, 0, diamond_m, 5.0),
        x("diamond.270", diamond(), 1.5 * PI, (6, 11, 7, 6), 1, 0, diamond_m, 5.0),
        x("dimple.360", xz_profile(vec![poly(&dimple_pts)]), TAU, (3, 3, 3, 5), 1, 0, poly_moments(&dimple_pts), 5.0),
        x("hemisphere.360", hemi(), TAU, (2, 1, 1, 2), 1, 0, hemi_m, 10.0),
        x("hemisphere.270", hemi(), 1.5 * PI, (4, 6, 4, 4), 1, 0, hemi_m, 10.0),
        x("bump.360", band(PI), TAU, (4, 4, 4, 8), 1, 1, bump_m, 5.0),
        x("bump.270", band(PI), 1.5 * PI, (6, 12, 8, 6), 1, 0, bump_m, 5.0),
        x("bite.360", band(-PI), TAU, (4, 4, 4, 8), 1, 1, bite_m, 5.0),
        x("bite.270", band(-PI), 1.5 * PI, (6, 12, 8, 6), 1, 0, bite_m, 5.0),
        x("sphere_dimple.360", sdimple(), TAU, (4, 3, 3, 6), 1, 0, sdimple_m, 5.0),
        x("sphere_dimple.270", sdimple(), 1.5 * PI, (6, 12, 8, 6), 1, 0, sdimple_m, 5.0),
    ]
}

fn build_extra(e: &Extra) -> Brep {
    revolve(&e.profile, P3::origin(), V3::z(), e.angle, &Tolerance::asm()).unwrap_or_else(|err| panic!("{}: {err}", e.id))
}

#[test]
fn partial_sweeps_holes_and_pockets() {
    for e in extras() {
        let b = build_extra(&e);
        assert_eq!(counts(&b), e.fevl, "{} F/E/V/L", e.id);
        assert_eq!(b.shells.len(), e.shells, "{} shells", e.id);
        assert_valid(e.id, &b, e.genus);
        check_against_closed_form(e.id, &b, &swept(&e.moments, e.angle));
    }
}

#[test]
fn voids_are_inward_shells_of_the_same_solid() {
    let e = extras().into_iter().find(|e| e.id == "plate_hole.360").unwrap();
    let b = build_extra(&e);
    assert_eq!(b.solids.len(), 1);
    assert_eq!(b.solids[0].shells.len(), 2);
    // the void shell alone is a loopless torus face whose outward normal points into the cavity
    let void = b.shell(b.solids[0].shells[1]);
    assert_eq!(void.faces.len(), 1);
    let f = b.face(void.faces[0]);
    assert!(f.reversed && f.loops.is_empty() && f.surface.type_name() == "torus");
}

// ---------------------------------------------------------------------------------------------
// symmetries: orientation, negative angle, tilted axis, scale
// ---------------------------------------------------------------------------------------------

#[test]
fn revolve_is_independent_of_input_loop_orientation_and_order() {
    let e = extras().into_iter().find(|e| e.id == "plate_hole.90").unwrap();
    let p = e.profile.clone();
    let flipped = Profile { plane: p.plane, loops: p.loops.iter().rev().map(|l| l.iter().rev().map(Seg::reversed).collect()).collect() };
    let a = mass(&revolve(&p, P3::origin(), V3::z(), e.angle, &Tolerance::asm()).unwrap());
    let b = mass(&revolve(&flipped, P3::origin(), V3::z(), e.angle, &Tolerance::asm()).unwrap());
    assert_mass_eq("flipped", &a, &b, 1e-13);
    // the same plane with its normal flipped (x and y swapped) describes the same region
    let swapped = Profile {
        plane: Frame { origin: p.plane.origin, x: p.plane.y, y: p.plane.x, z: -p.plane.z },
        loops: p.loops.iter().map(|l| l.iter().map(|s| swap_xy(*s)).collect()).collect(),
    };
    let c = mass(&revolve(&swapped, P3::origin(), V3::z(), e.angle, &Tolerance::asm()).unwrap());
    assert_mass_eq("swapped plane", &c, &a, 1e-12);
}

fn swap_xy(s: Seg) -> Seg {
    match s {
        Seg::Line { a, b } => Seg::Line { a: P2::new(a.y, a.x), b: P2::new(b.y, b.x) },
        Seg::Arc { c, r, a0, sweep } => Seg::Arc { c: P2::new(c.y, c.x), r, a0: FRAC_PI_2 - a0, sweep: -sweep },
    }
}

/// A negative angle sweeps the other way: the mirror image through the profile plane (y ↦ −y).
#[test]
fn negative_angle_is_the_mirror_image() {
    let mirror = |m: &MassProperties| {
        let s = [1.0, -1.0, 1.0];
        let mut i = m.inertia;
        for (r, row) in i.iter_mut().enumerate() {
            for (c, x) in row.iter_mut().enumerate() {
                *x *= s[r] * s[c];
            }
        }
        MassProperties { centroid: P3::new(m.centroid.x, -m.centroid.y, m.centroid.z), inertia: i, ..*m }
    };
    let mut inputs: Vec<(String, Profile, f64)> = ["rev.rect.quarter", "rev.circle.half", "rev.trapezoid"]
        .iter()
        .map(|id| {
            let (p, _, _, a) = case_input(id);
            (id.to_string(), p, a)
        })
        .collect();
    inputs.extend(extras().into_iter().filter(|e| e.angle < TAU).map(|e| (e.id.to_string(), e.profile, e.angle)));
    for (id, p, a) in inputs {
        let pos = revolve(&p, P3::origin(), V3::z(), a, &Tolerance::asm()).unwrap();
        let neg = revolve(&p, P3::origin(), V3::z(), -a, &Tolerance::asm()).unwrap();
        assert_eq!(counts(&neg), counts(&pos), "{id}");
        assert_valid(&id, &neg, validate(&pos, &Tolerance::asm()).genus);
        assert_mass_eq(&id, &mass(&neg), &mirror(&mass(&pos)), 1e-12);
        // reversing the axis instead is the same solid
        let flipped_axis = revolve(&p, P3::origin(), -V3::z(), a, &Tolerance::asm()).unwrap();
        assert_mass_eq(&id, &mass(&flipped_axis), &mass(&neg), 1e-12);
    }
}

/// Moving profile and axis rigidly (a general rotation and translation) moves the solid: volume
/// and area unchanged, centroid Q·c + t, inertia Q·I·Qᵀ.
#[test]
fn tilted_axis_rotates_mass_properties() {
    let q = Rotation3::from_axis_angle(&Unit::new_normalize(V3::new(1.0, 2.0, 3.0)), 0.7);
    let t = V3::new(5.0, -3.0, 12.0);
    let mut inputs: Vec<(String, Profile, P3, V3, f64)> = expectations()
        .iter()
        .map(|e| {
            let (p, o, d, a) = case_input(e.id);
            (e.id.to_string(), p, o, d, a)
        })
        .collect();
    inputs.extend(extras().into_iter().map(|e| (e.id.to_string(), e.profile, P3::origin(), V3::z(), e.angle)));
    for (id, p, o, d, a) in inputs {
        let base = revolve(&p, o, d, a, &Tolerance::asm()).unwrap();
        let moved = Profile {
            plane: Frame { origin: q * p.plane.origin + t, x: q * p.plane.x, y: q * p.plane.y, z: q * p.plane.z },
            loops: p.loops.clone(),
        };
        let b = revolve(&moved, q * o + t, q * d, a, &Tolerance::asm()).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(counts(&b), counts(&base), "{id}");
        assert_valid(&id, &b, validate(&base, &Tolerance::asm()).genus);
        let (m0, m1) = (mass(&base), mass(&b));
        let qm = q.matrix();
        let i0 = Matrix3::from_fn(|r, c| m0.inertia[r][c]);
        let ir = qm * i0 * qm.transpose();
        let want = MassProperties {
            centroid: q * m0.centroid + t,
            inertia: [[ir[(0, 0)], ir[(0, 1)], ir[(0, 2)]], [ir[(1, 0)], ir[(1, 1)], ir[(1, 2)]], [ir[(2, 0)], ir[(2, 1)], ir[(2, 2)]]],
            ..m0
        };
        assert_mass_eq(&id, &m1, &want, 1e-11);
    }
}

#[test]
fn revolve_scale_range() {
    let scale_seg = |g: &Seg, s: f64| match *g {
        Seg::Line { a, b } => Seg::Line { a: P2::from(a.coords * s), b: P2::from(b.coords * s) },
        Seg::Arc { c, r, a0, sweep } => Seg::Arc { c: P2::from(c.coords * s), r: r * s, a0, sweep },
    };
    for e in expectations() {
        let (p, o, d, a) = case_input(e.id);
        let exact = swept(&e.moments, a);
        for s in [1e-2, 1e4] {
            let sp = Profile { plane: Frame { origin: P3::from(p.plane.origin.coords * s), ..p.plane }, loops: p.loops.iter().map(|l| l.iter().map(|g| scale_seg(g, s)).collect()).collect() };
            let b = revolve(&sp, P3::from(o.coords * s), d, a, &Tolerance::asm()).unwrap_or_else(|err| panic!("{} at {s}: {err}", e.id));
            assert_eq!(counts(&b), e.fevl, "{} at {s}", e.id);
            let r = validate(&b, &Tolerance::asm());
            assert!(r.is_valid(), "{} at {s}: {:?}", e.id, r.issues);
            let m = mass(&b);
            assert!(rel(m.volume, exact.volume * s.powi(3)) < 1e-12, "{} at {s}: volume", e.id);
            assert!(rel(m.area, exact.area * s * s) < 1e-12, "{} at {s}: area", e.id);
            assert!((m.centroid - P3::from(exact.centroid.coords * s)).norm() < 1e-12 * 25.0 * s, "{} at {s}: centroid", e.id);
            let want = exact.inertia.map(|row| row.map(|x| x * s.powi(5)));
            assert_inertia(e.id, &m.inertia, &want, 1e-11);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// tessellation
// ---------------------------------------------------------------------------------------------

#[test]
fn revolve_tessellates_watertight() {
    // an inscribed mesh loses about (4/3)·chord/r (cylinder) to (5/3)·chord/r (sphere) of the
    // volume, so curved solids are held to 2·chord/r_min
    let chord = 0.01;
    let mut all: Vec<(String, Brep, f64)> = expectations().iter().map(|e| (e.id.to_string(), build(e.id), e.rmin)).collect();
    all.extend(extras().iter().map(|e| (e.id.to_string(), build_extra(e), e.rmin)));
    for (id, b, rmin) in all {
        let exact = mass(&b).volume;
        let mesh = tessellate_solid(&b, SolidId(0), chord).unwrap_or_else(|e| panic!("{id}: {e}"));
        features::assert_watertight(&id, &mesh);
        let v = mesh.signed_volume();
        let bound = 2.0 * chord / rmin;
        assert!(rel(v, exact) < bound, "{id}: mesh volume {v} vs {exact} (bound {bound:.1e})");
    }
}

// ---------------------------------------------------------------------------------------------
// invalid input
// ---------------------------------------------------------------------------------------------

#[test]
fn revolve_rejects_bad_input() {
    let (p, o, d, _) = case_input("rev.rect.full");
    let tol = Tolerance::asm();
    let bad = |r: Result<Brep, GeomError>, what: &str| assert!(matches!(r, Err(GeomError::InvalidInput(_))), "{what}: {r:?}");
    bad(revolve(&p, P3::new(15.0, 0.0, 0.0), d, PI, &tol), "axis crossing the region");
    bad(revolve(&p, P3::new(0.0, 0.0, 5.0), V3::x(), PI, &tol), "horizontal axis through the region");
    assert!(revolve(&p, P3::origin(), V3::x(), PI, &tol).is_ok(), "horizontal axis along the bottom side touches only");
    bad(revolve(&p, o, V3::new(0.0, 1.0, 1.0), PI, &tol), "axis direction out of the plane");
    bad(revolve(&p, P3::new(0.0, 1.0, 0.0), d, PI, &tol), "axis origin off the plane");
    bad(revolve(&p, o, V3::zeros(), PI, &tol), "zero axis");
    bad(revolve(&p, o, d, 0.0, &tol), "zero angle");
    bad(revolve(&p, o, d, TAU + 0.1, &tol), "angle above 2π");
    bad(revolve(&p, o, d, -TAU - 0.1, &tol), "angle below −2π");
    bad(revolve(&p, o, d, f64::NAN, &tol), "NaN angle");
    bad(revolve(&p, o, d, f64::INFINITY, &tol), "infinite angle");
    bad(revolve(&p, P3::new(f64::NAN, 0.0, 0.0), d, PI, &tol), "NaN axis origin");
    bad(revolve(&p, o, V3::new(0.0, 0.0, f64::NAN), PI, &tol), "NaN axis direction");
    let mut open = p.clone();
    open.loops[0].pop();
    bad(revolve(&open, o, d, PI, &tol), "open profile");
    // touching the axis is fine; crossing it by more than the tolerance is not
    let touch = xz_profile(vec![poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)])]);
    assert!(revolve(&touch, P3::origin(), V3::z(), PI, &tol).is_ok());
    let cross = xz_profile(vec![poly(&[(-1e-3, 0.0), (10.0, 0.0), (10.0, 5.0)])]);
    bad(revolve(&cross, P3::origin(), V3::z(), PI, &tol), "vertex 1e-3 across the axis");
    // the region entirely on the −x side is fine (mirror of the +x side)
    let left = xz_profile(vec![poly(&[(-20.0, 0.0), (-10.0, 0.0), (-10.0, 10.0), (-20.0, 10.0)])]);
    let m = mass(&revolve(&left, P3::origin(), V3::z(), TAU, &tol).unwrap());
    assert!(rel(m.volume, mass(&build("rev.rect.full")).volume) < 1e-13);
}

#[test]
fn arcs_reaching_the_axis_off_centre_are_unsupported() {
    // a lens: arc centred at ρ = −5 through (0, ±10), closed along the axis (spindle torus face)
    let r = 125.0f64.sqrt();
    let a = (10.0f64).atan2(5.0);
    let lens = xz_profile(vec![vec![Seg::Arc { c: P2::new(-5.0, 0.0), r, a0: -a, sweep: 2.0 * a }, Seg::Line { a: P2::new(0.0, 10.0), b: P2::new(0.0, -10.0) }]]);
    for angle in [PI, TAU] {
        assert!(matches!(revolve(&lens, P3::origin(), V3::z(), angle, &Tolerance::asm()), Err(GeomError::Unsupported(_))));
    }
}
