<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { save } from "@tauri-apps/plugin-dialog";
  import {
    PackageOpen,
    RefreshCw,
    UploadCloud,
    CheckCircle2,
    AlertTriangle,
    FolderOpen,
    Layers,
    Server,
    Box,
    FileArchive,
  } from "@lucide/svelte";
  import {
    projectPath,
    projectInfo,
    pushWorkTrail,
    isProjectRunning,
    runningInstances,
  } from "../lib/store";
  import EmptyState from "./EmptyState.svelte";

  type ExportResult = {
    path: string;
    fileCount: number;
    overrideCount: number;
    warnings?: string[] | null;
  };

  type ExportIssue = {
    severity: "error" | "warning";
    code: string;
    message: string;
    target?: string | null;
  };

  type ExportMode = "mrpack" | "server" | "prism" | "curseforge";

  type BatchRow = {
    kind: string;
    status: string;
    path?: string;
    files?: number;
    overrideCount?: number;
    error?: string;
    warnings?: string[] | null;
  };

  let targetPath = $state("");
  let serverTargetPath = $state("");
  let prismTargetPath = $state("");
  let curseforgeTargetPath = $state("");
  let projectDir = $state("");
  let exporting = $state(false);
  let batching = $state(false);
  let result = $state<ExportResult | null>(null);
  let batchResults = $state<BatchRow[]>([]);
  let error = $state<string | null>(null);
  let issues = $state<ExportIssue[]>([]);
  let issuesLoading = $state(false);
  let exportMode = $state<ExportMode>("mrpack");

  let lastPathForDefaults = $state("");
  let lastInfoKey = $state("");

  const modeMeta: Record<
    ExportMode,
    { title: string; ext: string; filters: { name: string; extensions: string[] }[] }
  > = {
    mrpack: {
      title: "Modrinth pack",
      ext: "mrpack",
      filters: [{ name: "Modrinth pack", extensions: ["mrpack"] }],
    },
    server: {
      title: "Server pack",
      ext: "zip",
      filters: [{ name: "ZIP archive", extensions: ["zip"] }],
    },
    prism: {
      title: "Prism instance",
      ext: "zip",
      filters: [{ name: "ZIP archive", extensions: ["zip"] }],
    },
    curseforge: {
      title: "CurseForge pack",
      ext: "zip",
      filters: [{ name: "ZIP archive", extensions: ["zip"] }],
    },
  };

  function pathForMode(mode: ExportMode): string {
    switch (mode) {
      case "mrpack":
        return targetPath;
      case "server":
        return serverTargetPath;
      case "prism":
        return prismTargetPath;
      case "curseforge":
        return curseforgeTargetPath;
    }
  }

  function setPathForMode(mode: ExportMode, value: string) {
    switch (mode) {
      case "mrpack":
        targetPath = value;
        break;
      case "server":
        serverTargetPath = value;
        break;
      case "prism":
        prismTargetPath = value;
        break;
      case "curseforge":
        curseforgeTargetPath = value;
        break;
    }
  }

  async function loadDefaultPaths(path: string) {
    issuesLoading = true;
    error = null;
    try {
      projectDir = await invoke<string>("get_project_dir", { path });
      issues = await invoke<ExportIssue[]>("validate_modrinth_export", { path });
      const id = $projectInfo?.id ?? "modpack";
      const version = $projectInfo?.version ?? "1.0.0";
      const exportDir = `${projectDir}/export`;
      targetPath = `${exportDir}/${id}-${version}.mrpack`;
      serverTargetPath = `${exportDir}/${id}-${version}-server.zip`;
      prismTargetPath = `${exportDir}/${id}-${version}-prism.zip`;
      curseforgeTargetPath = `${exportDir}/${id}-${version}-curseforge.zip`;
    } catch (e) {
      error = String(e);
      issues = [];
    } finally {
      issuesLoading = false;
    }
  }

  function refreshDefaultPath() {
    if (!$projectPath) return;
    void loadDefaultPaths($projectPath);
  }

  function onProjectPathChange(path: string | null, infoKey: string) {
    if (!path) return;
    if (path !== lastPathForDefaults) {
      lastPathForDefaults = path;
      lastInfoKey = infoKey;
      result = null;
      batchResults = [];
      void loadDefaultPaths(path);
      return;
    }
    // projectInfo loads async *after* the path is set — without this the
    // default filenames stick to the "modpack-1.0.0" fallbacks. Refresh once
    // when the real id/version arrive; never clobber afterwards (the user
    // may have typed custom paths — Refresh button covers renames).
    const hadInfo = lastInfoKey !== "" && lastInfoKey !== "@";
    lastInfoKey = infoKey;
    if (!hadInfo && infoKey !== "" && infoKey !== "@") {
      void loadDefaultPaths(path);
    }
  }

  async function browseSave(mode: ExportMode) {
    const meta = modeMeta[mode];
    const current = pathForMode(mode);
    const defaultPath = current || undefined;
    try {
      const picked = await save({
        title: `Save ${meta.title}`,
        defaultPath,
        filters: meta.filters,
      });
      if (typeof picked === "string" && picked.trim()) {
        setPathForMode(mode, picked);
      }
    } catch (e) {
      // User cancelled or dialog unavailable — keep typed path.
      if (String(e).toLowerCase().includes("cancel")) return;
      error = String(e);
    }
  }

  async function exportMrpack() {
    await runExport("export_modrinth_pack", targetPath || null);
  }

  async function exportServerPack() {
    await runExport("export_server_pack", serverTargetPath || null);
  }

  async function exportPrismInstance() {
    await runExport("export_prism_instance", prismTargetPath || null);
  }

  async function exportCurseForgePack() {
    await runExport("export_curseforge_pack", curseforgeTargetPath || null);
  }

  async function runExport(command: string, pathValue: string | null) {
    if (!$projectPath) return;
    exporting = true;
    error = null;
    result = null;
    batchResults = [];
    try {
      // Re-validate before mrpack so the button can't race a stale issues list.
      if (command === "export_modrinth_pack") {
        issues = await invoke<ExportIssue[]>("validate_modrinth_export", {
          path: $projectPath,
        });
        if (issues.some((i) => i.severity === "error")) {
          error = "Fix blocking export issues before building a .mrpack.";
          return;
        }
      }
      result = await invoke<ExportResult>(command, {
        path: $projectPath,
        targetPath: pathValue && pathValue.trim() ? pathValue.trim() : null,
      });
      if (result) {
        pushWorkTrail(`Export ready · ${result.path}`, [
          { id: "release", label: "Open Release", kind: "stage", stage: "release" },
          { id: "dismiss", label: "Dismiss", kind: "dismiss" },
        ]);
      }
    } catch (e) {
      error = String(e);
    } finally {
      exporting = false;
    }
  }

  async function exportAll() {
    if (!$projectPath) return;
    batching = true;
    exporting = true;
    error = null;
    result = null;
    batchResults = [];
    try {
      batchResults = await invoke<BatchRow[]>("batch_export_all", {
        path: $projectPath,
      });
      const failed = batchResults.filter((r) => r.status !== "ok");
      if (failed.length === batchResults.length && batchResults.length > 0) {
        error = failed.map((f) => `${f.kind}: ${f.error ?? "failed"}`).join("; ");
      } else if (failed.length > 0) {
        error = `Partial export — ${failed.length} format(s) failed.`;
      } else if (batchResults.length > 0) {
        const first = batchResults.find((r) => r.path);
        if (first?.path) {
          pushWorkTrail(`Exported ${batchResults.length} formats · ${first.path}`, [
            { id: "release", label: "Open Release", kind: "stage", stage: "release" },
            { id: "dismiss", label: "Dismiss", kind: "dismiss" },
          ]);
        }
      }
    } catch (e) {
      error = String(e);
    } finally {
      batching = false;
      exporting = false;
    }
  }

  async function revealPath(filePath: string) {
    try {
      await invoke("reveal_export_path", { path: filePath });
    } catch (e) {
      error = String(e);
    }
  }

  async function openExportFolder() {
    if (!projectDir) return;
    try {
      // Prefer export/ if it exists; otherwise open the project root.
      await invoke("reveal_export_path", { path: `${projectDir}/export` });
    } catch {
      try {
        await invoke("reveal_export_path", { path: projectDir });
      } catch {
        try {
          await invoke("open_project_folder", { path: $projectPath });
        } catch (e) {
          error = String(e);
        }
      }
    }
  }

  /** The game may be running while exporting: nothing in an export is
   *  write-locked by the JVM (jars are opened read-shared, saves/logs are not
   *  exported), so we ALLOW it — but say so instead of failing silently on a
   *  config that is mid-flush. */
  const gameRunning = $derived(isProjectRunning($projectPath, $runningInstances));

  const blockingErrors = $derived(issues.filter((i) => i.severity === "error"));
  const warnings = $derived(issues.filter((i) => i.severity === "warning"));
  const busy = $derived(exporting || batching || issuesLoading);

  $effect(() => {
    onProjectPathChange($projectPath, `${$projectInfo?.id ?? ""}@${$projectInfo?.version ?? ""}`);
  });
