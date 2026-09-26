<script lang="ts">
  import { Loader2, CheckCircle2, AlertCircle, PackageOpen } from "@lucide/svelte";
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";

  /**
   * Install progress modal for GitHub pack imports. Shows the current phase
   * plus per-mod download progress from mod-download-batch / mod-download-progress.
   * Stays open until `active` flips false or the user dismisses a failed install.
   */
  let {
    active = false,
    onclose = () => {},
  }: {
    active?: boolean;
    onclose?: () => void;
  } = $props();

  type DownloadItem = {
    id: string;
    name: string;
    downloaded: number;
    total: number;
    percent: number;
    status: "queued" | "downloading" | "done" | "failed" | "skipped" | string;
    error?: string | null;
  };

  type DownloadBatch = {
    phase: string;
    items?: DownloadItem[];
    downloaded?: string[];
    failed?: { modId: string; error: string }[];
    alreadyPresent?: string[];
    skipped?: string[];
    scopeModIds?: string[];
    batchComplete?: boolean;
  };

  let phase = $state("");
  let message = $state("");
  let items = $state<DownloadItem[]>([]);
  let done = $state(false);
  let failed = $state(false);
  let unlistenPack: UnlistenFn | null = null;
  let unlistenBatch: UnlistenFn | null = null;
  let unlistenProgress: UnlistenFn | null = null;

  const doneCount = $derived(items.filter((i) => i.status === "done" || i.status === "skipped").length);
  const failedCount = $derived(items.filter((i) => i.status === "failed").length);
  const overallPercent = $derived.by(() => {
    if (items.length === 0) return phase ? 35 : 0;
    const sum = items.reduce((acc, i) => acc + (i.percent || 0), 0);
    return Math.round(sum / items.length);
  });

  function phaseLabel(): string {
    const p = phase;
    if (p === "resolving") return "Resolving pack…";
    if (p === "download") return "Downloading files…";
    if (p === "done") return "Finishing install…";
    if (p === "error") return "Install failed";
    return p ? p.replace(/_/g, " ") : "Preparing…";
  }

  function stateLabel(item: DownloadItem): string {
    if (item.status === "downloading") {
      return item.total > 0 ? Math.round((item.downloaded / item.total) * 100) + "%" : "…";
    }
    if (item.status === "done") return "✓";
    if (item.status === "skipped") return "skipped";
    if (item.status === "failed") return "failed";
    return "";
  }

  function reset() {
    phase = "";
    message = "";
    items = [];
    done = false;
    failed = false;
  }

  onMount(() => {
    void listen<{ phase?: string; message?: string }>("modpack-install-progress", (event) => {
      if (!active) return;
      phase = event.payload?.phase ?? "";
      message = event.payload?.message ?? "";
      if (event.payload?.phase === "error") {
        failed = true;
      }
    }).then((u) => { unlistenPack = u; }).catch(() => {});

    void listen<DownloadBatch>("mod-download-batch", (event) => {
      if (!active) return;
      const payload = event.payload;
      if (payload.phase === "start") {
        items = (payload.items ?? []).map((item) => ({
          id: item.id,
          name: item.name,
          downloaded: 0,
          total: 0,
          percent: 0,
          status: "queued",
        }));
        phase = "download";
        message = "Downloading pack content…";
      } else if (payload.phase === "done") {
        const downloadedIds = new Set(payload.downloaded ?? []);
        const alreadyPresentIds = new Set(payload.alreadyPresent ?? []);
        const skippedIds = new Set(payload.skipped ?? []);
        const failedIds = new Set((payload.failed ?? []).map((f) => f.modId));
        const failureById = new Map((payload.failed ?? []).map((f) => [f.modId, f.error]));
        items = items.map((item) => {
          if (skippedIds.has(item.id)) return { ...item, status: "skipped", percent: 100 };
          if (downloadedIds.has(item.id) || alreadyPresentIds.has(item.id)) return { ...item, status: "done", percent: 100 };
          if (failedIds.has(item.id)) return { ...item, status: "failed", percent: 100, error: failureById.get(item.id) ?? "download failed" };
          return item;
        });
        if (failedIds.size > 0) {
          failed = true;
          phase = "error";
          message = "Some files failed to download.";
        } else {
          done = true;
          phase = "done";
          message = "Pack content ready.";
        }
      }
    }).then((u) => { unlistenBatch = u; }).catch(() => {});

    void listen<DownloadItem>("mod-download-progress", (event) => {
      if (!active) return;
      const item = event.payload;
      items = items.map((i) => (i.id === item.id ? { ...i, ...item } : i));
    }).then((u) => { unlistenProgress = u; }).catch(() => {});
  });

  $effect(() => {
    if (active) reset();
  });

  onDestroy(() => {
    unlistenPack?.();
    unlistenBatch?.();
    unlistenProgress?.();
  });
