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

---

## Experiment v6: feature-level reference results

**Date:** 2026-10-17 · **Status:** completed. All 26 cases built, every feature was `healthState = 0`
(`HealthyFeatureHealthState`) with an empty error/warning message, there were no step errors and no timeouts, and every
case produced exactly one solid body.

### Purpose

This experiment produces ASM reference results for the **feature-level** operations in
[`feature-cases.json`](feature-cases.json): extrude, revolve, fillet, chamfer and shell. They are
built the way a Fusion user builds them, as sketch + parametric features in a design. The Rust kernel
(`crates/cad-geom`, `sweep.rs` and later features) has to reproduce the topology conventions as well as
the numbers. So, on top of everything v5 records, v6 records each face's surface parameters and loops
(in coedge order, with the coedge sense), each edge's curve parameters, vertices, adjacent faces and
degeneracy, and each vertex's position.

The black-box rules are the same as v5. ASM was driven only through Fusion's public Python API. No
Fusion or ASM binary was disassembled, decompiled or inspected. The only on-disk facts used were the
shipped public API type stubs (`Api\Python\packages\adsk\defs\adsk\fusion.py` / `core.py`), which were
used for method signatures.

The case file holds **26** cases: 9 extrude, 8 revolve, 5 fillet, 3 chamfer and 1 shell.

### Environment

This is the same host and Fusion build as v5 (see the v5 environment table): Fusion `app.version` =
**2705.1.30** from the same webdeploy folder, embedded Python 3.14.0, `pointTolerance` = 1e-5 mm and
`vectorAngleTolerance` = 1e-10 rad (re-read at runtime). The account was the same signed-in Education
licence. Nothing was saved, and every document was closed with `close(False)`. The run took about
2.5 minutes, from the add-in heartbeat at 19:07:11 to the last step at 19:09:39 local time, after the
45 s settle delay.

### API signatures used

These were read at runtime with `inspect.signature`. The full docstrings are in the raw log under
`api_signatures`.

- `Sketches.add(planarEntity)`, `Sketch.modelToSketchSpace(Point3D) -> Point3D`, `Sketch.transform`
  (`Matrix3D.getAsCoordinateSystem() -> (origin, x, y, z)`)
- `ConstructionPlaneInput.setByAngle(linearEntity, angle: ValueInput, planarEntity)`; `setByOffset`
  was implemented but not needed (every case plane passes through the world origin)
- `SketchLines.addByTwoPoints(start: Base, end: Base)`, `SketchArcs.addByCenterStartSweep(center, start, sweepAngle: float)`,
  `SketchCircles.addByCenterRadius(center, radius)`, `SketchPoint.merge(point)`, `Profile.areaProperties(accuracy)`
- `ExtrudeFeatures.createInput(profile, FeatureOperations.NewBodyFeatureOperation)`,
  `ExtrudeFeatureInput.setOneSideExtent(DistanceExtentDefinition.create(ValueInput), ExtentDirections)`,
  `ExtrudeFeatureInput.setSymmetricExtent(distance, isFullLength)` (and `setTwoSidesExtent`, which was implemented but not needed)
- `RevolveFeatures.createInput(profile, axis, NewBodyFeatureOperation)`, `RevolveFeatureInput.setAngleExtent(isSymmetric, angle)`
  (the docstring says a negative angle reverses the direction and 360 deg gives a full revolve)
- `FilletFeatures.createInput()`, `FilletFeatureInput.edgeSetInputs.addConstantRadiusEdgeSet(entities, radius, isTangentChain)`
- `ChamferFeatures.createInput2()`, `ChamferFeatureInput.chamferEdgeSets.addEqualDistanceChamferEdgeSet(edges, distance, isTangentChain)`
- `ShellFeatures.createInput(inputEntities, isTangentChain=True)`, `ShellFeatureInput.insideThickness`
- `BRepEdge.evaluator` / `BRepFace.evaluator` (`getParameterAtPoint`, `getPointAtParameter`,
  `getParameterExtents`, `isParameterOnFace`, `getStrokes`, `getNormalAtPoint`), and
  `Component.findBRepUsingPoint(point, entityType, proximityTolerance, visibleEntitiesOnly)` as a cross-check
- `BRepBody.getPhysicalProperties(VeryHighCalculationAccuracy)`, `PhysicalProperties.getXYZMomentsOfInertia()`
- `ExportManager.createSTEPExportOptions(path, rootComponent)`, `TemporaryBRepManager.copy(body) -> BRepBody`,
  `TemporaryBRepManager.exportToFile([body], path)`

