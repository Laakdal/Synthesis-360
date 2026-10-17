# Differential validation report

Generated **2026-10-17T10:37:38Z** by `cargo run -p cad-diff` (crate `crates/cad-diff`). Raw data: `research/evidence/diff/results.json`. Case catalogue: `research/cases.json` (version 1, 19 cases).

## What was actually compared

* **cad-geom vs OCCT**: 19 of 19 cases compared field by field (MATCH 8, MISMATCH 2, TOPOLOGY_CONVENTION 9). cad-geom built 19 of 19 cases; OCCT built 19 of 19.
* **OCCT vs ASM**: 19 of 19 cases compared (MATCH 8, MISMATCH 2, TOPOLOGY_CONVENTION 9).
* **cad-geom vs ASM**: 19 of 19 cases compared (MATCH 19).
* ASM source: loaded 19 case rows from research/evidence/asm-v5/reference.json.
* **ASM vs its own STEP export re-measured by OCCT** (cross-check of the exported ASM geometry): 19 cases (MATCH 9, MISMATCH 3, TOPOLOGY_CONVENTION 7).
* `REFERENCE_VERIFIED` cases (an ASM row exists and cad-geom matches it within tolerance): **19**.
* Only the fields present on both sides of a pair are compared; each comparison in `results.json` lists them under `compared`.

## Kernels

| Kernel | Version | How the cases are built |
|---|---|---|
| cad-geom | 0.1.0 | crates/cad-geom (pure Rust, in development): primitives, boolean (planar polyhedra; box-only cases), mass::solid_properties, validate |
| occt | 7.8.1 | Open CASCADE (occt-sys 7.8.1, static) via vendored opencascade 0.3 primitives + STEP bridge into cad-kernel; booleans and measurements via cad-kernel |
| asm | 2705.1.30 | Autodesk ShapeManager through Fusion (recorded reference.json) |
| asm-step(occt) | 7.8.1 | the ASM probe's STEP exports (research/evidence/asm-v5/step) imported by cad-kernel and measured by OCCT |

ASM reference metadata: `{"extras":11,"fusion_version":"2705.1.30","global":{"point_tolerance_mm":0.00001,"vector_angle_tolerance_rad":1e-10},"kernel":"ASM","source":"Fusion 2705.1.30 TemporaryBRepManager (probe v5)","units":"mm"}`

ASM global tolerances (from the reference): point tolerance **0.00001 mm**, vector angle tolerance **1e-10 rad**. For comparison, OCCT `Precision::Confusion()` = 1e-7 mm and cad-geom `Tolerance::default()` = 1e-7 mm / 1e-12 rad; cad-geom booleans use `Tolerance::asm()` = 1e-5 mm / 1e-10 rad.

## Method