</script>

{#if active}
  <div class="pip-backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && failed && onclose()}>
    <div class="pip-card" role="status" aria-live="polite">
      <header>
        <span class="pip-icon"><PackageOpen size={18} /></span>
        <div class="pip-title">
          <strong>Installing GitHub pack</strong>
          <span>{message || "Preparing…"}</span>
        </div>
        {#if failed}
          <button class="pip-close" aria-label="Close" onclick={onclose}><AlertCircle size={16} /></button>
        {:else if done}
          <span class="pip-ok"><CheckCircle2 size={18} /></span>
        {:else}
          <span class="pip-spin"><Loader2 size={18} /></span>
        {/if}
      </header>

      {#if phase}
        <div class="pip-bar" aria-hidden="true">
          <div class="pip-fill" style="width: {overallPercent}%;"></div>
        </div>
        <div class="pip-meta">
          <span class="pip-phase">{phaseLabel()}</span>
          {#if items.length > 0}
            <span class="pip-count">{doneCount + failedCount}/{items.length} files</span>
          {/if}
        </div>
      {/if}

      {#if items.length > 0}
        <ul class="pip-list">
          {#each items.slice(0, 5) as item (item.id)}
            <li>
              <span class="pip-name">{item.name}</span>
              <span class="pip-state">{stateLabel(item)}</span>
            </li>
          {/each}
          {#if items.length > 5}
            <li class="pip-more">… {items.length - 5} more</li>
          {/if}
        </ul>
      {/if}

      {#if failed}
        <p class="pip-fail">
          {failedCount} file{failedCount === 1 ? "" : "s"} failed to download. Close this dialog, fix the network or source, then import again.
        </p>
      {/if}
    </div>
  </div>
{/if}

<style>
  .pip-backdrop {
    position: fixed;
    inset: 0;
    background: color-mix(in srgb, var(--bg-primary) 55%, transparent);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 190;
    backdrop-filter: blur(6px);
  }

  .pip-card {
    width: min(460px, 92vw);
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
    box-shadow: var(--shadow-lg);
    padding: 16px 18px;
    display: grid;
    gap: 10px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .pip-icon,
  .pip-ok,
  .pip-spin {
    display: inline-flex;
    color: var(--accent-primary);
    flex-shrink: 0;
  }
  .pip-spin { animation: pip-rotate 0.9s linear infinite; }
  .pip-close { background: none; border: none; color: var(--accent-danger); cursor: pointer; margin-left: auto; padding: 2px; }

  @keyframes pip-rotate {
    to { transform: rotate(360deg); }
  }

  .pip-title { display: grid; gap: 8px; min-width: 0; }
  .pip-title strong { font-size: 14px; color: var(--text-primary); }
  .pip-title span { font-size: 12px; color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .pip-bar {
    height: 6px;
    border-radius: 999px;
    background: var(--bg-elevated);
    overflow: hidden;
  }

  .pip-fill {
    height: 100%;
    background: var(--accent-primary);
    border-radius: 999px;
    transition: width 160ms ease;
  }

  .pip-meta {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    color: var(--text-muted);
  }

  .pip-phase { text-transform: capitalize; }

  .pip-list {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 8px;
    max-height: 140px;
    overflow-y: auto;
  }

  .pip-list li {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .pip-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .pip-state { flex-shrink: 0; color: var(--text-muted); }
  .pip-more { color: var(--text-muted); font-size: 12px; }
  .pip-fail { margin: 0; color: var(--accent-danger); font-size: 12px; }
</style>
