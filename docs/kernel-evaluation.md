# CAD kernel and solver evaluation

Evaluated 2026-10-16 on Windows 11 / MSVC 17.14 / Rust 1.95.

## Geometry kernel candidates
| Candidate | Version (crates.io) | Capabilities relevant to Fusion parity | Verdict |
|---|---|---|---|
| **OCCT via `opencascade-sys` / `opencascade`** | 0.3.0 (Aug 2026), `occt-sys` 7.8.1 static build | Full B-rep: prism, revol, booleans, constant/variable fillets, chamfers, shelling, offsets, STEP/IGES/STL, meshing, mass properties | **Selected** |
| `truck` (truck-modeling/shapeops/stepio) | 0.6 / 0.4 / 0.3 (Sep 2024) | Pure Rust B-rep, sweeps; booleans less robust; no general fillet/chamfer; partial STEP | Rejected: missing fillet/chamfer, inactive since 2024 |
| Fornjot | not on crates.io as a library | Early-stage code-CAD kernel | Rejected: not ready |
| replicad/opencascade.js (WASM) | used only in a throw-away feasibility spike | OCCT in WASM | Rejected after the Rust requirement (would make Rust a thin wrapper around JS) |

### Integration work required for OCCT on Windows/MSVC (all in `crates/vendor/opencascade-sys`, documented in-tree)
1. **Handle classes under MSVC** — OCCT defines `Handle_X` as a *subclass* of `opencascade::handle<X>`
   when `_MSC_VER >= 1800`, so the crate's `new opencascade::handle<X>(…)` → `unique_ptr<Handle_X>`
   conversions failed. Fix: a bridge-only override of `Standard_Handle.hxx` (`msvc-override/`) that
   selects OCCT's portable typedef branch; installed OCCT headers and libraries are untouched.
2. A static-member function pointer (`BRepLib_ToolTriangulatedShape::ComputeNormals`) wrapped in a free function.
3. Missing system library: `advapi32` (OCCT's OSD layer).
4. **Exception safety** — the bindings call `Shape()` without `IsDone()` checks and OCCT exceptions do
   not derive from `std::exception`, so a failed fillet would abort the process. CAD360 adds
   `include/cad360.hxx` + `src/cad360.rs`: every operation (`cad_make_face`, `cad_prism`, `cad_revol`,
   `cad_boolean`, `cad_fillet`, `cad_chamfer`, `cad_cylinder`, meshing, face/edge descriptors,
   adjacency, volume, bbox, validity, STEP/STL I/O) runs in `try/catch(Standard_Failure&)/catch(...)`
   and returns null + `cad_last_error()`.
5. A C++ argument-evaluation-order bug found and fixed in the shim (MSVC evaluates right-to-left).
6. Build hygiene: short `CARGO_TARGET_DIR` (`C:/c360t`) for MAX_PATH; portable CMake 3.31.8 in
   `C:\tools` (CMake 4 drops compatibility with OCCT's minimum version); the build script now
   re-runs when shim headers change.

### Verification
`crates/cad-kernel/tests/fusion_workflow.rs` replays the Fusion probe workflow and matches the
volumes Fusion reported (15000 → 30000 → 36000 → 35570.8 → 41570.8 → 39308.8 mm³), the "No target
body! Compute Failed" warning, rollback and suppression; it also covers revolve, chamfer, hole,
offset plane, STEP export → re-import and STL export.

## Persistent naming
OCCT's plain modelling API has no topological naming service comparable to Fusion's kernel, so
CAD360 builds **generative names from OCCT's modelling history** (`crates/cad-kernel/src/naming.rs`,
shim bridge `cad360_naming.rs`):
* Extrude/Revolve: lateral faces `{feature}/side:{sketch curve}#{k}` (k ordered by 2D midpoint, so it
  is independent of loop start), caps `{feature}/start|end`; profile edges are matched back to sketch
  segments by endpoints + midpoint because ShapeFix may reorder them.
* Booleans, fillet/chamfer, shell, transforms propagate names through `Modified/Generated/IsDeleted`;
  new faces get `{f}/blend:{a|b}`, `{f}/corner`, `{f}/inner:{name}`, `{f}/hole:wall|floor|top`;
  pattern/mirror/copy instances `{f}/i{k}:{name}`.
* An edge is identified by the sorted pair of its adjacent face names.
Resolution (`refs.rs`) restricts candidates to the same name (edges: same pair) and uses the old
geometric descriptor only to choose among same-name splits; a named reference whose name has
disappeared falls back to geometry and the feature shows a warning. References from older documents
(no names) still resolve geometrically. `tests/naming.rs` contains four scenarios that failed with
the purely geometric scheme (bbox-changing upstream cut before a fillet, an upstream boss before a
hole, a 50→300 mm width change, a chamfer after an upstream mirror) and now pass.
Remaining weak spots (not full topological naming): projected face-sketch edges use index-based ids,
corner blends and multi-profile caps share names, split faces are told apart geometrically,
imported/surface/mesh faces have ordinal or no names.

## Sketch constraint solver candidates
| Candidate | Status | Verdict |
|---|---|---|
| `slvs` (SolveSpace solver bindings) | 0.6.0, last release 2023-06, builds C++ libslvs | Rejected: unmaintained, extra C++ build |
| FreeCAD PlaneGCS (C++, Eigen) | only available as C++/WASM | Rejected for the Rust build (was validated in the throw-away WASM spike) |
| **Own solver (`cad-sketch`, nalgebra)** | implemented | **Selected**: residual equations per constraint, minimum-norm Gauss-Newton steps via SVD with Levenberg-Marquardt fallback; DOF = n − rank(J); per-unknown determinacy from the SVD nullspace; over-constraint = no rank increase. Tests reproduce Fusion's fully-constrained and over-constrained behaviour. |
