<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    History, RefreshCw, Search, FileText, Maximize2, Save, X, RotateCcw,
    ChevronDown, ChevronRight, ScanSearch, Stethoscope, Sparkles, AlertTriangle,
  } from "@lucide/svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import {
    diagnoseFocus,
    historyFocusEventId,
    historyFocusFingerprintKey,
    historyFocusSnapshotId,
    modsFocusId,
    modsFocusFileName,
    configFocusPath,
    ideStageRequest,
    projectPath,
  } from "../lib/store";
  import type { HistoryEpisode, HistoryListResult, ProjectChangeEntry } from "../lib/api";
  import EmptyState from "./EmptyState.svelte";

  type ChangeEntry = ProjectChangeEntry;

  let entries = $state<ChangeEntry[]>([]);
  let episodes = $state<HistoryEpisode[]>([]);
  let selectedId = $state("");
  let filter = $state("");
  let categoryFilter = $state("All");
  let actorFilter = $state("All");
  let outcomeFilter = $state("All");
  let methodFilter = $state("All");
  let viewMode = $state<"episodes" | "flat">("episodes");
  let loading = $state(false);
  let scanning = $state(false);
  let error = $state<string | null>(null);
  let message = $state<string | null>(null);
  let lastLoadedPath = $state<string | null>(null);
  let explainText = $state<string | null>(null);

  let editorOpen = $state(false);
  let editorPath = $state("");
  let editorContent = $state("");
  let editorOriginal = $state("");
  let saving = $state(false);
  let expanded = $state<Record<string, boolean>>({});
  let tracked = $state<Record<string, boolean>>({
    Mods: true,
    Configs: true,
    Shaders: true,
    "Resource Packs": true,
    Resolutions: true,
    "World/Data": false,
    Other: true,
  });
  let focusedScan = $state(false);
  let settingsLoadedPath = $state<string | null>(null);

  const rootsByCategory: Record<string, string[]> = {
    Mods: ["mods"],
    Configs: ["config", "defaultconfigs", "kubejs", "scripts", "overrides", "options.txt", "servers.dat"],
    Shaders: ["shaderpacks", "shaders"],
    "Resource Packs": ["resourcepacks", "texturepacks"],
    Resolutions: [],
    "World/Data": ["datapacks", "saves"],
    Other: [],
  };

  let confirmOpen = $state(false);
  let confirmEntry = $state<ChangeEntry | null>(null);
  let visibleLimit = $state(40);
  const VISIBLE_STEP = 40;
  let prevFilterKey = $state("");

  function actorLabel(actor?: string) {
    switch ((actor ?? "").toLowerCase()) {
      case "scan": return "Disk";
      case "ai": return "AI";
      case "user": return "You";
      case "launcher": return "Launcher";
      default: return actor || "Launcher";
    }
  }

  function actorClass(actor?: string) {
    switch ((actor ?? "").toLowerCase()) {
      case "scan": return "disk";
      case "ai": return "ai";
      case "user": return "you";
      default: return "launcher";
    }
  }

  function outcomeLabel(outcome?: string) {
    switch ((outcome ?? "").toLowerCase()) {
      case "fixed": return "Fixed";
      case "broke": return "Broke";
      case "open": return "Open";
      case "rolled_back": return "Rolled back";
      case "activity": return "Activity";
      default: return outcome || "Open";
    }
  }

  function outcomeClass(outcome?: string) {
    switch ((outcome ?? "").toLowerCase()) {
      case "fixed": return "fixed";
      case "broke": return "broke";
      case "open": return "open";
      case "rolled_back": return "rolled-back";
      case "activity": return "activity";
      default: return "open";
    }
  }

  function methodLabel(method?: string | null) {
    switch ((method ?? "").toLowerCase()) {
      case "ai": return "AI";
      case "heuristic": return "Heuristic";
      case "kb": return "KB";
      case "swarm": return "Swarm";
      case "manual": return "Manual";
      case "unknown": return "Unknown";
      default: return method || "Unknown";
    }
  }

  function methodClass(method?: string | null) {
    switch ((method ?? "").toLowerCase()) {
      case "ai": return "ai";
      case "heuristic": return "heuristic";
      case "kb": return "kb";
      case "swarm": return "swarm";
      case "manual": return "manual";
      default: return "unknown";
    }
  }

  function dayKey(iso: string) {
    if (!iso) return "Unknown";
    return iso.slice(0, 10);
  }

  /** Soft date range for episode subtitle — avoid identical started→ended dumps. */
  function formatEpisodeRange(startedAt: string, endedAt?: string | null) {
    const start = dayKey(startedAt);
    if (!endedAt || endedAt === startedAt) return start;
    const end = dayKey(endedAt);
    if (end === start || end === "Unknown") return start;
    return `${start} → ${end}`;
  }

  function entryById(id: string) {
    return entries.find((e) => e.id === id);
  }

  /** True when path is empty or a bare op token (e.g. mod_change), not a real file path. */
  function isBareOpPath(path: string, kind?: string, op?: string) {
    const p = (path ?? "").trim();
    if (!p) return true;
    if (kind && p === kind) return true;
    if (op && p === op) return true;
    // snake_case token with no path separator (mod_change, external_add, …)
    if (!p.includes("/") && !p.includes("\\") && /^[a-z][a-z0-9]*(?:_[a-z0-9]+)+$/.test(p)) {
      return true;
    }
    return false;
  }

  /** The manifest itself — diff noise from mod add/remove/update snapshots.
      Demoted in the timeline and shown as "Manifest" instead of the raw file name. */
  function isManifestPath(path: string) {
    return path.replace(/\\/g, "/").toLowerCase() === "project.tuffbox.json";
  }

  /** Manifest entries sink to the bottom of their timestamp group. */
  function manifestRank(entry: ChangeEntry): number {
    return isManifestPath(entry.path ?? "") ? 1 : 0;
  }

  /** Episode actions: chronological, but manifest diffs sink to the bottom. */
  function sortedEpisodeActions(actions: ChangeEntry[]): ChangeEntry[] {
    if (actions.length < 2) return actions;
    const ranked = actions.map((a, i) => ({ a, r: manifestRank(a), i }));
    ranked.sort((x, y) => x.r - y.r || y.a.createdAt.localeCompare(x.a.createdAt) || x.i - y.i);
    return ranked.map((x) => x.a);
  }

  /** Human sentence ops (Install…, Edited file.js) vs raw path dumps. */
  function isHumanOperation(operation: string) {
    const o = (operation ?? "").trim();
    if (!o) return false;
    if (o.includes("+ //") || o.includes(": added (") || o.includes(": removed (")) return false;
    if (
      o.startsWith("Added on disk:") ||
      o.startsWith("Removed from disk:") ||
      o.startsWith("Changed on disk:")
    ) {
      return false;
    }
    return /^(Install|Remove|Update|Disable|Enable|Added|Edited|Removed|Fixed|Rolled back)\b/i.test(o);
  }

  function entryTitle(entry: ChangeEntry) {
    const path = (entry.path ?? "").trim();
    const operation = (entry.operation ?? "").trim();
    const kind = entry.kind ?? "";
    const op = entry.op ?? "";
    if (isManifestPath(path)) return "Manifest";
    if (operation && (isBareOpPath(path, kind, op) || isHumanOperation(operation))) return operation;
    if (path && !isBareOpPath(path, kind, op)) return path;
    return operation || path || kind;
  }

  /** Sidebar meta line: real file path, else humanized kind/category (avoid duplicating the title). */
  function entrySidebarMeta(entry: ChangeEntry) {
    const path = (entry.path ?? "").trim();
    const kind = entry.kind ?? "";
    const title = entryTitle(entry);
    if (isManifestPath(path)) return "project.tuffbox.json";
    if (path && !isBareOpPath(path, kind, entry.op) && path !== title) return path;
    const humanKind = kind.replaceAll("_", " ");
    if (humanKind && humanKind !== title) return humanKind;
    if (entry.category && entry.category !== title) return entry.category;
    return humanKind || entry.category || "";
  }

  /** Distinct short action titles for episode collapsed preview. */
  function episodeActionTitles(actions: ChangeEntry[], limit = 3) {
    const out: string[] = [];
    for (const a of actions) {
      const t = entryTitle(a).trim();
      if (!t || out.includes(t)) continue;
      out.push(t);
      if (out.length >= limit) break;
    }
    return out;
  }

  /** Nested mini-preview only when it adds info beyond the title/path. */
  function previewAddsInfo(entry: ChangeEntry) {
    const preview = (entry.preview ?? "").trim();
    if (!preview) return false;
    const title = entryTitle(entry).trim();
    if (!title) return preview.length > 0;
    if (preview === title || preview.startsWith(title)) return false;
    const path = (entry.path ?? "").trim();
    if (path && (preview === path || preview.startsWith(`${path}:`) || preview.startsWith(`${path} `))) {
      return false;
    }
    if (preview.includes("+ //") || /^[+\-] /.test(preview)) return false;
    return true;
  }

  function resolveEpisodeActions(episode: HistoryEpisode): ChangeEntry[] {
    return episode.actionIds
      .map((id) => entryById(id))
      .filter((e): e is ChangeEntry => !!e);
  }

  function episodeMatchesActor(episode: HistoryEpisode, actions: ChangeEntry[]) {
    if (actorFilter === "All") return true;
    if (actorFilter === "user") {
      return (episode.fixMethod ?? "").toLowerCase() === "manual" ||
        actions.some((a) => (a.actor || "").toLowerCase() === "user");
    }
    return actions.some((a) => (a.actor || "launcher").toLowerCase() === actorFilter.toLowerCase());
  }

  function showRollbackConfirm(entry: ChangeEntry) {
    confirmEntry = entry;
    confirmOpen = true;
  }

  async function doRollback() {
    if (!$projectPath || !confirmEntry) return;
    confirmOpen = false;
    loading = true;
    error = null;
    try {
      await invoke("rollback_history_file", {
        path: $projectPath,
        snapshotId: confirmEntry.snapshotId,
        relativePath: confirmEntry.path,
      });
      message = `Rolled back ${confirmEntry.path}.`;
      await load(true);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
      confirmEntry = null;
    }
  }

  async function loadHistorySettings() {
    if (!$projectPath || settingsLoadedPath === $projectPath) return;
    try {
      const settings: { tracked: Record<string, boolean>; focusedScan?: boolean } =
        await invoke("get_history_settings", { path: $projectPath });
      tracked = { ...tracked, ...(settings.tracked ?? {}) };
      focusedScan = !!settings.focusedScan;
      settingsLoadedPath = $projectPath;
    } catch {
      // Keep defaults
    }
  }

  async function saveHistorySettings() {
    if (!$projectPath) return;
    try {
      await invoke("update_history_settings", {
        path: $projectPath,
        settings: { tracked, focusedScan },
      });
      message = "History settings saved.";
      try {
        window.dispatchEvent(new CustomEvent("tuffbox:history-settings-changed"));
      } catch {
        // ignore
      }
    } catch (e) {
      error = String(e);
    }
  }

  function scrollToId(prefix: string, id: string) {
    setTimeout(() => {
      document.getElementById(prefix + id)?.scrollIntoView({ behavior: "smooth", block: "center" });
    }, 50);
  }

  function applyDeepLinks() {
    if ($historyFocusEventId) {
      const eventId = $historyFocusEventId;
      historyFocusEventId.set(null);
      const ep = episodes.find((e) => e.actionIds.includes(eventId));
      if (ep && viewMode === "episodes") {
        selectedId = ep.id;
        expanded = { ...expanded, [ep.id]: true };
        scrollToId("episode-", ep.id);
      } else if (ep) {
        viewMode = "episodes";
        selectedId = ep.id;
        expanded = { ...expanded, [ep.id]: true };
        scrollToId("episode-", ep.id);
      } else {
        selectedId = eventId;
        viewMode = "flat";
        scrollToId("change-", eventId);
      }
      return;
    }
    if ($historyFocusFingerprintKey) {
      const fp = $historyFocusFingerprintKey;
      historyFocusFingerprintKey.set(null);
      const ep = episodes.find((e) => e.fingerprintKey === fp);
      if (ep) {
        viewMode = "episodes";
        selectedId = ep.id;
        expanded = { ...expanded, [ep.id]: true };
        scrollToId("episode-", ep.id);
      }
      return;
    }
    if ($historyFocusSnapshotId) {
      const snapId = $historyFocusSnapshotId;
      historyFocusSnapshotId.set(null);
      const ep = episodes.find((e) => e.snapshotId === snapId);
      if (ep && viewMode === "episodes") {
        selectedId = ep.id;
        expanded = { ...expanded, [ep.id]: true };
        scrollToId("episode-", ep.id);
        return;
      }
      const match = entries.find((e) => e.snapshotId === snapId);
      if (match) {
        selectedId = match.id;
        scrollToId("change-", match.id);
      }
    }
  }

  async function load(force = false) {
    if (!$projectPath) return;
    await loadHistorySettings();
    if (!force && lastLoadedPath === $projectPath && entries.length > 0) return;
    loading = true;
    error = null;
    try {
      const data: HistoryListResult = await invoke("list_project_change_history", { path: $projectPath });
      entries = data.entries ?? [];
      episodes = data.episodes ?? [];
      if (viewMode === "episodes") {
        selectedId = episodes[0]?.id ?? entries[0]?.id ?? "";
      } else {
        selectedId = entries[0]?.id ?? "";
      }
      lastLoadedPath = $projectPath;
      applyDeepLinks();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function scanNow(silent = false) {
    if (!$projectPath || scanning) return;
    scanning = true;
    if (!silent) {
      error = null;
      message = null;
    }
    try {
      await saveHistorySettings();
      const res: {
        added: number;
        modified: number;
        removed: number;
        jarDrift: number;
      } = await invoke("scan_project_changes", { path: $projectPath });
      if (!silent) {
        message = `Scan: +${res.added} ~${res.modified} -${res.removed}` +
          (res.jarDrift ? ` · ${res.jarDrift} jar drift` : "");
      }
      await load(true);
    } catch (e) {
      if (!silent) error = String(e);
    } finally {
      scanning = false;
    }
  }

  async function openFullFile(entry: ChangeEntry) {
    const relPath = normalizeEntryPath(entry);
    if (!$projectPath || !relPath) return;
    loading = true;
    error = null;
    try {
      const result: { path: string; content: string } = await invoke("read_project_history_file", {
        path: $projectPath,
        relativePath: relPath,
      });
      editorPath = result.path;
      editorContent = result.content;
      editorOriginal = result.content;
      editorOpen = true;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function isModHistoryEntry(entry: ChangeEntry) {
    const kind = (entry.kind || entry.op || "").toLowerCase();
    const cat = (entry.category || "").toLowerCase();
    if (cat === "mods") return true;
    if (kind === "mod_change" || kind === "jar_drift") return true;
    if (kind.startsWith("mod_")) return true;
    if (entry.tags?.includes("jar_drift")) return true;
    const path = (entry.path || "").replace(/\\/g, "/").toLowerCase();
    return /^mods\/.+\.jar(\.disabled)?$/i.test(path);
  }

  function isConfigHistoryPath(path: string) {
    const p = path.replace(/\\/g, "/").toLowerCase();
    if (!p || p.startsWith("crash://")) return false;
    if (p === "options.txt" || p.endsWith("/options.txt")) return true;
    return (
      p.startsWith("config/") ||
      p.startsWith("defaultconfigs/") ||
      p.startsWith("kubejs/") ||
      p.startsWith("scripts/") ||
      p.startsWith("overrides/")
    );
  }

  /** Strip scan/disk prefixes and return a clean relative path, or "" if none. */
  function normalizeEntryPath(entry: ChangeEntry): string {
    const p = (entry.path || "").trim().replace(/\\/g, "/");
    const stripped = p
      .replace(/^(added|removed|changed)\s+on\s+disk:\s+/i, "")
      .replace(/^file(?::\/\/)?\s*/i, "")
      .trim();
    if (!stripped || stripped.startsWith("crash://")) return "";
    if (stripped.includes("/") || stripped.includes("\\") || /\.\w+$/.test(stripped)) return stripped;
    return p;
  }

  function resolveModFocus(entry: ChangeEntry): { id?: string; fileName?: string } | null {
    const idMatch = entry.id.match(/:mod-(?:added|removed|updated):(.+)$/i);
    if (idMatch?.[1]) return { id: idMatch[1] };
    const path = normalizeEntryPath(entry);
    const jar = path.match(/^mods\/(.+\.jar(?:\.disabled)?)$/i)?.[1];
    if (jar) return { fileName: jar.replace(/\.disabled$/i, "") };
    // Operation lines like "Install Sodium 0.5.8" — pull the jar name out as the
    // strongest affordance we have, so Open still focuses the Content list.
    const opText = `${entry.operation ?? ""} ${entry.preview ?? ""}`;
    const jarInOp = opText.match(/([^\s]+\.jar(?:\.disabled)?)\b/i)?.[1];
    if (jarInOp) return { fileName: jarInOp.replace(/\.disabled$/i, "") };
    if (isModHistoryEntry(entry)) return {};
    return null;
  }

  function canOpenEntryTarget(entry: ChangeEntry) {
    if (isModHistoryEntry(entry) && resolveModFocus(entry)) return true;
    const path = normalizeEntryPath(entry);
    if (!path) return false;
    if (entry.canOpen || isConfigHistoryPath(path)) return true;
    // Any real relative file path is at least readable in the built-in editor.
    if (/[\\/]/.test(path) || /^mods\//i.test(path) || /^config\//i.test(path)) return true;
    return false;
  }

  function openTargetLabel(entry: ChangeEntry) {
    if (isModHistoryEntry(entry)) return "Open in Mods";
    const path = normalizeEntryPath(entry);
    if (isConfigHistoryPath(path)) return "Open in Tune";
    if (path && entry.canOpen) return "Open file";
    if (path) return "Quick view";
    return "Quick view";
  }

  async function openEntryTarget(entry: ChangeEntry) {
    selectedId = entry.id;
    message = null;
    error = null;
    if (isModHistoryEntry(entry)) {
      const focus = resolveModFocus(entry);
      if (focus) {
        modsFocusId.set(focus.id ?? null);
        modsFocusFileName.set(focus.fileName ?? null);
        ideStageRequest.set("content");
        const name = focus.id ?? focus.fileName ?? entryTitle(entry);
        message = `Opened ${name} in Content.`;
        return;
      }
      ideStageRequest.set("content");
      message = `Opened Content — locate the mod manually.`;
      return;
    }
    const path = normalizeEntryPath(entry);
    if (isConfigHistoryPath(path)) {
      configFocusPath.set(path);
      ideStageRequest.set("configs");
      message = `Opened ${path} in Tune.`;
      return;
    }
    if (path) {
      await openFullFile(entry);
      if (!editorOpen) message = `Opened ${path} in the built-in editor.`;
      return;
    }
    message = "Nothing to open — this entry has no saved file.";
  }

  async function saveEditor() {
    if (!$projectPath || !editorPath || editorContent === editorOriginal) return;
    saving = true;
    error = null;
    try {
      await invoke("write_config_file", {
        path: $projectPath,
        relativePath: editorPath,
        content: editorContent,
      });
      editorOriginal = editorContent;
      message = `Saved ${editorPath}.`;
      await load(true);
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  async function explainEntry(entry: ChangeEntry) {
    if (!$projectPath) return;
    explainText = null;
    try {
      const res: any = await invoke("explain_pack_change", {
        path: $projectPath,
        eventId: entry.id,
      });
      let text = String(res.explanation ?? "");
      const excerpts = Array.isArray(res.excerpts) ? res.excerpts : [];
      if (excerpts.length) {
        const lines = excerpts
          .slice(0, 5)
          .map((ex: any) => {
            const p = String(ex?.path ?? "");
            const e = String(ex?.excerpt ?? "").slice(0, 120);
            if (!p && !e) return "";
            return p ? `• ${p}: ${e}` : `• ${e}`;
          })
          .filter(Boolean);
        if (lines.length) text = `${text}\n\n${lines.join("\n")}`.trim();
      }
      explainText = text;
    } catch (e) {
      // Legacy snapshot-derived entries may not be in events.jsonl — explain heuristically
      explainText = `${actorLabel(entry.actor)} · ${entry.kind}: ${entry.operation} — ${entry.path}`;
    }
  }

  async function explainEpisode(episode: HistoryEpisode) {
    if (!$projectPath) return;
    explainText = null;
    try {
      const res: any = await invoke("explain_history_episode", {
        path: $projectPath,
        episodeId: episode.id,
      });
      explainText = String(res.explanation ?? "");
    } catch (e) {
      const actions = resolveEpisodeActions(episode);
      const previews = actions
        .slice(0, 3)
        .map((a) => a.preview || a.operation || a.path)
        .filter(Boolean);
      const parts = [
        `${outcomeLabel(episode.outcome)} via ${methodLabel(episode.fixMethod)}.`,
        episode.summary,
      ];
      if (previews.length) parts.push(previews.join(" · "));
      explainText = parts.filter(Boolean).join(" ");
    }
  }

  function openDiagnose(entry: ChangeEntry) {
    const paths = entry.path && !entry.path.startsWith("crash://") ? [entry.path] : [];
    diagnoseFocus.set({
      paths: paths.length ? paths : null,
      fingerprintKey: entry.crashFingerprintKey ?? null,
      logPath: entry.logPath ?? null,
      episodeId: entry.episodeId ?? null,
    });
    ideStageRequest.set("diagnose");
  }

  function openDiagnoseEpisode(episode: HistoryEpisode) {
    const actions = resolveEpisodeActions(episode);
    const paths = actions.map((a) => a.path).filter((p) => !!p && !p.startsWith("crash://"));
    const logPath = episode.logPath ?? actions.find((a) => a.logPath)?.logPath ?? null;
    diagnoseFocus.set({
      paths: paths.length ? paths : null,
      fingerprintKey: episode.fingerprintKey ?? null,
      logPath,
      episodeId: episode.id,
    });
    ideStageRequest.set("diagnose");
  }

  function openEpisodeSnapshot(episode: HistoryEpisode) {
    if (!episode.snapshotId) return;
    historyFocusSnapshotId.set(episode.snapshotId);
    ideStageRequest.set("snapshots");
  }

  async function toggleExpanded(entry: ChangeEntry) {
    const next = !expanded[entry.id];
    expanded = { ...expanded, [entry.id]: next };
    selectedId = entry.id;
    if (!next || entry.diff || !$projectPath) return;
    try {
      const diff: string = await invoke("get_history_entry_diff", {
        path: $projectPath,
        entryId: entry.id,
      });
      if (!diff) return;
      entries = entries.map((e) => (e.id === entry.id ? { ...e, diff } : e));
    } catch {
      /* keep preview */
    }
  }

  function toggleEpisodeExpanded(episode: HistoryEpisode) {
    expanded = { ...expanded, [episode.id]: !expanded[episode.id] };
    selectedId = episode.id;
  }

  function lineClass(line: string) {
    if (line.startsWith("+ ")) return "added";
    if (line.startsWith("- ")) return "removed";
    return "context";
  }

  function canRollback(entry: ChangeEntry) {
    return entry.kind === "file_changed" && !!entry.snapshotId;
  }

  const categories = $derived(["All", ...Array.from(new Set(entries.map((e) => e.category)))]);
  const actors = $derived(["All", "launcher", "scan", "ai", "user"]);
  const outcomes = $derived(["All", "fixed", "broke", "open", "rolled_back", "activity"]);
  const methods = $derived(["All", "ai", "heuristic", "kb", "swarm", "manual", "unknown"]);

  const visible = $derived(entries.filter((entry) => {
    const q = filter.toLowerCase();
    const matchesText =
      !q ||
      (entry.path ?? "").toLowerCase().includes(q) ||
      (entry.kind ?? "").toLowerCase().includes(q) ||
      (entry.preview ?? "").toLowerCase().includes(q) ||
      (entry.operation || "").toLowerCase().includes(q);
    const matchesCategory = categoryFilter === "All" || entry.category === categoryFilter;
    const matchesTracked = tracked[entry.category] ?? true;
    const matchesActor =
      actorFilter === "All" ||
      (entry.actor || "launcher").toLowerCase() === actorFilter.toLowerCase();
    return matchesText && matchesCategory && matchesTracked && matchesActor;
  }));

  const visibleEpisodes = $derived(episodes.filter((episode) => {
    const actions = resolveEpisodeActions(episode);
    const q = filter.toLowerCase();
    const matchesText =
      !q ||
      episode.summary.toLowerCase().includes(q) ||
      (episode.fingerprintKey ?? "").toLowerCase().includes(q) ||
      actions.some(
        (a) =>
          (a.path ?? "").toLowerCase().includes(q) ||
          (a.preview ?? "").toLowerCase().includes(q) ||
          (a.operation || "").toLowerCase().includes(q),
      );
    const matchesOutcome = outcomeFilter === "All" || episode.outcome === outcomeFilter;
    const matchesMethod = methodFilter === "All" || (episode.fixMethod || "unknown") === methodFilter;
    const matchesActor = episodeMatchesActor(episode, actions);
    const matchesCategory =
      categoryFilter === "All" ||
      actions.length === 0 ||
      actions.some((a) => a.category === categoryFilter);
    const matchesTracked =
      actions.length === 0 ||
      actions.some((a) => tracked[a.category] ?? true);
    return matchesText && matchesOutcome && matchesMethod && matchesActor && matchesCategory && matchesTracked;
  }));

  // Within each day: newest first, manifest diffs (project.tuffbox.json) sink last.
  const byDay = $derived(
    visible
      .slice()
      .sort(
        (a, b) =>
          manifestRank(a) - manifestRank(b) || b.createdAt.localeCompare(a.createdAt),
      )
      .reduce<Record<string, ChangeEntry[]>>((acc, entry) => {
        const key = dayKey(entry.createdAt);
        acc[key] = acc[key] ?? [];
        acc[key].push(entry);
        return acc;
      }, {}),
  );
  const dayKeys = $derived(Object.keys(byDay).sort((a, b) => b.localeCompare(a)));

  const episodesByDay = $derived(visibleEpisodes.reduce<Record<string, HistoryEpisode[]>>((acc, episode) => {
    const key = dayKey(episode.startedAt);
    acc[key] = acc[key] ?? [];
    acc[key].push(episode);
    return acc;
  }, {}));
  const episodeDayKeys = $derived(Object.keys(episodesByDay).sort((a, b) => b.localeCompare(a)));

  const visibleSlice = $derived(visible.slice(0, visibleLimit));
  const visibleEpisodeSlice = $derived(visibleEpisodes.slice(0, visibleLimit));
  const filterKey = $derived(`${viewMode}|${filter}|${categoryFilter}|${actorFilter}|${outcomeFilter}|${methodFilter}`);
  $effect(() => {
    if (filterKey !== prevFilterKey) {
      prevFilterKey = filterKey;
      visibleLimit = VISIBLE_STEP;
    }
  });
  const hasMoreVisible = $derived(
    viewMode === "episodes"
      ? visibleEpisodes.length > visibleLimit
      : visible.length > visibleLimit,
  );
  const remainingVisible = $derived(
    viewMode === "episodes"
      ? visibleEpisodes.length - visibleLimit
      : visible.length - visibleLimit,
  );
  const editorDirty = $derived(editorContent !== editorOriginal);
  const hasHistory = $derived(entries.length > 0 || episodes.length > 0);
  $effect(() => {
    if ($projectPath && lastLoadedPath !== $projectPath) {
      load(true).then(() => scanNow(true));
    }
  });
</script>

<div class="change-history">
  <div class="toolbar">
    <div class="title"><History size={18} /> History · pack timeline</div>
    <div class="toolbar-actions">
      <div class="view-toggle" role="group" aria-label="Timeline mode">
        <button
          class="toggle-btn"
          class:active={viewMode === "episodes"}
          onclick={() => (viewMode = "episodes")}
        >Episodes</button>
        <button
          class="toggle-btn"
          class:active={viewMode === "flat"}
          onclick={() => (viewMode = "flat")}
        >Flat</button>
      </div>
      <div class="search">
        <Search size={15} />
        <input bind:value={filter} placeholder="Search files, mods, configs…" />
      </div>
      <select bind:value={categoryFilter}>
        {#each categories as category (category)}<option value={category}>{category}</option>{/each}
      </select>
      <select bind:value={actorFilter} title="Actor filter">
        {#each actors as a (a)}<option value={a}>{a === "All" ? "All actors" : actorLabel(a)}</option>{/each}
      </select>
      <select
        bind:value={outcomeFilter}
        title="Outcome filter"
        disabled={viewMode === "flat"}
        class:filter-muted={viewMode === "flat"}
      >
        {#each outcomes as o (o)}<option value={o}>{o === "All" ? "All outcomes" : outcomeLabel(o)}</option>{/each}
      </select>
      <select
        bind:value={methodFilter}
        title="Method filter"
        disabled={viewMode === "flat"}
        class:filter-muted={viewMode === "flat"}
      >
        {#each methods as m (m)}<option value={m}>{m === "All" ? "All methods" : methodLabel(m)}</option>{/each}
      </select>
      <button class="secondary" onclick={() => scanNow()} disabled={!$projectPath || scanning} title="Delta-scan disk vs baseline">
        <ScanSearch size={16} /> {scanning ? "Scanning…" : "Scan now"}
      </button>
      <button class="secondary" onclick={saveHistorySettings} disabled={!$projectPath || loading}>Save settings</button>
      <button class="ghost" onclick={() => load(true)} disabled={!$projectPath || loading}>
        <RefreshCw size={16} class={loading ? "spin" : ""} />
      </button>
    </div>
  </div>

  {#if error}<div class="notice error">{error}</div>{/if}
  {#if message}<div class="notice success">{message}</div>{/if}
  {#if explainText}
    <div class="explain-panel">
      <div class="explain-head">
        <strong>Explain</strong>
        <button class="ghost mini" onclick={() => (explainText = null)}><X size={14} /></button>
      </div>
      <pre class="explain-body">{explainText}</pre>
    </div>
  {/if}

  <div class="tracking-controls">
    {#each Object.keys(tracked) as key (key)}
      <label><input type="checkbox" bind:checked={tracked[key]} /> {key}{#if key === "World/Data"}<small>opt-in</small>{/if}</label>
    {/each}
    <label title="While IDE is open, rescan every 60s">
      <input type="checkbox" bind:checked={focusedScan} onchange={saveHistorySettings} /> Focused scan
    </label>
  </div>

  {#if !$projectPath}
    <EmptyState icon={History} title="No project selected" description="Open a project to view pack activity." />
  {:else if loading && !hasHistory}
    <div class="empty">Loading history…</div>
  {:else if !hasHistory}
    <EmptyState icon={History} title="No changes yet" description="Run Scan now to capture external edits, or edit via Tune / Content." />
  {:else}
    <div class="history-layout">
      <aside class="change-tree">
        <div class="timeline-line"></div>
        {#if viewMode === "episodes"}
          {#each episodeDayKeys as day (day)}
            <section>
              <h3>{day}</h3>
              {#each episodesByDay[day] as episode (episode.id)}
                <div class="timeline-item">
                  <button
                    type="button"
                    class="file-strip episode"
                    class:selected={selectedId === episode.id}
                    onclick={() => {
                      selectedId = episode.id;
                      document.getElementById("episode-" + episode.id)?.scrollIntoView({ behavior: "smooth", block: "center" });
                    }}
                    title={episode.summary}
                  >
                    <span class="outcome-badge {outcomeClass(episode.outcome)}">{outcomeLabel(episode.outcome)}</span>
                    <span class="file-title">{episode.summary || "Episode"}</span>
                    <small>{methodLabel(episode.fixMethod)} · {episode.actionIds.length} action{episode.actionIds.length === 1 ? "" : "s"}</small>
                  </button>
                </div>
              {/each}
            </section>
          {/each}
        {:else}
          {#each dayKeys as day (day)}
            <section>
              <h3>{day}</h3>
              {#each byDay[day] as entry (entry.id)}
                <div class="timeline-item">
                  <button
                    class="file-strip {entry.kind}"
                    class:selected={selectedId === entry.id}
                    onclick={() => {
                      selectedId = entry.id;
                      document.getElementById("change-" + entry.id)?.scrollIntoView({ behavior: "smooth", block: "center" });
                    }}
                    ondblclick={() => {
                      if (canOpenEntryTarget(entry)) void openEntryTarget(entry);
                    }}
                    title={entryTitle(entry)}
                  >
                    <span class="actor-pill {actorClass(entry.actor)}">{actorLabel(entry.actor)}</span>
                    <span class="file-title">{entryTitle(entry)}</span>
                    <small>{entrySidebarMeta(entry)}</small>
                  </button>
                </div>
              {/each}
            </section>
          {/each}
        {/if}
      </aside>

      <section class="change-preview">
        <div class="all-changes-list">
          {#if viewMode === "episodes" && visibleEpisodes.length === 0}
            <div class="mode-empty">
              <EmptyState
                icon={History}
                title={episodes.length === 0 ? "No episodes yet" : "No episodes match filters"}
                description={episodes.length === 0 && entries.length > 0
                  ? "Crash→fix episodes appear after a launch crash and fix. Pack file edits are also grouped as Activity episodes — try Scan now, or switch to Flat."
                  : episodes.length === 0
                    ? "Run Scan now after editing the pack, or fix a crash to create a crash episode."
                    : "Clear search / outcome / method filters to see episodes again."}
              />
              {#if episodes.length === 0 && entries.length > 0}
                <div class="empty-actions-row">
                  <button class="secondary" onclick={() => (viewMode = "flat")}>Open Flat timeline</button>
                </div>
              {/if}
            </div>
          {:else if viewMode === "episodes"}
            {#each visibleEpisodeSlice as episode (episode.id)}
              {@const actions = sortedEpisodeActions(resolveEpisodeActions(episode))}
              <div class="change-card episode-card" id="episode-{episode.id}">
                <div class="preview-header">
                  <div>
                    <span class="eyebrow">Episode · {dayKey(episode.startedAt)}</span>
                    <span class="outcome-badge {outcomeClass(episode.outcome)}">{outcomeLabel(episode.outcome)}</span>
                    {#if episode.outcome !== "activity"}
                      <span class="method-badge {methodClass(episode.fixMethod)}">{methodLabel(episode.fixMethod)}</span>
                    {/if}
                    {#if episode.planSource}
                      <span class="plan-source-badge">{episode.planSource}</span>
                    {/if}
                    <h2><History size={18} /> {episode.summary || "Untitled episode"}</h2>
                    <p>
                      {formatEpisodeRange(episode.startedAt, episode.endedAt)}
                      · {actions.length} action{actions.length === 1 ? "" : "s"}
                    </p>
                    {#if episode.resolutionSummary}
                      <p class="resolution-blurb">{episode.resolutionSummary}</p>
                    {/if}
                  </div>
                  <div class="preview-actions">
                    {#if episode.outcome !== "activity"}
                      <button type="button" class="secondary" onclick={() => explainEpisode(episode)} title="Explain this episode">
                        <Sparkles size={16} /> Explain
                      </button>
                      <button
                        type="button"
                        class="secondary"
                        onclick={() => openDiagnoseEpisode(episode)}
                        title="Open Diagnose"
                      >
                        <Stethoscope size={16} /> Diagnose
                      </button>
                    {/if}
                    {#if episode.snapshotId}
                      <button
                        type="button"
                        class="secondary"
                        onclick={() => openEpisodeSnapshot(episode)}
                        title="Open related snapshot"
                      >
                        Snapshot
                      </button>
                    {/if}
                  </div>
                </div>

                <div
                  class="summary-card"
                  role="button"
                  tabindex="0"
                  onclick={() => toggleEpisodeExpanded(episode)}
                  onkeydown={(e) => (e.key === "Enter" || e.key === " ") && toggleEpisodeExpanded(episode)}
                >
                  <div class="summary-row">
                    <strong>{actions.length === 1 ? "Actions" : `${actions.length} changes`}</strong>
                    <span class="chev">{#if expanded[episode.id]}<ChevronDown size={16} />{:else}<ChevronRight size={16} />{/if}</span>
                  </div>
                  {#if !expanded[episode.id]}
                    <pre class="mini-preview">{episodeActionTitles(actions).join("\n") || "No actions linked."}</pre>
                  {/if}
                </div>

                {#if expanded[episode.id]}
                  <div class="episode-actions">
                    {#if actions.length === 0}
                      <div class="empty-actions">No linked actions found for this episode.</div>
                    {:else}
                      {#each actions as entry (entry.id)}
                        <div
                          class="nested-action"
                          class:manifest={isManifestPath(entry.path ?? "")}
                          id="change-{entry.id}"
                          role="button"
                          tabindex="0"
                          ondblclick={() => {
                            if (canOpenEntryTarget(entry)) void openEntryTarget(entry);
                          }}
                        >
                          <div class="nested-head">
                            <span class="actor-pill {actorClass(entry.actor)}">{actorLabel(entry.actor)}</span>
                            <strong>{entryTitle(entry)}</strong>
                            <small>{entrySidebarMeta(entry)}</small>
                          </div>
                          {#if previewAddsInfo(entry)}
                            <pre class="mini-preview nested">{entry.preview}</pre>
                          {/if}
                          <div class="preview-actions nested">
                            <button type="button" class="secondary" onclick={() => explainEntry(entry)} title="Explain this change">
                              <Sparkles size={14} /> Explain
                            </button>
                            <button type="button" class="secondary" onclick={() => openDiagnose(entry)} title="Open Diagnose">
                              <Stethoscope size={14} /> Diagnose
                            </button>
                            <button type="button" class="secondary" onclick={() => showRollbackConfirm(entry)} disabled={!canRollback(entry)}>
                              <RotateCcw size={14} /> Rollback
                            </button>
                            <button
                              type="button"
                              class="secondary"
                              onclick={() => void openEntryTarget(entry)}
                              disabled={!canOpenEntryTarget(entry)}
                              title={openTargetLabel(entry)}
                            >
                              <Maximize2 size={14} /> Open
                            </button>
                          </div>
                        </div>
                      {/each}
                    {/if}
                  </div>
                {/if}
              </div>
            {/each}
          {:else}
            {#each visibleSlice as entry (entry.id)}
              <div
                class="change-card"
                id="change-{entry.id}"
                class:jar-drift={entry.kind === "jar_drift" || entry.tags?.includes("jar_drift")}
                class:manifest={isManifestPath(entry.path ?? "")}
                role="button"
                tabindex="0"
                ondblclick={() => {
                  if (canOpenEntryTarget(entry)) void openEntryTarget(entry);
                }}
                onkeydown={(e) => {
                  if (e.key === "Enter" && canOpenEntryTarget(entry)) {
                    e.preventDefault();
                    void openEntryTarget(entry);
                  }
                }}
              >
                <div class="preview-header">
                  <div>
                    <span class="eyebrow">{entry.category} · {entry.kind.replaceAll("_", " ")}</span>
                    <span class="actor-pill {actorClass(entry.actor)}">{actorLabel(entry.actor)}</span>
                    {#if entry.fixMethod}
                      <span class="method-badge {methodClass(entry.fixMethod)}">{methodLabel(entry.fixMethod)}</span>
                    {/if}
                    {#if entry.tags?.includes("crash_resolved") || entry.kind === "crash_resolved"}
                      <span class="crash-resolved-badge">resolved{#if entry.planSource} · {entry.planSource}{/if}</span>
                    {:else if entry.tags?.includes("crash_fix")}
                      <span class="crash-fix-badge">crash_fix{#if entry.planSource} · {entry.planSource}{/if}</span>
                    {/if}
                    {#if entry.kind === "jar_drift" || entry.tags?.includes("jar_drift")}
                      <span class="drift-badge"><AlertTriangle size={11} /> jar drift — import to manifest or remove</span>
                    {/if}
                    <h2><FileText size={18} /> {entryTitle(entry)}</h2>
                    <p>{entry.createdAt} · {entry.reason}</p>
                  </div>
                  <div class="preview-actions">
                    <button class="secondary" onclick={() => explainEntry(entry)} title="Explain this change">
                      <Sparkles size={16} /> Explain
                    </button>
                    <button class="secondary" onclick={() => openDiagnose(entry)} title="Open Diagnose">
                      <Stethoscope size={16} /> Diagnose
                    </button>
                    <button class="secondary" onclick={() => showRollbackConfirm(entry)} disabled={!canRollback(entry)}>
                      <RotateCcw size={16} /> Rollback
                    </button>
                    <button
                      class="secondary"
                      onclick={() => void openEntryTarget(entry)}
                      disabled={!canOpenEntryTarget(entry)}
                      title={openTargetLabel(entry)}
                    >
                      <Maximize2 size={16} /> Open
                    </button>
                  </div>
                </div>

                <div
                  class="summary-card"
                  role="button"
                  tabindex="0"
                  onclick={() => toggleExpanded(entry)}
                  onkeydown={(e) => (e.key === "Enter" || e.key === " ") && toggleExpanded(entry)}
                >
                  <div class="summary-row">
                    <strong>{entry.operation}</strong>
                    <span class="chev">{#if expanded[entry.id]}<ChevronDown size={16} />{:else}<ChevronRight size={16} />{/if}</span>
                  </div>
                  {#if !expanded[entry.id]}
                    <pre class="mini-preview">{entry.preview || "No preview available."}</pre>
                  {/if}
                </div>

                {#if expanded[entry.id]}
                  <div class="diff-card">
                    <div class="diff-title">Details</div>
                    <pre>
{#each (entry.diff || entry.preview || "No diff available.").split("\n") as line, i (i)}
<span class={lineClass(line)}>{line}</span>
{/each}
                    </pre>
                  </div>
                {/if}
              </div>
            {/each}
          {/if}
          {#if hasMoreVisible}
            <div class="show-more-row">
              <button type="button" class="secondary" onclick={() => (visibleLimit += VISIBLE_STEP)}>
                Show more ({remainingVisible} remaining)
              </button>
            </div>
          {/if}
        </div>
      </section>
    </div>
  {/if}
</div>

{#if editorOpen}
  <div class="editor-backdrop">
    <div class="editor-modal">
      <div class="editor-head">
        <div>
          <span class="eyebrow">Built-in editor</span>
          <h2>{editorPath}</h2>
        </div>
        <div class="editor-actions">
          {#if editorDirty}<span class="dirty">Unsaved</span>{/if}
          <button onclick={saveEditor} disabled={!editorDirty || saving}>
            <Save size={16} /> {saving ? "Saving…" : "Save"}
          </button>
          <button class="icon-btn" onclick={() => (editorOpen = false)}><X size={18} /></button>
        </div>
      </div>
      <textarea bind:value={editorContent} spellcheck="false"></textarea>
    </div>
  </div>
{/if}

{#if confirmOpen}
  <ConfirmDialog title="Rollback file?" message={`Restore ${confirmEntry?.path ?? "file"} from snapshot?`} danger={false}
    confirmLabel="Rollback" onconfirm={doRollback} oncancel={() => (confirmOpen = false, confirmEntry = null)} />
{/if}

<style>
  .change-history { width: 100%; max-width: min(1440px, 100%); margin: 0 auto; }
  .toolbar, .toolbar-actions, .title, .preview-header, .preview-header h2, .editor-head, .editor-actions, .tracking-controls, .preview-actions, .summary-row { display: flex; align-items: center; }
  .toolbar { justify-content: space-between; gap: 16px; margin-bottom: 14px; flex-wrap: wrap; align-items: center; }
  .title { gap: 10px; color: var(--text-secondary); font-weight: 800; flex-shrink: 0; }
  .toolbar-actions { gap: 10px; flex-wrap: wrap; align-items: center; min-width: 0; }
  .toolbar-actions select {
    min-width: 132px;
    flex: 0 1 auto;
  }
  .view-toggle {
    display: inline-flex;
    flex-shrink: 0;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    overflow: hidden;
    background: var(--bg-elevated);
  }
  .toggle-btn {
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--text-muted);
    padding: 8px 12px;
    font-size: 12px;
    font-weight: 800;
  }
  .toggle-btn.active {
    /* Accent text on an accent tint measured 2.95:1 in the light themes; the
       mix keeps the hue but restores contrast. */
    color: color-mix(in srgb, var(--accent-primary) 62%, var(--text-primary));
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
  }
  .search {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 240px;
    flex: 1 1 240px;
    max-width: 320px;
    padding: 0 12px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-elevated);
    color: var(--text-muted);
  }
  .toolbar-actions select.filter-muted,
  .toolbar-actions select:disabled {
    opacity: 0.55;
    color: var(--text-muted);
    cursor: not-allowed;
  }
  .search :global(svg) {
    flex-shrink: 0;
    color: var(--text-muted);
  }
  .search input {
    flex: 1;
    min-width: 0;
    width: 100%;
    border: 0;
    background: transparent;
    color: var(--text-primary);
    padding: 10px 0;
    outline: none;
  }
  .notice, .empty { background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: var(--border-radius-lg); }
  .notice { padding: 12px 14px; margin-bottom: 14px; }
  .notice.error { color: var(--accent-danger); background: color-mix(in srgb, var(--accent-danger) 8%, transparent); border-color: color-mix(in srgb, var(--accent-danger) 28%, transparent); }
  .notice.success { color: var(--accent-primary); background: color-mix(in srgb, var(--accent-primary) 8%, transparent); border-color: color-mix(in srgb, var(--accent-primary) 25%, transparent); }
  .explain-panel {
    margin-bottom: 14px;
    padding: 12px 14px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
  }
  .explain-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin-bottom: 8px;
  }
  .explain-head strong { color: var(--text-secondary); font-weight: 800; }
  .explain-body {
    max-height: 220px;
    overflow: auto;
    white-space: pre-wrap;
    margin: 0;
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
    line-height: 1.55;
    color: var(--text-primary);
  }
  .resolution-blurb {
    margin: 8px 0 0;
    color: var(--text-secondary);
    font-size: 13px;
    line-height: 1.45;
  }
  .preview-header p.resolution-blurb {
    color: var(--text-secondary);
    font-size: 13px;
  }
  .mini { padding: 4px 8px; font-size: 12px; }
  .tracking-controls { flex-wrap: wrap; gap: 8px; margin-bottom: 14px; padding: 10px; border: 1px solid var(--border-color); border-radius: var(--border-radius-lg); background: rgba(255,255,255,.018); }
  .tracking-controls label { display: flex; align-items: center; gap: 8px; color: var(--text-secondary); font-size: 12px; }
  .tracking-controls label small { font-size: 12px; color: var(--text-muted); }
  .tracking-controls input { width: auto; }
  .tracking-controls small { color: var(--text-muted); }
  .empty { color: var(--text-muted); padding: 80px; text-align: center; }
  .history-layout { display: grid; grid-template-columns: 340px minmax(0, 1fr); gap: 16px; min-height: 76vh; }
  .change-tree, .change-preview, .summary-card, .diff-card { background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: var(--border-radius-lg); }
  .change-tree { position: relative; padding: 14px 14px 14px 28px; overflow: auto; background: transparent; border-color: transparent; }
  .timeline-line { position: absolute; left: 18px; top: 20px; bottom: 20px; width: 2px; background: linear-gradient(180deg, color-mix(in srgb, var(--accent-primary) 70%, transparent), color-mix(in srgb, var(--accent-secondary) 25%, transparent)); border-radius: 999px; }
  h3 { margin: 16px 6px 8px; color: var(--text-muted); font-size: 12px; text-transform: uppercase; letter-spacing: .08em; }
  .timeline-item { position: relative; margin-bottom: 10px; }
  .timeline-item::before { content: ""; position: absolute; left: -15px; top: 23px; width: 15px; height: 2px; background: color-mix(in srgb, var(--accent-primary) 50%, transparent); }
  .timeline-item::after { content: ""; position: absolute; left: -19px; top: 18px; width: 10px; height: 10px; border-radius: 50%; background: var(--bg-secondary); border: 2px solid var(--accent-primary); }
  .file-strip { width: 100%; min-height: 54px; display: flex; flex-direction: column; align-items: flex-start; justify-content: center; gap: 8px; text-align: left; background: var(--bg-tertiary); color: var(--text-primary); border: 1px solid transparent; border-radius: var(--border-radius-md); padding: 10px 12px; transform: none; }
  .file-strip:hover, .file-strip.selected { border-color: color-mix(in srgb, var(--accent-primary) 34%, transparent); background: color-mix(in srgb, var(--accent-primary) 7%, transparent); }
  .file-title { display: block; width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 800; }
  .file-strip small { display: block; width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-muted); font-size: 12px; }
  .actor-pill, .outcome-badge, .method-badge, .plan-source-badge {
    display: inline-block; font-size: 12px; font-weight: 800; text-transform: uppercase; letter-spacing: .04em;
    padding: 1px 6px; border-radius: 999px; border: 1px solid var(--border-color); margin-right: 6px;
  }
  .actor-pill.launcher { color: var(--accent-secondary); border-color: color-mix(in srgb, var(--accent-secondary) 35%, transparent); }
  .actor-pill.disk { color: var(--accent-warning); border-color: color-mix(in srgb, var(--accent-warning) 40%, transparent); }
  .actor-pill.ai { color: var(--accent-secondary); border-color: color-mix(in srgb, var(--accent-secondary) 40%, transparent); }
  .actor-pill.you { color: var(--accent-primary); border-color: color-mix(in srgb, var(--accent-primary) 40%, transparent); }
  .outcome-badge.fixed {
    color: var(--accent-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 40%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
  }
  .outcome-badge.broke {
    color: var(--accent-danger);
    border-color: color-mix(in srgb, var(--accent-danger) 40%, transparent);
    background: color-mix(in srgb, var(--accent-danger) 10%, transparent);
  }
  .outcome-badge.open {
    color: var(--accent-warning);
    border-color: color-mix(in srgb, var(--accent-warning) 40%, transparent);
    background: color-mix(in srgb, var(--accent-warning) 10%, transparent);
  }
  .outcome-badge.rolled-back {
    color: var(--text-muted);
    border-color: var(--border-color);
    background: color-mix(in srgb, var(--bg-elevated) 40%, transparent);
  }
  .outcome-badge.activity {
    color: var(--text-secondary);
    border-color: color-mix(in srgb, var(--accent-secondary) 35%, var(--border-color));
    background: color-mix(in srgb, var(--accent-secondary) 10%, transparent);
  }
  .empty-actions-row {
    display: flex;
    justify-content: center;
    margin-top: 8px;
    margin-bottom: 8px;
  }
  .mode-empty {
    padding: 24px 12px 12px;
  }
  .method-badge.ai { color: var(--accent-secondary); border-color: color-mix(in srgb, var(--accent-secondary) 40%, transparent); }
  .method-badge.heuristic { color: var(--accent-secondary); border-color: color-mix(in srgb, var(--accent-secondary) 35%, transparent); }
  .method-badge.kb { color: color-mix(in srgb, var(--accent-secondary) 62%, var(--text-primary)); border-color: color-mix(in srgb, var(--accent-secondary) 35%, transparent); }
  .method-badge.swarm { color: color-mix(in srgb, var(--accent-danger) 62%, var(--text-primary)); border-color: color-mix(in srgb, var(--accent-danger) 35%, transparent); }
  .method-badge.manual { color: var(--accent-primary); border-color: color-mix(in srgb, var(--accent-primary) 40%, transparent); }
  .method-badge.unknown { color: var(--text-muted); border-color: var(--border-color); }
  .plan-source-badge { color: var(--text-secondary); }
  .change-preview { min-width: 0; padding: 16px; overflow-y: auto; max-height: 80vh; }
  .change-card { margin-bottom: 28px; padding-bottom: 28px; border-bottom: 1px solid var(--border-color); }
  .change-card.jar-drift { border-color: color-mix(in srgb, var(--accent-warning) 40%, transparent); }
  /* Manifest diffs: quiet, secondary — they're bookkeeping, not real file edits. */
  .change-card.manifest { opacity: 0.62; }
  .change-card.manifest:hover { opacity: 1; }
  .change-card:last-child { border-bottom: none; }
  .show-more-row { display: flex; justify-content: center; padding: 8px 0 16px; }
  .mini-preview { margin-top: 10px; max-height: 80px; overflow: hidden; opacity: 0.7; mask-image: linear-gradient(to bottom, black 50%, transparent 100%); }
  .mini-preview.nested { max-height: 56px; margin-top: 6px; }
  .preview-header { justify-content: space-between; gap: 16px; margin-bottom: 14px; flex-wrap: wrap; }
  .preview-actions { gap: 8px; flex-wrap: wrap; justify-content: flex-end; }
  .preview-actions.nested { justify-content: flex-start; margin-top: 8px; }
  .preview-header h2 { gap: 10px; margin: 4px 0; font-size: 18px; }
  .preview-header p, .eyebrow { color: var(--text-muted); font-size: 12px; }
  .eyebrow { color: var(--accent-primary); text-transform: uppercase; letter-spacing: .1em; font-weight: 900; }
  .crash-fix-badge, .crash-resolved-badge, .drift-badge {
    display: inline-flex; align-items: center; gap: 8px; margin-left: 8px; padding: 2px 8px; border-radius: 999px; font-size: 12px;
  }
  .crash-fix-badge { background: color-mix(in srgb, var(--accent-warning) 15%, transparent); color: var(--accent-warning); border: 1px solid color-mix(in srgb, var(--accent-warning) 30%, transparent); }
  .crash-resolved-badge { background: color-mix(in srgb, var(--accent-primary) 15%, transparent); color: var(--accent-primary); border: 1px solid color-mix(in srgb, var(--accent-primary) 35%, transparent); }
  .drift-badge { background: color-mix(in srgb, var(--accent-warning) 12%, transparent); color: var(--accent-warning); border: 1px solid color-mix(in srgb, var(--accent-warning) 35%, transparent); }
  .summary-card, .diff-card { padding: 14px; margin-bottom: 14px; background: var(--bg-tertiary); cursor: pointer; }
  .summary-row { justify-content: space-between; }
  .summary-card strong, .diff-title { display: block; color: var(--text-secondary); margin-bottom: 10px; font-weight: 800; }
  .episode-actions { display: flex; flex-direction: column; gap: 10px; margin-bottom: 8px; }
  .nested-action {
    padding: 12px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-tertiary);
  }
  .nested-action.manifest { opacity: 0.62; }
  .nested-action.manifest:hover { opacity: 1; }
  .nested-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .nested-head strong { color: var(--text-primary); font-size: 13px; }
  .nested-head small { color: var(--text-muted); font-size: 12px; text-transform: uppercase; letter-spacing: .04em; }
  .empty-actions { color: var(--text-muted); font-size: 13px; padding: 12px; }
  pre { overflow: auto; white-space: pre-wrap; margin: 0; font-family: var(--font-mono, ui-monospace, monospace); font-size: 12px; line-height: 1.55; color: var(--text-secondary); }
  .diff-card pre { max-height: 58vh; background: var(--bg-elevated); border-radius: var(--border-radius-md); padding: 12px; }
  pre span { display: block; }
  .added { color: var(--accent-primary); background: color-mix(in srgb, var(--accent-primary) 8%, transparent); }
  .removed { color: var(--accent-danger); background: color-mix(in srgb, var(--accent-danger) 8%, transparent); }
  .context { color: var(--text-muted); }
  .editor-backdrop { position: fixed; inset: 0; z-index: 60; background: rgba(0,0,0,.68); backdrop-filter: blur(12px); display: flex; align-items: center; justify-content: center; padding: 24px; }
  .editor-modal { width: min(1500px, 96vw); height: min(900px, 92vh); background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: var(--border-radius-xl); overflow: hidden; display: flex; flex-direction: column; box-shadow: 0 30px 110px rgba(0,0,0,.55); }
  .editor-head { justify-content: space-between; gap: 16px; padding: 18px 20px; border-bottom: 1px solid var(--border-color); }
  .editor-head h2 { margin: 3px 0 0; font-size: 18px; }
  .editor-actions { gap: 10px; }
  .dirty { color: var(--accent-warning); font-size: 12px; font-weight: 800; }
  .icon-btn { width: 36px; height: 36px; padding: 0; background: transparent; color: var(--text-muted); }
  textarea { flex: 1; width: 100%; resize: none; border: 0; outline: none; padding: 20px; background: var(--bg-elevated); color: var(--text-primary); font-family: var(--font-mono, ui-monospace, monospace); font-size: 13px; line-height: 1.65; }
  :global(.spin) { animation: spin 900ms linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (max-width: 1000px) { .history-layout { grid-template-columns: 1fr; } .search { min-width: 0; } }
</style>
