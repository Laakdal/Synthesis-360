# REA experiments: Autodesk Fusion / ShapeManager (ASM) reference runs

## Earlier experiments (v1–v4)

Passes v1–v4 used the `CAD360Probe` add-in to cover the UI/workflow, sketch, joints and plane-frame
probes. They are documented in [`docs/evidence/findings-ledger.md`](../docs/evidence/findings-ledger.md),
with the method in [`docs/REA-readiness.md`](../docs/REA-readiness.md). They are not repeated here.

---

## Experiment v5: ASM reference results through `TemporaryBRepManager`

**Date:** 2026-10-17 · **Status:** completed; every case and extra ran with no step errors and no timeouts.

### Purpose

This experiment produces reference results from Autodesk ShapeManager (ASM), the kernel inside Fusion,
for every case in [`cases.json`](cases.json). The Rust differential harness (`crates/cad-geom`) can
then measure its parity against ASM. The results cover topology counts, surface and curve types,
degenerate and seam edges, volume, area, centroid, inertia, bounding box, tolerances and exported files.
Extra probes cover degenerate inputs, a self-intersecting torus and a size-limit sweep.

ASM was treated as a black box. It was driven only through Fusion's **public Python API**. No
Fusion or ASM binary was disassembled, decompiled or inspected. The only on-disk facts used were the
ASM DLL file names in the install folder and the shipped public API type stubs
(`Api\Python\packages\adsk\defs\adsk\*.py`), which were used for method signatures.

### Environment

