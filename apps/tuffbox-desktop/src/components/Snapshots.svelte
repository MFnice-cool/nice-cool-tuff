<script lang="ts">
  import {
    History, Plus, RefreshCw, RotateCcw, Calendar, GitCompare, FileText, Archive, Trash2,
    Search, ChevronDown, ChevronRight, ExternalLink, AlertTriangle, Sparkles, FolderOpen,
    ArrowRightLeft, Clock, Zap, Hand, ShieldAlert, Database, HardDrive, ShieldCheck,
  } from "@lucide/svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import SnapshotItem from "./SnapshotItem.svelte";
  import EmptyState from "./EmptyState.svelte";
  import {
    api,
    type BackupEntry,
    type ManifestSnapshotDiff,
    type PackSource,
    type PackStateDiff,
    type Snapshot,
    type SnapshotDetail,
    type SnapshotDiff,
    type SnapshotFileDiff,
    type SnapshotVsCurrent,
    type PruneResult,
  } from "../lib/api";
  import { historyFocusSnapshotId, ideStageRequest, projectPath } from "../lib/store";

  let snapshots = $state<Snapshot[]>([]);
  let loading = $state(false);
  let newName = $state("");
  let error = $state<string | null>(null);
  let message = $state<string | null>(null);
  let projectDir = $state<string | null>(null);
  let lastLoadedPath = $state<string | null>(null);
  let fromId = $state("");
  let toId = $state("");
  let diff = $state<SnapshotDiff | null>(null);
  let selectedDiffPath = $state("");
  let fileDiff = $state<SnapshotFileDiff | null>(null);
  let diffLoading = $state(false);

  let selectedId = $state("");
  let detail = $state<SnapshotDetail | null>(null);
  let detailLoading = $state(false);
  let search = $state("");
  let searchInput = $state<HTMLInputElement | null>(null);
  let filterKind = $state<"all" | "auto" | "manual" | "crash">("all");
  let backupsOpen = $state(false);
  let compareOpen = $state(false);

  // Diff vs current (project now) panel for the selected snapshot.
  let vsDiff = $state<SnapshotVsCurrent | null>(null);
  let vsLoading = $state(false);
  let vsSelectedPath = $state("");
  let vsFileDiff = $state<SnapshotFileDiff | null>(null);
  let vsFileLoading = $state(false);

  // Disk cleanup / prune.
  let cleanupOpen = $state(false);
  let pruneDays = $state<number>(30);
  let pruneLoading = $state(false);
  let pruneResult = $state<PruneResult | null>(null);

  let confirmOpen = $state(false);
  let confirmTitle = $state("");
  let confirmMessage = $state("");
  let confirmDanger = $state(false);
  let confirmAction = $state<(() => void) | null>(null);

  function showConfirm(title: string, message: string, action: () => void, danger = false) {
    confirmTitle = title;
    confirmMessage = message;
    confirmAction = action;
    confirmDanger = danger;
    confirmOpen = true;
  }

  function handleConfirm() {
    if (confirmAction) confirmAction();
    confirmOpen = false;
    confirmAction = null;
  }

  let manifestDiff = $state<ManifestSnapshotDiff | null>(null);
  let manifestDiffLoading = $state(false);

  // Pack Diff: compare across snapshots / backups / other instances.
  type DiffSourceKind = "snapshot" | "backup" | "manifest";
  let fromKind = $state<DiffSourceKind>("snapshot");
  let toKind = $state<DiffSourceKind>("snapshot");
  let otherManifestPath = $state("");
  let backupFromId = $state("");
  let backupToId = $state("");
  let packDiff = $state<PackStateDiff | null>(null);
  let packDiffLoading = $state(false);

  let backups = $state<BackupEntry[]>([]);
  let backupLoading = $state(false);
  let backupName = $state("");

  async function ensureProjectDir() {
    if (!$projectPath) return null;
    if (!projectDir || lastLoadedPath !== $projectPath) {
      projectDir = await api.project.getDir($projectPath);
    }
    return projectDir;
  }

  async function loadBackups() {
    if (!$projectPath) return;
    backupLoading = true;
    try {
      backups = await api.backups.list($projectPath);
    } catch {
      backups = [];
    } finally {
      backupLoading = false;
    }
  }

  async function createBackup() {
    if (!$projectPath) return;
    loading = true;
    try {
      await api.backups.create(backupName || null, $projectPath);
      backupName = "";
      await loadBackups();
      message = "Backup created.";
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function deleteBackup(id: string) {
    if (!$projectPath) return;
    showConfirm(
      "Delete backup",
      "Delete this backup permanently? This cannot be undone.",
      async () => {
        try {
          await api.backups.delete(id, $projectPath!);
          await loadBackups();
        } catch (e) {
          error = String(e);
        }
      },
      true,
    );
  }

  async function restoreBackup(id: string) {
    if (!$projectPath) return;
    showConfirm(
      "Restore backup",
      "Restore this backup? A safety snapshot will be created first.",
      async () => {
        loading = true;
        error = null;
        try {
          await api.backups.restore(id, $projectPath!);
          message = "Backup restored. A safety snapshot was created.";
          await load(true);
        } catch (e) {
          error = String(e);
        } finally {
          loading = false;
        }
      },
      true,
    );
  }

  function formatBytes(b: number) {
    if (b < 1024) return b + " B";
    if (b < 1048576) return (b / 1024).toFixed(1) + " KB";
    if (b < 1073741824) return (b / 1048576).toFixed(1) + " MB";
    return (b / 1073741824).toFixed(1) + " GB";
  }

  function formatDate(iso: string) {
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  }

  function formatRelative(iso: string) {
    try {
      const t = new Date(iso).getTime();
      const diffMs = Date.now() - t;
      const min = 60_000;
      if (diffMs < min) return "just now";
      if (diffMs < 60 * min) return `${Math.floor(diffMs / min)}m ago`;
      if (diffMs < 24 * 60 * min) return `${Math.floor(diffMs / (60 * min))}h ago`;
      if (diffMs < 7 * 24 * 60 * min) return `${Math.floor(diffMs / (24 * 60 * min))}d ago`;
      return new Date(iso).toLocaleDateString();
    } catch {
      return iso;
    }
  }

  function operationLabel(s: Snapshot): string {
    if (s.operation) return s.operation;
    if (s.name?.startsWith("auto-before-")) return s.name.slice("auto-before-".length);
    if (s.tags?.includes("crash_fix")) return "crash_fix";
    return s.name || "snapshot";
  }

  /** Keep implementation ids out of the primary history label. */
  function friendlySnapshotTitle(s: Snapshot): string {
    const operation = operationLabel(s).toLowerCase().replaceAll("_", "-");
    const summary = s.actionsSummary?.find(Boolean)?.trim();
    if (summary) {
      return summary
        .replace(/^(auto[- ]?before[- ]?)/i, "")
        .replace(/^(update|updated)\s+/i, "Updated ");
    }
    if (operation.includes("update-mod")) return "Mod update";
    if (operation.includes("add-mod")) return "Mod added";
    if (operation.includes("remove-mod")) return "Mod removed";
    if (operation.includes("disable")) return "Mod disabled";
    if (operation.includes("crash")) return "Crash recovery";
    if (s.name?.startsWith("auto-before-")) return "Automatic checkpoint";
    return s.name || "Snapshot";
  }

  function isCrash(s: Snapshot) {
    return !!s.tags?.includes("crash_fix") || operationLabel(s).includes("crash");
  }

  function isAuto(s: Snapshot) {
    return s.name?.startsWith("auto-") || s.actor === "launcher" || s.actor === "ai" || s.actor === "scan";
  }

  function isManual(s: Snapshot) {
    return s.actor === "user" || s.operation === "manual" || (!isAuto(s) && !isCrash(s));
  }

  function kindOf(s: Snapshot): "auto" | "manual" | "crash" {
    if (isCrash(s)) return "crash";
    if (isAuto(s)) return "auto";
    return "manual";
  }

  function kindLabel(kind: string): string {
    if (kind === "crash") return "Crash fix";
    if (kind === "auto") return "Auto";
    return "Manual";
  }

  /** Short prefixed id for tight badges ("bf3a…c21"). */
  function shortId(id: string) {
    if (id.length <= 9) return id;
    return `${id.slice(0, 4)}…${id.slice(-3)}`;
  }

  const totalBytes = $derived(snapshots.reduce((acc, s) => acc + (s.sizeBytes || 0), 0));
  const autoCount = $derived(snapshots.filter(isAuto).length);
  const manualCount = $derived(snapshots.filter(isManual).length);
  const crashCount = $derived(snapshots.filter(isCrash).length);

  function previewLine(s: Snapshot): string {
    const summary = s.actionsSummary?.filter(Boolean) ?? [];
    if (summary.length) return summary.slice(0, 2).join(" · ");
    return s.reason || "No action details";
  }

  const filtered = $derived((() => {
    const q = search.trim().toLowerCase();
    let list = [...snapshots].reverse();
    if (filterKind === "auto") list = list.filter(isAuto);
    else if (filterKind === "manual") list = list.filter(isManual);
    else if (filterKind === "crash") list = list.filter(isCrash);
    if (q) {
      list = list.filter((s) => {
        const hay = [
          s.name,
          s.reason,
          s.id,
          operationLabel(s),
          ...(s.actionsSummary ?? []),
          ...(s.tags ?? []),
        ]
          .join(" ")
          .toLowerCase();
        return hay.includes(q);
      });
    }
    return list;
  })());

  const stats = $derived({
    all: snapshots.length,
    auto: snapshots.filter(isAuto).length,
    manual: snapshots.filter(isManual).length,
    crash: snapshots.filter(isCrash).length,
  });

  function dayKey(iso: string): string {
    const d = new Date(iso);
    return isNaN(d.getTime()) ? "Other" : `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
  }
  function dayLabel(iso: string): string {
    const d = new Date(iso);
    if (isNaN(d.getTime())) return "Other";
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const start = new Date(d);
    start.setHours(0, 0, 0, 0);
    const diffDays = Math.round((today.getTime() - start.getTime()) / 86_400_000);
    if (diffDays === 0) return "Today";
    if (diffDays === 1) return "Yesterday";
    return d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
  }
  // Group filtered snapshots by local calendar day, newest day first.
  const timeline = $derived((() => {
    const groups = new Map<string, Snapshot[]>();
    for (const s of filtered) {
      const key = dayKey(s.createdAt);
      const arr = groups.get(key) ?? [];
      arr.push(s);
      groups.set(key, arr);
    }
    return Array.from(groups.entries())
      .sort((a, b) => (a[0] < b[0] ? 1 : -1))
      .map(([key, items]) => ({ key, label: dayLabel(items[0]?.createdAt ?? ""), items }));
  })());

  async function load(force = false) {
    if (!$projectPath) return;
    if (!force && lastLoadedPath === $projectPath && snapshots.length > 0) return;
    loading = true;
    error = null;
    try {
      const dir = await ensureProjectDir();
      if (!dir) return;
      snapshots = await api.snapshots.list(dir);
      lastLoadedPath = $projectPath;
      if (snapshots.length >= 2) {
        fromId ||= snapshots[snapshots.length - 2].id;
        toId ||= snapshots[snapshots.length - 1].id;
      }
      // Re-validate compare targets: a snapshot may have been deleted since
      // the last load, which would leave the compare panel pointing at a
      // non-existent id.
      const ids = new Set(snapshots.map((s) => s.id));
      if (fromId && !ids.has(fromId)) fromId = "";
      if (toId && !ids.has(toId)) toId = "";
      if (!fromId && !toId && snapshots.length >= 2) {
        fromId = snapshots[snapshots.length - 2].id;
        toId = snapshots[snapshots.length - 1].id;
      }
      if (selectedId && !snapshots.some((s) => s.id === selectedId)) {
        selectedId = "";
        detail = null;
      }
      const focusSnap = $historyFocusSnapshotId;
      if (focusSnap && snapshots.some((s) => s.id === focusSnap)) {
        historyFocusSnapshotId.set(null);
        await selectSnapshot(focusSnap);
      } else if (!selectedId && snapshots.length) {
        await selectSnapshot(snapshots[snapshots.length - 1].id);
      } else if (selectedId) {
        await selectSnapshot(selectedId);
      }
      await loadBackups();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  let snapshotRequestGen = 0;

  async function selectSnapshot(id: string) {
    selectedId = id;
    if (vsDiff) {
      vsDiff = null;
      vsFileDiff = null;
      vsSelectedPath = "";
    }
    const dir = await ensureProjectDir();
    if (!dir) return;
    const generation = ++snapshotRequestGen;
    detailLoading = true;
    error = null;
    try {
      const result = await api.snapshots.detail(id, dir);
      // Ignore stale responses if the user clicked another snapshot meanwhile.
      if (generation !== snapshotRequestGen) return;
      detail = result;
    } catch (e) {
      if (generation !== snapshotRequestGen) return;
      error = String(e);
      detail = null;
    } finally {
      if (generation === snapshotRequestGen) detailLoading = false;
    }
  }

  async function create() {
    if (!$projectPath) return;
    loading = true;
    error = null;
    message = null;
    try {
      const dir = await ensureProjectDir();
      if (!dir) return;
      const snap = await api.snapshots.create(newName || "manual", "Created from UI", dir);
      newName = "";
      await load(true);
      await selectSnapshot(snap.id);
      message = "Snapshot created.";
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function rollback(id: string) {
    if (!$projectPath) return;
    const s = snapshots.find((x) => x.id === id);
    const manifestOnly = detail?.snapshot.id === id ? detail.manifestOnly : !(s?.changedFiles?.length);
    const warn = manifestOnly
      ? "\n\nThis snapshot has no tracked file copies — rollback restores manifest/lockfile only. Mod jars on disk may differ; check History."
      : "";
    showConfirm(
      "Rollback snapshot",
      `Rollback project to snapshot ${id}? This will restore manifest and changed files.${warn}`,
      async () => {
        loading = true;
        error = null;
        message = null;
        try {
          const dir = await ensureProjectDir();
          if (!dir) return;
          await api.snapshots.rollback(id, dir);
          message = `Rolled back to ${id}.`;
          await load(true);
        } catch (e) {
          error = String(e);
        } finally {
          loading = false;
        }
      },
      true,
    );
  }

  async function removeSnapshot(id: string) {
    showConfirm(
      "Delete snapshot",
      `Permanently delete snapshot ${id}? This cannot be undone.`,
      async () => {
        loading = true;
        error = null;
        try {
          const dir = await ensureProjectDir();
          if (!dir) return;
          await api.snapshots.delete(id, dir);
          if (selectedId === id) {
            selectedId = "";
            detail = null;
          }
          message = "Snapshot deleted.";
          await load(true);
        } catch (e) {
          error = String(e);
        } finally {
          loading = false;
        }
      },
      true,
    );
  }

  function compareWithPrevious(id: string) {
    const idx = snapshots.findIndex((s) => s.id === id);
    if (idx <= 0) {
      error = "No previous snapshot to compare with.";
      return;
    }
    fromId = snapshots[idx - 1].id;
    toId = id;
    compareOpen = true;
    compare();
  }

  function swapCompare() {
    const tmp = fromId;
    fromId = toId;
    toId = tmp;
    void compare();
  }

  function openInHistory(id: string) {
    historyFocusSnapshotId.set(id);
    ideStageRequest.set("history");
  }

  async function compare() {
    if (!$projectPath || !fromId || !toId || fromId === toId) return;
    error = null;
    message = null;
    fileDiff = null;
    selectedDiffPath = "";
    try {
      const dir = await ensureProjectDir();
      if (!dir) return;
      diff = await api.snapshots.diff(fromId, toId, dir);
      selectedDiffPath = "";
      fileDiff = null;
    } catch (e) {
      error = String(e);
    }
  }

  async function loadManifestDiff() {
    const dir = await ensureProjectDir();
    if (!dir || !fromId || !toId) return;
    manifestDiffLoading = true;
    error = null;
    try {
      manifestDiff = await api.snapshots.diffManifest(fromId, toId, dir);
    } catch (e) {
      error = String(e);
    } finally {
      manifestDiffLoading = false;
    }
  }

  function packSourceFor(kind: DiffSourceKind, side: "from" | "to"): PackSource | null {
    const dir = projectDir ?? "";
    if (side === "from") {
      if (kind === "snapshot")
        return fromId ? { type: "snapshot", projectDir: dir, snapshotId: fromId } : null;
      if (kind === "backup")
        return backupFromId ? { type: "backup", projectDir: dir, backupId: backupFromId } : null;
      return otherManifestPath ? { type: "manifest", path: otherManifestPath } : null;
    }
    if (kind === "snapshot")
      return toId ? { type: "snapshot", projectDir: dir, snapshotId: toId } : null;
    if (kind === "backup")
      return backupToId ? { type: "backup", projectDir: dir, backupId: backupToId } : null;
    return otherManifestPath ? { type: "manifest", path: otherManifestPath } : null;
  }

  async function runPackDiff() {
    const dir = await ensureProjectDir();
    if (!dir) return;
    const a = packSourceFor(fromKind, "from");
    const b = packSourceFor(toKind, "to");
    if (!a || !b) {
      error = "Pick both sources first.";
      return;
    }
    packDiffLoading = true;
    error = null;
    packDiff = null;
    try {
      packDiff = await api.packDiff.compare(a, b);
    } catch (e) {
      error = String(e);
    } finally {
      packDiffLoading = false;
    }
  }

  async function openFileDiff(path: string) {
    const dir = await ensureProjectDir();
    if (!dir || !fromId || !toId) return;
    selectedDiffPath = path;
    diffLoading = true;
    error = null;
    try {
      fileDiff = await api.snapshots.fileDiff(fromId, toId, path, dir);
    } catch (e) {
      error = String(e);
    } finally {
      diffLoading = false;
    }
  }

  function focusSearch(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
      event.preventDefault();
      searchInput?.focus();
      searchInput?.select();
    }
  }

  async function loadVsCurrent(id: string) {
    const dir = await ensureProjectDir();
    if (!dir || !id) return;
    vsDiff = null;
    vsFileDiff = null;
    vsSelectedPath = "";
    vsLoading = true;
    error = null;
    try {
      vsDiff = await api.snapshots.diffVsCurrent(id, dir);
    } catch (e) {
      error = String(e);
      vsDiff = null;
    } finally {
      vsLoading = false;
    }
  }

  async function openVsFile(path: string) {
    const dir = await ensureProjectDir();
    if (!dir || !selectedId) return;
    vsSelectedPath = path;
    vsFileLoading = true;
    error = null;
    try {
      vsFileDiff = await api.snapshots.fileDiffVsCurrent(selectedId, path, dir);
    } catch (e) {
      error = String(e);
      vsFileDiff = null;
    } finally {
      vsFileLoading = false;
    }
  }

  const vsChangedFiles = $derived(vsDiff
    ? Array.from(new Set([...vsDiff.snapshotChangedFiles, ...vsDiff.snapshotGoneFiles, ...vsDiff.currentAddedFiles])).sort()
    : []);

  async function runPrune() {
    const dir = await ensureProjectDir();
    if (!dir || pruneDays < 1) return;
    const target = Math.max(1, Math.floor(pruneDays));
    const autoCountBefore = autoCount;
    showConfirm(
      "Clean up old auto-snapshots",
      `Delete automatic snapshots that are ${target} days old or older? Manual and crash-fix snapshots are kept. Up to ${autoCountBefore} auto snapshot(s) may qualify.`,
      async () => {
        pruneLoading = true;
        error = null;
        message = null;
        try {
          const res = await api.snapshots.pruneAuto(target, dir!);
          pruneResult = res;
          message = res.removedIds.length
            ? `Removed ${res.removedIds.length} old snapshot(s), freed ${formatBytes(res.totalBytes)}.`
            : "No automatic snapshots were old enough to remove.";
          await load(true);
          if (fromId && toId) {
            // Re-validate compare targets after deletion.
            const ids = new Set(snapshots.map((s) => s.id));
            if (!ids.has(fromId)) fromId = "";
            if (!ids.has(toId)) toId = "";
          }
        } catch (e) {
          error = String(e);
        } finally {
          pruneLoading = false;
        }
      },
      true,
    );
  }


  function lineClass(line: string) {
    if (line.startsWith("+ ")) return "added";
    if (line.startsWith("- ")) return "removed";
    return "context";
  }

  /** Friendly snapshot label for select dropdowns. */
  function snapshotSelectLabel(s: Snapshot) {
    return `${s.name} · ${formatRelative(s.createdAt)}`;
  }

  function changedCount(s: Snapshot) {
    return s.changedFiles?.length ?? 0;
  }

  const allDiffFiles = $derived(diff
    ? Array.from(new Set([...diff.addedFiles, ...diff.removedFiles, ...diff.modifiedFiles])).sort()
    : []);
  const detailKind = $derived(detail ? kindOf(detail.snapshot) : "manual");
  $effect(() => {
    if ($projectPath && lastLoadedPath !== $projectPath) load(true);
  });
  $effect(() => {
    const focusSnap = $historyFocusSnapshotId;
    if (!focusSnap || !snapshots.length) return;
    if (!snapshots.some((s) => s.id === focusSnap)) return;
    historyFocusSnapshotId.set(null);
    void selectSnapshot(focusSnap);
  });
</script>

<svelte:window onkeydown={focusSearch} />

<div class="snapshots flex flex-col gap-3.5 h-full min-h-0 w-full max-w-[1440px] mx-auto box-border bg-black/30 backdrop-blur-2xl rounded-2xl border border-white/[0.08] shadow-[inset_0_1px_0_rgba(255,255,255,0.1)] p-6">
  <!-- ── Toolbar ─────────────────────────────────────────────── -->
  <div class="flex justify-between items-center gap-4 flex-wrap shrink-0">
    <div class="grid gap-2">
      <div class="flex items-center gap-2.5 text-[var(--text-primary)] font-extrabold text-[16px]">
        <History size={19} class="text-[var(--accent-primary)]" />
        <span>Snapshots</span>
      </div>
      <p class="m-0 text-[var(--text-muted)] text-[13px]">Checkpoints of your pack — roll back to any saved state</p>
    </div>
    <div class="flex items-center gap-2.5 flex-wrap">
      <div class="quick-save" aria-label="Create a snapshot">
        <History size={17} class="quick-save-icon" />
        <input
          bind:value={newName}
          placeholder="Название точки сохранения..."
          onkeydown={(e) => e.key === "Enter" && !loading && ($projectPath ? create() : null)}
        />
        <button class="quick-save-action" onclick={create} disabled={!$projectPath || loading} title="Create a safety snapshot of the current state">
          <Plus size={16} /> Save point
        </button>
      </div>
      <button class="ghost w-[38px] h-[38px] p-0 shrink-0 justify-center" onclick={() => load(true)} title="Refresh" disabled={!$projectPath || loading}>
        <RefreshCw size={16} class={loading ? "spin" : ""} />
      </button>
    </div>
  </div>

  {#if error}<div class="px-3.5 py-3 rounded-[var(--border-radius-lg)] border text-[13px] leading-snug text-[#fecaca] bg-[rgba(239,68,68,0.08)] border-[rgba(239,68,68,0.28)]">{error}</div>{/if}
  {#if message}<div class="px-3.5 py-3 rounded-[var(--border-radius-lg)] border text-[13px] leading-snug text-[var(--accent-primary)] bg-[color-mix(in_srgb,var(--accent-primary)_8%,transparent)] border-[color-mix(in_srgb,var(--accent-primary)_25%,transparent)]">{message}</div>{/if}

  {#if loading && snapshots.length === 0}
    <div class="text-[var(--text-muted)] py-20 text-center text-[14px] bg-[var(--bg-secondary)] border border-[var(--border-color)] rounded-[var(--border-radius-lg)]">Loading snapshots…</div>
  {:else if !$projectPath}
    <EmptyState icon={History} title="No project selected" description="Open a project to manage snapshots." />
  {:else if snapshots.length === 0}
    <EmptyState icon={History} title="No snapshots yet" description="Create a snapshot to save the current state of your project." />
  {:else}
    <!-- ── Kind filter cards ───────────────────────────────────── -->
    <div class="grid grid-cols-4 max-[900px]:grid-cols-2 gap-2.5 shrink-0" role="group" aria-label="Filter snapshots">
      <button type="button" class="filter-card {filterKind === "all" ? "active" : ""}" onclick={() => (filterKind = "all")}>
        <Database size={16} />
        <span class="text-[19px] font-extrabold leading-none">{ stats.all }</span>
        <span>Все</span>
      </button>
      <button type="button" class="filter-card {filterKind === "auto" ? "active" : ""}" onclick={() => (filterKind = "auto")}>
        <Zap size={16} />
        <span class="text-[19px] font-extrabold leading-none">{ stats.auto }</span>
        <span>Автоматические</span>
      </button>
      <button type="button" class="filter-card {filterKind === "manual" ? "active" : ""}" onclick={() => (filterKind = "manual")}>
        <Hand size={16} />
        <span class="text-[19px] font-extrabold leading-none">{ stats.manual }</span>
        <span>Ручные</span>
      </button>
      <button type="button" class="filter-card {filterKind === "crash" ? "active" : ""}" onclick={() => (filterKind = "crash")}>
        <ShieldAlert size={16} />
        <span class="text-[19px] font-extrabold leading-none">{ stats.crash }</span>
        <span>Сбои</span>
      </button>
    </div>

    <!-- ── Search ──────────────────────────────────────────────── -->
    <div class="flex gap-3 items-center shrink-0">
      <div class="flex-1 min-w-[240px] flex items-center gap-2 bg-black/40 border border-white/10 rounded-[var(--border-radius-md)] px-2.5 text-[var(--text-muted)] focus-within:border-emerald-500/50 focus-within:ring-1 focus-within:ring-emerald-500/30">
        <Search size={15} />
        <input bind:this={searchInput} class="flex-1 border-0 bg-transparent text-[var(--text-primary)] py-2.5 outline-none min-w-0 text-[13px]" bind:value={search} placeholder="Search snapshots, actions, tags…" aria-label="Search snapshots" />
        <kbd>Ctrl F</kbd>
      </div>
      <span class="text-[var(--text-muted)] text-[13px] whitespace-nowrap tabular-nums">{ filtered.length } of { snapshots.length }</span>
    </div>

    <div class="summary">
      <div class="summary-stat"><strong>{snapshots.length}</strong><span>Checkpoints</span></div>
      <div class="summary-stat auto"><strong>{autoCount}</strong><span>Auto</span></div>
      <div class="summary-stat manual"><strong>{manualCount}</strong><span>Manual</span></div>
      <div class="summary-stat crash"><strong>{crashCount}</strong><span>Crash fixes</span></div>
      <div class="summary-stat size" title={formatBytes(totalBytes)}>
        <HardDrive size={14} />
        <strong>{formatBytes(totalBytes)}</strong>
        <span>on disk</span>
      </div>
    </div>

    <div class="collapsible">
      <button type="button" class="collapse-toggle" onclick={() => (cleanupOpen = !cleanupOpen)}>
        {#if cleanupOpen}<ChevronDown size={16} />{:else}<ChevronRight size={16} />{/if}
        <ShieldCheck size={16} /> Disk cleanup
      </button>
      {#if cleanupOpen}
        <div class="cleanup-panel">
          <p class="muted">
            Delete automatic (launcher / AI / scan) snapshots that are older than the selected age.
            Manual and crash-fix snapshots are always kept.
          </p>
          <div class="cleanup-controls">
            <label>
              Older than
              <select value={pruneDays} onchange={(e) => { pruneDays = parseInt((e.currentTarget as HTMLSelectElement).value, 10) || 30; }}>
                <option value={14}>14 days</option>
                <option value={30}>30 days</option>
                <option value={60}>60 days</option>
                <option value={90}>90 days</option>
              </select>
            </label>
            <button class="ghost" onclick={runPrune} disabled={pruneLoading || autoCount === 0}>
              {pruneLoading ? "Cleaning..." : "Clean up old auto-snapshots"}
            </button>
            <span class="muted">({autoCount} auto snapshot(s))</span>
          </div>
          {#if pruneResult && pruneResult.removedIds.length > 0}
            <div class="notice success">
              Removed {pruneResult.removedIds.length} old snapshot(s), freed {formatBytes(pruneResult.totalBytes)}.
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <!-- ── Master / detail ─────────────────────────────────────── -->
    <div class="master-detail">
      <aside class="list-pane">
        {#each timeline as group}
          <div class="timeline-group">
            <div class="timeline-header">
              <span class="timeline-dot"></span>
              <span class="timeline-label">{group.label}</span>
              <span class="timeline-count">{group.items.length}</span>
            </div>
            {#each group.items as s (s.id)}
              <button
                type="button"
                class="row"
                class:selected={selectedId === s.id}
                onclick={() => selectSnapshot(s.id)}
              >
                <div class="row-top">
                  <strong>{s.name}</strong>
                  <span class="op-badge">{operationLabel(s)}</span>
                </div>
                <p class="preview">{previewLine(s)}</p>
                <div class="row-meta">
                  <span><Calendar size={12} /> {formatDate(s.createdAt)}</span>
                  <span class="kind-badge" class:auto={kindOf(s) === "auto"} class:manual={kindOf(s) === "manual"} class:crash={kindOf(s) === "crash"}>{kindOf(s)}</span>
                  {#if s.sizeBytes}
                    <span class="size-badge"><HardDrive size={11} /> {formatBytes(s.sizeBytes)}</span>
                  {/if}
                  {#if s.tags?.length}
                    <span class="tags">
                      {#each s.tags as t}<span class="tag" class:crash-fix={t === "crash_fix"}>{t}</span>{/each}
                    </span>
                  {/if}
                </div>
              </button>
            {/each}
          </div>
        {:else}
          <div class="text-[var(--text-muted)] text-[13px] p-6">No snapshots match filters.</div>
        {/each}
      </aside>

      <section class="p-[18px] overflow-auto min-h-0 flex flex-col gap-3.5 bg-white/[0.02] border border-white/[0.08] backdrop-blur-2xl rounded-[var(--border-radius-lg)] [scrollbar-gutter:stable] shadow-xl">
        {#if detailLoading}
          <div class="flex flex-col gap-3">
            <span class="skeleton" style="width: 44%; height: 26px"></span>
            <span class="skeleton" style="width: 70%"></span>
            <span class="skeleton" style="width: 90%"></span>
            <span class="skeleton" style="height: 120px"></span>
          </div>
        {:else if detail}
          {@const s = detail.snapshot}
          <div class="flex justify-between gap-3.5 flex-wrap items-start">
            <div class="min-w-0">
              <div class="flex items-center gap-2.5 flex-wrap">
                <h2 class="m-0 text-[20px] leading-tight [overflow-wrap:anywhere] text-[var(--text-primary)]">{ friendlySnapshotTitle(s) }</h2>
                <span class="kind-pill { detailKind }">{ kindLabel(detailKind) }</span>
              </div>
              <div class="flex items-center gap-2 flex-wrap mt-1.5">
                <span class="text-[12px] text-[var(--text-muted)] bg-[var(--bg-elevated)] px-2 py-1 rounded font-mono" title={ s.id }>{ shortId(s.id) }</span>
                <span class="text-[13px] text-[var(--text-secondary)] inline-flex items-center gap-2"><Clock size={13} /> { formatRelative(s.createdAt) }</span>
                {#if s.actor}<span class="actor-pill">{ s.actor }</span>{/if}
                {#if s.planSource}<span class="actor-pill plan">{ s.planSource }</span>{/if}
              </div>
            </div>
            <div class="detail-actions">
              <button class="secondary" onclick={() => loadVsCurrent(s.id)} title="What changed in this project since this checkpoint?">
                <GitCompare size={14} /> Diff vs current
              </button>
              <button class="secondary" onclick={() => compareWithPrevious(s.id)} title="Compare with previous">
                <GitCompare size={14} /> Compare prev
              </button>
              <button class="secondary" onclick={() => openInHistory(s.id)} title="Open in History">
                <ExternalLink size={14} /> History
              </button>
              <button class="ghost rollback" onclick={() => rollback(s.id)}>
                <RotateCcw size={14} /> Rollback
              </button>
              <button class="ghost danger" onclick={() => removeSnapshot(s.id)}>
                <Trash2 size={14} /> Delete
              </button>
            </div>
          </div>

          {#if s.tags?.length || s.crashFingerprintKey || s.matchedCaseIds?.length}
            <div class="flex flex-wrap gap-2">
              {#each s.tags ?? [] as t}
                <span class="tag" class:crash-fix={ t === "crash_fix" }>{ t }</span>
              {/each}
              {#if s.crashFingerprintKey}
                <span class="tag font-mono max-w-[200px] overflow-hidden text-ellipsis whitespace-nowrap" title={ s.crashFingerprintKey }>{ s.crashFingerprintKey.slice(0, 28) }…</span>
              {/if}
              {#each s.matchedCaseIds ?? [] as cid}
                <span class="tag font-mono">{ cid }</span>
              {/each}
            </div>
          {/if}

          <p class="text-[14px] text-[var(--text-secondary)] m-0 leading-relaxed">{ s.reason }</p>

          {#if detail.manifestOnly}
            <div class="px-3.5 py-3 rounded-[var(--border-radius-lg)] border text-[13px] leading-snug inline-flex items-center gap-2 text-[#fcd34d] bg-[rgba(245,158,11,0.08)] border-[rgba(245,158,11,0.28)]">
              <AlertTriangle size={15} />
              <span><strong>Manifest-only checkpoint.</strong> File copies were skipped because the project uses deduplication and hardlinks. Rollback restores the manifest, but not mod jars from this snapshot.</span>
            </div>
          {/if}

          {#if detail.humanExplanation}
            <div class="grid gap-2.5">
              <h3 class="flex items-center gap-2 m-0 text-[12.5px] uppercase tracking-wider text-[var(--text-muted)] font-bold"><Sparkles size={14} class="text-[var(--accent-primary)]" /> Explanation</h3>
              <p class="m-0 text-[14px] text-[var(--text-secondary)] leading-relaxed">{ detail.humanExplanation }</p>
            </div>
          {/if}

          <div class="grid gap-2.5">
            <h3 class="flex items-center gap-2 m-0 text-[12.5px] uppercase tracking-wider text-[var(--text-muted)] font-bold"><Zap size={14} class="text-[var(--accent-primary)]" /> Actions ({ (detail.actionsSummary ?? []).length })</h3>
            {#if (detail.actionsSummary ?? []).length > 0}
              <ul class="m-0 p-0 list-none grid gap-2">
                {#each detail.actionsSummary ?? [] as line}
                  <li class="flex items-start gap-2.5 text-[13.5px] leading-snug text-[var(--text-secondary)] px-2.5 py-2 bg-[var(--bg-tertiary)] border border-[var(--border-color)] rounded-[var(--border-radius-sm)]">
                    <span class="w-1.5 h-1.5 rounded-full bg-[var(--accent-primary)] shrink-0 mt-[7px]" aria-hidden="true"></span>{ line }
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="text-[13px] text-[var(--text-muted)] m-0">No action details recorded.</p>
            {/if}
          </div>

          <div class="grid gap-2">
            <h3 class="flex items-center gap-2 m-0 text-[12.5px] uppercase tracking-wider text-[var(--text-muted)] font-bold"><FolderOpen size={14} class="text-[var(--accent-primary)]" /> Changed files ({ (detail.changedFiles ?? []).length })</h3>
            {#if (detail.changedFiles ?? []).length > 0}
              <ul class="m-0 p-0 list-none grid gap-2">
                {#each detail.changedFiles ?? [] as f}
                  <li class="flex items-center gap-2.5 px-2.5 py-1.5 rounded-[var(--border-radius-sm)] bg-[var(--bg-tertiary)] border border-transparent hover:border-[var(--border-color)] min-w-0">
                    <span class="shrink-0 text-[12px] font-extrabold uppercase tracking-wide px-1.5 py-0.5 rounded bg-[var(--bg-elevated)] text-[var(--text-muted)]">{ f.category }</span>
                    <span class="text-[var(--text-secondary)] font-mono text-[12.5px] flex-1 min-w-0 tb-truncate">{ f.path }</span>
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="text-[13px] text-[var(--text-muted)] m-0">No tracked files copied into this snapshot.</p>
            {/if}
          </div>

          {#if detail.relatedEvents.length}
            <div class="grid gap-2.5">
              <h3 class="flex items-center gap-2 m-0 text-[12.5px] uppercase tracking-wider text-[var(--text-muted)] font-bold"><History size={14} class="text-[var(--accent-primary)]" /> Related activity ({ detail.relatedEvents.length })</h3>
              <ul class="m-0 p-0 list-none grid gap-2">
                {#each detail.relatedEvents as ev}
                  <li class="flex items-center gap-2.5 px-2.5 py-2 bg-[var(--bg-tertiary)] border border-[var(--border-color)] rounded-[var(--border-radius-sm)] min-w-0">
                    <span class="actor-pill">{ ev.actor }</span>
                    <span class="text-[13.5px] text-[var(--text-secondary)] min-w-0 [overflow-wrap:anywhere]">{ ev.summary }</span>
                  </li>
                {/each}
              </ul>
            </div>
          {/if}

          {#if vsDiff}
            <div class="block vs-panel">
              <h3>What changed since this checkpoint?</h3>
              {#if vsDiff.manifestCompared && vsDiff.manifestDiff}
                <h4>Manifest</h4>
                <pre class="manifest-diff-text">
{#each vsDiff.manifestDiff.split("\n") as line}
<span class={lineClass(line)}>{line}</span>
{/each}
                </pre>
              {:else if vsDiff.manifestCompared}
                <p class="muted">Manifest on disk matches this snapshot.</p>
              {/if}
              {#if vsChangedFiles.length > 0}
                <h4>Tracked files</h4>
                <div class="inline-diff-shell">
                  <aside class="diff-files">
                    {#each vsChangedFiles as path}
                      <button class:selected={vsSelectedPath === path} onclick={() => openVsFile(path)}>
                        <span>{path}</span>
                        {#if vsDiff.snapshotGoneFiles.includes(path)}<small class="removed-label">gone</small>{/if}
                        {#if vsDiff.currentAddedFiles.includes(path)}<small class="added-label">new</small>{/if}
                        {#if vsDiff.snapshotChangedFiles.includes(path)}<small>changed</small>{/if}
                      </button>
                    {/each}
                  </aside>
                  <section class="inline-diff">
                    {#if vsFileLoading}
                      <div class="muted">Loading file diff...</div>
                    {:else if vsFileDiff}
                      <div class="inline-diff-header">
                        <strong>{vsFileDiff.path}</strong>
                        <span>{vsFileDiff.fromExists ? "snapshot" : "snapshot missing"} → {vsFileDiff.toExists ? "on disk" : "missing on disk"}</span>
                      </div>
                      <pre>
{#each vsFileDiff.text.split("\n") as line}
<span class={lineClass(line)}>{line}</span>
{/each}
                      </pre>
                    {:else}
                      <div class="muted">Select a file above to view the inline diff against the current state.</div>
                    {/if}
                  </section>
                </div>
              {:else}
                <p class="muted">No tracked files recorded for this snapshot.</p>
              {/if}
            </div>
          {/if}
        {:else}
          <EmptyState icon={History} title="Select a snapshot" description="Pick a checkpoint on the left to see actions, files, and rollback options." />
        {/if}
      </section>
    </div>

    <!-- ── Compare snapshots ───────────────────────────────────── -->
    <div class="overflow-hidden flex flex-col shrink-0 bg-[var(--bg-secondary)] border border-[var(--border-color)] rounded-[var(--border-radius-lg)]">
      <button type="button" class="w-full flex items-center gap-2.5 bg-transparent border-0 text-[var(--text-secondary)] font-bold px-3.5 py-3 text-[13.5px] cursor-pointer rounded-none transition-colors duration-150 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]" onclick={() => (compareOpen = !compareOpen)}>
        <span class="inline-flex text-[var(--text-muted)]">{#if compareOpen}<ChevronDown size={17} />{:else}<ChevronRight size={17} />{/if}</span>
        <GitCompare size={17} /> Compare snapshots
        <span class="flex-1"></span>
        {#if diff}
          <span class="text-[12px] font-bold text-[var(--text-muted)] bg-[var(--bg-elevated)] px-2 py-0.5 rounded-full">{ allDiffFiles.length } file{ allDiffFiles.length === 1 ? "" : "s" }</span>
        {/if}
      </button>
      {#if compareOpen}
        <div class="grid gap-3.5 px-3.5 pb-3.5">
          <div class="flex items-center gap-2.5 flex-wrap">
            <div class="grid gap-2 flex-1 min-w-[200px]">
              <label for="snap-from" class="text-[12px] uppercase tracking-wider text-[var(--text-muted)] font-extrabold">From</label>
              <select id="snap-from" class="w-full min-w-0" bind:value={fromId}>
                {#each snapshots as s}<option value={s.id}>{ snapshotSelectLabel(s) }</option>{/each}
              </select>
            </div>
            <button class="ghost w-[38px] h-[38px] p-0 justify-center shrink-0 border border-[var(--border-color)] bg-[var(--bg-elevated)] text-[var(--text-muted)] hover:text-[var(--accent-primary)] hover:border-[color-mix(in_srgb,var(--accent-primary)_40%,transparent)]" onclick={swapCompare} title="Swap direction" disabled={!fromId || !toId}>
              <ArrowRightLeft size={16} />
            </button>
            <div class="grid gap-2 flex-1 min-w-[200px]">
              <label for="snap-to" class="text-[12px] uppercase tracking-wider text-[var(--text-muted)] font-extrabold">To</label>
              <select id="snap-to" class="w-full min-w-0" bind:value={toId}>
                {#each snapshots as s}<option value={s.id}>{ snapshotSelectLabel(s) }</option>{/each}
              </select>
            </div>
            <div class="flex gap-2">
              <button class="secondary" onclick={compare} disabled={fromId === toId || !fromId || !toId}>
                Diff files
              </button>
              <button class="secondary" onclick={loadManifestDiff} disabled={fromId === toId || !fromId || !toId || manifestDiffLoading}>
                { manifestDiffLoading ? "Loading…" : "Diff manifest" }
              </button>
            </div>
          </div>

          <!-- ── Pack Diff: cross-source compare ─────────────────── -->
          <div class="grid gap-2.5 p-3.5 bg-[var(--bg-secondary)] border border-[var(--border-color)] rounded-[var(--border-radius-lg)]">
            <h3 class="m-0 text-[14px] text-[var(--text-secondary)] font-bold">Compare packs</h3>
            <div class="grid gap-2" style="grid-template-columns: repeat(2, minmax(180px, 1fr));">
              <div class="grid gap-2">
                <label for="pd-from-kind" class="text-[12px] uppercase tracking-wider text-[var(--text-muted)] font-extrabold">From source</label>
                <select id="pd-from-kind" bind:value={fromKind}>
                  <option value="snapshot">Snapshot</option>
                  <option value="backup">Zip backup</option>
                  <option value="manifest">Other instance manifest</option>
                </select>
              </div>
              <div class="grid gap-2">
                <label for="pd-to-kind" class="text-[12px] uppercase tracking-wider text-[var(--text-muted)] font-extrabold">To source</label>
                <select id="pd-to-kind" bind:value={toKind}>
                  <option value="snapshot">Snapshot</option>
                  <option value="backup">Zip backup</option>
                  <option value="manifest">Other instance</option>
                </select>
              </div>
              {#if fromKind === "snapshot" || toKind === "snapshot"}
                <p class="m-0 col-span-2 text-[12px] text-[var(--text-muted)]">Snapshot side uses the From/To snapshot selects above.</p>
              {/if}
              {#if fromKind === "backup" || toKind === "backup"}
                <label class="grid gap-2">
                  <span class="text-[12px] uppercase tracking-wider text-[var(--text-muted)] font-extrabold">Backup ids</span>
                  <span class="flex gap-2">
                    <input class="flex-1 min-w-0" bind:value={backupFromId} placeholder="From backup id" />
                    <input class="flex-1 min-w-0" bind:value={backupToId} placeholder="To backup id" />
                  </span>
                </label>
              {/if}
              {#if fromKind === "manifest" || toKind === "manifest"}
                <label class="grid gap-2 col-span-2">
                  <span class="text-[12px] uppercase tracking-wider text-[var(--text-muted)] font-extrabold">Other instance manifest path</span>
                  <input bind:value={otherManifestPath} placeholder="U:/…/project.tuffbox.json" />
                </label>
              {/if}
            </div>
            <button class="secondary" onclick={runPackDiff} disabled={packDiffLoading}>
              { packDiffLoading ? "Comparing…" : "Compare packs" }
            </button>

            {#if packDiff}
              {@const r = packDiff.report}
              <div class="grid gap-2 mb-1.5">
                {#if r.mcA !== r.mcB}
                  <div class="flex justify-between gap-2.5 px-2.5 py-2 rounded-[var(--border-radius-sm)] text-[13px] bg-[var(--bg-tertiary)] border border-[rgba(245,158,11,0.3)]">
                    <strong class="text-[var(--text-primary)]">MC version</strong><span class="text-[var(--text-muted)] tb-truncate">{ r.mcA || "—" } → { r.mcB || "—" }</span>
                  </div>
                {/if}
                <div class="flex justify-between gap-2.5 px-2.5 py-2 rounded-[var(--border-radius-sm)] text-[13px] bg-[var(--bg-tertiary)] border border-[color-mix(in_srgb,var(--accent-primary)_30%,transparent)]">
                  <strong class="text-[var(--accent-primary)]">+{ r.addedMods.length } mods</strong><span class="text-[var(--text-muted)] tb-truncate">{ r.addedMods.map((m) => m.id).join(", ") }</span>
                </div>
                <div class="flex justify-between gap-2.5 px-2.5 py-2 rounded-[var(--border-radius-sm)] text-[13px] bg-[var(--bg-tertiary)] border border-[rgba(239,68,68,0.3)]">
                  <strong class="text-[#fca5a5]">-{ r.removedMods.length } mods</strong><span class="text-[var(--text-muted)] tb-truncate">{ r.removedMods.map((m) => m.id).join(", ") }</span>
                </div>
                <div class="flex justify-between gap-2.5 px-2.5 py-2 rounded-[var(--border-radius-sm)] text-[13px] bg-[var(--bg-tertiary)] border border-[rgba(147,197,253,0.35)]">
                  <strong class="text-[#93c5fd]">~{ r.updatedMods.length } updated</strong><span class="text-[var(--text-muted)] tb-truncate">{ r.updatedMods.map((u) => `${u.id} ${u.from.version}→${u.to.version}`).join(", ") }</span>
                </div>
              </div>
              {#if packDiff.configDiffs.length}
                <div class="grid gap-2">
                  {#each packDiff.configDiffs as cd (cd.path)}
                    <details>
                      <summary class="cursor-pointer text-[12.5px] text-[var(--text-secondary)] font-mono">{ cd.path }</summary>
                      <pre class="m-1.5 p-3 rounded-[var(--border-radius-sm)] bg-[#0d0d10] text-[#b4b4bc] font-mono text-[12.5px] leading-relaxed max-h-[360px] overflow-auto whitespace-pre-wrap">{ cd.diffText }</pre>
                    </details>
                  {/each}
                </div>
              {:else}
                <p class="m-0 text-[12.5px] text-[var(--text-muted)]">No config file changes.</p>
              {/if}
            {/if}
          </div>
          {#if manifestDiff}
            <div class="p-3.5 bg-[var(--bg-secondary)] border border-[var(--border-color)] rounded-[var(--border-radius-lg)] grid gap-2.5">
              <h3 class="m-0 text-[14px] text-[var(--text-secondary)] font-bold">Manifest changes</h3>
              <div class="grid gap-2 mb-1.5">
                {#if manifestDiff.mcVersionChanged}
                  <div class="flex justify-between gap-2.5 px-2.5 py-2 rounded-[var(--border-radius-sm)] text-[13px] bg-[var(--bg-tertiary)] border border-[rgba(245,158,11,0.3)]">
                    <strong class="text-[var(--text-primary)]">MC version</strong><span class="text-[var(--text-muted)] tb-truncate">{ manifestDiff.fromMcVersion } → { manifestDiff.toMcVersion }</span>
                  </div>
                {/if}
                {#if manifestDiff.loaderVersionChanged}
                  <div class="flex justify-between gap-2.5 px-2.5 py-2 rounded-[var(--border-radius-sm)] text-[13px] bg-[var(--bg-tertiary)] border border-[rgba(245,158,11,0.3)]">
                    <strong class="text-[var(--text-primary)]">Loader</strong><span class="text-[var(--text-muted)] tb-truncate">{ manifestDiff.fromLoaderVersion } → { manifestDiff.toLoaderVersion }</span>
                  </div>
                {/if}
                {#if manifestDiff.addedMods?.length}
                  <div class="flex justify-between gap-2.5 px-2.5 py-2 rounded-[var(--border-radius-sm)] text-[13px] bg-[var(--bg-tertiary)] border border-[color-mix(in_srgb,var(--accent-primary)_30%,transparent)]">
                    <strong class="text-[var(--accent-primary)]">+{ manifestDiff.addedMods.length } mods</strong><span class="text-[var(--text-muted)] tb-truncate">{ manifestDiff.addedMods.join(", ") }</span>
                  </div>
                {/if}
                {#if manifestDiff.removedMods?.length}
                  <div class="flex justify-between gap-2.5 px-2.5 py-2 rounded-[var(--border-radius-sm)] text-[13px] bg-[var(--bg-tertiary)] border border-[rgba(239,68,68,0.3)]">
                    <strong class="text-[#fca5a5]">-{ manifestDiff.removedMods.length } mods</strong><span class="text-[var(--text-muted)] tb-truncate">{ manifestDiff.removedMods.join(", ") }</span>
                  </div>
                {/if}
              </div>
              <pre class="m-0 p-3 rounded-[var(--border-radius-sm)] bg-[#0d0d10] text-[#b4b4bc] font-mono text-[12.5px] leading-relaxed max-h-[360px] overflow-auto whitespace-pre-wrap">{ manifestDiff.diffText || "No differences." }</pre>
            </div>
          {/if}
          {#if diff}
            <div class="grid grid-cols-3 gap-2.5">
              <div class="bg-[var(--bg-tertiary)] border border-[color-mix(in_srgb,var(--accent-primary)_35%,transparent)] rounded-[var(--border-radius-md)] px-3.5 py-3 flex flex-col gap-0.5">
                <strong class="text-[26px] leading-tight text-[var(--accent-primary)]">{ diff.addedFiles.length }</strong>
                <span class="text-[var(--text-muted)] text-[13px]">Added</span>
              </div>
              <div class="bg-[var(--bg-tertiary)] border border-[rgba(239,68,68,0.35)] rounded-[var(--border-radius-md)] px-3.5 py-3 flex flex-col gap-0.5">
                <strong class="text-[26px] leading-tight text-[#fca5a5]">{ diff.removedFiles.length }</strong>
                <span class="text-[var(--text-muted)] text-[13px]">Removed</span>
              </div>
              <div class="bg-[var(--bg-tertiary)] border border-[rgba(147,197,253,0.35)] rounded-[var(--border-radius-md)] px-3.5 py-3 flex flex-col gap-0.5">
                <strong class="text-[26px] leading-tight text-[#93c5fd]">{ diff.modifiedFiles.length }</strong>
                <span class="text-[var(--text-muted)] text-[13px]">Modified</span>
              </div>
            </div>
            {#if allDiffFiles.length > 0}
              <div class="grid grid-cols-[320px_minmax(0,1fr)] max-[900px]:grid-cols-1 gap-3.5 p-3.5 bg-[var(--bg-tertiary)] border border-[var(--border-color)] rounded-[var(--border-radius-lg)]">
                <aside class="min-w-0">
                  <h3 class="text-[var(--text-muted)] text-[12.5px] uppercase tracking-wider mb-2.5 flex items-center gap-2 font-bold"><FileText size={14} /> Changed files</h3>
                  {#each allDiffFiles as path}
                    <button class="w-full flex items-center justify-between gap-2 text-left bg-transparent text-[var(--text-secondary)] border rounded-[var(--border-radius-sm)] px-2.5 py-2 mb-1.5 text-[12.5px] cursor-pointer transition-colors duration-150 {
                      selectedDiffPath === path
                        ? "bg-[var(--bg-elevated)] border-[color-mix(in_srgb,var(--accent-primary)_30%,transparent)] text-[var(--text-primary)]"
                        : "border-transparent hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)]"
                    }" onclick={() => openFileDiff(path)}>
                      <span class="tb-truncate">{ path }</span>
                      <span class="inline-flex gap-2 shrink-0">
                        {#if diff.addedFiles.includes(path)}<small class="diff-label added">added</small>{/if}
                        {#if diff.removedFiles.includes(path)}<small class="diff-label removed">removed</small>{/if}
                        {#if diff.modifiedFiles.includes(path)}<small class="diff-label modified">modified</small>{/if}
                      </span>
                    </button>
                  {/each}
                </aside>
                <section class="min-w-0 flex flex-col gap-2">
                  {#if diffLoading}
                    <div class="text-[var(--text-muted)] text-[13px]">Loading file diff…</div>
                  {:else if fileDiff}
                    <div class="flex justify-between items-center gap-3 text-[var(--text-secondary)]">
                      <strong class="text-[14px] tb-truncate">{ fileDiff.path }</strong>
                      <span class="text-[var(--text-muted)] text-[12.5px] shrink-0">{ fileDiff.fromExists ? "from exists" : "from missing" } → { fileDiff.toExists ? "to exists" : "to missing" }</span>
                    </div>
                    {#if fileDiff.text}
                      <pre class="overflow-auto max-h-[420px] bg-[#0d0d10] rounded-[var(--border-radius-md)] p-3 m-0 text-[12.5px] leading-relaxed">
{#each fileDiff.text.split("\n") as line}
<span class="block whitespace-pre-wrap font-mono { lineClass(line) }">{ line }</span>
{/each}
                      </pre>
                    {:else}
                      <div class="text-[var(--text-muted)] text-[13px] p-6">File looks identical — content unchanged.</div>
                    {/if}
                  {:else}
                    <div class="text-[var(--text-muted)] text-[13px]">Select a file to view inline diff.</div>
                  {/if}
                </section>
              </div>
            {/if}
          {/if}
        </div>
      {/if}
    </div>

    <!-- ── Project backups ─────────────────────────────────────── -->
    <div class="overflow-hidden flex flex-col shrink-0 bg-[var(--bg-secondary)] border border-[var(--border-color)] rounded-[var(--border-radius-lg)]">
      <button type="button" class="w-full flex items-center gap-2.5 bg-transparent border-0 text-[var(--text-secondary)] font-bold px-3.5 py-3 text-[13.5px] cursor-pointer transition-colors duration-150 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]" onclick={() => (backupsOpen = !backupsOpen)}>
        <span class="inline-flex text-[var(--text-muted)]">{#if backupsOpen}<ChevronDown size={17} />{:else}<ChevronRight size={17} />{/if}</span>
        <Archive size={17} /> Project backups
        <span class="flex-1"></span>
        <span class="text-[12px] font-bold text-[var(--text-muted)] bg-[var(--bg-elevated)] px-2 py-0.5 rounded-full">{ backups.length } saved</span>
      </button>
      {#if backupsOpen}
        <div class="px-3.5 pb-3.5 grid gap-3">
          <div class="flex items-center gap-2 flex-wrap">
            <input class="flex-1 min-w-[200px]" bind:value={backupName} placeholder="Backup name" onkeydown={(e) => e.key === "Enter" && !loading && ($projectPath ? createBackup() : null)} />
            <button class="secondary" onclick={createBackup} disabled={!$projectPath || loading}>
              <Archive size={16} /> Create zip
            </button>
            <button class="ghost w-[38px] h-[38px] p-0 justify-center shrink-0" onclick={loadBackups} disabled={backupLoading} title="Refresh backups">
              <RefreshCw size={15} class={backupLoading ? "spin" : ""} />
            </button>
            <p class="m-0 w-full text-[13px] text-[var(--text-muted)]">Full zip of tracked files — restore to bring back an older pack state.</p>
          </div>
          {#if backups.length > 0}
            <div class="grid gap-2">
              {#each backups.slice(0, 12) as b}
                <div class="flex items-center gap-3 px-3 py-2.5 rounded-[var(--border-radius-md)] bg-[var(--bg-tertiary)] border border-[var(--border-color)] min-w-0 hover:border-[color-mix(in_srgb,var(--accent-primary)_25%,var(--border-color))]">
                  <div class="inline-flex items-center justify-center w-8 h-8 rounded-[var(--border-radius-sm)] bg-[var(--bg-elevated)] border border-[var(--border-color)] text-[var(--text-muted)] shrink-0"><Archive size={16} /></div>
                  <div class="grid gap-0.5 flex-1 min-w-0">
                    <strong class="text-[var(--text-primary)] text-[13.5px] tb-truncate">{ b.name }</strong>
                    <span class="text-[var(--text-muted)] text-[12.5px]">{ formatDate(b.createdAt) }{#if b.fileCount} · { b.fileCount } files{/if}</span>
                  </div>
                  <span class="text-[var(--text-muted)] text-[12.5px] font-mono shrink-0">{ formatBytes(b.sizeBytes) }</span>
                  <button class="ghost mini" onclick={() => restoreBackup(b.id)} title="Restore">
                    <RotateCcw size={15} />
                  </button>
                  <button class="ghost mini danger" onclick={() => deleteBackup(b.id)} title="Delete">
                    <Trash2 size={15} />
                  </button>
                </div>
              {/each}
            </div>
          {:else}
            <p class="text-[13px] text-[var(--text-muted)] m-0">No zip backups yet.</p>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if confirmOpen}
    <ConfirmDialog
      title={confirmTitle}
      message={confirmMessage}
      danger={confirmDanger}
      onconfirm={handleConfirm}
      oncancel={() => ((confirmOpen = false), (confirmAction = null))}
    />
  {/if}
</div>

<style>
  .quick-save {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 42px;
    padding: 4px 5px 4px 12px;
    border: 1px solid rgba(52, 211, 153, .35);
    border-radius: var(--border-radius-md);
    background: linear-gradient(135deg, rgba(16,185,129,.14), rgba(16,185,129,.035));
    box-shadow: 0 0 22px rgba(16,185,129,.08), inset 0 1px 0 rgba(255,255,255,.08);
  }
  .quick-save-icon { color: #34d399; flex: 0 0 auto; }
  .quick-save input { min-width: 190px; width: 220px; padding: 8px 4px; border: 0; outline: 0; background: transparent; color: var(--text-primary); }
  .quick-save input::placeholder { color: color-mix(in srgb, var(--text-muted) 85%, transparent); }
  :global(.snapshots) .quick-save-action { min-height: 34px; padding: 7px 12px; border: 0; border-radius: var(--border-radius-sm); background: #10b981; color: #04130e; font-weight: 800; }
  :global(.snapshots) .quick-save-action:hover:not(:disabled) { background: #34d399; box-shadow: 0 0 14px rgba(52,211,153,.3); }
  .quick-save-action:disabled { opacity: .5; }
  .search-kbd, kbd { padding: 2px 6px; border: 1px solid var(--border-color); border-radius: 5px; color: var(--text-secondary); font: 12px ui-monospace, SFMono-Regular, monospace; white-space: nowrap; }

  .toolbar { display: flex; justify-content: space-between; align-items: center; gap: 16px; flex-wrap: wrap; flex-shrink: 0; }
  .title, .actions, .row-meta, .detail-sub, .detail-actions, .backup-create, .search, .collapse-toggle {
    display: flex; align-items: center; gap: 10px;
  }
  .title { color: var(--text-secondary); font-weight: 600; }
  .notice { padding: 12px 14px; border-radius: var(--border-radius-lg); border: 1px solid var(--border-color); display: flex; align-items: flex-start; gap: 8px; }
  .notice.error { color: var(--accent-danger); background: color-mix(in srgb, var(--accent-danger) 8%, transparent); border-color: color-mix(in srgb, var(--accent-danger) 28%, transparent); }
  .notice.success { color: var(--accent-primary); background: color-mix(in srgb, var(--accent-primary) 8%, transparent); border-color: color-mix(in srgb, var(--accent-primary) 25%, transparent); }
  .notice.warn { color: var(--accent-warning); background: color-mix(in srgb, var(--accent-warning) 8%, transparent); border-color: color-mix(in srgb, var(--accent-warning) 28%, transparent); font-size: 13px; }

  /* Layout comes from Tailwind utilities. Scoped styles only for
     pieces Tailwind can't express: shared app button skins used here
     (ghost/secondary/danger/mini are defined app-wide), the Ore-style
     kind tags, and skeletons. */

  /* Snapshots uses theme-token button skins, not the global Ore gray skin:
     secondary = tinted theme surface, ghost = quiet bordered pill. All
     colors come from CSS vars so every theme (light/sharp/minimal) reads
     correctly. Scoped to the component root class (not `section`) — the
     toolbar, Compare block and backups block are <div>s, and those buttons
     were falling back to the gray global skin. */
  :global(.snapshots) button.secondary {
    background: var(--bg-elevated);
    color: var(--text-primary);
    border: 1px solid var(--border-color);
    border-bottom-width: 2px;
    border-bottom-color: color-mix(in srgb, var(--border-color) 60%, transparent);
  }
  :global(.snapshots) button.secondary:hover:not(:disabled) {
    background: var(--bg-hover);
    border-color: color-mix(in srgb, var(--accent-primary) 30%, var(--border-color));
  }
  :global(.snapshots) button.secondary:active:not(:disabled) {
    filter: none;
    background: var(--bg-active);
  }
  :global(.snapshots) button.ghost:not(.mini) {
    background: var(--bg-secondary);
    color: var(--text-secondary);
    border: 1px solid var(--border-color);
    border-bottom-width: 2px;
    border-bottom-color: color-mix(in srgb, var(--border-color) 55%, transparent);
  }
  :global(.snapshots) button.ghost:not(.mini):hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 30%, var(--border-color));
  }
  :global(.snapshots) button.ghost:not(.mini):active:not(:disabled) {
    filter: none;
    background: var(--bg-active);
  }
  :global(.snapshots) button.ghost:disabled {
    opacity: 0.55;
  }
  .list-pane { overflow: auto; min-height: 0; max-height: none; padding: 10px; display: flex; flex-direction: column; gap: 16px; scrollbar-gutter: stable; }
  /* The grid needs an explicit single column: an auto-sized column plus the
     global `button { justify-content: center }` shrank the row content to its
     max-content width and centered it inside the full-width button. */
  .row { width: 100%; text-align: left; justify-content: start; grid-template-columns: minmax(0, 1fr); background: transparent; border: 1px solid transparent; border-radius: var(--border-radius-md); padding: 12px; color: var(--text-secondary); display: grid; gap: 8px; transform: none; }
  .row:hover, .row.selected { background: var(--bg-tertiary); border-color: color-mix(in srgb, var(--accent-primary) 28%, transparent); color: var(--text-primary); }
  .timeline-group { display: grid; gap: 8px; }
  .timeline-header { display: flex; align-items: center; gap: 8px; padding: 6px 6px 2px; position: sticky; top: 0; z-index: 2; background: var(--bg-secondary); }
  .timeline-dot { width: 8px; height: 8px; border-radius: 50%; background: color-mix(in srgb, var(--accent-primary) 55%, transparent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent-primary) 14%, transparent); flex-shrink: 0; }
  .timeline-label { font-size: 12px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-secondary); }
  .timeline-count {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
    font-weight: 700;
    color: var(--text-muted);
    background: var(--bg-elevated);
    border-radius: 999px;
    padding: 1px 7px;
  }
  .row-top strong { font-size: 13px; color: var(--text-primary); }
  .op-badge {
    display: inline-block;
    line-height: 1.4;
    vertical-align: baseline;
    font-size: 12px;
    padding: 2px 6px;
    border-radius: 4px;
    background: var(--bg-elevated);
    color: var(--text-muted);
    font-family: var(--font-mono, ui-monospace, monospace);
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .preview { margin: 0; font-size: 12px; color: var(--text-muted); line-height: 1.4; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .row-meta { font-size: 12px; color: var(--text-muted); flex-wrap: wrap; }
  .tags { display: flex; gap: 8px; flex-wrap: wrap; }
  .kind-badge { font-size: 12px; font-weight: 700; text-transform: uppercase; padding: 1px 7px; border-radius: 999px; letter-spacing: 0.04em; }
  .kind-badge.auto { color: var(--accent-primary); background: color-mix(in srgb, var(--accent-primary) 14%, transparent); }
  .kind-badge.manual { color: var(--text-secondary); background: var(--bg-elevated); }
  .kind-badge.crash { color: var(--accent-danger); background: color-mix(in srgb, var(--accent-danger) 14%, transparent); }
  .size-badge { display: inline-flex; align-items: center; gap: 8px; color: var(--text-muted); }

  .summary { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 10px; flex-shrink: 0; }
  .summary-stat { background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: var(--border-radius-lg); padding: 12px 14px; display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .summary-stat strong { font-size: 20px; color: var(--text-primary); line-height: 1; }
  .summary-stat span { font-size: 12px; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.05em; }
  .summary-stat.size { flex-direction: row; align-items: center; gap: 8px; }
  .summary-stat.size strong { font-size: 15px; }
  @media (max-width: 760px) { .summary { grid-template-columns: repeat(3, 1fr); } }

  .cleanup-panel { padding: 0 14px 14px; display: grid; gap: 10px; border: 0; background: transparent; }
  .cleanup-controls { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; }
  .cleanup-controls label { display: inline-flex; align-items: center; gap: 8px; font-size: 13px; color: var(--text-secondary); }
  .cleanup-controls select { min-width: 120px; }
  .vs-panel h4 { margin: 12px 0 8px; font-size: 12px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-muted); }

  /* Kind filter cards: glass chips, emerald accent fill when active. */
  .filter-card {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: var(--border-radius-md);
    background: rgba(255, 255, 255, 0.03);
    backdrop-filter: blur(12px);
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: all var(--motion-fast) ease;
  }
  .filter-card:hover:not(.active) {
    background: rgba(255, 255, 255, 0.06);
    color: var(--text-primary);
    border-color: rgba(16, 185, 129, 0.35);
  }
  .filter-card.active {
    color: #34d399;
    background: rgba(16, 185, 129, 0.12);
    border-color: rgba(16, 185, 129, 0.45);
    box-shadow: 0 0 12px rgba(16, 185, 129, 0.3);
  }

  /* Rollback / Delete detail actions: soft tinted pills, not gray blocks. */
  .rollback {
    color: var(--accent-primary);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }
  .rollback:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
  }
  .ghost.danger {
    color: #fca5a5;
    border: 1px solid rgba(239, 68, 68, 0.35);
  }
  .ghost.danger:hover:not(:disabled) {
    background: rgba(239, 68, 68, 0.12);
    color: #fecaca;
  }

  .kind-tag {
    font-size: 12px;
    font-weight: 800;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 1px 8px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    color: var(--text-muted);
  }
  .kind-tag.auto { color: #93c5fd; border-color: rgba(147, 197, 253, 0.4); }
  .kind-tag.manual { color: var(--accent-primary); border-color: color-mix(in srgb, var(--accent-primary) 40%, transparent); }
  .kind-tag.crash { color: #fbbf24; border-color: rgba(251, 191, 36, 0.4); }

  .actor-pill {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
    font-weight: 800;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 2px 9px;
    border-radius: 999px;
    border: 1px solid rgba(147, 197, 253, 0.4);
    color: #93c5fd;
    background: rgba(147, 197, 253, 0.07);
  }
  .actor-pill.plan { color: #c4b5fd; border-color: rgba(196, 181, 253, 0.35); background: rgba(196, 181, 253, 0.07); }

  .tag {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
    padding: 2px 9px;
    border-radius: 999px;
    background: var(--bg-elevated);
    color: var(--text-secondary);
  }
  .tag.crash-fix { color: var(--accent-primary); background: color-mix(in srgb, var(--accent-primary) 12%, transparent); }

  .diff-label { color: var(--text-muted); font-size: 12px; font-weight: 800; text-transform: uppercase; letter-spacing: 0.03em; padding: 1px 6px; border-radius: 4px; background: var(--bg-secondary); border: 1px solid var(--border-color); }
  .diff-label.added { color: var(--accent-primary); border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent); }
  .diff-label.removed { color: #fca5a5; border-color: rgba(239, 68, 68, 0.35); }
  .diff-label.modified { color: #93c5fd; border-color: rgba(147, 197, 253, 0.35); }

  .collapsible { overflow: hidden; }
  .collapse-toggle { width: 100%; justify-content: flex-start; background: transparent; border: 0; color: var(--text-secondary); font-weight: 600; padding: 12px 14px; transform: none; }
  .compare-panel { display: flex; flex-wrap: wrap; gap: 10px; align-items: center; padding: 0 14px 14px; }
  .diff-panel { display: grid; grid-template-columns: repeat(3, minmax(120px, 1fr)); gap: 12px; margin: 0 14px 14px; padding: 0; border: 0; background: transparent; }
  .muted { color: var(--text-muted); font-size: 12px; }
  .pad { padding: 24px; }
  .inline-diff-shell { display: grid; grid-template-columns: 310px minmax(0, 1fr); gap: 14px; margin: 0 14px 14px; padding: 14px; }
  .diff-files { border-right: 1px solid var(--border-color); padding-right: 14px; }
  .diff-files button { width: 100%; justify-content: space-between; text-align: left; background: transparent; color: var(--text-secondary); border: 1px solid transparent; padding: 9px 10px; margin-bottom: 5px; transform: none; }
  .diff-files button:hover, .diff-files button.selected { background: var(--bg-tertiary); border-color: color-mix(in srgb, var(--accent-primary) 28%, transparent); color: var(--text-primary); }
  .diff-files small { color: var(--text-muted); }
  .added-label { color: var(--accent-primary) !important; }
  .removed-label { color: var(--accent-danger) !important; }
  .manifest-diff-panel { margin: 0 14px 14px; padding: 14px; background: var(--bg-tertiary); border: 1px solid var(--border-color); border-radius: var(--border-radius-lg); }
  .manifest-diff-stats { display: grid; gap: 8px; margin-bottom: 12px; }
  .diff-stat { display: flex; justify-content: space-between; gap: 10px; padding: 8px 10px; border-radius: var(--border-radius-sm); font-size: 12px; background: var(--bg-secondary); border: 1px solid var(--border-color); }
  .diff-stat.changed { border-color: rgba(245,158,11,.30); }
  .diff-stat.added { border-color: color-mix(in srgb, var(--accent-primary) 30%, transparent); }
  .diff-stat.removed { border-color: rgba(239,68,68,.30); }
  .manifest-diff-text { margin: 0; padding: 12px; border-radius: 10px; background: var(--bg-elevated); color: var(--text-secondary); font-family: var(--font-mono, ui-monospace, monospace); font-size: 12px; line-height: 1.5; max-height: 360px; overflow: auto; white-space: pre-wrap; }
  .inline-diff { min-width: 0; }
  .inline-diff-header { display: flex; justify-content: space-between; gap: 12px; padding: 0 0 10px; color: var(--text-secondary); }
  .inline-diff-header span { color: var(--text-muted); font-size: 12px; }
  pre { overflow: auto; max-height: 420px; background: var(--bg-elevated); border-radius: var(--border-radius-md); padding: 12px; color: var(--text-secondary); font-size: 12px; line-height: 1.5; margin: 0; }
  pre span { display: block; white-space: pre-wrap; font-family: var(--font-mono, ui-monospace, monospace); }
  pre span.added { color: var(--accent-primary); background: color-mix(in srgb, var(--accent-primary) 8%, transparent); }
  pre span.removed { color: var(--accent-danger); background: color-mix(in srgb, var(--accent-danger) 8%, transparent); }
  pre span.context { color: var(--text-muted); }

  .backup-section { padding: 0 14px 14px; display: grid; gap: 10px; border: 0; background: transparent; }
  .backup-list { display: grid; gap: 8px; }
  .backup-row { display: flex; justify-content: space-between; align-items: center; gap: 12px; padding: 10px 12px; border-radius: 10px; background: var(--bg-tertiary); border: 1px solid var(--border-color); }
  .backup-info { display: grid; gap: 8px; flex: 1; }
  .rollback { padding: 6px 10px; font-size: 12px; font-weight: 600; }
  .danger { color: var(--accent-danger); }
  .loading { color: var(--text-muted); padding: 80px; text-align: center; background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: var(--border-radius-lg); }

  :global(.spin) { animation: spin 900ms linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
