# GachaTracker Desktop

Offline-first local desktop tracker for:
- Wuthering Waves
- Honkai: Star Rail
- Zenless Zone Zero
- Endfield

The app scans local logs/cache files, runs a full local import orchestration pipeline (discover → extract → parse/normalize → persist → recompute diagnostics), and stores results in a local SQLite database.

## Runtime network policy

- Default mode is **strict local-only** (all online calls blocked).
- Official API exceptions require explicit opt-in:
  1. disable strict offline mode,
  2. enable official API exceptions.
- Even in exception mode, only officially confirmed API hosts are eligible.
- Current confirmed-host set is intentionally empty until official/public API support is confirmed.
- See decision record: `docs\official-api-exceptions-decision.md`.

## Settings and diagnostics UX

- Root path is optional. When left empty, the app performs **automatic install discovery** across available local drives (plus known LocalLow log locations).
- Per-game **path overrides** can still be set to force a specific install path.
- Optional **manual fallback file paths** support recovery scans when auto-discovery misses logs/cache files.
- The UI now surfaces **data completeness** summaries and per-game **troubleshooting details** (checked path hints, fallback status, and next-step guidance).

## Endfield local-source research (current sample)

- Sample inspected: `..\Endfield\Player.log` (Unity startup/runtime log only).
- Adapter behavior:
  - scans `Player.log` / `Player-prev.log` from known install and LocalLow locations,
  - infers cache probe paths from Unity subsystem lines,
  - reports `no-history` with `partial` completeness when local artifacts are found but no confirmed pull-history token exists yet.
- Reliable local artifacts confirmed:
  - `Endfield\Player.log` (and likely rotated `Endfield\Player-prev.log` when present).
  - `Endfield_Data\UnitySubsystems` path line can reveal the install root, which can be used to probe `webCaches\...\Cache\Cache_Data\data_2`.
- Pull-history indicators found in sample: **none** (no gacha/history URLs, auth tokens, cookies, or API hosts).
- Confidence for direct pull-history extraction from current sample: **low**.
- Next data needed to improve confidence:
  1. Logs captured immediately after opening in-game recruitment history.
  2. Any `webCaches\*\Cache\Cache_Data\data_2` files from the same play session.
  3. Additional rotated logs (`Player-prev.log`) from sessions that include pull-history UI usage.

## Development

1. Install dependencies:
   - Node.js + npm
   - Rust toolchain
2. Install project packages:
   - `npm install`
3. Run desktop app:
   - `npm run tauri dev`

## Build executable

- Build Windows installer artifacts (MSI + setup EXE):
  - `npm run release:windows`
- Build portable ZIP from the latest release binary:
  - `npm run release:windows:portable`
- Build both installer and portable artifacts in one command:
  - `npm run release:windows:all`
- Tagged release automation (GitHub Actions):
  - Push a tag matching `v*` (or `gachatrackerapp-v*`) to run `.github/workflows/gachatrackerapp-release.yml`.
  - The workflow publishes MSI installer, NSIS setup EXE, and portable ZIP to both Actions artifacts and GitHub Release assets.

Output artifacts are created under `src-tauri\target\release\bundle`.
