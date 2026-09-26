# CPU Affinity for Minecraft Java (P-core pinning) — Implementation Plan

> **For Hermes:** Implement task-by-task (subagent-driven-development or direct — backend Rust tasks are small enough for direct edits; OMP optional).

**Goal:** Let the player pin the spawned Minecraft Java process to performance cores (Intel 12–14th gen hybrid P/E split, or a manual mask for AMD X3D dual-CCD) via `SetProcessAffinityMask`, configured in Settings → Launcher → Java.

**Architecture:** New desktop-crate module `cpu_affinity.rs` reads core topology via `GetLogicalProcessorInformationEx` and applies an affinity mask to the running child PID right after `spawn_and_track_with_cleanup` returns. Mode + optional manual mask are stored in `LauncherSettings` (same JSON file as the rest of launcher settings) and surfaced in the existing Java settings card.

**Tech Stack:** Rust (`windows` crate 0.58 — already a dependency; add the `Win32_System_SystemInformation` feature), Svelte 5 settings UI, no new crates.

---

## Context (verified against the codebase)

| What | Where |
|---|---|
| Launch command builder + spawn call site | `apps/tuffbox-desktop/src-tauri/src/lib.rs:13293` (`build_and_spawn`), spawn at `:13814` (`tuffbox_core::process::spawn_and_track_with_cleanup`), result emitted at `:13829` |
| `RunningProcess { id, profile_id, pid, log_path, started_at }` | `crates/tuffbox-core/src/process.rs:297` — `pid` is what we pin |
| Child spawn internals (no change needed) | `crates/tuffbox-core/src/process.rs:208-331` |
| Launcher settings struct / load / save | `apps/tuffbox-desktop/src-tauri/src/launcher_settings.rs:15` (struct), `:128` (Default), `:181` (save), `:517` (`save_launcher_settings_cmd`) |
| `windows` crate features | `apps/tuffbox-desktop/src-tauri/Cargo.toml:57-63` (has `Win32_System_Threading`; missing `Win32_System_SystemInformation`) |
| TS `LauncherSettings` type | `apps/tuffbox-desktop/src/lib/store.ts:187-222` |
| Settings UI state + Java card | `apps/tuffbox-desktop/src/components/Settings.svelte:176-199` (initial state), `:1496` (`launcher/java` tab), Custom JVM args label at `:1513-1521` |
| Progress logging inside `build_and_spawn` | `progress.log(&format!(...))` — available after spawn (used at `lib.rs:13710`) |

**Design decisions:**
- Pin **after spawn** (post-spawn `OpenProcess` + `SetProcessAffinityMask`), not pre-spawn `CREATE_SUSPENDED` + resume. Rationale: zero restructuring of `tuffbox-core::process`, the few-ms race window before pinning is irrelevant for a multi-hour game session. Pre-spawn pinning stays a possible future refinement.
- Topology detection: `RelationProcessorCore` entries with `EfficiencyClass > 0` = performance cores (Windows 11 hybrid semantics). If all cores share one class (no split — most AMD CPUs), "performance" mode is a logged no-op; AMD X3D users pick **manual** mask.
- `SetProcessAffinityMask` is single-processor-group — fine for all consumer CPUs (< 64 logical CPUs per group).
- Non-Windows: module compiles to no-ops (mode logged, nothing applied). Linux `sched_setaffinity` = future work.

---

### Task 1: Add the `Win32_System_SystemInformation` feature

**Objective:** Enable `GetLogicalProcessorInformationEx` in the existing `windows` dependency.

**Files:**
- Modify: `apps/tuffbox-desktop/src-tauri/Cargo.toml:57-63`

**Step 1:** Add one line to the feature list:

```toml
windows = { version = "0.58", features = [
  "Win32_Foundation",
  "Win32_System_Diagnostics_Debug",
  "Win32_System_LibraryLoader",
  "Win32_System_Memory",
  "Win32_System_Threading",
  "Win32_System_SystemInformation",
] }
```

**Step 2: Verify it still resolves**

Run: `export tmp="C:/Users/admin/AppData/Local/Temp"; cargo check -p tuffbox-desktop`
Expected: compiles (pre-existing ~18 warnings are the baseline).

**Step 3: Commit**

```bash
git add apps/tuffbox-desktop/src-tauri/Cargo.toml
git commit -m "feat(launch): enable Win32_System_SystemInformation for core topology"
```

---

### Task 2: Create `cpu_affinity.rs` — pure logic first (TDD)

