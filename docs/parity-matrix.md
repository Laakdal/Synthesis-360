# Feature-parity matrix (CAD360 vs Fusion 2705.1.30)

Status: **✅ implemented & verified** (automated test and/or scenario screenshot) · **🟡 partial** ·
**⬜ not implemented** (present in the UI only as a disabled "Not yet supported in CAD360" entry) ·
**⛔ blocked/out of scope**. "Ref" = evidence row in `evidence/findings-ledger.md` (V verified in
Fusion, D documented). "Origin": **R** behaviour reproduced from the Fusion evidence, **I** designed
independently (no Fusion evidence available or a deliberate difference).

## Interface
| Feature | Status | Ref | Origin | Verification / notes |
|---|---|---|---|---|
| App bar (file menu, save, undo/redo, document tab) | ✅ | F1 | R | UI tests; screenshots 01–14 |
| Ribbon: DESIGN ▾, SOLID tab, panels CREATE…SELECT, drop-downs | ✅ | F1 | R | generated from the command registry; UI tests |
| SURFACE, MESH, UTILITIES tabs (Fusion panel layout from the v3 ribbon dump) | 🟡 | F1 | R | implemented subset below; ~35 other commands of these tabs registered but disabled |
| SHEET METAL, PLASTIC, MANAGE tabs, other workspaces | ⬜ | F1 | — | visible, disabled ("Not yet supported in CAD360") |
| Contextual SKETCH tab + FINISH SKETCH | ✅ | F9 | R | screenshot 03/04 |
| Browser tree (Document Settings, Named Views, Origin, Bodies, Sketches, Construction) | ✅ | F2 | R | UI tests |
| Timeline (icons, marker drag, playback, health colours, context menu, reorder) | ✅ | F3, F30 | R | UI tests; screenshot 12 |
| ViewCube (faces, edges, corners, home, projection) | ✅ | F5 | R/I | UI tests (edge behaviour I — U2) |
| Navigation bar (Orbit, Look At, Pan, Zoom, Fit, Display, Grid) | ✅ | F4 | R | UI tests; Viewports ⬜ |
| Mouse navigation (MMB pan, Shift+MMB orbit, wheel zoom at cursor, F6) | ✅ | F34 | R | camera unit tests |
| Z-up, orthographic default, standard views | ✅ | F6, F7 | R | camera tests vs probe vectors |
| Marking menu (right-click) | 🟡 | F33/D | R | radial menu + list; no gesture flicks |
| Keyboard shortcuts (E, L, R, C, D, F, H, X, I, Ctrl+Z/Y/S/O/N, F6, Del, Esc) | ✅ | F33 | R | registry-driven |
| Command dialogs at the right of the canvas, OK/Cancel, live preview | ✅ | F23 | R | features tests |
| Status/messages, tooltips | ✅ | — | I | |
| Selection: hover pre-highlight, click, Shift/Ctrl add | ✅ | D 17 | R | picking tests |
| Window/crossing selection, selection filters | ⬜ | D 17 | — | |
| On-canvas manipulators (drag arrows with live value box) | ✅ | F23 | R | Extrude, Offset Plane, Fillet/Chamfer (Shell unverified); snapped, Shift = fine; manipulator tests; `05b_extrude_manipulator.png` |

## Sketching
| Feature | Status | Ref | Origin | Verification / notes |
|---|---|---|---|---|
| Create Sketch on origin plane / planar face / construction plane, plane prompt | ✅ | F8, F11, F12 | R | workflow test; screenshot 02 |
| Face sketch projects the face boundary (associative) | ✅ | F29 | R | kernel test `face_sketch_projects_face_boundary_like_fusion` |
| Line (chains, H/V inference), 2-Point & Center Rectangle (H/V), Circle, 3-Point Arc, Point | ✅ | F13, F15, F16 | R | sketch tests |
| Snapping to points/origin (coincident), curves, grid | ✅ | F16 | R | sketch tests |
| Trim, Extend, Sketch Fillet, Offset, Mirror, Polygon, Slot, 2-Point Circle, Center Arc, 3-Point Rectangle | ✅ | D | R | sketch tests (fillet between lines and/or arcs, radius capped to fit; Mirror needs Enter between picking objects and the line) |
| Spline, Project, Text, Ellipse | ⬜ | D | — | |
| Sketch Dimension (type from selection, expressions, edit, driven) | ✅ | F18, F20–F22 | R | sketch tests |
| 12 geometric constraints (Curvature excepted: 11) | ✅ | D 8 | R | sketch tests; Curvature ⬜ (no splines) |
| Over-constraint detection + driven alternative | ✅ | F18 | R | solver + sketch tests |
| Constraint state colours, DOF status, browser lock | ✅ | F17, F19 | R | screenshot 04 (white instead of black on dark canvas) |
| Drag geometry with live solve | ✅ | — | I | sketch tests |
| Normal/Construction linetype | ✅ | D 16b | R | |
| Sketch Palette options | ✅ | F10 | R | screenshot 03/04 (Slice, 3D Sketch ⬜) |
| Profiles: closed regions, nested holes, overlaps, open chains | ✅ | F29 | R | profile tests |

