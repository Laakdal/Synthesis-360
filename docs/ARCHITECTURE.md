# CAD360 architecture

CAD360 is a native Rust desktop application. Its architecture follows the principles found in the
ArtCraft/CADCraft investigation (`docs/evidence/artcraft-architecture.md`): a pure-Rust engine in
layered crates that never depend on the UI, a single swappable UI crate, "everything is a command",
local-first operation and a self-contained native document format. Fusion 360 is the behavioural
reference (`docs/evidence/findings-ledger.md`).

## Crates and layering

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ cad-app  (binary `cad360`)                       egui 0.37 · eframe · wgpu 30 │
│                                                                              │
│  ui/            ribbon · browser · timeline · dialogs · sketch palette ·     │
│                 ViewCube · nav bar · marking menu · theme · icons            │
│  viewport/      camera · offscreen wgpu renderer (MSAA, depth) · scene ·     │
│                 CPU picking (faces / edges / profiles / planes)              │
│  app.rs         layout, input routing, shortcuts, file dialogs               │
│  scenario.rs    scripted command replay + screenshots                        │
│  state/   ◄──── headless application state (testable without a window)      │
│     mod.rs      AppState: document, kernel, model, history, selection, mode, │
│                 transactions (begin/commit/cancel), files, messages          │
│     command.rs  command registry (labels, shortcuts, panels, supported)      │
│     sketch.rs   sketch session, tools, dimensions, constraints, overlay      │
│     features.rs feature dialogs (extrude, revolve, fillet, …), parameters    │
└───────────────┬──────────────────────────────────────────────────────────────┘
                │ depends on (never the reverse; enforced by tests/layering.rs)
┌───────────────▼──────────────────────────────────────────────────────────────┐
│ cad-kernel        geometry kernel integration (no UI)                        │
│   recompute.rs    timeline evaluation, health, incremental cache             │
│   refs.rs         persistent face/edge references                            │
│   body.rs         solids, topology, display meshes with face/edge ids        │
│   frame.rs        sketch-plane frames (Fusion-verified)                      │
│   exchange.rs     STEP / STL                                                 │
│   occ.rs          safe wrapper ──► opencascade-sys (vendored, cxx bridges)   │
│                                    └─ cad360.hxx exception-safe shim         │
│                                    └─ occt-sys: static OCCT 7.8.1 build      │
└───────────────┬──────────────────────────────────────────────────────────────┘
┌───────────────▼──────────────────────────────────────────────────────────────┐
│ cad-core          document model (no geometry, no UI)                        │
│   document.rs     features, parameters, refs, dependencies, .c360 JSON       │
│   expr.rs         unit-aware expression engine (d1 / 2, 30 mm, …)            │
│   history.rs      snapshot undo / redo                                       │
│   ops.rs          feature-creation operations                                │
└───────────────┬──────────────────────────────────────────────────────────────┘
┌───────────────▼──────────────────────────────────────────────────────────────┐
│ cad-sketch        2D sketch engine (pure Rust + nalgebra)                    │
│   model.rs        entities & constraints                                     │
│   solver.rs       Gauss-Newton / Levenberg-Marquardt solver, SVD DOF,        │
│                   per-entity constrained state, over-constraint detection    │
│   profiles.rs     planar arrangement → closed profiles with holes            │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Hybrid kernel: OCCT (production) + `cad-geom` (own kernel) + `cad-diff` (differential tests)

The application's geometry is produced by OCCT through `cad-kernel`, and that integration is
preserved unchanged (OCCT 7.8.1, static, vendored `opencascade-sys`; see `docs/BASELINE.md`). Next
to it grows an independently implemented pure-Rust kernel whose long-term goal is observable
parity with Autodesk ShapeManager (ASM), measured rather than assumed:

```
            research/cases.json  (one case catalogue, mm)
          ┌──────────────┼──────────────────────────┐
          ▼              ▼                          ▼
   cad-geom (Rust)   cad-kernel → OCCT 7.8.1    ASM via Fusion API (probe v5, black-box)
          │              │                          │  research/evidence/asm-v5/reference.json
          └──────► cad-diff ◄────────────────────────┘
                   normalised KernelResult per case and kernel; comparator with explicit
                   tolerances → research/VALIDATION_REPORT.md, research/evidence/diff/results.json
```

* **A. OCCT-backed** (`cad-kernel`): every feature the application offers today. Nothing here is
  replaced until the Rust equivalent is `REFERENCE_VERIFIED` and the switch is authorised.
* **B. Own kernel** (`crates/cad-geom`): math and tolerance policy, analytic curves and surfaces,
  NURBS (Piegl & Tiller), projection and intersections, a half-edge B-rep in typed arenas
  (solid → shell → face → loop → coedge → edge → vertex), primitives, exact mass properties,
  validation (Euler–Poincaré, manifoldness, orientation, geometry-on-topology) and tessellation.
  It may not depend on OCCT, cxx or any other workspace crate (enforced by
  `crates/cad-core/tests/layering.rs`). The application does not use it yet.
* **C. Differential layer** (`crates/cad-diff`): runs each catalogue case through B and A, loads
  the recorded ASM results, and classifies each pair as MATCH, MISMATCH, TOPOLOGY_CONVENTION
  (same volume and area, different entity counts, e.g. seam or pole edges), ONE_SIDED_FAILURE,
  BOTH_FAILED_SAME or NOT_IMPLEMENTED. Entities are compared by invariants (counts, surface-type
  multisets, volume, area, centroid, inertia, bounding box), never by identifier or order.