**Objective:** Mask parsing + target-mask resolution as pure, testable functions; topology detection and WinAPI apply behind `#[cfg(windows)]`.

**Files:**
- Create: `apps/tuffbox-desktop/src-tauri/src/cpu_affinity.rs`

**Step 1: Write the module with tests included** (test-first: the tests below are the spec)

```rust
//! CPU affinity pinning for the Minecraft Java process.
//!
//! On hybrid CPUs (Intel 12th–14th gen P/E cores) Windows may schedule the
//! render thread on E-cores. When enabled in Settings, the launcher pins the
//! spawned javaw process to performance cores via SetProcessAffinityMask.
//! Manual mode covers AMD X3D dual-CCD CPUs (no OS-reported efficiency split).

/// Settings mode for CPU affinity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AffinityConfig {
    /// `off` | `performance` | `manual`
    pub mode: String,
    /// Manual mask (hex string), used only when mode == "manual".
    pub mask_raw: String,
}

/// Core topology summary from GetLogicalProcessorInformationEx.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTopology {
    /// Mask of every logical processor the process may use.
    pub all_mask: u64,
    /// Mask of logical processors in the highest efficiency class.
    /// Equals `all_mask` when the CPU reports no efficiency split.
    pub performance_mask: u64,
    /// True when at least two distinct efficiency classes were reported.
    pub has_efficiency_split: bool,
}

/// Parse a hex bitmask like "0xFF0" / "ff0". Decimal is intentionally NOT
/// accepted — ambiguity between 0x10 and 10 is a footgun.
pub fn parse_affinity_mask(raw: &str) -> Option<u64> {
    let s = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(s, 16).ok().filter(|m| *m != 0)
}

/// Resolve the mask to apply for the given config + detected topology.
/// Returns None when nothing should be applied (off / invalid / no split).
pub fn resolve_target_mask(cfg: &AffinityConfig, topo: Option<&CoreTopology>) -> Option<u64> {
    match cfg.mode.as_str() {
        "performance" => {
            let topo = topo?;
            if topo.has_efficiency_split {
                Some(topo.performance_mask)
            } else {
                None
            }
        }
        "manual" => {
            let mask = parse_affinity_mask(&cfg.mask_raw)?;
            let all = topo.map(|t| t.all_mask).unwrap_or(u64::MAX);
            // Must stay inside what the process is allowed to use.
            (mask & !all == 0).then_some(mask)
        }
        _ => None,
    }
}

/// Detect core topology (Windows only; None elsewhere).
#[cfg(windows)]
pub fn detect_core_topology() -> Result<CoreTopology, String> {
    use windows::Win32::System::SystemInformation::{
        GetLogicalProcessorInformationEx, RelationProcessorCore,
        SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    };

    unsafe {
        // First call with None returns the required buffer size in `len`.
        let mut len: u32 = 0;
        let _ = GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut len);
        if len == 0 {
            return Err("GetLogicalProcessorInformationEx returned size 0".into());
        }
        let mut buf = vec![0u8; len as usize];
        let info = buf.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX;
        if !GetLogicalProcessorInformationEx(RelationProcessorCore, Some(info), &mut len).as_bool() {
            return Err(format!(
                "GetLogicalProcessorInformationEx failed: {}",
                windows::core::Error::from_win32()
            ));
        }

        let mut all_mask: u64 = 0;
        let mut max_class: u8 = 0;
        // (group, mask, class) per physical core entry
        let mut cores: Vec<(u16, u64, u8)> = Vec::new();
        let mut offset = 0usize;
        while offset < len as usize {
            let entry = &*(buf.as_ptr().add(offset) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX);
            let size = entry.Size as usize;
            if size == 0 {
                break;
            }
            if entry.Relationship == RelationProcessorCore {
                let p = &entry.Anonymous.Processor;
                let class = p.EfficiencyClass;
                max_class = max_class.max(class);
                for i in 0..p.GroupCount as usize {
                    let gm = &p.GroupMask[i];
                    cores.push((gm.Group, gm.Mask as u64, class));
                    all_mask |= gm.Mask as u64;
                }
            }
            offset += size;
        }

        let has_split = cores.iter().any(|(_, _, c)| *c > 0);
        let performance_mask = if has_split {
            cores
                .iter()
                .filter(|(_, _, c)| *c == max_class)
                .fold(0u64, |acc, (_, m, _)| acc | m)
        } else {
            all_mask
        };
        if all_mask == 0 {
            return Err("no processor cores reported".into());
        }
        Ok(CoreTopology {
            all_mask,
            performance_mask,
            has_efficiency_split: has_split,
        })
    }
}

#[cfg(not(windows))]
pub fn detect_core_topology() -> Result<CoreTopology, String> {
    Err("CPU affinity is only supported on Windows".into())
}

/// Apply `mask` to the process `pid` (post-spawn). Windows only.
#[cfg(windows)]
pub fn apply_mask_to_pid(pid: u32, mask: u64) -> Result<(), String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetProcessAffinityMask, OpenProcess, SetProcessAffinityMask,
        PROCESS_QUERY_INFORMATION, PROCESS_SET_INFORMATION,
    };

    if mask == 0 {
        return Err("affinity mask is empty".into());
    }
    unsafe {
        let process = OpenProcess(
            PROCESS_SET_INFORMATION | PROCESS_QUERY_INFORMATION,
            false,
            pid,
        )
        .map_err(|e| format!("OpenProcess({pid}): {e}"))?;
        let result = (|| {
            let mut current: usize = 0;
            let mut system: usize = 0;
            if !GetProcessAffinityMask(process, &mut current, &mut system).as_bool() {
                return Err(format!("GetProcessAffinityMask({pid}): {}", windows::core::Error::from_win32()));
            }
            if mask & !(current as u64) != 0 {
                return Err(format!("mask 0x{mask:X} is outside the process affinity 0x{current:X}"));
            }
            if !SetProcessAffinityMask(process, mask as usize).as_bool() {
                return Err(format!("SetProcessAffinityMask({pid}): {}", windows::core::Error::from_win32()));
            }
            Ok(())
        })();
        let _ = CloseHandle(process);
        result
    }
}

#[cfg(not(windows))]
pub fn apply_mask_to_pid(_pid: u32, _mask: u64) -> Result<(), String> {
    Err("CPU affinity is only supported on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(mode: &str, mask: &str) -> AffinityConfig {
        AffinityConfig { mode: mode.into(), mask_raw: mask.into() }
    }

    #[test]
    fn parse_hex_mask() {
        assert_eq!(parse_affinity_mask("0xFF0"), Some(0xFF0));
        assert_eq!(parse_affinity_mask("ff0"), Some(0xFF0));
        assert_eq!(parse_affinity_mask(" 0x1 "), Some(1));
        assert_eq!(parse_affinity_mask(""), None);
        assert_eq!(parse_affinity_mask("xyz"), None);
        assert_eq!(parse_affinity_mask("0x0"), None);
    }

    #[test]
    fn off_mode_never_applies() {
        assert_eq!(resolve_target_mask(&cfg("off", ""), None), None);
        assert_eq!(resolve_target_mask(&cfg("off", "0xFF"), None), None);
    }

    #[test]
    fn performance_mode_needs_split() {
        let uniform = CoreTopology { all_mask: 0xFF, performance_mask: 0xFF, has_efficiency_split: false };
        let hybrid = CoreTopology { all_mask: 0xFF, performance_mask: 0xF0, has_efficiency_split: true };
        assert_eq!(resolve_target_mask(&cfg("performance", ""), Some(&uniform)), None);
        assert_eq!(resolve_target_mask(&cfg("performance", ""), Some(&hybrid)), Some(0xF0));
        assert_eq!(resolve_target_mask(&cfg("performance", ""), None), None);
    }

    #[test]
    fn manual_mode_validates_against_topology() {
        let topo = CoreTopology { all_mask: 0xFF, performance_mask: 0xF0, has_efficiency_split: true };
        assert_eq!(resolve_target_mask(&cfg("manual", "0x0F"), Some(&topo)), Some(0x0F));
        // Outside the allowed set → rejected.
        assert_eq!(resolve_target_mask(&cfg("manual", "0xF00"), Some(&topo)), None);
        // No topology available → permissive (mask applied, OS clamps).
        assert_eq!(resolve_target_mask(&cfg("manual", "0xFF0"), None), Some(0xFF0));
    }

    #[cfg(windows)]
    #[test]
    fn topology_smoke() {
        let topo = detect_core_topology().expect("topology detection");
        assert!(topo.all_mask.count_ones() >= 1);
        assert!(topo.performance_mask & topo.all_mask == topo.performance_mask);
    }
}
```

