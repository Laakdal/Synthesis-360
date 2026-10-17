# Kernel feature matrix (cad-geom vs OCCT vs ASM)

Kernel-level operations only. Application features (sketching, timeline, parameters, UI) are in
`docs/parity-matrix.md`, measured against Fusion. Status values:

* `NOT_IMPLEMENTED`: absent from cad-geom.
* `EXPERIMENTAL`: code exists, coverage incomplete or known gaps.
* `IMPLEMENTED`: works, with unit tests only.
* `TESTED`: integration and edge-case tests, agrees with OCCT where the harness compares it.
* `REFERENCE_VERIFIED`: an ASM reference row exists (`research/evidence/asm-v5/reference.json`, or `asm-v6` for features)
  and cad-geom matches it within the stated tolerances (`research/VALIDATION_REPORT.md`).
* `KNOWN_DISCREPANCY`: a documented difference from ASM.

"OCCT" is what the application uses today through `cad-kernel` (preserved, OCCT 7.8.1). Run
`cargo run -p cad-diff` to regenerate the comparison; the counts below are from the 2026-10-17 run.

## Foundation and geometry

| Operation | cad-geom status | OCCT (app) | ASM evidence | Tests / notes |
|---|---|---|---|---|
| Points, vectors, frames, transforms | TESTED | yes | — | `math.rs`, `transform.rs` |
| Tolerance policy (absolute + size-scaled) | TESTED | Confusion 1e-7 mm | point 1e-5 mm, angle 1e-10 rad (measured, v5) | `Tolerance::asm()` (1e-5 mm, 1e-10 rad) and `Tolerance::occt()` (1e-7 mm, 1e-12 rad); booleans use `asm()` (`boolean::BOOLEAN_TOLERANCE`). `Tolerance::default()` stays `occt()`: the intersection tests specify 1e-7 mm behaviour (`line_sphere_cases`: a 1e-6 mm cut gives two points), which 1e-5 mm breaks |
| Robust predicates | TESTED | — | — | `predicates.rs`: point vs plane / 2D line with a tolerance band, Above/On/Below; forward-error filter, Dot2 compensated fallback (unit tests) |
| Analytic curves: line, circle, ellipse (eval, d1, d2, length) | TESTED | yes | — | `tests/geometry.rs` |
| Analytic surfaces: plane, cylinder, cone, sphere, torus | TESTED | yes | face types match on 9 primitives | `tests/geometry.rs` |
| NURBS curves: eval, derivatives, knot insertion, split, extract, Bézier decomposition, degree elevation, exact conic arcs | TESTED | yes | — | `tests/nurbs.rs` (14); periodic split returns a clamped curve |
| NURBS surfaces: eval, derivatives, extract, Bézier patches | IMPLEMENTED | yes | — | no knot insertion or degree elevation for surfaces yet |
| Analytic → NURBS exact conversion | TESTED | yes | — | point-set equality by projection, incl. poles and apex |
| Point projection (analytic closed form, NURBS Newton) | TESTED | yes | — | `project.rs` |
| Line/curve × plane, quadric, torus intersection | TESTED | yes | — | quartic for the torus; tangency decided by distance |
| Plane × plane, sphere, cylinder; plane × cone (perpendicular) | TESTED | yes | — | oblique plane × cone: `Unsupported` |
| Curve × curve | EXPERIMENTAL | yes | — | lines and circles only |
| Curve × NURBS surface | EXPERIMENTAL | yes | — | subdivision with a work budget |
| Surface × surface (general) | NOT_IMPLEMENTED | yes | — | plane pairs only in `intersect.rs`; booleans add sphere–sphere, parallel cylinder–cylinder and coaxial cylinder–sphere internally (`boolean_quadric.rs`, `tests/curved_booleans.rs::quadric_pairs`) |

## Topology, primitives, properties

