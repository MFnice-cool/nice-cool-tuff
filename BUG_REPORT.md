# Bug Report — TuffBox Desktop Frontend

**Date:** 2026-07-20  
**Scope:** `apps/tuffbox-desktop/src/` — all Svelte components + lib files  
**Verified:** All findings double-checked against source code

---

## Bug 1 — Infinite retry loop: OreGenVisualizer `loadWorlds` never marks failed path

**File:** `src/components/OreGenVisualizer.svelte`  
**Lines:** 59–71, 168  
**Type:** Infinite loop  
**Severity:** HIGH  

```js
let lastWorldsPath: string | null = null;

async function loadWorlds() {
  if (!$projectPath) return;
  worldsLoading = true;
  worldsError = null;
  try {
    worlds = (await invoke("list_worlds", { path: $projectPath }));
    lastWorldsPath = $projectPath;       // ← only set on SUCCESS
  } catch (e) {
    worldsError = String(e);             // ← lastWorldsPath NOT set
  } finally {
    worldsLoading = false;
  }
}

$: if ($projectPath && $projectPath !== lastWorldsPath && !worldsLoading) loadWorlds();
```

When `invoke("list_worlds")` throws, `lastWorldsPath` is never updated. The reactive statement re-evaluates: `$projectPath !== lastWorldsPath` is still `true` and `worldsLoading` is `false` (reset in `finally`), so `loadWorlds()` fires again immediately. This loops indefinitely, spamming the backend with failed calls.

**Fix:** Move `lastWorldsPath = $projectPath;` into the `finally` block so it's set regardless of success/failure.

---

## Bug 2 — Stale state: Dashboard and Library `launching` flag resets instantly

**File:** `src/lib/launch.ts`, `src/components/LibraryInstancesPane.svelte`, `src/components/IdeNextBar.svelte`, `src/components/Diagnostics.svelte`  
**Type:** State bug  
**Severity:** MEDIUM  
**Status:** ✅ RESOLVED — shared launch state machine (see checklist below)

`launch_profile` is fire-and-forget — it spawns the Minecraft process and returns immediately. The old pattern set `launching = true`, awaited the invoke, then cleared it in a `finally` block within milliseconds, so the "Launching..." spinner flashed and vanished without ever reflecting whether the game actually came up.

**Fix (implemented):** A single shared launch state machine in `src/lib/launch.ts` is now the single source of truth for "launching". It is driven by backend lifecycle events — `process-started`, `process-exited`, `launch-phase`, `launch-crashed` — so a path stays in the *launching* state (phases `preparing` → `resolving_java` → `downloading` → `starting`) until the backend confirms `running`, and clears only on `running` / `exited` / a failed invoke. `launchWithFeedback` no longer resets state in a `finally`; callers must not either. The Rust backend now emits explicit `launch-phase` events alongside the existing `process-started` / `process-exited` events.

**Status: FIXED (2026-08-11).**

Launch state is now owned by one shared lifecycle contract instead of component-local
`finally` blocks. The backend emits `launch-phase` (`preflight` →
`resolving_java` → `downloading` → `starting` → `running` → `exited` / `failed`)
and `process-exited`; `list_running_instances` remains the startup/reconnect source
of truth. A Stop request stays visible as `stopping` until the child wait callback
reports the actual exit.

**Play-surface closure checklist:**

- [x] Dashboard — derives Launching / Running / Stop from shared lifecycle + process registry.
- [x] Library / `LibraryInstancesPane` — per-instance Launching persists through Running or Failed; Stop uses `kill_running_instance`.
- [x] Sidebar rail — Play switches to Stop for the active tracked instance.
- [x] IDE Next Bar — no local `finally` reset; observes the shared session.
- [x] Diagnose Test launch — observes lifecycle and links directly to the Live log / crash findings.
- [x] Test Runs — launch lab consumes the same lifecycle for launch/exit truth; polling is telemetry only.
- [x] Instance Home Quick Play / Join, Recipe Browser JEI Live, Optimize Test launch, command-palette and shortcut launches — all route through `lib/launch.ts`.
- [x] `LaunchLogModal` — labels and tails the backend-resolved live source (`latest.log` when ready, otherwise TuffBox console).