**Step 2: Register the module**

In `apps/tuffbox-desktop/src-tauri/src/lib.rs:2` (alphabetical order in the `mod` list):

```rust
mod auth;
mod cosmetics_local;
mod cpu_affinity;   // <- add
```

**Step 3: Run the tests**

Run: `export tmp="C:/Users/admin/AppData/Local/Temp"; cargo test -p tuffbox-desktop --lib cpu_affinity`
Expected: 5 passed (4 pure + 1 windows topology smoke).

**Step 4: Commit**

```bash
git add apps/tuffbox-desktop/src-tauri/src/cpu_affinity.rs apps/tuffbox-desktop/src-tauri/src/lib.rs
git commit -m "feat(launch): cpu_affinity module — topology detection, mask resolve, tests"
```

---

### Task 3: Settings fields (Rust)

**Objective:** Persist `cpuAffinityMode` + `cpuAffinityMask` in `launcher_settings.json`.

**Files:**
- Modify: `apps/tuffbox-desktop/src-tauri/src/launcher_settings.rs` (struct at `:15-83`, Default impl at `:128-155`)

**Step 1: Add fields to `LauncherSettings`** (after `ingame_overlay` at `:82-83`):

```rust
    /// CPU affinity for the game process: `off` | `performance` | `manual`.
    /// `performance` pins to the highest-efficiency-class cores on hybrid CPUs
    /// (no-op on uniform CPUs); `manual` uses `cpu_affinity_mask`.
    #[serde(default = "default_cpu_affinity_mode")]
    pub cpu_affinity_mode: String,
    /// Hex bitmask for `manual` mode (e.g. "0xFF0" = first 4 E-cores excluded).
    #[serde(default)]
    pub cpu_affinity_mask: String,
```

