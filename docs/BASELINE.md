# Baseline (before the pure-Rust kernel work)

Recorded 2026-10-17 (commands run 15:33–15:42 local time, UTC+7) on Windows 11 Pro 22631. This is
the reference point the `cad-geom` kernel and the `cad-diff` harness are measured against. The
working tree was shared with other workers during the run, so the `cad-geom` figures below are a
snapshot, not a stable result.

## Repository state

| Item | Value |
|---|---|
| HEAD | `6b79276f038205c1d518f2dc461b6ed8742df3ef` "Update docs" (2026-10-17 03:27 +0700). The commit `04c75bb` named in the task brief exists in no ref (`git log --all`); `6b79276` is the actual HEAD. |
| History | 9 commits (`0e9620f` Initial commit … `6b79276` Update docs) |
| Working tree (uncommitted) | `Cargo.toml` (adds the `crates/cad-geom` member; `crates/cad-diff` was added after the baseline run), `Cargo.lock` (+23 lines for cad-geom's dependencies), `crates/cad-core/tests/layering.rs` (+13 lines, a third layering test), `docs/architecture.md` renamed to `docs/ARCHITECTURE.md` (staged, +35 lines); untracked `crates/cad-geom/` (pure-Rust kernel, in progress) and `research/` |
| Workspace members at the baseline run | `cad-core`, `cad-sketch`, `cad-geom`, `cad-kernel`, `cad-app`, `spikes/occt-spike` |
| Toolchain | `rustc 1.95.0 (59807616e 2026-04-14)`, `cargo 1.95.0 (f2d3ce0bd 2026-03-21)`, MSVC (VS 2022, MSVC 14.44), CMake 3.31.8 (`C:\tools\cmake-3.31.8-windows-x86_64`; CMake 4 rejects OCCT's minimum version) |
| Target dir | `C:/c360t` (`.cargo/config.toml`, for MAX_PATH and the apostrophe/spaces in the repo path) |
| Profiles | `[profile.dev.package."*"] opt-level = 2` (dependencies, including the OCCT bindings, are optimised in dev builds) |

## Open CASCADE integration

* **Version**: OCCT **7.8.1**, built from source by the `occt-sys 7.8.1` crate (feature `builtin`
  of `opencascade-sys`) into `C:\c360t\OCCT` with CMake (`Release` configuration in every profile).
  `opencascade-sys/build.rs` requires major 7, minor ≥ 8.
* **Linking: static.** The generated `occ_info.txt` has `BUILD_SHARED_LIBS=` (empty), so `build.rs`
  emits `rustc-link-lib=static=` for the 24 toolkits (TKernel, TKMath, TKBRep, TKTopAlgo, TKPrim,
  TKBO, TKBool, TKFillet, TKOffset, TKFeat, TKMesh, TKShHealing, TKDESTEP, TKDEIGES, TKDESTL, TKDE,
  TKXSBase, TKCAF, TKLCAF, TKXCAF, TKG2d, TKG3d, TKGeomBase, TKGeomAlgo) plus `user32` and
  `advapi32`; the C++ glue is compiled by `cxx_build` into the static `rust-occt` library. The
  release exe is one 42.9 MB binary with no OCCT DLLs (`docs/build-and-release.md`).
* **Bindings**: `crates/vendor/opencascade-sys` (from crates.io 0.3.0, LGPL-2.1) and the high-level
  `crates/vendor/opencascade` 0.3.0, both substituted through `[patch.crates-io]` in the root
  `Cargo.toml`. `cad-kernel` depends on `opencascade-sys` only; the high-level `opencascade` crate
  is used by `spikes/occt-spike` and, since this baseline, by `crates/cad-diff` (for its
  `BRepPrimAPI` primitive builders).
* **cxx bridges**: 41 upstream bridge files (`b_rep*.rs`, `geom*.rs`, `gp.rs`, `top_*.rs`,
  `topo_ds.rs`, `step_control.rs`, `stl_api.rs`, …) plus four CAD360 bridges over the
  exception-safe shim `include/cad360.hxx` (1219 lines; every function catches
  `Standard_Failure`/`std::exception`/`...`, records `cad_last_error()` and returns null/false):

| Bridge | Exposes |
|---|---|
| `src/cad360.rs` | `cad_last_error`, `cad_quiet`; modelling: `cad_make_face` (planar face from line/arc loops), `cad_prism`, `cad_revol`, `cad_boolean` (fuse/cut/common), `cad_compound_new/add`, `cad_fillet`, `cad_chamfer`, `cad_cylinder` (BRepPrimAPI_MakeCylinder), `cad_shell`, `cad_transform` (affine / mirror); query: `cad_subshapes` (TopExp::MapShapes), `cad_edge_faces`, `cad_face_info` (GeomAbs type, area, centroid, normal, cylinder axis/radius), `cad_edge_info`, `cad_volume`, `cad_area`, `cad_bbox` (BRepBndLib::Add), `cad_is_valid` (BRepCheck_Analyzer), `cad_shape_type`, `cad_copy`; mesh: `cad_mesh`, `cad_face_triangles`, `cad_edge_polyline`; exchange: `cad_write_step`, `cad_read_step`, `cad_write_stl` |
| `src/cad360_naming.rs` | history-reporting variants for generative face names: `cad_prism_named`, `cad_revol_named`, `cad_boolean_named`, `cad_blend_named` (fillet/chamfer), `cad_shell_named`, `cad_transform_named`, `cad_subshape_indices` |
| `src/cad360_surface.rs` | surface/mesh modelling: `cad_make_wires`, `cad_children`, `cad_sew`, `cad_offset_surface`, `cad_thicken`, `cad_fill`, `cad_mesh_face`, `cad_mesh_to_brep` |
| `src/cad360_inspect.rs` | `cad_min_distance` (BRepExtrema), `cad_mass_props` (BRepGProp: volume, area, centroid; **no inertia**) |

* **Patches** (root `Cargo.toml` comment and [`kernel-evaluation.md`](kernel-evaluation.md#integration-work-required-for-occt-on-windowsmsvc-all-in-cratesvendoropencascade-sys-documented-in-tree)):
  (1) MSVC `Handle_X` subclass problem fixed by a bridge-only `msvc-override/Standard_Handle.hxx`
  that selects OCCT's portable typedef branch; (2) the static member function
  `BRepLib_ToolTriangulatedShape::ComputeNormals` wrapped in a free function (`include/b_rep_lib.hxx`);
  (3) `advapi32` added to the link line; (4) the exception-safe `cad360` shim above; (5) a C++
  argument-evaluation-order bug fixed in the shim (MSVC evaluates right to left; `cad_make_face`
  reads its input sequentially); (6) build hygiene: short target dir, CMake 3.x, `rerun-if-changed`
  on `include/` and `msvc-override/`.

## What relies on OCCT and what is pure Rust

| Relies on OCCT (through `cad-kernel` → `cad360` shim) | Pure Rust |
|---|---|
| Extrude (`cad_prism[_named]` + booleans for Join/Cut/Intersect), Revolve (`cad_revol[_named]`) — `recompute.rs` | Sketch constraint solver (Gauss-Newton/SVD + LM, DOF analysis) — `cad-sketch/solver.rs` |
| Booleans / Combine (`cad_boolean[_named]`) — `recompute.rs`, `recompute_more.rs` | Sketch profiles (closed regions, nesting, overlaps), interaction, modify tools (trim, extend, fillet, offset, mirror) — `cad-sketch` |
| Fillet, Chamfer (`cad_blend_named`; BRepFilletAPI) | Document model, features, dependencies, parameters, unit-aware expressions — `cad-core/document.rs`, `expr.rs`, `ops.rs` |
| Shell (`cad_shell_named`; BRepOffsetAPI_MakeThickSolid) | Snapshot undo/redo — `cad-core/history.rs` |
| Hole (`Shape::cylinder` tool + cut) — `recompute.rs` | `.c360` serialisation (serde JSON, versioned) — `cad-core/document.rs` |
| Rectangular/circular patterns, Mirror, Move/Copy (`cad_transform[_named]` + fuse) — `recompute_more.rs` | Sketch-plane frames and joint mathematics (frames, mate transforms, graph solve) — `cad-kernel/frame.rs`, `recompute_assembly.rs` (applies results with `cad_transform`) |
| Surface Extrude/Revolve, Patch, Stitch, Offset surface, Thicken, mesh → B-rep (`cad360_surface`) — `surface.rs`, `recompute_surface.rs` | Picking (CPU ray tests against the OCCT-produced display meshes) — `cad-app/viewport/picking.rs` |
| Display meshes and edge polylines (`cad_mesh`, `cad_face_triangles`, `cad_edge_polyline`) — `body.rs` | Camera, ViewCube, navigation — `cad-app/viewport/camera.rs`, `ui/viewcube.rs` |
| STEP export/import, STL export (`cad_write_step`, `cad_read_step`, `cad_write_stl`) — `exchange.rs`, `mesh.rs`; STL *reading* is pure Rust (`mesh.rs`) | Rendering (wgpu 30, own WGSL pipelines, section caps) — `cad-app/viewport/renderer.rs`, `scene.rs` |
| Mass properties, minimum distance, interference (`cad_mass_props`, `cad_min_distance`, `cad_boolean`) — `inspect.rs` | UI (egui), command registry, scenarios — `cad-app` |
| Topology queries and generative naming (`cad_subshapes`, `cad_edge_faces`, `cad_face_info`, `cad_edge_info`, `*_named`, `cad_subshape_indices`) — `body.rs`, `naming.rs`, `refs.rs` (reference resolution logic itself is Rust) | `cad-geom` (new, in progress): curves, surfaces, NURBS, topology, primitives, mass properties, validation, tessellation — no OCCT |

## Command results

### (a) `cargo fmt --all --check` — fails (finding, not reformatted)
Exit 1; **98 files** would be reformatted: `cad-app` 50, `cad-kernel` 20, `cad-sketch` 7,
`cad-core` 5, vendored `opencascade-sys` 7 (incl. `build.rs`), `spikes/occt-spike` 1, and `cad-geom`
8 (in progress). The repository has evidently never been formatted with the default rustfmt
settings (no `rustfmt.toml`; long single-line expressions throughout).

### (b) `cargo clippy --workspace --all-targets` — 0 errors, 100 unique warnings (exit 0, 400 s)
| Crate | Warnings (target) |
|---|---|
| `cad-app` | 75 (bin `cad360`, incl. 1 duplicate; the test target repeats 70 of them) |
| `cad-sketch` | 8 (lib) + 1 (lib test) |
| `cad-kernel` | 7 (lib) + 1 (`tests/surface.rs`) |
| `cad-core` | 5 (lib) + 1 (`tests/layering.rs`) |
| `opencascade-sys` (vendored) | 3 (`too_many_arguments` in the cad360 bridges) |
| `cad-geom`, `occt-spike` | 0 at the baseline run (cad-geom later showed 2 while being edited) |

Top lint kinds (unique locations): `collapsible_if` / collapsible `else if` 65, `nonminimal_bool` 4,
dead code (never used/read/constructed) 10, `type_complexity` 3, `too_many_arguments` 3,
`enum_variant_names` 2, then single instances (`needless_range_loop`, `manual_range_contains`,
`manual_clamp`, `large_enum_variant`, `items_after_test_module`, `redundant_closure`,
`duplicated_attributes`, a deprecated egui `Id` API, an unused import, …).

### (c) `cargo build --workspace` — success (exit 0, 71 s incremental, OCCT already built)
Only warnings: `cad-app` (bin `cad360`) 12 rustc warnings: 10 dead code (unused methods, fields,
enum variants), 1 unused import (`SketchTool`), 1 deprecated `egui::Id::new` (`app.rs:391`).

### (d) `cargo test --workspace --no-fail-fast -- --test-threads=4` — **270 passed, 0 failed, 0 ignored** (exit 0, 79 s)
| Suite | Passed |
|---|---|
| `cad-app` (bin unit tests) | 147 |
| `cad-core` unit | 15 |
| `cad-core/tests/layering.rs` | 3 (was 2 in `docs/test-results.md`; the uncommitted third test covers the new crate layering) |
| `cad-kernel` unit | 6 |
| `cad-kernel/tests`: assembly 13, fusion_workflow 5, inspect 6, more_features 10, naming 8, surface 12 | 54 |
| `cad-sketch` unit | 44 |
| vendored `opencascade-sys/tests/triangulation.rs` | 1 |
| `cad-geom` | 0 (no tests yet at the snapshot; doc-tests 0) |
| `occt-spike`, doc-tests (cad_core, cad_geom, cad_kernel, cad_sketch, opencascade_sys) | 0 |

The pre-existing suites therefore reproduce the documented 269 exactly (269 + the one new layering
test = 270). No cad-geom test existed at the time, so no cad-geom failure is mixed into these numbers.

### (e) Running the application (documented, not launched)
```sh
export PATH="/c/tools/cmake-3.31.8-windows-x86_64/bin:$PATH"
cargo run -p cad-app --release                 # -> C:\c360t\release\cad360.exe
cad360 [design.c360] [--scenario steps.json [--screenshot-dir DIR]]
C:\c360t\release\cad360.exe --scenario docs\scenarios\workflow.json --screenshot-dir docs\screenshots\cad360
```
Scenarios: `docs/scenarios/{workflow,modify,assembly,viewport}.json`; usage is in
`crates/cad-app/src/main.rs` and `scenario.rs`, packaging and smoke tests in
[`build-and-release.md`](build-and-release.md). A first release build takes about 11 min with
OCCT already built; a fresh target dir pays the full OCCT compile (30–60 min, ~6 GB).

## Documented defects and limitations (summary; see the linked documents)

* **Persistent naming is generative, not full topological naming**
  ([kernel-evaluation.md § Persistent naming](kernel-evaluation.md#persistent-naming),
  [parity-matrix.md](parity-matrix.md)): projected face-sketch edges use index ids; corner blends and
  multi-profile caps share names; split faces are told apart geometrically; imported/surface/mesh
  faces have ordinal or no names.
* **Not implemented** ([parity-matrix.md](parity-matrix.md)): extrude Two Sides / To Object / Taper /
  Thin / Start offset; Sweep, Loft, Rib, Web, Draft, Press Pull, Thread, Split, Scale, primitives;
  splines, Project, Text, Ellipse in sketches; window/crossing selection and selection filters;
  feature (as opposed to body) patterns; counterbore/countersink/threaded holes; IGES/3MF/OBJ/F3D;
  most SURFACE/MESH/UTILITIES commands; joint limits, as-built joints, nested components; physical
  materials. Sheet metal, plastic, T-splines, drawings, CAM, simulation, cloud are out of scope.
* **Partial**: marking menu without gesture flicks; joints (Flip+angle, slider direction and moved
  components partly designed independently); Shell manipulator unverified.
* **Deliberate difference**: an oversized fillet raises an error where Fusion produced odd geometry.
* **Testing limitations** ([test-results.md § Known limitations](test-results.md#known-limitations-of-the-testing)):
  UI tests use synthetic egui input headlessly; GPU output is checked only by scenario screenshots;
  Fusion comparisons rely on one probe workflow's numbers; parallel tests needed OCCT messenger and
  exchange serialisation (earlier `STATUS_HEAP_CORRUPTION`).
* **Release** ([build-and-release.md](build-and-release.md)): MSI not built (WiX absent); exe not
  code-signed; licence file is a placeholder; static LGPL OCCT requires a relinking offer; the VC++
  runtime is app-local (a static CRT would need OCCT rebuilt with `/MT`).
* **Harness-relevant OCCT gaps** (found while building `crates/cad-diff`): cad-kernel exposes no
  inertia tensor (`cad_mass_props` returns volume, area, centroid only), no primitive other than the
  cylinder, and no constructor from a raw `TopoDS_Shape`; `cad_bbox` uses `BRepBndLib::Add`
  (tolerance-enlarged, not tight on curved faces). See `research/VALIDATION_REPORT.md`.
