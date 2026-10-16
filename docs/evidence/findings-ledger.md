# Findings ledger

Confidence: **V** = verified in the running application (probe), **D** = official documentation,
**I** = inference, **U** = could not be established. Probe logs live in `docs/evidence/fusion-probe/`
(`out-v1/probe-log.jsonl`, `out-v2-v3/probe-log-v2.jsonl`, `out-v2-v3/probe-log-v3.jsonl`). Screenshots
are redacted (title bar / account name).

| # | Finding | Evidence | Conf. | Implementation implication |
|---|---|---|---|---|
| F1 | Design workspace layout: application bar (home, data panel, file, save, undo/redo), document tab, ribbon with workspace switcher "DESIGN ▾" and tabs SOLID · SURFACE · MESH · SHEET METAL · PLASTIC · UTILITIES · MANAGE; SOLID panels CREATE · MODIFY · ASSEMBLE · CONFIGURE · CONSTRUCT · INSPECT · INSERT · SELECT | `out-v2-v3/screen-v2_06_extrude_dialog.png`, `window-*.png` | V | Same top-level structure and panel order |
| F2 | Browser (top-left, floating): root "(Unsaved)" → Document Settings (Units: mm, g; Hybrid Design), Named Views, Origin (hidden), Bodies, Sketches; eye icons | `screen-v2_08_fillet_error_state.png` | V | Model browser with the same nodes and visibility toggles |
| F3 | Timeline at the bottom with playback buttons (⏮ ◀ ▶ ▶ ⏭), feature icons, marker, settings gear | `window-v2_*.png` | V | Timeline widget with marker and icons |
| F4 | Navigation bar (bottom-centre): Orbit, Look At, Pan, Zoom, Fit, Display Settings, Grid and Snaps, Viewports | `ui-v2-sketch-mode.json` (NavToolbar) | V | Same bar with working Orbit/Pan/Zoom/Fit modes |
| F5 | ViewCube top-right with TOP/FRONT/RIGHT faces and XYZ triad; face → orthographic view, corner → iso | screenshots; official docs 2a | V+D | ViewCube widget |
| F6 | Default modeling orientation Z up; standard views: top eye +Z, front eye −Y (up +Z), right eye +X | `probe-log.jsonl` (v1) `camera` rows; docs 18 | V+D | Z-up world, same standard views |
| F7 | Default camera type orthographic (CameraTypes 0) | v1 `camera` rows `"type":0` | V | Orthographic default, perspective toggle |
| F8 | Create Sketch without a selection shows a plane-selection prompt; with a preselected plane it enters sketch mode directly | `window-02_create_sketch_select_plane.png`, `screen-v2_02…` | V | Both flows |
| F9 | Sketch mode: contextual SKETCH tab (CREATE, MODIFY, CONSTRAINTS panels), green ✓ FINISH SKETCH, camera looks at the plane (TOP), mm grid labels | `window-03_sketch_mode.png` | V | Sketch environment |
| F10 | Sketch Palette: Options — Linetype, Look At, Sketch Grid, Snap, Slice, Profile, Points, Dimensions, Constraints, Projected Geometries, Construction Geometries, 3D Sketch; "Finish Sketch" button | `screen-v2_04_ui_line_drawn.png`; docs 6c | V+D | Sketch palette panel |
| F11 | Origin-plane sketch frames: XY (x+X, y+Y, n+Z), XZ (x+X, y−Z, n+Y), YZ (x−Z, y+Y, n+X) | v3 `v3_origin_plane_sketch_frames` | V | `cad_kernel::Frame::origin_plane` (unit-tested) |
| F12 | Face sketch origin = world origin projected on the face, x+X, y+Y for a +Z face | v2 `v2_face_sketch_transform` | V | `Frame::on_plane` (unit-tested) |
| F13 | UI 2-Point Rectangle adds 2 Horizontal + 2 Vertical constraints | v3 `v3_ui_rectangle_sketch` | V | `Sketch::add_rectangle` adds H/V |
| F14 | API `addTwoPointRectangle` adds **no** constraints; dimension edits then skew it (V=33000 instead of 36000) | v1 `sketch_after_rectangle`, snapshots | V | Rectangle tool must add H/V (F13) |
| F15 | Line tool infers Horizontal when drawn ~1° off horizontal (endpoint snapped to y=40) | v2 `ui_line_sketch` | V | Auto-horizontal/vertical inference within a few degrees |
| F16 | Circle whose radius point lands on the origin gets an automatic Coincident | v3 `v3_ui_circle_sketch` | V | Snap to existing points adds coincident |
| F17 | H/V + origin coincident + 2 dimensions ⇒ `isFullyConstrained = True`; one dimension ⇒ False | v2 `v2_sketch_fully_dimensioned`, `v2_sketch_one_dim` | V | DOF analysis (cad-sketch tests) |
| F18 | Over-constraining dimension/constraint fails: "VCS_SKETCH_OVER_CONSTRAINTS - Sketch geometry is over constrained"; the same dimension is accepted as driven (`isDriving=False`) | v2 `attempt_error`/`attempt_ok` | V+D | `would_overconstrain` → offer driven dimension |
| F19 | Fully constrained curves black + browser lock; unconstrained blue; construction dashed orange | docs 7a/7b; blue line in `screen-v2_04` | D+V | Constraint-state colouring |
| F20 | Model parameters named d1, d2, … in creation order; owned by Sketch/Extrude/Fillet/… | v1 final `parameters` | V | Same naming |
| F21 | Expressions: "d2 / 2" accepted; "foo * 2" and "15 deg" (length) rejected "Expression is invalid." | v2 attempts | V | `cad_core::expr` (tests) |
| F22 | Negative sketch dimension flips geometry (width −10 → bbox x −10…0) | v2 `v2_after_negative_width_attempt` | V | Sign-preserving distance residuals |
| F23 | Extrude dialog: Type, Profiles (n selected), Start (Profile Plane), Direction (One Side), Extent Type (Distance), Distance, Taper Angle, Operation (New Body); OK/Cancel; on-canvas arrow + distance box; status "1 Profile \| Area : 1500.00 mm^2" | `screen-v2_06_extrude_dialog.png`; docs 12 | V+D | Extrude command panel |
| F24 | Zero extrude distance → "Some input argument is invalid." | v1 `extrude_zero_distance` | V | Distance-zero error |
| F25 | Volumes: 50×30×10 = 15000 (F6 E12); 20 mm → 30000; width 60 → 36000; 4×R5 vertical fillets → 35570.8 (F10 E24); width 70 → 41570.8; Ø12 through-all cut → 39308.8 | v2 snapshots | V | Kernel integration test reproduces all values |
| F26 | Cut whose tool misses the body: "No target body!Compute Failed" warning; recovers when geometry returns | v2 `v2_width_20_hole_outside` | V | Same warning text |
| F27 | Deleting a sketch used by a cut: warning "Profile 1 missing … The profile reference is lost and this feature is using cached geometry. Edit this feature and select new profile references." | v2 `v2_after_deleting_cut_sketch` | V | CAD360 reports the lost reference as an **error** and does not keep stale geometry (documented difference) |
| F28 | Oversized fillet (R20 on 30 mm depth) gave rounded ends without an error state; R40 on a skewed body gave a degenerate 265.6 mm³ body | v2/v1 snapshots | V | OCCT fails such fillets → CAD360 shows an error and keeps the previous body (documented difference) |
| F29 | Open 3-line chain → 0 profiles; circle inside rectangle → 2 profiles (1965.4 + 113.1 mm²) | v2 `v2_open_profile_sketch`, `v2_face_sketch_profiles` | V | `find_profiles` (tests) |
| F30 | Timeline: roll back before a feature (marker), suppress/unsuppress; Undo removes the last item (8→7), Redo restores | v1 timeline/undo rows | V | Marker, suppression, snapshot undo |
| F31 | Health states: Healthy, Warning, Error, Suppressed, RolledBack | API `FeatureHealthStates` (fusion.py) | D | Same health model |
| F32 | Exports available: STEP, STL, F3D (via ExportManager); exported files lose associativity | v1 `export` (step/stl/f3d true); docs 15a | V+D | STEP/STL export; native `.c360` keeps history |
| F33 | Keyboard shortcuts: E Extrude, L Line, R Rectangle, C Circle, D Dimension, F Fillet, H Hole, X Construction, S toolbox, Ctrl+B Compute All | docs 16b | D | Same shortcuts |
| F34 | Mouse: middle-drag pan, Shift+middle orbit, wheel zoom; F6 fit | docs 1a–1c | D | Same bindings |
| U1 | Internal algorithms (ASM/Parasolid-like kernel, topological naming, constraint solver) | not accessible | U | Independently designed (OCCT, geometric re-resolution, own LM solver) |
| U2 | ViewCube edge clicks, exact over-constraint dialog wording, dark-theme constraint colours | not found | U | Designed independently |
| F35 | Joint between two planar faces (A top, B bottom; neither component grounded): the joint origins' **Z axes are aligned in the same direction**, so B ends up flipped into A (B world bbox z 10…20 on A's top at z = 20) until Flip is used; Revolute 30 deg rotates B +30° about the joint Z (rotation row (0.866, −0.5, 0)) | `out-v4/probe-log.jsonl` `v4_joint_A_then_B`; `out-v4/viewport-v4_01_joint_A_then_B.png` | V | Joint mate aligns joint Z axes; Flip reverses |
| F36 | Which component moves: Component2 (created second) moved in **both** pick orders (geometryOne = A, then geometryOne = B) | `out-v4/probe-log.jsonl` `v4_joint_A_then_B`, `v4_joint_B_then_A` | V (rule U) | Pick order alone does not decide; rule not established (two trials) |
| F37 | A second joint between the same pair is refused: "A joint in system exists for the provided input. System will be over constrained with this input." — the first joint stays healthy | `out-v4/probe-log.jsonl` `v4_conflicting_second_joint` | V | Over-constraining joints are rejected with this message |