Kernel operation status lives in `docs/FEATURE_MATRIX.md`; the order of work is in `docs/ROADMAP.md`;
the ASM evidence is in `research/`.

## Data flow

```
user input ──► app.rs ──► AppState method / execute(CommandId)
                              │  edits Document (inside a transaction for dialogs/sketches)
                              ▼
                       Kernel::recompute(&Document)            (incremental: cached per
                              │  for each feature in timeline     feature, keyed by all
                              │  order up to the marker:          upstream state)
                              │   sketch  → cad_sketch::solve + find_profiles
                              │   extrude/revolve → OCCT face → prism/revol → boolean
                              │   fillet/chamfer  → resolve EdgeRefs → BRepFilletAPI
                              │   hole / plane / import
                              ▼
                       Model { bodies (B-rep), health per feature, sketches, planes }
                              │
          ┌───────────────────┼──────────────────────────┐
          ▼                   ▼                          ▼
   viewport::scene      ui: browser / timeline     exchange: STEP / STL
   (meshes, edges,      (structure + health)
    overlays) → wgpu
```

* **Document** (`.c360`, JSON) stores only the parametric definition: ordered features, model and
  user parameters (expressions), sketches (entities + constraints + last solved positions),
  persistent references, timeline marker, body appearance/visibility. Geometry is always
  recomputed, so files stay small and the history is always editable.
* **Kernel**: Open CASCADE Technology 7.8.1 (B-rep), statically linked. Every OCCT call goes
  through `cad360.hxx`, which catches `Standard_Failure` (it cannot cross the cxx bridge) and
  turns failures into feature-health errors instead of crashes.
* **Sketch solver**: numerical, analytic residual equations per constraint, minimum-norm
  Gauss-Newton with LM fallback; Jacobian SVD gives degrees of freedom, per-entity
  "fully constrained" state (Fusion's black vs blue) and over-constraint detection (rank test).
* **Transactions**: dialogs and sketch sessions snapshot the document, preview live by editing it
  and recomputing, then commit one undo entry (OK / Finish Sketch) or restore (Cancel / Esc).
* **Undo/redo**: document snapshots; restoring a snapshot and recomputing is exact.
* **Rendering**: an offscreen MSAA colour + depth target rendered by CAD360's own wgpu pipelines
  (background gradient, lit meshes with per-face highlight colours, screen-space thick lines/points),
  resolved into a texture that egui shows. Picking is done on the CPU against the same meshes.
* **Commands**: one registry drives the ribbon, panel menus, marking menu, shortcuts and scripted
  scenarios; unsupported Fusion tools appear disabled with an explicit tooltip.
* **Components** (one level, Fusion "Hybrid Design"): `Document.components` lists the components
  (the root component is implicit, `None`); `Document.active_component` is where new features go.
  Every `Feature` records the component that was active when it was created, and a body belongs
  to the component of the feature that created it (`Model::component_of`). "New Component" with
  bodies selected moves them by reassigning their *creating features* (so every body of that
  feature, `{fid}` and `{fid}~k`, moves together); because the component lives in the feature
  JSON it is part of the recompute cache key. A component that still owns features cannot be
  deleted. Hiding a component hides its bodies (`AppState::visible_bodies`, also used by export).
* **Joints** (`FeatureKind::Joint`, `recompute_assembly.rs`): each side references a planar face
  (frame at the centroid, z = outward normal) or a circular edge (frame at the centre, z = axis,
  oriented like the adjacent planar face) through the usual `FaceRef`/`EdgeRef`. Joints are
  solved as a graph: accepted joints form a forest of components; each cluster is anchored at the
  root, a grounded component, or else its earliest-created component, and placements propagate
  outward from the anchor. The mate follows the Fusion probe (F35): joint origins coincide with
  their **Z axes aligned in the same direction** (`flip` reverses), then `angle` rotates and
  `offset` slides along the joint Z; the probe's two placements are reproduced exactly in
  `tests/assembly.rs`. A joint between components that are already connected (or two fixed ones)
  is refused with Fusion's message "A joint in system exists for the provided input. System will
  be over constrained with this input." (F37). Each component carries its placement, so bodies,
  sketches and origin planes created later in a moved component appear in the moved frame. Bodies
  keep id, kind and face names; `Shape::transformed` bakes placements into the B-rep, so STEP/STL
  export the assembled positions.

## Technology decisions (summary — details in `docs/kernel-evaluation.md`)
| Concern | Choice | Why |
|---|---|---|
| Language | Rust (100% of application code; C++ only in the 500-line OCCT shim) | requirement; memory safety; native performance |
| Geometry kernel | OCCT 7.8.1 via vendored `opencascade-sys` + own shim | mature B-rep: booleans, fillets, chamfers, revolve, STEP; Rust-native kernels lack fillets/robust booleans |
| Sketch solver | own numerical solver in `cad-sketch` | Rust-native, exposes DOF/redundancy analysis directly; `slvs` (SolveSpace) bindings are unmaintained (2023) and need a C++ build |
| UI | egui/eframe 0.37 | native immediate-mode UI; same stack as CADCraft; AccessKit for tests |
| Rendering | wgpu 30 (DX12/Vulkan), custom WGSL | portable GPU API; offscreen target decoupled from egui |
| Persistence | JSON `.c360` (serde) | human-readable, versioned, preserves full history |
| Exchange | STEP (exact B-rep), STL (mesh) | standard formats supported by OCCT; neither preserves history |
