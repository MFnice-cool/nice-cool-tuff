<script lang="ts">
  import { CheckCircle, AlertTriangle, CircleHelp, Loader2, Wrench } from "@lucide/svelte";
  import {
    type Problem,
    countBySeverity,
    hasBlockingProblems,
    severityChip,
  } from "./problemModel";

  let {
    problems = [],
    sessionOk = false,
    loading = false,
    analyzing = false,
    liveStage = null,
    cascadeStage = null,
    cascadeLabel = null,
    cascadeDetail = null,
    sourceLabel = "Game log",
    fixAllCount = 0,
    fixAllBusy = false,
    onScrollToWarnings,
    onFixAll,
  }: {
    problems?: Problem[];
    sessionOk?: boolean;
    loading?: boolean;
    analyzing?: boolean;
    /** Live backend stage label (task bus); shown while loading/analyzing. */
    liveStage?: string | null;
    cascadeStage?: string | null;
    cascadeLabel?: string | null;
    cascadeDetail?: string | null;
    sourceLabel?: string;
    fixAllCount?: number;
    fixAllBusy?: boolean;
    onScrollToWarnings?: () => void;
    onFixAll?: () => void;
  } = $props();

  const counts = $derived(countBySeverity(problems));
  const blocking = $derived(hasBlockingProblems(problems));
  const warnOnly = $derived(!blocking && counts.warning + counts.info > 0);

  const cascadeHint = $derived.by(() => {
    switch (cascadeStage) {
      case "l1_searching":
        return "Searching known fixes…";
      case "l1_hit":
        return "Known fix from network/KB";
      case "l2_asking":
        return "Asking a community volunteer…";
      case "l2_hit":
        return "Plan from community volunteer";
      case "l3_asking":
        return "Generating with AI…";
      case "l3_hit":
        return "AI-generated plan";
      case "heuristic":
        return "Local heuristic plan";
      default:
        return null;
    }
  });
</script>

<div
  class="dx-status"
  class:ok={sessionOk && !blocking}
  class:warn={blocking}
  class:neutral={!sessionOk && !blocking && !warnOnly}
  class:soft={warnOnly && sessionOk}
>
  <div class="dx-status-icon">
    {#if loading || analyzing}
      <Loader2 size={18} class="spin" />
    {:else if sessionOk && !blocking}
      <CheckCircle size={18} />
    {:else if blocking}
      <AlertTriangle size={18} />
    {:else}
      <CircleHelp size={18} />
    {/if}
  </div>
  <div class="dx-status-body">
    {#if loading && problems.length === 0}
      <strong>Loading…</strong>
      <span>{liveStage ?? "Reading logs and pack graph"}</span>
    {:else if analyzing}
      <strong>Analyzing…</strong>
      <span>{cascadeHint ?? liveStage ?? "Rules and AI are scanning this source"}</span>
    {:else if sessionOk && !blocking}
      <strong>Healthy</strong>
      <span>
        Last launch looked fine
        {#if warnOnly}
          ·
          <button type="button" class="linkish" onclick={() => onScrollToWarnings?.()}>
            {counts.warning + counts.info} worth checking
          </button>
        {/if}
        · {sourceLabel}
      </span>
    {:else if blocking}
      <strong>Needs fix</strong>
      <span>
        {#if counts.critical}{counts.critical} critical{/if}
        {#if counts.critical && counts.error} · {/if}
        {#if counts.error}{counts.error} error{counts.error === 1 ? "" : "s"}{/if}
        {#if counts.warning}
          · {counts.warning} warning{counts.warning === 1 ? "" : "s"}
        {/if}
        · {sourceLabel}
      </span>
    {:else if warnOnly}
      <strong>{severityChip("warning")}</strong>
      <span>{counts.warning + counts.info} item(s) · {sourceLabel}</span>
    {:else}
      <strong>No clear signal yet</strong>
      <span>Re-analyze or Test launch · {sourceLabel}</span>
    {/if}
    {#if !analyzing && !loading && cascadeLabel}
      <span class="cascade-chip" title={cascadeDetail ?? undefined}>
        {cascadeLabel}{#if cascadeDetail}<span class="cascade-detail"> · {cascadeDetail}</span>{/if}
      </span>
    {/if}
  </div>
  {#if fixAllCount > 0 && !loading && !analyzing}
    <button
      type="button"
      class="fix-all-btn"
      disabled={fixAllBusy}
      onclick={() => onFixAll?.()}
      title="Review and apply all automatic fixes at once"
    >
      <Wrench size={14} />
      {fixAllBusy ? "Applying…" : `Fix all (${fixAllCount})`}
    </button>
  {/if}
</div>

<style>
  .dx-status {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    margin-bottom: 14px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
  }
  .dx-status.warn {
    border-color: rgba(245, 158, 11, 0.45);
    background: linear-gradient(135deg, rgba(245, 158, 11, 0.1), var(--bg-secondary) 70%);
  }
  .dx-status.ok,
  .dx-status.soft {
    border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent);
    background: linear-gradient(135deg, color-mix(in srgb, var(--accent-primary) 8%, transparent), var(--bg-secondary) 70%);
  }
  .dx-status-icon {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: var(--border-radius-sm);
    background: var(--bg-tertiary);
    color: var(--text-muted);
    flex-shrink: 0;
  }
  .dx-status.warn .dx-status-icon { color: var(--accent-warning); background: rgba(245, 158, 11, 0.14); }
  .dx-status.ok .dx-status-icon,
  .dx-status.soft .dx-status-icon { color: var(--accent-primary); background: color-mix(in srgb, var(--accent-primary) 14%, transparent); }
  .dx-status-body { min-width: 0; display: flex; flex-direction: column; gap: 8px; }
  .dx-status-body strong { font-size: 13px; color: var(--text-primary); }
  .dx-status-body span { font-size: 12px; color: var(--text-muted); }
  .cascade-chip {
    display: inline-flex;
    align-items: center;
    align-self: flex-start;
    margin-top: 4px;
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 12px !important;
    font-weight: 700;
    color: var(--accent-primary) !important;
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 28%, transparent);
  }
  .cascade-detail { font-weight: 500; opacity: 0.85; }
  .linkish {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent-primary);
    font: inherit;
    font-weight: 700;
    cursor: pointer;
    text-decoration: underline;
  }
  .fix-all-btn {
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 7px 12px;
    border-radius: var(--border-radius-sm);
    border: none;
    background: var(--accent-primary);
    color: #000;
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
  }
  .fix-all-btn:disabled { opacity: 0.55; cursor: not-allowed; }
  :global(.spin) { animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
