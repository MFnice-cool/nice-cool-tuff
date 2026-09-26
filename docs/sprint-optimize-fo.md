# Sprint: Optimize pack — Fabulously Optimized set (2026-09-11)

## Goal
Add a third Optimize mode that installs the **exact Fabulously Optimized mod set**
(pinned builds from the official FO `.mrpack`) into the current instance.
Curated/custom flows stay untouched, except a guard that redirects modpack-type
curated mappings to the new tab.

## What was built
- **Core — `crates/tuffbox-core/src/fo_pack.rs` (new):** pure, network-free
  parsing — `(project_id, version_id)` extraction from Modrinth CDN URLs,
  `modrinth.index.json` → client-jar filter (`env.client != unsupported`,
  non-mod/server-only counted), jar-stem display fallback. 6 unit tests.
- **Core — `importer.rs`:** additive `read_mrpack_index_json(&[u8])` (in-memory
  `.mrpack` → index JSON) + test. File-based import untouched.
- **Core — `provider/modrinth.rs`:** additive `get_projects_batch(&[String])`
  (`GET /v2/projects?ids=`, chunked ×100) for FO name/slug resolution.
- **Desktop — `preview_fo_optimize_pack`:** Fabric/Quilt gate, exact-MC match
  against `optimize-packs.json` (error lists FO-supported MCs), Quilt→Fabric
  build fallback, `.mrpack` download → index parse → SHA-1 fallback for
  unparsed URLs → batch names (+ api-cache warmup) → selectable rows
  (not-installed first) + shared config templates + warnings.
- **Desktop — `install_fo_optimize_pack`:** one snapshot + pinned adds via new
  `add_mod_from_modrinth_pinned` (exact `version_id`, project/version
  consistency check) + empty-seed dep-fill rounds + one download batch +
  history/co-occurrence + optional config plan.
- **Desktop — curated guard:** `preview_curated_optimize_pack` errors when the
  mapped project is a *modpack*, pointing at the FO tab (previously it would
  download a stray `.mrpack` into `mods/`).
- **Frontend — `OptimizePackModal.svelte`:** `Mode = curated | fo | custom`,
  FO tab reusing the selectable-offer rows (jar name, `pinned`/`installed`
  pills), shared config/test-launch UI; bootstrap + curated-tab errors that
  are FO-redirects open the FO tab directly (fallback to Custom if FO fails);
  curated empty-state gains an “Open FO set” CTA on redirect.
- **Frontend — `api.ts`:** `previewFoOptimizePack` / `applyFoOptimizePlan`.

## Checks
- `svelte-check`: 0 errors, 138 warnings (pre-existing baseline, none new).
- `vitest`: 35/35 pass.
- `lint:tokens` (border-radius), `check:bridge` (396 TS ↔ 474 Rust, incl. the
  2 new commands), `lint:lazy-views`: all OK.
- Rust: **not compiled here** — no Rust toolchain in the sandbox
  (`cargo: command not found`). New core tests (`fo_pack`, `read_mrpack_index_json`)
  and `cargo check -p tuffbox-desktop` must run in CI / locally.
- Live Modrinth verification impossible from the sandbox (no external egress —
  TLS fails); FO data-path formats follow Modrinth docs + existing code paths.

## Manual verification (needs a dev machine)
1. Fabric instance, MC with an FO mapping (e.g. 1.21.1) → Content → Optimize:
   lands on the FO tab, ~100+ pinned rows, warnings sane.
2. Deselect a few → Apply → mods install at FO-pinned versions, one snapshot,
   configs applied, Test launch works.
3. Quilt instance → FO tab works (Fabric-build fallback warning when no
   Quilt-tagged FO build).
4. Forge/NeoForge instance → FO tab shows the loader error inline; Curated tab
   still falls back to Custom as before.
5. Instance on an MC without FO mapping → FO tab lists supported MC versions.

## Backlog / follow-ups
- Cache the downloaded FO `.mrpack`/index per (project, version) to make
  re-opens instant.
- Optional: import FO `overrides/` configs as an opt-in config source.
- Run the new Rust unit tests + `cargo check` in CI.
