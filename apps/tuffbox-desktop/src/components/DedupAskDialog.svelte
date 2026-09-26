<script lang="ts">
  import { Share2, Shield } from "@lucide/svelte";

  /**
   * Import-time dedup question (docs/17 §4): before a pack is imported, ask
   * whether its files should join the shared dedup store. Promise-free by
   * design — the caller keeps a resolver in state and feeds the answer back
   * through `onanswer`. Cancel (backdrop / Escape) aborts the import.
   */
  let {
    open = false,
    busy = false,
    packName = "",
    onanswer,
    oncancel,
  }: {
    open?: boolean;
    busy?: boolean;
    packName?: string;
    onanswer: (dedup: boolean) => void;
    oncancel: () => void;
  } = $props();

  function onKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      oncancel();
    } else if (e.key === "Enter") {
      e.preventDefault();
      onanswer(true);
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dedup-ask-backdrop" role="presentation" onclick={oncancel}>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      class="dedup-ask"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      aria-label="Use file deduplication for this pack?"
      onclick={(e) => e.stopPropagation()}
    >
      <h3>
        Use file deduplication{packName ? ` for “${packName}”` : " for this pack"}?
      </h3>
      <p class="dedup-ask-desc">
        Identical mods, resource packs and shaders are stored once on disk and shared between
        your packs. Nothing is deleted — every pack still sees its own complete file list.
      </p>
      <div class="dedup-ask-options">
        <button
          type="button"
          class="dedup-ask-opt"
          disabled={busy}
          onclick={() => onanswer(true)}
        >
          <Share2 size={16} />
          <span class="dedup-ask-opt-text">
            <strong>Share identical files</strong>
            <small>Recommended — saves disk space when packs overlap</small>
          </span>
        </button>
        <button
          type="button"
          class="dedup-ask-opt"
          disabled={busy}
          onclick={() => onanswer(false)}
        >
          <Shield size={16} />
          <span class="dedup-ask-opt-text">
            <strong>Keep independent files</strong>
            <small>Uses more disk space; the pack is 100% self-contained</small>
          </span>
        </button>
      </div>
      <p class="dedup-ask-note">
        You can change this at any time in Project Settings → File deduplication.
      </p>
    </div>
  </div>
{/if}

<style>
  .dedup-ask-backdrop {
    position: fixed;
    inset: 0;
    z-index: 220;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
    background: color-mix(in srgb, var(--bg-primary) 62%, transparent);
    -webkit-backdrop-filter: blur(4px);
    backdrop-filter: blur(4px);
  }

  .dedup-ask {
    width: min(440px, 100%);
    padding: 22px;
    border-radius: var(--border-radius-lg);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
    box-shadow: var(--shadow-lg);
  }

  .dedup-ask h3 {
    margin: 0 0 8px;
    font-size: 16px;
    color: var(--text-primary);
  }

  .dedup-ask-desc {
    margin: 0 0 16px;
    font-size: 13px;
    line-height: 1.45;
    color: var(--text-secondary);
  }

  .dedup-ask-options {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .dedup-ask-opt {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    cursor: pointer;
    text-align: left;
    transition:
      border-color var(--motion-fast) var(--ease-out),
      background var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }

  .dedup-ask-opt:hover:not(:disabled) {
    border-color: var(--accent-primary);
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .dedup-ask-opt:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .dedup-ask-opt-text {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .dedup-ask-opt-text strong {
    font-size: 13.5px;
    color: var(--text-primary);
  }

  .dedup-ask-opt-text small {
    font-size: 12px;
    line-height: 1.35;
    color: var(--text-secondary);
  }

  .dedup-ask-note {
    margin: 14px 0 0;
    font-size: 12px;
    line-height: 1.4;
    color: var(--text-muted);
  }
</style>
