<script lang="ts">
  let {
    open = $bindable(false),
    explanation = "",
    rows = $bindable([]),
    needsAck = false,
    acknowledged = $bindable(false),
    canApply = false,
    selectedCount = 0,
    busy = false,
    onCancel,
    onConfirm,
  }: {
    open?: boolean;
    explanation?: string;
    rows?: {
      key: string;
      selected: boolean;
      op: string;
      path: string | null;
      patchPreview: string | null;
      diffBefore: string | null;
      diffAfter: string | null;
      reason: string;
      risk: string;
      raw: any;
    }[];
    needsAck?: boolean;
    acknowledged?: boolean;
    canApply?: boolean;
    selectedCount?: number;
    busy?: boolean;
    onCancel?: () => void;
    onConfirm?: () => void;
  } = $props();
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
          <h2>Review Tune config plan</h2>
          <p>Snapshot will be created first. Uncheck patches you do not want applied.</p>
        </div>
        <button class="icon-btn" type="button" onclick={() => onCancel?.()} aria-label="Close">×</button>
      </div>
      <p class="plan-review-expl">{explanation}</p>
      <div class="plan-review-list">
        {#each rows as row (row.key)}
          <label class="plan-review-row">
            <input type="checkbox" bind:checked={row.selected} />
            <div class="plan-review-body">
              <div class="plan-review-top">
                <strong>{row.op}</strong>
                {#if row.path}<code>{row.path}</code>{/if}
                <span class="risk-pill">{row.risk}</span>
              </div>
              {#if row.reason}<p>{row.reason}</p>{/if}
              {#if row.patchPreview}
                <pre class="patch-preview">{row.patchPreview}</pre>
              {/if}
              {#if row.diffBefore != null || row.diffAfter != null}
                <details class="diff-details">
                  <summary>Preview diff</summary>
                  <div class="diff-grid">
                    <pre class="diff-before">{row.diffBefore ?? "(empty)"}</pre>
                    <pre class="diff-after">{row.diffAfter ?? "(empty)"}</pre>
                  </div>
                </details>
              {/if}
            </div>
          </label>
        {/each}
      </div>
      {#if needsAck}
        <label class="plan-review-ack">
          <input type="checkbox" bind:checked={acknowledged} />
          I reviewed these config changes (required)
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
          Apply {selectedCount} patch{selectedCount === 1 ? "" : "es"} (snapshot first)
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
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
    width: min(720px, 100%);
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
  .plan-review-list { display: flex; flex-direction: column; gap: 8px; margin-bottom: 12px; }
  .plan-review-row {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
    background: var(--bg-tertiary);
    cursor: pointer;
  }
  .plan-review-body { flex: 1; min-width: 0; }
  .plan-review-top { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin-bottom: 4px; }
  .plan-review-body p { margin: 4px 0; font-size: 12px; color: var(--text-secondary); }
  .plan-review-body code { font-size: 12px; color: var(--accent-primary); }
  .risk-pill {
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    padding: 2px 6px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
  }
  .patch-preview {
    margin: 6px 0 0;
    padding: 8px;
    font-size: 12px;
    overflow: auto;
    max-height: 120px;
    border-radius: var(--border-radius-sm);
    background: var(--bg-elevated);
    border: 1px solid var(--border-color);
  }
  .diff-details { margin-top: 8px; font-size: 12px; color: var(--text-muted); }
  .diff-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin-top: 6px;
  }
  .diff-before, .diff-after {
    margin: 0;
    padding: 8px;
    font-size: 12px;
    max-height: 180px;
    overflow: auto;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .diff-before { background: color-mix(in srgb, var(--accent-danger) 8%, transparent); }
  .diff-after { background: color-mix(in srgb, var(--accent-primary) 8%, transparent); }
  .plan-review-ack {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 12px;
    margin-bottom: 12px;
    color: var(--text-secondary);
  }
  .plan-review-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .icon-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 20px;
    cursor: pointer;
  }
  @media (max-width: 640px) {
    .diff-grid { grid-template-columns: 1fr; }
  }
</style>
