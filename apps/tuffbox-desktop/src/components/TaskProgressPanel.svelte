<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onDestroy, onMount } from "svelte";
  import { X, Loader2, CheckCircle2, AlertTriangle, Pause, Play } from "@lucide/svelte";
  import { toasts } from "../lib/toast";
  import { taskPollIntervalMs } from "../lib/idlePerf";

  type BackgroundTask = {
    id: string;
    title: string;
    status:
      | "running"
      | "paused"
      | "cancelRequested"
      | "cancelled"
      | "succeeded"
      | "failed"
      | "dismissed";
    progress?: number | null;
    detail?: string | null;
    error?: string | null;
  };

  let tasks = $state<BackgroundTask[]>([]);
  let unlistenPullDone: UnlistenFn | null = null;

  async function refresh() {
    try {
      const next = await invoke<BackgroundTask[]>("list_background_tasks");
      // Skip the reassign when nothing changed — an identical array would
      // still re-run the each-block diff every tick.
      if (
        tasks.length === next.length &&
        JSON.stringify(tasks) === JSON.stringify(next)
      ) {
        return;
      }
      tasks = next;
    } catch {
      tasks = [];
    }
  }

  async function dismiss(id: string) {
    try {
      await invoke("dismiss_background_task", { id });
      await refresh();
    } catch {
      /* ignore */
    }
  }

  async function cancelTask(id: string) {
    try {
      await invoke("cancel_background_task", { id });
      toasts.info("Cancelling…", 2500);
      await refresh();
    } catch (e) {
      toasts.error(String(e));
    }
  }

  function isOllamaPull(id: string) {
    return id.startsWith("ollama-pull-");
  }

  function ollamaModelFromTaskId(id: string) {
    return id.slice("ollama-pull-".length);
  }

  async function pauseOllamaPull() {
    try {
      await invoke("pause_ollama_model_pull");
      toasts.info("Pausing model download…", 3000);
    } catch (e) {
      toasts.error(String(e));
    }
  }

  async function resumeOllamaPull(id: string) {
    const model = ollamaModelFromTaskId(id);
    if (!model) return;
    try {
      await invoke("pull_ollama_model", {
        model,
        endpoint: null,
        binaryPath: null,
        modelsPath: null,
      });
      toasts.info(`Resumed ${model} in background`, 4000);
      await refresh();
    } catch (e) {
      toasts.error(String(e));
    }
  }

  onMount(() => {
    void refresh();
    void listen<{
      ok: boolean;
      paused?: boolean;
      model: string;
      error?: string;
    }>("ollama-pull-finished", (ev) => {
      const p = ev.payload;
      if (p.paused) {
        toasts.info(`Paused ${p.model} — resume from the task panel or AI settings`, 6000);
      } else if (p.ok) {
        toasts.success(`AI model installed: ${p.model}`, 6000);
      } else if (p.error) {
        toasts.error(`AI model download failed: ${p.error}`, 10000);
      }
      void refresh();
    }).then((u) => {
      unlistenPullDone = u;
    });
  });

  onDestroy(() => {
    void unlistenPullDone?.();
  });

  const visible = $derived(
    tasks.filter(
      (t) =>
        t.status === "running" ||
        t.status === "failed" ||
        t.status === "paused" ||
        t.status === "cancelRequested",
    ),
  );

  // Poll cadence tracks what's on screen: fast while background work is
  // visible, relaxed when the panel is empty (the old fixed 800ms tick kept
  // the WebView2 process awake on an idle home page). A hidden window
  // (minimized / occluded) skips the IPC entirely.
  $effect(() => {
    const activeCount = visible.length;
    const id = setInterval(() => {
      if (document.hidden) return;
      void refresh();
    }, taskPollIntervalMs(activeCount));
    return () => clearInterval(id);
  });
</script>

{#if visible.length}
  <aside class="task-panel" aria-label="Background tasks">
    {#each visible as t (t.id)}
      <div
        class="task"
        class:failed={t.status === "failed"}
        class:running={t.status === "running"}
        class:paused={t.status === "paused"}
      >
        <div class="row">
          {#if t.status === "running"}
            <Loader2 size={14} class="spin" />
          {:else if t.status === "failed"}
            <AlertTriangle size={14} />
          {:else if t.status === "paused"}
            <Pause size={14} />
          {:else}
            <CheckCircle2 size={14} />
          {/if}
          <strong>{t.title}</strong>
          {#if t.status === "running" && isOllamaPull(t.id)}
            <button type="button" class="ghost" title="Pause download" onclick={() => pauseOllamaPull()}>
              <Pause size={14} />
            </button>
          {:else if t.status === "paused" && isOllamaPull(t.id)}
            <button type="button" class="ghost" title="Resume download" onclick={() => resumeOllamaPull(t.id)}>
              <Play size={14} />
            </button>
          {:else if t.status === "running" && !isOllamaPull(t.id)}
            <button type="button" class="ghost" title="Cancel task" onclick={() => cancelTask(t.id)}>
              <X size={14} />
            </button>
          {/if}
          <button type="button" class="ghost" title="Dismiss" onclick={() => dismiss(t.id)}>
            <X size={14} />
          </button>
        </div>
        {#if (t.status === "running" || t.status === "paused") && t.progress != null}
          <div class="bar"><div class="fill" style={`width: ${Math.round((t.progress || 0) * 100)}%`}></div></div>
        {/if}
        {#if t.detail}
          <small>{t.detail}</small>
        {/if}
        {#if t.error}
          <small class="err">{t.error}</small>
        {/if}
      </div>
    {/each}
  </aside>
{/if}

<style>
  .task-panel {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: 80;
    width: min(320px, calc(100vw - 32px));
    display: flex;
    flex-direction: column;
    gap: 8px;
    pointer-events: none;
  }
  .task {
    pointer-events: auto;
    background: var(--bg-elevated, #e2e8df);
    border: 1px solid var(--border-color, #cfd8cf);
    border-radius: var(--border-radius-sm);
    padding: 10px 12px;
    box-shadow: var(--shadow-md, 0 4px 14px rgba(21, 40, 28, 0.09));
  }
  .task.failed {
    border-color: #c44;
  }
  .task.paused {
    border-color: #64748b;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .row strong {
    flex: 1;
    font-size: 13px;
  }
  .ghost {
    background: transparent;
    border: none;
    color: inherit;
    cursor: pointer;
    opacity: 0.7;
    padding: 2px;
  }
  .bar {
    margin-top: 8px;
    height: 4px;
    background: rgba(255, 255, 255, 0.08);
    border-radius: 2px;
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent, #6cf);
    transition: width 0.2s ease;
  }
  .paused .fill {
    background: #94a3b8;
  }
  :global(html.potato-pc) .fill {
    transition: none;
  }
  small {
    display: block;
    margin-top: 4px;
    opacity: 0.75;
    font-size: 12px;
  }
  .err {
    color: #f88;
  }
  :global(.spin) {
    animation: spin 0.9s linear infinite;
  }
  :global(html.potato-pc) :global(.spin) {
    animation: none;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