1. Every case in `research/cases.json` is built by each kernel. **cad-geom**: `cad_geom::primitives::make_*`, `mass::solid_properties`, `validate`; every call is wrapped in `catch_unwind`, so bodies that are still `unimplemented!()` become `NOT_IMPLEMENTED` rows. Booleans whose operands are all boxes, cylinders or spheres fold the tools into the target in order with `cad_geom::boolean` (polyhedral pipeline for boxes, quadric pipeline otherwise; ASM tolerance 1e-5 mm / 1e-10 rad); their results are validated at 1e-7 mm, falling back to the boolean tolerance with a note (the near-tangent case). Line contact is one edge with 4 coedges flagged in `Brep::nonmanifold_edges`, as ASM builds it. Booleans with a cone or torus operand are `NOT_IMPLEMENTED` (cad-geom returns `Unsupported`).
2. **OCCT**: primitives are true `BRepPrimAPI_Make*` primitives from the vendored `opencascade` crate. Its shape type is private and cad-kernel has no raw-shape constructor, so each primitive is handed to cad-kernel through a temporary STEP file (`write_step` → `cad_kernel::occ::Shape::read_step`, serialised by a mutex). Booleans fold the operands in order with cad-kernel `fuse`/`cut`/`common` (`BRepAlgoAPI_*` behind the exception-safe shim). Cones use the crate's `ConeBuilder`, which only builds +Z cones; every catalogue cone is +Z.
3. OCCT measurements use cad-kernel's public API only: counts are unique TopoDS entities (`TopExp::MapShapes`: solids, shells, faces, wires = loops, edges, vertices); surface and curve types from `BRepAdaptor` (GeomAbs); native degenerate edges from `BRep_Tool::Degenerated`; seam = non-degenerate edge with one distinct adjacent face; `degenerate_edges_literal` = the ASM probe's literal definition (no geometry, zero length, or start == end, so closed circles count), computed the same way for every kernel; volume/area/centroid from `inspect::mass_props` (BRepGProp); validity from `BRepCheck_Analyzer`; bbox from `BRepBndLib::Add`. **OCCT inertia is not exposed by cad-kernel**, so the OCCT inertia column is empty (harness limitation; no binding was added).
4. cad-geom counts are its arena sizes (edges include degenerate edges; seams are edges used twice by one face; literal count = no curve or start vertex == end vertex). ASM rows are loaded leniently from `research/evidence/asm-v5/reference.json` and type names normalised (e.g. `Arc3D`→circle, `NurbsSurface`→bspline).
5. Each kernel pair is classified: `MATCH`, `MISMATCH(field, a, b)`, `TOPOLOGY_CONVENTION` (volume and area agree; only counts or type multiplicities differ, e.g. seam/pole edges), `ONE_SIDED_FAILURE`, `BOTH_FAILED_SAME`, `NOT_IMPLEMENTED`, `NOT_COMPARED` (a side has no row).
6. Kernel-native `degenerate_edges` and `seam_edges` are **informational only** (ASM has no seam or pole edges; its `degenerate_edges` field is the literal count, compared as `degenerate_edges_literal`). A field that is null on either side is not compared (e.g. ASM's centroid and inertia for the tiny box, a Fusion mass-property failure at tiny volumes). Inertia is about the centroid, unit density, mm⁵, products of inertia as −∫xy dV (ASM and cad-geom).
7. Where the ASM probe exported a STEP file (`research/evidence/asm-v5/step/<id>.step`), it is imported with cad-kernel's STEP reader, measured by OCCT (`asm-step(occt)`) and compared with the ASM row: a cross-check of the exported geometry.

## Tolerances

| Constant | Value | Applies to | Reason |
|---|---|---|---|
| `REL_TOL_EXACT` | 1e-6 | volume, area, inertia (relative), cad-geom vs OCCT | both integrate the exact B-rep; quadrature error on analytic faces is far below this |
| `REL_TOL_ASM` | 1e-6 | same, when one side is ASM | ASM mass_path = base_feature (PhysicalProperties, VeryHigh accuracy, exact B-rep); measured OCCT-vs-ASM agreement is ~2e-8 or better, so the exact tolerance applies (a looser one would hide the ~9e-6 STEP-export loss on box∩sphere) |
| `REL_TOL_MESH` | 1e-3 | same, when a side's `mass_path` is mesh/low/medium accuracy | tessellation error dominates |
| `CENTROID_REL_SIZE_EXACT` | 1e-6 | centroid distance ÷ bbox diagonal (exact pair) | scales with model size (0.01 mm to 100 m boxes) |
| `CENTROID_REL_SIZE_ASM` | 1e-6 | same, ASM pairs | as REL_TOL_ASM |
| `CENTROID_ABS_FLOOR_MM` | 1e-9 | minimum centroid tolerance (mm) | keeps the tiny box above round-off |
| `BBOX_NOTE_REL` | 1e-6 | bbox (informational only) | OCCT `BRepBndLib::Add` adds the shape tolerance gap; cad-geom exposes no bbox |
| counts | exact | bodies, lumps, shells, faces, edges, vertices, loops, `degenerate_edges_literal`, edge-type multiset (native degenerate/seam counts: informational) | |
| face types | multiset equality | a different *set* of distinct face types is a MISMATCH; different multiplicities only are a convention difference | |

## STEP bridge fidelity (OCCT harness self-check)

* box: bridged == native (counts, types, validity, volume within 1e-9)
* cylinder: bridged == native (counts, types, validity, volume within 1e-9)
* cone, sphere and torus cannot be built natively in cad-kernel, so the bridge is not cross-checked for them (the STEP reader may rebuild their seam and degenerate edges).

## Per-case results

Kernel cells: volume (mm³) and unique face/edge/vertex counts. Closed-form error = |V_occt − V_exact| / V_exact for primitives.

| Case | cad-geom | OCCT | ASM | OCCT vs closed form | cad-geom vs OCCT | OCCT vs ASM | cad-geom vs ASM | Verdict |
|---|---|---|---|---|---|---|---|---|
| `prim.box` | V=15000 F/E/V=6/12/8 | V=15000 F/E/V=6/12/8 | V=15000 F/E/V=6/12/8 | 0.00e0 | MATCH | MATCH | MATCH | REFERENCE_VERIFIED |
| `prim.box.tiny` | V=6.000000e-6 F/E/V=6/12/8 | V=6.000000e-6 F/E/V=6/12/8 | V=6.000000e-6 F/E/V=6/12/8 | 1.41e-16 | MATCH | MATCH | MATCH | REFERENCE_VERIFIED |
| `prim.box.huge` | V=1.000000e14 F/E/V=6/12/8 | V=1.000000e14 F/E/V=6/12/8 | V=1.000000e14 F/E/V=6/12/8 | 0.00e0 | MATCH | MATCH | MATCH | REFERENCE_VERIFIED |
| `prim.cylinder` | V=6283.185307 F/E/V=3/2/2 | V=6283.185307 F/E/V=3/3/2 | V=6283.185307 F/E/V=3/2/2 | 0.00e0 | TOPOLOGY_CONVENTION(edges, 2, 3; loops, 4, 3; edge_types, ["circle", "circle"], ["circle", "circle", "line"]) | TOPOLOGY_CONVENTION(edges, 3, 2; loops, 3, 4; edge_types, ["circle", "circle", "line"], ["circle", "circle"]) | MATCH | REFERENCE_VERIFIED |
| `prim.cylinder.tilted` | V=753.982237 F/E/V=3/2/2 | V=753.982237 F/E/V=3/3/2 | V=753.982237 F/E/V=3/2/2 | 1.87e-14 | TOPOLOGY_CONVENTION(edges, 2, 3; loops, 4, 3; edge_types, ["circle", "circle"], ["circle", "circle", "line"]) | TOPOLOGY_CONVENTION(edges, 3, 2; loops, 3, 4; edge_types, ["circle", "circle", "line"], ["circle", "circle"]) | MATCH | REFERENCE_VERIFIED |
| `prim.cone` | V=2094.395102 F/E/V=2/2/2 | V=2094.395102 F/E/V=2/3/2 | V=2094.395102 F/E/V=2/2/2 | 4.85e-13 | TOPOLOGY_CONVENTION(edges, 2, 3; loops, 3, 2; edge_types, ["circle"], ["circle", "line"]) | TOPOLOGY_CONVENTION(edges, 3, 2; loops, 2, 3; edge_types, ["circle", "line"], ["circle"]) | MATCH | REFERENCE_VERIFIED |
| `prim.frustum` | V=3665.191429 F/E/V=3/2/2 | V=3665.191429 F/E/V=3/3/2 | V=3665.191429 F/E/V=3/2/2 | 2.62e-13 | TOPOLOGY_CONVENTION(edges, 2, 3; loops, 4, 3; edge_types, ["circle", "circle"], ["circle", "circle", "line"]) | TOPOLOGY_CONVENTION(edges, 3, 2; loops, 3, 4; edge_types, ["circle", "circle", "line"], ["circle", "circle"]) | MATCH | REFERENCE_VERIFIED |
| `prim.sphere` | V=4188.790205 F/E/V=1/0/0 | V=4188.790205 F/E/V=1/3/2 | V=4188.790205 F/E/V=1/0/0 | 2.17e-16 | TOPOLOGY_CONVENTION(edges, 0, 3; vertices, 0, 2; loops, 0, 1; degenerate_edges_literal, 0, 2; edge_types, [], ["circle"]) | TOPOLOGY_CONVENTION(edges, 3, 0; vertices, 2, 0; loops, 1, 0; degenerate_edges_literal, 2, 0; edge_types, ["circle"], []) | MATCH | REFERENCE_VERIFIED |
| `prim.torus` | V=9869.604401 F/E/V=1/0/0 | V=9869.604401 F/E/V=1/2/1 | V=9869.604401 F/E/V=1/0/0 | 4.90e-14 | TOPOLOGY_CONVENTION(edges, 0, 2; vertices, 0, 1; loops, 0, 1; degenerate_edges_literal, 0, 2; edge_types, [], ["circle", "circle"]) | TOPOLOGY_CONVENTION(edges, 2, 0; vertices, 1, 0; loops, 1, 0; degenerate_edges_literal, 2, 0; edge_types, ["circle", "circle"], []) | MATCH | REFERENCE_VERIFIED |
| `bool.union.overlap` | V=15000 F/E/V=12/30/20 | V=15000 F/E/V=12/30/20 | V=15000 F/E/V=12/30/20 | — | MATCH | MATCH | MATCH | REFERENCE_VERIFIED |
| `bool.cut.hole` | V=13869.026645 F/E/V=7/14/10 | V=13869.026645 F/E/V=7/15/10 | V=13869.026645 F/E/V=7/14/10 | — | TOPOLOGY_CONVENTION(edges, 14, 15; loops, 10, 9; edge_types, ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "li… | TOPOLOGY_CONVENTION(edges, 15, 14; loops, 9, 10; edge_types, ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "li… | MATCH | REFERENCE_VERIFIED |
| `bool.union.coincident_face` | V=16000 F/E/V=6/12/8 | V=16000 F/E/V=10/20/12 | V=16000 F/E/V=6/12/8 | — | TOPOLOGY_CONVENTION(faces, 6, 10; edges, 12, 20; vertices, 8, 12; loops, 6, 10; edge_types, ["line", "line", "line", "line", "line", "line", "line", "line", … | TOPOLOGY_CONVENTION(faces, 10, 6; edges, 20, 12; vertices, 12, 8; loops, 10, 6; edge_types, ["line", "line", "line", "line", "line", "line", "line", "line", … | MATCH | REFERENCE_VERIFIED |
| `bool.union.coincident_partial` | V=10000 F/E/V=11/24/16 | V=10000 F/E/V=11/24/16 | V=10000 F/E/V=11/24/16 | — | MATCH | MATCH | MATCH | REFERENCE_VERIFIED |
| `bool.cut.coplanar_through` | V=6000 F/E/V=10/24/16 | V=6000 F/E/V=10/24/16 | V=6000 F/E/V=10/24/16 | — | MATCH | MATCH | MATCH | REFERENCE_VERIFIED |
| `bool.union.tangent_cylinder` | V=14283.185307 F/E/V=10/17/10 | V=14283.185307 F/E/V=11/20/12 | V=14283.185307 F/E/V=10/17/10 | — | MISMATCH(bodies, 1, 2; lumps, 1, 2; shells, 1, 2; faces, 10, 11; edges, 17, 20; vertices, 10, 12; loops, 10, 11; degenerate_edges_literal, 2, 0; edge_types, … | MISMATCH(bodies, 2, 1; lumps, 2, 1; shells, 2, 1; faces, 11, 10; edges, 20, 17; vertices, 12, 10; loops, 11, 10; degenerate_edges_literal, 0, 2; edge_types, … | MATCH | REFERENCE_VERIFIED |
| `bool.union.near_tangent_cylinder` | V=14283.184974 F/E/V=10/17/10 | V=14283.185303 F/E/V=13/25/14 | V=14283.185307 F/E/V=10/17/10 | — | MISMATCH(area, 4284.955492153877, 4283.824221021223; faces, 10, 13; edges, 17, 25; vertices, 10, 14; loops, 10, 13; degenerate_edges_literal, 2, 0; edge_type… | MISMATCH(area, 4283.824221021223, 4284.955592153876; faces, 13, 10; edges, 25, 17; vertices, 14, 10; loops, 13, 10; degenerate_edges_literal, 0, 2; edge_type… | MATCH | REFERENCE_VERIFIED |
| `bool.intersect.box_sphere` | V=6383.716272 F/E/V=7/6/6 | V=6383.716137 F/E/V=7/9/7 | V=6383.716274 F/E/V=7/6/6 | — | TOPOLOGY_CONVENTION(edges, 6, 9; vertices, 6, 7; loops, 12, 10; degenerate_edges_literal, 6, 5; edge_types, ["circle", "circle", "circle", "circle", "circle"… | TOPOLOGY_CONVENTION(edges, 9, 6; vertices, 7, 6; loops, 10, 12; degenerate_edges_literal, 5, 6; edge_types, ["circle", "circle", "circle", "circle", "circle"… | MATCH | REFERENCE_VERIFIED |
| `bool.cut.disjoint` | V=1000 F/E/V=6/12/8 | V=1000 F/E/V=6/12/8 | V=1000 F/E/V=6/12/8 | — | MATCH | MATCH | MATCH | REFERENCE_VERIFIED |
| `bool.union.repeated` | V=4320 F/E/V=31/72/48 | V=4320 F/E/V=31/72/48 | V=4320 F/E/V=31/72/48 | — | MATCH | MATCH | MATCH | REFERENCE_VERIFIED |

### ASM STEP exports re-measured by OCCT

| Case | ASM (reported) | ASM STEP → OCCT | ASM vs ASM-STEP(OCCT) |
|---|---|---|---|
| `prim.box` | V=15000 F/E/V=6/12/8 | V=15000 F/E/V=6/12/8 | MATCH |
| `prim.box.tiny` | V=6.000000e-6 F/E/V=6/12/8 | V=6.000000e-6 F/E/V=6/12/8 | MATCH |
| `prim.box.huge` | V=1.000000e14 F/E/V=6/12/8 | V=1.000000e14 F/E/V=6/12/8 | MATCH |
| `prim.cylinder` | V=6283.185307 F/E/V=3/2/2 | V=6283.185307 F/E/V=3/3/2 | TOPOLOGY_CONVENTION(edges, 2, 3; loops, 4, 3; edge_types, ["circle", "circle"], ["circle", "circle", "line"]) |
| `prim.cylinder.tilted` | V=753.982237 F/E/V=3/2/2 | V=753.982237 F/E/V=3/3/2 | TOPOLOGY_CONVENTION(edges, 2, 3; loops, 4, 3; edge_types, ["circle", "circle"], ["circle", "circle", "line"]) |
| `prim.cone` | V=2094.395102 F/E/V=2/2/2 | V=2094.395102 F/E/V=2/3/2 | TOPOLOGY_CONVENTION(edges, 2, 3; loops, 3, 2; edge_types, ["circle"], ["circle", "line"]) |
| `prim.frustum` | V=3665.191429 F/E/V=3/2/2 | V=3665.191429 F/E/V=3/3/2 | TOPOLOGY_CONVENTION(edges, 2, 3; loops, 4, 3; edge_types, ["circle", "circle"], ["circle", "circle", "line"]) |
| `prim.sphere` | V=4188.790205 F/E/V=1/0/0 | V=4188.790205 F/E/V=1/3/2 | TOPOLOGY_CONVENTION(edges, 0, 3; vertices, 0, 2; loops, 0, 1; degenerate_edges_literal, 0, 2; edge_types, [], ["circle"]) |
| `prim.torus` | V=9869.604401 F/E/V=1/0/0 | V=9869.604401 F/E/V=1/2/1 | TOPOLOGY_CONVENTION(edges, 0, 2; vertices, 0, 1; loops, 0, 1; degenerate_edges_literal, 0, 2; edge_types, [], ["circle", "circle"]) |
| `bool.union.overlap` | V=15000 F/E/V=12/30/20 | V=15000 F/E/V=12/30/20 | MATCH |
| `bool.cut.hole` | V=13869.026645 F/E/V=7/14/10 | V=13869.026645 F/E/V=7/15/10 | TOPOLOGY_CONVENTION(edges, 14, 15; loops, 10, 9; edge_types, ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "li… |
| `bool.union.coincident_face` | V=16000 F/E/V=6/12/8 | V=16000 F/E/V=6/12/8 | MATCH |
| `bool.union.coincident_partial` | V=10000 F/E/V=11/24/16 | V=10000 F/E/V=11/24/16 | MATCH |
| `bool.cut.coplanar_through` | V=6000 F/E/V=10/24/16 | V=6000 F/E/V=10/24/16 | MATCH |
| `bool.union.tangent_cylinder` | V=14283.185307 F/E/V=10/17/10 | V=14283.185307 F/E/V=10/18/12 | MISMATCH(bodies, 1, 2; lumps, 1, 2; shells, 1, 2; edges, 17, 18; vertices, 10, 12; edge_types, ["circle", "circle", "line", "line", "line", "line", "line", "… |
| `bool.union.near_tangent_cylinder` | V=14283.185307 F/E/V=10/17/10 | V=14283.185307 F/E/V=10/18/12 | MISMATCH(bodies, 1, 2; lumps, 1, 2; shells, 1, 2; edges, 17, 18; vertices, 10, 12; edge_types, ["circle", "circle", "line", "line", "line", "line", "line", "… |
| `bool.intersect.box_sphere` | V=6383.716274 F/E/V=7/6/6 | V=6383.660486 F/E/V=7/9/7 | MISMATCH(volume, 6383.716273841073, 6383.660486083535; area, 1734.1591447824858, 1734.1461671986103; edges, 6, 9; vertices, 6, 7; loops, 12, 10; degenerate_e… |
| `bool.cut.disjoint` | V=1000 F/E/V=6/12/8 | V=1000 F/E/V=6/12/8 | MATCH |
| `bool.union.repeated` | V=4320 F/E/V=31/72/48 | V=4320 F/E/V=31/72/48 | MATCH |

## Discrepancies

* `prim.cylinder` cad-geom vs occt: **TOPOLOGY_CONVENTION**
  * edges: cad-geom = 2, occt = 3
  * loops: cad-geom = 4, occt = 3
  * edge_types: cad-geom = ["circle", "circle"], occt = ["circle", "circle", "line"]
  * note: seam_edges (informational): cad-geom 0, occt 1
* `prim.cylinder` occt vs asm: **TOPOLOGY_CONVENTION**
  * edges: occt = 3, asm = 2
  * loops: occt = 3, asm = 4
  * edge_types: occt = ["circle", "circle", "line"], asm = ["circle", "circle"]
  * note: seam_edges (informational): occt 1, asm 0
* `prim.cylinder` asm vs asm-step(occt): **TOPOLOGY_CONVENTION**
  * edges: asm = 2, asm-step(occt) = 3
  * loops: asm = 4, asm-step(occt) = 3
  * edge_types: asm = ["circle", "circle"], asm-step(occt) = ["circle", "circle", "line"]
  * note: seam_edges (informational): asm 0, asm-step(occt) 1
* `prim.cylinder.tilted` cad-geom vs occt: **TOPOLOGY_CONVENTION**
  * edges: cad-geom = 2, occt = 3
  * loops: cad-geom = 4, occt = 3
  * edge_types: cad-geom = ["circle", "circle"], occt = ["circle", "circle", "line"]
  * note: seam_edges (informational): cad-geom 0, occt 1
* `prim.cylinder.tilted` occt vs asm: **TOPOLOGY_CONVENTION**
  * edges: occt = 3, asm = 2
  * loops: occt = 3, asm = 4
  * edge_types: occt = ["circle", "circle", "line"], asm = ["circle", "circle"]
  * note: seam_edges (informational): occt 1, asm 0
* `prim.cylinder.tilted` asm vs asm-step(occt): **TOPOLOGY_CONVENTION**
  * edges: asm = 2, asm-step(occt) = 3
  * loops: asm = 4, asm-step(occt) = 3
  * edge_types: asm = ["circle", "circle"], asm-step(occt) = ["circle", "circle", "line"]
  * note: seam_edges (informational): asm 0, asm-step(occt) 1
* `prim.cone` cad-geom vs occt: **TOPOLOGY_CONVENTION**
  * edges: cad-geom = 2, occt = 3
  * loops: cad-geom = 3, occt = 2
  * edge_types: cad-geom = ["circle"], occt = ["circle", "line"]
  * note: seam_edges (informational): cad-geom 0, occt 1
* `prim.cone` occt vs asm: **TOPOLOGY_CONVENTION**
  * edges: occt = 3, asm = 2
  * loops: occt = 2, asm = 3
  * edge_types: occt = ["circle", "line"], asm = ["circle"]
  * note: seam_edges (informational): occt 1, asm 0
* `prim.cone` asm vs asm-step(occt): **TOPOLOGY_CONVENTION**
  * edges: asm = 2, asm-step(occt) = 3
  * loops: asm = 3, asm-step(occt) = 2
  * edge_types: asm = ["circle"], asm-step(occt) = ["circle", "line"]
  * note: seam_edges (informational): asm 0, asm-step(occt) 1
* `prim.frustum` cad-geom vs occt: **TOPOLOGY_CONVENTION**
  * edges: cad-geom = 2, occt = 3
  * loops: cad-geom = 4, occt = 3
  * edge_types: cad-geom = ["circle", "circle"], occt = ["circle", "circle", "line"]
  * note: seam_edges (informational): cad-geom 0, occt 1
* `prim.frustum` occt vs asm: **TOPOLOGY_CONVENTION**
  * edges: occt = 3, asm = 2
  * loops: occt = 3, asm = 4
  * edge_types: occt = ["circle", "circle", "line"], asm = ["circle", "circle"]
  * note: seam_edges (informational): occt 1, asm 0
* `prim.frustum` asm vs asm-step(occt): **TOPOLOGY_CONVENTION**
  * edges: asm = 2, asm-step(occt) = 3
  * loops: asm = 4, asm-step(occt) = 3
  * edge_types: asm = ["circle", "circle"], asm-step(occt) = ["circle", "circle", "line"]
  * note: seam_edges (informational): asm 0, asm-step(occt) 1
* `prim.sphere` cad-geom vs occt: **TOPOLOGY_CONVENTION**
  * edges: cad-geom = 0, occt = 3
  * vertices: cad-geom = 0, occt = 2
  * loops: cad-geom = 0, occt = 1
  * degenerate_edges_literal: cad-geom = 0, occt = 2
  * edge_types: cad-geom = [], occt = ["circle"]
  * note: degenerate_edges (informational): cad-geom 0, occt 2
  * note: seam_edges (informational): cad-geom 0, occt 1
* `prim.sphere` occt vs asm: **TOPOLOGY_CONVENTION**
  * edges: occt = 3, asm = 0
  * vertices: occt = 2, asm = 0
  * loops: occt = 1, asm = 0
  * degenerate_edges_literal: occt = 2, asm = 0
  * edge_types: occt = ["circle"], asm = []
  * note: degenerate_edges (informational): occt 2, asm 0
  * note: seam_edges (informational): occt 1, asm 0
* `prim.sphere` asm vs asm-step(occt): **TOPOLOGY_CONVENTION**
  * edges: asm = 0, asm-step(occt) = 3
  * vertices: asm = 0, asm-step(occt) = 2
  * loops: asm = 0, asm-step(occt) = 1
  * degenerate_edges_literal: asm = 0, asm-step(occt) = 2
  * edge_types: asm = [], asm-step(occt) = ["circle"]
  * note: degenerate_edges (informational): asm 0, asm-step(occt) 2
  * note: seam_edges (informational): asm 0, asm-step(occt) 1
* `prim.torus` cad-geom vs occt: **TOPOLOGY_CONVENTION**
  * edges: cad-geom = 0, occt = 2
  * vertices: cad-geom = 0, occt = 1
  * loops: cad-geom = 0, occt = 1
  * degenerate_edges_literal: cad-geom = 0, occt = 2
  * edge_types: cad-geom = [], occt = ["circle", "circle"]
  * note: seam_edges (informational): cad-geom 0, occt 2
* `prim.torus` occt vs asm: **TOPOLOGY_CONVENTION**
  * edges: occt = 2, asm = 0
  * vertices: occt = 1, asm = 0
  * loops: occt = 1, asm = 0
  * degenerate_edges_literal: occt = 2, asm = 0
  * edge_types: occt = ["circle", "circle"], asm = []
  * note: seam_edges (informational): occt 2, asm 0
  * note: bbox differs by up to 5.000e0 mm (informational)
* `prim.torus` asm vs asm-step(occt): **TOPOLOGY_CONVENTION**
  * edges: asm = 0, asm-step(occt) = 2
  * vertices: asm = 0, asm-step(occt) = 1
  * loops: asm = 0, asm-step(occt) = 1
  * degenerate_edges_literal: asm = 0, asm-step(occt) = 2
  * edge_types: asm = [], asm-step(occt) = ["circle", "circle"]
  * note: seam_edges (informational): asm 0, asm-step(occt) 2
  * note: bbox differs by up to 5.000e0 mm (informational)
* `bool.cut.hole` cad-geom vs occt: **TOPOLOGY_CONVENTION**
  * edges: cad-geom = 14, occt = 15
  * loops: cad-geom = 10, occt = 9
  * edge_types: cad-geom = ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"], occt = ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"]
  * note: seam_edges (informational): cad-geom 0, occt 1
* `bool.cut.hole` occt vs asm: **TOPOLOGY_CONVENTION**
  * edges: occt = 15, asm = 14
  * loops: occt = 9, asm = 10
  * edge_types: occt = ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"], asm = ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"]
  * note: seam_edges (informational): occt 1, asm 0
* `bool.cut.hole` asm vs asm-step(occt): **TOPOLOGY_CONVENTION**
  * edges: asm = 14, asm-step(occt) = 15
  * loops: asm = 10, asm-step(occt) = 9
  * edge_types: asm = ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"], asm-step(occt) = ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"]
  * note: seam_edges (informational): asm 0, asm-step(occt) 1
* `bool.union.coincident_face` cad-geom vs occt: **TOPOLOGY_CONVENTION**
  * faces: cad-geom = 6, occt = 10
  * edges: cad-geom = 12, occt = 20
  * vertices: cad-geom = 8, occt = 12
  * loops: cad-geom = 6, occt = 10
  * edge_types: cad-geom = ["line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"], occt = ["line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"]
  * face_types: cad-geom = ["plane", "plane", "plane", "plane", "plane", "plane"], occt = ["plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane"]
* `bool.union.coincident_face` occt vs asm: **TOPOLOGY_CONVENTION**
  * faces: occt = 10, asm = 6
  * edges: occt = 20, asm = 12
  * vertices: occt = 12, asm = 8
  * loops: occt = 10, asm = 6
  * edge_types: occt = ["line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"], asm = ["line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"]
  * face_types: occt = ["plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane"], asm = ["plane", "plane", "plane", "plane", "plane", "plane"]
* `bool.union.tangent_cylinder` cad-geom vs occt: **MISMATCH**
  * bodies: cad-geom = 1, occt = 2
  * lumps: cad-geom = 1, occt = 2
  * shells: cad-geom = 1, occt = 2
  * faces: cad-geom = 10, occt = 11
  * edges: cad-geom = 17, occt = 20
  * vertices: cad-geom = 10, occt = 12
  * loops: cad-geom = 10, occt = 11
  * degenerate_edges_literal: cad-geom = 2, occt = 0
  * edge_types: cad-geom = ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"], occt = ["circle", "circle", "circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"]
  * face_types: cad-geom = ["cylinder", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane"], occt = ["cylinder", "cylinder", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane"]
  * note: seam_edges (informational): cad-geom 1, occt 0
* `bool.union.tangent_cylinder` occt vs asm: **MISMATCH**
  * bodies: occt = 2, asm = 1
  * lumps: occt = 2, asm = 1
  * shells: occt = 2, asm = 1
  * faces: occt = 11, asm = 10
  * edges: occt = 20, asm = 17
  * vertices: occt = 12, asm = 10
  * loops: occt = 11, asm = 10
  * degenerate_edges_literal: occt = 0, asm = 2
  * edge_types: occt = ["circle", "circle", "circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"], asm = ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"]
  * face_types: occt = ["cylinder", "cylinder", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane"], asm = ["cylinder", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane"]
  * note: seam_edges (informational): occt 0, asm 1
* `bool.union.tangent_cylinder` asm vs asm-step(occt): **MISMATCH**
  * bodies: asm = 1, asm-step(occt) = 2
  * lumps: asm = 1, asm-step(occt) = 2
  * shells: asm = 1, asm-step(occt) = 2
  * edges: asm = 17, asm-step(occt) = 18
  * vertices: asm = 10, asm-step(occt) = 12
  * edge_types: asm = ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"], asm-step(occt) = ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"]
* `bool.union.near_tangent_cylinder` cad-geom vs occt: **MISMATCH**
  * area: cad-geom = 4284.955492153877, occt = 4283.824221021223
  * faces: cad-geom = 10, occt = 13
  * edges: cad-geom = 17, occt = 25
  * vertices: cad-geom = 10, occt = 14
  * loops: cad-geom = 10, occt = 13
  * degenerate_edges_literal: cad-geom = 2, occt = 0
  * edge_types: cad-geom = ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"], occt = ["circle", "circle", "circle", "circle", "circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"]
  * face_types: cad-geom = ["cylinder", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane"], occt = ["cylinder", "cylinder", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane"]
  * note: seam_edges (informational): cad-geom 1, occt 0
* `bool.union.near_tangent_cylinder` occt vs asm: **MISMATCH**
  * area: occt = 4283.824221021223, asm = 4284.955592153876
  * faces: occt = 13, asm = 10
  * edges: occt = 25, asm = 17
  * vertices: occt = 14, asm = 10
  * loops: occt = 13, asm = 10
  * degenerate_edges_literal: occt = 0, asm = 2
  * edge_types: occt = ["circle", "circle", "circle", "circle", "circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"], asm = ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"]
  * face_types: occt = ["cylinder", "cylinder", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane"], asm = ["cylinder", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane", "plane"]
  * note: seam_edges (informational): occt 0, asm 1
* `bool.union.near_tangent_cylinder` asm vs asm-step(occt): **MISMATCH**
  * bodies: asm = 1, asm-step(occt) = 2
  * lumps: asm = 1, asm-step(occt) = 2
  * shells: asm = 1, asm-step(occt) = 2
  * edges: asm = 17, asm-step(occt) = 18
  * vertices: asm = 10, asm-step(occt) = 12
  * edge_types: asm = ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"], asm-step(occt) = ["circle", "circle", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line", "line"]
* `bool.intersect.box_sphere` cad-geom vs occt: **TOPOLOGY_CONVENTION**
  * edges: cad-geom = 6, occt = 9
  * vertices: cad-geom = 6, occt = 7
  * loops: cad-geom = 12, occt = 10
  * degenerate_edges_literal: cad-geom = 6, occt = 5
  * edge_types: cad-geom = ["circle", "circle", "circle", "circle", "circle", "circle"], occt = ["circle", "circle", "circle", "circle", "circle", "circle", "circle", "circle", "circle"]
  * note: seam_edges (informational): cad-geom 0, occt 2
* `bool.intersect.box_sphere` occt vs asm: **TOPOLOGY_CONVENTION**
  * edges: occt = 9, asm = 6
  * vertices: occt = 7, asm = 6
  * loops: occt = 10, asm = 12
  * degenerate_edges_literal: occt = 5, asm = 6
  * edge_types: occt = ["circle", "circle", "circle", "circle", "circle", "circle", "circle", "circle", "circle"], asm = ["circle", "circle", "circle", "circle", "circle", "circle"]
  * note: seam_edges (informational): occt 2, asm 0
  * note: bbox differs by up to 2.000e0 mm (informational)
* `bool.intersect.box_sphere` asm vs asm-step(occt): **MISMATCH**
  * volume: asm = 6383.716273841073, asm-step(occt) = 6383.660486083535
  * area: asm = 1734.1591447824858, asm-step(occt) = 1734.1461671986103
  * edges: asm = 6, asm-step(occt) = 9
  * vertices: asm = 6, asm-step(occt) = 7
  * loops: asm = 12, asm-step(occt) = 10
  * degenerate_edges_literal: asm = 6, asm-step(occt) = 5
  * edge_types: asm = ["circle", "circle", "circle", "circle", "circle", "circle"], asm-step(occt) = ["circle", "circle", "circle", "circle", "circle", "circle", "circle", "circle", "circle"]
  * note: seam_edges (informational): asm 0, asm-step(occt) 2
  * note: bbox differs by up to 2.000e0 mm (informational)

## Harness limitations

* OCCT inertia tensors are not compared: cad-kernel exposes `mass_props` = (volume, area, centroid) only.
* OCCT primitives pass through a STEP write/read (see the fidelity check above); booleans therefore operate on STEP-read operands.
* OCCT bounding boxes are tolerance-enlarged and not tight on curved faces; bbox is never a MATCH criterion.
* cad-geom is under active development by other workers; its rows are a snapshot at the timestamp above.