**Step 2: Add the default fn + Default impl entries:**

```rust
fn default_cpu_affinity_mode() -> String {
    "off".into()
}
```

In `impl Default for LauncherSettings` add:

```rust
            cpu_affinity_mode: default_cpu_affinity_mode(),
            cpu_affinity_mask: String::new(),
```

**Step 3: Verify**

Run: `export tmp="C:/Users/admin/AppData/Local/Temp"; cargo test -p tuffbox-desktop --lib launcher_settings`
Expected: existing settings tests pass; old JSON files (missing the new keys) deserialize via `#[serde(default)]`.

**Step 4: Commit**

```bash
git add apps/tuffbox-desktop/src-tauri/src/launcher_settings.rs
git commit -m "feat(launch): cpu affinity settings fields (mode + manual mask)"
```

---

### Task 4: Apply affinity in `build_and_spawn`

**Objective:** After the game spawns, pin it and log the outcome to the launch console.

**Files:**
- Modify: `apps/tuffbox-desktop/src-tauri/src/lib.rs` (insert between the `spawn_and_track_with_cleanup` result at `:13814-13827` and the `process-started` emit at `:13829`)

**Step 1:** Before the spawn call (e.g. right after the `CrashExitCtx` block, ~`:13720`), resolve the config once:

```rust
    // CPU affinity (performance-core pinning) — resolved once, applied post-spawn.
    let affinity_cfg = cpu_affinity::AffinityConfig {
        mode: launch_settings.cpu_affinity_mode.clone(),
        mask_raw: launch_settings.cpu_affinity_mask.clone(),
    };
```

**Step 2:** After the `?;` that ends the `spawn_and_track_with_cleanup(...)` block (`:13827`), insert:

```rust
    if let Some(mask) =
        cpu_affinity::resolve_target_mask(&affinity_cfg, cpu_affinity::detect_core_topology().ok().as_ref())
    {
        match cpu_affinity::apply_mask_to_pid(running.pid, mask) {
            Ok(()) => progress.log(&format!(
                "# CPU affinity: pinned to 0x{mask:X} ({} logical cores)",
                mask.count_ones()
            )),
            Err(e) => progress.log(&format!("# WARNING: CPU affinity: {e}")),
        }
    }
```

Notes:
- Synchronous on purpose (< 1 ms, no Send/threading concerns with `progress`).
- All failures are warnings, never launch errors — a pinning failure must not block the game.
- Server launches go through the same `build_and_spawn` path and benefit identically.

**Step 3: Verify (compile + baseline tests)**

Run: `export tmp="C:/Users/admin/AppData/Local/Temp"; cargo test -p tuffbox-desktop --lib cpu_affinity`
Expected: pass.

**Step 4: Commit**

```bash
git add apps/tuffbox-desktop/src-tauri/src/lib.rs
git commit -m "feat(launch): pin game process to performance cores when enabled"
```

---

### Task 5: TS type + Settings UI

