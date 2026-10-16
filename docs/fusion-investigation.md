# Autodesk Fusion 360 investigation report

Reference build: **Autodesk Fusion 2705.1.30** (Windows, signed-in Education licence), studied on
2026-10-16. Method and tool limits: see `REA-readiness.md`. Evidence index: `evidence/findings-ledger.md`
(F1–F34, U1–U2), probe source and logs in `evidence/fusion-probe/`, official documentation in
`evidence/official-docs.md`. Confidence labels: **V** verified in the running app, **D** documented,
**I** inferred, **U** not established.

## 1. How the evidence was obtained
1. REA: artifact identity/metadata of `Fusion360.exe` (PE resources, artifact graph) — it is a 0.9 MB
   launcher; Fusion's code lives in DLLs and was **not** decompiled (no decompiler installed; licence).
2. Three scripted passes of a temporary add-in inside the real Fusion using its public API: ribbon/
   toolbar/palette dumps (3121 command definitions), a sketch → constrain → extrude → edit → fillet →
   cut → rollback → suppress → revolve → export workflow with measured volumes, face/edge counts,
   timeline health and error texts, synthetic mouse clicks with the real Rectangle/Line/Circle tools,
   viewport images and full-window screenshots (redacted).
3. Fusion's shipped API type definitions (`adsk/fusion.py`, `adsk/core.py`, version-matched).
4. Autodesk help pages (fetched; quotes in `official-docs.md`).

## 2. User interface (reference screenshots: `evidence/fusion-probe/out-v2-v3/screen-*.png`)
* **Application bar**: home, data panel, File ▾, Save, Undo/Redo ▾; document tab centred; account,
  notifications, help at the right (F1).
* **Ribbon**: workspace switcher DESIGN ▾; tabs SOLID · SURFACE · MESH · SHEET METAL · PLASTIC ·
  UTILITIES · MANAGE; SOLID panels CREATE ▾ · MODIFY ▾ · ASSEMBLE ▾ · CONFIGURE ▾ · CONSTRUCT ▾ ·
  INSPECT ▾ · INSERT ▾ · SELECT ▾ with large promoted icons (F1).
* **Sketch mode**: contextual SKETCH tab with CREATE / MODIFY / CONSTRAINTS panels and a green ✓
  FINISH SKETCH; the camera turns to look at the sketch plane; mm grid labels (F9); **Sketch
  Palette** floating at the right with Options (Linetype, Look At, Sketch Grid, Snap, Slice, Profile,
  Points, Dimensions, Constraints, Projected/Construction Geometries, 3D Sketch) and Finish Sketch (F10).
* **Browser** floating top-left: (Unsaved) › Document Settings (Units: mm, g; Hybrid Design) ·
  Named Views · Origin (hidden by default) · Bodies · Sketches, each with eye toggles (F2).
* **Timeline** along the bottom: ⏮ ◀ ▶ ▶ ⏭ playback, one icon per feature, marker, gear (F3).
* **Canvas**: ViewCube top-right (TOP/FRONT/RIGHT + XYZ triad) (F5); navigation bar bottom-centre:
  Orbit, Look At, Pan, Zoom, Fit, Display Settings, Grid and Snaps, Viewports (F4); COMMENTS bottom-left;
  dark grey gradient (user theme), Z-up, orthographic by default (F6, F7).
* **Command dialogs** float at the right of the canvas (e.g. EXTRUDE: Type, Profiles "1 selected",
  Start, Direction, Extent Type, Distance, Taper Angle, Operation, OK/Cancel) with an on-canvas
  manipulator and distance box, and a status line "1 Profile | Area : 1500.00 mm^2" (F23).

## 3. Sketching workflow (V unless noted)
* Create Sketch without a selection shows the origin planes and waits for a plane or planar face;
  with a preselected plane it enters sketch mode immediately (F8).
* Sketch frames (F11/F12): XY (x+X, y+Y), XZ (x+X, y−Z), YZ (x−Z, y+Y); a face sketch's origin is the
  world origin projected onto the face.
* 2-Point Rectangle adds 2 Horizontal + 2 Vertical constraints (F13); the line tool infers
  Horizontal when drawn about 1° off (F15); snapping a circle's radius point to the origin adds a
  Coincident (F16).
* H/V + origin coincident + two dimensions ⇒ fully constrained; one dimension ⇒ not (F17). Fully
  constrained curves are black with a browser lock, unconstrained blue, construction dashed orange (D, F19).
* Over-constraint: a third driving dimension or a redundant parallel fails with
  "VCS_SKETCH_OVER_CONSTRAINTS - Sketch geometry is over constrained"; the same dimension can be
  added as a driven dimension (F18, D).
* Model parameters d1, d2, … are created per dimension/feature value in creation order (F20);
  expressions such as `d2 / 2` work; `foo * 2` or `15 deg` for a length are rejected with
  "Expression is invalid." (F21); a negative dimension flips the geometry (F22).
* Profiles: closed regions shade blue; an open chain yields no profile; a circle inside a rectangle
  yields two profiles (ring + disc) (F29, D).

