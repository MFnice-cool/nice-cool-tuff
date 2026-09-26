# Sprint: InstanceHome → Library content drawer + real server ping

Date: 2026-09-11. Branch: `arena/01a08f9f-tuffbox-build-test`.

## Goal

Move the home-dashboard InstanceHome block into the Library (out of the way),
drop Worlds from it, and make Servers a real status board: online state plus
player counts.

## What changed

- **New `LibraryInstanceContent.svelte`** (replaces `InstanceHome.svelte`, deleted):
  tabs Mods / Packs / Shaders / **Servers** (default). No Worlds — worlds live
  in the World view. No inner collapse; the host owns visibility.
- **Library side rail** (`LibraryInstancesPane.svelte`): selected pack now has a
  "Content — mods · packs · shaders · servers" drawer under the meta grid,
  **collapsed by default** (persisted `tuffbox-library-content-collapsed`).
  "Open Mods" opens the selected pack in IDE Content.
- **Home dashboard**: InstanceHome mount removed; dead `hasInstanceHome` /
  `hideInstanceHome` deriveds removed. `hideInstanceHome` also removed from the
  frontend settings type/defaults; the backend field stays as a serde-defaulted
  legacy field so old settings files still load.
- **Real Server List Ping** (`crates/tuffbox-core/src/servers_dat.rs`):
  `ping_server_address` now does TCP connect + Java handshake/status round-trip
  and returns `playersOnline`/`playersMax` (new serde-defaulted fields on
  `ServerPingResult`, mirrored in `McServerPing`). TCP-only success (Bedrock,
  filtered MOTD) still reports online with players `None`.
  Pure helpers (`split_host_port`, varint, status-JSON parse) have unit tests.
- **Servers tab**: auto ping-all on first open and on refresh, explicit
  "Ping all" button, online-first sort, per-row status dot + latency +
  `👥 online/max`, tab badge `online/total` once pinged.

## Checks (sandbox)

- `svelte-check`: 0 errors / 138 warnings (baseline).
- `vitest`: 35/35. `lint:tokens`, `check:bridge`, `lint:lazy-views`: OK.
- Rust not verified in sandbox (no cargo): run
  `cargo check -p tuffbox-desktop` and `cargo test -p tuffbox-core servers_dat`.

## Manual scenarios (need internet + a Java server)

1. Library → select pack → expand Content → Servers tab is default; rows show
   green/grey/red dots, latency, `online/max` players; online servers sort first.
2. Add `mc.hypixel.net` → Ping all → player counts appear; tab badge `n/m`.
3. Add a Bedrock/unreachable address → row shows offline (or online without
   players for TCP-only), no crash; delete works.
4. Collapse persists across restarts; Home no longer shows the block; Settings
   has no InstanceHome toggle.
