# Sprint: GPU detect + discrete-by-default launch

Date: 2026-09-11. Branch: `arena/01a08f9f-tuffbox-build-test`.

## Goal

Detect discrete vs integrated GPUs; launch Minecraft on the discrete GPU by
default. (Performance-core affinity already exists and is untouched.)

## What changed

- **New `src-tauri/src/gpu.rs`** (mirrors `cpu_affinity.rs`):
  - Windows: DXGI adapter enumeration (skips WARP/software), name / PCI
    vendor+device / VRAM / primary (attached outputs).
  - Linux: `/sys/class/drm/cardN` sysfs scan (vendor/device/class/boot_vga,
    driver, PCI slot, amdgpu VRAM), names via `pci.ids` (`hwdata`/`misc`)
    with a vendor+driver fallback label.
  - Other OSes: detection returns empty (documented backlog).
  - Pure, unit-tested core: `classify_gpu` (NVIDIA→discrete, Intel Arc→discrete
    else integrated, AMD RX/Pro/big-Vega→discrete else APU→integrated),
    `resolve_target_gpu` (auto = discrete → primary → first),
    `linux_prime_env` (NVIDIA offload trio / `DRI_PRIME=pci-…`), Windows
    `GpuPreference=2;/1;` values, pci.ids parser.
- **Launch hook** (`build_and_spawn`, after the overlay env): detect → resolve
  by `gpu_preference` → Linux sets PRIME env on the game command, Windows writes
  `HKCU\Software\Microsoft\DirectX\UserGpuPreferences` for the exact java binary
  (+ `javaw.exe` sibling). Console log line `# GPU: <name> (<kind>, <vendor>)`.
  Best-effort everywhere — never blocks the game.
- **Setting `gpu_preference`** (`auto` default): backend struct + serde default
  + `Default` impl, `LauncherSettings` TS type, Settings default. Old settings
  files load fine (serde default).
- **Settings → Java → GPU**: Auto / Discrete / Integrated select + Detect button
  + detected-adapter list (`name (kind, primary?, VRAM)`), loaded on mount.
- **Command `detect_gpus`** registered; frontend `api.launcher.detectGpus()`.
- Deps: `Win32_Graphics_Dxgi` windows feature + `winreg.workspace` (Windows
  only) in the desktop crate. No new crates.

## Checks (sandbox)

- `svelte-check`: 0 errors / 138 warnings (baseline).
- `vitest`: 35/35. `lint:tokens`, `check:bridge`, `lint:lazy-views`: OK.
- Rust **not** verified in sandbox (no cargo): CI/local must run
  `cargo check -p tuffbox-desktop` (Windows + Linux) and
  `cargo test -p tuffbox-desktop gpu`.

## Manual scenarios

1. Windows hybrid laptop (Intel iGPU + NVIDIA): Settings → GPU shows both
   adapters (`… (integrated, primary)` + `… (discrete)`); launch → console has
   `# GPU: NVIDIA … (discrete, nvidia)`; registry
   `HKCU\…\UserGpuPreferences\<java.exe>` = `GpuPreference=2;`; game renders on
   NVIDIA (check `F3` → Display or `nvidia-smi`).
2. Force integrated: select "Integrated GPU" → launch → `GpuPreference=1;`,
   console names the iGPU.
3. Linux PRIME laptop: launch → game env contains `__NV_PRIME_RENDER_OFFLOAD=1`
   (NVIDIA dGPU) or `DRI_PRIME=pci-…` (AMD dGPU); `DRI_PRIME=1 glxinfo` sanity.
4. Single-GPU desktop: launch → console names the only adapter, no env/registry
   side effects that change rendering.
5. Regression: CPU affinity modes still pin; old `launcher-settings.json`
   without `gpuPreference` loads and saves with `auto`.

## Backlog

- macOS detection (`system_profiler SPDisplaysDataType`).
- Per-instance GPU override (Project Settings) on top of the global default.
- Diagnostics view: show detected GPUs next to CPU/RAM.
- `lspci` name fallback when `pci.ids` is missing (minimal distros).
