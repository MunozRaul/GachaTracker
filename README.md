# GachaTracker Desktop

Offline-first local desktop gacha tracker for:
- Wuthering Waves
- Honkai: Star Rail
- Zenless Zone Zero
- Endfield (scanner support; history extraction still evolving)

The app scans local logs/cache files, extracts URL tokens, fetches pull history from official endpoints (when enabled), and persists everything into a local SQLite database.

## Current project status

- Wuthering Waves flow is the primary stable path.
- Pull history UI includes:
  - convene filtering (featured resonator / featured weapon),
  - hidden 3-star rows in the main table (summary/pity logic still counts all pulls),
  - 5-star summary panel,
  - "pulls since latest 5-star" indicator,
  - pity color thresholds:
    - green: `<= 40`
    - yellow/orange: `41-65`
    - red: `> 65`
- Pull history persistence now includes a permanent ledger table to keep data beyond API retention windows.

## Runtime behavior and data flow

Each import run performs:
1. Discover local files (logs + cache candidates)
2. Extract history URLs/tokens
3. Fetch and normalize API rows into `history_pull` entries
4. Persist scan/import metadata
5. Upsert history rows into a permanent ledger
6. Load a merged session for UI display

The merged session uses:
- current scan pulls as primary source,
- plus older ledger rows not present in the current scan window,
- with dedupe to avoid duplicates.

## Network policy

- Two modes exist:
  - `strict-local-only` (blocks online calls),
  - `official-api-exceptions` (allows only approved official hosts).
- Approved hosts are hardcoded in backend (`CONFIRMED_OFFICIAL_API_HOSTS`) and currently include WuWa + HoYo API domains used by this project.
- In the current UI flow, scans are launched with:
  - `strictOffline: false`
  - `allowOfficialApiExceptions: true`

## SQLite database

### Database location

The backend resolves DB path using Tauri app data dir:
- file name: `tracker.db`
- code: `database_path(...)` in `src-tauri/src/lib.rs`

With identifier `com.gachatracker.desktop`, the database paths are:
- Windows: `%APPDATA%\com.gachatracker.desktop\tracker.db`
- Linux: `~/.local/share/com.gachatracker.desktop/tracker.db`
- macOS: `~/Library/Application Support/com.gachatracker.desktop/tracker.db`

### Schema version

- `DB_SCHEMA_VERSION = 6`
- Migrations are tracked in `schema_migrations` and applied automatically on startup/open.

### Core tables

- `scans`, `game_results`, `findings`
  - raw scan metadata and extracted local findings.
- `import_sessions`
  - one import snapshot per scan (`scan_id` primary key).
- `import_pulls`, `import_banners`, `import_sources`, `import_diagnostics`
  - materialized import session used for UI and troubleshooting.
- `history_ledger_pulls` (new permanent storage)
  - deduped long-term history ledger across scans.
  - key columns:
    - `dedupe_key` (PK)
    - `game_id`, `banner_id`
    - `source_pull_id`
    - `item_name`, `item_type_name`, `rarity`, `pulled_at`
    - `first_seen_scan_id`, `last_seen_scan_id`

### How dedupe works

For ledger upsert, backend computes a key:
- preferred: `game_id + source_pull_id`
- fallback fingerprint if source ID is missing:
  - game/banner/time/item/type/rarity/source file

Rows are inserted with `ON CONFLICT(dedupe_key) DO UPDATE`, so repeated imports update existing rows instead of duplicating them.

### Why this ledger exists

Game URL tokens expire quickly and APIs may expose only recent history.  
The ledger keeps previously fetched rows locally, so older pulls remain visible even when:
- token is expired,
- API no longer returns old entries.

## Endfield note

Endfield adapter currently detects local artifacts and candidate URLs, but confirmed robust history extraction depends on reliable token/API evidence from real sessions.

## Development

1. Install dependencies:
   - Node.js + npm
   - Rust toolchain
2. Install packages:
   - `npm install`
3. Run desktop app:
   - `npm run tauri dev`

## Build executable

- Windows installer artifacts (MSI + NSIS setup EXE):
  - `npm run release:windows`
- Portable ZIP from latest release binary:
  - `npm run release:windows:portable`
- Build installer + portable in one command:
  - `npm run release:windows:all`

Output artifacts are created under `src-tauri\target\release\bundle`.
