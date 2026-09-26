<script lang="ts">
  let {
    total = 0,
    rac = 0,
    loading = false,
    hint = "",
    compact = false,
    title = "Your Kudos",
    onclick,
  }: {
    total?: number;
    rac?: number;
    loading?: boolean;
    hint?: string;
    compact?: boolean;
    title?: string;
    onclick?: (() => void) | undefined;
  } = $props();

  const totalLabel = $derived(loading ? "Loading Kudos…" : Number(total).toFixed(0));
  const racLabel = $derived(Number(rac).toFixed(1));
  const compactText = $derived(
    loading ? "Loading Kudos…" : `${title}: ${totalLabel} · RAC ${racLabel}`,
  );
</script>

{#if compact}
  {#if onclick}
    <button type="button" class="kudos-chip" {onclick}>
      {compactText}
    </button>
  {:else}
    <span class="kudos-chip">{compactText}</span>
  {/if}
{:else if onclick}
  <button type="button" class="kudos-strip" {onclick}>
    {#if loading}
      Loading Kudos…
    {:else}
      {title}: <strong>{totalLabel}</strong>
      · RAC <strong>{racLabel}</strong>
      {#if hint}
        <span class="kudos-hint">({hint})</span>
      {/if}
    {/if}
  </button>
{:else}
  <p class="kudos-strip">
    {#if loading}
      Loading Kudos…
    {:else}
      {title}: <strong>{totalLabel}</strong>
      · RAC <strong>{racLabel}</strong>
      {#if hint}
        <span class="kudos-hint">({hint})</span>
      {/if}
    {/if}
  </p>
{/if}

<style>
  .kudos-strip {
    margin: 0;
    padding: 8px 12px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    font: inherit;
    font-size: 12px;
    color: var(--text-secondary);
    text-align: left;
    width: 100%;
    box-sizing: border-box;
  }
  button.kudos-strip {
    cursor: pointer;
  }
  .kudos-strip strong {
    color: var(--text-primary);
  }
  .kudos-hint {
    margin-left: 6px;
    color: var(--text-muted);
    font-size: 12px;
  }
  .kudos-chip {
    display: inline-flex;
    align-items: center;
    padding: 4px 10px;
    font: inherit;
    font-size: 12px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    line-height: 1.3;
  }
  button.kudos-chip {
    cursor: pointer;
  }
</style>
