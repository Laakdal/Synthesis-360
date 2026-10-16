# Reference vs implementation screenshots

Left column: Autodesk Fusion 2705.1.30 captured by the investigation probe (redacted; dark theme as
configured on the reference machine). `out-v1/window-*` images are PrintWindow captures, which omit
Fusion’s floating Qt panels (browser, dialogs, palettes); `out-v2-v3/screen-*` images are screen
copies that include them. The v1 probe used an unconstrained rectangle (F14), so its geometry after
edits differs; numeric comparisons use the v2 snapshots. Right column: CAD360 captured by the scripted scenario
`docs/scenarios/workflow.json` (`cad360 --scenario … --screenshot-dir docs/screenshots/cad360`),
which drives the real application through the same commands and tool events a user produces.
Icons and artwork in CAD360 are original; only layout, organisation and behaviour follow Fusion.

| State | Fusion (reference) | CAD360 (implementation) | Notes |
|---|---|---|---|
| Empty design | `evidence/fusion-probe/out-v2-v3/screen-v2_01_new_design.png` | `screenshots/cad360/01_new_design.png` | App bar, ribbon (DESIGN ▾, SOLID…MANAGE, CREATE…SELECT), browser, ViewCube, nav bar, timeline |
| Create Sketch: pick a plane | `evidence/fusion-probe/out-v1/window-02_create_sketch_select_plane.png` (PrintWindow: floating panels not captured) | `screenshots/cad360/02_create_sketch_select_plane.png` | Origin planes shown for picking (F8) |
| Sketch mode | `evidence/fusion-probe/out-v1/window-03_sketch_mode.png`, `out-v2-v3/screen-v2_04_ui_line_drawn.png` | `screenshots/cad360/03_sketch_mode.png` | SKETCH contextual tab, FINISH SKETCH, Sketch Palette, TOP view |
| Dimensioned rectangle | `evidence/fusion-probe/out-v2-v3/screen-v2_05_constrained_rectangle.png` | `screenshots/cad360/04_sketch_rectangle_dimensioned.png` | Fully constrained (Fusion: black; CAD360 dark canvas: white) + browser lock; H/V glyphs; d1/d2 |
| Extrude dialog | `evidence/fusion-probe/out-v2-v3/screen-v2_06_extrude_dialog.png` | `screenshots/cad360/05_extrude_dialog.png` | Same inputs/order; "1 Profile \| Area : 1500.00 mm^2"; live preview |
| Extruded body | `evidence/fusion-probe/out-v1/window-06_extruded_box.png` | `screenshots/cad360/06_extruded_box.png` | 50×30×10 = 15000 mm³ in both (Fusion v1 and v2 snapshots) |
| Parameter edits | no matching Fusion screenshot (the v1 capture `out-v1/window-07_after_parameter_edits.png` shows the *unconstrained* probe rectangle skewed by the edit, 33000 mm³ — finding F14) | `screenshots/cad360/07_after_parameter_edits.png` | numbers compared with Fusion’s v2 snapshot (constrained rectangle): width 60, depth 20 → 36000 mm³ in both |
| Fillet | no matching Fusion screenshot of the dialog (v1 `window-08_fillet_dialog.png` is a PrintWindow capture, which omits Fusion’s floating dialog, and shows the skewed v1 body) | `screenshots/cad360/08_fillet_dialog.png` | 4 vertical edges, R5: 35570.8 mm³ in CAD360 and in Fusion’s v2 snapshot |
| Cut through all | `evidence/fusion-probe/out-v2-v3/screen-v2_10_final_model.png` (note: `screen-v2_09_after_delete_dependency.png` shows the state *after the cut’s sketch was deleted*, F27) | `screenshots/cad360/09_cut_dialog.png`, `10_final_model.png` | Ø12 through-all hole: 39308.8 mm³ in both (v2 snapshot) |
| Change Parameters | (dialog not captured in the probe; documented in official-docs.md row 14) | `screenshots/cad360/11_change_parameters.png` | Model parameters d1…d5 with owners, user parameters |
| Timeline rolled back | `evidence/fusion-probe/out-v1/window-11_timeline_rolled_back.png` (v1 skewed body; rolled back before the cut) | `screenshots/cad360/12_timeline_rolled_back.png` | rolled-back features dimmed in both; CAD360 marker before Fillet1 |
| Section analysis | (not captured) | `screenshots/cad360/13_section_analysis.png` | Display-only cutting plane |
| Reopened document | — | `screenshots/cad360/14_reopened.png` | History, parameters and geometry restored from `.c360` |
| Shell dialog (live preview) | (not captured; official-docs.md) | `screenshots/cad360/20_shell_dialog.png` | top face removed, 2 mm walls |
| Rectangular pattern | (not captured) | `screenshots/cad360/21_rectangular_pattern_dialog.png`, `22_pattern_result.png` | 3 instances as separate bodies |
| Components | (not captured) | `screenshots/cad360/30_two_components.png` | Component1 (active) with its own Bodies/Sketches |
| Joint dialog + result | `evidence/fusion-probe/out-v4/viewport-v4_01_joint_A_then_B.png` (Revolute 30°, top face ↔ bottom face) | `screenshots/cad360/31_joint_dialog.png`, `32_assembled.png` | both: joint Z axes aligned (F35), so B sits flipped into A with only its outline on A's top face until Flip; placements match the probe numerically (`tests/assembly.rs`) |
| Section analysis (capped) | — | `screenshots/cad360/13_section_analysis.png` | hatched stencil caps; hole shows as a gap |
| Extrude manipulator | `evidence/fusion-probe/out-v2-v3/screen-v2_06_extrude_dialog.png` (arrow + on-canvas distance box) | `screenshots/cad360/05b_extrude_manipulator.png` | arrow at the profile, live value box, dialog in sync |

## Visible differences (intentional or not yet implemented)
* CAD360 has no cloud features (data panel, comments, Autodesk Assistant); the corresponding
  buttons report that they are not part of CAD360.
* Fully constrained sketch curves are white on CAD360's dark canvas (Fusion documents black on
  its default light canvas; the dark-theme colour was not established — U2).
* SHEET METAL, PLASTIC and MANAGE tabs and the unimplemented SURFACE/MESH/UTILITIES tools are
  present but disabled with "Not yet supported in CAD360"; see `parity-matrix.md`.
* On-canvas manipulators exist for Extrude, Offset Plane and Fillet/Chamfer only; other dialogs
  take values in the dialog.
