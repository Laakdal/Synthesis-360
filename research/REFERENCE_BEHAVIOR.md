# ASM reference behaviour (observed)

All rows were observed in experiment **v5**: Fusion 2705.1.30, `TemporaryBRepManager` through the
public API. See [`REA_EXPERIMENTS.md`](REA_EXPERIMENTS.md) for the method. The numeric values are in
[`evidence/asm-v5/reference.json`](evidence/asm-v5/reference.json), and the raw per-entity records
are in `evidence/asm-v5/raw/probe-log.jsonl`.

Units are mm. "Verified" means the behaviour was measured directly through the API in this run. It
is not inferred or estimated.

## Topology conventions

| behaviour | source (v5 case id) | input | observed output / error | confidence | implication for the Rust kernel |
|---|---|---|---|---|---|
| A box is 6 planar faces, 12 line edges and 8 vertices; each face has 1 loop and each edge has 2 coedges | prim.box | box 50×30×10 | 6/12/8, loops 6, coedges per edge {2: 12}, solid, 1 lump, 1 shell | verified | baseline; must match exactly |
| The cylinder has **no seam edge**. The lateral face has **2 loops** (both `isOuter=true`), each a single closed circle edge with **one vertex** | prim.cylinder, prim.cylinder.tilted | r10 h20; tilted axis (1,1,1) | 3 faces (cylinder + 2 plane), 2 circle edges, 2 vertices, 4 loops, seam_edges 0. `degenerate_edges` = 2 under the task definition (start = end vertex), but ASM flags neither as `isDegenerate` | verified | F/E/V parity requires periodic faces with no seam (closed edges bounding a periodic face). A kernel that adds a seam will report 3/3/2; count-normalise or adopt the seamless convention |
| The cone apex is a **degenerate edge**: `isDegenerate=true`, `geometry=None`, 1 coedge (on the cone face), start = end vertex at the apex. The cone face has 2 loops | prim.cone | r0 10, r1 0, h 20 | 2 faces (cone + plane), 2 edges (closed circle + degenerate apex edge), 2 vertices, 3 loops. `degenerate_edges` = 2 under the task definition (start = end vertex): 1 closed circle + 1 truly degenerate apex | verified | ASM represents the apex as a degenerate edge plus a vertex, not as a bare vertex. Expect E=2, V=2 |
| The frustum has the same structure as the cylinder: 3 faces, 2 closed circle edges, 2 vertices, no seam | prim.frustum | r0 10, r1 5, h 20 | 3/2/2, face types cone + 2 plane | verified | same as the cylinder |
| The sphere is **1 face, 0 edges, 0 vertices, 0 loops**: no seam and no pole edges or vertices | prim.sphere | r10 | 1/0/0, loops 0 | verified | a closed periodic face with no boundary at all. A kernel with a seam and poles (e.g. 1/1/2 or 1/3/2) will differ |
| The torus is **1 face, 0 edges, 0 vertices, 0 loops** | prim.torus | R20 r5 | 1/0/0 | verified | same as the sphere: a doubly periodic face with no seam edges |
| No edge or vertex was ever tolerant; `BRepEdge.tolerance` and `BRepVertex.tolerance` return 0.0 for exact entities | all cases | — | max_edge_tolerance 0.0, max_vertex_tolerance 0.0, 0 tolerant entities | verified | analytic primitives and these booleans produce exact (non-tolerant) topology, so there is no tolerant-edge parity to match |
| Persisting the body in a base feature does not change topology | all cases | `BRepBodies.add(body, baseFeature)` | persisted F/E/V and face types identical to the temporary body in every case | verified | the temporary-body counts are the reference |

## Booleans

