# Decision Record: official API exceptions baseline

- **Date:** 2026-04-14
- **Todo:** `evaluate-official-api-exceptions`
- **Scope:** WuWa, HSR, ZZZ, Endfield

## Evidence reviewed in this repository

### Wuthering Waves (WuWa)
- `WutheringWaves\Client.log` parser in app targets:
  - `https://aki-gm-resources.../aki/gacha/index.html#/record...`
- `WutheringWaves\FetchData.ps1` delegates to community script (`wuwatracker` GitHub).
- **Assessment:** pull-history URL token patterns appear locally discoverable, but official/public API confirmation is **unknown**.

### Honkai: Star Rail (HSR)
- `HonkaiStarrail\get_warp_link_os.ps1` extracts cache URLs containing:
  - `getGachaLog`
  - `getLdGachaLog`
  - `authkey` query params
- Script validates response `retcode == 0` when URL is still valid.
- **Assessment:** tokenized history endpoints appear reachable from local cache artifacts, but official/public API support is **unknown**.

### Zenless Zone Zero (ZZZ)
- `ZZZ\Player.log` contains gacha web URL examples on `gs.hoyoverse.com` with `authkey`.
- `ZZZ\get_signal_link_os.ps1` follows same cache-token extraction approach (`getGachaLog`).
- **Assessment:** tokenized history URLs appear locally available, but official/public API support is **unknown**.

### Endfield
- `Endfield\Player.log` sample did not provide clear gacha/authkey URL pattern in this repo snapshot.
- App keeps this on heuristic detection (`possible_history_source`) only.
- **Assessment:** official/public API availability is **unknown** and local token pattern is currently **not confirmed**.

## Decision

1. Keep **strict local-only** as default runtime policy.
2. Allow exceptions only for hosts that are **officially confirmed** as supported public APIs.
3. Because confirmation is currently unknown for all four games in this repository context, the confirmed-host allowlist remains **empty** for now.
4. UI and backend messaging must state that enabling exceptions does not permit unknown/unconfirmed endpoints.

## Follow-up criteria to add a host

Add hosts only after recording evidence of official confirmation (publisher docs/changelog/legal API statement) and mapping the game + endpoint usage path in code.