| Item | Value |
|---|---|
| Host | Windows 11 Pro 10.0.22631, x64 |
| Fusion | `app.version` = **2705.1.30**, at `%LOCALAPPDATA%\Autodesk\webdeploy\production\736d2b4c5e6b95e77c11721da222cf21c6a1fc1f\Fusion360.exe` |
| Embedded Python | 3.14.0 (Fusion's own; also used offline, with `-I`, to post-process the log) |
| ASM DLLs present (names only) | `ASMKERN232.dll`, `ASMBASE232.dll`, `ASMBOOL232.dll`, `ASMSBOOL232.dll`, `ASMINTR232.dll`, `ASMLOP232.dll`, `ASMLOPT232.dll`, `ASMOFST232.dll`, `ASMBLND232.dll`, `ASMSHL232.dll`, `ASMSKIN232.dll`, `ASMSWP232.dll`, `ASMHEAL232.dll`, `ASMCOVR232.dll`, `ASMRBASE232.dll`, `ASMRBI232.dll`, `ASMREM232.dll`, `ASMTWK232.dll`, `ASMDEFM232.dll`, `ASMEULR232.dll`, `ASMFCT232.dll`, `ASMGA232.dll`, `ASMIMPORT232.dll`, `ASMDATAX232.dll`, `ASMLAW232.dll`, `ASMNPCH232.dll`, `ASMOPER232.dll`, `ASMPID232.dll`, `ASMSASM232.dll`, `ASMSBAP232.dll`, `ASMTOPT232.dll`, `ASMUFLD232.dll`, `ASMWELD232.dll`, `ASMCSTR232.dll`, `ASMCT232.dll`, `ASMAHL232.dll`, `ASMFREC232.dll`, `atf_asm_interface.dll`, `NsASMInterface10.dll`, `NsASMEntityTracking10.dll` (the suffix `232` is the ASM library generation) |
| Global tolerances | `Application.pointTolerance` = 1e-6 cm = **1e-5 mm**; `Application.vectorAngleTolerance` = **1e-10 rad** |
| Licence / cloud | signed-in Education licence; nothing saved; every document was closed with `close(False)` |

### API signatures used

These were read at runtime with `inspect.signature`, and the full docstrings are in the raw log under
`api_signatures`.

- `TemporaryBRepManager.createBox(box: OrientedBoundingBox3D) -> BRepBody`
- `OrientedBoundingBox3D.create(centerPoint, lengthDirection, widthDirection, length, width, height)`. Centre = corner + size/2, length along +X, width along +Y; values in cm.
- `TemporaryBRepManager.createCylinderOrCone(pointOne, pointOneRadius, pointTwo, pointTwoRadius) -> BRepBody`, with pointTwo = base + normalised axis · h.
- `TemporaryBRepManager.createSphere(center, radius) -> BRepBody`
- `TemporaryBRepManager.createTorus(center, axis, majorRadius, minorRadius) -> BRepBody`
- `TemporaryBRepManager.booleanOperation(targetBody, toolBody, booleanType) -> bool`. This modifies the target in place; each tool is applied in order. `BooleanTypes`: Difference=0, Intersection=1, Union=2.
- `TemporaryBRepManager.exportToFile(bodies: list[BRepBody], filename) -> bool`
- `BRepBody.getPhysicalProperties(accuracy)` (with `VeryHighCalculationAccuracy`), `Component.getPhysicalProperties(accuracy)`, `PhysicalProperties.getXYZMomentsOfInertia() -> (ok, xx, yy, zz, xy, yz, xz)`
- `BRepBodies.add(body, targetBaseFeature)` inside `BaseFeature.startEdit()` / `finishEdit()`, in a Parametric design.

### Procedure (exact)

1. **Pre-check.** `Get-Process Fusion360` returned nothing (Fusion was not running), and
   `%APPDATA%\Autodesk\Autodesk Fusion 360\API\AddIns\` was empty.
2. **Add-in source.** `docs/evidence/fusion-probe/addin-src/CAD360AsmProbe/` contains
   `CAD360AsmProbe.py` and `CAD360AsmProbe.manifest` (`runOnStartup: true`). It uses the same
   pattern as `CAD360Probe`:
   - It is sentinel-gated. It does nothing unless `%LOCALAPPDATA%\Temp\CAD360AsmProbe-out\RUN_ASM_PROBE`
     exists, and deletes the sentinel on start.
   - A worker thread fires one custom event per step, with a 45 s settle time, a 300 s timeout per
     step and 1 s between steps.
   - Every API call is wrapped so that one failure can't stop the pass.
   - Each step appends one JSON record to `probe-log.jsonl`.
3. **Install and run (PowerShell).**
   ```powershell
   $stage = "$env:LOCALAPPDATA\Temp\CAD360AsmProbe-out"; New-Item -ItemType Directory -Force $stage
   $dst = "$env:APPDATA\Autodesk\Autodesk Fusion 360\API\AddIns\CAD360AsmProbe"; New-Item -ItemType Directory -Force $dst
   Copy-Item "docs\evidence\fusion-probe\addin-src\CAD360AsmProbe\*" $dst
   Set-Content "$stage\RUN_ASM_PROBE" "v5"
   Start-Process "$env:LOCALAPPDATA\Autodesk\webdeploy\production\736d2b4c5e6b95e77c11721da222cf21c6a1fc1f\Fusion360.exe"
   ```
4. **Steps.** The add-in ran these in order:
   1. Environment, tolerances and signatures.
   2. The 19 catalogue cases.
   3. Extras (a), the degenerate inputs.
   4. Extras (b), the self-intersecting tori.
   5. Extras (c), the size sweep, with the largest sizes last.
   6. Close all documents without saving, then write `DONE`.

   For each case:
   - Build the body with `TemporaryBRepManager` and measure it: topology, per-face loops, coedges per
     edge, edge and vertex tolerances, `body.volume/area`, and the bounding box.
   - Call `getPhysicalProperties` on the **temporary** body.
   - Call `exportToFile` with `.smt`, `.sat`, `.smb`, `.sab` and `.step`.
   - Open a new unsaved design and switch it to Parametric. Add the body to a base feature
     (`startEdit` / `BRepBodies.add(body, bf)` / `finishEdit`), then read the per-body and
     component `getPhysicalProperties(VeryHigh)`.
   - For catalogue cases only, export STEP through `exportManager.createSTEPExportOptions(path, rootComponent)`.
   - Close the document without saving.
5. **Close.** `DONE` appeared a few minutes after launch. `CloseMainWindow()` was called on the Fusion
   process, and it exited normally with no save prompt.
6. **Uninstall.** After the process had exited:
   `Remove-Item -Recurse "$env:APPDATA\Autodesk\Autodesk Fusion 360\API\AddIns\CAD360AsmProbe"`.
   The AddIns folder was empty afterwards.
7. **Post-process.** An offline script, run with Fusion's `python.exe -I` and kept outside `research/`:
   - Reads the staging log.
   - Scrubs absolute user-profile paths in any separator form (replaced with `%APPDATA%`,
     `%LOCALAPPDATA%` or `<userprofile>`) and the Windows user name (replaced with `<user>`).
   - Normalises the results to mm into `evidence/asm-v5/reference.json`.
   - Writes the scrubbed raw log to `evidence/asm-v5/raw/probe-log.jsonl`.
   - Copies the scrubbed `.step`, `.sat` and `.smt` files.

   A final grep of `research/` for the user name and for absolute user-profile paths returned zero hits.

### Normalisation rules (`reference.json`)

- **Units.** Lengths are in mm, areas in mm², volumes in mm³ and inertia in mm⁵. Raw cm values are in the raw log.
- **Mass path.** `mass_path` is `"base_feature"` for every case. The temporary-body path returned
  zeros, as recorded in the behaviour table. `volume`, `area`, `centroid` and `inertia_centroid` come
  from the base-feature component's `getPhysicalProperties(VeryHighCalculationAccuracy)`.
- **Inertia.** `inertia_centroid` is the full tensor about the centroid, for unit density, with
  products in tensor form: I_xy = −∫(x−cx)(y−cy) dV. Fusion reports kg·cm² about the **world origin**,
  with products already in tensor form, at density 0.00785 kg/cm³. The script applies the
  parallel-axis shift to the centroid, divides by the density and multiplies by 1e5.
- **Rejected mass data.** The centroid and inertia are set to `null` when the reported centre of mass
  is not strictly inside the bounding box. This is the tiny-body defect described below.
- **Degenerate edges.** `degenerate_edges` uses the task's definition: an edge counts if its start
  vertex is its end vertex, OR it is shorter than `pointTolerance`, OR it has no geometry. Under this
  rule, **full closed circles count**: the cylinder gives 2 and box ∩ sphere gives 6. `notes` splits
  the count into closed, non-degenerate circles and truly degenerate edges (ASM `isDegenerate=true`,
  no geometry). Only the cone apex and the two torus-pinch extras have truly degenerate edges.
- **Seam edges.** `seam_edges` counts edges whose coedges include the same face twice.
- **Tolerances.** `max_edge_tolerance` and `max_vertex_tolerance` are the maximum of `.tolerance` over
  all edges and vertices. No entity was tolerant, and the API returns 0.0 for exact entities. For a
  sphere or torus with no edges or vertices the value is `null`.

### Inputs (from `cases.json`, mm)

| id | input |
|---|---|
| prim.box | box corner (0,0,0) size 50×30×10 |
| prim.box.tiny | box corner (0,0,0) size 0.01×0.02×0.03 |
| prim.box.huge | box corner (0,0,0) size 100000×50000×20000 |
| prim.cylinder | base (0,0,0) axis +Z r=10 h=20 |
| prim.cylinder.tilted | base (1,2,3) axis (1,1,1) r=4 h=15 |
| prim.cone | base (0,0,0) axis +Z r0=10 r1=0 h=20 |
| prim.frustum | base (0,0,0) axis +Z r0=10 r1=5 h=20 |
| prim.sphere | centre (0,0,0) r=10 |
| prim.torus | centre (0,0,0) axis +Z major 20 minor 5 |
| bool.union.overlap | box 20³ at 0 ∪ box 20³ at (10,10,10) |
| bool.cut.hole | box 50×30×10 − cylinder r6 from (25,15,−5) h20 |
| bool.union.coincident_face | box 20³ at 0 ∪ box 20³ at (20,0,0) (full shared face) |
| bool.union.coincident_partial | box 20³ ∪ box 20×10×10 at (20,5,5) (partial shared face) |
| bool.cut.coplanar_through | box 20³ − box 10×10×20 at (5,5,0) (coplanar top/bottom) |
| bool.union.tangent_cylinder | box 20³ ∪ cylinder r10 at (30,10,0) h20 (line contact at x=20) |
| bool.union.near_tangent_cylinder | same, cylinder centred at x=29.99999, so it spans x 19.99999–39.99999 and **overlaps** the box by 1e-5 mm (= pointTolerance) |
| bool.intersect.box_sphere | box 20³ centred at 0 ∩ sphere r12 |
| bool.cut.disjoint | box 10³ − box 5³ at (20,20,20) |
| bool.union.repeated | box 40×10×10 ∪ five 4×4×4 boxes on top (z=10) |
| extra.box.zero_height | box 10×10×0 |
| extra.box.negative_height | box 10×10×−5 |
| extra.sphere.zero_radius | sphere r=0 |
| extra.torus.minor_gt_major | torus major 5, minor 20 |
| extra.torus.minor_eq_major | torus major 10, minor 10 (horn torus) |
| extra.size.cube_* | cube at the origin with edge 1e-6, 1e-5, 1e-4, 1e-3, 1e6, 1e7 mm |

### Results (catalogue cases)

| id | ok | F/E/V | face types | volume mm³ | area mm² |
|---|---|---|---|---|---|
| prim.box | yes | 6/12/8 | 6 plane | 15000 | 4600 |
| prim.box.tiny | yes | 6/12/8 | 6 plane | 6e-06 | 0.0022 |
| prim.box.huge | yes | 6/12/8 | 6 plane | 1e+14 | 1.6e+10 |
| prim.cylinder | yes | 3/2/2 | cylinder, 2 plane | 6283.185307 | 1884.955592 |
| prim.cylinder.tilted | yes | 3/2/2 | cylinder, 2 plane | 753.9822369 | 477.5220833 |
| prim.cone | yes | 2/2/2 | cone, plane | 2094.395102 | 1016.640738 |
| prim.frustum | yes | 3/2/2 | cone, 2 plane | 3665.191429 | 1364.182957 |
| prim.sphere | yes | 1/0/0 | sphere | 4188.790205 | 1256.637061 |
| prim.torus | yes | 1/0/0 | torus | 9869.604401 | 3947.84176 |
| bool.union.overlap | yes | 12/30/20 | 12 plane | 15000 | 4200 |
| bool.cut.hole | yes | 7/14/10 | cylinder, 6 plane | 13869.02664 | 4750.796447 |
| bool.union.coincident_face | yes | 6/12/8 | 6 plane | 16000 | 4000 |
| bool.union.coincident_partial | yes | 11/24/16 | 11 plane | 10000 | 3200 |
| bool.cut.coplanar_through | yes | 10/24/16 | 10 plane | 6000 | 3000 |
| bool.union.tangent_cylinder | yes | 10/17/10 | cylinder, 9 plane | 14283.18531 | 4284.955592 |
| bool.union.near_tangent_cylinder | yes | 10/17/10 | cylinder, 9 plane | 14283.18531 | 4284.955592 |
| bool.intersect.box_sphere | yes | 7/6/6 | 6 plane, sphere | 6383.716274 | 1734.159145 |
| bool.cut.disjoint | yes | 6/12/8 | 6 plane | 1000 | 600 |
| bool.union.repeated | yes | 31/72/48 | 31 plane | 4320 | 2120 |

All 19 cases have `solid: true`, 1 lump and 1 shell. The base-feature body had the same F/E/V as the
temporary body in every case. Centroid and inertia are `null` only for `prim.box.tiny`.

### Results (extras)

| id | outcome |
|---|---|
| extra.box.zero_height | accepted: an open **sheet** body, 1 plane face / 4 edges / 4 vertices, `isSolid=false`, volume 0, area 100 mm² |
| extra.box.negative_height | rejected by `OrientedBoundingBox3D.create`: `RuntimeError: 3 : invalid argument height` |
| extra.sphere.zero_radius | rejected by `createSphere`: `RuntimeError: 2 : InternalValidationError : result` |
| extra.torus.minor_gt_major | accepted: solid, 1 torus face (2 loops) / 2 degenerate edges / 2 vertices, volume 56374.66 mm³, area 7158.38 mm² (outer "apple" surface; see behaviour table) |
| extra.torus.minor_eq_major | accepted: solid, 1 torus face (2 loops) / 2 degenerate edges / 1 vertex, volume 19739.21 mm³ = 2π²Rr² |
| extra.size.cube_1e-06mm | rejected by `createBox`: `RuntimeError: 3 : invalid argument box` (the OBB itself was created) |
| extra.size.cube_1e-05mm | accepted, 6/12/8, volume 1e-15 mm³; bounding box collapsed to a point; centroid and inertia invalid |
| extra.size.cube_0.0001mm, _0.001mm | accepted, 6/12/8, exact volume and area; centroid and inertia invalid |
| extra.size.cube_1e+06mm, _1e+07mm | accepted, 6/12/8, exact volume and area, valid centroid and inertia; no warning or error |

### Outputs

- `evidence/asm-v5/reference.json`: the normalised reference, which the Rust harness consumes.
- `evidence/asm-v5/raw/probe-log.jsonl`: the full raw records in cm, including every call's return
  value or exception and the per-face, per-edge and per-vertex details (scrubbed).
- `evidence/asm-v5/step/<id>.step` (19): STEP files written by the design `ExportManager` from the
  base-feature document. `TemporaryBRepManager.exportToFile` rejects `.step`.
- `evidence/asm-v5/sat/<id>.sat` and `evidence/asm-v5/smt/<id>.smt` (27 each, extras included):
  native ASM files from `TemporaryBRepManager.exportToFile`. `.smb` and `.sab` also worked, but those
  binary files were not copied into `research/`.

### Limitations

- The physical properties of temporary bodies are unusable through this API, so every mass value
  comes from a persisted copy of the body in a base feature. Topology was identical for the
  persisted copy, and volume agrees with `body.volume` on the temporary body to within about 1e-13
  relative.
- `CalculationAccuracy.VeryHigh` was used throughout. Fusion's docs describe `Low` as ±1%; no error
  bound is documented for `VeryHigh`. The analytic primitives match their closed forms to ≥12
  significant digits.
- The size sweep found a lower creation limit (between 1e-6 and 1e-5 mm) but no upper limit up to
  1e7 mm. The threshold at which the tiny-body mass-property defect starts lies somewhere between
  6e-6 mm³ (defective) and 754 mm³ (`prim.cylinder.tilted`, correct); it was not pinned down further.
- Optional probes suggested by the public-research worker were **not run**: `imprintOverlapBodies` on
  the coincident-face boxes, and `getParamAnomaly` on the sphere, cone and torus faces. They would need
  another Fusion session.