## 4. Modelling workflow (V)
* Extrude 50×30 by 10 mm → 15000 mm³, 6 faces, 12 edges; editing the distance or a sketch dimension
  recomputes the body (30000, 36000 mm³) (F25).
* Fillet four vertical edges R5 → 35570.8 mm³ (10 faces); changing the width upstream keeps the
  fillet (41570.8 mm³) — Fusion re-identifies the edges after the edit (F25).
* Sketch on the top face + Ø12 through-all cut → 39308.8 mm³; when the hole falls outside the body
  the cut shows "No target body! Compute Failed" and recovers when it fits again (F26).
* Deleting the sketch used by the cut leaves the cut with a lost-reference warning and cached
  geometry (F27). Oversized fillets produced odd geometry without an error (F28).
* Zero distance → "Some input argument is invalid." (F24). Revolve 360° of a rectangle produced a ring.
* Timeline: rolling the marker before a feature hides its effect; suppress/unsuppress; Undo removes
  the last timeline item and Redo restores it (F30). Health states: Healthy, Warning, Error,
  Suppressed, RolledBack (F31).
* Exchange: STEP, STL and F3D exports succeeded; exports do not keep associativity (F32, D).

## 5. Interaction
Middle-drag pan, Shift+middle orbit, wheel zoom, F6 fit; ViewCube face → orthographic view, corner
→ isometric; right-click marking menu; window/crossing selection; selection priority filters;
keyboard shortcuts E/L/R/C/D/F/H/X/S… (D, F33–F34). Standard views (top eye +Z, front eye −Y,
right eye +X, iso (1,−1,1)) were measured from the camera API (F6).

## 6. Feature inventory and prioritisation
Importance: ★★★ core parametric workflow · ★★ common · ★ advanced. Feasibility with OCCT + egui:
H high · M medium · L low.

| Area | Feature | Importance | Feasibility | CAD360 plan |
|---|---|---|---|---|
| Sketch | Line, rectangles, circle, 3-point arc, point | ★★★ | H | Milestone 2 |
| Sketch | Dimensions with expressions, driven dimensions | ★★★ | H | Milestone 2 |
| Sketch | Geometric constraints (12 types) | ★★★ | H (own solver) | Milestone 2 |
| Sketch | Constraint-state colours, DOF, lock | ★★★ | H | Milestone 2 |
| Sketch | Trim, extend, offset, mirror, fillet, polygon, spline, slot, project | ★★ | M | later |
| Solid | Extrude (distance/All, one side/symmetric, join/cut/intersect/new) | ★★★ | H | Milestone 3 |
| Solid | Revolve, fillet, chamfer, hole | ★★ | H | Milestone 3/6 |
| Solid | Shell, draft, sweep, loft, patterns, mirror, press-pull | ★★ | M | later |
| History | Timeline, marker, edit, suppress, delete, reorder, health | ★★★ | H | Milestone 4 |
| History | Parameters dialog (model + user parameters) | ★★★ | H | Milestone 4 |
| Files | Native save/open with history | ★★★ | H | Milestone 5 |
| Files | STEP export/import, STL export | ★★★ | H | Milestone 5 |
| Construct | Offset plane | ★★ | H | Milestone 6 |
| Inspect | Measure | ★★ | H | Milestone 6 |
| Inspect | Section analysis, interference | ★ | M | later |
| Appearance | Body colours | ★ | H | Milestone 6 |
| Assembly | Components (one level, activation, visibility), joints (Rigid / Revolute / Slider, parametric angle / offset) | ★★ | M | implemented (simultaneous joint solving, nested components, motion study: later) |
| Surface/Mesh/Sheet metal/CAM/Simulation | | ★ | L | out of scope |

## 7. Known uncertainties (U)
* Internal algorithms (kernel, topological naming, solver, recompute scheduling) are not observable
  and were not reverse-engineered; CAD360's implementations are independent designs.
* ViewCube edge clicks, exact over-constraint dialog wording in the UI, dark-theme constraint
  colours, Grid-and-Snaps menu contents.
* Whether the Rectangle tool's H/V constraints are added in every variant (only the 2-point tool
  was observed).
* Assemblies were probed once (v4, ledger F35–F37, two ungrounded components). Verified: planar
  face joint origins have Z = outward normal and the two joint Z axes are **aligned** (Flip
  reverses); Revolute 30° gives B's rotation row (0.866, −0.5, 0) and origin translation
  (10.76, 33.995, 20), i.e. B turns −30° about A's joint Z; the component created second moved in
  both pick orders; a second joint between the same pair is refused ("A joint in system exists for
  the provided input. System will be over constrained with this input."). Still uncertain: the
  full rule for which component moves (CAD360: root / grounded never move, otherwise the
  earlier-created component of the cluster stays — only two trials), Flip combined with an angle,
  the slider offset direction, circular-edge origin axes, behaviour of features created in a moved
  component on its origin axes (Move, patterns), motion limits, as-built joints and component
  nesting.
* Shell, Combine, Mirror, patterns and Move were not probed either; CAD360's dialogs follow the
  documented inputs (D) with independently designed defaults (I).