| Operation | cad-geom status | OCCT (app) | ASM evidence | Tests / notes |
|---|---|---|---|---|
| B-rep arenas, builder, queries | TESTED | yes | — | `topology.rs` |
| Validation (references, loops, edge use, Euler–Poincaré + genus, geometry on topology, orientation, unevaluable faces) | TESTED | `BRepCheck` | — | `tests/validate.rs` (13), incl. broken-model cases. Only edges listed in `Brep::nonmanifold_edges` may have 2k > 2 uses (k each way; counted as k edges in Euler–Poincaré); the same model without the flag is reported |
| Box | REFERENCE_VERIFIED | yes | `prim.box`, `prim.box.tiny`, `prim.box.huge` | 6/12/8 like ASM; tiny box: ASM centroid not usable (Fusion bug), compared on volume, area, topology |
| Cylinder (axis-aligned and tilted) | REFERENCE_VERIFIED | yes | `prim.cylinder`, `prim.cylinder.tilted` | ASM seamless topology 3/2/2 reproduced; OCCT differs (seam edge: TOPOLOGY_CONVENTION) |
| Cone (pointed) and frustum | REFERENCE_VERIFIED | yes | `prim.cone`, `prim.frustum` | apex = degenerate edge with no curve, as in ASM |
| Sphere | REFERENCE_VERIFIED | yes | `prim.sphere` | 1 face, 0 edges, 0 vertices, like ASM; OCCT has a seam and 2 pole edges |
| Torus (ring) | REFERENCE_VERIFIED | yes | `prim.torus` | 1/0/0 like ASM |
| Spindle / horn torus (minor ≥ major) | KNOWN_DISCREPANCY | — | v5 extras: ASM accepts it as the outer "apple" surface with apex edges | cad-geom rejects it |
| Degenerate input handling | KNOWN_DISCREPANCY | — | v5 extras: a zero-height box gives a sheet body in ASM; a negative height is rejected; r = 0 sphere fails | cad-geom rejects every non-positive dimension |
| Size range | TESTED | — | ASM refuses a 1e-6 mm cube, accepts 1e-5 mm to 1e7 mm | cad-geom has no size box; exact from 0.01 mm to 1e5 mm (tested) |
| Mass properties: volume, area, centroid, inertia (exact, Green + Gauss–Legendre) | REFERENCE_VERIFIED (primitives, all 10 booleans) | volume/area/centroid (no inertia binding) | all 9 primitives; the 6 box booleans (incl. plane faces with inner loops, `tests/booleans.rs::mass_of_faces_with_inner_loops`); the 4 cylinder/sphere booleans (plane faces with circle loops, a seamless hole wall, a cylinder face with a seam, a sphere face with 6 hole loops) | worst error vs closed form 5e-14 (curved booleans 1e-12). ASM's own inertia on the frustum and tilted cylinder departs from axisymmetry by ~1.7e-8 (base-feature accuracy); `tests/asm_reference.rs` documents the bound used. Fixed: a sphere face lying outside loops that do not wind round its axis (both poles inside) integrated to minus the hole (`tests/mass.rs::sphere_face_outside_a_non_winding_loop`). Loops passing close to a sphere's parametric pole lose accuracy (~1e-9); boolean results therefore carry a sphere frame whose poles are far from every loop |
| Both topology conventions (seamed OCCT-style and seamless ASM-style) in mass and tessellation | TESTED | — | — | seamed builders kept as test fixtures |
| Tessellation of analytic faces | TESTED | yes | — | watertight; volume error ≈ (5/3)·chord/r (sphere), ≈ (4/3)·chord/r for a circle section; plane faces with holes and trimmed curved faces (`tests/tess.rs`, 16); edge sample counts propagate transitively through grid faces (fixed for torus/cone rim bands) |
| NURBS faces in mass, tessellation, validation | NOT_IMPLEMENTED | yes | — | reported as `Unevaluable` / `Unsupported`, never silently passed |

## Modelling operations