</script>

<div class="export-builder">
  <div class="toolbar">
    <div class="title"><UploadCloud size={18} /> Export builder</div>
    <div class="toolbar-actions">
      <button class="ghost" onclick={refreshDefaultPath} disabled={!$projectPath || busy}>
        <RefreshCw size={16} />
        Refresh paths
      </button>
      <button class="ghost" onclick={openExportFolder} disabled={!projectDir || busy}>
        <FolderOpen size={16} />
        Open export folder
      </button>
    </div>
  </div>

  {#if error}
    <div class="notice error" role="alert">
      <AlertTriangle size={16} />
      <span>{error}</span>
    </div>
  {/if}
  {#if gameRunning}
    <div class="notice warn" role="status">
      <AlertTriangle size={16} />
      <div class="notice-body">
        <strong>Minecraft is running with this pack</strong>
        <span>
          Exporting is safe — the game only reads mod jars, and saves/logs are never exported. Config
          files reflect the live session; stop the game first for a byte-perfect export.
        </span>
      </div>
    </div>
  {/if}
  {#if result}
    <div class="notice success" role="status">
      <CheckCircle2 size={16} />
      <div class="notice-body">
        <strong>Export complete</strong>
        <span>
          {result.fileCount} remote entries · {result.overrideCount} override files
        </span>
        {#if result.warnings && result.warnings.length > 0}
          <span class="export-warnings">
            {result.warnings.length} file{result.warnings.length === 1 ? "" : "s"} skipped (unreadable —
            locked or in use): {result.warnings.slice(0, 3).join("; ")}{result.warnings.length > 3 ? "…" : ""}
          </span>
        {/if}
        <code class="path-chip">{result.path}</code>
        <button class="linkish" onclick={() => void revealPath(result!.path)}>
          <FolderOpen size={14} /> Show in folder
        </button>
      </div>
    </div>
  {/if}
  {#if batchResults.length > 0}
    <div class="batch-results" role="status">
      <h3>Batch export</h3>
      <ul>
        {#each batchResults as row (row.kind)}
          <li class:ok={row.status === "ok"} class:bad={row.status !== "ok"}>
            <strong>{row.kind}</strong>
            {#if row.status === "ok"}
              <span>{row.files ?? 0} files · {row.overrideCount ?? 0} overrides</span>
              {#if row.path}
                <code>{row.path}</code>
                <button class="linkish" onclick={() => void revealPath(row.path!)}>
                  Show
                </button>
              {/if}
            {:else}
              <span class="err-text">{row.error ?? "failed"}</span>
            {/if}
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  {#if !$projectPath}
    <EmptyState
      icon={PackageOpen}
      title="No project selected"
      description="Open a project to export a modpack."
    />
  {:else}
    <section class="panel">
      <div class="format-grid" role="tablist" aria-label="Export format">
        <button
          type="button"
          role="tab"
          class="format-card"
          class:active={exportMode === "mrpack"}
          aria-selected={exportMode === "mrpack"}
          onclick={() => (exportMode = "mrpack")}
        >
          <PackageOpen size={28} />
          <div>
            <h2>Modrinth .mrpack</h2>
            <p>modrinth.index.json + remote downloads + overrides.</p>
          </div>
        </button>
        <button
          type="button"
          role="tab"
          class="format-card"
          class:active={exportMode === "server"}
          aria-selected={exportMode === "server"}
          onclick={() => (exportMode = "server")}
        >
          <Server size={28} />
          <div>
            <h2>Server pack</h2>
            <p>Server-safe mods, configs, manifest and start scripts.</p>
          </div>
        </button>
        <button
          type="button"
          role="tab"
          class="format-card"
          class:active={exportMode === "prism"}
          aria-selected={exportMode === "prism"}
          onclick={() => (exportMode = "prism")}
        >
          <Box size={28} />
          <div>
            <h2>Prism instance</h2>
            <p>instance.cfg + mmc-pack.json + mods/configs.</p>
          </div>
        </button>
        <button
          type="button"
          role="tab"
          class="format-card"
          class:active={exportMode === "curseforge"}
          aria-selected={exportMode === "curseforge"}
          onclick={() => (exportMode = "curseforge")}
        >
          <FileArchive size={28} />
          <div>
            <h2>CurseForge zip</h2>
            <p>manifest.json + overrides + remote mod manifest.</p>
          </div>
        </button>
      </div>

      <div class="path-row">
        <label>
          Output path
          {#if exportMode === "mrpack"}
            <input bind:value={targetPath} placeholder="…/export/my-pack-1.0.0.mrpack" />
          {:else if exportMode === "server"}
            <input
              bind:value={serverTargetPath}
              placeholder="…/export/my-pack-1.0.0-server.zip"
            />
          {:else if exportMode === "prism"}
            <input
              bind:value={prismTargetPath}
              placeholder="…/export/my-pack-1.0.0-prism.zip"
            />
          {:else}
            <input
              bind:value={curseforgeTargetPath}
              placeholder="…/export/my-pack-1.0.0-curseforge.zip"
            />
          {/if}
        </label>
        <button
          type="button"
          class="ghost browse"
          onclick={() => void browseSave(exportMode)}
          disabled={busy}
        >
          Browse…
        </button>
      </div>

      <div class="checks">
        <div>
          <strong>Dependencies</strong>
          <span>Minecraft + selected loader are written to the index.</span>
        </div>
        <div>
          <strong>Mods</strong>
          <span>Modrinth/direct URL mods export as remote downloads; local jars go to overrides.</span>
        </div>
        <div>
          <strong>Overrides</strong>
          <span
            >config / defaultconfigs / kubejs / scripts / resourcepacks / shaderpacks / datapacks
            (+ .tuffboxignore).</span
          >
        </div>
        <div>
          <strong>Server pack</strong>
          <span>Skips client-only mods; includes start scripts and download manifest.</span>
        </div>
      </div>

      {#if issuesLoading}
        <div class="notice muted">Checking pack for export issues…</div>
      {:else if issues.length > 0}
        <div class="issues">
          <div class="issues-head">
            {#if blockingErrors.length > 0}
              <AlertTriangle size={16} />
              <strong
                >{blockingErrors.length} blocking error{blockingErrors.length === 1
                  ? ""
                  : "s"}</strong
              >
            {:else}
              <CheckCircle2 size={16} />
              <strong>Ready to export</strong>
              <span class="muted-inline"
                >({warnings.length} warning{warnings.length === 1 ? "" : "s"})</span
              >
            {/if}
          </div>
          {#each issues as issue (issue.code + (issue.target ?? "") + issue.message)}
            <div class="issue {issue.severity}">
              <strong>{issue.code}</strong>
              <span>{issue.message}</span>
              {#if issue.target}<code>{issue.target}</code>{/if}
            </div>
          {/each}
        </div>
      {:else}
        <div class="notice muted success-lite">
          <CheckCircle2 size={16} /> No export issues detected.
        </div>
      {/if}

      <div class="publish-section">
        <h3>Publish is in Release</h3>
        <p>
          This stage only builds local artifacts (and records them for Release). Configure tokens
          in Settings, then open the Release stage to publish to Modrinth, CurseForge or GitHub
          Releases.
        </p>
      </div>

      <div class="export-actions">
        {#if exportMode === "mrpack"}
          <button
            class="export"
            onclick={exportMrpack}
            disabled={busy || blockingErrors.length > 0}
          >
            <UploadCloud size={16} />
            {exporting && !batching ? "Exporting…" : "Export .mrpack"}
          </button>
        {:else if exportMode === "server"}
          <button class="export" onclick={exportServerPack} disabled={busy}>
            <Server size={16} />
            {exporting && !batching ? "Exporting…" : "Export server pack"}
          </button>
        {:else if exportMode === "prism"}
          <button class="export" onclick={exportPrismInstance} disabled={busy}>
            <Box size={16} />
            {exporting && !batching ? "Exporting…" : "Export Prism instance"}
          </button>
        {:else if exportMode === "curseforge"}
          <button class="export" onclick={exportCurseForgePack} disabled={busy}>
            <FileArchive size={16} />
            {exporting && !batching ? "Exporting…" : "Export CurseForge zip"}
          </button>
        {/if}

        <button class="secondary" onclick={exportAll} disabled={busy || blockingErrors.length > 0}>
          <Layers size={16} />
          {batching ? "Exporting all…" : "Export all formats"}
        </button>
      </div>
    </section>
  {/if}
</div>

<style>
  .export-builder {
    /* Document page (form + cards), not a canvas — same centered cap as the
       sibling document stages ReleaseRoom/OreGenVisualizer (1240). The old
       max-width: none stretched the path input and card grids edge-to-edge
       on wide windows. */
    max-width: min(1240px, 100%);
    margin: 0 auto;
    width: 100%;
  }
  .toolbar,
  .title,
  .notice,
  .format-card,
  .toolbar-actions,
  .path-row,
  .export-actions,
  .issues-head {
    display: flex;
    align-items: center;
  }
  .toolbar {
    justify-content: space-between;
    margin-bottom: 16px;
    gap: 12px;
    flex-wrap: wrap;
  }
  .toolbar-actions {
    gap: 8px;
    flex-wrap: wrap;
  }
  .title {
    gap: 10px;
    color: var(--text-secondary);
    font-weight: 700;
  }
  .notice {
    gap: 10px;
    padding: 12px 14px;
    border-radius: var(--border-radius-lg);
    margin-bottom: 14px;
    border: 1px solid var(--border-color);
  }
  .notice.error {
    color: #fecaca;
    background: rgba(239, 68, 68, 0.08);
    border-color: rgba(239, 68, 68, 0.28);
  }
  .notice.success {
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
    border-color: color-mix(in srgb, var(--accent-primary) 25%, transparent);
    align-items: flex-start;
  }
  .notice.muted,
  .notice.success-lite {
    color: var(--text-muted);
    background: var(--bg-tertiary);
  }
  .notice.warn {
    color: var(--text-secondary);
    background: color-mix(in srgb, var(--accent-warning) 10%, transparent);
    border-color: color-mix(in srgb, var(--accent-warning) 30%, transparent);
    align-items: flex-start;
  }
  .export-warnings {
    color: var(--accent-warning);
    font-size: 12px;
    word-break: break-all;
  }
  .notice-body {
    display: grid;
    gap: 8px;
  }
  .path-chip {
    display: block;
    word-break: break-all;
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
    color: var(--text-secondary);
  }
  .linkish {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    background: none;
    border: none;
    color: var(--accent-primary);
    cursor: pointer;
    padding: 0;
    font: inherit;
    width: fit-content;
  }
  .panel {
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
    padding: 22px;
    display: grid;
    gap: 18px;
  }
  .format-grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 12px;
  }
  .format-card {
    gap: 14px;
    padding: 18px;
    text-align: left;
    justify-content: flex-start;
    color: var(--text-secondary);
    border-radius: var(--border-radius-lg);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    transform: none;
    cursor: pointer;
  }
  .format-card.active {
    background: radial-gradient(
        circle at top left,
        color-mix(in srgb, var(--accent-primary) 12%, transparent),
        transparent 45%
      ),
      var(--bg-tertiary);
    border-color: color-mix(in srgb, var(--accent-primary) 45%, transparent);
    color: var(--text-primary);
  }
  .format-card h2 {
    margin: 0 0 4px;
    font-size: 15px;
  }
  .format-card p,
  .checks span {
    color: var(--text-muted);
    margin: 0;
    font-size: 12px;
    line-height: 1.4;
  }
  label {
    display: grid;
    gap: 8px;
    color: var(--text-secondary);
    font-weight: 700;
    flex: 1;
    min-width: 0;
  }
  input {
    width: 100%;
  }
  .path-row {
    gap: 10px;
    align-items: end;
  }
  .browse {
    flex-shrink: 0;
    height: 40px;
  }
  .checks {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 12px;
  }
  .checks div {
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    border-radius: 14px;
    padding: 14px;
    display: grid;
    gap: 8px;
  }
  .issues {
    display: grid;
    gap: 8px;
  }
  .issues-head {
    gap: 8px;
    color: var(--text-secondary);
  }
  .muted-inline {
    color: var(--text-muted);
    font-weight: 500;
  }
  .issue {
    display: grid;
    gap: 8px;
    padding: 12px;
    border-radius: var(--border-radius-md);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
  }
  .issue.warning {
    border-color: rgba(245, 158, 11, 0.3);
  }
  .issue.error {
    border-color: rgba(239, 68, 68, 0.3);
  }
  .issue span {
    color: var(--text-muted);
  }
  code {
    color: var(--text-secondary);
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
    word-break: break-all;
  }
  .publish-section {
    padding: 16px;
    border: 1px solid color-mix(in srgb, var(--accent-primary) 25%, transparent);
    border-radius: var(--border-radius-lg);
    background: color-mix(in srgb, var(--accent-primary) 3%, transparent);
  }
  .publish-section h3 {
    color: var(--text-primary);
    font-size: 14px;
    margin: 0 0 4px;
  }
  .publish-section p {
    color: var(--text-muted);
    font-size: 12px;
    margin: 0;
    line-height: 1.45;
  }
  .export-actions {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
  }
  .export {
    justify-self: start;
  }
  .batch-results {
    margin-bottom: 14px;
    padding: 14px 16px;
    border-radius: var(--border-radius-lg);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
  }
  .batch-results h3 {
    margin: 0 0 10px;
    font-size: 14px;
  }
  .batch-results ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 8px;
  }
  .batch-results li {
    display: grid;
    gap: 8px;
    padding: 10px 12px;
    border-radius: var(--border-radius-md);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
  }
  .batch-results li.ok {
    border-color: color-mix(in srgb, var(--accent-primary) 30%, transparent);
  }
  .batch-results li.bad {
    border-color: rgba(239, 68, 68, 0.35);
  }
  .err-text {
    color: #fecaca;
  }
  /* Laptop band: 4 cards x ~250px get cramped — drop to 2x2 before the
     single-column fallback. */
  @media (min-width: 901px) and (max-width: 1280px) {
    .checks,
    .format-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
  @media (max-width: 900px) {
    .checks,
    .format-grid {
      grid-template-columns: 1fr;
    }
    .path-row {
      flex-direction: column;
      align-items: stretch;
    }
  }
</style>
