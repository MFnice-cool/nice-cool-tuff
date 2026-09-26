<script lang="ts">
  type Timing = { elapsedMs: number; cacheHit: boolean };
  let { timings }: { timings: Record<string, Timing> } = $props();
  const rows = $derived(
    Object.entries(timings)
      .map(([phase, value]) => ({ phase, ...value }))
      .sort((a, b) => b.elapsedMs - a.elapsedMs),
  );
</script>

{#if rows.length}
  <details class="diagnose-performance">
    <summary>Performance phases</summary>
    <div class="timing-grid">
      {#each rows as row (row.phase)}
        <div class="timing-row">
          <span>{row.phase}</span>
          <strong>{row.elapsedMs} ms</strong>
          <small>{row.cacheHit ? "cache" : "cold"}</small>
        </div>
      {/each}
    </div>
  </details>
{/if}

<style>
  .diagnose-performance {
    margin-top: 4px;
    color: var(--text-muted);
    font-size: 12px;
  }
  summary { cursor: pointer; user-select: none; }
  .timing-grid { display: grid; gap: 8px; margin-top: 8px; }
  .timing-row {
    display: grid;
    grid-template-columns: 1fr auto auto;
    gap: 10px;
    align-items: center;
    padding: 4px 0;
  }
  .timing-row strong { color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .timing-row small { color: var(--accent-primary); }
</style>
