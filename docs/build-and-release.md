# Building and releasing CAD360 (Windows)

How to build the release executable and package it as a standalone distribution, with the
timings and sizes measured on the reference machine (Intel Core i5-1135G7, 16 GB RAM, Windows 11 23H2 build 22631, SSD).

## Prerequisites
| Tool | Version used | Notes |
|---|---|---|
| Rust (MSVC toolchain) | 1.95.0 | `rustup default stable-x86_64-pc-windows-msvc` |
| Visual Studio 2022 | 17.x, MSVC 14.44 | "Desktop development with C++" workload; provides `cl.exe`, `rc.exe` (Windows SDK, used to embed the icon), `dumpbin.exe` and the VC++ runtime redistributables |
| CMake | 3.31.8 | must be 3.x (CMake 4 rejects OCCT's minimum version). Portable copy in `C:\tools\cmake-3.31.8-windows-x86_64` |
| PowerShell | 7.x (`pwsh`) | packaging scripts; Windows PowerShell 5.1 also works |
| WiX Toolset | optional, v5+ | only for the `.msi`; `dotnet tool install --global wix` |

The cargo target directory is `C:\c360t` (`.cargo/config.toml`): the repository path contains spaces
and an apostrophe, and OCCT's CMake tree is too deep for `MAX_PATH` under it.

## 1. Release build
```powershell
$env:PATH = "C:\tools\cmake-3.31.8-windows-x86_64\bin;$env:PATH"
cargo build --release -p cad-app          # -> C:\c360t\release\cad360.exe
```
* **OCCT is not rebuilt for the release profile.** `occt-sys` installs OCCT into `C:\c360t\OCCT`
  (`OUT_DIR/../../../../OCCT`, shared by all profiles) and its CMake configuration is `Release` in
  both profiles, so a release build only re-runs CMake's up-to-date check. Only a fresh target
  directory pays the full OCCT 7.8.1 compile (30-60 min, about 6 GB).
* Measured: **11 min 10 s** for the first release build with OCCT already built (all Rust crates
  compiled in release, LTO off), **27 s** for an incremental rebuild after a change in `cad-app`.
* The exe links OCCT statically: one 42.9 MB `cad360.exe`, no OCCT DLLs.

## 2. Application icon
The icon is CAD360's own artwork (an isometric cube in an orange orbit ring on a dark tile),
drawn from vectors at every size by `packaging/icon/make-icon.ps1`:
```powershell
pwsh -File packaging\icon\make-icon.ps1   # -> crates/cad-app/assets/cad360.ico (16-256 px) + cad360-256.png
```
* `crates/cad-app/build.rs` embeds `cad360.ico` plus version information (product name, file
  description, version from `Cargo.toml`) into `cad360.exe` with
  [`winresource`](https://crates.io/crates/winresource) `=0.1.34` (build-dependency, Windows only;
  a missing resource compiler only produces a build warning).
* `crates/cad-app/src/branding.rs` exposes `app_icon() -> egui::IconData` (decoded from
  `cad360-256.png`) for `ViewportBuilder::with_icon`, the window and taskbar icon.

## 3. Distribution package
```powershell
pwsh -File packaging\windows\make-dist.ps1             # build + package
pwsh -File packaging\windows\make-dist.ps1 -SkipBuild  # package the existing release exe
```
Output in `dist\`:
```
CAD360-<version>-win64.zip
CAD360-<version>-win64\          (the unzipped folder, kept for inspection)
CAD360-<version>-win64-sample.png  screenshot of the sample design
SHA256SUMS.txt                   checksum of the zip (and the msi, when built)
```
Contents of the zip:
```
CAD360-<version>-win64\
  cad360.exe
  README.txt                     getting started, requirements, command line
  LICENSE.txt                    CAD360 licence (placeholder, see below)
  samples\mounting-plate.c360    sketch + extrude + fillets + slot cut, made by the packaged exe
  THIRD-PARTY-NOTICES\
    OpenCASCADE-NOTICE.txt       OCCT notice, relinking offer, opencascade-rs notice
    OpenCASCADE-LGPL-2.1.txt     from C:\c360t\OCCT\LICENSE_LGPL_21.txt
    OpenCASCADE-LGPL-EXCEPTION.txt  from C:\c360t\OCCT\OCCT_LGPL_EXCEPTION.txt
    rust-crates.txt              every Rust crate compiled in, with its declared licence
  SHA256SUMS.txt                 checksums of every file above
  MSVCP140.dll, VCRUNTIME140.dll, VCRUNTIME140_1.dll   Visual C++ runtime, app-local (see below)
```
What the script does:
1. builds the release exe (unless `-SkipBuild`);
2. lists the exe's DLL imports with `dumpbin /dependents`; anything that is not a Windows system
   DLL (the Visual C++ runtime) is copied app-local from Visual Studio's `VC\Redist` folder;
3. writes the README / licence files (version, OCCT version and source URL filled in) and the
   crate list (`cargo tree -e normal --target x86_64-pc-windows-msvc`);
4. runs the **staged** exe with `packaging/windows/sample-design.json` to create
   `samples/mounting-plate.c360` (so the sample always matches the shipped file format);
5. writes `SHA256SUMS.txt`, zips the folder, and builds the MSI when `wix` is available.

### MSI (optional)
`packaging/windows/cad360.wxs` describes a per-machine install to `Program Files\CAD360` with a
Start-menu shortcut and the `.c360` file association (WiX v5 schema, uses `<Files>`). WiX was not
installed on the reference machine, so the MSI has **not been built or tested yet**. To enable it:
```powershell
dotnet tool install --global wix
pwsh -File packaging\windows\make-dist.ps1 -SkipBuild   # now also writes dist\CAD360-<version>-win64.msi
```

## 4. Smoke test of the package
```powershell
pwsh -File packaging\windows\smoke-test.ps1 [-Zip dist\CAD360-<version>-win64.zip] [-Keep]
```
Extracts the zip into a fresh temp folder and checks: every file against `SHA256SUMS.txt`; that the
exe imports only Windows system DLLs or DLLs shipped next to it (`dumpbin /dependents`); and runs
`packaging/windows/smoke.json` (sketch a rectangle, extrude 15 mm, export STEP, open the bundled
sample, three screenshots, close), requiring exit code 0, the screenshots, a STEP file containing
a solid and a clean log.

## Measured results (2026-10-16, CAD360 0.1.0)
| Step | Result |
|---|---|
| `cargo build --release -p cad-app` (OCCT already built) | 11 min 10 s, exit 0 |
| incremental release rebuild (icon resource added) | 27 s |
| `make-dist.ps1 -SkipBuild` (stage, sample design, checksums, zip) | 12 s |
| `cad360.exe` | 44,980,224 bytes (42.9 MB); embedded icon + version info (ProductName CAD360, 0.1.0) |
| unzipped folder | 45,764,334 bytes, 12 files |
| `CAD360-0.1.0-win64.zip` | 16,894,116 bytes (16.1 MB), SHA-256 `2c11e808...cbbc53c64` |
| `samples/mounting-plate.c360` | 9,320 bytes |
| rust-crates.txt | 159 third-party crates (name, version, declared licence) |
| MSI | skipped: WiX not installed |
| `smoke-test.ps1` | **passed**, 10/10 checks; scenario ran in 8 s from a fresh temp folder |

DLL imports of `cad360.exe` (`dumpbin /dependents`): Windows system DLLs (kernel32, user32,
gdi32, advapi32, shell32, ole32, oleaut32, combase, dwmapi, uxtheme, imm32, ws2_32, setupapi,
opengl32, dxgi, uiautomationcore, bcryptprimitives, ntdll), the Universal CRT (`api-ms-win-crt-*`,
part of Windows 10/11) and the Visual C++ runtime: `MSVCP140.dll` (OCCT is C++), `VCRUNTIME140.dll`,
`VCRUNTIME140_1.dll`. Those three are copied next to the exe from
`VC\Redist\MSVC\14.44.35112\x64\Microsoft.VC143.CRT`, so the zip runs on a clean Windows
without the VC++ Redistributable. (A fully static CRT would need OCCT rebuilt with `/MT`.)

## Licensing checklist before a public release
* `LICENSE.txt` is a **placeholder**: the workspace declares `LGPL-2.1-only`; the project owner must
  confirm the licence and copyright holder.
* OCCT 7.8.1 is linked **statically**. LGPL-2.1 §6 then requires that users can relink against a
  modified OCCT: the notice points to the source repository (`-SourceUrl`, default
  `https://github.com/Laakdal/Synthesis-360`), which must therefore be public (or the object files
  must be offered) when the binary is distributed.
* The Visual C++ runtime DLLs, when bundled, are redistributed app-local under the Visual Studio
  licence's redistributable-code terms.
* `rust-crates.txt` lists each crate's licence but not the licence texts. MIT/BSD-style licences
  ask for the copyright and permission notice to accompany binaries, so generate the full texts
  before a public release, e.g. `cargo install cargo-about` and `cargo about generate`.
* `dist/` is a build output; add `/dist/` to `.gitignore`.
* The executable is not code-signed; SmartScreen will warn on first launch. Signing needs a
  certificate (`signtool sign /fd sha256 /tr <timestamp-url> /td sha256 cad360.exe`).