| behaviour | source | input | observed output / error | confidence | implication |
|---|---|---|---|---|---|
| A general overlapping union gives a single lump with no leftover imprint edges | bool.union.overlap | 20³ ∪ 20³ shifted (10,10,10) | 12/30/20, V 15000 (= 8000 + 8000 − 1000), A 4200 | verified | standard result |
| A through-hole cut leaves a cylinder face with 2 loops and **no seam**; the top and bottom faces get an inner loop | bool.cut.hole | 50×30×10 − r6 cylinder | 7/14/10, loops 10, V 13869.02664 (= 15000 − 360π) | verified | the hole wall is seamless, as for the primitive cylinder |
| A **fully coincident face** union merges the two coplanar faces completely: the result is a plain 40×20×20 box with no imprint edges | bool.union.coincident_face | 20³ ∪ 20³ at x=20 | 6/12/8, V 16000, A 4000 | verified | the kernel must merge coplanar same-surface faces after a union (face "regularisation"). Leaving split faces gives 10/20/12 or similar |
| A **partial coincident face** union leaves the target's x=20 face with 1 outer and 1 inner loop. The tool's contact face disappears, and its side faces are not merged with the target's faces (they are not coplanar) | bool.union.coincident_partial | 20³ ∪ 20×10×10 at (20,5,5) | 11/24/16, loops 12, V 10000, A 3200 | verified | the expected count for a correct coplanar union |
| A cut with a tool whose faces are coplanar with the target's top and bottom produces a clean through-hole (genus 1); the coplanar faces are not left as slivers | bool.cut.coplanar_through | 20³ − 10×10×20 at (5,5,0) | 10/24/16, top and bottom faces 2 loops each, V 6000, A 3000 | verified | coplanar cap handling must yield exactly a through-hole |
| A **tangent (line-contact) union succeeds with a single lump and a non-manifold edge**. The tangent line edge has 4 coedges (cylinder face twice, two halves of the box's x=20 face). The cylinder face has a single loop, and the box's x=20 face is split in two along the contact line | bool.union.tangent_cylinder | 20³ ∪ r10 cylinder centred at x=30 | returned `true`; 10/17/10, loops 10, 1 lump, 1 shell, `isSolid=true`, seam_edges 1 (the non-manifold edge), V 14283.18531 (= 8000 + 2000π, exact sum), V−E+F = 3 | verified | ASM does **not** separate line-contact solids into 2 lumps or reject them: it builds a non-manifold single-shell solid. The Rust kernel needs a declared policy (match it, or report the difference as expected) |
| A **near-tangent union with a 1e-5 mm overlap (= pointTolerance) behaves exactly like the tangent case**: the thin overlap sliver is not resolved | bool.union.near_tangent_cylinder | cylinder centred at x=29.99999, r10, so it spans x 19.99999–39.99999 and overlaps the box (x ≤ 20) by 1e-5 mm | identical topology (10/17/10 with the same 4-coedge non-manifold line edge), and volume and area identical to the tangent case to all printed digits (14283.18531 / 4284.955592). The bbox max x is 39.99999 (cylinder geometry not moved), and the centroid x is 18.798013 vs 18.798017 | verified | ASM treats a 1e-5 mm overlap as line contact. The Rust kernel's coincidence tolerance should be 1e-5 mm to reproduce this |
| Box ∩ sphere leaves 6 planar disc faces (1 loop each) and one sphere face with 6 loops (2 flagged outer, 4 inner). Each circle edge is closed, with 1 vertex | bool.intersect.box_sphere | 20³ centred ∩ sphere r12 | 7/6/6, loops 12, V 6383.716274, A 1734.159145 | verified | another periodic face bounded only by closed edges, with no seams. Loop "outer" flags on a sphere are arbitrary (2 of 6) |
| A cut with a disjoint tool returns `true` and leaves the target unchanged | bool.cut.disjoint | 10³ − 5³ far away | `booleanOperation` returned `true`; 6/12/8, V 1000 | verified | a no-op difference is a success, not an error |
| Repeated unions accumulate correctly; the base top face ends with 1 outer and 5 inner loops | bool.union.repeated | 40×10×10 ∪ 5 small boxes | each step returned `true`; final 31/72/48, loops 36, V 4320, A 2120 | verified | per-step counts: 11/24/16, 16/36/24, 21/48/32, 26/60/40, 31/72/48 |

## Degenerate inputs and the self-intersecting torus

| behaviour | source | input | observed output / error | confidence | implication |
|---|---|---|---|---|---|
| A zero-height box is **accepted** and returns an open sheet body (not an error) | extra.box.zero_height | 10×10×0 | 1 plane face, 4 line edges (1 coedge each), 4 vertices, `isSolid=false`, volume 0, area 100 | verified | ASM degrades to a 2-D sheet. The Rust kernel should either match or reject explicitly; the harness should not expect an error |
| A negative box dimension is rejected **before** ASM, in `OrientedBoundingBox3D.create` | extra.box.negative_height | 10×10×−5 | `RuntimeError: 3 : invalid argument height` | verified | input validation belongs at the API layer |
| A zero-radius sphere is rejected | extra.sphere.zero_radius | r = 0 | `RuntimeError: 2 : InternalValidationError : result` | verified | reject r ≤ 0 |
| A **spindle torus (minor > major) is accepted** as a valid solid: the outer "apple" surface. The inner lemon is removed, and both apexes on the axis are degenerate edges | extra.torus.minor_gt_major | R 5, r 20 | solid, 1 torus face with 2 loops, 2 degenerate edges (no geometry), 2 vertices at z = ±√(r²−R²) = ±19.365. bbox ±25 × ±25 × ±20. V 56374.6566, A 7158.3827 | verified; V and A match the analytic apple region (revolve the disc part with ρ ≥ 0, counted once) to about 1e-9 relative, **not** Pappus 2π²Rr² = 39478.4 | the self-intersecting case is defined as the union point set. A Rust kernel that refuses r > R, or uses Pappus, will differ |
| A horn torus (minor = major) is accepted: 2 degenerate edges sharing **one** vertex at the centre | extra.torus.minor_eq_major | R 10, r 10 | solid, 1 face (2 loops), 2 degenerate edges, 1 vertex, V 19739.2088 (= 2π²Rr²), A 3947.8418 (= 4π²Rr) | verified | the pinch point is a single shared vertex |

## Size limits and tolerances

| behaviour | source | input | observed output / error | confidence | implication |
|---|---|---|---|---|---|
| Global tolerances | environment | `Application.pointTolerance`, `vectorAngleTolerance` | 1e-6 cm = **1e-5 mm**; **1e-10 rad** | verified | use these as the coincidence tolerances in the parity harness |
| The lower creation limit lies between 1e-6 mm and 1e-5 mm (i.e. at pointTolerance) | extra.size.cube_1e-06mm, cube_1e-05mm | cubes of edge 1e-6 / 1e-5 mm | 1e-6 mm: `createBox` raises `RuntimeError: 3 : invalid argument box` (the OBB was created). 1e-5 mm: accepted, 6/12/8, V 1e-15, A 6e-10 | verified | reject features below pointTolerance |
| At an edge of exactly pointTolerance (1e-5 mm), `BRepBody.boundingBox` collapses to a single point | extra.size.cube_1e-05mm | cube 1e-5 mm | bbox min = max = (1e-5, 1e-5, 1e-5) mm, while the edge lengths are 1e-5 mm | verified | the bbox is unreliable at the tolerance limit; don't compare bboxes there |
| Large models: no upper limit up to 1e7 mm (10 km); creation and mass properties are exact, with no warning | prim.box.huge, extra.size.cube_1e+06mm, cube_1e+07mm | boxes with edges from 1e5 mm (100 m) to 1e7 mm (10 km) | 6/12/8, V and A exact (1e21 mm³ for 1e7), centroid and inertia valid | verified | no upper bound is needed for parity at these scales |
| **Tiny-body mass-property defect.** For small volumes, `PhysicalProperties.centerOfMass` is the **unnormalised first moment** ∫x dV (not divided by V), and `getXYZMomentsOfInertia` returns **plain second moments** (∫x² dm, +∫xy dm about the origin) instead of the inertia tensor. Volume and area stay correct | prim.box.tiny; extra.size.cube_1e-05mm, 1e-04mm, 1e-03mm | 0.01×0.02×0.03 box; cubes ≤ 1e-3 mm | box.tiny: centerOfMass = (3e-12, 6e-12, 9e-12) cm = V·(cx,cy,cz). xx = 1.57e-17 = m·a²/3 = ∫x² dm, and products are positive. Bodies of 754 mm³ (`prim.cylinder.tilted`) and larger are correct. The threshold lies between 6e-6 mm³ and 754 mm³ and was not pinned down | verified | treat ASM centroid and inertia as **unavailable** for tiny bodies (they are `null` in `reference.json`). Don't fit the Rust kernel to them |
| Convention of Fusion's XYZ moments | prim.box | box 50×30×10 at the origin | `getXYZMomentsOfInertia` = (0.3925, 1.0205, 1.3345, −0.44156, −0.08831, −0.14719) kg·cm². This is about the **world origin**, with products in **tensor form** (−∫xy dm), matching the closed form exactly | verified | `reference.json` reports the centroidal tensor (parallel-axis shift), unit density, mm⁵, products = −∫(x−cx)(y−cy) dV |

## API-level behaviour (affects how references can be produced)

| behaviour | source | input | observed output / error | confidence | implication |
|---|---|---|---|---|---|
| `getPhysicalProperties` on a **temporary** body silently returns placeholders: volume 0, area 0, density 0, mass 0, centerOfMass (0,0,0), XYZ moments (1,1,1,0,0,0), principal (1,1,1), radius of gyration 1.797e308. The same happens with `BRepBody.physicalProperties` | all cases | `body.getPhysicalProperties(VeryHigh)` | `ok=true`, values as listed, no exception | verified | mass properties require persisting the body (base feature). `BRepBody.volume` and `.area` on the temporary body **do** work |
| `TemporaryBRepManager.exportToFile` supports `.smt`, `.sat`, `.smb` and `.sab`; `.step` is rejected | all cases | `exportToFile([body], "<id>.step")` | `RuntimeError: 3 : invalid argument filename` | verified | STEP references must come from a design document's `ExportManager` (as done here) |
| `booleanOperation` modifies the target in place and returns `bool`; it returned `true` in every catalogue case, including the tangent, near-tangent and disjoint cases | all bool.* | — | `true` | verified | there is no failure signal for degenerate configurations; compare the topology instead |