### Procedure (exact)

1. **Pre-check.** `Get-Process Fusion360` returned nothing, and
   `%APPDATA%\Autodesk\Autodesk Fusion 360\API\AddIns\` was empty (0 entries).
2. **Add-in source.** `docs/evidence/fusion-probe/addin-src/CAD360FeatureProbe/` contains
   `CAD360FeatureProbe.py` and `.manifest` (`runOnStartup: true`). It uses the v5 pattern:
   - The sentinel is `%LOCALAPPDATA%\Temp\CAD360FeatureProbe-out\RUN_FEATURE_PROBE`, deleted on start.
   - A worker thread fires one custom event per step, with a 45 s settle time, a 300 s timeout per
     step and 1 s between steps.
   - Every API call is wrapped.
   - Each step appends one JSON record to `probe-log.jsonl`.
3. **Install and run.** The install is the same as v5, with the new names. The add-in folder was
   copied to `%APPDATA%\Autodesk\Autodesk Fusion 360\API\AddIns\CAD360FeatureProbe`, the sentinel was
   written, and `Fusion360.exe` was started.
4. **Per case**, in file order. Each case runs in a new unsaved design with
   `designType = ParametricDesignType`. Internal units are cm, so every mm value is divided by 10.
   - **Sketch plane.** Let the case normal be z = x × y.
     - If z is parallel to a world axis and the origin lies on the base plane, the add-in uses
       `xYConstructionPlane` or `xZConstructionPlane`.
     - For `ext.tilted`, the plane contains the world X axis, so it uses
       `setByAngle(xConstructionAxis, atan2(−z_y, z_z), xYConstructionPlane)` = **+45°**.
     - It then checks that the construction-plane normal is parallel to z and that the plane contains
       the case origin. The residual was 0 in all cases.
     - All sketch points are case-plane points mapped to world coordinates and then through
       `modelToSketchSpace`. The largest |sketch z| was 0.
   - **Sweep sign.** sign = (sketch normal from `Sketch.transform`) · z. Each arc's sweep is passed as
     `sweep·sign`. The arc's world midpoint is then compared with the case midpoint, and the sweep is
     retried with the opposite sign if they differ.
     - Observed: on world XZ the sketch axes are x = +X, y = **−Z**, normal **+Y**, so sign = −1. On
       XY and on the tilted plane, sign = +1.
     - The sign rule was right first time for all 4 arcs (midpoint error 0).
   - **Connectivity.** Segments are chained by passing the previous segment's end `SketchPoint` as the
     next start. Loops are closed onto the first point; a closing arc is closed with
     `SketchPoint.merge`. The arc's end point is found by position, because Fusion stores sketch arcs
     counter-clockwise and may swap start and end.
   - **Profile.** The profile is the one whose `areaProperties(VeryHigh).area` matches the expected
     region area, outer minus holes, computed with Green's theorem over the case segments. The
     relative error was ≤ 2e-16 in every case. `ext.rect_hole` and `ext.tilted` had 2 profiles: the
     holed region and the disc.
   - **Extrude.** The operation is `NewBodyFeatureOperation`.
     - If one offset is 0, it uses `setOneSideExtent(DistanceExtentDefinition(|to−from|), dir)`. The
       direction is `Positive` when (case direction · sign) > 0, and `Negative` otherwise.
     - If from = −to, it uses `setSymmetricExtent(half, isFullLength=False)`.
     - Verification projects the body onto z. All edge strokes and vertices are projected, and the
       span must equal [min, max] within 1e-5 mm. The add-in also checks that the prism centroid lies
       at the mid offset. If the span were wrong, the add-in would retry with the direction flipped.
       **It never had to flip.**
     - `PositiveExtentDirection` = along the sketch normal. `ext.rect.negative` used `NegativeExtentDirection`.
   - **Revolve.**
     - The axis is `root.zConstructionAxis`, with direction +Z = the case's `axis.dir`.
     - The angle is set with `setAngleExtent(False, ValueInput.createByReal(angle_rad))`.
     - For partial angles the add-in checks that the body centroid lies on the side `axis.dir × r̂`,
       where r̂ points from the axis to the profile interior. This is the right-hand rule: +X about +Z
       goes towards +Y. If the centroid were on the wrong side, the add-in would retry with −angle.
     - **The positive angle was right first time**, even though the sketch normal (+Y) is opposite to
       the case normal (−Y). For `rev.rect.quarter` the centroid was (9.903, 9.903, 5), and for
       `rev.circle.half` it was (0, 12.931, 0).
   - **Fillet, chamfer and shell.** The add-in first builds the `base` case body in the same document
     (with the same verification). It then picks each case point's edge or face by iterating all edges
     or faces and accepting an entity when:
     - `getParameterAtPoint` → `getPointAtParameter` lies within 1e-4 mm of the point, and
     - the parameter is inside the edge extents, or `isParameterOnFace` holds for a face.

     Every pick had **exactly one** hit at distance 0. `findBRepUsingPoint(..., 1e-5 cm, visibleEntitiesOnly=False)`
     also returned 1 in every case. The features were then created as follows:
     - **Fillet:** `addConstantRadiusEdgeSet(edges, r, isTangentChain=False)`. `isRollingBallCorner`
       was left at its default (`True`, logged), and the edge-set `continuity` default was 1 = `TangentSurfaceContinuityType` (logged).
     - **Chamfer:** `addEqualDistanceChamferEdgeSet(edges, d, isTangentChain=False)`. `cornerType` was
       left at its default (0 = `ChamferCornerType`).
     - **Shell:** `ShellFeatures.createInput([face], isTangentChain=False)`, then `insideThickness = t`.
       `shellType` was left at its default (0 = `SharpOffsetShellType`).
   - **Measure.** The add-in measured every body in `rootComponent.bRepBodies`; there was always
     exactly 1. It records:
     - the v5 topology;
     - for each face: its surface and parameters (from `face.geometry`, cast to the concrete type),
       `isParamReversed`, area, `pointOnFace` and the face normal there, and its loops (`isOuter`, plus
       the coedges in `BRepLoop.coEdges` order with `isOpposedToEdge`);
     - for each edge: its curve and parameters, `isDegenerate`, start and end vertex indices, length,
       midpoint, the faces of its coedges, and its tolerance;
     - for each vertex: its position, tolerance and edges.

     It then calls `getPhysicalProperties(VeryHigh)` on the persisted feature body and on the root component.
   - **Export.** STEP comes from the design's `ExportManager`. `.smt` and `.sat` come from
     `TemporaryBRepManager.exportToFile` on `TemporaryBRepManager.copy(body)`.
   - **Close** the document with `close(False)`.
5. **Close Fusion.** After `DONE`, `CloseMainWindow()` was called and the process exited normally
   with no save prompt.
6. **Uninstall.** The `...\AddIns\CAD360FeatureProbe` folder was removed, and the AddIns folder was
   back to 0 entries.
7. **Post-process.** An offline script, run with Fusion's `python.exe -I` from a temp directory
   outside `research/`, does the following:
   - Scrubs the log and exports. User-profile paths in `\`, `\\` and `/` forms (including the 8.3
     short profile name) become `%APPDATA%`, `%LOCALAPPDATA%` or `<userprofile>`, and the Windows user
     name becomes `<user>`.
   - Normalises the results to mm in `evidence/asm-v6/reference.json`.
   - Copies the scrubbed raw log and the STEP, SMT and SAT files.

   A final grep of `research/` for the user name, its 8.3 short form and absolute user-profile paths
   returned zero hits. The unscrubbed staging folder `%LOCALAPPDATA%\Temp\CAD360FeatureProbe-out`
   was deleted afterwards.

### Normalisation rules (`reference.json`)

These are the same as v5, with these differences:

- **Mass path.** `mass_path = "feature_body"`: volume, area, centroid and inertia come from the
  persisted feature body's `getPhysicalProperties(VeryHighCalculationAccuracy)`.
  - The density used to normalise to unit density is taken per case as mass/volume. It was 0.00785
    kg/cm³ (the default material) in every case.
  - The inertia is normalised exactly as in v5: the full tensor about the centroid, products in
    tensor form (I_xy = −∫(x−cx)(y−cy) dV), mm⁵.
  - Where v6 builds the same solid as a v5 case (`ext.rect`/`prim.box`, `ext.circle`/`prim.cylinder`,
    `ext.rect_hole`/`bool.cut.hole`, `rev.triangle.on_axis`/`prim.cone`, `rev.semicircle`/`prim.sphere`,
    `rev.circle.full`/`prim.torus`), the diagonal inertia agrees with v5 to 12 digits.
- **Degenerate edges.** `degenerate_edges` uses the v5 definition: start vertex = end vertex, OR length
  < pointTolerance, OR no geometry. `notes` splits the count into closed circles and truly degenerate
  edges.
- **Seam edges.** `seam_edges` counts edges whose coedges include the same face twice. It was 0 in
  every case.
- **New per-case keys.**
  - `kind`
  - `faces_detail`, `edges_detail` and `vertices_detail`. Indices are positions in the body's
    `faces`, `edges` and `vertices` enumeration order, which is the order the probe reads them in. All
    lengths are in mm, and directions are unit vectors.
  - `feature_api`: the sketch plane and construction method, sketch transform, sign, arc sweeps,
    profile areas, the extent API string and every try, the edge/face picks, and the defaults read for
    the fillet, chamfer and shell inputs.
  - `feature_health`: `healthState`, `errorOrWarningMessage`, the number of feature bodies and faces created.
  - `closed_form_volume`, `closed_form_source` and `volume_rel_error`.

  In the detail records, keys ending in `_mm`/`_mm2`/`_mm3` were converted from the API's
  `_cm`/`_cm2`/`_cm3`. Angles are in radians. NURBS knots are left unscaled (no case produced NURBS).
- **Surface origins.** Surface "origin" points are whatever `face.geometry` returned. They are not
  canonical: for example, the cylinder origin is at the base for an extrusion but at mid-height for a
  revolve. Compare cylinders, cones and tori by axis line, radii and half-angle, not by origin.

### Results

Closed forms marked † come from the task. The others were derived for this table: Pappus' theorem for
solids of revolution, and the spandrel area r²(1−π/4) with its centroid r(10−3π)/(12−3π) from the corner
for fillets. `fillet.corner3` has no simple closed form.

| id | ok | F/E/V | loops | face types | volume mm³ | closed form mm³ | rel. error |
|---|---|---|---|---|---|---|---|
| ext.rect | yes | 6/12/8 | 6 | 6 plane | 15000 | 15000 † | 0 |
| ext.rect.negative | yes | 6/12/8 | 6 | 6 plane | 15000 | 15000 † | 0 |
| ext.rect.symmetric | yes | 6/12/8 | 6 | 6 plane | 15000 | 15000 † | 0 |
| ext.circle | yes | 3/2/2 | 4 | cylinder, 2 plane | 6283.185307 | 2000π † | 0 |
| ext.rect_hole | yes | 7/14/10 | 10 | cylinder, 6 plane | 13869.02664 | (1500−36π)·10 † | 3.4e-15 |
| ext.slot | yes | 6/12/8 | 6 | 2 cylinder, 4 plane | 1892.699082 | (300+25π)·5 † | 1.2e-16 |
| ext.lshape | yes | 8/18/12 | 8 | 8 plane | 4800 | 4800 † | 1.9e-16 |
| ext.notch | yes | 8/18/12 | 8 | cylinder, 7 plane | 3857.522204 | (800−50π)·6 † | 3.5e-16 |
| ext.tilted | yes | 7/14/10 | 10 | cylinder, 6 plane | 1202.079663 | (200−9π)·7 | 5.7e-16 |
| rev.rect.full | yes | 4/4/4 | 8 | 2 cylinder, 2 plane | 9424.777961 | π(400−100)·10 † | 1.4e-14 |
| rev.rect.quarter | yes | 6/12/8 | 6 | 2 cylinder, 4 plane | 2356.19449 | ¼ of the above † | 0 |
| rev.rect.on_axis | yes | 3/2/2 | 4 | cylinder, 2 plane | 6283.185307 | 2000π † | 1.4e-16 |
| rev.triangle.on_axis | yes | 2/2/2 | 3 | cone, plane | 2094.395102 | ⅓π·100·20 † | 2.2e-16 |
| rev.trapezoid | yes | 4/4/4 | 8 | cone, cylinder, 2 plane | 6544.984695 | 2π·3125/3 (Pappus) | 0 |
| rev.semicircle | yes | 1/0/0 | 0 | sphere | 4188.790205 | ⁴⁄₃π·1000 † | 0 |
| rev.circle.full | yes | 1/0/0 | 0 | torus | 9869.604401 | 2π²·20·25 † | 0 |
| rev.circle.half | yes | 3/2/2 | 4 | torus, 2 plane | 4934.802201 | π²·20·25 † | 5.5e-16 |
| fillet.edge | yes | 7/15/10 | 7 | cylinder, 6 plane | 14903.42917 | 15000 − 50·9(1−π/4) | 0 |
| fillet.vertical4 | yes | 10/24/16 | 10 | 4 cylinder, 6 plane | 14785.39816 | 15000 − 4·10·25(1−π/4) | 2.5e-16 |
| fillet.corner3 | yes | 10/21/13 | 10 | 3 cylinder, 6 plane, sphere | 14830.69243 | – | – |
| fillet.cyl_top | yes | 4/3/3 | 6 | cylinder, 2 plane, torus | 6231.659467 | Pappus on spandrel | 2.9e-16 |
| fillet.hole_edge | yes | 8/15/11 | 12 | cylinder, 6 plane, torus | 13860.63516 | Pappus on spandrel | 4.6e-15 |
| chamfer.edge | yes | 7/15/10 | 7 | 7 plane | 14900 | 15000 − 2·50 † | 0 |
| chamfer.vertical4 | yes | 10/24/16 | 10 | 10 plane | 14500 | 15000 − 4·12.5·10 | 0 |
| chamfer.cyl_top | yes | 4/3/3 | 6 | cone, cylinder, 2 plane | 6165.899181 | 2000π − 2π(10−⅔)·2 | 3.4e-15 |
| shell.top | yes | 11/24/16 | 12 | 11 plane | 5432 | 15000 − 46·26·8 † | 3.4e-16 |

Summary across all 26 cases:
- Every case has `solid: true`, 1 lump, 1 shell and 1 body.
- `seam_edges` is 0 everywhere, and no edge or vertex is tolerant (max tolerance 0.0, as in v5).
- Every centroid lies strictly inside its bounding box, so none was rejected.
- The only truly degenerate edge is the cone apex of `rev.triangle.on_axis`.
- Topology and volume match the analogous v5 `TemporaryBRepManager` solids exactly. The pairs are
  box/`ext.rect`, cylinder/`ext.circle` and `rev.rect.on_axis`, `bool.cut.hole`/`ext.rect_hole`,
  cone/`rev.triangle.on_axis`, sphere/`rev.semicircle` and torus/`rev.circle.full`.

The topology conventions are summarised in [`REFERENCE_BEHAVIOR.md`](REFERENCE_BEHAVIOR.md#feature-level-topology-conventions-v6).

### Outputs

- `evidence/asm-v6/reference.json`: the normalised reference (26 cases).
- `evidence/asm-v6/raw/probe-log.jsonl`: the full raw records in cm/rad (scrubbed), including the
  `api_signatures` docstrings, every try, the picks, and both the per-body and component physical properties.
- `evidence/asm-v6/step/<id>.step` (26), from the design `ExportManager`.
- `evidence/asm-v6/smt/<id>.smt` and `evidence/asm-v6/sat/<id>.sat` (26 each), from
  `TemporaryBRepManager.exportToFile` on a `copy()` of the feature body.

### Limitations

- Only the defaults were exercised for the fillet corner (`isRollingBallCorner = True`), the chamfer
  corner type (`ChamferCornerType`) and the shell type (`SharpOffsetShellType`). Setback corners,
  miter/blend chamfer corners and rounded shells were not run, and their topology may differ.
- `isTangentChain=False` was passed everywhere. No case has a tangent chain that it would have
  extended, so the flag's effect was not observed.
- No case has adjacent coplanar faces or adjacent collinear or co-circular sketch segments, so face
  and edge **merging** after a feature was not tested. The tangent line/arc joints in `ext.slot` are a
  different situation; see the behaviour doc.
- All cases use planes through the world origin and the world Z axis as the revolve axis. The
  offset-plane and arbitrary-axis paths in the add-in were not exercised.
- Loop coedge order is the `BRepLoop.coEdges` enumeration order.
  - **Checked offline:** in all 567 coedges of all 26 bodies, each coedge's end vertex equals the next
    coedge's start vertex (taking `isOpposedToEdge` into account).
  - Generalising this to other bodies is a hypothesis.
  - Entity indices are enumeration order and are not stable identifiers across kernels.
- `edges_detail[].curve` is the raw `edge.geometry`. Its parameterisation sense can be **opposite** to
  the edge's startVertex→endVertex direction.
  - This was observed for 7 of 255 line/arc edges, all on the inner offset faces of `shell.top`.
  - The edge direction must therefore be taken from `startVertex`/`endVertex`, not from the curve's
    `startPoint`/`endPoint`.
  - For closed circles the curve sense relative to the edge cannot be told from this data. The
    `Circle3D.normal` is the curve's normal, and the edge sense was not recorded separately.
- `CalculationAccuracy.VeryHigh` was used throughout. Every closed form matched to ≤ 1.5e-14 relative.
