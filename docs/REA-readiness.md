# REA readiness report

Date: 2026-10-16 · Host: Windows 11 Pro 22631, x64, Node 22.21.1 · Target: Autodesk Fusion 2705.1.30
(installed at `%LOCALAPPDATA%\Autodesk\webdeploy\production\736d2b4c…\Fusion360.exe`, signed-in Education licence).

## What REA is here
REA ("Reverse Engineer Anything", npm `rea-agents` 6.4.0) — an evidence-producing reverse-engineering
toolkit available as a CLI (`npx -y rea-agents@6.4.0`) and as an MCP server (`rea`, 170 tools).

## Verification performed
| Check | Result | Evidence |
|---|---|---|
| MCP server at session start | **Failed to connect** (30 s connect timeout) | session start notice |
| `rea doctor --client claude_code --json` | registration `stale`/`config_drift`; node healthy; Hopper, Ghidra, IDA **not installed** | `docs/evidence/rea/rea-doctor.json` |
| `rea setup --dry-run` (not applied) | would rewrite the `rea` entry in `~/.claude.json` with a backup | `docs/evidence/rea/rea-setup-dry-run.json` |
| MCP later connected (the user confirmed "rea is connected") | `binary_session`: 59 of 170 tools available | `docs/evidence/rea/rea-mcp-binary-session.json` |
| Unavailable reasons | `target_required` (59, need an opened binary), `provider_missing` (42: Android/JEB/firmware/ELF), `unsupported_host` (10: **native UI observation and capture are macOS-only**) | same |

## REA operations actually run, with Evidence IDs
| Operation | Target | Result | Evidence ID |
|---|---|---|---|
| `inspect-pe-resources` (CLI) | Fusion360.exe (sha256 `ddc460d8…a5a`, 901 696 bytes) | complete coverage; 20 resources: string tables, 2 icon groups, version, manifest — the exe is a small launcher, code lives in DLLs | `ev_8ae3da4c120cad7f25600eefc12c71a212fe9757e03931e9475a3670b2644dd0` |
| `inspect-artifact` (CLI) | Fusion360.exe | artifact graph / inventory | `ev_20cc9049c3242ba7007da6ab0da9b8009b6817cc063812782bb8584d4e7cca92` (inventory `ev_61425f5f…4376`) |
| `binary_session` (MCP) | — | capability/tool availability snapshot | `rea-mcp-binary-session.json` |
| `capture_browser_scenario` (MCP) | https://getartcraft.com/ (headless Chrome 155) | accessibility tree captured; browser cleanup reported incomplete (no REA processes remained) | partial observation, accessibility sha256 `c1051161…809b` |

## Limits for investigating Fusion 360 with REA on this host
* **No native UI observation**: `observe_native_ui` / `capture_native_ui_scenario` require macOS.
* **No decompiler**: Ghidra/Hopper/IDA are not installed; decompiling Autodesk binaries would also conflict
  with the Autodesk licence terms and the task's rule not to reproduce proprietary code, so it was not attempted.
* Fusion's UI is a native Qt/DirectX application (not a CDP-exposed browser/Electron page), so REA's browser
  and Electron tools do not apply to it.

## Investigation method chosen
1. **REA** for artifact identity and metadata (above) and for the ArtCraft website observation.
2. **Fusion's own public scripting API inside the real application** (the documented extension mechanism):
   a sentinel-gated, temporary add-in (`docs/evidence/fusion-probe/addin-src`) drove three scripted passes,
   logged measurements and states to JSON, and captured viewport + window screenshots. It never saved to the
   cloud, closed all documents without saving, and was **uninstalled afterwards** (removed from
   `%APPDATA%\Autodesk\Autodesk Fusion 360\API\AddIns\CAD360Probe`).
3. **Fusion's shipped API type definitions** (`API\Python\defs\adsk\*.py`, version-matched 2705.1.30).
4. **Official Autodesk help pages** (`docs/evidence/official-docs.md`).

Observations from (2)–(4) are labelled as *non-REA observations* throughout the reports; only the rows above are REA Evidence.

## Next step to repair the MCP registration (optional, not applied)
`npx -y rea-agents@6.4.0 setup --client claude_code --yes` then restart Claude Code (writes `~/.claude.json`, keeps a backup).