**Regression rule:** UI code must call `launchWithFeedback` / `killWithFeedback`; it
must not invoke `launch_profile`, `launch_with_quick_play`, `launch_server`, or
`kill_running_instance` directly, and it must not use a local `finally` to decide
whether Minecraft is still launching.

**Checklist — every Play path verified against the shared store (no local `finally` reset):**

| Play button / path | File | Uses shared store |
|---|---|---|
| Global `isLaunching` spinner (Header, Dashboard Play/Stop, Sidebar) | `Dashboard.svelte`, `Sidebar.svelte` | ✅ derived from `launchingPath` |
| Library instance Launch | `LibraryInstancesPane.svelte` | ✅ `$launchingPath === selected.path` |
| IDE "Next" rail Play | `IdeNextBar.svelte` | ✅ `$derived($launchingPath === $projectPath)` |
| Diagnostics "Test launch" | `Diagnostics.svelte` | ✅ `$derived($launchingPath === $projectPath)` |
| CLI `--launch` shortcut / command palette test-launch | `App.svelte` | ✅ via `launchWithFeedback` |
| `TestRuns.svelte` | own `livePhase` machine (reference UX) | ✅ additive, still feeds the store |

Stop buttons use `kill_running_instance` → backend `process-exited` → shared store clears `launching`/`running`.

---

## Bug 3 — Memory leak: WorldMap `flashTimer` not cleaned on destroy

**File:** `src/components/WorldMap.svelte`  
**Lines:** 347–353  
**Type:** Memory leak  
**Severity:** LOW  

```js
let flashMsg: string | null = null;
let flashTimer: any;
function flash(msg: string) {
  flashMsg = msg;
  clearTimeout(flashTimer);
  flashTimer = setTimeout(() => (flashMsg = null), 2500);
}
```

No `onDestroy` cleanup for `flashTimer`. If the component unmounts while a flash message is active (e.g. after "Deleted chunks"), the timeout callback fires and tries to set `flashMsg` on a destroyed component. Svelte won't crash, but the closure retains a reference to the destroyed instance.

**Fix:** Add `import { onDestroy } from "svelte"` and `onDestroy(() => clearTimeout(flashTimer))`.

---

## Bug 4 — Race condition: SkinPreview3D uncancelled async `loadSkin`

**File:** `src/components/SkinPreview3D.svelte`  
**Lines:** 53–69  
**Type:** Race condition  
**Severity:** LOW  

```js
$: if (viewer && skinUrl) {
  loadSkin(skinUrl);  // async, not awaited by reactive
}
```

The reactive block calls `loadSkin` (async) without awaiting. If `skinUrl` changes rapidly (e.g. user switches accounts quickly), multiple concurrent `loadSkin` calls overlap. The older in-flight fetch may complete after the newer one, overwriting the correct skin with a stale one. Also, if the component is destroyed mid-fetch, `viewer.loadSkin(dataUrl)` could operate on a disposed WebGL context.

**Fix:** Use a generation counter or AbortController to cancel stale loads. Increment a counter before each call; if the counter changed when the fetch resolves, discard the result.

---

## Bug 5 — Theme desync: Settings reads localStorage, not the store

**File:** `src/components/Settings.svelte`  
**Lines:** 24, 194–198  
**Type:** State desync  
**Severity:** LOW  

```js
let theme = localStorage.getItem("tuffbox-theme") || "dark";

function toggleTheme() {
  theme = theme === "dark" ? "light" : "dark";
  localStorage.setItem("tuffbox-theme", theme);
  document.documentElement.setAttribute("data-theme", theme);
}
```

