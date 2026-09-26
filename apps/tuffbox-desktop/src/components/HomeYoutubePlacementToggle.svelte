<script lang="ts">
  import { Columns2, Rows2 } from "@lucide/svelte";
  import { homeYoutubePlacement } from "../lib/store";

  let { compact = false }: { compact?: boolean } = $props();

  const besideSkin = $derived($homeYoutubePlacement === "right");
  const label = $derived(besideSkin ? "Show below packs" : "Show beside skin");

  function onToggle(e: MouseEvent) {
    e.stopPropagation();
    e.preventDefault();
    homeYoutubePlacement.toggle();
  }
</script>

<button
  type="button"
  class="placement-btn"
  class:compact
  title={label}
  aria-label={label}
  aria-pressed={besideSkin}
  onclick={onToggle}
>
  {#if besideSkin}
    <Rows2 size={14} />
  {:else}
    <Columns2 size={14} />
  {/if}
  {#if !compact}
    <span>{label}</span>
  {/if}
</button>

<style>
  .placement-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    padding: 5px 10px;
    border: 1px solid transparent;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .placement-btn:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }
  .placement-btn[aria-pressed="true"] {
    color: var(--accent-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 10%, transparent);
  }
  .placement-btn.compact {
    padding: 5px;
  }
</style>
