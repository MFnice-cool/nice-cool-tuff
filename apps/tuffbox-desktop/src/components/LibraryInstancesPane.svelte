<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import DedupAskDialog from "./DedupAskDialog.svelte";
  import { fade, fly, slide } from "svelte/transition";
  import { quintOut } from "svelte/easing";
  import {
    Plus,
    Folder,
    FolderOpen,
    Settings,
    HelpCircle,
    RefreshCw,
    Play,
    Square,
    Tags,
    Share2,
    Copy,
    Trash2,
    Link2,
    ChevronDown,
    ChevronRight,
    Package,
  ArrowUpCircle,
  Pencil,
    Wrench,
  SlidersHorizontal,
    Minus,
    ImageIcon,
    Eraser,
    Search,
    X,
    Compass,
    Server,
    LayoutGrid,
    List,
  } from "@lucide/svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open as openDialog, confirm } from "@tauri-apps/plugin-dialog";
  import { open as openShell } from "@tauri-apps/plugin-shell";
  import {
    recentProjects,
    projectPath,
    projectInfo,
    ideStageRequest,
    openAddInstance,
    runningInstances,
    isProjectRunning,
    launchSessions,
    isProjectLaunching,
    formatPlaytime,
    authState,
    skinPath,
    loginTypeLabel,
    loginModalOpen,
    uiScalePercentLive,
    libraryTabRequest,
    type RecentProject,
  } from "../lib/store";
  import { homeIcons, homeStats } from "../lib/homeBootstrap";
  import { toasts } from "../lib/toast";
  import { listen } from "@tauri-apps/api/event";
  import { api, githubInspectMeta } from "../lib/api";
  import { copyText } from "../lib/clipboard";
  import { launchWithFeedback, killWithFeedback, launchingPath } from "../lib/launch";
  import {
    DEFAULT_GROUP,
    loadGroupMap,
    setGroup,
    getGroup,
    loadCollapsedGroups,
    toggleCollapsed,
    listGroupNames,
    folderFromDrop,
    type GroupMap,
  } from "../lib/libraryGroups";
  import { getNote, loadNotes, setNote } from "../lib/libraryNotes";
