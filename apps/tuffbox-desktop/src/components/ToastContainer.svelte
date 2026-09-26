<script lang="ts">
  import type { Component } from "svelte";
  import { toasts, type Toast } from "../lib/toast";
  import { X, CheckCircle2, AlertTriangle, Info, AlertCircle, Copy } from "@lucide/svelte";
  import { fly, fade } from "svelte/transition";
  import { copyText } from "../lib/clipboard";

  function icon(t: string): Component<{ size?: number; color?: string }> {
    if (t === "success") return CheckCircle2;
    if (t === "error") return AlertCircle;
    if (t === "warning") return AlertTriangle;
    return Info;
  }
  function clr(t: string): string {
    if (t === "success") return "var(--accent-primary)";
    if (t === "error") return "var(--accent-danger)";
    if (t === "warning") return "var(--accent-warning)";
    return "var(--accent-secondary)";
  }

  let copiedId = $state<number | null>(null);

  async function copyDetails(t: Toast) {
    try {
      await copyText(t.message);
      copiedId = t.id;
      setTimeout(() => { if (copiedId === t.id) copiedId = null; }, 1500);
    } catch {
      // clipboard unavailable — leave the toast visible for manual selection
    }
  }

  let now = $state(Date.now());

  $effect(() => {
    const interval = setInterval(() => {
      now = Date.now();
    }, 100);
    return () => clearInterval(interval);
  });

  function progress(t: Toast): number {
    if (t.duration <= 0) return 1;
    const elapsed = now - t.timestamp;
    return Math.min(1, elapsed / t.duration);
  }
</script>
<div class="tc">
  {#each $toasts as t (t.id)}
    {@const Icon = icon(t.type)}
    <div
      class="t {t.type} tb-toast-enter"
      style="--tc:{clr(t.type)}; --progress:{progress(t)}"
      in:fly={{ y: 12, duration: 220 }}
      out:fade={{ duration: 140 }}
    >
      <span class="ti"><Icon size={16} color="var(--tc)" /></span>
      <span class="tm">{t.message}</span>
      {#if t.type === "error"}
        <button
          class="ta copy"
          onclick={() => void copyDetails(t)}
          title="Copy error details"
          aria-label="Copy error details"
        >
          {#if copiedId === t.id}<CheckCircle2 size={12} />{:else}<Copy size={12} /> Copy{/if}
        </button>
      {/if}
      {#if t.actions}
        {#each t.actions as a}
          <button class="ta" onclick={() => { a.run(); toasts.dismiss(t.id); }}>{a.label}</button>
        {/each}
      {/if}
      <button class="tx" onclick={() => toasts.dismiss(t.id)}><X size={12} /></button>
      {#if t.duration > 0}
        <div class="progress-bar" style="--tc:{clr(t.type)}; width: {((1 - progress(t)) * 100)}%"></div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .tc {
    position: fixed;
    bottom: 20px;
    right: 20px;
    z-index: 1000;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 420px;
    pointer-events: none;
  }

  .t {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    border-radius: var(--border-radius-md);
    background: var(--bg-elevated);
    border: 1px solid var(--tc);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    pointer-events: all;
    backdrop-filter: blur(12px);
    overflow: hidden;
  }

  .ti { flex-shrink: 0; }
  .tm { flex: 1; font-size: 13px; color: var(--text-primary); line-height: 1.4; white-space: pre-wrap; }

  .ta {
    flex-shrink: 0;
    margin-left: 4px;
    padding: 4px 10px;
    font-size: 12px;
    font-weight: 600;
    color: #0c0c0f;
    background: var(--tc);
    border: none;
    border-radius: 6px;
    cursor: pointer;
  }
  .ta:hover { filter: brightness(1.1); }

  .ta.copy {
    background: var(--bg-hover);
    color: var(--text-primary);
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .ta.copy:hover {
    background: color-mix(in srgb, var(--tc) 25%, var(--bg-hover));
    filter: none;
  }

  .tx {
    flex-shrink: 0;
    width: 20px;
    height: 20px;
    padding: 0;
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
  }
  .tx:hover { background: var(--bg-hover); color: var(--text-primary); }

  .progress-bar {
    position: absolute;
    bottom: 0;
    left: 0;
    height: 2px;
    background: var(--tc);
    opacity: 0.6;
    transition: width 0.1s linear;
    border-radius: 0 1px 0 0;
  }
</style>