**Objective:** Surface the setting in Settings → Launcher → Java.

**Files:**
- Modify: `apps/tuffbox-desktop/src/lib/store.ts:187-222` (`LauncherSettings`)
- Modify: `apps/tuffbox-desktop/src/components/Settings.svelte:176-199` (initial state) and `:1513-1521` (Java card, after the Custom JVM arguments label)

**Step 1:** In `store.ts`, add to `LauncherSettings` (before the closing brace at `:222`):

```ts
  /** CPU affinity for the game process: off | performance | manual. */
  cpuAffinityMode: "off" | "performance" | "manual";
  /** Hex bitmask used when mode is "manual" (e.g. "0xFF0"). */
  cpuAffinityMask: string | null;
```

**Step 2:** In `Settings.svelte` initial state (`:192`, after `ingameOverlay: true,`):

```ts
    cpuAffinityMode: "off",
    cpuAffinityMask: null,
```

**Step 3:** In the Java card, after the Custom JVM arguments `</label>` (`:1521`), add (Svelte 5: `onchange`/`onblur` props — matches the card's existing style):

```svelte
        <label>
          CPU affinity
          <div class="path-row">
            <select
              bind:value={launcher.cpuAffinityMode}
              onchange={() => persistLauncher({ cpuAffinityMode: launcher.cpuAffinityMode })}
            >
              <option value="off">Off (let Windows decide)</option>
              <option value="performance">Performance cores (hybrid CPUs)</option>
              <option value="manual">Manual mask</option>
            </select>
          </div>
          <small class="auto-tune-msg">
            Pins the game process to fast cores via SetProcessAffinityMask. "Performance cores"
            needs a P/E hybrid CPU (Intel 12th gen+); AMD X3D users should pick a manual mask.
          </small>
        </label>
        {#if launcher.cpuAffinityMode === "manual"}
          <label>
            Affinity mask (hex)
            <input
              bind:value={launcher.cpuAffinityMask}
              placeholder="0xFF0"
              onblur={() =>
                persistLauncher({ cpuAffinityMask: launcher.cpuAffinityMask?.trim() || null })}
            />
          </label>
        {/if}
```

**Step 4: Verify**

Run: `cd apps/tuffbox-desktop && npm run check`
Expected: 0 errors (baseline: 1 pre-existing warning in TestRuns.svelte).

**Step 5: Commit**

```bash
git add apps/tuffbox-desktop/src/lib/store.ts apps/tuffbox-desktop/src/components/Settings.svelte
git commit -m "feat(settings): CPU affinity toggle (performance cores / manual mask)"
```

---

### Task 6: End-to-end verification

**Objective:** Prove the feature works on a real launch.

**Step 1:** Full gates:

```bash
export tmp="C:/Users/admin/AppData/Local/Temp"
cargo test -p tuffbox-desktop --lib cpu_affinity
cargo check -p tuffbox-desktop
cd apps/tuffbox-desktop && npm run check
```

Expected: all green.

**Step 2:** Manual E2E (dev machine):
1. `npm run tauri dev` in `apps/tuffbox-desktop`, Settings → Launcher → Java → set CPU affinity to "Performance cores".
2. Launch any instance. In the console log expect `# CPU affinity: pinned to 0x... (N logical cores)` (or the WARNING line when no hybrid split exists).
3. Confirm externally while the game runs: Task Manager → Details → `javaw.exe` → "Set affinity" dialog shows the expected checked cores; or `Get-Process javaw | select ProcessorAffinity` in PowerShell (mask should match, PowerShell prints a decimal — convert with hex).
4. Switch mode to "Off", relaunch, confirm no pinning line and unrestricted affinity.

**Step 3:** Final commit / push per repo workflow.

---

## Rollback / risk

- Feature is strictly additive: new module, two new optional serde fields (default `off`), one post-spawn call. Removing it = revert the commits.
- Failure modes are non-blocking by design (WARNING + launch continues).
- Single-processor-group limitation: irrelevant for consumer CPUs; document in the setting tooltip.

## Future work (explicitly out of scope, YAGNI now)

- AMD X3D auto-detection (L3 cache per CCD via `RelationCache`) — manual mask covers it today.
- Linux `sched_setaffinity` via `libc`.
- Pre-spawn pinning (`CREATE_SUSPENDED` + `SetProcessAffinityMask` + resume) to influence JVM's startup `availableProcessors` (GC thread count).
- Per-instance override in project settings (global launcher setting is enough for v1).
