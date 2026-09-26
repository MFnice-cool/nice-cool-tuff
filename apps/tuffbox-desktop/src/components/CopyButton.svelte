<script lang="ts">
  import { Copy, Check } from "@lucide/svelte";
  import { fly } from "svelte/transition";

  let {
    text,
    label = "",
    size = "sm",
  }: {
    text: string;
    label?: string;
    size?: "sm" | "md";
  } = $props();

  let copied = $state(false);
  let timeout: ReturnType<typeof setTimeout>;

  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      document.body.removeChild(ta);
    }
    copied = true;
    clearTimeout(timeout);
    timeout = setTimeout(() => (copied = false), 1500);
  }
</script>

<button
  class="copy-btn"
  class:md={size === "md"}
  onclick={copy}
  title={label || "Copy to clipboard"}
  aria-label={label || "Copy to clipboard"}
>
  {#if copied}
    <span class="icon-wrap" in:fly={{ y: 4, duration: 150 }}>
      <Check size={size === "md" ? 16 : 14} />
    </span>
    <span class="copy-text">Copied</span>
  {:else}
    <span class="icon-wrap">
      <Copy size={size === "md" ? 16 : 14} />
    </span>
    {#if label}
      <span class="copy-text">{label}</span>
    {/if}
  {/if}
</button>

<style>
  .copy-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 4px 10px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
    color: var(--text-muted);
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
    transition: all 0.12s ease;
    white-space: nowrap;
  }

  .copy-btn.md {
    padding: 6px 14px;
    font-size: 13px;
    border-radius: var(--border-radius-md);
  }

  .copy-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
    border-color: var(--accent-primary);
  }

  .copy-btn:active {
    transform: scale(0.97);
  }

  .icon-wrap {
    display: flex;
    align-items: center;
    flex-shrink: 0;
  }

  .copy-text {
    color: inherit;
  }
</style>
