<script lang="ts">
  let {
    open = $bindable(false),
    source = "ai",
    explanation = "",
    hasDestructive = false,
    rows = $bindable([]),
    needsAck = false,
    acknowledged = $bindable(false),
    canApply = false,
    selectedCount = 0,
    busy = false,
    networkTrust = null,
    onCancel,
    onConfirm,
  }: {
    open?: boolean;
    source?: "ai" | "network" | "fix-all";
    explanation?: string;
    hasDestructive?: boolean;
    rows?: {
      key: string;
      selected: boolean;
      op: string;
      modId: string | null;
      path: string | null;
      patchPreview: string | null;
      reason: string;
      risk: string;
      diffKind?: "add" | "remove" | "change" | "other";
      destructive?: boolean;
      problemTitle?: string | null;
      raw: any;
    }[];
    needsAck?: boolean;
    acknowledged?: boolean;
    canApply?: boolean;
    selectedCount?: number;
    busy?: boolean;
    networkTrust?: {
      trustPercent: number | null;
      keeps: number | null;
      discards: number | null;
      mc: string | null;
      loader: string | null;
    } | null;
    onCancel?: () => void;
    onConfirm?: () => void;
  } = $props();

  const showTrust = $derived(
    source === "network" &&
      !!networkTrust &&
      (networkTrust.trustPercent != null ||
        networkTrust.keeps != null ||
        networkTrust.discards != null ||
        networkTrust.mc != null ||
        networkTrust.loader != null),
  );
  const modalTitle = $derived(
    source === "fix-all"
      ? "Review fixes"
      : source === "network"
        ? "Review network ActionPlan"
        : "Review AI ActionPlan",
  );
</script>

{#if open}
  <div
    class="modal-backdrop"
    role="button"
    tabindex="-1"
    onclick={(e) => e.target === e.currentTarget && onCancel?.()}
    onkeydown={() => {}}
  >
    <div class="modal plan-review-modal" role="dialog" aria-modal="true">
      <div class="modal-header">
        <div>
          <h2>{modalTitle}</h2>
          <p>Snapshot will be created first. Uncheck actions you do not want applied.</p>
        </div>
        <button class="icon-btn" type="button" onclick={() => onCancel?.()} aria-label="Close">×</button>
      </div>
      <p class="plan-review-expl">{explanation}</p>
      {#if showTrust && networkTrust}
        <div class="trust-card-line">
          {#if networkTrust.trustPercent != null}
            <span class="trust-chip">Trust {networkTrust.trustPercent}%</span>
          {/if}
          {#if networkTrust.keeps != null}
            <span class="trust-chip">Keeps {networkTrust.keeps}</span>
          {/if}
          {#if networkTrust.discards != null}
            <span class="trust-chip">Discards {networkTrust.discards}</span>
          {/if}
          {#if networkTrust.mc}
            <span class="trust-chip">MC {networkTrust.mc}</span>
          {/if}
          {#if networkTrust.loader}
            <span class="trust-chip">{networkTrust.loader}</span>
          {/if}
        </div>
      {/if}
      {#if (source === "network" || source === "fix-all") && hasDestructive}
        <p class="plan-review-warn">
          This plan includes destructive actions (disable/remove). A snapshot will be created first — use Restore on the home screen if something breaks.
        </p>
      {/if}
      <div class="plan-review-list">
        {#each rows as row (row.key)}
          <label class="plan-review-row">
            <input type="checkbox" bind:checked={row.selected} />
            <div class="plan-review-body">
              <div class="plan-review-top">
                <span class="diff-chip {row.diffKind ?? 'other'}">
                  {row.diffKind === "add" ? "+" : row.diffKind === "remove" ? "−" : row.diffKind === "change" ? "~" : "·"}
                </span>
                <strong>{row.op}</strong>
                {#if row.modId}<code>{row.modId}</code>{/if}
                <span class="risk-pill">{row.risk}</span>
              </div>
              {#if row.problemTitle}
                <p class="problem-ref">From: {row.problemTitle}</p>
              {/if}
              {#if row.reason}<p>{row.reason}</p>{/if}
              {#if row.patchPreview}
                <pre class="patch-preview">{row.patchPreview}</pre>
              {/if}
            </div>
          </label>
        {/each}
      </div>
      {#if needsAck}
        <label class="plan-review-ack">
          <input type="checkbox" bind:checked={acknowledged} />
          {source === "fix-all"
            ? "I reviewed these fixes including any destructive steps"
            : "I reviewed these actions (required — plan flagged needsUserReview)"}
        </label>
      {/if}
      <div class="plan-review-actions">
        <button class="ghost" type="button" onclick={() => onCancel?.()}>Cancel</button>
        <button
          class="primary"
          type="button"
          disabled={!canApply || busy}
          onclick={() => onConfirm?.()}
        >
          Apply {selectedCount} action{selectedCount === 1 ? "" : "s"} (snapshot first)
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .diff-chip {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 700;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }
  .diff-chip.add { color: var(--accent-primary); border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent); background: color-mix(in srgb, var(--accent-primary) 10%, transparent); }
  .diff-chip.remove { color: var(--accent-danger); border-color: color-mix(in srgb, var(--accent-danger) 35%, transparent); background: color-mix(in srgb, var(--accent-danger) 10%, transparent); }
  .diff-chip.change { color: var(--accent-warning); border-color: color-mix(in srgb, var(--accent-warning) 35%, transparent); background: color-mix(in srgb, var(--accent-warning) 10%, transparent); }
  .plan-review-warn {
    margin: 0 0 10px !important;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    font-size: 12px !important;
    color: var(--accent-warning) !important;
    background: color-mix(in srgb, var(--accent-warning) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent-warning) 28%, transparent);
  }
  .modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 80;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.55);
    padding: 16px;
  }
  .plan-review-modal {
    width: min(560px, 100%);
    max-height: 85vh;
    overflow: auto;
    padding: 16px 18px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
  }
  .plan-review-modal .modal-header {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 10px;
  }
  .plan-review-modal h2 { margin: 0 0 4px; font-size: 16px; }
  .plan-review-modal p { margin: 0; font-size: 13px; color: var(--text-muted); }
  .plan-review-expl { margin: 0 0 12px !important; color: var(--text-secondary) !important; }
  .trust-card-line {
    font-size: 12px;
    color: var(--text-secondary);
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    margin: 0 0 12px;
  }
  .trust-chip {
    display: inline-flex;
    align-items: center;
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 700;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }
  .plan-review-list { display: flex; flex-direction: column; gap: 8px; margin-bottom: 12px; }
  .plan-review-row {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 10px;
    border-radius: 10px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    cursor: pointer;
  }
  .plan-review-top { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .plan-review-body p { margin: 4px 0 0; font-size: 12px; color: var(--text-secondary); }
  .problem-ref {
    margin: 0 0 2px !important;
    font-size: 12px !important;
    color: var(--text-muted) !important;
    font-weight: 600;
  }
  .patch-preview {
    margin: 6px 0 0;
    padding: 8px;
    max-height: 120px;
    overflow: auto;
    font-size: 12px;
    border-radius: 6px;
    background: var(--bg-primary);
    color: var(--text-muted);
  }
  .plan-review-ack {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 13px;
    margin-bottom: 12px;
  }
  .plan-review-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .risk-pill {
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    padding: 1px 6px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    color: var(--text-muted);
  }
</style>
