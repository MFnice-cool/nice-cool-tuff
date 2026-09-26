<!-- Prism-style "Edit instance" manager: mods, backups and health in one
     dialog, opened from the Library side rail / context menus. Every
     mutation goes through an existing api.* command that owns its own
     auto-snapshot on the Rust side. -->
<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { open as openShell } from "@tauri-apps/plugin-shell";
  import {
    AlertTriangle,
    Archive,
    ArrowUpCircle,
    Camera,
    Check,
    CheckCircle2,
    Globe,
    ChevronDown,
    Copy,
    FolderOpen,
    HeartPulse,
    History,
    MoreVertical,
    Package,
    Play,
    Plus,
    Power,
    RefreshCw,
    RotateCcw,
    Search,
    Trash2,
    Wrench,
    X,
    XCircle,
  } from "@lucide/svelte";
  import { fade, fly } from "svelte/transition";
  import { quintOut } from "svelte/easing";
  import { api, type ModInfo } from "../lib/api";
  import type { RecentProject } from "../lib/store";
  import { toasts } from "../lib/toast";
  import { trapFocus } from "../lib/focusTrap";
  import { portal } from "../lib/portal";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import PromptDialog from "./PromptDialog.svelte";
  import { openModsBrowserWindow } from "../lib/modsBrowserWindow";
  import { refreshUpdateCount, setUpdateCount } from "../lib/instanceUpdates";
  import { get } from "svelte/store";
  import { t } from "../lib/i18n";
  import {
    isProjectLaunching,
    isProjectRunning,
    launchSessions,
    runningInstances,
  } from "../lib/store";
  import { launchWithFeedback } from "../lib/launch";
  import {
    formatBytes,
    formatDayStamp,
    formatStamp,
    matchesModFilter,
    mergeUpdates,
    sortMods,
    sourceChip,
    updateLabel,
    versionOptions,
    type ModSortMode,
    type ModUpdateInfo,
    type VersionOption,
  } from "../lib/instanceMods";

  let {
    project,
    initialTab = "mods",
    onclose,
    onBrowseMods,
  }: {
    project: RecentProject;
    initialTab?: ManagerTab;
    onclose?: () => void;
    onBrowseMods?: () => void;
  } = $props();

  type ManagerTab = "mods" | "worlds" | "shots" | "backups" | "health";
  type HealthReport = Awaited<ReturnType<typeof api.diagnostics.getPackHealth>>;
  type BackupEntry = Awaited<ReturnType<typeof api.backups.list>>[number];
  type UpdateRow = ModInfo & { hasUpdate?: boolean };

  const SORT_KEY = "tuffbox.manager.sort";
  const TAB_KEY = "tuffbox.manager.tab";

  function readStored<T extends string>(key: string, allowed: T[], fallback: T): T {
    try {
      const raw = localStorage.getItem(key);
      if (raw && (allowed as string[]).includes(raw)) return raw as T;
    } catch {
      /* ignore */
    }
    return fallback;
  }

  let tab = $state<ManagerTab>(
    readStored(TAB_KEY, ["mods", "worlds", "shots", "backups", "health"], "mods"),
  );
  // Apply before first paint; the modal remounts on each open so this runs once.
  $effect.pre(() => {
    tab = readStored(TAB_KEY, ["mods", "worlds", "shots", "backups", "health"], initialTab);
  });
  $effect(() => {
    try {
      localStorage.setItem(TAB_KEY, tab);
    } catch {
      /* ignore */
    }
  });

  const path = $derived(project.path);
  const loaderArg = $derived(
    !project.info.loaderKind || project.info.loaderKind === "vanilla"
      ? undefined
      : project.info.loaderKind,
  );

  // ── Mods tab ─────────────────────────────────────────────────────────
  let mods = $state<UpdateRow[]>([]);
  let modsLoading = $state(false);
  let modsLoadedOnce = $state(false);
  let modFilter = $state("");
  let modSort = $state<ModSortMode>(readStored(SORT_KEY, ["name", "source", "updates"], "name"));
  $effect(() => {
    try {
      localStorage.setItem(SORT_KEY, modSort);
    } catch {
      /* ignore */
    }
  });
  let updates: Record<string, ModUpdateInfo> = $state({});
  let updatesChecked = $state(false);
  let checkingUpdates = $state(false);
  let updatingAll = $state(false);
  let rowBusy = $state<string | null>(null);
  let overflowOpen = $state(false);
  let rowMenuId = $state<string | null>(null);
  let confirmRemoveMod: ModInfo | null = $state(null);
  let confirmRemoveMods: ModInfo[] | null = $state(null);

  // ── Multi-select + batch operations ──
  let selectedMods = $state<Set<string>>(new Set());
  let batchBusy = $state<{ label: string; done: number; total: number } | null>(null);
  /** Chip quick-filter: show only disabled mods / only ones with updates. */
  let quickFilter = $state<"none" | "disabled" | "updates">("none");
  let versionTarget: ModInfo | null = $state(null);
  let versionChoices: VersionOption[] = $state([]);

  const modRows = $derived(
    sortMods(
      mods
        .filter((m) => !m.contentType || m.contentType === "mod")
        .map((m) => ({ ...m, hasUpdate: !!updates[m.id] })),
      modSort,
    ),
  );
  const quickFiltered = $derived(
    quickFilter === "disabled"
      ? modRows.filter((m) => m.disabled)
      : quickFilter === "updates"
        ? modRows.filter((m) => m.hasUpdate)
        : modRows,
  );
  const visibleMods = $derived(quickFiltered.filter((m) => matchesModFilter(m, modFilter)));
  const disabledCount = $derived(modRows.filter((m) => m.disabled).length);
  const updateCount = $derived(modRows.filter((m) => m.hasUpdate).length);
  const selectedRows = $derived(visibleMods.filter((m) => selectedMods.has(m.id)));
  const selectedUpdateCount = $derived(selectedRows.filter((m) => updates[m.id]).length);

  // Keep the library badge truthful after in-manager checks and updates.
  $effect(() => {
    if (!path || !updatesChecked) return;
    setUpdateCount(path, updateCount);
  });

  function toggleModSelected(id: string) {
    const next = new Set(selectedMods);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selectedMods = next;
  }
  function selectAllFiltered() {
    selectedMods = new Set(visibleMods.map((m) => m.id));
  }
  function clearSelection() {
    selectedMods = new Set();
  }

  /** Runs op over ids sequentially with progress; toasts a failure summary. */
  async function runBatch(ids: string[], label: string, op: (id: string) => Promise<void>) {
    if (batchBusy || ids.length === 0) return;
    batchBusy = { label, done: 0, total: ids.length };
    let failed = 0;
    let firstError = "";
    for (let i = 0; i < ids.length; i++) {
      try {
        await op(ids[i]);
      } catch (e) {
        failed++;
        if (!firstError) firstError = String(e);
      }
      batchBusy = { label, done: i + 1, total: ids.length };
    }
    batchBusy = null;
    const L = get(t);
    if (failed > 0)
      toasts.error(L("manager.toastBatchFail", { label, n: failed, first: firstError }));
    else toasts.success(L("manager.toastBatchDone", { label, n: ids.length }));
  }

  async function batchEnable() {
    const ids = [...selectedMods];
    await runBatch(ids, "Enable", (id) => api.mods.enable(id, path).then(() => undefined));
    clearSelection();
    await loadMods(true);
  }
  async function batchDisable() {
    const ids = [...selectedMods];
    await runBatch(ids, "Disable", (id) => api.mods.disable(id, path).then(() => undefined));
    clearSelection();
    await loadMods(true);
  }
  async function batchUpdateSelected() {
    const ids = selectedRows.filter((m) => updates[m.id]).map((m) => m.id);
    await runBatch(ids, "Update", async (id) => {
      await api.mods.update(id, path, updates[id]?.versionId ?? undefined);
      delete updates[id];
      updates = { ...updates };
    });
    clearSelection();
    await loadMods(true);
  }
  async function batchRemoveMods(list: ModInfo[]) {
    confirmRemoveMods = null;
    await runBatch(
      list.map((m) => m.id),
      "Remove",
      (id) => api.mods.remove(id, path),
    );
    clearSelection();
    await loadMods(true);
  }

  async function loadMods(force = false) {
    if (modsLoading) return;
    if (modsLoadedOnce && !force) return;
    modsLoading = true;
    try {
      const list = await api.mods.list(path);
      mods = Array.isArray(list) ? list : [];
      modsLoadedOnce = true;
      quickFilter = "none";
    } catch (e) {
      toasts.error(get(t)("manager.toastReadFail", { e: String(e) }));
    } finally {
      modsLoading = false;
    }
  }

  async function checkUpdates() {
    if (checkingUpdates) return;
    checkingUpdates = true;
    try {
      const list = await api.mods.checkUpdates(path);
      updates = mergeUpdates(list);
      updatesChecked = true;
      if (updateCountOf(updates) === 0) toasts.success(get(t)("manager.toastAllUpToDate"));
    } catch (e) {
      toasts.error(get(t)("manager.toastCheckFail", { e: String(e) }));
    } finally {
      checkingUpdates = false;
    }
  }

  function updateCountOf(map: Record<string, ModUpdateInfo>): number {
    return Object.keys(map).length;
  }

  async function updateAll() {
    if (updatingAll) return;
    updatingAll = true;
    try {
      const res = await api.mods.updateAll(path, false);
      const updated = Array.isArray(res.updated) ? res.updated.length : 0;
      const errors = Array.isArray(res.errors) ? res.errors : [];
      const L = get(t);
      if (errors.length > 0) {
        toasts.warning(
          L("library.toastUpdatePartial", {
            ok: updated,
            failed: errors.length,
            first: errors[0],
          }),
        );
      } else if (updated === 1) {
        toasts.success(L("library.toastUpdated1"));
      } else if (updated > 0) {
        toasts.success(L("library.toastUpdatedN", { n: updated }));
      } else {
        toasts.success(L("library.toastNothingToUpdate"));
      }
      updates = {};
      updatesChecked = false;
      if (errors.length > 0) {
        // Partial failure — re-check for the real remaining count.
        void refreshUpdateCount(path, true);
      } else {
        setUpdateCount(path, 0);
      }
      await loadMods(true);
    } catch (e) {
      toasts.error(get(t)("library.toastUpdateFailed", { e: String(e) }));
    } finally {
      updatingAll = false;
    }
  }

  async function updateOne(mod: UpdateRow) {
    const upd = updates[mod.id];
    if (!upd || rowBusy) return;
    rowBusy = mod.id;
    try {
      await api.mods.update(mod.id, path, upd.versionId ?? undefined);
      toasts.success(get(t)("manager.toastModUpdated", { name: mod.name }));
      delete updates[mod.id];
      updates = { ...updates };
      await loadMods(true);
    } catch (e) {
      toasts.error(get(t)("library.toastUpdateFailed", { e: String(e) }));
    } finally {
      rowBusy = null;
    }
  }

  async function toggleMod(mod: UpdateRow) {
    if (rowBusy) return;
    rowBusy = mod.id;
    try {
      if (mod.disabled) await api.mods.enable(mod.id, path);
      else await api.mods.disable(mod.id, path);
      await loadMods(true);
    } catch (e) {
      toasts.error(String(e));
    } finally {
      rowBusy = null;
    }
  }

  async function removeMod(mod: ModInfo) {
    confirmRemoveMod = null;
    if (rowBusy) return;
    rowBusy = mod.id;
    try {
      await api.mods.remove(mod.id, path);
      toasts.success(get(t)("manager.toastModRemoved", { name: mod.name }));
      await loadMods(true);
    } catch (e) {
      toasts.error(get(t)("manager.toastRemoveFail", { e: String(e) }));
    } finally {
      rowBusy = null;
    }
  }

  async function openVersionPicker(mod: ModInfo) {
    rowMenuId = null;
    versionTarget = mod;
    versionChoices = [];
    try {
      const list = await api.mods.getVersions(mod.id, project.info.minecraftVersion, loaderArg);
      const opts = versionOptions(Array.isArray(list) ? list : [], mod.version);
      if (opts.length === 0) {
        toasts.error(get(t)("manager.toastNoVersions"));
        versionTarget = null;
        return;
      }
      versionChoices = opts;
    } catch (e) {
      toasts.error(get(t)("manager.toastVersionLookupFail", { e: String(e) }));
      versionTarget = null;
    }
  }

  async function applyVersion(label: string) {
    const mod = versionTarget;
    const choice = versionChoices.find((v) => v.label === label);
    versionTarget = null;
    if (!mod || !choice || rowBusy) return;
    rowBusy = mod.id;
    try {
      await api.mods.changeVersion(mod.id, choice.id, path);
      toasts.success(`${mod.name} → ${choice.label.split(" · ")[0]}`);
      await loadMods(true);
    } catch (e) {
      toasts.error(get(t)("manager.toastVersionFail", { e: String(e) }));
    } finally {
      rowBusy = null;
    }
  }

  async function syncFolder() {
    overflowOpen = false;
    try {
      const res = await api.mods.syncFolder(path);
      toasts.success(
        get(t)("manager.toastSynced", { n: Array.isArray(res) ? res.length : 0 }),
      );
      await loadMods(true);
    } catch (e) {
      toasts.error(String(e));
    }
  }

  async function openModsFolder() {
    overflowOpen = false;
    try {
      await api.files.openFolder(path, "mods");
    } catch (e) {
      toasts.error(String(e));
    }
  }

  function browseMods() {
    overflowOpen = false;
    onBrowseMods?.();
    onclose?.();
  }

  /** Open the standalone content browser window; fall back to the Discover
      tab when separate windows are unavailable. */
  async function addMods() {
    overflowOpen = false;
    if (await openModsBrowserWindow(path, "mod")) return;
    browseMods();
  }

  // ── Backups tab ──────────────────────────────────────────────────────
  let backups = $state<BackupEntry[]>([]);
  let backupsLoading = $state(false);
  let backupsLoadedOnce = $state(false);
  let creatingBackup = $state(false);
  let confirmRestore: BackupEntry | null = $state(null);
  let confirmDeleteBackup: BackupEntry | null = $state(null);

  async function loadBackups(force = false) {
    if (backupsLoading) return;
    if (backupsLoadedOnce && !force) return;
    backupsLoading = true;
    try {
      backups = await api.backups.list(path);
      backupsLoadedOnce = true;
    } catch (e) {
      toasts.error(get(t)("manager.toastBackupListFail", { e: String(e) }));
    } finally {
      backupsLoading = false;
    }
  }

  async function createBackup() {
    if (creatingBackup) return;
    creatingBackup = true;
    try {
      const entry = await api.backups.create(null, path);
      toasts.success(get(t)("manager.toastBackupCreated", { name: entry.name }));
      await loadBackups(true);
    } catch (e) {
      toasts.error(get(t)("manager.toastBackupFail", { e: String(e) }));
    } finally {
      creatingBackup = false;
    }
  }

  async function restoreBackup(entry: BackupEntry) {
    confirmRestore = null;
    try {
      await api.backups.restore(entry.id, path);
      toasts.success(get(t)("manager.toastRestored", { name: entry.name }));
      updates = {};
      updatesChecked = false;
      // The restored pack is a different state — re-check its updates.
      void refreshUpdateCount(path, true);
      await Promise.all([loadMods(true), loadBackups(true)]);
    } catch (e) {
      toasts.error(get(t)("manager.toastRestoreFail", { e: String(e) }));
    }
  }

  async function deleteBackup(entry: BackupEntry) {
    confirmDeleteBackup = null;
    try {
      await api.backups.delete(entry.id, path);
      backups = backups.filter((b) => b.id !== entry.id);
      toasts.success(get(t)("manager.toastBackupDeleted"));
    } catch (e) {
      toasts.error(get(t)("manager.toastDeleteFail", { e: String(e) }));
    }
  }

  // ── Worlds tab ───────────────────────────────────────────────────────
  type WorldRow = Awaited<ReturnType<typeof api.worlds.list>>[number];
  let worlds = $state<WorldRow[]>([]);
  let worldsLoading = $state(false);
  let worldsLoadedOnce = $state(false);
  let worldIcons = $state<Record<string, string>>({});
  let worldBusy = $state<string | null>(null);
  let confirmDeleteWorld: WorldRow | null = $state(null);

  async function loadWorlds(force = false) {
    if (worldsLoading) return;
    if (worldsLoadedOnce && !force) return;
    worldsLoading = true;
    try {
      worlds = await api.worlds.list(path);
      worldsLoadedOnce = true;
      void loadWorldIcons();
    } catch (e) {
      toasts.error(get(t)("manager.toastSavesFail", { e: String(e) }));
    } finally {
      worldsLoading = false;
    }
  }

  async function loadWorldIcons() {
    // Parallel in small chunks — a 30-world saves folder used to serialize
    // into 30 sequential IPC round-trips before the last icon appeared.
    const pending = worlds.filter((w) => !worldIcons[w.name]);
    const CHUNK = 6;
    for (let i = 0; i < pending.length; i += CHUNK) {
      const batch = pending.slice(i, i + CHUNK);
      const loaded = await Promise.all(
        batch.map(async (w) => {
          try {
            return [w.name, await api.worlds.readIcon(w.name, path)] as const;
          } catch {
            return [w.name, null] as const;
          }
        }),
      );
      const next = { ...worldIcons };
      let dirty = false;
      for (const [name, icon] of loaded) {
        if (icon) {
          next[name] = icon;
          dirty = true;
        }
      }
      if (dirty) worldIcons = next;
    }
  }

  async function playWorld(world: WorldRow) {
    if (worldBusy) return;
    if (isProjectRunning(path, $runningInstances) || isProjectLaunching(path, $launchSessions)) {
      toasts.warning(get(t)("manager.toastStopFirst"));
      return;
    }
    worldBusy = world.name;
    try {
      await launchWithFeedback({
        path,
        profile: "client",
        quickPlayType: "world",
        quickPlayValue: world.name,
      });
    } catch (e) {
      toasts.error(get(t)("manager.toastLaunchFail", { e: String(e) }));
    } finally {
      worldBusy = null;
    }
  }

  async function backupWorld(world: WorldRow) {
    worldBusy = `backup:${world.name}`;
    try {
      const file = await api.worlds.backup(world.name, path);
      toasts.success(get(t)("manager.toastWorldBackedUp", { file }));
    } catch (e) {
      toasts.error(get(t)("manager.toastBackupFail", { e: String(e) }));
    } finally {
      worldBusy = null;
    }
  }

  async function deleteWorld(world: WorldRow) {
    confirmDeleteWorld = null;
    worldBusy = world.name;
    try {
      await api.worlds.delete(world.name, true, path);
      toasts.success(
        get(t)("manager.toastWorldDeleted", { name: world.displayName || world.name }),
      );
      worldIcons = { ...worldIcons, [world.name]: undefined } as Record<string, string>;
      await loadWorlds(true);
    } catch (e) {
      toasts.error(get(t)("manager.toastDeleteFail", { e: String(e) }));
    } finally {
      worldBusy = null;
    }
  }

  function openSavesFolder() {
    void api.files
      .openFolder(path, "saves")
      .catch((e) => toasts.error(String(e)));
  }

  // ── Screenshots tab ──────────────────────────────────────────────────
  let shots = $state<Awaited<ReturnType<typeof api.worlds.listScreenshots>>>([]);
  let shotsLoading = $state(false);
  let shotsLoadedOnce = $state(false);
  let confirmDeleteShot: (typeof shots)[number] | null = $state(null);
  /** In-app full preview — replaces jumping straight to the system viewer. */
  let lightbox = $state<(typeof shots)[number] | null>(null);

  $effect(() => {
    if (!lightbox) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") lightbox = null;
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  async function deleteFromLightbox() {
    const shot = lightbox;
    lightbox = null;
    if (shot) await deleteShot(shot);
  }

  function openFromLightbox() {
    const shot = lightbox;
    if (shot) void openShot(shot);
  }

  async function loadShots(force = false) {
    if (shotsLoading) return;
    if (shotsLoadedOnce && !force) return;
    shotsLoading = true;
    try {
      shots = await api.worlds.listScreenshots(path);
      shotsLoadedOnce = true;
    } catch (e) {
      toasts.error(get(t)("manager.toastShotsFail", { e: String(e) }));
    } finally {
      shotsLoading = false;
    }
  }

  async function deleteShot(shot: (typeof shots)[number]) {
    confirmDeleteShot = null;
    try {
      await api.worlds.deleteScreenshot(shot.fileName, path);
      shots = shots.filter((s) => s.fileName !== shot.fileName);
      toasts.success(get(t)("manager.toastShotDeleted"));
    } catch (e) {
      toasts.error(get(t)("manager.toastDeleteFail", { e: String(e) }));
    }
  }

  function shotUrl(absPath: string): string {
    return convertFileSrc(absPath);
  }

  async function openShot(shot: (typeof shots)[number]) {
    try {
      await openShell(shot.path);
    } catch (e) {
      toasts.error(String(e));
    }
  }

  function openScreenshotsFolder() {
    void api.files
      .openFolder(path, "screenshots")
      .catch((e) => toasts.error(String(e)));
  }

  // ── Health tab ───────────────────────────────────────────────────────
  let health = $state<HealthReport | null>(null);
  let healthLoading = $state(false);
  let healthLoadedOnce = $state(false);
  let validating = $state(false);
  let validation = $state<Record<string, unknown> | null>(null);
  let keepingDup = $state<string | null>(null);

  async function loadHealth(force = false) {
    if (healthLoading) return;
    if (healthLoadedOnce && !force) return;
    healthLoading = true;
    try {
      health = await api.diagnostics.getPackHealth(path);
      healthLoadedOnce = true;
    } catch (e) {
      toasts.error(get(t)("manager.toastHealthFail", { e: String(e) }));
    } finally {
      healthLoading = false;
    }
  }

  async function keepOneDuplicate(modId: string, keepFileName: string) {
    if (keepingDup) return;
    keepingDup = modId;
    try {
      await api.mods.keepOneDuplicateModJar(modId, keepFileName, path);
      toasts.success(get(t)("manager.toastKept", { name: keepFileName }));
      health = null;
      healthLoadedOnce = false;
      await Promise.all([loadHealth(true), loadMods(true)]);
    } catch (e) {
      toasts.error(String(e));
    } finally {
      keepingDup = null;
    }
  }

  async function runValidation() {
    if (validating) return;
    validating = true;
    try {
      validation = await api.project.runValidation(path);
    } catch (e) {
      toasts.error(get(t)("manager.toastValidationFail", { e: String(e) }));
    } finally {
      validating = false;
    }
  }

  function num(v: unknown): number {
    return typeof v === "number" ? v : 0;
  }

  // Tab drives lazy loading — each pane loads itself once.
  $effect(() => {
    if (tab === "mods") void loadMods();
    else if (tab === "worlds") void loadWorlds();
    else if (tab === "shots") void loadShots();
    else if (tab === "backups") void loadBackups();
    else if (tab === "health") void loadHealth();
  });

  // Close floating menus on any outside pointerdown (library-wide pattern).
  $effect(() => {
    function onGlobalPointerDown(e: MouseEvent) {
      const t = e.target as HTMLElement | null;
      if (overflowOpen && !t?.closest?.(".im-overflow-wrap")) overflowOpen = false;
      if (rowMenuId && !t?.closest?.(".im-row-menu-wrap")) rowMenuId = null;
    }
    window.addEventListener("pointerdown", onGlobalPointerDown, true);
    return () => window.removeEventListener("pointerdown", onGlobalPointerDown, true);
  });

  const overallMeta = $derived.by(() => {
    const overall = health?.overall ?? "healthy";
    if (overall === "errors") return { cls: "bad", label: "Errors found", Icon: XCircle };
    if (overall === "warnings") return { cls: "warn", label: "Warnings", Icon: AlertTriangle };
    return { cls: "ok", label: "Healthy", Icon: CheckCircle2 };
  });

  /** Normalized view of the loosely-typed run_project_validation payload. */
  const validationSummary = $derived.by(() => {
    if (!validation) return null;
    const errList = Array.isArray(validation.graphErrorList)
      ? (validation.graphErrorList as unknown[]).map(String)
      : [];
    const errors = num(validation.jsonErrors) + num(validation.graphErrors);
    const warnings =
      num(validation.graphWarnings) + num(validation.modsWithoutHash) + num(validation.modsWithoutSource);
    return {
      passed: validation.passed === true && errors === 0,
      totalMods: num(validation.totalMods),
      totalProfiles: num(validation.totalProfiles),
      errors,
      warnings,
      errorPreview: errList.slice(0, 5),
      errorMore: Math.max(0, errList.length - 5),
    };
  });
</script>

<div
  class="im-backdrop"
  use:portal
  style="position:fixed; inset:0; z-index:10000;"
  role="presentation"
  transition:fade={{ duration: 140 }}
  onclick={(e) => e.target === e.currentTarget && onclose?.()}
  onkeydown={(e) => e.key === "Enter" && onclose?.()}
>
  <div
    class="im-modal"
    role="dialog"
    aria-modal="true"
    aria-labelledby="im-title"
    use:trapFocus={{ onEscape: () => onclose?.() }}
    transition:fly={{ y: 16, duration: 200, opacity: 0, easing: quintOut }}
  >
    <header class="im-head">
      <div
        class="im-hero-icon"
        style={`background: linear-gradient(135deg, var(--accent-primary), var(--accent-secondary, var(--accent-primary)))`}
        aria-hidden="true"
      >
        {project.info.name[0]?.toUpperCase() ?? "?"}
      </div>
      <div class="im-head-copy">
        <h2 id="im-title" title={project.info.name}>{project.info.name}</h2>
        <span class="im-head-meta">
          {project.info.minecraftVersion} · {project.info.loaderKind}
        </span>
      </div>
      <button type="button" class="im-close" onclick={() => onclose?.()} aria-label={$t("manager.closeManager")}>
        <X size={17} />
      </button>
    </header>

    <div class="im-tabs" role="tablist" aria-label={$t("manager.sections")}>
      <button
        type="button"
        role="tab"
        aria-selected={tab === "mods"}
        class:active={tab === "mods"}
        onclick={() => (tab = "mods")}
      >
        <Package size={14} /> {$t("manager.mods")}
        {#if modsLoadedOnce}
          <span class="im-tab-badge">{modRows.length}</span>
          {#if updateCount > 0}
            <span class="im-tab-badge update" title={$t("manager.updatesAvailable")}>{updateCount}</span>
          {/if}
        {/if}
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={tab === "worlds"}
        class:active={tab === "worlds"}
        onclick={() => (tab = "worlds")}
      >
        <Globe size={14} /> {$t("manager.worlds")}
        {#if worldsLoadedOnce && worlds.length > 0}
          <span class="im-tab-badge">{worlds.length}</span>
        {/if}
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={tab === "shots"}
        class:active={tab === "shots"}
        onclick={() => (tab = "shots")}
      >
        <Camera size={14} /> {$t("manager.screenshots")}
        {#if shotsLoadedOnce && shots.length > 0}
          <span class="im-tab-badge">{shots.length}</span>
        {/if}
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={tab === "backups"}
        class:active={tab === "backups"}
        onclick={() => (tab = "backups")}
      >
        <Archive size={14} /> {$t("manager.backups")}
        {#if backupsLoadedOnce && backups.length > 0}
          <span class="im-tab-badge">{backups.length}</span>
        {/if}
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={tab === "health"}
        class:active={tab === "health"}
        onclick={() => (tab = "health")}
      >
        <HeartPulse size={14} /> {$t("manager.health")}
        {#if healthLoadedOnce && health}
          <span class="im-tab-dot {overallMeta.cls}" aria-hidden="true"></span>
        {/if}
      </button>
    </div>

    <div class="im-body">
      <!-- ════════════════════════ MODS ════════════════════════ -->
      {#if tab === "mods"}
        <div class="im-toolbar">
          <div class="im-search">
            <Search size={14} />
            <input
              type="text"
              placeholder={$t("manager.filterMods")}
              aria-label={$t("manager.filterModsAria")}
              bind:value={modFilter}
              maxlength={120}
            />
          </div>
          <select
            class="im-sort"
            aria-label={$t("manager.sortMods")}
            value={modSort}
            onchange={(e) => (modSort = (e.currentTarget as HTMLSelectElement).value as ModSortMode)}
          >
            <option value="name">{$t("manager.sortName")}</option>
            <option value="source">{$t("manager.sortSource")}</option>
            <option value="updates">{$t("manager.sortUpdates")}</option>
          </select>
          <button
            type="button"
            class="im-tool-btn"
            title={$t("manager.refreshMods")}
            aria-label={$t("manager.refreshMods")}
            disabled={modsLoading}
            onclick={() => void loadMods(true)}
          >
            <RefreshCw size={14} class={modsLoading ? "spin" : ""} />
          </button>
          <div class="im-overflow-wrap">
            <button
              type="button"
              class="im-tool-btn"
              title={$t("manager.moreModActions")}
              aria-label={$t("manager.moreModActions")}
              aria-expanded={overflowOpen}
              onclick={() => (overflowOpen = !overflowOpen)}
            >
              <MoreVertical size={14} />
            </button>
            {#if overflowOpen}
              <div class="im-menu" role="menu">
                <button type="button" role="menuitem" onclick={() => { overflowOpen = false; void checkUpdates(); }} disabled={checkingUpdates}>
                  <RefreshCw size={13} /> {$t("manager.checkUpdates")}
                </button>
                <button
                  type="button"
                  role="menuitem"
                  onclick={() => { overflowOpen = false; void updateAll(); }}
                  disabled={updatingAll || !updatesChecked || updateCount === 0}
                >
                  <ArrowUpCircle size={13} /> {$t("manager.updateAll")}{#if updateCount > 0}&nbsp;({updateCount}){/if}
                </button>
                <button type="button" role="menuitem" onclick={() => void addMods()}>
                  <Plus size={13} /> {$t("manager.addMods")}
                </button>
                <button type="button" role="menuitem" onclick={() => void syncFolder()}>
                  <RefreshCw size={13} /> {$t("manager.syncFolder")}
                </button>
                <button type="button" role="menuitem" onclick={() => void openModsFolder()}>
                  <FolderOpen size={13} /> {$t("manager.openModsFolder")}
                </button>
              </div>
            {/if}
          </div>
        </div>

        {#if selectedMods.size > 0 || batchBusy}
          <div class="im-batchbar" role="toolbar" aria-label={$t("manager.batchActions")}>
            {#if batchBusy}
              <span class="im-batch-status">
                <RefreshCw size={13} class="spin" />
                {batchBusy.label} {batchBusy.done}/{batchBusy.total}…
              </span>
            {:else}
              <span class="im-batch-count">{$t("manager.selected", { n: selectedMods.size })}</span>
              <button type="button" class="im-mini" disabled={batchBusy !== null} onclick={() => void batchEnable()}>
                <Power size={12} /> {$t("manager.enable")}
              </button>
              <button type="button" class="im-mini" disabled={batchBusy !== null} onclick={() => void batchDisable()}>
                <Power size={12} /> {$t("manager.disable")}
              </button>
              <button
                type="button"
                class="im-mini"
                disabled={batchBusy !== null || selectedUpdateCount === 0}
                title={selectedUpdateCount === 0
                  ? $t("manager.noUpdatesSelected")
                  : $t("manager.updateN", { n: selectedUpdateCount })}
                onclick={() => void batchUpdateSelected()}
              >
                <ArrowUpCircle size={12} /> {$t("manager.update")}{#if selectedUpdateCount > 0}&nbsp;({selectedUpdateCount}){/if}
              </button>
              <button
                type="button"
                class="im-mini danger"
                disabled={batchBusy !== null}
                onclick={() => (confirmRemoveMods = selectedRows)}
              >
                <Trash2 size={12} /> {$t("manager.remove")}
              </button>
              <span class="im-spacer"></span>
              <button type="button" class="im-mini" disabled={batchBusy !== null} onclick={selectAllFiltered}>{$t("manager.all")}</button>
              <button type="button" class="im-mini" disabled={batchBusy !== null} onclick={clearSelection}>{$t("manager.clear")}</button>
            {/if}
          </div>
        {/if}

        {#if modsLoadedOnce}
          <div class="im-chips" role="toolbar" aria-label={$t("manager.quickFilters")}>
            <button
              type="button"
              class="im-chip chip-btn"
              class:active-chip={quickFilter === "none"}
              title={quickFilter === "none" ? $t("manager.allMods") : $t("manager.clearQuickFilter")}
              aria-pressed={quickFilter === "none"}
              onclick={() => (quickFilter = "none")}
            >
              {modRows.length} {modRows.length === 1 ? $t("manager.modWord") : $t("manager.modsWord")}
            </button>
            {#if disabledCount > 0}
              <button
                type="button"
                class="im-chip chip-btn muted"
                class:active-chip={quickFilter === "disabled"}
                title={quickFilter === "disabled"
                  ? $t("manager.showingDisabled")
                  : $t("manager.showDisabled")}
                aria-pressed={quickFilter === "disabled"}
                onclick={() => (quickFilter = quickFilter === "disabled" ? "none" : "disabled")}
              >
                {disabledCount} {$t("manager.disabledWord")}
              </button>
            {/if}
            {#if updatesChecked}
              <button
                type="button"
                class="im-chip chip-btn"
                class:warn={updateCount > 0}
                class:active-chip={quickFilter === "updates"}
                disabled={updateCount === 0 && quickFilter !== "updates"}
                title={quickFilter === "updates"
                  ? $t("manager.showingUpdates")
                  : $t("manager.showUpdates")}
                aria-pressed={quickFilter === "updates"}
                onclick={() => (quickFilter = quickFilter === "updates" ? "none" : "updates")}
              >
                {updateCount === 0 ? $t("manager.upToDate") : $t("manager.updatesCount", { n: updateCount })}
              </button>
            {/if}
          </div>
        {/if}

        {#if modsLoading && !modsLoadedOnce}
          <div class="im-empty" in:fade>{$t("manager.readingMods")}</div>
        {:else if !modsLoadedOnce}
          <div class="im-empty">{$t("manager.modsError")}</div>
        {:else if visibleMods.length === 0}
          <div class="im-empty" in:fade>
            {#if modFilter.trim()}
              {$t("manager.noModsMatch", { filter: modFilter.trim() })}
            {:else}
              {$t("manager.noMods")}
              <button type="button" class="im-link" onclick={() => void addMods()}>{$t("manager.openCatalog")}</button>
            {/if}
          </div>
        {:else}
          <ul class="im-rows" in:fade>
            {#each visibleMods as mod (mod.id)}
              <li class="im-row" class:disabled-row={mod.disabled} class:selected-row={selectedMods.has(mod.id)}>
                <button
                  type="button"
                  role="checkbox"
                  aria-checked={selectedMods.has(mod.id)}
                  aria-label={`Select ${mod.name}`}
                  class="im-check"
                  class:checked={selectedMods.has(mod.id)}
                  onclick={() => toggleModSelected(mod.id)}
                >
                  {#if selectedMods.has(mod.id)}<Check size={12} />{/if}
                </button>
                <span
                  class="im-row-icon"
                  style={mod.iconUrl ? "background: var(--bg-tertiary)" : `background: linear-gradient(135deg, hsl(${(mod.name.length * 47) % 360} 45% 42%), hsl(${(mod.name.length * 47 + 40) % 360} 45% 30%))`}
                  aria-hidden="true"
                >
                  {#if mod.iconUrl}
                    <img class="im-row-img" src={mod.iconUrl} alt="" loading="lazy" />
                  {:else}
                    {mod.name[0]?.toUpperCase() ?? "?"}
                  {/if}
                </span>
                <div class="im-row-main">
                  <span class="im-row-name" title={mod.fileName ?? mod.name}>{mod.name}</span>
                  <span class="im-row-sub">
                    {mod.version || "?"} · {sourceChip(mod.source)}
                    {#if mod.side && mod.side !== "both"}· {mod.side}{/if}
                  </span>
                </div>
                {#if updates[mod.id]}
                  <button
                    type="button"
                    class="im-update-badge"
                    class:breaking={updates[mod.id].breakingLoader || updates[mod.id].breakingMinecraft}
                    title={`Update to ${updates[mod.id].latestVersion}${updates[mod.id].breakingLoader || updates[mod.id].breakingMinecraft ? " (may break — check loader/MC support)" : ""}`}
                    disabled={rowBusy === mod.id}
                    onclick={() => void updateOne(mod)}
                  >
                    <ArrowUpCircle size={12} />
                    {updateLabel(updates[mod.id])}
                  </button>
                {:else if mod.disabled}
                  <span class="im-state-chip">disabled</span>
                {/if}
                <div class="im-row-menu-wrap">
                  <button
                    type="button"
                    class="im-tool-btn small"
                    title={mod.disabled ? "Enable mod" : "Disable mod"}
                    aria-label={mod.disabled ? `Enable ${mod.name}` : `Disable ${mod.name}`}
                    disabled={rowBusy === mod.id}
                    onclick={() => void toggleMod(mod)}
                  >
                    <Power size={13} />
                  </button>
                  <button
                    type="button"
                    class="im-tool-btn small"
                    title={$t("common.more")}
                    aria-label={`More actions for ${mod.name}`}
                    aria-expanded={rowMenuId === mod.id}
                    disabled={rowBusy === mod.id}
                    onclick={() => (rowMenuId = rowMenuId === mod.id ? null : mod.id)}
                  >
                    <MoreVertical size={13} />
                  </button>
                  {#if rowMenuId === mod.id}
                    <div class="im-menu right" role="menu">
                      {#if updates[mod.id]}
                        <button type="button" role="menuitem" onclick={() => { rowMenuId = null; void updateOne(mod); }}>
                          <ArrowUpCircle size={13} /> Update to {updates[mod.id].latestVersion}
                        </button>
                      {/if}
                      <button type="button" role="menuitem" onclick={() => void openVersionPicker(mod)}>
                        <History size={13} /> {$t("manager.changeVersion")}
                      </button>
                      <button type="button" role="menuitem" class="danger" onclick={() => { rowMenuId = null; confirmRemoveMod = mod; }}>
                        <Trash2 size={13} /> {$t("manager.removeMod")}
                      </button>
                    </div>
                  {/if}
                </div>
              </li>
            {/each}
          </ul>
        {/if}

        <!-- ════════════════════════ WORLDS ════════════════════════ -->
      {:else if tab === "worlds"}
        <div class="im-toolbar">
          <button type="button" class="im-primary" onclick={openSavesFolder}>
            <FolderOpen size={13} /> Open saves folder
          </button>
          <span class="im-toolbar-hint">{$t("manager.worldsHint")}</span>
          <span class="im-spacer"></span>
          <button
            type="button"
            class="im-tool-btn"
            title={$t("manager.refreshWorlds")}
            aria-label={$t("manager.refreshWorlds")}
            disabled={worldsLoading}
            onclick={() => void loadWorlds(true)}
          >
            <RefreshCw size={14} class={worldsLoading ? "spin" : ""} />
          </button>
        </div>

        {#if worldsLoading && !worldsLoadedOnce}
          <div class="im-empty" in:fade>{$t("manager.readingWorlds")}</div>
        {:else if worlds.length === 0}
          <div class="im-empty" in:fade>
            No worlds yet — create one in game (Singleplayer), it will show up here.
          </div>
        {:else}
          <ul class="im-rows" in:fade>
            {#each worlds as world (world.name)}
              <li class="im-row">
                <span class="im-row-icon" aria-hidden="true">
                  {#if worldIcons[world.name]}
                    <img class="im-world-img" src={worldIcons[world.name]} alt="" />
                  {:else}
                    {(world.displayName || world.name)[0]?.toUpperCase() ?? "?"}
                  {/if}
                </span>
                <div class="im-row-main">
                  <span class="im-row-name" title={world.displayName || world.name}>
                    {world.displayName || world.name}
                  </span>
                  <span class="im-row-sub">
                    {world.sizeFormatted} · last played {formatDayStamp(world.lastPlayed ?? null)}
                  </span>
                  <span class="im-world-chips">
                    {#if world.gameType}{world.gameType}{/if}
                    {#if world.difficulty} · {world.difficulty}{/if}
                    {#if world.hardcore} · hardcore{/if}
                    {#if !world.hasLevelDat} · unreadable level.dat{/if}
                  </span>
                </div>
                <div class="im-row-actions">
                  <button
                    type="button"
                    class="im-mini"
                    title={$t("manager.launchWorld")}
                    disabled={worldBusy === world.name || worldBusy?.startsWith("backup:") || !world.hasLevelDat}
                    onclick={() => void playWorld(world)}
                  >
                    {#if worldBusy === world.name}<RefreshCw size={12} class="spin" />{:else}<Play size={12} />{/if}
                    Play
                  </button>
                  <button
                    type="button"
                    class="im-mini"
                    disabled={!!worldBusy}
                    title={$t("manager.zipWorld")}
                    onclick={() => void backupWorld(world)}
                  >
                    {#if worldBusy === `backup:${world.name}`}<RefreshCw size={12} class="spin" />{:else}<Archive size={12} />{/if}
                    Backup
                  </button>
                  <button
                    type="button"
                    class="im-tool-btn small"
                    title={$t("manager.deleteWorldTitle")}
                    aria-label={$t("manager.deleteWorldAria", { name: world.displayName || world.name })}
                    disabled={!!worldBusy}
                    onclick={() => (confirmDeleteWorld = world)}
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              </li>
            {/each}
          </ul>
        {/if}

        <!-- ════════════════════════ SCREENSHOTS ════════════════════════ -->
      {:else if tab === "shots"}
        <div class="im-toolbar">
          <button type="button" class="im-primary" onclick={openScreenshotsFolder}>
            <FolderOpen size={13} /> Open folder
          </button>
          <span class="im-toolbar-hint">{$t("manager.shotsHint")}</span>
          <span class="im-spacer"></span>
          <button
            type="button"
            class="im-tool-btn"
            title={$t("manager.refreshShots")}
            aria-label={$t("manager.refreshShots")}
            disabled={shotsLoading}
            onclick={() => void loadShots(true)}
          >
            <RefreshCw size={14} class={shotsLoading ? "spin" : ""} />
          </button>
        </div>

        {#if shotsLoading && !shotsLoadedOnce}
          <div class="im-empty" in:fade>{$t("manager.listingShots")}</div>
        {:else if shots.length === 0}
          <div class="im-empty" in:fade>
            No screenshots yet — press F2 in game and they will appear here.
          </div>
        {:else}
          <div class="im-shots" in:fade>
            {#each shots as shot (shot.fileName)}
              <figure class="im-shot">
                <button
                  type="button"
                  class="im-shot-frame"
                  title={$t("manager.preview")}
                  aria-label={`Preview ${shot.fileName}`}
                  onclick={() => (lightbox = shot)}
                >
                  <img src={shotUrl(shot.path)} alt={shot.fileName} loading="lazy" />
                </button>
                <figcaption class="im-shot-cap">
                  <span class="im-shot-name" title={shot.fileName}>{shot.fileName}</span>
                  <span class="im-shot-meta">{formatDayStamp(shot.modifiedMs)} · {shot.sizeFormatted}</span>
                  <button
                    type="button"
                    class="im-tool-btn small"
                    title={$t("manager.deleteShot")}
                    aria-label={`Delete ${shot.fileName}`}
                    onclick={() => (confirmDeleteShot = shot)}
                  >
                    <Trash2 size={13} />
                  </button>
                </figcaption>
              </figure>
            {/each}
          </div>
        {/if}

        <!-- ════════════════════════ BACKUPS ════════════════════════ -->
      {:else if tab === "backups"}
        <div class="im-toolbar">
          <button
            type="button"
            class="im-primary"
            disabled={creatingBackup}
            onclick={() => void createBackup()}
          >
            {#if creatingBackup}<RefreshCw size={13} class="spin" />{:else}<Plus size={13} />{/if}
            Create backup
          </button>
          <span class="im-toolbar-hint">{$t("manager.backupsHint")}</span>
          <span class="im-spacer"></span>
          <button
            type="button"
            class="im-tool-btn"
            title={$t("manager.refreshBackups")}
            aria-label={$t("manager.refreshBackups")}
            disabled={backupsLoading}
            onclick={() => void loadBackups(true)}
          >
            <RefreshCw size={14} class={backupsLoading ? "spin" : ""} />
          </button>
        </div>

        {#if backupsLoading && !backupsLoadedOnce}
          <div class="im-empty" in:fade>{$t("manager.loadingBackups")}</div>
        {:else if backups.length === 0}
          <div class="im-empty" in:fade>
            No backups yet. Create one before trying risky changes — updates and
            imports snapshot automatically, manual edits don't.
          </div>
        {:else}
          <ul class="im-rows" in:fade>
            {#each backups as entry (entry.id)}
              <li class="im-row">
                <span class="im-row-icon backup" aria-hidden="true"><Archive size={16} /></span>
                <div class="im-row-main">
                  <span class="im-row-name" title={entry.name}>{entry.name}</span>
                  <span class="im-row-sub">
                    {formatStamp(entry.createdAt)} · {formatBytes(entry.sizeBytes)} · {entry.fileCount} files
                  </span>
                </div>
                <div class="im-row-actions">
                  <button
                    type="button"
                    class="im-mini"
                    disabled={creatingBackup}
                    onclick={() => (confirmRestore = entry)}
                  >
                    <RotateCcw size={12} /> {$t("manager.restore")}
                  </button>
                  <button
                    type="button"
                    class="im-tool-btn small"
                    title={$t("manager.deleteBackup")}
                    aria-label={`Delete backup ${entry.name}`}
                    onclick={() => (confirmDeleteBackup = entry)}
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              </li>
            {/each}
          </ul>
        {/if}

        <!-- ════════════════════════ HEALTH ════════════════════════ -->
      {:else}
        <div class="im-toolbar">
          <button
            type="button"
            class="im-primary"
            disabled={validating}
            onclick={() => void runValidation()}
          >
            {#if validating}<RefreshCw size={13} class="spin" />{:else}<Wrench size={13} />{/if}
            Run validation
          </button>
          <span class="im-toolbar-hint">{$t("manager.healthHint")}</span>
          <span class="im-spacer"></span>
          <button
            type="button"
            class="im-tool-btn"
            title={$t("manager.refreshHealth")}
            aria-label={$t("manager.refreshHealth")}
            disabled={healthLoading}
            onclick={() => void loadHealth(true)}
          >
            <RefreshCw size={14} class={healthLoading ? "spin" : ""} />
          </button>
        </div>

        {#if healthLoading && !healthLoadedOnce}
          <div class="im-empty" in:fade>{$t("manager.scanningPack")}</div>
        {:else if !health}
          <div class="im-empty">{$t("manager.healthError")}</div>
        {:else}
          <div class="im-health" in:fade>
            <div class={`im-verdict ${overallMeta.cls}`}>
              <overallMeta.Icon size={18} />
              <div>
                <strong>{overallMeta.label}</strong>
                <span>
                  {health.diagnostics.errors} errors · {health.diagnostics.warnings} warnings
                </span>
              </div>
            </div>

            <div class="im-tiles">
              <div class="im-tile" class:bad={health.wrongLoaderCount > 0} title={$t("manager.wrongLoader")}>
                <strong>{health.wrongLoaderCount}</strong>
                <span>{$t("manager.wrongLoaderWord")}</span>
              </div>
              <div class="im-tile" class:bad={health.duplicateGroups.length > 0} title={$t("manager.duplicateJarsTitle")}>
                <strong>{health.duplicateGroups.length}</strong>
                <span>{$t("manager.dupGroups")}</span>
              </div>
              <div class="im-tile" class:bad={health.questIssues > 0} title={$t("manager.questIssuesTitle")}>
                <strong>{health.questIssues}</strong>
                <span>{$t("manager.questIssuesWord")}</span>
              </div>
              <div class="im-tile" class:bad={!!health.lastCrash} title={health.lastCrash ? `Last crash ${formatStamp(health.lastCrash.at)} (exit ${health.lastCrash.exitCode ?? "?"})` : "No crashes recorded"}>
                <strong>{health.lastCrash ? "yes" : "no"}</strong>
                <span>{$t("manager.recentCrash")}</span>
              </div>
            </div>

            {#if health.exportIssues.length > 0}
              <div class="im-section-title">{$t("manager.exportIssues")}</div>
              <ul class="im-rows compact">
                {#each health.exportIssues.slice(0, 12) as issue}
                  <li class="im-issue" class:severe={issue.severity === "error"}>
                    <AlertTriangle size={13} />
                    <span class="im-issue-text" title={`${issue.code}: ${issue.message}`}>{issue.message}</span>
                    <span class="im-issue-code">{issue.code}</span>
                  </li>
                {/each}
                {#if health.exportIssues.length > 12}
                  <li class="im-issue-more">{$t("manager.andMore", { n: health.exportIssues.length - 12 })}</li>
                {/if}
              </ul>
            {/if}

            {#if health.duplicateGroups.length > 0}
              <div class="im-section-title">{$t("manager.duplicateJars")}</div>
              <ul class="im-rows compact">
                {#each health.duplicateGroups as dup (dup.modId)}
                  <li class="im-issue">
                    <Copy size={13} />
                    <span class="im-issue-text">
                      {$t("manager.jarsOf", { n: dup.count, mod: dup.modId })}
                    </span>
                    <button
                      type="button"
                      class="im-mini"
                      disabled={keepingDup === dup.modId}
                      title={$t("manager.keepRest", { name: dup.keepCandidate })}
                      onclick={() => void keepOneDuplicate(dup.modId, dup.keepCandidate)}
                    >
                      {$t("manager.keep", {
                        name: dup.keepCandidate.length > 18 ? `${dup.keepCandidate.slice(0, 18)}…` : dup.keepCandidate,
                      })}
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}

            {#if validationSummary}
              <div class="im-section-title">{$t("manager.lastValidation")}</div>
              <div class={`im-verdict ${validationSummary.passed ? "ok" : validationSummary.errors > 0 ? "bad" : "warn"}`}>
                {#if validationSummary.passed}
                  <CheckCircle2 size={18} />
                {:else}
                  <AlertTriangle size={18} />
                {/if}
                <div>
                  <strong>{validationSummary.passed ? $t("manager.validationPassed") : $t("manager.issuesFound")}</strong>
                  <span>
                    {validationSummary.totalMods} mods · {validationSummary.totalProfiles} profiles ·
                    {validationSummary.errors} errors · {validationSummary.warnings} warnings
                  </span>
                </div>
              </div>
              {#if validationSummary.errorPreview.length > 0}
                <ul class="im-rows compact">
                  {#each validationSummary.errorPreview as err}
                    <li class="im-issue severe"><XCircle size={13} /><span class="im-issue-text">{err}</span></li>
                  {/each}
                  {#if validationSummary.errorMore > 0}
                    <li class="im-issue-more">…and {validationSummary.errorMore} more</li>
                  {/if}
                </ul>
              {/if}
            {/if}
          </div>
        {/if}
      {/if}
    </div>
  </div>
</div>

{#if confirmRemoveMod}
  <ConfirmDialog
    title={$t("manager.removeMod")}
    message={$t("manager.removeModMsg", { name: confirmRemoveMod.name })}
    confirmLabel={$t("manager.remove")}
    danger
    onconfirm={() => confirmRemoveMod && void removeMod(confirmRemoveMod)}
    oncancel={() => (confirmRemoveMod = null)}
  />
{/if}

{#if confirmRemoveMods && confirmRemoveMods.length > 0}
  <ConfirmDialog
    title={$t("manager.removeMods")}
    message={$t("manager.removeModsMsg", { n: confirmRemoveMods.length })}
    confirmLabel={$t("manager.remove")}
    danger
    onconfirm={() => void batchRemoveMods(confirmRemoveMods!)}
    oncancel={() => (confirmRemoveMods = null)}
  />
{/if}

{#if confirmDeleteWorld}
  <ConfirmDialog
    title={$t("manager.deleteWorld")}
    message={$t("manager.deleteWorldMsg", { name: confirmDeleteWorld.displayName || confirmDeleteWorld.name })}
    confirmLabel={$t("common.delete")}
    danger
    onconfirm={() => confirmDeleteWorld && void deleteWorld(confirmDeleteWorld)}
    oncancel={() => (confirmDeleteWorld = null)}
  />
{/if}

{#if lightbox}
  <div
    class="im-lightbox"
    use:portal
    style="position:fixed; inset:0; z-index:10001;"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    aria-label={`Screenshot ${lightbox.fileName}`}
    transition:fade={{ duration: 120 }}
    onclick={(e) => e.target === e.currentTarget && (lightbox = null)}
    onkeydown={(e) => e.key === "Enter" && (lightbox = null)}
  >
    <img class="im-lightbox-img" src={shotUrl(lightbox.path)} alt={lightbox.fileName} />
    <div class="im-lightbox-bar">
      <span class="im-lightbox-name" title={lightbox.fileName}>{lightbox.fileName}</span>
      <span class="im-lightbox-meta">{formatDayStamp(lightbox.modifiedMs)} · {lightbox.sizeFormatted}</span>
      <span class="im-spacer"></span>
      <button type="button" class="im-mini" onclick={openFromLightbox}>
        <FolderOpen size={12} /> {$t("manager.openInViewer")}
      </button>
      <button type="button" class="im-mini danger" onclick={() => void deleteFromLightbox()}>
        <Trash2 size={12} /> {$t("common.delete")}
      </button>
      <button type="button" class="im-mini" onclick={() => (lightbox = null)}>{$t("common.close")}</button>
    </div>
  </div>
{/if}

{#if confirmDeleteShot}
  <ConfirmDialog
    title={$t("manager.deleteShot")}
    message={$t("manager.deleteShotMsg", { name: confirmDeleteShot.fileName })}
    confirmLabel={$t("common.delete")}
    danger
    onconfirm={() => confirmDeleteShot && void deleteShot(confirmDeleteShot)}
    oncancel={() => (confirmDeleteShot = null)}
  />
{/if}

{#if confirmRestore}
  <ConfirmDialog
    title={$t("manager.restoreBackup")}
    message={$t("manager.restoreBackupMsg", {
      name: confirmRestore.name,
      stamp: formatStamp(confirmRestore.createdAt),
    })}
    confirmLabel={$t("manager.restore")}
    danger
    onconfirm={() => confirmRestore && void restoreBackup(confirmRestore)}
    oncancel={() => (confirmRestore = null)}
  />
{/if}

{#if confirmDeleteBackup}
  <ConfirmDialog
    title={$t("manager.deleteBackup")}
    message={$t("manager.deleteBackupMsg", { name: confirmDeleteBackup.name })}
    confirmLabel={$t("common.delete")}
    danger
    onconfirm={() => confirmDeleteBackup && void deleteBackup(confirmDeleteBackup)}
    oncancel={() => (confirmDeleteBackup = null)}
  />
{/if}

{#if versionTarget}
  <PromptDialog
    title={$t("manager.changeVersionTitle")}
    message={versionChoices.length === 0 ? "Loading versions…" : `Versions of "${versionTarget.name}" for ${project.info.minecraftVersion}`}
    mode="select"
    options={versionChoices.map((v) => v.label)}
    confirmLabel={$t("manager.install")}
    onconfirm={(label) => void applyVersion(label)}
    oncancel={() => (versionTarget = null)}
  />
{/if}

<style>
  /* Backdrop: plain dim — no backdrop-filter so glass/compositing rules and
     potato-pc stay untouched. */
  .im-backdrop {
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
  }
  .im-modal {
    display: flex;
    flex-direction: column;
    width: min(720px, 100%);
    max-height: min(640px, 100%);
    border-radius: var(--border-radius-lg);
    border: 1px solid var(--border-color);
    background: var(--bg-primary);
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.45);
    overflow: hidden;
  }

  .im-head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 16px;
    border-bottom: 1px solid var(--border-color);
  }
  .im-hero-icon {
    width: 42px;
    height: 42px;
    border-radius: var(--border-radius-md);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 19px;
    font-weight: 900;
    color: #fff;
    flex-shrink: 0;
  }
  .im-head-copy {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .im-head-copy h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 800;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .im-head-meta {
    font-size: 12px;
    color: var(--text-muted);
  }
  .im-close {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    transform: none;
  }
  .im-close:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .im-tabs {
    display: flex;
    gap: 8px;
    padding: 8px 12px 0;
    border-bottom: 1px solid var(--border-color);
  }
  .im-tabs [role="tab"] {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border: none;
    border-bottom: 2px solid transparent;
    border-radius: var(--border-radius-sm) var(--border-radius-sm) 0 0;
    background: transparent;
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
  }
  .im-tabs [role="tab"]:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }
  .im-tabs [role="tab"].active {
    color: var(--accent-primary);
    border-bottom-color: var(--accent-primary);
  }
  .im-tab-badge {
    display: inline-flex;
    align-items: center;
    height: 18px;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--bg-tertiary);
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 800;
  }
  .im-tab-badge.update {
    background: var(--accent-warning);
    color: #1a1a1a;
  }
  .im-tab-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .im-tab-dot.ok {
    background: var(--accent-primary);
  }
  .im-tab-dot.warn {
    background: var(--accent-warning);
  }
  .im-tab-dot.bad {
    background: var(--accent-danger);
  }

  .im-body {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    overflow-y: auto;
    min-height: 220px;
  }

  .im-toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .im-search {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex: 1 1 180px;
    min-width: 150px;
    height: 30px;
    padding: 0 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
    color: var(--text-muted);
  }
  .im-search input {
    flex: 1;
    min-width: 0;
    border: none;
    outline: none;
    background: transparent;
    color: var(--text-primary);
    font-size: 12px;
  }
  .im-sort {
    height: 30px;
    padding: 0 8px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    font-size: 12px;
    cursor: pointer;
  }
  .im-tool-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    cursor: pointer;
    transform: none;
    flex-shrink: 0;
  }
  .im-tool-btn:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent-primary) 40%, var(--border-color));
    color: var(--text-primary);
  }
  .im-tool-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .im-tool-btn.small {
    width: 26px;
    height: 26px;
  }
  .im-primary {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 12px;
    border: none;
    border-radius: var(--border-radius-md);
    background: var(--accent-primary);
    color: var(--on-accent, #fff);
    font-size: 12px;
    font-weight: 800;
    cursor: pointer;
    transform: none;
  }
  .im-primary:disabled {
    opacity: 0.6;
    cursor: wait;
  }
  .im-primary:hover:not(:disabled) {
    filter: brightness(1.08);
  }
  .im-toolbar-hint {
    font-size: 12px;
    color: var(--text-muted);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .im-spacer {
    flex: 1;
  }

  .im-overflow-wrap,
  .im-row-menu-wrap {
    position: relative;
    display: inline-flex;
  }
  .im-menu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 20;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: 4px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-primary);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.35);
  }
  .im-menu button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 9px;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    text-align: left;
    cursor: pointer;
    white-space: nowrap;
  }
  .im-menu button:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .im-menu button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .im-menu button.danger {
    color: var(--accent-danger);
  }

  .im-chips {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .im-chip {
    display: inline-flex;
    align-items: center;
    height: 20px;
    padding: 0 9px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--accent-primary) 25%, var(--border-color));
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 800;
  }
  .im-chip.muted {
    border-color: var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-muted);
  }
  .im-chip.warn {
    border-color: color-mix(in srgb, var(--accent-warning) 45%, transparent);
    background: color-mix(in srgb, var(--accent-warning) 14%, transparent);
    color: var(--text-primary);
  }

  .im-rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .im-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 9px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
  }
  .im-row:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 25%, var(--border-color));
  }
  .im-row.disabled-row {
    opacity: 0.62;
  }
  .im-row-icon {
    width: 30px;
    height: 30px;
    border-radius: var(--border-radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 13px;
    font-weight: 900;
    color: #fff;
    flex-shrink: 0;
  }
  .im-row-icon.backup {
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }
  .im-row-main {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
    flex: 1;
  }
  .im-row-name {
    font-size: 13px;
    font-weight: 700;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .im-row-sub {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .im-row-actions {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }
  .im-state-chip {
    display: inline-flex;
    align-items: center;
    height: 18px;
    padding: 0 7px;
    border-radius: 999px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 700;
    flex-shrink: 0;
  }
  .im-update-badge {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 22px;
    padding: 0 8px;
    border: 1px solid color-mix(in srgb, var(--accent-warning) 55%, transparent);
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent-warning) 16%, transparent);
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 800;
    white-space: nowrap;
    cursor: pointer;
    flex-shrink: 0;
    transform: none;
  }
  .im-update-badge.breaking {
    border-color: color-mix(in srgb, var(--accent-danger) 55%, transparent);
    background: color-mix(in srgb, var(--accent-danger) 14%, transparent);
  }
  .im-update-badge:hover:not(:disabled) {
    filter: brightness(1.08);
  }
  .im-update-badge:disabled {
    opacity: 0.6;
    cursor: wait;
  }
  .im-mini {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 24px;
    padding: 0 9px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 700;
    white-space: nowrap;
    cursor: pointer;
    transform: none;
  }
  .im-mini:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent-primary) 40%, var(--border-color));
    color: var(--text-primary);
  }
  .im-mini:disabled {
    opacity: 0.55;
    cursor: wait;
  }

  .im-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 28px 16px;
    border: 1px dashed var(--border-color);
    border-radius: var(--border-radius-md);
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
  }
  .im-link {
    border: none;
    background: transparent;
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
    text-decoration: underline;
    padding: 0;
  }

  .im-health {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .im-verdict {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
  }
  .im-verdict > div {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .im-verdict strong {
    font-size: 13px;
    color: var(--text-primary);
  }
  .im-verdict span {
    font-size: 12px;
    color: var(--text-muted);
  }
  .im-verdict.ok :global(svg) {
    color: var(--accent-primary);
  }
  .im-verdict.warn :global(svg) {
    color: var(--accent-warning);
  }
  .im-verdict.bad :global(svg) {
    color: var(--accent-danger);
  }
  .im-verdict.ok {
    border-color: color-mix(in srgb, var(--accent-primary) 30%, var(--border-color));
  }
  .im-verdict.warn {
    border-color: color-mix(in srgb, var(--accent-warning) 35%, var(--border-color));
  }
  .im-verdict.bad {
    border-color: color-mix(in srgb, var(--accent-danger) 35%, var(--border-color));
  }
  .im-tiles {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
  }
  .im-tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 8px 4px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
  }
  .im-tile strong {
    font-size: 15px;
    color: var(--text-primary);
  }
  .im-tile span {
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
  }
  .im-tile.bad {
    border-color: color-mix(in srgb, var(--accent-danger) 40%, var(--border-color));
  }
  .im-tile.bad strong {
    color: var(--accent-danger);
  }
  .im-section-title {
    margin-top: 4px;
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .im-issue {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 8px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
    background: var(--bg-secondary);
    color: var(--accent-warning);
    font-size: 12px;
    list-style: none;
  }
  .im-issue.severe {
    color: var(--accent-danger);
  }
  .im-issue-text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-secondary);
  }
  .im-issue-text strong {
    color: var(--text-primary);
  }
  .im-issue-code {
    font-size: 12px;
    color: var(--text-muted);
    flex-shrink: 0;
  }
  .im-issue-more {
    list-style: none;
    padding: 2px 8px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .im-row-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: inherit;
  }
  /* Chips double as quick-filter toggles; the count chip clears them. */
  .im-chips .im-chip.active-chip {
    border-color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 16%, transparent);
  }
  .chip-btn {
    cursor: pointer;
    font: inherit;
    font-size: 12px;
    font-weight: 800;
    transform: none;
  }
  .chip-btn:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, var(--border-color));
  }
  .chip-btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .chip-btn.warn.active-chip {
    border-color: var(--accent-warning);
  }

  /* Full-screen screenshot preview. */
  .im-lightbox {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 24px;
    background: rgba(0, 0, 0, 0.82);
  }
  .im-lightbox-img {
    max-width: min(1200px, 100%);
    max-height: calc(100vh - 110px);
    object-fit: contain;
    border-radius: var(--border-radius-md);
    box-shadow: 0 18px 64px rgba(0, 0, 0, 0.6);
  }
  .im-lightbox-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    width: min(900px, 100%);
  }
  .im-lightbox-name {
    font-size: 12px;
    font-weight: 700;
    color: #fff;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .im-lightbox-meta {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.72);
    white-space: nowrap;
  }

  /* Multi-select checkbox + batch action bar. */
  .im-check {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
    background: var(--bg-tertiary);
    color: transparent;
    cursor: pointer;
    flex-shrink: 0;
    transform: none;
  }
  .im-check:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, var(--border-color));
  }
  .im-check.checked {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
    color: var(--on-accent, #fff);
  }
  .im-row.selected-row {
    border-color: color-mix(in srgb, var(--accent-primary) 40%, var(--border-color));
    background: color-mix(in srgb, var(--accent-primary) 6%, var(--bg-secondary));
  }
  .im-batchbar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    padding: 7px 10px;
    border: 1px solid color-mix(in srgb, var(--accent-primary) 35%, var(--border-color));
    border-radius: var(--border-radius-md);
    background: color-mix(in srgb, var(--accent-primary) 7%, var(--bg-secondary));
  }
  .im-batch-count {
    font-size: 12px;
    font-weight: 800;
    color: var(--accent-primary);
    white-space: nowrap;
  }
  .im-batch-status {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    font-weight: 700;
    color: var(--text-secondary);
  }
  .im-mini.danger {
    border-color: color-mix(in srgb, var(--accent-danger) 40%, var(--border-color));
    color: var(--accent-danger);
  }
  .im-mini.danger:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-danger) 10%, var(--bg-tertiary));
  }

  /* Worlds: icon thumbnails + metadata chips. */
  .im-world-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: inherit;
  }
  .im-world-chips {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Screenshots: responsive grid, fixed-height frames, caption row. */
  .im-shots {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 10px;
  }
  .im-shot {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0;
    padding: 6px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
  }
  .im-shot-frame {
    display: block;
    width: 100%;
    aspect-ratio: 16 / 9;
    padding: 0;
    border: none;
    border-radius: var(--border-radius-sm);
    overflow: hidden;
    background: var(--bg-tertiary);
    cursor: zoom-in;
  }
  .im-shot-frame img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .im-shot-frame:hover img {
    filter: brightness(1.08);
  }
  .im-shot-cap {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .im-shot-name {
    flex: 1;
    min-width: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .im-shot-meta {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .spin {
    animation: im-spin 0.9s linear infinite;
  }
  @keyframes im-spin {
    to {
      transform: rotate(360deg);
    }
  }
  :global(.potato-pc) .spin {
    animation: none;
  }

  @media (max-width: 560px) {
    .im-tiles {
      grid-template-columns: repeat(2, 1fr);
    }
  }
</style>
