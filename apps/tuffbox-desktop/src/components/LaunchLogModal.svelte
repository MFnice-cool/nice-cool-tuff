<script lang="ts">
  import { X, Loader2, RotateCcw, FileText, Folder, Radio, Share2 } from "@lucide/svelte";
  import { onMount, onDestroy, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { quintOut } from "svelte/easing";
  import { invoke } from "@tauri-apps/api/core";
  import { trapFocus } from "../lib/focusTrap";
  import CopyButton from "./CopyButton.svelte";
  import { shareCrashLogWithFeedback } from "../lib/mclogs";

  let {
    projectPath,
    title = null,
    onclose,
  }: {
    projectPath: string;
    title?: string | null;
    onclose?: () => void;
  } = $props();

  let log = $state("");
  let loading = $state(true);
  let interval: ReturnType<typeof setInterval> | null = null;
  let logFiles = $state<{ name: string; size: number; modified?: number | null }[]>([]);
  /** `__live__` = auto-pick console/latest for the running session */
  let selectedLog = $state("__live__");
  let logListOpen = $state(false);
  let followTail = $state(true);
  let logPreEl = $state<HTMLPreElement | null>(null);
  let userScrolledUp = $state(false);
  let lastFetchedLen = $state(-1);
  let loadGen = 0;
  let analyzeTimer: ReturnType<typeof setTimeout> | null = null;
  let sharing = $state(false);
  /** Actual target of the Live tab, resolved by the same backend helper as get_launch_log. */
  let liveSourcePath = $state<string | null>(null);

  type SuspectSummary = { id: string; name: string; confidence: number };
  let suspects = $state<SuspectSummary[]>([]);
  let suspectCount = $state(0);

  function isLiveTab() {
    return selectedLog === "__live__";
  }

  function displayLiveSource(path: string): string {
    const separator = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
    return separator >= 0 ? path.slice(separator + 1) : path;
  }

  async function analyzeLog(text: string) {
    if (!text || text.length > 400_000) {
      suspects = [];
      suspectCount = 0;
      return;
    }
    // Cap analysis to the last chunk so huge latest.log stays responsive.
    const sample = text.length > 120_000 ? text.slice(text.length - 120_000) : text;
    try {
      const res = (await invoke("analyze_log_text", {
        path: projectPath,
        text: sample,
      })) as {
        suspectedMods: { id: string; name: string; confidence: number }[];
      };
      suspects = (res.suspectedMods ?? []).slice(0, 12);
      suspectCount = res.suspectedMods?.length ?? 0;
    } catch {
      suspects = [];
      suspectCount = 0;
    }
  }

  function scheduleAnalyze(text: string) {
    if (analyzeTimer) clearTimeout(analyzeTimer);
    analyzeTimer = setTimeout(() => {
      analyzeTimer = null;
      void analyzeLog(text);
    }, isLiveTab() ? 2500 : 400);
  }

  async function scrollToBottomIfFollowing() {
    if (!followTail || userScrolledUp || !logPreEl) return;
    await tick();
    logPreEl.scrollTop = logPreEl.scrollHeight;
  }

  function onLogScroll() {
    if (!logPreEl) return;
    const dist = logPreEl.scrollHeight - logPreEl.scrollTop - logPreEl.clientHeight;
    userScrolledUp = dist > 48;
    if (!userScrolledUp) followTail = true;
  }

  async function loadLog(opts: { forceAnalyze?: boolean } = {}) {
    const gen = ++loadGen;
    try {
      let result: string;
      let resolvedLiveSource: string | null = null;
      if (selectedLog === "__live__") {
        // Resolve the label via the exact backend fallback helper, but keep
        // tailing functional with an older binary that does not expose the
        // optional source-path command yet.
        const sourceRequest = invoke<string>("resolve_live_launch_log_path", { path: projectPath })
          .catch(() => null);
        result = await invoke<string>("get_launch_log", { path: projectPath });
        resolvedLiveSource = await sourceRequest;
      } else {
        // Always read the named file — never route latest.log through get_launch_log
        // (that prefers the XML console and showed raw log4j/timestamp markup).
        result = (await invoke("read_instance_log", {
          path: projectPath,
          logName: selectedLog,
        })) as string;
      }
      if (gen !== loadGen) return;
      liveSourcePath = resolvedLiveSource;

      const next = result ?? "";
      if (next.length === lastFetchedLen && next === log) {
        return;
      }
      const changed = next !== log;
      log = next;
      lastFetchedLen = next.length;

      if (changed || opts.forceAnalyze) {
        if (isLiveTab()) {
          scheduleAnalyze(next);
        } else {
          await analyzeLog(next);
        }
      }
      await scrollToBottomIfFollowing();
    } catch (e) {
      if (gen !== loadGen) return;
      log = (log ? log + "\n" : "") + `[error] ${e}`;
    } finally {
      if (gen === loadGen) loading = false;
    }
  }

  async function loadLogList() {
    try {
      logFiles = await invoke("list_instance_logs", { path: projectPath });
    } catch {
      logFiles = [];
    }
  }

  async function shareCurrent() {
    if (sharing || !projectPath) return;
    sharing = true;
    try {
      // Live must share the same file the Live tab tails (latest.log / console).
      // Passing null used to auto-pick the newest crash-report instead.
      const name = selectedLog === "__live__" ? "__live__" : selectedLog;
      await shareCrashLogWithFeedback(projectPath, name);
    } finally {
      sharing = false;
    }
  }

  function syncPolling() {
    if (interval) {
      clearInterval(interval);
      interval = null;
    }
    // Only live-tail polls — static files (latest.log, archives) load once.
    if (isLiveTab()) {
      interval = setInterval(() => void loadLog(), 1000);
    }
  }

  async function switchLog(name: string) {
    if (selectedLog === name && log) return;
    selectedLog = name;
    loading = true;
    lastFetchedLen = -1;
    userScrolledUp = false;
    followTail = true;
    suspects = [];
    suspectCount = 0;
    syncPolling();
    await loadLog({ forceAnalyze: true });
  }

  onMount(() => {
    void loadLog({ forceAnalyze: true });
    void loadLogList();
    syncPolling();
  });

  onDestroy(() => {
    if (interval) clearInterval(interval);
    if (analyzeTimer) clearTimeout(analyzeTimer);
  });
</script>

<div
  class="modal-backdrop"
  onclick={(e) => e.target === e.currentTarget && onclose?.()}
  role="button"
  tabindex="-1"
  aria-label="Close"
  onkeydown={() => {}}
>
  <div
    class="modal"
    role="dialog"
    aria-modal="true"
    use:trapFocus={{ onEscape: () => onclose?.() }}
  >
    <div class="modal-header">
      <div class="modal-header-left">
        <h2>
          <Radio size={16} class="live-icon" />
          {title || "Live logs"}
        </h2>
        <div class="log-selector">
          <button
            class="log-select-btn"
            class:active={selectedLog === "__live__"}
            onclick={() => switchLog("__live__")}
            title={liveSourcePath ? `Live source: ${liveSourcePath}` : "Auto: latest.log when ready, else console"}
          >
            <Radio size={13} /> Live{#if liveSourcePath} · {displayLiveSource(liveSourcePath)}{/if}
          </button>
          <button
            class="log-select-btn"
            class:active={selectedLog === "latest.log"}
            onclick={() => switchLog("latest.log")}
          >
            <FileText size={13} /> latest.log
          </button>
          <button
            class="log-select-btn"
            class:active={selectedLog === "tuffbox-console.log"}
            onclick={() => switchLog("tuffbox-console.log")}
          >
            <FileText size={13} /> console
          </button>
          {#if logFiles.length > 0}
            <button
              class="log-select-btn toggle"
              onclick={() => {
                logListOpen = !logListOpen;
                if (logListOpen) loadLogList();
              }}
            >
              <Folder size={13} /> {logFiles.length} logs
            </button>
          {/if}
        </div>
        {#if logListOpen}
          <div class="log-dropdown">
            {#each logFiles as f (f.name)}
              <button
                class="log-file-row"
                class:selected={selectedLog === f.name}
                onclick={() => {
                  switchLog(f.name);
                  logListOpen = false;
                }}
              >
                <span>{f.name}</span>
                <small
                  >{f.size < 1024
                    ? f.size + " B"
                    : f.size < 1048576
                      ? (f.size / 1024).toFixed(1) + " KB"
                      : (f.size / 1048576).toFixed(1) + " MB"}</small
                >
              </button>
            {/each}
          </div>
        {/if}
      </div>
      <div class="modal-header-right">
        <label class="follow-toggle" title="Keep scrolling to the newest lines">
          <input
            type="checkbox"
            bind:checked={followTail}
            onchange={() => {
              if (followTail) {
                userScrolledUp = false;
                void scrollToBottomIfFollowing();
              }
            }}
          />
          Follow
        </label>
        {#if log}
          <CopyButton text={log} label="Copy log" />
        {/if}
        <button class="icon-btn" onclick={() => onclose?.()} aria-label="Close">
          <X size={18} />
        </button>
      </div>
    </div>

    <div class="modal-body">
      {#if loading && !log}
        <div class="loader" transition:fade={{ duration: 160 }}>
          <Loader2 size={20} class="spin" />
          Waiting for process output…
        </div>
      {/if}
      {#if suspectCount > 0}
        <div class="suspect-banner" transition:fly={{ y: -8, duration: 220, opacity: 0, easing: quintOut }}>
          <strong>{suspectCount}</strong> mod{suspectCount === 1 ? "" : "s"} referenced
          {#if suspects.length}
            — {suspects.map((s) => s.name).join(", ")}{suspectCount > suspects.length ? "…" : ""}
          {/if}
        </div>
      {/if}
      {#key selectedLog}
        <div class="log-pane" in:fly={{ x: 16, duration: 280, opacity: 0, easing: quintOut }}>
          {#if log}
            <!-- Single text node: per-line Svelte each-blocks made latest.log lag hard. -->
            <pre class="log" bind:this={logPreEl} onscroll={onLogScroll}>{log}</pre>
          {:else}
            <pre class="log">Waiting for process output…</pre>
          {/if}
        </div>
      {/key}
    </div>

    <div class="modal-footer">
      <span class="live-hint"
        >{selectedLog === "__live__"
          ? "Live tail · refreshes ~1s · XML console auto-formatted"
          : `Showing ${selectedLog} (loaded once)`}</span
      >
      <button class="ghost" onclick={shareCurrent} disabled={!log || sharing} title={selectedLog === "__live__" ? "Upload the Live log (latest.log when available)" : `Upload ${selectedLog} to mclo.gs`}>
        <Share2 size={16} />
        {sharing ? "Sharing…" : "Share"}
      </button>
      <button class="ghost" onclick={() => loadLog({ forceAnalyze: true })}>
        <RotateCcw size={16} />
        Refresh
      </button>
      <button class="ghost" onclick={() => onclose?.()}>Close</button>
    </div>
  </div>
</div>

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
    padding: 24px;
  }

  .modal {
    width: min(960px, 100%);
    max-height: min(85vh, 820px);
    background: var(--bg-elevated, #1a1f28);
        border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
        border-radius: var(--border-radius-md);
        display: flex;
        flex-direction: column;
        box-shadow: 0 24px 64px rgba(0, 0, 0, 0.45);
      }

      .modal-header {
        display: flex;
        justify-content: space-between;
        gap: 12px;
        padding: 14px 16px;
        border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.06));
    position: relative;
  }

  .modal-header-left {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .modal-header-left h2 {
    margin: 0;
    font-size: 16px;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  :global(.live-icon) {
    color: var(--accent-primary);
  }

  .modal-header-right {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .follow-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted, #9aa4b2);
    cursor: pointer;
    user-select: none;
  }

  .log-selector {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .log-dropdown {
    position: absolute;
    top: 100%;
    left: 16px;
    z-index: 5;
    max-height: 220px;
    overflow: auto;
    background: var(--bg-elevated, #1a1f28);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: var(--border-radius-sm);
    min-width: 260px;
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.4);
  }

  .log-file-row {
    display: flex;
    justify-content: space-between;
    width: 100%;
    padding: 8px 12px;
    border: none;
    background: transparent;
    color: inherit;
    cursor: pointer;
    font-size: 12px;
  }

  .log-file-row:hover,
  .log-file-row.selected {
    background: color-mix(in srgb, var(--text-primary) 6%, transparent);
  }

  .icon-btn {
    border: none;
    background: transparent;
    color: var(--text-muted, #9aa4b2);
    cursor: pointer;
    padding: 4px;
    border-radius: 6px;
  }

  .icon-btn:hover {
      background: var(--bg-hover, rgba(255, 255, 255, 0.06));
      color: var(--text-primary, #fff);
    }

  .modal-body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 0;
    overflow: hidden;
  }

  .log-pane {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .log-select-btn {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 4px 10px;
      border-radius: 6px;
      border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    background: transparent;
    color: var(--text-muted, #9aa4b2);
    font-size: 12px;
    cursor: pointer;
    transition:
      background var(--motion-fast, 160ms) var(--ease-out, ease),
      border-color var(--motion-fast, 160ms) var(--ease-out, ease),
      color var(--motion-fast, 160ms) var(--ease-out, ease),
      transform var(--motion-fast, 160ms) var(--ease-spring, ease);
  }

  .log-select-btn:hover {
    transform: translateY(-1px);
  }

  .log-select-btn.active {
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent);
    color: #fff;
    transform: scale(1.03);
  }

  .loader {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 16px;
    color: var(--text-muted, #9aa4b2);
    font-size: 13px;
  }

  :global(.spin) {
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .suspect-banner {
    padding: 8px 14px;
    font-size: 12px;
    background: rgba(255, 180, 60, 0.1);
    border-bottom: 1px solid rgba(255, 180, 60, 0.2);
  }

  .log {
    flex: 1;
    margin: 0;
    padding: 12px 14px;
    overflow: auto;
    font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
    font-size: 12px;
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-word;
    color: #d5dbe6;
    background: #0e1218;
    min-height: 360px;
  }

  .modal-footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    border-top: 1px solid rgba(255, 255, 255, 0.06);
  }

  .live-hint {
    flex: 1;
    font-size: 12px;
    color: var(--text-muted, #9aa4b2);
  }

  .ghost {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.1);
    background: transparent;
    color: inherit;
    cursor: pointer;
    font-size: 13px;
  }

  .ghost:hover {
    background: color-mix(in srgb, var(--text-primary) 5%, transparent);
  }
</style>