## Solid modelling
| Feature | Status | Ref | Origin | Verification / notes |
|---|---|---|---|---|
| Extrude: distance/All, one side/flip/symmetric, Join/Cut/Intersect/New Body | ✅ | F23–F26 | R | kernel + features tests; volumes equal to Fusion's |
| Extrude: Two Sides, To Object, Taper, Thin, Start offset | ⬜ | D 12 | — | |
| Revolve (sketch line or origin axis, angle) | ✅ | D 13a | R | kernel test |
| Fillet (constant radius, edge sets) | ✅ | F25, F28 | R | equals Fusion 35570.8 mm³; oversize → error (Fusion produced odd geometry) |
| Chamfer (equal distance) | ✅ | D 13c | R | kernel test |
| Hole (simple, depth / through all) | ✅ | D 13d | R | counterbore/countersink/threads ⬜ |
| Shell | ✅ | D | R | kernel test (exact volume) |
| Combine (join/cut/intersect, keep tools) | ✅ | D | R | kernel test |
| Mirror (bodies), Rectangular/Circular Pattern (bodies), Move/Copy | ✅ | D | R/I | kernel tests; feature patterns ⬜ |
| Offset construction plane | ✅ | D | R | kernel test; other plane/axis/point types ⬜ |
| Sweep, Loft, Rib, Web, Draft, Press Pull, Thread, Split, Scale, primitives | ⬜ | D | — | |
| Appearance (body colours) | ✅ | D 16b | I | features test; physical materials ⬜ |
| INSPECT: Measure (length/area/volume, minimum distance, angles), Interference, Center of Mass | ✅ | D | I | `tests/inspect.rs` (6), `features_inspect_tests.rs` (5); on-canvas markers; results dropped when the model changes; Curvature/Zebra/Draft analysis ⬜ |
| Section analysis (display-only, capped) | ✅ | D | I | per-body GPU stencil caps with hatching, live while dragging the offset; `13_section_analysis.png`; arbitrary (non-origin) planes ⬜ |
| Surface Extrude / Revolve, Patch (planar + G0 filling), Stitch (→ solid when closed), Offset surface, Thicken | ✅ | D | R/I | `tests/surface.rs` (12), surface dialog tests; Ruled, Trim/Untrim, Extend, Unstitch, Reverse Normal, Boundary Fill ⬜ |
| Insert Mesh (STL, embedded) + Convert to B-rep; UTILITIES ▸ 3D Print (STL with refinement) | ✅ | D | R/I | surface tests; mesh repair/remesh/reduce tools ⬜ |
| Solid booleans ignore surface/mesh bodies; STEP export skips mesh bodies (warning) | ✅ | — | I | regression test `solid_join_next_to_a_surface_body_ignores_the_surface` |
| Sheet metal, plastic, T-spline forms | ⛔ | — | — | out of scope |

## History, parameters, documents
| Feature | Status | Ref | Origin | Verification / notes |
|---|---|---|---|---|
| Parametric timeline with real recompute (incremental cache) | ✅ | F25, F30 | R | kernel tests |
| Edit feature (double-click) reopening its dialog | ✅ | D 5a | R | features tests |
| Edit an earlier sketch → downstream recompute | ✅ | F25 | R | workflow test |
| Persistent face/edge references across edits | 🟡 | F25 | I | generative (OCCT-history) face names + edge = face-name pair, geometry only to pick among same-name splits, warning on geometric fallback; `tests/naming.rs` covers bbox-changing upstream cuts, inserted upstream bosses, large width changes, mirrored copies. Still ambiguous: face-sketch projected edges, corner blends, multi-profile caps, split faces, imported/surface/mesh faces (see kernel-evaluation.md) |
| Roll back marker, suppress, delete, rename, reorder (dependency-checked) | ✅ | F30, D 5 | R | app tests |
| Health states and messages (warning/error) | ✅ | F24, F26, F27, F31 | R | kernel tests (lost references → error instead of cached geometry) |
| Model parameters d1…, user parameters, unit-aware expressions | ✅ | F20, F21 | R | expr/document tests; Change Parameters dialog |
| Undo/redo (snapshot), one step per command | ✅ | F30 | R | history + app tests |
| Native document save/open with full history (`.c360`) | ✅ | — | I | workflow test; screenshot 14 |
| STEP export / import (exact solids; import is non-parametric) | ✅ | F32 | R | workflow + kernel tests |
| STL export | ✅ | F32 | R | binary layout verified |
| IGES, 3MF, OBJ, F3D import/export | ⬜ | D 15 | — | F3D is proprietary ⛔ |
| Components (create, activate, hide, rename, delete; bodies follow their feature's component) | ✅ | D | R/I | assembly tests; one level deep |
| Joints: Rigid / Revolute / Slider between planar faces or circular edges, parametric angle/offset, graph solve (root / grounded / earlier-created component fixed), over-constraint refused, Ground | 🟡 | F35–F37/D | R/I | assembly tests incl. the probe numbers (rigid B bbox [[5,12.5,10],[35,27.5,20]], revolute 30° [[3.26,6.005,10],[36.74,33.995,20]] + origin translation); joint Z axes aligned, Flip reverses (F35); mover not decided by pick order (F36, rule beyond "earlier-created stays" I); second joint on a connected pair gets Fusion's over-constraint error (F37). Flip+angle, slider direction, origin-axis Move/patterns in moved components I; no motion limits, as-built joints, nesting ⬜ |
| Cloud data panel, collaboration, comments, assistant | ⛔ | F1 | — | local-first by design |
| Drawings, CAM, simulation, rendering, animation | ⛔ | — | — | out of scope |
