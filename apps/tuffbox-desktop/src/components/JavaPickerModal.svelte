<script lang="ts">
  import { X, Search, Loader2, FolderOpen, Check, Download } from "@lucide/svelte";
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { trapFocus } from "../lib/focusTrap";

  type JavaRuntime = { path: string; version: string; major: number };

  let {
    current,
    onclose,
    onselected,
  }: {
    current: string;
    onclose?: () => void;
    onselected?: (path: string) => void;
  } = $props();

  let runtimes = $state<JavaRuntime[]>([]);
  let loading = $state(true);
  let installing = $state(false);
  let error = $state("");

  onMount(async () => {
    try {
      runtimes = await invoke("find_java_runtimes");
      runtimes.sort((a, b) => b.major - a.major);
    } catch (e) {
      error = `${e}`;
    } finally {
      loading = false;
    }
  });

  async function browse() {
    const isWindows = typeof navigator !== "undefined" && /win/i.test(navigator.platform || "");
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [
        {
          name: "Java executable",
          extensions: isWindows ? ["exe"] : ["java", "bin", "*"],
        },
      ],
    });
    if (selected && typeof selected === "string") {
      onselected?.(selected);
    }
  }

  async function downloadGraalVm() {
    if (installing) return;
    installing = true;
    error = "";
    try {
      const runtime = await invoke<JavaRuntime>("ensure_java_runtime");
      runtimes = await invoke<JavaRuntime[]>("find_java_runtimes");
      runtimes.sort((a, b) => b.major - a.major);
      onselected?.(runtime.path);
    } catch (e) {
      error = `${e}`;
    } finally {
      installing = false;
    }
  }

  function select(path: string) {
    onselected?.(path);
  }
</script>

<div class="modal-backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose?.()}>
  <div class="modal" role="dialog" aria-modal="true" use:trapFocus={{ onEscape: () => onclose?.() }}>
    <div class="modal-header">
      <h2>
        <Search size={18} />
        Select Java Runtime
      </h2>
      <button class="icon-btn" onclick={() => onclose?.()} aria-label="Close">
        <X size={18} />
      </button>
    </div>

    <div class="modal-body">
      {#if error}
        <div class="error">{error}</div>
      {/if}

      <div class="actions-row">
        <button class="ghost" onclick={browse} disabled={installing}>
          <FolderOpen size={16} />
          Browse manually
        </button>
        <button class="secondary" onclick={downloadGraalVm} disabled={installing}>
          {#if installing}
            <Loader2 size={16} class="spin" />
            Downloading GraalVM…
          {:else}
            <Download size={16} />
            Download GraalVM
          {/if}
        </button>
      </div>

      {#if loading}
        <div class="loader">
          <Loader2 size={20} class="spin" />
          Scanning for Java installations...
        </div>
      {:else if runtimes.length === 0}
        <div class="empty">
          No Java installations found. Download GraalVM to install a managed runtime.
        </div>
      {:else}
        <div class="runtimes">
          {#each runtimes as runtime (runtime.path)}
            <button
              class="runtime"
              class:active={runtime.path === current}
              onclick={() => select(runtime.path)}
            >
              <div class="runtime-info">
                <span class="runtime-version">Java {runtime.major}</span>
                <span class="runtime-path">{runtime.path}</span>
              </div>
              {#if runtime.path === current}
                <Check size={16} />
              {/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <div class="modal-footer">
      <button class="ghost" onclick={() => onclose?.()}>Cancel</button>
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
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-xl);
    width: 100%;
    max-width: 560px;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5);
    display: flex;
    flex-direction: column;
    max-height: 80vh;
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 20px 24px;
    border-bottom: 1px solid var(--border-color);
  }

  .modal-header h2 {
    font-size: 18px;
    font-weight: 800;
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .icon-btn {
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--border-radius-md);
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
  }

  .icon-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .modal-body {
    padding: 20px 24px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .actions-row {
    display: flex;
    gap: 10px;
  }

  .actions-row button {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .loader {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 32px;
    color: var(--text-muted);
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .empty {
    text-align: center;
    padding: 32px;
    color: var(--text-muted);
  }

  .runtimes {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .runtime {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px;
    background: var(--bg-elevated);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    text-align: left;
    cursor: pointer;
    color: var(--text-primary);
  }

  .runtime:hover {
    border-color: var(--bg-hover);
  }

  .runtime.active {
    border-color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
  }

  .runtime-info {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .runtime-version {
    font-weight: 700;
  }

  .runtime-path {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    padding: 16px 24px;
    border-top: 1px solid var(--border-color);
  }

  .error {
    background: rgba(239, 68, 68, 0.12);
    color: #ef4444;
    padding: 10px 12px;
    border-radius: var(--border-radius-md);
    font-size: 13px;
  }
</style>