| Operation | cad-geom status | OCCT (app) | ASM evidence (v5 TemporaryBRepManager, v6 design features) | Notes |
|---|---|---|---|---|
| Planar profiles (lines, arcs, circles; one outer loop + holes) | TESTED | yes (sketch profiles) | — | `profile.rs`: closure, crossing/touching segments, containment, orientation (outer CCW, holes CW); unit tests. Merging consecutive collinear lines / co-circular arcs is a design choice with no ASM evidence (no recorded case has such neighbours) |
| Extrude (distance, negative, symmetric/two-sided, tilted plane, holes, arcs) | REFERENCE_VERIFIED for the 9 `ext.*` cases | yes | v6: all 9 `ext.*` cases MATCH (F/E/V/L, face/edge types, volume/area ≤ 4e-15, inertia ≤ 8e-10) | `extrude.rs`; one face per segment, seamless side face for a full circle, exactly as ASM's ExtrudeFeature. `tests/extrude.rs` (8: closed forms, = box/cylinder primitives, = ASM `bool.cut.hole`, ASM v6 record, watertight mesh, orientation independence, scale). No taper / to-face extents |
| Revolve (full and partial, on-axis edges and points, holes and pockets as void shells) | REFERENCE_VERIFIED for the 8 `rev.*` cases | yes | v6: all 8 `rev.*` cases MATCH (seamless circles; sphere and torus 1/0/0; cone apex = degenerate edge; partial revolve with planar caps) | `revolve.rs`; `tests/revolve.rs` (14: Pappus closed forms incl. inertia, = cylinder/cone/sphere/torus primitives, mirror/tilt/scale, watertight mesh, invalid input). ASM's own inertia for `rev.rect.full` is 1.2e-8 off the exact value (ours 1e-12), so that one case is held to 1e-8 against ASM. Void-shell handling of holes, separate apex vertices at an axis pinch and partial sweeps touching the axis have no ASM case. An arc off the axis that reaches it (spindle/horn torus) → `Unsupported` |
| Boolean union / cut / intersect (planar polyhedra) | REFERENCE_VERIFIED for the 6 recorded box cases; TESTED for other polyhedra (no ASM rows yet) | yes | 10 cases recorded; the 6 box cases MATCH | `boolean.rs`: face splitting + classification + coplanar-face merge and collinear-edge merge; `tests/asm_booleans.rs`, `tests/booleans.rs` (contact, containment, empty result, near-coincidence, scale 1e-2/1e4, rotated boxes). Two *polyhedra* touching along an edge or at a vertex give separate lumps (no ASM row for that configuration; ASM's recorded line contact is box ∪ cylinder, below). |
| Boolean union / cut / intersect (planes, cylinders, spheres) | REFERENCE_VERIFIED for the 4 recorded cases (`bool.cut.hole`, `bool.union.tangent_cylinder`, `bool.union.near_tangent_cylinder`, `bool.intersect.box_sphere`); TESTED otherwise | yes | the 4 cases MATCH: F/E/V/L, shells, lumps, face/edge types, loops per face, coedges per edge; volume, area, centroid within 1e-9 and inertia within 1e-8 (near-tangent: 2.3e-8, see the next table) | `boolean_quadric.rs`: per-face arrangement of lines and circles in 3D, cycles grouped in an orientation-preserving chart (plane frame; annulus map for cylinders, so seamless bands stay seamless; stereographic for spheres), ray-cast classification, same-surface merge. Line contact = ONE edge with 4 coedges in one lump, flagged in `Brep::nonmanifold_edges` (validator exemption), as ASM. Surface pairs: plane with plane/cylinder (axis-parallel or perpendicular)/sphere, sphere–sphere, parallel cylinders, coaxial cylinder–sphere; oblique plane × cylinder (ellipse), other cylinder/sphere pairs, cones, tori and NURBS → `Unsupported`. `tests/curved_booleans.rs` (12: containment both ways, off-centre hole and notch, sphere cut by a box corner, holes cut twice, tangent contact at ±5e-6/±1e-5/±2e-5 mm, quadric pairs, rotation and scale 1e-2/1e3; volumes vs circle-segment, spherical-cap and octant closed forms), `tests/asm_booleans.rs::curved_booleans_match_asm` |
| Face split, trim, imprint | NOT_IMPLEMENTED | yes | — | |
| Chamfer (equal distance) and constant-radius fillet | REFERENCE_VERIFIED for the 8 recorded cases (`fillet.edge`, `fillet.vertical4`, `fillet.corner3`, `fillet.cyl_top`, `fillet.hole_edge`, `chamfer.edge`, `chamfer.vertical4`, `chamfer.cyl_top`) | yes | v6: all 8 MATCH (F/E/V/L, face and edge types; volume/area ≤ 3e-14; inertia ≤ 6e-9) | `blend.rs`: straight edge between two planes (any dihedral angle, convex or concave; several edges without a shared vertex), the three-edge convex corner with a sphere vertex blend, a closed circle between a plane and a coaxial cylinder (torus / cone band). `tests/blend.rs` (16: closed forms, = rounded-rectangle / octagon / L extrudes, OCCT volumes, ASM record, watertight mesh, edge picking, invalid input). `Unsupported`: two selected edges at a vertex other than the convex 3-edge corner, oblique end faces for fillets, other face pairs, spindle tori. Found and fixed: tessellation sample counts were not propagated transitively across grid faces (`tests/blend.rs::capsule_tessellates_watertight`) |
| Shell (inside, removed faces) | REFERENCE_VERIFIED for `shell.top`; TESTED otherwise | yes | v6: `shell.top` MATCH (11/24/16, 5432 mm³) | `shell.rs`: planar solids only; kept faces offset inward exactly, inner solid subtracted with the planar boolean (retry with removed faces in place when an outward push would cut material next to a reflex edge). `tests/shell.rs` (11: L shape, two removed faces, closed hollow (void shell), pyramid with 4 faces at the apex, rotated box, thickness limits). Curved faces, outside direction, thickness that changes topology → `Unsupported` / `InvalidInput` |
| Offset (whole solid) | NOT_IMPLEMENTED | yes | — | |
| Sweep, loft | NOT_IMPLEMENTED | partly | — | |
| Sewing / stitching, healing | NOT_IMPLEMENTED | yes | — | |
| STEP import/export | NOT_IMPLEMENTED | yes | 19 STEP + 27 SAT/SMT exports recorded | |

## Recorded ASM vs OCCT differences (the targets for cad-geom's booleans)

| Case | ASM | OCCT (as the app uses it) |
|---|---|---|
| Tangent cylinder union (`bool.union.tangent_cylinder`) | one non-manifold solid: the contact line is one edge with 4 coedges | 2 solids (cad-geom: same as ASM, 10/17/10, one lump, the edge flagged non-manifold; MATCH) |
| Near-tangent union, overlap 1e-5 mm (`bool.union.near_tangent_cylinder`) | treated as tangent (offset = its point tolerance) | cuts the real sliver (~1.13 mm² more area) (cad-geom: treated as tangent, 10/17/10, MATCH; the contact vertices sit halfway between plane and cylinder, so the volume is 3.3e-4 mm³ (2.3e-8) below ASM's plain sum and the result validates at 1e-5 mm, not 1e-7 mm) |
| Through hole (`bool.cut.hole`), box ∩ sphere (`bool.intersect.box_sphere`) | seamless hole wall and sphere face bounded by closed circles (7/14/10, 7/6/6) | seam edges (7/15/10, 7/9/7) (cad-geom: same as ASM, MATCH) |
| Coincident face union | merged into one 40×20×20 box, 6/12/8 (cad-geom: same, MATCH) | faces left unmerged (10/20/12) |
| Seams and poles on periodic faces | none | seam edges and degenerate pole edges |
| Features on circles (v6: `ext.circle`, `ext.rect_hole`, `ext.tilted`, 360° revolves, `fillet.cyl_top`, `fillet.hole_edge`, `chamfer.cyl_top`) | seamless: closed circles with one vertex, sphere/torus faces with no edges | seam edges, pole edges; the application passes circles as two half arcs, so its extruded circle is 4/6/4 (cad-geom: same as ASM, MATCH) |
| Vertex blend (`fillet.corner3`) | sphere patch bounded by 3 arcs, 10/21/13 | 10/22/13 (an extra degenerate edge) (cad-geom: same as ASM, MATCH) |

Source: `research/VALIDATION_REPORT.md`, `research/REFERENCE_BEHAVIOR.md` (v6 section: feature-level conventions).
