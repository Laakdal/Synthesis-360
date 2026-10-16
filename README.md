# CAD360

A native, Fusion 360-inspired parametric CAD application written in **Rust**: sketches with a real
geometric/dimensional constraint solver, B-rep solid modelling on **Open CASCADE Technology**,
an editable feature timeline with recompute, and a Fusion-style desktop interface built with
**egui + wgpu**. Local-first, no browser, no cloud service.

> CAD360 is an independent implementation. It is not affiliated with Autodesk, contains no Autodesk
> code or assets, and does not claim parity with Fusion 360. See `docs/parity-matrix.md` for what is
> implemented and verified.

## Repository layout
| Path | Contents |
|---|---|
| `crates/cad-sketch` | 2D sketch model, constraint solver (DOF / over-constraint analysis), profile detection |
| `crates/cad-core` | document model, unit-aware parameter expressions, dependencies, undo/redo, `.c360` persistence |
| `crates/cad-kernel` | feature recompute on OCCT, persistent references, meshes, STEP/STL |
| `crates/cad-app` | the `cad360` desktop application (UI, viewport, commands, scenarios) |
| `crates/vendor/opencascade-sys` | vendored OCCT bindings (LGPL-2.1) with MSVC fixes and the exception-safe `cad360` shim |
| `docs/` | investigation reports, evidence, architecture, evaluations, test results, parity matrix |

## Build requirements (Windows, as verified)
* Rust 1.95+ (MSVC toolchain), Visual Studio 2022 with the C++ workload.
* CMake **3.x** (3.31.8 used; CMake 4 drops compatibility with OCCT's minimum version). A portable copy
  can live in `C:\tools\cmake-3.31.8-windows-x86_64` — keep its `bin` on `PATH` for any build that
  (re)builds `opencascade-sys`: the first build, and every build after the shim headers in
  `crates/vendor/opencascade-sys/include` change.
* ~6 GB free disk for the OCCT build. The first build compiles OCCT 7.8.1 statically (30–60 min);
  later builds reuse it.
* The target directory is `C:/c360t` (`.cargo/config.toml`) because the project path contains
  spaces/an apostrophe and OCCT's build tree is deep.

## Build and run
```powershell
$env:PATH = "C:\tools\cmake-3.31.8-windows-x86_64\bin;$env:PATH"   # needed whenever opencascade-sys rebuilds
cargo build --release -p cad-app
C:\c360t\release\cad360.exe                 # start with an empty design (release build verified by the workflow scenario)
C:\c360t\release\cad360.exe my_part.c360    # open a design
```

Packaging a standalone Windows zip (exe + VC runtime + licence notices + sample): see `docs/build-and-release.md`.

## Tests
```powershell
cargo test --workspace                     # unit + integration tests (headless, no GPU needed)
cargo test -p cad-kernel --test fusion_workflow   # geometry checked against Fusion's own numbers
```
Scripted UI scenarios (real window, screenshots):
```powershell
C:\c360t\release\cad360.exe --scenario docs\scenarios\workflow.json --screenshot-dir docs\screenshots\cad360
```

## Using CAD360 (Fusion-style)
* **Navigate**: middle-drag pan · Shift+middle-drag orbit · wheel zoom (at cursor) · F6 fit · ViewCube faces/corners/home ·
  nav-bar Orbit/Pan/Zoom modes for left-drag (useful on touchpads/remote desktops).
* **Sketch**: Create Sketch → pick an origin plane or planar face → draw (L line, R rectangle, C circle, arc, point) →
  D dimension (type an expression such as `d1 / 2` or `30 mm`) → constraints from the SKETCH tab →
  Finish Sketch. Under-constrained geometry is blue, fully constrained geometry is white (Fusion's
  black on its light canvas); fully constrained sketches show a lock in the browser.
  Advanced sketch tools: T Trim, Extend, Sketch Fillet, O Offset, Mirror, Polygon, Slot, 2-Point
  Circle, Center Point Arc, 3-Point Rectangle.
* **Model**: select profiles → E Extrude (distance, direction, extent All, Join/Cut/Intersect/New Body) ·
  Revolve · H Hole · F Fillet / Chamfer on selected edges · Shell · Combine · Mirror · Rectangular /
  Circular Pattern · Move/Copy · Offset Plane · Change Parameters · Appearance · I Measure ·
  Section Analysis (display only).
* **Assemble**: New Component (activates it; new features go into the active component) · J Joint
  (Rigid / Revolute / Slider between planar faces or circular edges, with parametric angle/offset).
* **History**: every feature is in the timeline; double-click to edit, drag the marker to roll back,
  right-click to suppress/delete/rename. Edits recompute everything downstream; failures are
  shown as red/yellow timeline items with messages.
* **Files**: `.c360` keeps the full parametric history. STEP (exact solids) and STL (mesh) exports keep
  geometry only. STEP import creates a non-parametric base feature.