import { refreshUpdateCount, updateCounts } from "../lib/instanceUpdates";
import { get } from "svelte/store";
import { t } from "../lib/i18n";
  import {
    isValidSortMode,
    matchesInstanceFilter,
    sortInstances,
    type SortMode,
  } from "../lib/librarySort";
  import { portal } from "../lib/portal";
  import PromptDialog from "./PromptDialog.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import InstanceManager from "./InstanceManager.svelte";
  import GithubPackInstallProgress from "./GithubPackInstallProgress.svelte";
  import HeadAvatar from "./HeadAvatar.svelte";
  import LibraryInstanceContent from "./LibraryInstanceContent.svelte";

  let {
    currentView = $bindable(),
    toolbarLeading,
  }: {
    currentView: "dashboard" | "ide" | "mods" | "graph" | "diagnostics" | "snapshots" | "configs" | "settings" | "project-settings" | "ore-gen" | "recipes" | "quests" | "library" | "chats" | "me" | "world";
    /** Optional leading toolbar slot (Library tabs render here). */
    toolbarLeading?: import("svelte").Snippet;
  } = $props();

  const LONG_PRESS_MS = 420;
  const MOVE_CANCEL_PX = 10;

  let selectedPath = $state<string | null>($projectPath);
  /** Per-instance content (mods/packs/shaders/servers) — collapsed by default
      so the Library side rail stays quiet until needed. */
  const CONTENT_COLLAPSE_KEY = "tuffbox-library-content-collapsed";
  let contentCollapsed = $state(
    typeof localStorage === "undefined"
      ? true
      : localStorage.getItem(CONTENT_COLLAPSE_KEY) !== "false",
  );
  function toggleContentCollapsed() {
    contentCollapsed = !contentCollapsed;
    try {
      localStorage.setItem(CONTENT_COLLAPSE_KEY, String(contentCollapsed));
    } catch {
      /* ignore */
    }
  }
  let actionBusy = $state(false);
  let exportMenuOpen = $state(false);
  let addMenuOpen = $state(false);
  let moreMenuOpen = $state(false);
  let githubImportOpen = $state(false);
  let githubConfirmOpen = $state(false);
  let githubInstallActive = $state(false);
  let githubPendingSource = $state("");
  let githubInspectSummary = $state("");
  let foldersMenuOpen = $state(false);
  let instanceFilter = $state("");
  /** Library sort mode; persisted in localStorage. */
  const SORT_KEY = "tuffbox.library.sort";
  let sortMode = $state<SortMode>(
    ((): SortMode => {
      try {
        const v = localStorage.getItem(SORT_KEY);
        return isValidSortMode(v) ? v : "recent";
      } catch {
        return "recent";
      }
    })(),
  );
  function setSortMode(m: SortMode) {
    sortMode = m;
    try {
      localStorage.setItem(SORT_KEY, m);
    } catch {
      /* ignore */
    }
  }
  let groupMap = $state<GroupMap>(loadGroupMap());
  let collapsed = $state(loadCollapsedGroups());
  let projectStats = $state<Record<string, { playtime: number; lastLaunch: string | null }>>({});
  let refreshing = $state(false);

  /** Grid (cover tiles) vs list (dense rows); persisted. */
  const VIEW_KEY = "tuffbox.library.view";
  let viewMode = $state<"grid" | "list">(
    ((): "grid" | "list" => {
      try {
        return localStorage.getItem(VIEW_KEY) === "list" ? "list" : "grid";
      } catch {
        return "grid";
      }
    })(),
  );
  function setViewMode(m: "grid" | "list") {
    viewMode = m;
    try {
      localStorage.setItem(VIEW_KEY, m);
    } catch {
      /* ignore */
    }
  }

  /** Short relative "last played" for tiles/rows ("5m ago", "3d ago", date). */
  function lastPlayedShort(iso: string | null): string {
    const L = get(t);
    if (!iso) return L("library.neverPlayed");
    const ts = new Date(iso).getTime();
    if (!Number.isFinite(ts)) return L("library.neverPlayed");
    const min = Math.floor((Date.now() - ts) / 60000);
    if (min < 1) return L("library.justNow");
    if (min < 60) return L("library.minAgo", { n: min });
    const h = Math.floor(min / 60);
    if (h < 24) return L("library.hourAgo", { n: h });
    const d = Math.floor(h / 24);
    if (d < 7) return L("library.dayAgo", { n: d });
    return new Date(iso).toLocaleDateString(undefined, { day: "numeric", month: "short" });
  }

  let showClonePrompt = $state(false);
  let cloneTarget = $state<RecentProject | null>(null);
  let clonePromptName = $state("");
  /** Prism-style instance manager dialog target (mods / backups / health). */
  let manageTarget = $state<RecentProject | null>(null);
  /** Rename prompt target (manifest display name; folder path unchanged). */
  let renameTarget = $state<RecentProject | null>(null);
  /** Per-instance notes (Prism-style), autosaved to localStorage. */
  let notesDraft = $state("");
  // Reload the draft when the selection changes. localStorage is read
  // non-reactively on purpose: re-tracking a notes state object would reset
  // the draft on every autosave (and trim trailing spaces while typing).
  $effect(() => {
    const p = selectedPath;
    notesDraft = p ? getNote(loadNotes(), p) : "";
  });

  function saveNote() {
    if (!selectedPath) return;
    setNote(loadNotes(), selectedPath, notesDraft);
  }

  // ── Update center: badge refresh + one-click Update all ──
  $effect(() => {
    const p = selectedPath;
    if (!p) return;
    // Lazily refresh when the cached counter is missing or stale (6h window).
    void refreshUpdateCount(p);
  });

  async function updateAllSelected() {
    const target = selected;
    if (!target || actionBusy) return;
    actionBusy = true;
    try {
      const res = await api.mods.updateAll(target.path, false);
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
      // Re-check for real — a partial failure leaves some updates pending.
      void refreshUpdateCount(target.path, true);
    } catch (e) {
      toasts.error(get(t)("library.toastUpdateFailed", { e: String(e) }));
    } finally {
      actionBusy = false;
    }
  }

  let showGroupPrompt = $state(false);
  let groupTarget = $state<RecentProject | null>(null);
  let groupPromptName = $state(DEFAULT_GROUP);

  let ctxMenu = $state<{ x: number; y: number; project: RecentProject } | null>(null);

  /** Android-style long-press → drag onto another tile to make a folder. */
  let dragSource = $state<RecentProject | null>(null);
  let dropTargetPath = $state<string | null>(null);
  // Import-time dedup question (docs/17 §4).
  let dedupAsk = $state<{ resolve: (v: boolean | null) => void; name: string } | null>(null);

  function askDedupChoice(name: string): Promise<boolean | null> {
    return new Promise((resolve) => {
      dedupAsk = {
        resolve: (v) => {
          dedupAsk = null;
          resolve(v);
        },
        name,
      };
    });
  }
  let dropTargetGroup = $state<string | null>(null);
  let dragGhost = $state<{ x: number; y: number; letter: string; colorA: string; colorB: string } | null>(null);
  let suppressNextClick = $state(false);
  let longPressTimer = $state<ReturnType<typeof setTimeout> | null>(null);
  let pressOrigin = $state<{ x: number; y: number; project: RecentProject } | null>(null);
  let dragging = $state(false);
  let holdingPath = $state<string | null>(null);

  function prefersReducedMotion(): boolean {
    if (typeof document === "undefined") return true;
    if (document.documentElement.classList.contains("potato-pc")) return true;
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  function tileIntro(node: Element) {
    if (prefersReducedMotion()) return { duration: 0 };
    return fade(node, { duration: 160 });
  }

  function sideIntro(node: Element) {
    if (prefersReducedMotion()) return { duration: 0 };
    return fly(node, { x: 12, duration: 200, opacity: 0, easing: quintOut });
  }

  function groupBodyIntro(node: Element) {
    if (prefersReducedMotion()) return { duration: 0 };
    return slide(node, { duration: 200, easing: quintOut });
  }

  const selected = $derived($recentProjects.find((p) => p.path === selectedPath) ?? null);
  const selectedRunning = $derived(
    isProjectRunning(selectedPath, $runningInstances),
  );
  const selectedLaunching = $derived(
    isProjectLaunching(selectedPath, $launchSessions),
  );
  const selectedLaunchMessage = $derived(
    selectedPath
      ? ($launchSessions[selectedPath]?.message ?? get(t)("library.launching"))
      : get(t)("library.launching"),
  );
  /** Multiplier from the Settings UI-scale (Auto mode derives it from screen size). */
  const sideScale = $derived(($uiScalePercentLive ?? 100) / 100);
  let selectingPath = false;
  $effect(() => {
    const recent = $recentProjects;
    const current = $projectPath;
    if (selectingPath) return;
    if (selectedPath && !recent.some((p) => p.path === selectedPath)) {
      selectedPath = recent[0]?.path ?? null;
      return;
    }
    if (!selectedPath && recent.length) {
      selectedPath =
        current && recent.some((p) => p.path === current) ? current : recent[0].path;
    }
  });

  const grouped = $derived((() => {
    const byGroup = new Map<string, RecentProject[]>();
    let total = 0;
    for (const p of $recentProjects) {
      if (!matchesInstanceFilter(p, instanceFilter, getGroup(groupMap, p.path))) continue;
      total++;
      const g = getGroup(groupMap, p.path);
      const list = byGroup.get(g) ?? [];
      list.push(p);
      byGroup.set(g, list);
    }
    const q = instanceFilter.trim().length > 0;
    const names = listGroupNames(
      groupMap,
      $recentProjects.map((p) => p.path),
    ).filter((n) => byGroup.has(n));
    return { groups: names.map((name) => ({
      name,
      projects: sortInstances(byGroup.get(name) ?? [], sortMode, $homeStats),
      collapsed: q ? false : collapsed.has(name),
    })), total, filtered: q };
  })());
  const visibleCount = $derived(grouped.total);

  const existingGroups = $derived(listGroupNames(
    groupMap,
    $recentProjects.map((p) => p.path),
  ));

  /** Instance icon data URLs (from listing iconPath), keyed by project path. */
  const instanceIcons = $derived($homeIcons);
  const iconRequested = new Set<string>();

  /** Load a single instance's listing icon into the shared homeIcons store. */
  async function loadInstanceIcon(path: string) {
    try {
      const listing = await api.project.getListing(path);
      const rel = listing.iconPath;
      if (!rel) {
        homeIcons.update((m) => ({ ...m, [path]: null }));
        return;
      }
      const data = await api.project.readListingAsset(rel, path);
      homeIcons.update((m) => ({ ...m, [path]: data }));
    } catch {
      homeIcons.update((m) => ({ ...m, [path]: null }));
    }
  }

  // Batch-fetch icons for instances that have none cached yet.
  $effect(() => {
    const missing = $recentProjects
      .map((p) => p.path)
      .filter((path) => !iconRequested.has(path) && instanceIcons[path] === undefined);
    if (!missing.length) return;
    for (const path of missing) iconRequested.add(path);
    void api.home
      .projectBriefs(missing)
      .then((briefs) => {
        const icons: Record<string, string | null> = {};
        for (const b of briefs) icons[b.path] = b.iconDataUrl ?? null;
        homeIcons.update((prev) => ({ ...prev, ...icons }));
        for (const path of missing) {
          if (icons[path] === undefined) void loadInstanceIcon(path);
        }
      })
      .catch(() => {
        for (const path of missing) void loadInstanceIcon(path);
      });
  });

  function gradientFrom(name: string) {
    const colors = ["var(--accent-primary)", "var(--accent-secondary)", "#3b82f6", "#f59e0b", "#ec4899", "#06b6d4", "#ef4444"];
    let hash = 0;
    for (let i = 0; i < name.length; i++) hash = name.charCodeAt(i) + ((hash << 5) - hash);
    return colors[Math.abs(hash) % colors.length];
  }

  async function loadStats(path: string) {
    try {
      const s = await api.stats.get(path);
      projectStats[path] = { playtime: s.totalPlaytimeSeconds ?? 0, lastLaunch: s.lastLaunch ?? null };
      projectStats = { ...projectStats };
    } catch {
      projectStats[path] = { playtime: 0, lastLaunch: null };
      projectStats = { ...projectStats };
    }
  }

  function ensureStats(paths: string[]) {
    for (const path of paths) {
      if (projectStats[path] !== undefined) continue;
      void loadStats(path);
    }
  }

  $effect(() => {
    ensureStats($recentProjects.map((p) => p.path));
  });

  function formatLastLaunch(iso: string | null): string {
    if (!iso) return get(t)("library.never");
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return get(t)("library.never");
    return d.toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
  }

  /** Expose the configured JRE for the selected pack (path or "Auto"). */
  function javaLabel(javaPath: string | null): string {
    if (!javaPath) return get(t)("library.autoWord");
    const base = javaPath.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? javaPath;
    return base || get(t)("library.autoWord");
  }

  function memoryLabel(memoryMb: number): string {
    if (!memoryMb || memoryMb <= 0) return "Auto";
    if (memoryMb >= 1024) return `${(memoryMb / 1024).toFixed(memoryMb % 1024 === 0 ? 0 : 1)} GB`;
    return `${memoryMb} MB`;
  }

  async function selectInstance(
    project: RecentProject,
    opts?: { keepMenus?: boolean },
  ): Promise<string> {
    if (!opts?.keepMenus) closeMenus();
    selectingPath = true;
    // Optimistic select so tiles feel clickable even while validate runs.
    selectedPath = project.path;
    try {
      const info = (await invoke("validate_project", {
        path: project.path,
      })) as RecentProject["info"] & { manifestPath?: string };
      const manifestPath = info.manifestPath || project.path;
      recentProjects.add(
        { path: manifestPath, info },
        {
          reorder: false,
          ...(manifestPath !== project.path
            ? { replacePath: project.path }
            : {}),
        },
      );
      selectedPath = manifestPath;
      projectPath.set(manifestPath);
      projectInfo.set(info);
      return manifestPath;
    } catch {
      selectedPath = project.path;
      projectPath.set(project.path);
      projectInfo.set(project.info);
      return project.path;
    } finally {
      selectingPath = false;
    }
  }

  function openInIde(project: RecentProject) {
    void selectInstance(project);
    ideStageRequest.set("content");
    currentView = "ide";
  }

  function openEdit(project: RecentProject) {
    // Edit = open the pack in IDE Content (mods), not Setup.
    openInIde(project);
  }

  async function launchInstance(project: RecentProject) {
    closeMenus();
    const path = await selectInstance(project);
    if (
      isProjectRunning(path, $runningInstances) ||
      isProjectRunning(project.path, $runningInstances)
    ) {
      await killWithFeedback(path);
      if (path !== project.path) await killWithFeedback(project.path);
      return;
    }
    if (isProjectLaunching(path, $launchSessions)) return;
    try {
      await invoke("set_last_opened_project", { path });
      await launchWithFeedback({ path, profile: "client" });
    } finally {
      void loadStats(selectedPath ?? project.path);
    }
  }

  async function stopInstance(project: RecentProject) {
    closeMenus();
    if (!isProjectRunning(project.path, $runningInstances)) return;
    await killWithFeedback(project.path);
  }

  function closeMenus() {
    ctxMenu = null;
    exportMenuOpen = false;
    addMenuOpen = false;
    foldersMenuOpen = false;
    moreMenuOpen = false;
  }

  function clearLongPressTimer() {
    if (longPressTimer != null) {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    }
  }

  function endDrag() {
    clearLongPressTimer();
    pressOrigin = null;
    holdingPath = null;
    dragging = false;
    dragSource = null;
    dropTargetPath = null;
    dropTargetGroup = null;
    dragGhost = null;
  }

  function beginDrag(
    project: RecentProject,
    x: number,
    y: number,
    target?: EventTarget | null,
    pointerId?: number,
  ) {
    dragging = true;
    holdingPath = null;
    dragSource = project;
    closeMenus();
    void selectInstance(project);
    dragGhost = {
      x,
      y,
      letter: project.info.name[0]?.toUpperCase() ?? "?",
      colorA: gradientFrom(project.info.name),
      colorB: gradientFrom(project.info.id),
    };
    if (target && pointerId != null && target instanceof HTMLElement) {
      try {
        target.setPointerCapture(pointerId);
      } catch {
        /* ignore */
      }
    }
    try {
      navigator.vibrate?.(12);
    } catch {
      /* ignore */
    }
  }

  function hitTestDrop(clientX: number, clientY: number) {
    dropTargetPath = null;
    dropTargetGroup = null;
    if (!dragSource) return;

    // Prefer rect hit-testing over elementFromPoint — CSS `zoom` on `.app-shell`
    // can desync the latter from the visual cursor in Chromium/Electron.
    const tiles = document.querySelectorAll<HTMLElement>(
      ".prism-lib .inst-tile[data-path], .prism-lib .inst-row[data-path]",
    );
    for (const tile of tiles) {
      const r = tile.getBoundingClientRect();
      if (clientX < r.left || clientX > r.right || clientY < r.top || clientY > r.bottom) continue;
      const path = tile.dataset.path ?? null;
      if (path && path !== dragSource.path) {
        dropTargetPath = path;
        return;
      }
    }

    const headers = document.querySelectorAll<HTMLElement>(".prism-lib .group-header[data-group]");
    for (const header of headers) {
      const r = header.getBoundingClientRect();
      if (clientX < r.left || clientX > r.right || clientY < r.top || clientY > r.bottom) continue;
      const name = header.dataset.group ?? null;
      if (name && name !== getGroup(groupMap, dragSource.path)) {
        dropTargetGroup = name;
      }
      return;
    }
  }

  function applyDrop() {
    if (!dragSource) return;
    if (dropTargetPath) {
      const target = $recentProjects.find((p) => p.path === dropTargetPath);
      if (!target) return;
      const result = folderFromDrop(
        groupMap,
        dragSource.path,
        target.path,
        target.info.name,
      );
      if (!result) return;
      groupMap = result.map;
      // Ensure the new/merged folder is expanded so the user sees the result.
      if (collapsed.has(result.groupName)) {
        collapsed = toggleCollapsed(collapsed, result.groupName);
      }
      toasts.success(
        result.created
          ? `Folder “${result.groupName}” created`
          : `Moved into “${result.groupName}”`,
      );
      return;
    }
    if (dropTargetGroup) {
      const name = dropTargetGroup;
      groupMap = setGroup(groupMap, dragSource.path, name);
      if (collapsed.has(name)) {
        collapsed = toggleCollapsed(collapsed, name);
      }
      toasts.success(
        name === DEFAULT_GROUP ? "Removed from folder" : `Moved into “${name}”`,
      );
    }
  }

  // ── Roving keyboard focus across tiles/rows (↑↓←→ / Home / End) ──
  function focusableItems(): HTMLElement[] {
    return Array.from(
      document.querySelectorAll<HTMLElement>(
        ".prism-lib .inst-tile[data-path], .prism-lib .inst-row[data-path]",
      ),
    ).filter((el) => el.offsetParent !== null);
  }

  function onTileKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      const path = (e.currentTarget as HTMLElement).dataset.path;
      const project = path ? $recentProjects.find((p) => p.path === path) : null;
      if (project) selectInstance(project);
      return;
    }
    const keys = ["ArrowRight", "ArrowLeft", "ArrowUp", "ArrowDown", "Home", "End"];
    if (!keys.includes(e.key)) return;
    const items = focusableItems();
    const idx = items.indexOf(e.currentTarget as HTMLElement);
    if (idx < 0) return;
    // Group items into visual rows by offsetTop (shared offsetParent, so the
    // CSS zoom on .app-shell does not matter). Left/Right step within the
    // flat order; Up/Down keep the column position across rows.
    const rows: number[][] = [];
    let top: number | null = null;
    for (let i = 0; i < items.length; i++) {
      const t = items[i].offsetTop;
      if (t !== top) {
        rows.push([i]);
        top = t;
      } else {
        rows[rows.length - 1].push(i);
      }
    }
    let rowIdx = 0;
    let colIdx = 0;
    for (let r = 0; r < rows.length; r++) {
      const c = rows[r].indexOf(idx);
      if (c >= 0) {
        rowIdx = r;
        colIdx = c;
        break;
      }
    }
    let target = -1;
    if (e.key === "ArrowRight") target = Math.min(items.length - 1, idx + 1);
    else if (e.key === "ArrowLeft") target = Math.max(0, idx - 1);
    else if (e.key === "Home") target = 0;
    else if (e.key === "End") target = items.length - 1;
    else if (e.key === "ArrowDown") target = rows[rowIdx + 1]?.[colIdx] ?? idx;
    else if (e.key === "ArrowUp") target = rows[rowIdx - 1]?.[Math.min(colIdx, rows[rowIdx - 1].length - 1)] ?? idx;
    if (target >= 0 && target !== idx) {
      e.preventDefault();
      items[target].focus();
    }
  }

  function onTilePointerDown(e: PointerEvent, project: RecentProject) {
    if (e.button !== 0) return;
    clearLongPressTimer();
    holdingPath = project.path;
    pressOrigin = { x: e.clientX, y: e.clientY, project };
    const origin = pressOrigin;
    const target = e.currentTarget;
    const pointerId = e.pointerId;
    longPressTimer = setTimeout(() => {
      longPressTimer = null;
      if (!pressOrigin || pressOrigin.project.path !== origin.project.path) return;
      beginDrag(origin.project, origin.x, origin.y, target, pointerId);
      suppressNextClick = true;
    }, LONG_PRESS_MS);
  }

  function onTilePointerMove(e: PointerEvent) {
    if (pressOrigin && !dragging) {
      const dx = e.clientX - pressOrigin.x;
      const dy = e.clientY - pressOrigin.y;
      if (dx * dx + dy * dy > MOVE_CANCEL_PX * MOVE_CANCEL_PX) {
        clearLongPressTimer();
        pressOrigin = null;
        holdingPath = null;
      }
      return;
    }
    if (!dragging || !dragSource || !dragGhost) return;
    dragGhost = { ...dragGhost, x: e.clientX, y: e.clientY };
    hitTestDrop(e.clientX, e.clientY);
  }

  function onTilePointerUp(e: PointerEvent) {
    clearLongPressTimer();
    if (dragging) {
      hitTestDrop(e.clientX, e.clientY);
      applyDrop();
      suppressNextClick = true;
      endDrag();
      setTimeout(() => {
        suppressNextClick = false;
      }, 80);
      return;
    }
    pressOrigin = null;
  }

  function onTilePointerCancel() {
    clearLongPressTimer();
    if (dragging) {
      suppressNextClick = true;
      endDrag();
      setTimeout(() => {
        suppressNextClick = false;
      }, 80);
    }
    pressOrigin = null;
  }

  function onTileClick(project: RecentProject) {
    if (suppressNextClick || dragging) {
      suppressNextClick = false;
      return;
    }
    void selectInstance(project);
  }

  function openCtxMenu(e: MouseEvent, project: RecentProject) {
    if (dragging) {
      e.preventDefault();
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    const pad = 8;
    const menuW = 230;
    const menuH = 420;
    // Zoom is on <html> — clientX/Y and position:fixed share one frame.
    let x = e.clientX;
    let y = e.clientY;
    if (x + menuW > window.innerWidth - pad) x = window.innerWidth - menuW - pad;
    if (y + menuH > window.innerHeight - pad) y = window.innerHeight - menuH - pad;
    void selectInstance(project, { keepMenus: true });
    ctxMenu = { x: Math.max(pad, x), y: Math.max(pad, y), project };
  }

  function onGlobalPointerDown(e: MouseEvent) {
    const t = e.target as HTMLElement | null;
    if (addMenuOpen && !t?.closest?.(".tb-add-wrap")) addMenuOpen = false;
    if (foldersMenuOpen && !t?.closest?.(".tb-folders-wrap")) foldersMenuOpen = false;
    if (exportMenuOpen && !t?.closest?.(".tb-export-wrap")) exportMenuOpen = false;
    if (moreMenuOpen && !t?.closest?.(".side-more-wrap")) moreMenuOpen = false;
    if (!ctxMenu || e.button === 2) return;
    if (t?.closest?.(".pack-ctx-menu")) return;
    ctxMenu = null;
  }

  function onGlobalKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (dragging) {
        endDrag();
        return;
      }
      closeMenus();
    }
  }

  function toggleGroup(name: string) {
    collapsed = toggleCollapsed(collapsed, name);
  }

  async function openInstancesFolder() {
    foldersMenuOpen = false;
    try {
      const info = await api.launcher.instancesPathInfo();
      const settings = await api.launcher.get();
      const dir = (settings.instancesPath?.trim() || info.current || info.default || "").replace(
        /[\\/]+$/,
        "",
      );
      if (!dir) {
        toasts.error(get(t)("library.toastFolderNotSet"));
        return;
      }
      await openShell(dir);
    } catch (e) {
      toasts.error(String(e));
    }
  }

  async function openSelectedFolder(project: RecentProject) {
    try {
      await invoke("open_project_folder", { path: project.path });
    } catch (e) {
      toasts.error(String(e));
    }
  }

  async function refreshAll() {
    refreshing = true;
    try {
      const next: RecentProject[] = [];
      for (const p of $recentProjects) {
        try {
          const info = (await invoke("validate_project", { path: p.path })) as RecentProject["info"] & {
            manifestPath?: string;
          };
          const manifestPath = (info as { manifestPath?: string }).manifestPath || p.path;
          next.push({ path: manifestPath, info: info as RecentProject["info"] });
        } catch {
          next.push(p);
        }
      }
      recentProjects.set(next);
      projectStats = {};
      ensureStats(next.map((p) => p.path));
      if (selectedPath) {
        const sel = next.find((p) => p.path === selectedPath);
        if (sel) projectInfo.set(sel.info);
      }
      toasts.info(get(t)("library.toastLibraryRefreshed"));
    } catch (e) {
      toasts.error(String(e));
    } finally {
      refreshing = false;
    }
  }

  async function importPackFile() {
    addMenuOpen = false;
    const selected = await openDialog({
      multiple: false,
      title: "Import .mrpack or .zip",
      filters: [
        { name: "Modpacks", extensions: ["mrpack", "zip"] },
        { name: "All", extensions: ["*"] },
      ],
    });
    if (typeof selected !== "string" || !selected) return;
    await importFromSource(selected);
  }

  async function importInstanceFolder() {
    addMenuOpen = false;
    const selected = await openDialog({
      directory: true,
      multiple: false,
      title: "Import Prism / MultiMC / CurseForge / mods folder",
    });
    if (typeof selected !== "string" || !selected) return;
    await importFromSource(selected);
  }

  async function importGithubRepo() {
    addMenuOpen = false;
    githubImportOpen = true;
  }

  async function confirmGithubImport(source: string) {
    githubImportOpen = false;
    const trimmed = source.trim();
    if (!trimmed) return;
    try {
      const info = await api.transport.github.inspectSource(trimmed);
      if (info.status === "publishing") {
        toasts.error(get(t)("library.toastOversized"));
        return;
      }
      githubPendingSource = trimmed;
      const version = info.packVersion ? ` v${info.packVersion}` : "";
      const ready = info.ready
        ? "ready"
        : info.status
          ? String(info.status)
          : "packwiz pack";
      const meta = githubInspectMeta(info);
      githubInspectSummary = `${info.fullName || trimmed}${version} · ${ready}${
        meta ? ` (${meta})` : ""
      }. Install anonymously?`;
      githubConfirmOpen = true;
    } catch (e) {
      toasts.error(String(e));
    }
  }

  async function confirmGithubInstall() {
    githubConfirmOpen = false;
    const source = githubPendingSource;
    githubPendingSource = "";
    if (source) await importFromSource(source);
  }

  async function resolveImportTargetDir(): Promise<string> {
    try {
      const info = await api.launcher.instancesPathInfo();
      const settings = await api.launcher.get();
      return (settings.instancesPath?.trim() || info.current || info.default || "").replace(
        /[\\/]+$/,
        "",
      );
    } catch {
      return "";
    }
  }

  async function importFromSource(source: string) {
    actionBusy = true;
    const isGithub = /^(gh:|https:\/\/github\.com\/|[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$)/.test(source.trim()) && !/\.(mrpack|zip)$/i.test(source.trim());
    if (isGithub) githubInstallActive = true;
    let dedup: boolean | null = true;
    dedup = await askDedupChoice(source.replace(/\\/g, "/").split("/").pop() ?? "");
    if (dedup === null) {
      actionBusy = false;
      githubInstallActive = false;
      return; // user closed the question — abort the import
    }
    try {
      const targetDir = await resolveImportTargetDir();
      if (!targetDir) {
        toasts.error(get(t)("library.toastSetFolder"));
        return;
      }
      const result: any = await invoke("install_modpack", {
        source,
        targetDir,
        instanceName: null,
        dedup,
      });
      const dedupInfo = result?.dedup;
      if (dedupInfo?.mode === "shared" && (dedupInfo.linked ?? 0) > 0) {
        const mb = Math.round((dedupInfo.bytesReclaimed ?? 0) / 1e6);
        toasts.info(
          `File deduplication on — ${dedupInfo.linked} file(s) shared${mb > 0 ? `, ~${mb} MB saved` : ""}.`,
          4000,
        );
      }
      const path = result.path;
      if (!path) throw new Error("Import returned no path");
      const info = (await invoke("validate_project", { path })) as RecentProject["info"] & {
        manifestPath?: string;
      };
      const manifestPath = info.manifestPath || path;
      recentProjects.add({ path: manifestPath, info: info as RecentProject["info"] });
      void selectInstance({ path: manifestPath, info: info as RecentProject["info"] });
      toasts.success(
        get(t)("library.toastImported", { name: result.name ?? info.name ?? "pack" }),
      );
    } catch (e) {
      toasts.error(String(e));
    } finally {
      actionBusy = false;
      githubInstallActive = false;
    }
  }

  async function runAction(action: string, project: RecentProject) {
    closeMenus();
    switch (action) {
      case "launch":
        await launchInstance(project);
        break;
      case "stop":
        await stopInstance(project);
        break;
      case "edit":
        openEdit(project);
        break;
      case "change-group":
        groupPromptName = getGroup(groupMap, project.path);
        groupTarget = project;
        showGroupPrompt = true;
        break;
      case "folder":
        await openSelectedFolder(project);
        break;
      case "change-icon":
        await changeInstanceIcon(project);
        break;
      case "clear-icon":
        await clearInstanceIcon(project);
        break;
      case "export-mrpack":
        actionBusy = true;
        try {
          const exported = await api.export.modrinthPack(null, project.path);
          try {
            await copyText(exported.path);
            toasts.success(get(t)("library.toastExportCopied", { path: exported.path }));
          } catch {
            toasts.success(get(t)("library.toastExport", { path: exported.path }));
          }
        } catch (e) {
          toasts.error(String(e));
        } finally {
          actionBusy = false;
        }
        break;
      case "export-prism":
        actionBusy = true;
        try {
          const exported = await api.export.prismInstance(null, project.path);
          try {
            await copyText(exported.path);
            toasts.success(get(t)("library.toastPrismCopied", { path: exported.path }));
          } catch {
            toasts.success(get(t)("library.toastPrism", { path: exported.path }));
          }
        } catch (e) {
          toasts.error(String(e));
        } finally {
          actionBusy = false;
        }
        break;
      case "export-server":
        actionBusy = true;
        try {
          const exported = await api.export.serverPack(null, project.path);
          try {
            await copyText(exported.path);
            toasts.success(get(t)("library.toastServerCopied", { path: exported.path }));
          } catch {
            toasts.success(get(t)("library.toastServer", { path: exported.path }));
          }
        } catch (e) {
          toasts.error(String(e));
        } finally {
          actionBusy = false;
        }
        break;
      case "copy":
        clonePromptName = `${project.info.name} copy`;
        cloneTarget = project;
        showClonePrompt = true;
        break;
      case "shortcut":
        actionBusy = true;
        try {
          const path = await api.files.createDesktopShortcut(project.path);
          toasts.success(get(t)("library.toastShortcut", { path }));
        } catch (e) {
          toasts.error(String(e));
        } finally {
          actionBusy = false;
        }
        break;
      case "open-ide":
        openInIde(project);
        break;
      case "copy-path":
        try {
          const dir = await api.project.getDir(project.path);
          await copyText(dir);
          toasts.success(get(t)("library.toastPathCopied"));
        } catch (e) {
          toasts.error(String(e));
        }
        break;
      case "repair":
        actionBusy = true;
        try {
          const report: {
            downloaded?: unknown[];
            failed?: unknown[];
            duplicates?: unknown[];
            wrongLoader?: unknown[];
          } = await invoke("repair_project", { path: project.path });
          const downloaded = report.downloaded?.length ?? 0;
          const failed = report.failed?.length ?? 0;

          // Extended repair sweep: duplicates + wrong-loader jars, so the user
          // gets a full health picture in one action instead of hunting
          // through Diagnostics.
          const dupes = (await api.mods.detectDuplicateModJars(project.path)) as Array<{
            modId: string;
            keepCandidate: string;
            jars: unknown[];
          }>;
          const wrongLoader =
            ((await api.mods.detectWrongLoader(project.path)) as Array<Record<string, unknown>>) ??
            [];

          const L = get(t);
          const parts: string[] = [];
          if (downloaded > 0) parts.push(L("library.repairRedownloaded", { n: downloaded }));
          if (failed > 0) parts.push(L("library.repairFailed", { n: failed }));
          if (dupes.length > 0) parts.push(L("library.repairDupes", { n: dupes.length }));
          if (wrongLoader.length > 0)
            parts.push(L("library.repairWrongLoader", { n: wrongLoader.length }));
          if (parts.length === 0) {
            toasts.success(get(t)("library.toastRepairOk"));
          } else if (dupes.length === 0 && wrongLoader.length === 0) {
            toasts.success(get(t)("library.toastRepairReport", { parts: parts.join(", ") }));
          } else {
            toasts.warning(
              get(t)("library.toastRepairFindings", { parts: parts.join(" ") }),
              10000,
            );
          }
        } catch (e) {
          toasts.error(String(e));
        } finally {
          actionBusy = false;
        }
        break;
      case "remove":
        recentProjects.remove(project.path);
        if (selectedPath === project.path) {
          selectedPath = $recentProjects[0]?.path ?? null;
          projectPath.set(selectedPath);
          projectInfo.set($recentProjects[0]?.info ?? null);
        }
        toasts.info(get(t)("library.toastRemoved", { name: project.info.name }));
        break;
      case "delete": {
        const ok = await confirm(`Delete "${project.info.name}" from disk?`, {
          title: "Delete instance",
          kind: "warning",
        });
        if (!ok) break;
        try {
          await invoke("delete_project", { path: project.path });
          recentProjects.remove(project.path);
          if (selectedPath === project.path) {
            selectedPath = $recentProjects[0]?.path ?? null;
            projectPath.set(selectedPath);
            projectInfo.set($recentProjects[0]?.info ?? null);
          }
          toasts.success(get(t)("library.toastDeleted", { name: project.info.name }));
        } catch (e) {
          toasts.error(String(e));
        }
        break;
      }
    }
  }

  /** Pick a PNG and apply it as the instance's listing icon (shared homeIcons store). */
  async function changeInstanceIcon(project: RecentProject) {
    const selected = await openDialog({
      multiple: false,
      title: `Choose icon for "${project.info.name}"`,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
    });
    if (typeof selected !== "string" || !selected) return;
    actionBusy = true;
    try {
      await api.project.setListingIcon(selected, project.path);
      iconRequested.delete(project.path);
      await loadInstanceIcon(project.path);
      toasts.success(get(t)("library.toastIconUpdated", { name: project.info.name }));
    } catch (e) {
      toasts.error(String(e));
    } finally {
      actionBusy = false;
    }
  }

  /** Remove the custom listing icon and fall back to the gradient letter tile. */
  async function clearInstanceIcon(project: RecentProject) {
    actionBusy = true;
    try {
      await api.project.clearListingIcon(project.path);
      homeIcons.update((prev) => ({ ...prev, [project.path]: null }));
      toasts.info(get(t)("library.toastIconCleared", { name: project.info.name }));
    } catch (e) {
      toasts.error(String(e));
    } finally {
      actionBusy = false;
    }
  }

  async function confirmRename(newName: string) {
    const target = renameTarget;
    renameTarget = null;
    if (!target || !newName.trim()) return;
    actionBusy = true;
    try {
      const applied = await api.files.rename(newName.trim(), target.path);
      const info = (await api.project.validate(target.path)) as RecentProject["info"] & {
        manifestPath?: string;
      };
      recentProjects.updateInfo(info.manifestPath || target.path, info);
      toasts.success(get(t)("library.toastRenamed", { name: applied }));
    } catch (e) {
      toasts.error(String(e));
    } finally {
      actionBusy = false;
    }
  }

  async function confirmClone(newName: string) {
    showClonePrompt = false;
    if (!cloneTarget || !newName.trim()) return;
    actionBusy = true;
    try {
      const clonedPath = await invoke<string>("clone_project", {
        path: cloneTarget.path,
        newName: newName.trim(),
      });
      const info = (await invoke("validate_project", { path: clonedPath })) as RecentProject["info"] & {
        manifestPath?: string;
      };
      const manifestPath = info.manifestPath || clonedPath;
      recentProjects.add({ path: manifestPath, info: info as RecentProject["info"] });
      void selectInstance({ path: manifestPath, info: info as RecentProject["info"] });
      toasts.success(get(t)("library.toastCopiedTo", { path: manifestPath }));
    } catch (e) {
      toasts.error(String(e));
    } finally {
      actionBusy = false;
      cloneTarget = null;
    }
  }

  function confirmGroup(name: string) {
    showGroupPrompt = false;
    if (!groupTarget) return;
    groupMap = setGroup(groupMap, groupTarget.path, name);
    groupTarget = null;
  }

  function applyExistingGroup(name: string) {
    if (!groupTarget) return;
    groupMap = setGroup(groupMap, groupTarget.path, name);
    showGroupPrompt = false;
    groupTarget = null;
  }

  onMount(() => {
    if ($recentProjects.length && !selectedPath) {
      selectedPath = $projectPath ?? $recentProjects[0].path;
    }
  });

  onDestroy(() => {
    clearLongPressTimer();
    endDrag();
  });
</script>

<DedupAskDialog
  open={!!dedupAsk}
  packName={dedupAsk?.name ?? ""}
  onanswer={(v) => dedupAsk?.resolve(v)}
  oncancel={() => dedupAsk?.resolve(null)}
/>

<div class="prism-lib" class:drag-mode={dragging}>
  <div class="prism-toolbar lib-toolbar-enter">
    <div class="tb-left">
      {#if toolbarLeading}
        <div class="tb-leading">{@render toolbarLeading()}</div>
      {/if}
      <div class="tb-add-wrap">
        <button
          type="button"
          class="tb-btn primary"
          title={$t("library.addInstanceTitle")}
          onclick={(e) => { e.stopPropagation(); (addMenuOpen = !addMenuOpen);  }}
        >
          <Plus size={16} />
          <span>{$t("library.addInstance")}</span>
          <ChevronDown size={14} />
        </button>
        {#if addMenuOpen}
          <div class="tb-menu" role="menu">
            <button type="button" role="menuitem" onclick={() => { addMenuOpen = false; openAddInstance("blank"); }}>
              <Plus size={14} /> {$t("library.createNew")}
            </button>
            <button type="button" role="menuitem" onclick={importPackFile} disabled={actionBusy}>
              <FolderOpen size={14} /> {$t("library.importFile")}
            </button>
            <button type="button" role="menuitem" onclick={importInstanceFolder} disabled={actionBusy}>
              <Folder size={14} /> {$t("library.importFolder")}
            </button>
            <button type="button" role="menuitem" onclick={importGithubRepo} disabled={actionBusy}>
              <Link2 size={14} /> {$t("library.importGithubRepo")}
            </button>
            <div class="menu-sep"></div>
            <button type="button" role="menuitem" onclick={() => { addMenuOpen = false; libraryTabRequest.set("discover"); }}>
              <Compass size={14} /> {$t("library.findInCatalog")}
            </button>
          </div>
        {/if}
      </div>

      <div class="tb-folders-wrap">
        <button
          type="button"
          class="tb-btn"
          title={$t("library.folders")}
          onclick={(e) => { e.stopPropagation(); (foldersMenuOpen = !foldersMenuOpen);  }}
        >
          <Folder size={16} />
          <span>{$t("library.folders")}</span>
        </button>
        {#if foldersMenuOpen}
          <div class="tb-menu" role="menu">
            <button type="button" role="menuitem" onclick={openInstancesFolder}>
              <FolderOpen size={14} /> {$t("library.instancesFolder")}
            </button>
            {#if selected}
              <button type="button" role="menuitem" onclick={() => { foldersMenuOpen = false; void openSelectedFolder(selected); }}>
                <Folder size={14} /> {$t("library.selectedInstance")}
              </button>
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <div class="tb-right">
      <button
        type="button"
        class="tb-btn"
        title={$t("common.refresh")}
        disabled={refreshing}
        onclick={() => void refreshAll()}
      >
        <span class:spinning={refreshing}><RefreshCw size={16} /></span>
        <span>{$t("library.update")}</span>
      </button>
      <button
        type="button"
        class="tb-btn"
        title={$t("common.help")}
        onclick={() => window.dispatchEvent(new CustomEvent("tuffbox:show-shortcuts"))}
      >
        <HelpCircle size={16} />
        <span>{$t("common.help")}</span>
      </button>
      <button type="button" class="tb-btn" title={$t("common.settings")} onclick={() => (currentView = "settings")}>
        <Settings size={16} />
        <span>{$t("common.settings")}</span>
      </button>
      {#if $authState.loggedIn && $authState.profile}
        <button
          type="button"
          class="tb-account"
          title={$t("library.account")}
          onclick={() => (currentView = "me")}
        >
          <HeadAvatar skinSrc={$skinPath} size={28} alt={$authState.profile.name} />
          <span class="tb-account-name">{$authState.profile.name}</span>
          <span class="tb-account-badge">
            {loginTypeLabel(
              $authState.loginType,
              $authState.accounts.find((a) => a.uuid === $authState.activeAccountUuid)?.authority,
            )}
          </span>
        </button>
      {:else}
        <button type="button" class="tb-btn" onclick={() => loginModalOpen.set(true)}>
          Sign in
        </button>
      {/if}
    </div>
  </div>

  <div class="prism-body" class:is-dragging={dragging} style={`--side-scale: ${sideScale}`}>
    <div class="prism-grid-pane">
      <div class="lib-page-head">
        <div class="lib-page-title">
          <h2 class="lib-title">{$t("library.title")}</h2>
          {#if $recentProjects.length > 0}
            <span class="lib-title-count">{visibleCount} of {$recentProjects.length}</span>
          {/if}
        </div>
        <div class="lib-filterbar" role="search">
          <div class="tb-search">
            <Search size={15} class="tb-search-icon" />
            <input
              type="text"
              placeholder={$t("library.filterPlaceholder")}
              aria-label={$t("library.filterAria")}
              spellcheck="false"
              bind:value={instanceFilter}
            />
            {#if instanceFilter}
              <button
                type="button"
                class="tb-search-clear"
                aria-label={$t("library.clearFilter")}
                onclick={() => (instanceFilter = "")}
              >
                <X size={13} />
              </button>
            {/if}
          </div>
          <select
            class="tb-sort"
            aria-label={$t("library.sort")}
            title="Sort instances"
            value={sortMode}
            onchange={(e) => setSortMode((e.currentTarget as HTMLSelectElement).value as SortMode)}
          >
            <option value="recent">{$t("library.sortRecent")}</option>
            <option value="name">{$t("library.sortName")}</option>
            <option value="playtime">{$t("library.sortPlaytime")}</option>
          </select>
          <div class="view-toggle" role="group" aria-label={$t("library.layout")}>
            <button
              type="button"
              class="view-btn"
              class:active={viewMode === "grid"}
              title="Grid view"
              aria-label={$t("library.gridView")}
              aria-pressed={viewMode === "grid"}
              onclick={() => setViewMode("grid")}
            >
              <LayoutGrid size={15} />
            </button>
            <button
              type="button"
              class="view-btn"
              class:active={viewMode === "list"}
              title="List view"
              aria-label={$t("library.listView")}
              aria-pressed={viewMode === "list"}
              onclick={() => setViewMode("list")}
            >
              <List size={15} />
            </button>
          </div>
        </div>
      </div>
      {#if $recentProjects.length === 0}
        <div class="empty-state">
          <h3>{$t("library.emptyTitle")}</h3>
          <p>{$t("library.emptyBody")}</p>
          <button type="button" class="empty-cta" onclick={() => openAddInstance("blank")}>
            <Plus size={16} /> Add Instance
          </button>
        </div>
      {:else if visibleCount === 0}
        <div class="empty-state">
          <h3>{$t("library.noMatchesTitle")}</h3>
          <p>{$t("library.noMatchesBody", { filter: instanceFilter })}</p>
          <button type="button" class="empty-cta" onclick={() => (instanceFilter = "")}>
            <X size={16} /> Clear filter
          </button>
        </div>
      {:else}
        <p class="drag-hint" class:visible={dragging}>
          Drop on another instance to make a folder
        </p>
{#snippet instanceTile(project: RecentProject)}
  {@const tileRunning = isProjectRunning(project.path, $runningInstances)}
  {@const tileLaunching = isProjectLaunching(project.path, $launchSessions)}
    <div
      class="inst-tile"
      class:selected={selectedPath === project.path}
      class:running={tileRunning}
      class:dragging={dragSource?.path === project.path}
      class:drop-target={dropTargetPath === project.path}
      class:holding={holdingPath === project.path && !dragging}
      data-path={project.path}
      role="button"
      tabindex="0"
      aria-label={`${project.info.name}. Hold and drag onto another instance to create a folder`}
      in:tileIntro
      onclick={() => onTileClick(project)}
      ondblclick={() => !dragging && void launchInstance(project)}
      onkeydown={onTileKeydown}
      oncontextmenu={(e) => openCtxMenu(e, project)}
      onpointerdown={(e) => onTilePointerDown(e, project)}
      onpointermove={onTilePointerMove}
      onpointerup={onTilePointerUp}
      onpointercancel={onTilePointerCancel}
    >
      <div class="hold-ring" aria-hidden="true"></div>
      <div
        class="inst-icon"
        class:has-image={!!instanceIcons[project.path]}
        class:folder-preview={dropTargetPath === project.path}
        style={`background: linear-gradient(135deg, ${gradientFrom(project.info.name)}, ${gradientFrom(project.info.id)})`}
      >
        {#if instanceIcons[project.path]}
          <img
            class="inst-icon-img"
            src={instanceIcons[project.path]!}
            alt=""
            draggable="false"
          />
        {:else if dropTargetPath === project.path && dragSource}
          <span class="folder-stack" aria-hidden="true">
            <span class="stack-a">{dragSource.info.name[0]?.toUpperCase()}</span>
            <span class="stack-b">{project.info.name[0]?.toUpperCase()}</span>
          </span>
        {:else}
          {project.info.name[0]?.toUpperCase() ?? "?"}
        {/if}
        {#if ($updateCounts[project.path]?.count ?? 0) > 0}
          <span
            class="tile-upd"
            title={$t("library.updatesBadgeTitle", { n: $updateCounts[project.path]?.count ?? 0 })}
          >
            {($updateCounts[project.path]?.count ?? 0) > 9
              ? "9+"
              : $updateCounts[project.path]?.count}
          </span>
        {/if}
        {#if !dragging && !dropTargetPath}
          <!-- Home-card-style delayed hover reveal; buttons stop
               propagation so they never start a tile drag/hold,
               re-select or double-launch. -->
          <span class="tile-actions">
            <button
              type="button"
              class="tile-play"
              class:stop={tileRunning}
              disabled={tileLaunching}
              title={tileRunning ? $t("common.stop") : $t("common.play")}
              aria-label={tileRunning
                ? $t("library.stopAria", { name: project.info.name })
                : $t("library.playAria", { name: project.info.name })}
              onpointerdown={(e) => e.stopPropagation()}
              onclick={(e) => { e.stopPropagation(); void launchInstance(project); }}
              ondblclick={(e) => e.stopPropagation()}
              onkeydown={(e) => e.stopPropagation()}
            >
              {#if tileLaunching}
                <span class="mini-spinner"></span>
              {:else if tileRunning}
                <Square size={12} fill="currentColor" /> Stop
              {:else}
                <Play size={13} fill="currentColor" /> Play
              {/if}
            </button>
            <span class="tile-acts">
              <button
                type="button"
                class="tile-act"
                title={$t("library.openInIde")}
                aria-label={`Open ${project.info.name} in IDE`}
                onpointerdown={(e) => e.stopPropagation()}
                onclick={(e) => { e.stopPropagation(); openInIde(project); }}
                ondblclick={(e) => e.stopPropagation()}
                onkeydown={(e) => e.stopPropagation()}
              >
                <Package size={14} />
              </button>
              <button
                type="button"
                class="tile-act"
                title={$t("library.openFolder")}
                aria-label={`Open ${project.info.name} folder`}
                onpointerdown={(e) => e.stopPropagation()}
                onclick={(e) => { e.stopPropagation(); void runAction("folder", project); }}
                ondblclick={(e) => e.stopPropagation()}
                onkeydown={(e) => e.stopPropagation()}
              >
                <Folder size={14} />
              </button>
            </span>
          </span>
        {/if}
      </div>
      <span
        class="inst-name"
        title={project.info.name}
      >{project.info.name}</span>
      <span class="inst-version" title={`${project.info.minecraftVersion} · ${project.info.loaderKind}`}>
        {project.info.minecraftVersion} · {project.info.loaderKind}
      </span>
      <span class="inst-last" title={formatLastLaunch(projectStats[project.path]?.lastLaunch ?? null)}>
        {lastPlayedShort(projectStats[project.path]?.lastLaunch ?? null)}
      </span>
    </div>
{/snippet}

{#snippet instanceRow(project: RecentProject)}
  {@const rowRunning = isProjectRunning(project.path, $runningInstances)}
  {@const rowLaunching = isProjectLaunching(project.path, $launchSessions)}
  {@const stats = projectStats[project.path]}
  <div
    class="inst-row"
    class:selected={selectedPath === project.path}
    class:running={rowRunning}
    class:dragging={dragSource?.path === project.path}
    class:drop-target={dropTargetPath === project.path}
    data-path={project.path}
    role="button"
    tabindex="0"
    aria-label={project.info.name}
    in:tileIntro
    onclick={() => onTileClick(project)}
    ondblclick={() => !dragging && void launchInstance(project)}
    onkeydown={onTileKeydown}
    oncontextmenu={(e) => openCtxMenu(e, project)}
    onpointerdown={(e) => onTilePointerDown(e, project)}
    onpointermove={onTilePointerMove}
    onpointerup={onTilePointerUp}
    onpointercancel={onTilePointerCancel}
  >
    <div
      class="row-icon"
      class:has-image={!!instanceIcons[project.path]}
      style={`background: linear-gradient(135deg, ${gradientFrom(project.info.name)}, ${gradientFrom(project.info.id)})`}
    >
      {#if instanceIcons[project.path]}
<img class="inst-icon-img" src={instanceIcons[project.path]!} alt="" draggable="false" />
      {:else}
{project.info.name[0]?.toUpperCase() ?? "?"}
      {/if}
      {#if rowRunning}
<span class="row-running-dot" aria-hidden="true"></span>
      {/if}
    </div>
    <div class="row-main">
      <span class="row-name" title={project.info.name}>
        {project.info.name}
        {#if ($updateCounts[project.path]?.count ?? 0) > 0}
          <span class="row-upd" title={$t("library.updatesBadgeTitle", { n: $updateCounts[project.path]?.count ?? 0 })}>
            ↑ {$updateCounts[project.path]?.count > 9 ? "9+" : $updateCounts[project.path]?.count}
          </span>
        {/if}
      </span>
      <span class="row-sub" title={`${project.info.minecraftVersion} · ${project.info.loaderKind}`}>
{project.info.minecraftVersion} · {project.info.loaderKind}
      </span>
    </div>
    <div class="row-stats" aria-label={$t("library.playStats")}>
      <span class="row-stat" title={$t("library.lastPlayed")}>
{lastPlayedShort(stats?.lastLaunch ?? null)}
      </span>
      <span class="row-stat" title={$t("library.playtime")}>
{formatPlaytime(stats?.playtime ?? 0)} played
      </span>
    </div>
    <div class="row-actions">
      <button
type="button"
class="row-play"
class:stop={rowRunning}
disabled={rowLaunching}
title={rowRunning ? $t("common.stop") : $t("common.play")}
aria-label={rowRunning
      ? $t("library.stopAria", { name: project.info.name })
      : $t("library.playAria", { name: project.info.name })}
onpointerdown={(e) => e.stopPropagation()}
onclick={(e) => { e.stopPropagation(); void launchInstance(project); }}
ondblclick={(e) => e.stopPropagation()}
onkeydown={(e) => e.stopPropagation()}
      >
{#if rowLaunching}
  <span class="mini-spinner"></span>
{:else if rowRunning}
  <Square size={12} fill="currentColor" /> Stop
{:else}
  <Play size={13} fill="currentColor" /> Play
{/if}
      </button>
      <button
type="button"
class="row-act"
title={$t("library.openInIde")}
aria-label={`Open ${project.info.name} in IDE`}
onpointerdown={(e) => e.stopPropagation()}
onclick={(e) => { e.stopPropagation(); openInIde(project); }}
ondblclick={(e) => e.stopPropagation()}
onkeydown={(e) => e.stopPropagation()}
      >
<Package size={15} />
      </button>
      <button
type="button"
class="row-act"
title={$t("library.openFolder")}
aria-label={`Open ${project.info.name} folder`}
onpointerdown={(e) => e.stopPropagation()}
onclick={(e) => { e.stopPropagation(); void runAction("folder", project); }}
ondblclick={(e) => e.stopPropagation()}
onkeydown={(e) => e.stopPropagation()}
      >
<Folder size={15} />
      </button>
    </div>
  </div>
{/snippet}

        {#each grouped.groups as group (group.name)}
          <section class="inst-group">
            <button
              type="button"
              class="group-header"
              class:drop-target={dragging && dropTargetGroup === group.name}
              data-group={group.name}
              onclick={() => toggleGroup(group.name)}
              aria-expanded={!group.collapsed}
            >
              {#if group.collapsed}
                <ChevronRight size={16} />
              {:else}
                <ChevronDown size={16} />
              {/if}
              <span>{group.name}</span>
              <span class="group-count">{group.projects.length}</span>
            </button>
            {#if !group.collapsed}
              {#if viewMode === "grid"}
                <div class="inst-grid" transition:groupBodyIntro>
                  {#each group.projects as project (project.path)}
                    {@render instanceTile(project)}
                  {/each}
                </div>
              {:else}
                <div class="inst-rows" transition:groupBodyIntro>
                  {#each group.projects as project (project.path)}
                    {@render instanceRow(project)}
                  {/each}
                </div>
              {/if}
            {/if}
          </section>
        {/each}
      {/if}
      <div class="lib-footer" aria-live="polite">
        <span>
          {#if selected}
            Minecraft {selected.info.minecraftVersion} · {selected.info.loaderKind}
            {#if projectStats[selected.path]?.playtime}
              · {formatPlaytime(projectStats[selected.path].playtime)} played
            {/if}
          {:else}
            —
          {/if}
        </span>
        <span>
          Total playtime: {formatPlaytime(
            Object.values(projectStats).reduce((s, p) => s + (p?.playtime ?? 0), 0),
          )}
        </span>
      </div>
    </div>

    <aside class="prism-side lib-side-enter" aria-label="Instance actions">
      {#if selected}
        {#key selected.path}
          <div class="side-panel" in:sideIntro>
            <div class="side-hero">
              <div
                class="side-icon"
                class:has-image={!!instanceIcons[selected.path]}
                style={`background: linear-gradient(135deg, ${gradientFrom(selected.info.name)}, ${gradientFrom(selected.info.id)})`}
              >
                {#if instanceIcons[selected.path]}
                  <img
                    class="side-icon-img"
                    src={instanceIcons[selected.path]!}
                    alt=""
                    draggable="false"
                  />
                {:else}
                  {selected.info.name[0]?.toUpperCase() ?? "?"}
                {/if}
              </div>
              <div class="side-title" title={selected.info.name}>{selected.info.name}</div>
              <div class="side-meta">
                {selected.info.minecraftVersion} · {selected.info.loaderKind}
              </div>
              <button
                type="button"
                class="side-group-chip"
                title={$t("library.changeGroup")}
                onclick={() => void runAction("change-group", selected)}
              >
                <Tags size={12} />
                <span class="side-group-name">{getGroup(groupMap, selected.path)}</span>
              </button>
            </div>

            <div class="side-actions">
              <button
                type="button"
                class={["side-btn", "launch", { stop: selectedRunning }]}
                disabled={actionBusy || selectedLaunching}
                onclick={() => void runAction(selectedRunning ? "stop" : "launch", selected)}
              >
                {#if selectedLaunching}
                  <span class="mini-spinner"></span> {selectedLaunchMessage}
                {:else if selectedRunning}
                  <Square size={16} fill="currentColor" /> {$t("common.stop")}
                {:else}
                  <Play size={18} fill="currentColor" /> {$t("common.play")}
                {/if}
              </button>

              {#if ($updateCounts[selected.path]?.count ?? 0) > 0}
                <button
                  type="button"
                  class="side-updates"
                  title={$t("library.updatesTitle")}
                  disabled={actionBusy}
                  onclick={() => void updateAllSelected()}
                >
                  <ArrowUpCircle size={14} />
                  <span class="side-updates-text">
                    {$updateCounts[selected.path]?.count === 1
                      ? $t("library.modHasUpdate")
                      : $t("library.modsHaveUpdates", { n: $updateCounts[selected.path]?.count ?? 0 })}
                  </span>
                  <strong>{$t("library.updateAll")}</strong>
                </button>
              {/if}

              <!-- Labeled secondaries: the two most common destinations after
                   Play, promoted out of the icon row for discoverability. -->
              <div class="side-secondary-row">
                <button
                  type="button"
                  class="side-secondary manage"
                  title={$t("library.modsBackupsHealth")}
                  aria-label={$t("library.manageInstance")}
                  onclick={() => (manageTarget = selected)}
                >
                  <SlidersHorizontal size={15} /> {$t("library.manageEllipsis")}
                </button>
                <button
                  type="button"
                  class="side-secondary"
                  title={$t("library.openIdeAria")}
                  aria-label={$t("library.openInIde")}
                  disabled={actionBusy}
                  onclick={() => runAction("open-ide", selected)}
                >
                  <Package size={15} /> {$t("library.openInIde")}
                </button>
                <button
                  type="button"
                  class="side-secondary"
                  title={$t("library.openFolderAria")}
                  aria-label={$t("library.openFolder")}
                  disabled={actionBusy}
                  onclick={() => void runAction("folder", selected)}
                >
                  <Folder size={15} /> {$t("library.folder")}
                </button>
              </div>

              <div class="side-icon-row">
                <div class="tb-export-wrap">
                  <button
                    type="button"
                    class="side-icon-btn"
                    title={$t("common.export")}
                    aria-label={$t("common.export")}
                    disabled={actionBusy}
                    onclick={(e) => { e.stopPropagation(); (exportMenuOpen = !exportMenuOpen);  }}
                  >
                    <Share2 size={16} />
                  </button>
                  {#if exportMenuOpen}
                    <div class="tb-menu side-menu" role="menu" transition:fade={{ duration: prefersReducedMotion() ? 0 : 120 }}>
                      <button type="button" role="menuitem" onclick={() => void runAction("export-mrpack", selected)}>
                        {$t("home.exportMrpack")}
                      </button>
                      <button type="button" role="menuitem" onclick={() => void runAction("export-server", selected)}>
                        {$t("home.serverPack")}
                      </button>
                      <button type="button" role="menuitem" onclick={() => void runAction("export-prism", selected)}>
                        {$t("library.exportPrism")}
                      </button>
                    </div>
                  {/if}
                </div>
                <div class="side-more-wrap">
                  <button
                    type="button"
                    class="side-icon-btn"
                    title={$t("common.more")}
                    aria-label={$t("library.moreActions")}
                    onclick={(e) => { e.stopPropagation(); (moreMenuOpen = !moreMenuOpen); }}
                  >
                    <Settings size={16} />
                  </button>
                  {#if moreMenuOpen}
                    <div class="tb-menu side-menu" role="menu" transition:fade={{ duration: prefersReducedMotion() ? 0 : 120 }}>
                      <button type="button" role="menuitem" onclick={() => { moreMenuOpen = false; manageTarget = selected; }}>
                        <SlidersHorizontal size={14} /> {$t("library.manageEllipsis")}
                      </button>
                      <button type="button" role="menuitem" onclick={() => { moreMenuOpen = false; renameTarget = selected; }}>
                        <Pencil size={14} /> {$t("library.renameEllipsis")}
                      </button>
                      <button type="button" role="menuitem" onclick={() => { moreMenuOpen = false; void runAction("change-group", selected); }}>
                        <Tags size={14} /> {$t("library.changeGroup")}
                      </button>
                      <button type="button" role="menuitem" onclick={() => { moreMenuOpen = false; void runAction("change-icon", selected); }} disabled={actionBusy}>
                        <ImageIcon size={14} /> {$t("library.changeIcon")}
                      </button>
                      {#if instanceIcons[selected.path]}
                        <button type="button" role="menuitem" onclick={() => { moreMenuOpen = false; void runAction("clear-icon", selected); }} disabled={actionBusy}>
                          <Eraser size={14} /> {$t("library.clearIcon")}
                        </button>
                      {/if}
                      <button type="button" role="menuitem" onclick={() => { moreMenuOpen = false; void runAction("copy", selected); }}>
                        <Copy size={14} /> {$t("library.copyInstance")}
                      </button>
                      <button type="button" role="menuitem" onclick={() => { moreMenuOpen = false; void runAction("shortcut", selected); }}>
                        <Link2 size={14} /> {$t("library.createShortcut")}
                      </button>
                      <button type="button" role="menuitem" onclick={() => { moreMenuOpen = false; void runAction("repair", selected); }} disabled={actionBusy}>
                        <Wrench size={14} /> {$t("library.repair")}
                      </button>
                      <button type="button" role="menuitem" onclick={() => { moreMenuOpen = false; void runAction("copy-path", selected); }}>
                        <Copy size={14} /> {$t("library.copyPath")}
                      </button>
                      <div class="menu-sep"></div>
                      <button type="button" role="menuitem" onclick={() => { moreMenuOpen = false; void runAction("remove", selected); }}>
                        <Minus size={14} /> {$t("library.removeFromLibrary")}
                      </button>
                      <button type="button" role="menuitem" class="danger" onclick={() => { moreMenuOpen = false; void runAction("delete", selected); }}>
                        <Trash2 size={14} /> {$t("library.deleteFromDisk")}
                      </button>
                    </div>
                  {/if}
                </div>
              </div>
            </div>

            <div class="side-meta-grid">
              <div class="side-meta-item">
                <span class="side-meta-label">{$t("library.playTime")}</span>
                <span class="side-meta-value">{formatPlaytime(projectStats[selected.path]?.playtime ?? 0)}</span>
              </div>
              <div class="side-meta-item">
                <span class="side-meta-label">{$t("library.lastPlayed")}</span>
                <span class="side-meta-value">{formatLastLaunch(projectStats[selected.path]?.lastLaunch ?? null)}</span>
              </div>
              <div class="side-meta-item">
                <span class="side-meta-label">{$t("library.java")}</span>
                <span class="side-meta-value">{javaLabel(selected.info.javaPath)}</span>
              </div>
              <div class="side-meta-item">
                <span class="side-meta-label">{$t("library.memory")}</span>
                <span class="side-meta-value">{memoryLabel(selected.info.memoryMb)}</span>
              </div>
            </div>
            <div class="side-notes">
              <label class="side-notes-label" for="side-notes-input">{$t("library.notes")}</label>
              <textarea
                id="side-notes-input"
                class="side-notes-input"
                placeholder={$t("library.notesPlaceholder")}
                bind:value={notesDraft}
                oninput={() => saveNote()}
                rows={3}
                spellcheck="false"
              ></textarea>
            </div>
            <div class="side-content">
              <button
                type="button"
                class="side-content-toggle"
                aria-expanded={!contentCollapsed}
                onclick={toggleContentCollapsed}
              >
                <span class="side-content-title">{$t("library.content")}</span>
                <span class="side-content-hint">mods · packs · shaders · servers</span>
                <ChevronDown size={14} class={!contentCollapsed ? "flipped" : ""} />
              </button>
              {#if !contentCollapsed}
                <div class="side-content-body">
                  <LibraryInstanceContent
                    projectPath={selected.path}
                    onOpenMods={() => openInIde(selected)}
                    onManage={() => (manageTarget = selected)}
                  />
                </div>
              {/if}
            </div>
          </div>
        {/key}
      {:else}
        <div class="side-empty" in:fade={{ duration: prefersReducedMotion() ? 0 : 160 }}>{$t("library.selectInstance")}</div>
      {/if}
    </aside>
  </div>
</div>

{#if dragGhost}
  <div
    class="drag-ghost"
    use:portal
    style={`position:fixed; left:${dragGhost.x}px; top:${dragGhost.y}px; z-index:10000; background: linear-gradient(135deg, ${dragGhost.colorA}, ${dragGhost.colorB})`}
    aria-hidden="true"
  >
    <span class="ghost-letter">{dragGhost.letter}</span>
    <span class="ghost-ring"></span>
  </div>
{/if}

{#if ctxMenu}
  {@const menuProject = ctxMenu.project}
  <div
    class="pack-ctx-menu"
    use:portal
    style={`position:fixed; left:${ctxMenu.x}px; top:${ctxMenu.y}px; z-index:10000`}
    role="menu"
  >
    <button
      type="button"
      role="menuitem"
      onclick={() =>
        void runAction(
          isProjectRunning(menuProject.path, $runningInstances) ? "stop" : "launch",
          menuProject,
        )}
      disabled={actionBusy || isProjectLaunching(menuProject.path, $launchSessions)}
    >
      {#if isProjectRunning(menuProject.path, $runningInstances)}
        <Square size={14} /> {$t("common.stop")}
      {:else}
        <Play size={14} /> {$t("common.play")}
      {/if}
    </button>
    <button type="button" role="menuitem" onclick={() => runAction("open-ide", menuProject)}>
      <Package size={14} /> {$t("library.openIde")}
    </button>
    <button type="button" role="menuitem" onclick={() => { ctxMenu = null; manageTarget = menuProject; }}>
      <SlidersHorizontal size={14} /> {$t("library.manageEllipsis")}
    </button>
    <button type="button" role="menuitem" onclick={() => { ctxMenu = null; renameTarget = menuProject; }}>
      <Pencil size={14} /> {$t("library.renameEllipsis")}
    </button>
    <button type="button" role="menuitem" onclick={() => void runAction("change-group", menuProject)}>
      <Tags size={14} /> {$t("library.changeGroup")}
    </button>
    <button type="button" role="menuitem" onclick={() => void runAction("folder", menuProject)}>
      <Folder size={14} /> {$t("library.folder")}
    </button>
    <button type="button" role="menuitem" onclick={() => void runAction("change-icon", menuProject)} disabled={actionBusy}>
      <ImageIcon size={14} /> {$t("library.changeIcon")}
    </button>
    {#if instanceIcons[menuProject.path]}
      <button type="button" role="menuitem" onclick={() => void runAction("clear-icon", menuProject)} disabled={actionBusy}>
        <Eraser size={14} /> {$t("library.clearIcon")}
      </button>
    {/if}
    <button type="button" role="menuitem" onclick={() => void runAction("copy", menuProject)}>
      <Copy size={14} /> {$t("library.copyInstance")}
    </button>
    <button type="button" role="menuitem" onclick={() => void runAction("shortcut", menuProject)}>
      <Link2 size={14} /> {$t("library.createShortcut")}
    </button>
    <button type="button" role="menuitem" onclick={() => void runAction("export-mrpack", menuProject)} disabled={actionBusy}>
      <Package size={14} /> {$t("home.exportMrpack")}
    </button>
    <button type="button" role="menuitem" onclick={() => void runAction("export-server", menuProject)} disabled={actionBusy}>
      <Server size={14} /> {$t("home.serverPack")}
    </button>
    <div class="menu-sep"></div>
    <button type="button" role="menuitem" onclick={() => void runAction("copy-path", menuProject)}>
      <Copy size={14} /> {$t("library.copyPath")}
    </button>
    <button type="button" role="menuitem" onclick={() => void runAction("repair", menuProject)} disabled={actionBusy}>
      <Wrench size={14} /> {$t("library.repair")}
    </button>
    <div class="menu-sep"></div>
    <button type="button" role="menuitem" onclick={() => void runAction("remove", menuProject)}>
      <Minus size={14} /> {$t("library.removeFromLibrary")}
    </button>
    <button type="button" role="menuitem" class="danger" onclick={() => void runAction("delete", menuProject)}>
      <Trash2 size={14} /> {$t("library.deleteFromDisk")}
    </button>
  </div>
{/if}

{#if manageTarget}
  <InstanceManager
    project={manageTarget}
    onclose={() => (manageTarget = null)}
    onBrowseMods={() => libraryTabRequest.set("discover")}
  />
{/if}

{#if renameTarget}
  <PromptDialog
    title={$t("library.renameInstance")}
    message={$t("library.newNameFor", { name: renameTarget.info.name })}
    mode="text"
    defaultValue={renameTarget.info.name}
    confirmLabel={$t("library.rename")}
    onconfirm={(v) => void confirmRename(v)}
    oncancel={() => (renameTarget = null)}
  />
{/if}

{#if showClonePrompt && cloneTarget}
  <PromptDialog
    title={$t("library.copyInstance")}
    message={$t("library.copyOf", { name: cloneTarget.info.name })}
    mode="text"
    defaultValue={clonePromptName}
    confirmLabel={$t("common.copy")}
    onconfirm={(v) => confirmClone(v)}
    oncancel={() => {
      showClonePrompt = false;
      cloneTarget = null;
    }}
  />
{/if}

{#if githubImportOpen}
  <PromptDialog
    title={$t("library.importGithub")}
    message={$t("library.githubMsg")}
    mode="text"
    defaultValue=""
    confirmLabel={$t("library.preview")}
    onconfirm={(v) => void confirmGithubImport(v)}
    oncancel={() => (githubImportOpen = false)}
  />
{/if}

{#if githubConfirmOpen}
  <ConfirmDialog
    title={$t("library.installGithubPack")}
    message={githubInspectSummary}
    confirmLabel={$t("manager.install")}
    onconfirm={() => void confirmGithubInstall()}
    oncancel={() => (githubConfirmOpen = false)}
  />
{/if}

<GithubPackInstallProgress active={githubInstallActive} onclose={() => (githubInstallActive = false)} />

{#if showGroupPrompt && groupTarget}
  <div
    class="group-dialog-backdrop"
    use:portal
    style="position:fixed; inset:0; z-index:10000;"
    role="presentation"
    onclick={() => { showGroupPrompt = false; groupTarget = null; }}
  >
    <div
      class="group-dialog"
      role="dialog"
      aria-labelledby="group-dlg-title"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <h3 id="group-dlg-title">{$t("library.groupDialogTitle")}</h3>
      <p>{$t("library.groupDialogBody", { name: groupTarget.info.name })}</p>
      <div class="group-chips">
        {#each existingGroups as g (g)}
          <button type="button" class="chip" class:active={groupPromptName === g} onclick={() => applyExistingGroup(g)}>
            {g}
          </button>
        {/each}
      </div>
      <label class="group-new-label" for="group-new-input">{$t("library.groupOrNew")}</label>
      <input id="group-new-input" bind:value={groupPromptName} onkeydown={(e) => e.key === "Enter" && confirmGroup(groupPromptName)} />
      <div class="group-dlg-actions">
        <button type="button" class="ghost" onclick={() => { showGroupPrompt = false; groupTarget = null; }}>{$t("common.cancel")}</button>
        <button type="button" class="accent" onclick={() => confirmGroup(groupPromptName)}>{$t("common.apply")}</button>
      </div>
    </div>
  </div>
{/if}

<svelte:window onmousedown={onGlobalPointerDown} onkeydown={onGlobalKeydown} />

<style>
  .prism-lib {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    height: 100%;
    border: none;
    border-radius: 0;
    background: transparent;
    overflow: hidden;
    position: relative;
  }
  .lib-toolbar-enter {
    animation: lib-toolbar-in 160ms var(--ease-out) both;
  }
  .lib-side-enter {
    animation: lib-side-in 160ms var(--ease-out) both;
  }

  .prism-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 4px 8px;
    border-bottom: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    flex-wrap: wrap;
    position: relative;
    z-index: 2;
  }
  .tb-left {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  /* Library tabs (Your packs / Discover / Create) rendered inside the toolbar. */
  .tb-leading {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-right: 10px;
    margin-right: 4px;
    border-right: 1px solid var(--border-color);
    flex-shrink: 0;
  }
  .tb-right { margin-left: auto; }
  /* Instance filter — live search over name / version / loader. */
  .tb-search {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-width: 180px;
    max-width: 260px;
    padding: 5px 8px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    color: var(--text-muted);
    transition: border-color var(--motion-fast) var(--ease-out), box-shadow var(--motion-fast) var(--ease-out);
  }
  .tb-search:focus-within {
    border-color: var(--accent-primary);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent-primary) 18%, transparent);
  }
  .tb-search :global(.tb-search-icon) {
    flex-shrink: 0;
    pointer-events: none;
  }
  .tb-search input {
    flex: 1;
    min-width: 0;
    border: none;
    background: transparent;
    color: var(--text-primary);
    font: inherit;
    font-size: 13px;
    outline: none;
  }
  .tb-search input::placeholder { color: var(--text-muted); }
  .tb-search-clear {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 2px;
    border: none;
    border-radius: var(--border-radius-sm, 6px);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }
  .tb-search-clear:hover { color: var(--text-primary); background: var(--bg-hover); }

  /* Sort selector — compact, matches toolbar buttons. */
  .tb-sort {
    padding: 7px 24px 7px 10px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    outline: none;
    appearance: none;
    background-image: linear-gradient(45deg, transparent 50%, var(--text-muted) 50%),
      linear-gradient(135deg, var(--text-muted) 50%, transparent 50%);
    background-position: calc(100% - 14px) 55%, calc(100% - 9px) 55%;
    background-size: 5px 5px;
    background-repeat: no-repeat;
    transition: border-color var(--motion-fast) var(--ease-out), color var(--motion-fast) var(--ease-out);
  }
  .tb-sort:hover { border-color: color-mix(in srgb, var(--accent-primary) 40%, var(--border-color)); color: var(--text-primary); }
  .tb-sort:focus-visible { outline: 2px solid var(--accent-primary); outline-offset: 1px; }


  .tb-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 7px 12px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--motion-ease),
      border-color var(--motion-fast) var(--motion-ease),
      color var(--motion-fast) var(--motion-ease);
  }
  .tb-btn:hover {
    background: var(--bg-tertiary);
    border-color: color-mix(in srgb, var(--accent-primary) 35%, var(--border-color));
    color: var(--text-primary);
  }
  .tb-btn:active:not(:disabled) {
    background: var(--bg-active);
  }
  /* Task: Ore UI treatment for the primary toolbar key (Add Instance). */
  .tb-btn.primary {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
    border-bottom-color: color-mix(in srgb, var(--accent-primary) 60%, #000);
    color: var(--on-accent);
    font-weight: 700;
  }
  .tb-btn.primary:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
    border-bottom-color: color-mix(in srgb, var(--accent-primary) 50%, #000);
    color: var(--on-accent);
    box-shadow: 0 4px 14px color-mix(in srgb, var(--accent-primary) 28%, transparent);
  }
  .tb-btn.primary:active:not(:disabled) {
    background: color-mix(in srgb, var(--accent-primary) 85%, #000);
  }
  .tb-btn:disabled { opacity: 0.5; cursor: default; }
  .tb-add-wrap,
    .tb-folders-wrap,
    .tb-export-wrap,
    .side-more-wrap {
      position: relative;
    }
  .tb-menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 40;
    min-width: 220px;
    padding: 6px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated, #1a1f28);
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.4);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .tb-menu.side-menu { left: auto; right: 0; }
  .tb-menu .menu-sep {
    height: 1px;
    background: var(--border-color);
    margin: 4px 2px;
  }
  .tb-menu button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    padding: 8px 10px;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .tb-menu button:hover {
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    color: var(--accent-primary);
  }
  .tb-account {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 4px 10px 4px 4px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    color: var(--text-primary);
    cursor: pointer;
  }
  .tb-account-name { font-size: 12px; font-weight: 700; max-width: 120px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tb-account-badge {
    font-size: 12px;
    font-weight: 700;
    color: var(--text-muted);
    text-transform: uppercase;
  }

  .prism-body {
    display: grid;
    grid-template-columns: minmax(0, 1fr) calc(clamp(300px, 26vw, 460px) * var(--side-scale, 1));
    flex: 1;
    min-height: 0;
  }
  .prism-body.is-dragging {
    cursor: grabbing;
    user-select: none;
  }
  .prism-body.is-dragging .inst-tile,
  .prism-body.is-dragging .inst-row {
    cursor: grabbing;
  }
  @media (max-width: 720px) {
    .prism-body { grid-template-columns: 1fr; }
    .prism-side { border-left: none; border-top: 1px solid var(--border-color); }
  }

  .prism-grid-pane {
    padding: 8px 10px 12px;
    overflow: auto;
    min-height: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .lib-page-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
    margin: 4px 2px 14px;
  }
  .lib-page-title {
    display: flex;
    align-items: baseline;
    gap: 10px;
    min-width: 0;
  }
  .lib-title {
    margin: 0;
    font-size: 26px;
    line-height: 1.1;
    font-weight: 800;
    letter-spacing: -0.02em;
    color: var(--text-primary);
  }
  .lib-title-count {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
  }
  .lib-filterbar {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }
  .lib-footer {
    margin-top: auto;
    padding-top: 12px;
    border-top: 1px solid var(--border-color);
    display: flex;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
  }
  .lib-footer span:last-child {
    color: var(--accent-primary);
  }
  .drag-hint {
    margin: 0 0 10px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent-primary);
    opacity: 0;
    max-height: 0;
    overflow: hidden;
    transition: opacity 0.15s ease, max-height 0.15s ease;
  }
  .drag-hint.visible {
    opacity: 1;
    max-height: 24px;
  }
  .inst-group {
    margin-bottom: 14px;
  }
  .group-header {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 5px 12px;
    margin-bottom: 10px;
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    border-radius: 999px;
    transition:
      color var(--motion-fast) var(--ease-out),
      background var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out),
      border-color var(--motion-fast) var(--ease-out);
  }
  .group-header:hover {
    color: var(--text-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 35%, var(--border-color));
    background: var(--bg-tertiary);
  }
  .group-header.drop-target {
    background: color-mix(in srgb, var(--accent-primary) 18%, transparent);
    color: var(--accent-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 45%, transparent);
  }
  .group-count {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
  }
  .inst-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 12px;
  }
  .inst-tile {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
    padding: 10px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    cursor: pointer;
    text-align: center;
    outline: none;
    touch-action: manipulation;
    position: relative;
    transition:
      transform var(--motion-fast) var(--ease-out),
      background var(--motion-fast) var(--ease-out),
      border-color var(--motion-fast) var(--ease-out),
      opacity var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out);
  }
  .inst-tile:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 35%, var(--border-color));
    background: var(--bg-tertiary);
  }
  .inst-tile:hover .inst-icon {
    transform: translateY(-1px);
    filter: brightness(1.04);
  }
  .inst-tile:hover:not(.selected) .inst-name {
    color: var(--text-primary);
  }
  .inst-tile.selected {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, var(--border-color));
    background: color-mix(in srgb, var(--accent-primary) 8%, var(--bg-secondary));
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent-primary) 30%, transparent);
  }
  .inst-tile.selected .inst-name {
    color: var(--accent-primary);
  }
  .inst-tile.running .inst-icon {
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent-primary) 55%, transparent);
  }
  .inst-tile.running::after {
    content: "";
    position: absolute;
    top: 10px;
    right: 14px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent-primary);
    box-shadow: 0 0 0 2px var(--bg-primary, transparent);
    z-index: 3;
  }
  .inst-tile.dragging {
    opacity: 0.28;
    filter: saturate(0.7);
  }
  .drag-mode .inst-tile:not(.dragging):not(.drop-target),
  .drag-mode .inst-row:not(.dragging):not(.drop-target) {
    opacity: 0.7;
  }
  .inst-tile.drop-target {
    background: color-mix(in srgb, var(--accent-primary) 22%, transparent);
    border-color: color-mix(in srgb, var(--accent-primary) 55%, transparent);
    z-index: 2;
  }
  .inst-tile.holding .inst-icon {
    filter: brightness(0.96);
  }
  .inst-tile.holding .hold-ring {
    opacity: 1;
    animation: lib-hold-ring 420ms linear forwards;
  }
  .inst-tile:focus-visible {
    outline: none;
  }
  .inst-tile:focus-visible .inst-icon {
    box-shadow:
      0 0 0 2px color-mix(in srgb, var(--accent-primary) 70%, transparent),
      0 0 0 4px color-mix(in srgb, var(--accent-primary) 22%, transparent);
  }
  .inst-tile:focus-visible .inst-name {
    outline: 2px solid color-mix(in srgb, var(--accent-primary) 55%, transparent);
    outline-offset: 1px;
  }

  .hold-ring {
    --hold: 0deg;
    position: absolute;
    top: 8px;
    left: 50%;
    width: 82px;
    height: 82px;
    margin-left: -41px;
    border-radius: 50%;
    opacity: 0;
    pointer-events: none;
    background: conic-gradient(
      from -90deg,
      var(--accent-primary) var(--hold),
      transparent 0
    );
    -webkit-mask: radial-gradient(farthest-side, transparent calc(100% - 2px), #000 calc(100% - 1px));
    mask: radial-gradient(farthest-side, transparent calc(100% - 2px), #000 calc(100% - 1px));
    z-index: 1;
  }

  .inst-icon {
    width: 100%;
    aspect-ratio: 16 / 10;
    border-radius: var(--border-radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 34px;
    font-weight: 900;
    color: #fff;
    text-shadow: 0 2px 8px rgba(0, 0, 0, 0.35);
    flex-shrink: 0;
    position: relative;
    z-index: 2;
    transition:
      transform var(--motion-fast) var(--ease-out),
      border-radius var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out),
      filter var(--motion-fast) var(--ease-out);
  }
  .inst-tile:hover .inst-icon,
  .inst-tile.selected .inst-icon,
  .inst-tile.running .inst-icon,
  .inst-tile:focus-visible .inst-icon {
    border-radius: var(--border-radius-lg);
  }
  .inst-icon.folder-preview {
    border-radius: var(--border-radius-lg);
  }

  /* Instance listing icon: cover the whole tile face with crisp pixel art. */
  .inst-icon-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    image-rendering: pixelated;
    border-radius: inherit;
  }

  .folder-stack {
    position: relative;
    width: 100%;
    height: 100%;
  }
  .folder-stack .stack-a,
  .folder-stack .stack-b {
    position: absolute;
    width: 34px;
    height: 34px;
    border-radius: var(--border-radius-sm);
    background: rgba(0, 0, 0, 0.28);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 16px;
    font-weight: 800;
  }
  .folder-stack .stack-a { top: 10px; left: 10px; }
  .folder-stack .stack-b { bottom: 10px; right: 10px; }

  /* ── Tile hover quick actions (home-card parity) ──────────────────
     Scrim + Play / IDE / Folder revealed on hover or keyboard focus with
     the same calm delayed reveal the home shelf uses. Buttons stop event
     propagation in markup so they never start a tile drag/hold. */
  .tile-actions {
    position: absolute;
    inset: auto 0 0 0;
    z-index: 3;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 16px 6px 6px;
    border-radius: 0 0 var(--border-radius-sm) var(--border-radius-sm);
    background: linear-gradient(180deg, transparent 0%, rgba(0, 0, 0, 0.62) 78%);
    opacity: 0;
    transform: translateY(4px);
    pointer-events: none;
    transition:
      opacity var(--motion-fast, 160ms) var(--ease-out),
      transform var(--motion-fast, 160ms) var(--ease-out);
    transition-delay: var(--motion-hover-delay, 70ms);
  }
  .inst-tile:hover .tile-actions,
  .inst-tile:focus-within .tile-actions {
    opacity: 1;
    transform: translateY(0);
    pointer-events: auto;
    transition-delay: var(--motion-hover-delay, 70ms);
  }
  /* Hidden while a drag/hold/folder-drop is in progress on the tile. */
  .inst-tile.holding .tile-actions,
  .inst-tile.dragging .tile-actions,
  .inst-tile.drop-target .tile-actions,
  .drag-mode .inst-tile .tile-actions {
    opacity: 0;
    pointer-events: none;
    transition-delay: 0ms;
  }
  :global(.potato-pc) .tile-actions {
    transition: none;
  }
  .tile-play {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    padding: 0 10px;
    border: none;
    border-radius: 999px;
    background: var(--accent-primary);
    color: var(--on-accent, #fff);
    font-size: 12px;
    font-weight: 800;
    cursor: pointer;
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.4);
    transform: none;
  }
  .tile-play.stop {
    background: var(--accent-danger, #ef4444);
  }
  .tile-play:disabled {
    cursor: wait;
    opacity: 0.85;
  }
  .tile-play:hover {
    filter: brightness(1.08);
  }
  .tile-acts {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .tile-act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: var(--border-radius-sm);
    background: rgba(0, 0, 0, 0.55);
    color: #fff;
    cursor: pointer;
    transform: none;
  }
  .tile-act:hover {
    background: rgba(0, 0, 0, 0.78);
  }

  /* Last-played line under the version — makes recency glanceable. */
  .inst-last {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── List view: dense rows, same selection/hover language as tiles ── */
  .inst-rows {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .inst-row {
    display: grid;
    grid-template-columns: 44px minmax(0, 1fr) auto auto;
    gap: 12px;
    align-items: center;
    padding: 8px 12px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    cursor: pointer;
    outline: none;
    position: relative;
    transition:
      border-color var(--motion-fast) var(--ease-out),
      background var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out);
  }
  .inst-row:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 35%, var(--border-color));
    background: var(--bg-tertiary);
  }
  .inst-row.selected {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, var(--border-color));
    background: color-mix(in srgb, var(--accent-primary) 8%, var(--bg-secondary));
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent-primary) 30%, transparent);
  }
  .inst-row.selected .row-name {
    color: var(--accent-primary);
  }
  .inst-row.drop-target {
    background: color-mix(in srgb, var(--accent-primary) 22%, transparent);
    border-color: color-mix(in srgb, var(--accent-primary) 55%, transparent);
  }
  .inst-row.dragging {
    opacity: 0.28;
  }
  .inst-row:focus-visible {
    outline: 2px solid color-mix(in srgb, var(--accent-primary) 70%, transparent);
    outline-offset: 1px;
  }
  .row-icon {
    position: relative;
    width: 44px;
    height: 44px;
    border-radius: var(--border-radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 19px;
    font-weight: 900;
    color: #fff;
    overflow: hidden;
    flex-shrink: 0;
  }
  .row-icon .inst-icon-img {
    border-radius: 0;
  }
  .row-running-dot {
    position: absolute;
    right: -2px;
    bottom: -2px;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: var(--accent-primary);
    border: 2px solid var(--bg-primary);
  }
  .row-main {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }
  .row-name {
    font-size: 13px;
    font-weight: 700;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row-sub {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row-stats {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 8px;
    flex-shrink: 0;
  }
  .row-stat {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
  }
  .row-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }
  .row-play {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 28px;
    padding: 0 11px;
    border: none;
    border-radius: 999px;
    background: var(--accent-primary);
    color: var(--on-accent, #fff);
    font-size: 12px;
    font-weight: 800;
    cursor: pointer;
    transform: none;
  }
  .row-play.stop {
    background: var(--accent-danger, #ef4444);
  }
  .row-play:disabled {
    cursor: wait;
    opacity: 0.85;
  }
  .row-play:hover {
    filter: brightness(1.08);
  }
  .row-act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    cursor: pointer;
    transform: none;
  }
  .row-act:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 40%, var(--border-color));
    color: var(--text-primary);
  }

  /* Grid ⇄ list segmented toggle in the filter bar. */
  .view-toggle {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 2px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    flex-shrink: 0;
  }
  .view-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: 999px;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    transform: none;
  }
  .view-btn:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }
  .view-btn.active {
    background: color-mix(in srgb, var(--accent-primary) 16%, transparent);
    color: var(--accent-primary);
  }

  .inst-name {
    font-size: 13px;
    font-weight: 700;
    color: var(--text-primary);
    max-width: 100%;
    width: 100%;
    line-height: 1.3;
    white-space: normal;
    overflow: hidden;
    word-break: break-word;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    padding: 0 2px;
    box-sizing: border-box;
    transition:
      color var(--motion-fast) var(--ease-out),
      background var(--motion-fast) var(--ease-out),
      border-color var(--motion-fast) var(--ease-out);
  }
  .inst-version {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-muted);
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 0 2px;
    box-sizing: border-box;
  }
  .inst-tile.drop-target .inst-name { color: var(--accent-primary); }

  .drag-ghost {
    position: fixed;
    z-index: 200;
    width: 64px;
    height: 64px;
    margin: -32px 0 0 -32px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 24px;
    font-weight: 900;
    color: #fff;
    pointer-events: none;
    box-shadow: 0 10px 24px rgba(0, 0, 0, 0.45), 0 0 0 1px color-mix(in srgb, var(--accent-primary) 30%, transparent);
    opacity: 0.95;
    will-change: left, top;
  }
  .ghost-letter { position: relative; z-index: 1; }
  .ghost-ring {
    position: absolute;
    inset: -5px;
    border-radius: 50%;
    border: 1px solid color-mix(in srgb, var(--accent-primary) 40%, transparent);
    opacity: 0.7;
  }

  .prism-side {
    border-left: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    padding: 20px 18px;
    display: flex;
    flex-direction: column;
    gap: 16px;
    overflow: auto;
  }
  .side-panel { display: flex; flex-direction: column; gap: 16px; }
  .side-hero { text-align: center; min-width: 0; }
  .side-icon {
    width: 104px;
    height: 104px;
    margin: 0 auto 12px;
    border-radius: var(--border-radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    font-size: 40px;
    font-weight: 900;
    color: #fff;
    box-shadow: 0 8px 20px rgba(0, 0, 0, 0.32);
  }
  .side-icon-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    image-rendering: pixelated;
  }
  .side-title {
    font-size: 18px;
    font-weight: 700;
    color: var(--text-primary);
    margin-bottom: 4px;
    line-height: 1.3;
    word-break: break-word;
  }
  .side-meta {
    font-size: 13px;
    color: var(--text-muted);
    text-transform: capitalize;
  }
  .side-actions {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  /* Labeled secondaries under Play — IDE + folder, the two most common
     destinations, promoted out of the 32px icon row. */
  .side-secondary-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }
  .side-secondary.manage {
    grid-column: 1 / -1;
    border-color: color-mix(in srgb, var(--accent-primary) 30%, var(--border-color));
    color: var(--accent-primary);
  }
  .side-secondary.manage:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent-primary) 50%, var(--border-color));
    background: color-mix(in srgb, var(--accent-primary) 10%, var(--bg-hover));
    color: var(--accent-primary);
  }
  .side-secondary {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 9px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 700;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
    transform: none;
    transition:
      border-color var(--motion-fast) var(--ease-out),
      background var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }
  .side-secondary:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent-primary) 40%, var(--border-color));
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .side-secondary:disabled {
    opacity: 0.55;
    cursor: default;
  }
  /* Update-center: amber count chip on the tile icon + row badge + side CTA. */
  .tile-upd {
    position: absolute;
    top: 6px;
    right: 6px;
    z-index: 2;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 20px;
    height: 20px;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--accent-warning);
    color: #1a1a1a;
    font-size: 12px;
    font-weight: 800;
    pointer-events: none;
  }
  .row-upd {
    display: inline-flex;
    align-items: center;
    margin-left: 6px;
    padding: 0 6px;
    height: 16px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent-warning) 18%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent-warning) 45%, transparent);
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 800;
    vertical-align: middle;
  }
  .side-updates {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 11px;
    border: 1px solid color-mix(in srgb, var(--accent-warning) 45%, var(--border-color));
    border-radius: var(--border-radius-sm);
    background: color-mix(in srgb, var(--accent-warning) 10%, transparent);
    color: var(--text-primary);
    font-size: 12px;
    cursor: pointer;
    transform: none;
    text-align: left;
    transition:
      border-color var(--motion-fast) var(--ease-out),
      background var(--motion-fast) var(--ease-out);
  }
  .side-updates:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-warning) 16%, transparent);
    border-color: color-mix(in srgb, var(--accent-warning) 65%, var(--border-color));
  }
  .side-updates:disabled {
    opacity: 0.6;
    cursor: wait;
  }
  .side-updates :global(svg) {
    color: var(--accent-warning);
    flex-shrink: 0;
  }
  .side-updates-text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }
  .side-updates strong {
    color: var(--accent-warning);
    white-space: nowrap;
  }

  /* Prism-style per-instance notes — autosaving, keyed by instance path. */
  .side-notes {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .side-notes-label {
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .side-notes-input {
    width: 100%;
    min-height: 58px;
    max-height: 130px;
    padding: 7px 9px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-size: 12px;
    line-height: 1.45;
    resize: vertical;
    outline: none;
  }
  .side-notes-input:focus {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, var(--border-color));
  }
  .side-notes-input::placeholder {
    color: var(--text-muted);
  }

  /* Group chip under the side hero — shows where the pack lives and opens
     the change-group prompt on click. */
  .side-group-chip {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    max-width: 100%;
    margin-top: 8px;
    padding: 3px 10px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--accent-primary) 25%, var(--border-color));
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
    transform: none;
    transition:
      border-color var(--motion-fast) var(--ease-out),
      background var(--motion-fast) var(--ease-out);
  }
  .side-group-chip:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, var(--border-color));
    background: color-mix(in srgb, var(--accent-primary) 14%, transparent);
  }
  .side-group-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .side-btn {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 12px 14px;
    border-radius: var(--border-radius-sm);
    border: 1px solid transparent;
    background: transparent;
    color: var(--text-primary);
    font-size: 15px;
    font-weight: 600;
    cursor: pointer;
    text-align: left;
    transition:
      background var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      border-color var(--motion-fast) var(--ease-out);
  }
  .side-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-primary) 10%, transparent);
    color: var(--accent-primary);
  }
  .side-btn:active:not(:disabled) { opacity: 0.9; }
  .side-btn:disabled { opacity: 0.4; cursor: default; }
  .side-btn.launch {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
    border-bottom-color: color-mix(in srgb, var(--accent-primary) 60%, #000);
    color: var(--on-accent);
    font-size: 16px;
    font-weight: 700;
    padding: 15px 14px;
    margin-bottom: 2px;
    justify-content: center;
    box-shadow: 0 6px 18px color-mix(in srgb, var(--accent-primary) 30%, transparent);
  }
  .side-btn.launch:hover:not(:disabled) {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
    color: var(--on-accent);
    box-shadow: 0 8px 22px color-mix(in srgb, var(--accent-primary) 38%, transparent);
  }
  .side-btn.launch.stop {
    background: var(--accent-danger, #ef4444);
    border-color: var(--accent-danger, #ef4444);
    border-bottom-color: color-mix(in srgb, var(--accent-danger, #ef4444) 60%, #000);
    color: var(--on-accent, #fff);
  }
  .side-btn.launch.stop:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-danger, #ef4444) 90%, #000);
    border-color: color-mix(in srgb, var(--accent-danger, #ef4444) 90%, #000);
    color: var(--on-accent, #fff);
    box-shadow: 0 8px 22px color-mix(in srgb, var(--accent-danger, #ef4444) 38%, transparent);
  }
  .side-icon-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .side-icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    height: 38px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      border-color var(--motion-fast) var(--ease-out);
  }
  .side-icon-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-primary) 10%, transparent);
    border-color: color-mix(in srgb, var(--accent-primary) 35%, var(--border-color));
    color: var(--accent-primary);
  }
  .side-icon-btn:disabled { opacity: 0.4; cursor: default; }
  .side-meta-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin-top: 2px;
  }
  .side-meta-item {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    min-width: 0;
  }
  .side-meta-label {
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-muted);
  }
  .side-meta-value {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .side-empty {
    padding: 24px 8px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 48px 24px;
    text-align: center;
    color: var(--text-muted);
  }
  .empty-state h3 { margin: 0; color: var(--text-primary); font-size: 16px; }
  .empty-state p { margin: 0; font-size: 13px; }
  .empty-cta {
    margin-top: 6px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    border: none;
    border-radius: var(--border-radius-md);
    background: var(--accent-primary);
    color: var(--on-accent, #000);
    font-weight: 700;
    font-size: 13px;
    cursor: pointer;
  }

  .pack-ctx-menu {
    position: fixed;
    z-index: 200;
    min-width: 210px;
    padding: 6px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated, #1a1f28);
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.45);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .pack-ctx-menu button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 10px;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    text-align: left;
  }
  .pack-ctx-menu button:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    color: var(--accent-primary);
  }
  .pack-ctx-menu button:disabled { opacity: 0.45; cursor: default; }
  .pack-ctx-menu button.danger:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-danger) 12%, transparent);
    color: var(--accent-danger);
  }
  .pack-ctx-menu .menu-sep {
    height: 1px;
    background: var(--border-color);
    margin: 4px 2px;
  }

  .group-dialog-backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
  }
  .group-dialog {
    width: min(400px, 100%);
    padding: 18px;
    border-radius: var(--border-radius-lg);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated, #1a1f28);
    box-shadow: 0 20px 48px rgba(0, 0, 0, 0.5);
  }
  .group-dialog h3 { margin: 0 0 6px; font-size: 16px; color: var(--text-primary); }
  .group-dialog p { margin: 0 0 12px; font-size: 13px; color: var(--text-muted); }
  .group-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 12px;
  }
  .chip {
    padding: 6px 10px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .chip.active,
  .chip:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 40%, transparent);
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 10%, transparent);
  }
  .group-new-label {
    display: block;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    margin-bottom: 4px;
  }
  .group-dialog input {
    width: 100%;
    height: 40px;
    padding: 0 12px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-primary);
    font-size: 13px;
    box-sizing: border-box;
  }
  .group-dlg-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 14px;
  }
  .group-dlg-actions .ghost {
    padding: 8px 12px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    font-weight: 600;
    font-size: 12px;
  }
  .group-dlg-actions .accent {
    padding: 8px 14px;
    border-radius: var(--border-radius-sm);
    border: none;
    background: var(--accent-primary);
    color: var(--on-accent, #000);
    cursor: pointer;
    font-weight: 700;
    font-size: 12px;
  }

  .mini-spinner {
    width: 14px;
    height: 14px;
    border: 2px solid color-mix(in srgb, var(--accent-primary) 25%, transparent);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    display: inline-block;
  }
  .spinning {
    display: inline-flex;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @property --hold {
    syntax: "<angle>";
    inherits: false;
    initial-value: 0deg;
  }

  @keyframes lib-hold-ring {
    from { --hold: 0deg; opacity: 1; }
    to { --hold: 360deg; opacity: 1; }
  }
  @keyframes lib-toolbar-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }
  @keyframes lib-side-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  /* Per-instance content drawer (mods/packs/shaders/servers status board). */
  .side-content {
    margin-top: 10px;
    border-top: 1px solid var(--border);
    padding-top: 8px;
  }

  .side-content-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 4px;
    background: transparent;
    border: none;
    border-radius: var(--border-radius-sm);
    cursor: pointer;
    color: var(--text);
  }

  .side-content-toggle:hover {
    background: var(--surface-hover);
  }

  .side-content-title {
    font-size: 12px;
    font-weight: 700;
  }

  .side-content-hint {
    flex: 1 1 auto;
    text-align: left;
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .side-content-toggle :global(.flipped) {
    transform: rotate(180deg);
  }

  .side-content-body {
    margin-top: 6px;
  }

  .drag-mode .prism-grid-pane {
    background: radial-gradient(ellipse at center, color-mix(in srgb, var(--accent-primary) 3%, transparent), transparent 70%);
  }

  :global(.potato-pc) .lib-toolbar-enter,
  :global(.potato-pc) .lib-side-enter,
  :global(.potato-pc) .hold-ring {
    animation: none !important;
  }
  :global(.potato-pc) .inst-tile:hover .inst-icon {
    transform: none !important;
    filter: none !important;
  }
  :global(.potato-pc) .drag-mode .inst-tile:not(.dragging):not(.drop-target),
  :global(.potato-pc) .drag-mode .inst-row:not(.dragging):not(.drop-target) {
    opacity: 1;
  }

  @media (prefers-reduced-motion: reduce) {
    .lib-toolbar-enter,
    .lib-side-enter,
    .hold-ring {
      animation: none !important;
    }
    .inst-tile:hover .inst-icon {
      transform: none !important;
      filter: none !important;
    }
    .drag-mode .inst-tile:not(.dragging):not(.drop-target) {
      opacity: 1;
    }
  }
</style>
