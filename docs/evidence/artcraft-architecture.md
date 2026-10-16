# ArtCraft / CADCraft architecture investigation

Date: 2026-10-16. Methods: (1) REA `capture_browser_scenario` (headless Chrome 155, accessibility capture)
of https://getartcraft.com/ — partial observation, accessibility sha256
`c10511614ab0baab162fc68976861d10273959b548f51ac10067bca80d8a809b` (browser cleanup reported incomplete;
no REA-owned processes remained afterwards); (2) read-only shallow clones of
`github.com/storytold/artcraft` (HEAD d4e4ac6, 2026-10-11) and `github.com/storytold/cadcraft` (2026-10-15),
inspected without building or running anything.

## What the site states (REA observation)
"ArtCraft builds open source tools for artists." · "macOS · Windows · Web" · "No subscription required" ·
links to "Open source on GitHub" (storytold/artcraft) and a family of "Craft Apps" including **CADCraft**
(`/apps/cadcraft`).

## ArtCraft (the main app) — findings from source
| Topic | Finding | Evidence |
|---|---|---|
| Shell | **Tauri 2** (system webview) | root `Cargo.toml`: `tauri = { version = "2.11.2", ... }`; `AGENTS.md`: "Artcraft is a public-facing Tauri desktop app" |
| UI | React 18 + Vite + Nx + Zustand + Tailwind in the webview | `tauri.conf.json` `"frontendDist": "../../../frontend/apps/artcraft/dist"`; `frontend/package.json` `"react": "18.3.1"` |
| 3D | three.js `^0.171.0` (TypeScript), Spark (splats), ammo.js | `frontend/libs/components/pagescene/src/lib/engine/` |
| Languages | Rust ≈272k lines (≈195k are remote API clients); TS/TSX ≈207k lines (all UI) | file counts in clone |
| Core/UI split | API crates are Tauri-free and reused by a second front end | `artcraft-native` manifest: "Internal (Tauri-free) API crates shared with the Tauri app." |
| Native port | `artcraft-native`: egui 0.36 / eframe (glow), partial, not in CI | its README: "The other tools show a 'coming soon' page" |
| Local vs cloud | Generation is remote (`api.storyteller.ai`); local SQLite task queue + caches | `artcraft_client/src/utils/api_host.rs`; `ROADMAP.md`: "Remove dependence on ArtCraft hosted services." |
| Packaging | `tauri-action` → MSI/NSIS, universal signed+notarised DMG | `.github/workflows/artcraft-{windows,macos}-publish.yml` |
| License | MIT OR Apache-2.0 (trademarks reserved) | `LICENSE*`, `NOTICE` |

**Conflict:** ArtCraft's shipping desktop app is exactly the "web UI wrapped in a desktop window" the CAD360
requirements forbid. Its shell, UI and rendering choices are therefore *not* adopted.

## CADCraft (sibling "Craft App") — the relevant reference
"Computer-aided design and drafting, reimagined in pure Rust" (2D drafting; DXF/DWG; no solids/B-rep).
| Principle | CADCraft evidence | Adopted in CAD360 as |
|---|---|---|
| Enforced layering | `docs/architecture.md`: L0 geom/dxf → … → L5 ui-egui; "`cargo xtask layers` enforces the layering: nothing below L5 depends on egui, eframe, winit, wgpu or rfd." | `cad-sketch` → `cad-core` → `cad-kernel` → `cad-app`; a layering test asserts no lower crate depends on egui/eframe/wgpu/rfd |
| Swappable UI crate, thin app binary | "The UI crate is swappable." | UI only in `cad-app` |
| Display list built per change | "`render::build` turns a space into a display list once per change; the UI uploads it to wgpu" | kernel produces a `SceneMesh` (triangles+edges with face/edge ids) per recompute; viewport uploads it |
| Everything is a command | "every action is a `CommandSpec`… menus, toolbar, scripts, CLI… reach the same commands" | single command registry used by ribbon, marking menu, shortcuts and tests |
| Never crash | `unsafe_code = "forbid"`, no unwrap/panic, `catch_unwind` around execution | kernel calls wrapped in `catch_unwind`; errors surface as feature health, not crashes |
| Native packaging | WiX MSI, notarised DMG, deb/rpm/AppImage | release build of a single native `.exe` (installer packaging documented as remaining work) |
| Local-first | all CAD work offline | no network use for core functionality |
| Stack | egui/eframe 0.36 + egui-wgpu ("DirectX 12 by default on Windows") | egui/eframe 0.37 + wgpu |
