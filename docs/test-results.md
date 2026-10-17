# Test results

Run: 2026-10-17 (after adding extrude, revolve, chamfer/fillet and shell to the own kernel `cad-geom` and the feature cases to `cad-diff`; the release build `C:\c360t\release\cad360.exe`
also completed the workflow scenario), Windows 11 Pro 22631, rustc 1.95.0 (MSVC), GPU NVIDIA GeForce
MX450 (Vulkan) over RDP. Command: `cargo test --workspace --no-fail-fast -- --test-threads=4` —
**469 passed, 0 failed, 0 ignored** (raw log: `docs/test-run-raw.txt`). Scripted UI scenarios (real window, real
GPU) were run with `cad360 --scenario docs/scenarios/{workflow,modify,assembly,viewport}.json
--screenshot-dir docs/screenshots/cad360` and completed without errors.

| Suite | Tests | Result |
|---|---|---|
| `cad-app` (state, sketch incl. advanced tools, features, surface/mesh, inspect, assembly, UI chrome, manipulators, picking, camera, workflow, branding) | 147 | ✅ |
| `cad-core` unit (expressions, parameters, document, history, ops) | 15 | ✅ |
| `cad-core/tests/layering.rs` (crate layering is enforced; `cad-geom` independent of OCCT) | 3 | ✅ |
| `cad-kernel` unit (frames, joint maths) | 6 | ✅ |
| `cad-kernel/tests/fusion_workflow.rs` (geometry vs Fusion's numbers) | 5 | ✅ |
| `cad-kernel/tests/naming.rs` (history-based persistent names) | 8 | ✅ |
| `cad-kernel/tests/assembly.rs` (graph-solved joints, Fusion v4 probe placements) | 13 | ✅ |
| `cad-kernel/tests/more_features.rs` (shell, combine, mirror, patterns, move) | 10 | ✅ |
| `cad-kernel/tests/surface.rs` (surface and mesh features) | 12 | ✅ |
| `cad-kernel/tests/inspect.rs` (minimum distance, angles, interference, centre of mass) | 6 | ✅ |
| `cad-sketch` unit (solver, profiles, interaction, shapes, modify incl. fillet corner/arc cases) | 44 | ✅ |
| `cad-geom` unit (NURBS basis, projection, intersection helpers, predicates, boolean and triangulation internals, profile checks) | 19 | ✅ |
| `cad-geom/tests/geometry.rs` (analytic curves/surfaces, projection, intersections incl. tangent and near-tangent) | 20 | ✅ |
| `cad-geom/tests/nurbs.rs` (basis, derivatives, knot insertion, split, exact conics, analytic → NURBS) | 14 | ✅ |
| `cad-geom/tests/primitives.rs` (ASM-convention topology, both conventions) | 7 | ✅ |
| `cad-geom/tests/mass.rs` (exact volume/area/centroid/inertia vs closed forms, seamless and seamed) | 16 | ✅ |
| `cad-geom/tests/validate.rs` (clean primitives; broken models raise the expected issue; flagged non-manifold edges) | 13 | ✅ |
| `cad-geom/tests/asm_reference.rs` (all 9 primitives vs the recorded ASM reference) | 1 | ✅ |
| `cad-geom/tests/booleans.rs` (planar booleans: contact, containment, empty result, near-coincidence at the 1e-5 mm ASM tolerance, scale, rotated boxes, sliver regressions) | 13 | ✅ |
| `cad-geom/tests/curved_booleans.rs` (plane/cylinder/sphere booleans vs circle-segment, spherical-cap and octant formulas; tangency at 5e-6/1e-5/2e-5 mm; repeated holes) | 12 | ✅ |
| `cad-geom/tests/tess.rs` (planar faces with holes and trimmed curved faces: watertight, outward, exact planar volume, chord bound, convergence) | 16 | ✅ |
| `cad-geom/tests/asm_booleans.rs` (all 10 recorded boolean cases vs the ASM reference) | 4 | ✅ |
| `cad-geom/tests/extrude.rs` (closed forms, = box/cylinder primitives, = ASM `bool.cut.hole`, all 9 ASM v6 extrude records, watertight mesh, orientation, scale) | 8 | ✅ |
| `cad-geom/tests/revolve.rs` (Pappus closed forms incl. inertia, = primitives, ASM v6 records, partial sweeps, holes/pockets, mirror/tilt/scale, mesh, invalid input) | 14 | ✅ |
| `cad-geom/tests/blend.rs` (chamfer/fillet closed forms, = rounded extrudes, vertex blend, rim torus/cone, ASM v6 records, edge picking, invalid input, tess regression) | 16 | ✅ |
| `cad-geom/tests/shell.rs` (shell.top vs OCCT/ASM, L shape, two openings, closed hollow, pyramid apex, rotation, thickness limits) | 11 | ✅ |
| `cad-diff/tests/diff.rs` (OCCT closed forms incl. 24 feature cases, comparator classification, ASM loader, cad-geom vs OCCT, all 10 booleans MATCH ASM, feature catalogue, extrude vs OCCT) | 15 | ✅ |
| vendored `opencascade` triangulation test | 1 | ✅ |

**Not tested with REA**: REA's native-UI observation is macOS-only on this host, so UI behaviour was
verified with headless synthetic-input tests (egui, AccessKit-free) and scripted scenarios with
window screenshots.

## Phase 7 functional workflow
Primary evidence: `state::workflow_tests::phase7_workflow_through_tools_and_dialogs`, which drives
only the interactive paths — `execute(Rectangle2Pt)` + pointer events (Move/Press/Release/Click as
`app.rs` emits them), the Sketch Dimension tool and its value box, Finish Sketch, profile selection +
the Extrude dialog (`dialog_set_primary`, `dialog_ok`), timeline edit (`edit_feature`) with a rejected
`15 deg`, the sketch dimension value editor, Undo/Redo commands, save/open and STEP export/re-import —
with a volume assertion at each step. A second test, `phase7_workflow_headless`, checks the same
workflow at the document/model level (geometry built with `cad_core::ops`), and the real window was
driven through the same steps by `docs/scenarios/workflow.json` (screenshots; the scenario itself
does not assert values). 

| # | Scenario | Expected | Actual | Result | Evidence |
|---|---|---|---|---|---|
| 1 | Create a new document (`NewDesign`) | empty design, no features | empty design | ✅ | UI-path test; `01_new_design.png` |
| 2 | Select a sketch plane | Create Sketch without selection prompts for a plane; selecting XY enters sketch mode | prompt shown, sketch mode entered | ✅ | test; `02_…`, `03_sketch_mode.png` |
| 3 | Draw a rectangle | 4 lines + 2 Horizontal + 2 Vertical (Fusion F13) | 4 lines, 4 H/V | ✅ | test; sketch tool tests |
| 4 | Dimensions + constraints | origin coincident + 50/30 mm dims → fully constrained | DOF 0, fully constrained, browser lock | ✅ | test; `04_…png` |
| 5 | Finish the sketch | one undo step | one "Create Sketch" step | ✅ | test |
| 6 | Extrude the profile | 50×30×10 solid, 15000 mm³ | 15000 mm³ | ✅ | test; `05`, `06` |
| 7 | Modify the extrusion distance | 20 mm → 30000 mm³ | 30000 mm³ | ✅ | test |
| 8 | Verify the solid changes / invalid value | "15 deg" rejected with an error, solid unchanged | rejected, unchanged | ✅ | test |
| 9 | Edit the original sketch | downstream rolled back while editing | Extrude1 RolledBack during edit | ✅ | test |
| 10 | Dependent geometry recomputes | width 60 → 36000 mm³ | 36000 mm³ | ✅ | test; `07_…png` |
| 11 | Undo / redo | undo → 30000, redo → 36000 | as expected | ✅ | test |
| 12 | Save | `.c360` written | written | ✅ | test; scenario |
| 13 | Reopen | document loads | loads | ✅ | test; `14_reopened.png` |
| 14 | Geometry, parameters, history intact | 2 features, identical parameter table, 36000 mm³, sketch fully constrained | identical | ✅ | test |
| 15 | Export and verify | STEP has `MANIFOLD_SOLID_BREP`; binary STL size = 84 + 50·n; re-imported STEP volume equal | all true | ✅ | test |

## Geometry tests
| Scenario | Expected | Actual | Result | Test |
|---|---|---|---|---|
| Closed-profile validation | rectangle → 1 profile; circle in rectangle → ring + disc; open chain → 0; overlaps → 3 regions | as expected | ✅ | `profiles::tests::*` |
| Face-sketch boundary projection | 2 profiles (ring 1986.9 + disc 113.1 mm²), follows width edits | as expected | ✅ | `face_sketch_projects_face_boundary_like_fusion` |
| Invalid sketch handling | conflicting dimension values → solve fails, geometry unchanged; over-constraint rejected, driven allowed | as expected | ✅ | `solver::tests::*`, sketch tests |
| Extrusion direction/dimensions | bbox [0,0,0]–[50,30,10], 6 faces, 12 edges; negative width flips like Fusion | as expected | ✅ | `sketch_extrude_edit_fillet_cut_like_fusion`, solver tests |
| Booleans | through-all Ø12 cut → 39308.8 mm³ (Fusion value); join/cut/intersect/combine | as expected | ✅ | kernel tests |
| Fillet / chamfer | 4×R5 → 35570.8 mm³ (Fusion), survives width edit (41570.8, Fusion); oversize radius → error, body unchanged | as expected | ✅ | kernel + features tests |
| Shell / patterns / mirror / move | exact volumes (shell 8472 mm³), instance counts and positions | as expected | ✅ | `more_features.rs` |
| Feature recomputation & cache | editing a late feature recomputes only it; constraint-only edits are not served stale | as expected | ✅ | `recomputed_from` assertion; cache regression test |
| Dependency handling | deleted sketch → dependent cut errors; reorder respects dependencies; suppression | as expected | ✅ | kernel + app tests |
| Kernel errors | zero distance, impossible fillet/shell → feature Error health, no crash | as expected | ✅ | `invalid_operations_report_errors_instead_of_crashing` |
| Invalid parameters | unknown names, unit mismatch, cycles, division by zero → rejected | as expected | ✅ | `expr` / `document` tests |
| Displayed vs stored dimensions | dimension labels show the parameter value; driven values measured | as expected | ✅ | sketch overlay tests |
| Assemblies | rigid mate, revolute 90°/45°, slider 15 mm, parametric angle edit, errors, round trip | as expected | ✅ | `assembly.rs` |

## UI tests
| Scenario | Result | Evidence |
|---|---|---|
| Toolbar/ribbon actions, tab switching, disabled unsupported tools | ✅ | `ui::ribbon` tests; screenshots |
| Sketch and modelling modes (contextual SKETCH tab, Finish Sketch) | ✅ | ribbon/sketch tests; `03`, `04` |
| Selection and highlighting (faces, edges, profiles, sketch entities) | ✅ | picking + sketch tests; `08`, `20` |
| Camera navigation (orbit/pan/zoom-at-cursor, standard views, ViewCube) | ✅ | `camera` tests, `ui::viewcube` tests |
| Property editing (dialogs with live preview, expressions, errors) | ✅ | features tests; `05`, `08`, `09`, `20`, `21`, `31` |
| Timeline interactions (marker drag, playback, rename, suppress, reorder, edit) | ✅ | `ui::timeline` tests; `12` |
| Error messages (health in dialogs and timeline, message log) | ✅ | features/statusbar tests |
| Save/load workflows | ✅ | workflow test; scenario |

## Known limitations of the testing
* UI tests use synthetic egui input in a headless context; GPU rendering is validated by the
  scenario screenshots, not by automated image comparison.
* Fusion comparisons rely on the probe's measured numbers for one workflow; other Fusion behaviours
  are compared qualitatively (screenshots, documentation).
* Parallel test runs were stabilised after fixing a process-global OCCT messenger race and
  serialising OCCT data-exchange calls; earlier parallel runs had crashed (`STATUS_HEAP_CORRUPTION`).
