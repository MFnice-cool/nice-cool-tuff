# Sprint: JVM + heap auto-tuning per pack

Date: 2026-09-11. Branch: `arena/01a08f9f-tuffbox-build-test`.

## Goal

Auto-pick Java launch flags per pack from loader + game version (ZGC mandatory
on vanilla), never emit RAM-size flags from the tuner, and size heap
automatically from mod counts in Modrinth/CurseForge categories (same taxonomy
as the dependency graph).

## What changed

- **New `tuffbox-core/src/jvm_tuning.rs`** (pure, unit-tested):
  - `recommend_jvm_args(loader, mc_version, java_major, heavy_pack)`:
    vanilla → `zgc-generational` (Java 21+) / `zgc` (15–20) / `g1-legacy`
    fallback below 15 with an explanatory note; heavy modded → `g1-aikar`
    (byte-identical to the Project Settings preset; minus two Java 9+ flags
    on Java 8); light modded → `g1-balanced` / `g1-legacy` on Java 8.
  - `recommend_heap_mb(total_ram, loader, mods_categories)`: loader base
    (Forge/NeoForge 3072, Fabric/Quilt 2560, vanilla 2048) + per-mod demand
    from the heaviest provider category (`worldgen` 160 … `optimization` 10,
    unknown 60); same clamp/ceiling/rounding as the legacy estimator.
  - Guarantees (asserted in tests): no `-Xmx`/`-Xms`/`-Xmn` in any profile;
    `UnlockExperimentalVMOptions` precedes the experimental G1 flags.
- **Launch** (`build_and_spawn`): replaces the old mod-count GC block. Computes
  heap estimate + JVM profile from the manifest, applies flags via GC-aware
  dedupe, logs `# JVM auto-tune (<profile>): <note>` and `# Heap estimate: …`.
  Heap itself still comes from the memory setting (`-Xmx` built as before).
- **GC-conflict fix**: `append_stability_jvm_args` no longer forces `UseG1GC`
  when any collector is selected; new `append_tuned_jvm_arg` skips tuned GC
  selectors unless no collector is set (G1+ZGC together refuse to start).
- **Empty JVM args are the new default**: `create_instance` and the create
  modal default to `[]`; Project Settings fallback/reset → empty with an
  "auto-tuned" placeholder; stale `NO_GC_SETTING` validation finding removed
  (nothing referenced it).
- **Project Settings**: memory card shows "Auto for this pack: X (Apply)";
  JVM card shows an "Auto: \<profile>" card (Apply writes flags, matches the
  Aikar/ZGC preset cards when identical). Backed by new commands
  `recommend_heap_cmd` / `recommend_jvm_cmd` (preview uses required Java).
- Legacy global `get_auto_tune` (Settings Auto button) untouched.

## Checks (sandbox)

- `svelte-check`: 0 errors / 138 warnings (baseline).
- `vitest`: 35/35. `check:bridge`, `lint:tokens`, `lint:lazy-views`: OK.
- Rust **not** verified in sandbox (no cargo): CI/local must run
  `cargo test -p tuffbox-core jvm_tuning` and `cargo check -p tuffbox-desktop`.

## Manual scenarios

1. Vanilla 1.21 instance, empty JVM args → launch log shows
   `# JVM auto-tune (zgc-generational)`; game runs with ZGC (check
   `tuffbox-console.log` command line / F3 debug).
2. Heavy Forge 1.20.1 pack → `(g1-aikar)`; JVM card "Auto" matches the Aikar
   preset; Apply → preset card highlights as active.
3. Old Forge 1.16.5 on Java 8 → `(g1-aikar)` without the two Java 9+ flags;
   game starts (no "Unrecognized VM option").
4. Explicit `-XX:+UseG1GC` in profile args on vanilla → stays G1 (user wins),
   no ZGC appended, no conflict error.
5. Project Settings memory card → "Auto for this pack" → Apply → slider moves;
   value scales with worldgen/tech mod counts and caps at 60% RAM.

## Backlog

- Fill `ModSource.categories` for mods installed from CurseForge/imports that
  lack them (today they fall back to the 60 MB default weight).
- Server-profile launches (`launch_server`) don't use the tuner yet.
- Optional: warn when configured heap is far below the estimate.
