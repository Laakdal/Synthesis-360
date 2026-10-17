# Kernel roadmap and resumable checklist

Goal: an independently implemented Rust kernel (`crates/cad-geom`) with measured, observable
parity with Autodesk ShapeManager (ASM) for a defined operation set, while the application keeps
running on the preserved OCCT integration (`docs/BASELINE.md`, `docs/ARCHITECTURE.md`).
Status per operation: `docs/FEATURE_MATRIX.md`. Evidence: `research/`.

Rules that hold for every milestone:
* ASM is studied black-box only (public documentation, the Fusion API, exported reference
  geometry). ASM binaries are never disassembled or decompiled.
* OCCT code, version and build stay as they are unless a change is explicitly authorised.
* A feature is `REFERENCE_VERIFIED` only after an ASM comparison was actually run and matched.
* Every bug found gets a regression test; tests are never weakened to pass.

## Checklist (tick only with evidence: a test name, report row or file)

### M0: Baseline and research infrastructure
- [x] `docs/BASELINE.md`: build, test, fmt, clippy results at `6b79276`; OCCT integration inventory
- [x] Shared case catalogue `research/cases.json` (9 primitives, 10 booleans)
- [x] `cad-geom` seam types (math, tolerance, curve, surface, nurbs, topology) + independence test
- [x] `research/ASM_RESEARCH.md`: public sources, ASM identity (DLL version resources), API surface
- [x] ASM probe v5 (TemporaryBRepManager) → `research/evidence/asm-v5/reference.json`
- [x] `research/REA_EXPERIMENTS.md`, `research/REFERENCE_BEHAVIOR.md`
- [x] `crates/cad-diff` harness + `research/VALIDATION_REPORT.md`

### M1: Mathematical foundation
- [x] Frames, points, vectors (nalgebra), scale-aware `Tolerance::for_size`
- [x] Robust predicates: point vs plane and 2D line, filtered with a compensated fallback (`predicates.rs`)

### M2: Curves and surfaces
- [x] Analytic curve and surface evaluation, first and second derivatives
- [x] NURBS curves and surfaces: evaluation, derivatives, knot insertion, split, exact conics
- [x] Exact analytic → NURBS conversion
- [x] Point projection (analytic closed form, NURBS Newton)
- [~] Intersections: plane–plane, line–quadric/torus, plane–sphere/cylinder, line/circle curve–curve, curve–NURBS surface done; oblique plane–cone, general curve–curve pending
- [ ] Surface–surface intersection (general marching): not started

### M3: B-rep topology
- [x] Builder, queries, transform
- [x] Validation: Euler–Poincaré, edge use, loop closure, orientation, geometry on topology
- [x] Primitives: box, cylinder, cone/frustum, sphere, torus
- [x] Exact mass properties (volume, area, centroid, inertia)
- [x] Tessellation: analytic faces, planar faces with holes, trimmed curved faces

### M4: Solid modelling and booleans
- [ ] Extrude / revolve of planar profiles into B-rep
- [x] Planar (polyhedral) booleans: face–face intersection, splitting, classification, sewing (6 box cases MATCH ASM)
- [~] Booleans with analytic curved faces: planes, cylinders (parallel/perpendicular), spheres done (all 4 recorded ASM cases MATCH); oblique plane × cylinder, cones, tori, NURBS pending
- [x] Coincident and tangent handling matched to recorded ASM behaviour: coplanar merge, 1e-5 mm contact, non-manifold line contact as one lump (box–box edge contact still gives 2 lumps; no ASM row)

### M5: Advanced operations
- [ ] Chamfer, constant-radius fillet (rolling ball), shell, offset
- [ ] Sweep, loft; stitching/sewing; healing

### M6: Parametric layer
Already exists in the application (cad-core / cad-kernel, OCCT-backed): history, dependency
graph, parameters, sketch constraints, recompute, undo/redo, persistent naming, `.c360`.
Remaining work: make `cad-kernel` able to evaluate selected features through `cad-geom` behind a
switch, once those operations are `REFERENCE_VERIFIED`.

### M7: Application integration
- [ ] Optional `cad-geom` backend for primitives and mass properties in the app (behind a setting)

## Resume notes
* Build environment: `export PATH="/c/tools/cmake-3.31.8-windows-x86_64/bin:$PATH"`; target dir
  `C:/c360t` (shared by all crates).
* Kernel-only loop: `cargo test -p cad-geom && cargo test -p cad-diff && cargo run -p cad-diff`.
* Full regression: `cargo test --workspace --no-fail-fast -- --test-threads=4`.
* Re-running the ASM probe requires Fusion and the procedure in `research/REA_EXPERIMENTS.md`.
