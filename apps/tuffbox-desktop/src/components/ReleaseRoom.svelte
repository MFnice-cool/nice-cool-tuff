<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-shell";
  import { Rocket, RefreshCw, AlertTriangle, CheckCircle2, Camera, Package, Server, FolderOpen, FolderTree, UploadCloud } from "@lucide/svelte";
  import { api } from "../lib/api";
  import { projectPath, projectInfo, recentProjects } from "../lib/store";
  import EmptyState from "./EmptyState.svelte";
  import ReleaseMetrics from "./ReleaseMetrics.svelte";
  import ChangelogEditor from "./ChangelogEditor.svelte";
  import PublishPlatformsConfig from "./PublishPlatformsConfig.svelte";

  type Issue = { severity: "error" | "warning"; code: string; message: string; target?: string | null };
  type Artifact = { id: string; kind: string; path: string; createdAt: string; fileCount: number; overrideCount: number };
  type PublishConfig = {
    githubRepository: string;
    modrinthProjectId: string;
    curseforgeProjectId: string;
    curseforgeGameVersionIds: number[];
  };
  type PublishResult = {
    target: string;
    id: string;
    url?: string | null;
    uploadedFiles?: string[];
  };

  let version = $state($projectInfo?.version ?? "1.0.0");
  let changelog = $state("");
  let issues = $state<Issue[]>([]);
  let artifacts = $state<Artifact[]>([]);
  let checklist = $state<Record<string, boolean>>({
      version: false,
      validation: false,
      artifacts: false,
      changelog: false,
      snapshot: false,
      publish_target: false,
  });

  let publishConfig = $state<PublishConfig>({
    githubRepository: "",
    modrinthProjectId: "",
    curseforgeProjectId: "",
    curseforgeGameVersionIds: [],
  });
  let curseforgeGameVersionIdsText = $state("");
  let configLoading = $state(false);
  let configSaving = $state(false);
  let publishingTarget = $state<string | null>(null);
  let publishResults = $state<Record<string, PublishResult>>({});
  let publishErrors = $state<Record<string, string>>({});
  let selectedTargets = $state<Record<string, boolean>>({ github: true, modrinth: true, curseforge: true });

  let exportLoading = $state<string | null>(null);
  let githubRelease = $state<any>(null);
  let githubLoading = $state(false);
  let loading = $state(false);
  let error = $state("");
  let message = $state("");
  let lastLoadedPath: string | null = null;

  const CHECKLIST_LABELS: Record<string, string> = {
    version: "Version set",
    validation: "No blocking errors",
    artifacts: "Artifacts exported",
    changelog: "Changelog written",
    snapshot: "Snapshot created",
    publish_target: "Publish target configured (GitHub repo or Modrinth project)",
  };

  function parseGameVersionIds(text: string): number[] {
    return text
      .split(/[,\s]+/)
      .map((part) => part.trim())
      .filter(Boolean)
      .map((part) => Number(part))
      .filter((n) => Number.isFinite(n) && n > 0);
  }

  function hasArtifact(kind: string) {
    return artifacts.some((a) => a.kind === kind);
  }

  function canPublish(target: string) {
    if (!$projectPath || !!publishingTarget || errorCount > 0) return false;
    if (target === "github") {
      return !!publishConfig.githubRepository.trim() && artifacts.length > 0;
    }
    if (target === "modrinth") {
      return !!publishConfig.modrinthProjectId.trim() && hasArtifact("mrpack");
    }
    if (target === "curseforge") {
      return (
        !!publishConfig.curseforgeProjectId.trim() &&
        publishConfig.curseforgeGameVersionIds.length > 0 &&
        hasArtifact("curseforge")
      );
    }
    return false;
  }

  function targetState(id: string) {
    if (publishResults[id]) {
      return publishResults[id].url
        ? `published · ${publishResults[id].id}`
        : `published · id ${publishResults[id].id}`;
    }
    if (publishErrors[id]) return "publish failed";
    if (id === "github") {
      return artifacts.length > 0
        ? (publishConfig.githubRepository.trim() ? "ready to publish" : "needs repository")
        : "needs artifacts";
    }
    if (id === "modrinth") {
      if (!hasArtifact("mrpack")) return "not exported";
      return publishConfig.modrinthProjectId.trim() ? "ready to publish" : "needs project id";
    }
    if (id === "curseforge") {
      if (!hasArtifact("curseforge")) return "not exported";
      if (!publishConfig.curseforgeProjectId.trim()) return "needs project id";
      if (publishConfig.curseforgeGameVersionIds.length === 0) return "needs game version ids";
      return "ready to publish";
    }
    return "idle";
  }

  async function loadPublishConfig() {
    if (!$projectPath) return;
    configLoading = true;
    try {
      publishConfig = await invoke<PublishConfig>("get_publish_config", { path: $projectPath });
      curseforgeGameVersionIdsText = (publishConfig.curseforgeGameVersionIds ?? []).join(", ");
    } catch (e) {
      error = String(e);
    } finally {
      configLoading = false;
    }
  }

  async function savePublishConfig() {
    if (!$projectPath) return;
    configSaving = true;
    error = "";
    message = "";
    try {
      const config: PublishConfig = {
        githubRepository: publishConfig.githubRepository.trim(),
        modrinthProjectId: publishConfig.modrinthProjectId.trim(),
        curseforgeProjectId: publishConfig.curseforgeProjectId.trim(),
        curseforgeGameVersionIds: parseGameVersionIds(curseforgeGameVersionIdsText),
      };
      await invoke("save_publish_config", { path: $projectPath, config });
      publishConfig = config;
      curseforgeGameVersionIdsText = config.curseforgeGameVersionIds.join(", ");
      message = "Publish config saved.";
    } catch (e) {
      error = String(e);
    } finally {
      configSaving = false;
    }
  }

  async function publish(target: string) {
    if (!$projectPath || !canPublish(target)) return;
    publishingTarget = target;
    error = "";
    message = "";
    publishErrors = { ...publishErrors, [target]: "" };
    try {
      const result = await invoke<PublishResult>("publish_release", {
        path: $projectPath,
        target,
        changelog,
      });
      publishResults = { ...publishResults, [target]: result };
      message = result.url
        ? `Published to ${target}: ${result.url}`
        : `Published to ${target} (id ${result.id}).`;
    } catch (e) {
      publishErrors = { ...publishErrors, [target]: String(e) };
      error = String(e);
    } finally {
      publishingTarget = null;
    }
  }

  async function publishAllSelected() {
    for (const target of ["github", "modrinth", "curseforge"]) {
      if (selectedTargets[target] && canPublish(target)) await publish(target);
    }
  }

  function showPublishSummary() {
    message = `Dry-run: ${Object.entries(selectedTargets).filter(([, selected]) => selected).map(([target]) => target).join(", ") || "no channels selected"}.`;
  }

  async function openPublishUrl(url?: string | null) {
    if (!url) return;
    try {
      await open(url);
    } catch (e) {
      error = String(e);
    }
  }

  async function generateGithubRelease() {
    if (!$projectPath) return;
    githubLoading = true;
    try {
      const tag = version.trim() ? `v${version.trim()}` : null;
      githubRelease = await invoke("generate_github_release", { path: $projectPath, tag, target: null });
      message = `GitHub release notes prepared: ${githubRelease.tagName}`;
    } catch (e) {
      error = String(e);
    } finally {
      githubLoading = false;
    }
  }

  async function copyReleaseBody() {
    if (!githubRelease) return;
    try {
      await navigator.clipboard.writeText(githubRelease.body);
      message = "Release body copied to clipboard.";
    } catch {
      message = "Failed to copy.";
    }
  }

  async function refresh() {
    if (!$projectPath) return;
    loading = true;
    error = "";
    message = "";
    try {
      issues = await invoke("validate_modrinth_export", { path: $projectPath });
      changelog = await invoke("generate_release_changelog", { path: $projectPath });
      artifacts = await invoke("list_release_artifacts", { path: $projectPath });
      version = $projectInfo?.version ?? version;
      await loadPublishConfig();
      lastLoadedPath = $projectPath;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function exportArtifact(kind: "mrpack" | "server" | "prism" | "curseforge" | "packwiz") {
    if (!$projectPath) return;
    exportLoading = kind;
    error = "";
    message = "";
    try {
      let result: { path: string; fileCount: number };
      if (kind === "mrpack") result = await api.export.modrinthPack(null, $projectPath);
      else if (kind === "server") result = await api.export.serverPack(null, $projectPath);
      else if (kind === "prism") result = await api.export.prismInstance(null, $projectPath);
      else if (kind === "curseforge") result = await api.export.curseforgePack(null, $projectPath);
      else result = await api.export.packwizPack(null, $projectPath);
      message = `Exported ${kind}: ${result.path}`;
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      exportLoading = null;
    }
  }

  async function openProjectFolder() {
    if (!$projectPath) return;
    try {
      await invoke("open_project_folder", { path: $projectPath });
    } catch (e) {
      error = String(e);
    }
  }

  function incrementVersion(part: "patch" | "minor" | "major") {
    const [major = 0, minor = 0, patch = 0] = version.split(".").map((value) => Number.parseInt(value, 10) || 0);
    version = part === "major" ? `${major + 1}.0.0` : part === "minor" ? `${major}.${minor + 1}.0` : `${major}.${minor}.${patch + 1}`;
  }

  async function saveVersion() {
    if (!$projectPath || !version.trim()) return;
    loading = true;
    error = "";
    message = "";
    try {
      const info: any = await invoke("update_project_version", { path: $projectPath, version: version.trim() });
      projectInfo.set(info);
      recentProjects.updateInfo($projectPath, info);
      message = `Version updated to ${version}. Auto snapshot created.`;
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function copyArtifactPath(path: string) {
    try {
      await navigator.clipboard.writeText(path);
      message = "Artifact path copied.";
    } catch {
      message = path;
    }
  }

  async function createReleaseDraft() {
    if (!$projectPath) return;
    loading = true;
    error = "";
    message = "";
    try {
      const result: any = await invoke("create_release_draft", { path: $projectPath, changelog });
      message = `Release draft created: ${result.draftPath} (${result.artifactCount} artifacts).`;
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function createReleaseSnapshot() {
    if (!$projectPath) return;
    loading = true;
    error = "";
    message = "";
    try {
      const result: any = await invoke("create_release_snapshot", { path: $projectPath, changelog });
      checklist.snapshot = true;
      message = `Release snapshot ${result.snapshot.id} created. Changelog: ${result.changelogPath}`;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function onProjectPathChange(path: string | null) {
    if (!path || path === lastLoadedPath) return;
    void refresh();
  }

  const errorCount = $derived(issues.filter((issue) => issue.severity === "error").length);
  const warningCount = $derived(issues.filter((issue) => issue.severity === "warning").length);
  $effect(() => {
    checklist.version = Boolean(version.trim());
  });
  $effect(() => {
    checklist.validation = errorCount === 0;
  });
  $effect(() => {
    checklist.artifacts = artifacts.length > 0;
  });
  $effect(() => {
    checklist.changelog = changelog.trim().length > 0;
  });
  $effect(() => {
    checklist.publish_target =
      publishConfig.githubRepository.trim().length > 0 ||
      publishConfig.modrinthProjectId.trim().length > 0;
  });
  const releaseReady = $derived(Object.values(checklist).every(Boolean));
  $effect(() => {
    onProjectPathChange($projectPath);
  });
</script>

<div class="release-room bg-black/30 backdrop-blur-2xl rounded-2xl border border-white/[0.08] shadow-[inset_0_1px_0_rgba(255,255,255,0.1)] p-6">
  <div class="toolbar">
    <div class="title"><Rocket size={18} /> Release room</div>
    <button class="ghost" onclick={refresh} disabled={!$projectPath || loading}>
      <RefreshCw size={16} class={loading ? "spin" : ""} /> Refresh
    </button>
  </div>

  {#if error}<div class="notice error"><AlertTriangle size={16} /> {error}</div>{/if}
  {#if message}<div class="notice success"><CheckCircle2 size={16} /> {message}</div>{/if}

  {#if !$projectPath}
    <EmptyState icon={Rocket} title="No project selected" description="Open a project to prepare a release." />
  {:else}
    <div class="layout">
      <section class="panel release-panel">
        <h2>Version & ship</h2>
        <ReleaseMetrics
          bind:version
          changedMods={warningCount + errorCount}
          components={changelog.split("\n").filter(Boolean).length}
          onSave={saveVersion}
          onIncrement={incrementVersion}
        />

        <PublishPlatformsConfig
          bind:config={publishConfig}
          bind:gameVersionIdsText={curseforgeGameVersionIdsText}
          configSaving={configSaving}
          configLoading={configLoading}
          onSave={savePublishConfig}
        />

        <div class="publish-targets">
          <h3>Export & publish</h3>
          <div class="publish-target">
            <div><strong>Modrinth</strong><span>{targetState("modrinth")}</span></div>
            <div class="target-actions">
              <button class="secondary mini" onclick={() => exportArtifact("mrpack")} disabled={!!exportLoading || errorCount > 0}>
                {exportLoading === "mrpack" ? "…" : "Export .mrpack"}
              </button>
              <button class="mini" onclick={() => publish("modrinth")} disabled={!canPublish("modrinth")}>
                <UploadCloud size={12} /> {publishingTarget === "modrinth" ? "Publishing…" : "Publish"}
              </button>
            </div>
            {#if publishErrors.modrinth}<small class="pub-err">{publishErrors.modrinth}</small>{/if}
            {#if publishResults.modrinth}
              <small class="pub-ok">
                id {publishResults.modrinth.id}
                {#if publishResults.modrinth.url}
                  · <button class="linkish" onclick={() => openPublishUrl(publishResults.modrinth.url)}>{publishResults.modrinth.url}</button>
                {/if}
              </small>
            {/if}
          </div>

          <div class="publish-target">
            <div><strong>CurseForge</strong><span>{targetState("curseforge")}</span></div>
            <div class="target-actions">
              <button class="secondary mini" onclick={() => exportArtifact("curseforge")} disabled={!!exportLoading || errorCount > 0}>
                {exportLoading === "curseforge" ? "…" : "Export zip"}
              </button>
              <button class="mini" onclick={() => publish("curseforge")} disabled={!canPublish("curseforge")}>
                <UploadCloud size={12} /> {publishingTarget === "curseforge" ? "Publishing…" : "Publish"}
              </button>
            </div>
            {#if publishErrors.curseforge}<small class="pub-err">{publishErrors.curseforge}</small>{/if}
            {#if publishResults.curseforge}
              <small class="pub-ok">
                id {publishResults.curseforge.id}
                {#if publishResults.curseforge.url}
                  · <button class="linkish" onclick={() => openPublishUrl(publishResults.curseforge.url)}>{publishResults.curseforge.url}</button>
                {/if}
              </small>
            {/if}
          </div>

          <div class="publish-target">
            <div><strong>GitHub Releases</strong><span>{targetState("github")}</span></div>
            <div class="target-actions">
              <button class="secondary mini" onclick={generateGithubRelease} disabled={githubLoading}>
                {githubLoading ? "…" : "Prepare notes"}
              </button>
              <button class="mini" onclick={() => publish("github")} disabled={!canPublish("github")}>
                <UploadCloud size={12} /> {publishingTarget === "github" ? "Publishing…" : "Publish"}
              </button>
            </div>
            {#if publishErrors.github}<small class="pub-err">{publishErrors.github}</small>{/if}
            {#if publishResults.github}
              <small class="pub-ok">
                id {publishResults.github.id}
                {#if publishResults.github.url}
                  · <button class="linkish" onclick={() => openPublishUrl(publishResults.github.url)}>{publishResults.github.url}</button>
                {/if}
              </small>
            {/if}
          </div>
        </div>

        <details class="release-checklist-details" open={!releaseReady}>
          <summary>Release checklist</summary>
          <div class="release-checklist">
            {#each Object.entries(checklist) as [key, done] (key)}
              <label class:done>
                <input type="checkbox" bind:checked={checklist[key]} />
                <span>{CHECKLIST_LABELS[key] ?? key}</span>
              </label>
            {/each}
            <div class="ready" class:ok={releaseReady}>{releaseReady ? "Ready to ship" : "Release not ready yet"}</div>
          </div>
        </details>

        <div class="quick-exports">
          <h3>Quick exports</h3>
          <div class="export-btns">
            <button class="secondary mini" onclick={() => exportArtifact("server")} disabled={!!exportLoading}>
              <Server size={12} /> {exportLoading === "server" ? "…" : "Server pack"}
            </button>
            <button class="secondary mini" onclick={() => exportArtifact("prism")} disabled={!!exportLoading}>
              <Package size={12} /> {exportLoading === "prism" ? "…" : "Prism zip"}
            </button>
            <button class="secondary mini" onclick={() => exportArtifact("packwiz")} disabled={!!exportLoading}>
              <FolderTree size={12} /> {exportLoading === "packwiz" ? "…" : "Packwiz"}
            </button>
            <button class="ghost mini" onclick={openProjectFolder}>
              <FolderOpen size={12} /> Open folder
            </button>
          </div>
        </div>

        <div class="artifact-list">
          <h3>Artifacts</h3>
          {#if artifacts.length === 0}
            <div class="muted-box">No exported artifacts recorded yet. Use Export stage first.</div>
          {:else}
            {#each artifacts.slice(0, 6) as artifact (artifact.id)}
              <div class="artifact-row">
                <strong>{artifact.kind}</strong>
                <span>{artifact.path}</span>
                <small>{artifact.fileCount} files · {artifact.overrideCount} overrides</small>
                <button class="ghost mini" onclick={() => copyArtifactPath(artifact.path)}>Copy path</button>
              </div>
            {/each}
          {/if}
        </div>

        <div class="issues">
          {#if issues.length === 0}
            <div class="issue ok"><CheckCircle2 size={16} /> Export validation passed.</div>
          {:else}
            {#each issues as issue (issue.code + (issue.target ?? '') + issue.message)}
              <div class="issue {issue.severity}">
                <strong>{issue.code}</strong>
                <span>{issue.message}</span>
                {#if issue.target}<code>{issue.target}</code>{/if}
              </div>
            {/each}
          {/if}
        </div>

        {#if githubRelease}
          <div class="github-preview">
            <h4>GitHub Release notes: {githubRelease.tagName}</h4>
            <div class="github-actions">
              <button class="secondary mini" onclick={copyReleaseBody}>Copy body</button>
              <span class="gh-meta">{githubRelease.artifactCount} artifacts · release.json saved</span>
            </div>
            <pre class="gh-body-preview">{githubRelease.body?.slice(0, 2000)}{githubRelease.body?.length > 2000 ? "..." : ""}</pre>
          </div>
        {/if}

        <div class="release-actions">
                  <button class="primary-emerald" onclick={createReleaseDraft} disabled={loading || !changelog.trim()}>
                    <Rocket size={16} /> Create release draft
                  </button>
                  <button onclick={createReleaseSnapshot} disabled={loading || errorCount > 0}>
                    <Camera size={16} /> Create release snapshot
                  </button>
                </div>
      </section>

      <ChangelogEditor bind:value={changelog} onRegenerate={refresh} onAi={generateGithubRelease} disabled={loading || githubLoading} />
    </div>
    <div class="sticky bottom-3 z-20 mt-4 flex flex-col gap-4 rounded-2xl border border-emerald-500/20 bg-neutral-950/90 p-4 shadow-[0_0_25px_rgba(16,185,129,0.12)] backdrop-blur-2xl lg:flex-row lg:items-center lg:justify-between">
      <div class="flex flex-wrap gap-3 text-xs text-[var(--text-secondary)]">
        {#each ["github", "modrinth", "curseforge"] as target}
          <label class="flex items-center gap-2"><input type="checkbox" checked={selectedTargets[target]} onchange={(event) => selectedTargets = { ...selectedTargets, [target]: event.currentTarget.checked }} /> {target === "github" ? "GitHub" : target === "modrinth" ? "Modrinth" : "CurseForge"}</label>
        {/each}
      </div>
      <div class="flex flex-wrap items-center gap-2"><button type="button" class="rounded-xl border border-white/10 px-4 py-3 text-sm text-neutral-200 hover:bg-white/10" onclick={showPublishSummary}>Предпросмотр перед отправкой</button><button type="button" class="rounded-xl bg-emerald-600 px-8 py-3 font-bold text-white shadow-[0_0_20px_rgba(16,185,129,0.35)] hover:bg-emerald-500" onclick={publishAllSelected} disabled={!!publishingTarget || errorCount > 0}>Опубликовать релиз во всех каналах</button></div>
    </div>
  {/if}
</div>

<style>
  .release-room { max-width: min(1240px, 100%); margin: 0 auto; width: 100%; }
  .toolbar, .title, .notice, .version-row, .changelog-header { display: flex; align-items: center; }
  .toolbar { justify-content: space-between; margin-bottom: 16px; }
  .title { gap: 10px; color: var(--text-secondary); font-weight: 700; }
  .notice { gap: 10px; padding: 12px 14px; border-radius: var(--border-radius-lg); margin-bottom: 14px; border: 1px solid var(--border-color); }
  .notice.error { color: #fecaca; background: rgba(239, 68, 68, 0.08); border-color: rgba(239, 68, 68, 0.28); }
  .notice.success { color: var(--accent-primary); background: color-mix(in srgb, var(--accent-primary) 8%, transparent); border-color: color-mix(in srgb, var(--accent-primary) 25%, transparent); }
  .layout { display: grid; grid-template-columns: 380px minmax(0, 1fr); gap: 16px; }
    .panel { background: rgba(255, 255, 255, 0.03); border: 1px solid rgba(255, 255, 255, 0.08); border-radius: var(--border-radius-lg); backdrop-filter: blur(20px); box-shadow: 0 20px 50px rgba(0, 0, 0, 0.35); }
    .panel { padding: 18px; }
    .release-panel { display: grid; gap: 18px; align-content: start; }
    label { display: grid; gap: 8px; color: var(--text-secondary); font-weight: 700; }
    .version-row { gap: 10px; }
    .scorecards { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; }
    .scorecards strong { font-size: 24px; }
    .scorecards span{ color: var(--text-muted); font-size: 12px; }
    .error-card { border-color: rgba(239, 68, 68, 0.35) !important; color: #fecaca; }
    .warning-card { border-color: rgba(245, 158, 11, 0.35) !important; color: #fde68a; }
    .release-checklist, .artifact-list, .publish-targets, .publish-config { display: grid; gap: 8px; }
    .release-checklist-details { border: 1px solid rgba(255, 255, 255, 0.08); border-radius: var(--border-radius-md); background: rgba(255, 255, 255, 0.03); }
    .release-checklist-details summary { padding: 10px 12px; cursor: pointer; color: var(--text-secondary); font-size: 14px; font-weight: 700; list-style: none; }
    .release-checklist-details summary::-webkit-details-marker { display: none; }
    .release-checklist-details .release-checklist { padding: 0 12px 12px; }
    .artifact-list h3,
 .publish-targets h3{ margin: 0; color: var(--text-secondary); font-size: 14px; }
    .config-hint { margin: 0; color: var(--text-muted); font-size: 12px; line-height: 1.4; }
    .release-checklist label { display: flex; align-items: center; gap: 8px; padding: 9px 10px; background: rgba(255, 255, 255, 0.03); border: 1px solid rgba(255, 255, 255, 0.08); border-radius: var(--border-radius-md); text-transform: none; letter-spacing: 0; }
    .release-checklist label.done { border-color: rgba(16, 185, 129, 0.35); }
    .release-checklist input { width: auto; }
    .ready { padding: 9px 10px; border-radius: var(--border-radius-md); color: var(--text-muted); background: rgba(255, 255, 255, 0.03); border: 1px solid rgba(255, 255, 255, 0.08); }
    .ready.ok { color: #34d399; border-color: rgba(16, 185, 129, 0.4); }
    .publish-target { display: grid; gap: 8px; padding: 10px; border-radius: var(--border-radius-md); background: rgba(255, 255, 255, 0.03); border: 1px solid rgba(255, 255, 255, 0.08); }
  .publish-target > div:first-child { display: grid; gap: 8px; }
  .target-actions, .export-btns { display: flex; gap: 8px; flex-wrap: wrap; }
  .quick-exports { display: grid; gap: 8px; }
  .quick-exports h3 { margin: 0; color: var(--text-secondary); font-size: 14px; }
  .publish-target strong { color: var(--text-primary); }
  .publish-target span { color: var(--text-muted); font-size: 12px; }
  .pub-err { color: #fecaca; font-size: 12px; word-break: break-word; }
  .pub-ok { color: var(--accent-primary); font-size: 12px; word-break: break-all; }
  .linkish { background: none; border: none; color: var(--accent-secondary); padding: 0; font-size: 12px; cursor: pointer; text-decoration: underline; }
  .artifact-row, .muted-box { display: grid; gap: 8px; padding: 10px; border-radius: var(--border-radius-md); background: var(--bg-tertiary); border: 1px solid var(--border-color); }
  .artifact-row strong { color: var(--text-primary); text-transform: uppercase; font-size: 12px; }
  .artifact-row span, .artifact-row small, .muted-box { color: var(--text-muted); font-size: 12px; word-break: break-all; }
  .mini { padding: 5px 8px; font-size: 12px; justify-self: start; }
  .primary-emerald {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 9px 18px;
    border: none;
    border-radius: 999px;
    background: #059669;
    color: #fff;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    transition: background var(--motion-fast) ease, box-shadow var(--motion-fast) ease;
  }
  .primary-emerald:hover:not(:disabled) { background: #10b981; box-shadow: 0 0 20px rgba(16, 185, 129, 0.3); }
  .primary-emerald:disabled { opacity: 0.5; cursor: default; }
  .release-actions { display: flex; gap: 10px; flex-wrap: wrap; }
  .issues { display: grid; gap: 8px; }
  .issue { display: grid; gap: 8px; padding: 12px; border-radius: var(--border-radius-md); background: var(--bg-tertiary); border: 1px solid var(--border-color); }
  .issue.warning { border-color: rgba(245, 158, 11, 0.3); }
  .issue.error { border-color: rgba(239, 68, 68, 0.3); }
  .issue.ok { color: var(--accent-primary); display: flex; align-items: center; gap: 8px; }
  .github-preview { margin-top: 14px; padding: 14px; border: 1px solid color-mix(in srgb, var(--accent-secondary) 25%, transparent); border-radius: var(--border-radius-lg); background: color-mix(in srgb, var(--accent-secondary) 3%, transparent); }
  .github-preview h4 { color: var(--accent-secondary); margin: 0 0 8px; font-size: 14px; }
  .github-actions { display: flex; gap: 8px; align-items: center; margin-bottom: 10px; }
  .gh-meta { color: var(--text-muted); font-size: 12px; }
  .gh-body-preview { margin: 0; padding: 12px; border-radius: var(--border-radius-sm); background: #0d0d10; color: #d4d4d8; font-size: 12px; line-height: 1.5; max-height: 300px; overflow: auto; white-space: pre-wrap; font-family: var(--font-mono, ui-monospace, monospace); }

  .issue span { color: var(--text-muted); }
  code { color: var(--text-secondary); font-family: var(--font-mono, ui-monospace, monospace); }
  .changelog-panel { overflow: hidden; display: flex; flex-direction: column; min-height: 680px; }
    .changelog-header { justify-content: space-between; gap: 16px; padding-bottom: 14px; border-bottom: 1px solid rgba(255, 255, 255, 0.08); margin-bottom: 0; }
  /* (removed dead .empty rule — no element uses it) */
  :global(.spin) { animation: spin 900ms linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (max-width: 1100px) { .layout { grid-template-columns: 1fr; } }
</style>
