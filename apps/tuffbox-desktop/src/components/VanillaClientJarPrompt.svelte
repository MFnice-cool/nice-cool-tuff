<script lang="ts">
  import { Download, Loader2 } from "@lucide/svelte";
  import { trapFocus } from "../lib/focusTrap";

  let {
    open = false,
    version,
    downloadSize = null,
    downloading = false,
    error = null,
    ondownload,
    ondismiss,
  }: {
    open?: boolean;
    version: string;
    downloadSize?: number | null;
    downloading?: boolean;
    error?: string | null;
    ondownload?: () => void;
    ondismiss?: () => void;
  } = $props();

  const sizeHint = $derived.by(() => {
    if (downloadSize == null || downloadSize <= 0) return null;
    const mb = downloadSize / (1024 * 1024);
    if (mb >= 1) return `~${Math.round(mb)} MB`;
    const kb = downloadSize / 1024;
    return `~${Math.max(1, Math.round(kb))} KB`;
  });

  function handleBackdropClick(e: MouseEvent) {
    if (downloading) return;
    if (e.target === e.currentTarget) ondismiss?.();
  }
</script>

{#if open}
  <div
    class="vcj-backdrop"
    role="presentation"
    onclick={handleBackdropClick}
  >
    <div
      class="vcj-dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="vcj-title"
      use:trapFocus={{
        onEscape: () => {
          if (!downloading) ondismiss?.();
        },
      }}
    >
      <div class="vcj-icon">
        <Download size={28} color="var(--accent-primary)" />
      </div>
      <h3 id="vcj-title">Minecraft {version} client missing</h3>
      <p>
        Vanilla recipes and the quest item picker need the official Minecraft client jar.
        Download it from Mojang into the TuffBox runtime so these features can work offline.
        {#if sizeHint}
          Approximate download size: {sizeHint}.
        {/if}
      </p>
      {#if error}
        <p class="vcj-error">{error}</p>
      {/if}
      <div class="vcj-actions">
        <button class="ghost" disabled={downloading} onclick={() => ondismiss?.()}>
          Not now
        </button>
        <button disabled={downloading} onclick={() => ondownload?.()}>
          {#if downloading}
            <Loader2 size={16} class="spin" />
            Downloading…
          {:else}
            <Download size={16} />
            Download
          {/if}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .vcj-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 200;
    backdrop-filter: blur(8px);
  }
  .vcj-dialog {
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-xl);
    padding: 28px;
    width: 440px;
    text-align: center;
    box-shadow: var(--shadow-lg);
  }
  .vcj-icon {
    margin-bottom: 12px;
  }
  .vcj-dialog h3 {
    font-size: 18px;
    margin-bottom: 8px;
    color: var(--text-primary);
  }
  .vcj-dialog p {
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.5;
    margin-bottom: 20px;
  }
  .vcj-error {
    color: var(--accent-danger);
    margin-top: -12px;
    margin-bottom: 16px;
  }
  .vcj-actions {
    display: flex;
    gap: 10px;
    justify-content: center;
  }
  .vcj-actions button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }
  .vcj-actions button:disabled {
    opacity: 0.65;
    cursor: not-allowed;
  }
  .vcj-actions :global(.spin) {
    animation: vcj-spin 0.8s linear infinite;
  }
  @keyframes vcj-spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