The theme is initialized from `localStorage` directly, not from a reactive store. `App.svelte` also reads from `localStorage` on mount to set the DOM attribute. When the user toggles the theme in Settings, only the DOM `data-theme` attribute is updated — no Svelte store is notified. Sidebar/Header/etc. don't react because they don't observe a store. This works only because CSS variables respond to the DOM attribute, but it's fragile and un-Svelte-like.

**Fix:** Create a `theme` writable store in `store.ts` and have both Settings and App.svelte subscribe to it.

---

## Bug 6 — Timing: ExportBuilder `loadDefaultPaths` runs before `$projectInfo` loads

**File:** `src/components/ExportBuilder.svelte`  
**Lines:** 25–34, 80  
**Type:** Timing / null safety  
**Severity:** LOW  

```js
$: onProjectPathChange($projectPath);

async function loadDefaultPaths(path: string) {
  projectDir = await invoke("get_project_dir", { path });
  const id = $projectInfo?.id ?? "modpack";
  const version = $projectInfo?.version ?? "1.0.0";
  // ...sets default filenames using id and version
}
```

`loadDefaultPaths` is called reactively when `$projectPath` changes. But `$projectInfo` is loaded asynchronously in `App.svelte`'s `onMount` and may not be available yet. The `?.` operator prevents a crash, but the default filenames use fallback values ("modpack" / "1.0.0") even when the real project info arrives milliseconds later. The paths won't update when `projectInfo` finishes loading.

**Fix:** Make `loadDefaultPaths` reactive to both `$projectPath` and `$projectInfo`, or call it again when `$projectInfo` changes.

---

## Bug 7 — Performance: RecipeBrowser `filteredCounts` built twice on initial load

**File:** `src/components/RecipeBrowser.svelte`  
**Lines:** ~595–600, ~623–625  
**Type:** Redundant computation  
**Severity:** LOW  

```js
// In rebuildIndexes():
filteredCounts = buildFilteredCounts(recipes, categoryFilter, modFilter);

// Reactive block that also fires on initial load:
$: if (recipes.length && categoryFilter && modFilter) {
  filteredCounts = buildFilteredCounts(recipes, categoryFilter, modFilter);
}
```

On initial load, `rebuildIndexes()` builds `filteredCounts`, then the reactive block fires and rebuilds it again with the same arguments. The computation is duplicated.

**Fix:** Remove the redundant reactive block, or gate it to skip if `rebuildIndexes` already ran in the same tick.

---

## Bug 8 — Unused import: Dashboard imports `onDestroy` but never calls it

**File:** `src/components/Dashboard.svelte`  
**Line:** 2  
**Type:** Dead code  
**Severity:** INFO  

```js
import { onMount, onDestroy } from "svelte";
```

`onDestroy` is imported but never used anywhere in the component. This is dead code — possibly a leftover from a previous iteration that had event listeners needing cleanup.

**Fix:** Remove `onDestroy` from the import statement.

---

## Summary

| # | File | Bug Type | Severity |
|---|------|----------|----------|
| 1 | OreGenVisualizer.svelte | Infinite retry loop on failed `loadWorlds` | **HIGH** |
| 2 | Dashboard / Library / all Play surfaces | **FIXED:** shared launch lifecycle replaces fire-and-forget local flags | MEDIUM ✅ RESOLVED |
| 3 | WorldMap.svelte | `flashTimer` not cleaned on destroy | LOW |
| 4 | SkinPreview3D.svelte | Uncancelled async `loadSkin` race condition | LOW |
| 5 | Settings.svelte | Theme desync (localStorage, no store) | LOW |
| 6 | ExportBuilder.svelte | `loadDefaultPaths` runs before `$projectInfo` loads | LOW |
| 7 | RecipeBrowser.svelte | `filteredCounts` built twice on initial load | LOW |
| 8 | Dashboard.svelte | Unused `onDestroy` import | INFO |
