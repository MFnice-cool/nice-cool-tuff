<script lang="ts">
  import { Github, KeyRound, UploadCloud, Link2, Copy, Check, CircleCheck, CircleAlert } from "@lucide/svelte";
  import { onDestroy, onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { api } from "../lib/api";
  import { projectPath } from "../lib/store";
  import { copyText } from "../lib/clipboard";
  import { open } from "@tauri-apps/plugin-shell";

  let { repository = $bindable("") }: { repository?: string } = $props();

  let authed = $state(false);
  let busy = $state(false);
  let error = $state("");
  let message = $state("");
  let userCode = $state("");
  let verificationUri = $state("");
  let preview = $state<{
    packVersion: string;
    fileCount: number;
    hasExternalAssets: boolean;
  } | null>(null);
  let shareUrl = $state("");
  type PublishPhase = "" | "staging" | "commit" | "assets" | "done";

  let phase = $state<PublishPhase>("");
  let assetProgress = $state<{ index: number; total: number } | null>(null);
  let conflict = $state("");
  let pollTimer: ReturnType<typeof setTimeout> | null = null;
  let pollGeneration = 0;
  let polling = $state(false);
  let loginStarting = $state(false);
  let oauthClientMissing = $state(false);
  let twoPhase = $state<boolean | null>(null);
  let stopProgress: (() => void) | null = null;
  let destroyed = false;
  let attemptId = 0;
  let activePublishAttempt = 0;
  let scopedPath: string | null = "";
  let scopedRepository = repository.trim();

  onMount(() => {
    void listen<{ phase?: string; assetIndex?: number; assetTotal?: number }>("github-pack-progress", (event) => {
      if (!activePublishAttempt) return;
      const nextPhase = event.payload?.phase;
      if (nextPhase === "staging" || nextPhase === "commit" || nextPhase === "assets" || nextPhase === "done") {
        phase = nextPhase;
      }
      if (nextPhase === "assets" && typeof event.payload?.assetTotal === "number" && event.payload.assetTotal > 0) {
        assetProgress = { index: event.payload?.assetIndex ?? 0, total: event.payload.assetTotal };
      } else if (nextPhase !== "assets") {
        assetProgress = null;
      }
    }).then((unlisten) => {
      if (destroyed) {
        unlisten();
      } else {
        stopProgress = unlisten;
      }
    }).catch((cause) => {
      if (!destroyed) error = `GitHub publish progress is unavailable: ${String(cause)}`;
    });
  });

  onMount(() => projectPath.subscribe((path) => {
    if (path !== scopedPath) {
      scopedPath = path;
      clearScopedState();
    }
    void refreshAuth();
  }));

  $effect(() => {
    const nextRepository = repository.trim();
    if (nextRepository !== scopedRepository) {
      scopedRepository = nextRepository;
      clearScopedState();
    }
  });

  onDestroy(() => {
    destroyed = true;
    stopPolling();
    stopProgress?.();
    stopProgress = null;
    if (copyTimer) clearTimeout(copyTimer);
  });

  function clearScopedState() {
    activePublishAttempt = 0;
    preview = null;
    shareUrl = "";
    phase = "";
    assetProgress = null;
    twoPhase = null;
    conflict = "";
    message = "";
    error = "";
    // A device-login flow belongs to the previous project/repo scope: stop
    // polling and unblock the login button instead of leaving it disabled.
    if (polling || loginStarting) {
      stopPolling();
      loginStarting = false;
      userCode = "";
    }
  }

  function stopPolling() {
    pollGeneration += 1;
    polling = false;
    if (pollTimer) clearTimeout(pollTimer);
    pollTimer = null;
  }

  async function refreshAuth() {
    try {
      authed = await api.transport.github.authStatus();
    } catch {
      authed = false;
    }
  }

  async function startLogin() {
    if (loginStarting || polling) return;
    stopPolling();
    const generation = pollGeneration;
    loginStarting = true;
    error = "";
    message = "";
    oauthClientMissing = false;
    try {
      const info = await api.transport.github.startDeviceCode();
      if (generation !== pollGeneration) return;
      userCode = info.userCode;
      verificationUri = info.verificationUri;
      message = info.message;
      polling = true;
      const expiresAt = Date.now() + Math.max(0, info.expiresIn) * 1000;
      let intervalSeconds = Math.max(5, info.interval);

      const schedulePoll = () => {
        if (!polling || generation !== pollGeneration) return;
        const remainingMs = expiresAt - Date.now();
        if (remainingMs <= 0) {
          stopPolling();
          userCode = "";
          error = "GitHub device code expired. Start login again.";
          return;
        }
        pollTimer = setTimeout(poll, Math.min(intervalSeconds * 1000, remainingMs));
      };

      const poll = async () => {
        pollTimer = null;
        if (!polling || generation !== pollGeneration) return;
        if (Date.now() >= expiresAt) {
          stopPolling();
          userCode = "";
          error = "GitHub device code expired. Start login again.";
          return;
        }
        try {
          await api.transport.github.pollDeviceCode();
          if (generation !== pollGeneration) return;
          stopPolling();
          authed = true;
          userCode = "";
          message = "GitHub login saved on this device.";
        } catch (e) {
          if (generation !== pollGeneration) return;
          const text = String(e);
          const normalized = text.toLowerCase();
          if (normalized.includes("slow_down")) {
            intervalSeconds += 5;
            schedulePoll();
          } else if (normalized.includes("authorization_pending")) {
            // The current backend folds GitHub's slow_down into authorization_pending,
            // so conservatively back off while still honoring the initial interval.
            intervalSeconds += 5;
            schedulePoll();
          } else {
            stopPolling();
            userCode = "";
            if (normalized.includes("expired_token") || normalized.includes("expired")) {
              error = "GitHub device code expired. Start login again.";
            } else if (normalized.includes("access_denied") || normalized.includes("denied")) {
              error = "GitHub login was denied.";
            } else {
              error = text;
            }
          }
        }
      };

      schedulePoll();
    } catch (e) {
      if (generation !== pollGeneration) return;
      const text = String(e);
      oauthClientMissing = text.toLowerCase().includes("oauth client id") && text.toLowerCase().includes("not configured");
      error = text;
    } finally {
      if (generation === pollGeneration) loginStarting = false;
    }
  }

  async function runPreview() {
    if (!$projectPath) return;
    const targetPath = $projectPath;
    const attempt = ++attemptId;
    busy = true;
    activePublishAttempt = 0;
    preview = null;
    shareUrl = "";
    phase = "";
    assetProgress = null;
    twoPhase = null;
    conflict = "";
    message = "";
    error = "";
    try {
      const result = await api.transport.github.stagePreview(targetPath);
      if (attempt !== attemptId || targetPath !== $projectPath) return;
      preview = {
        packVersion: result.packVersion,
        fileCount: result.fileCount,
        hasExternalAssets: result.hasExternalAssets,
      };
      message = `Staged ${result.fileCount} files (v${result.packVersion}).`;
    } catch (e) {
      if (attempt !== attemptId || targetPath !== $projectPath) return;
      error = String(e);
    } finally {
      if (attempt === attemptId) busy = false;
    }
  }

  async function publishPack() {
    if (!$projectPath || !repository.trim()) return;
    const targetPath = $projectPath;
    const targetRepository = repository.trim();
    const attempt = ++attemptId;
    const hasExternalAssets = preview?.hasExternalAssets ?? null;
    busy = true;
    activePublishAttempt = attempt;
    preview = null;
    shareUrl = "";
    error = "";
    message = "";
    conflict = "";
    assetProgress = null;
    phase = "staging";
    twoPhase = hasExternalAssets;
    try {
      const result = await api.transport.github.publish(targetRepository, targetPath);
      if (attempt !== attemptId || targetPath !== $projectPath || targetRepository !== repository.trim()) return;
      if (result.conflict) {
        conflict = String(result.message ?? "Branch moved. Pull or resolve the GitHub repo, then publish again. TuffBox never force-pushes.");
        phase = "";
        return;
      }
      shareUrl = String(result.shareUrl ?? "");
      twoPhase = typeof result.twoPhase === "boolean" ? result.twoPhase : twoPhase;
      phase = "done";
      message = result.noop
        ? String(result.message ?? "Already up to date.")
        : `Published ${shareUrl || "to GitHub"}.`;
    } catch (e) {
      if (attempt !== attemptId || targetPath !== $projectPath || targetRepository !== repository.trim()) return;
      error = String(e);
      phase = "";
    } finally {
      if (activePublishAttempt === attempt) activePublishAttempt = 0;
      if (attempt === attemptId) busy = false;
    }
  }

  async function openShare() {
    if (!shareUrl) return;
    await open(shareUrl);
  }

  // Ready-to-paste message for messengers: browsable https link first (works
  // for everyone), one-click tuffbox:// deep link second (for installed apps).
  let shareMessage = $derived.by(() => {
    if (!shareUrl) return "";
    const repoPath = shareUrl.replace(/^https:\/\/github\.com\//, "").replace(/\/+$/, "");
    const meta = preview ? ` v${preview.packVersion}` : "";
    const lines = [
      `Minecraft pack${meta} — ${repoPath}`,
      shareUrl,
      `Install in TuffBox: tuffbox://install?repo=${encodeURIComponent(repoPath)}`,
    ];
    return lines.join("\n");
  });

  let copied = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | null = null;

  async function copyShare() {
    if (!shareUrl) return;
    try {
      await copyText(shareMessage || shareUrl);
      copied = true;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {
        copied = false;
      }, 1600);
    } catch {
      message = "Copy failed — select and copy the share URL manually.";
    }
  }

  function phaseLabel(value: string): string {
    if (value === "staging") return "Staging…";
    if (value === "commit") return "Publishing staged Git tree…";
    if (value === "assets") {
      return assetProgress
        ? `Uploading Release assets (${assetProgress.index}/${assetProgress.total})…`
        : "Uploading Release assets…";
    }
    if (value === "done") return "Publish complete";
    return "Publishing…";
  }

  function phaseRank(value: PublishPhase): number {
    return ["", "staging", "commit", "assets", "done"].indexOf(value);
  }

  let publishSteps = $derived(
    twoPhase === false ? ["staging", "commit", "done"] : ["staging", "commit", "assets", "done"],
  );
  let repoInvalid = $derived(repository.trim().length > 0 && repository.trim().split("/").filter(Boolean).length !== 2);
</script>

<section class="rounded-xl border border-[var(--border-color)] bg-[var(--bg-tertiary)] p-4 grid gap-3">
  <h3 class="flex items-center gap-2 m-0 text-sm font-bold text-[var(--text-secondary)]">
    <Github size={16} /> GitHub Pack Transport
  </h3>
  <p class="m-0 text-xs leading-relaxed text-[var(--text-muted)]">
    Publishes the canonical TuffBox manifest plus a packwiz tree as one Git commit.
    Friends install anonymously from the public repo URL — no Microsoft or Modrinth account.
  </p>

  <div class="flex items-center gap-2 flex-wrap">
    <span
      class="inline-flex items-center gap-2 text-xs font-semibold"
      class:text-[var(--accent-primary)]={authed}
      class:text-[var(--text-muted)]={!authed}
    >
      {#if authed}<CircleCheck size={13} />{:else}<CircleAlert size={13} />{/if}
      {authed ? "Author signed in" : "Author login required"}
    </span>
    <button
      class="ml-auto inline-flex items-center gap-2 h-7 px-2.5 rounded-lg border border-[var(--border-color)] bg-transparent text-xs font-bold text-[var(--text-secondary)] cursor-pointer transition-colors duration-[160ms] hover:border-[var(--accent-primary)] hover:text-[var(--text-primary)] disabled:opacity-45 disabled:cursor-not-allowed disabled:pointer-events-none"
      onclick={startLogin}
      disabled={busy || loginStarting || polling}
    >
      <KeyRound size={12} /> {loginStarting ? "Starting GitHub login…" : polling ? "Waiting for GitHub…" : authed ? "Re-login" : "GitHub device login"}
    </button>
  </div>

  {#if oauthClientMissing}
    <p class="m-0 p-2 rounded-lg border border-[rgba(239,68,68,0.35)] text-xs text-[var(--accent-danger)]">
      Device OAuth is unavailable: this build has no GitHub OAuth client ID. Set
      <code class="font-mono text-[12px] text-[var(--text-secondary)]">TUFFBOX_GITHUB_CLIENT_ID</code> when building, or add a GitHub PAT in Settings.
    </p>
  {/if}

  {#if userCode}
    <p class="m-0 text-xs text-[var(--text-muted)] flex items-center gap-2 flex-wrap">
      Open
      <button
        class="bg-transparent border-0 p-0 text-[var(--accent-primary)] text-xs font-semibold cursor-pointer underline underline-offset-2"
        onclick={() => open(verificationUri)}
      >{verificationUri}</button>
      and enter
      <code class="px-1.5 py-0.5 rounded bg-[var(--bg-secondary)] border border-[var(--border-color)] font-mono text-xs tracking-widest text-[var(--text-primary)]">{userCode}</code>
    </p>
  {/if}

  <label class="grid gap-2 text-[13px] font-bold text-[var(--text-secondary)]">
    Public repository
    <input
      bind:value={repository}
      placeholder="owner/repository"
      aria-invalid={repoInvalid}
      class="h-8 px-2.5 rounded-lg border bg-[var(--bg-secondary)] text-[13px] text-[var(--text-primary)] outline-none transition-colors duration-[160ms] placeholder:text-[var(--text-muted)] focus:border-[var(--accent-primary)]"
      class:border-[var(--border-color)]={!repoInvalid}
      class:border-[rgba(239,68,68,0.55)]={repoInvalid}
    />
    {#if repoInvalid}
      <span class="text-[12px] font-normal text-[var(--accent-danger)]">Use the owner/repository format, e.g. acme/cool-pack.</span>
    {/if}
  </label>

  <div class="flex items-center gap-2 flex-wrap">
    <button
      class="inline-flex items-center gap-2 h-7 px-2.5 rounded-lg border border-[var(--border-color)] bg-transparent text-xs font-bold text-[var(--text-secondary)] cursor-pointer transition-colors duration-[160ms] hover:border-[var(--accent-primary)] hover:text-[var(--text-primary)] disabled:opacity-45 disabled:cursor-not-allowed disabled:pointer-events-none"
      onclick={runPreview}
      disabled={busy || !$projectPath}
    >Preview tree</button>
    <button
      class="inline-flex items-center gap-2 h-7 px-2.5 rounded-lg border-0 bg-[var(--accent-primary)] text-xs font-bold text-[#06251a] cursor-pointer transition-colors duration-[160ms] hover:bg-[var(--accent-hover)] disabled:opacity-45 disabled:cursor-not-allowed disabled:pointer-events-none"
      onclick={publishPack}
      disabled={busy || !authed || !repository.trim() || repoInvalid}
    >
      <UploadCloud size={12} /> {busy ? phaseLabel(phase) : "Publish pack"}
    </button>
    {#if shareUrl}
      <button
        class="inline-flex items-center gap-2 h-7 px-2.5 rounded-lg border border-[var(--border-color)] bg-transparent text-xs text-[var(--text-muted)] cursor-pointer transition-colors duration-[160ms] hover:text-[var(--text-primary)]"
        onclick={copyShare}
        title="Copy share message (link + one-click install)"
      >
        {#if copied}
          <Check size={12} class="text-[var(--accent-primary)]" /> Copied!
        {:else}
          <Copy size={12} /> Copy share message
        {/if}
      </button>
      <button
        class="inline-flex items-center gap-2 h-7 px-2.5 rounded-lg border border-[var(--border-color)] bg-transparent text-xs text-[var(--text-muted)] cursor-pointer transition-colors duration-[160ms] hover:text-[var(--text-primary)]"
        onclick={openShare}
      ><Link2 size={12} /> Open share link</button>
    {/if}
  </div>

  {#if phase}
    <div class="grid gap-2" aria-live="polite">
      <div class="flex items-baseline gap-2 flex-wrap text-xs">
        <strong class="text-[var(--text-primary)]">{phaseLabel(phase)}</strong>
        {#if twoPhase === true}
          <span class="text-[var(--text-muted)]">Two-phase publish: Git commit first, then Release assets, then ready.</span>
        {:else if twoPhase === false}
          <span class="text-[var(--text-muted)]">Single-phase Git commit publish.</span>
        {/if}
      </div>
      <ol
        class="grid gap-2 m-0 p-0 list-none"
        style={`grid-template-columns: repeat(${publishSteps.length}, minmax(0, 1fr));`}
        aria-label="Publish progress"
      >
        {#each publishSteps as step (step)}
          {@const active = phase === step}
          {@const complete = phaseRank(phase) > phaseRank(step as PublishPhase)}
          <li
            class="px-1.5 py-1 rounded border text-[12px] text-center transition-colors duration-[160ms]"
            class:border-[var(--accent-primary)]={active || complete}
            class:text-[var(--text-primary)]={active}
            class:text-[var(--accent-primary)]={complete && !active}
            class:text-[var(--text-muted)]={!active && !complete}
            class:bg-[color-mix(in_srgb,var(--accent-primary)_10%,transparent)]={active || complete}
            class:border-[var(--border-color)]={!active && !complete}
          >
            {phaseLabel(step)}
          </li>
        {/each}
      </ol>
      {#if phase === "assets" && assetProgress}
        <div class="h-1 rounded-full bg-[var(--bg-secondary)] overflow-hidden">
          <div
            class="h-full bg-[var(--accent-primary)] rounded-full transition-[width] duration-[240ms]"
            style={`width: ${Math.round((assetProgress.index / Math.max(1, assetProgress.total)) * 100)}%;`}
          ></div>
        </div>
      {/if}
    </div>
  {/if}

  {#if preview}
    <small class="text-[12px] text-[var(--text-muted)]">{preview.fileCount} files · v{preview.packVersion}{preview.hasExternalAssets ? " · large jars via Release assets" : ""}</small>
  {/if}
  {#if message}<p class="m-0 text-xs text-[var(--accent-primary)]">{message}</p>{/if}
  {#if conflict}<p class="m-0 text-xs text-[var(--accent-danger)]">Conflict: {conflict}</p>{/if}
  {#if error}<p class="m-0 text-xs text-[var(--accent-danger)] break-words">{error}</p>{/if}
</section>
