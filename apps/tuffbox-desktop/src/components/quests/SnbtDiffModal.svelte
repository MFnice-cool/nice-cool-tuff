<script lang="ts">
  import { unifiedDiffLines, type DiffLine, type SnbtDiffFile } from "../../lib/snbtDiff";
  import { trapFocus } from "../../lib/focusTrap";

  let {
    open = false,
    title = "Review SNBT changes",
    leftLabel = "Disk",
    rightLabel = "Editor",
    leftText = "",
    rightText = "",
    files = null as SnbtDiffFile[] | null,
    confirmLabel = "Save",
    onConfirm,
    onCancel,
  }: {
    open?: boolean;
    title?: string;
    leftLabel?: string;
    rightLabel?: string;
    leftText?: string;
    rightText?: string;
    files?: SnbtDiffFile[] | null;
    confirmLabel?: string;
    onConfirm?: () => void;
    onCancel?: () => void;
  } = $props();

  let activeIndex = $state(0);

  $effect(() => {
    if (open) activeIndex = 0;
  });

  let fileList = $derived.by<SnbtDiffFile[]>(() => {
    if (files && files.length > 0) return files;
    return [
      {
        id: "_single",
        label: leftLabel || "file",
        leftText,
        rightText,
        leftLabel,
        rightLabel,
      },
    ];
  });

  let active = $derived(fileList[Math.min(activeIndex, fileList.length - 1)] ?? fileList[0]!);

  let lines = $derived.by<DiffLine[]>(() => {
    if (!open || !active) return [];
    return unifiedDiffLines(active.leftText, active.rightText);
  });

  let stats = $derived.by(() => {
    let added = 0;
    let removed = 0;
    for (const l of lines) {
      if (l.kind === "add") added++;
      else if (l.kind === "del") removed++;
    }
    return { added, removed };
  });

  let totalStats = $derived.by(() => {
    let added = 0;
    let removed = 0;
    for (const f of fileList) {
      for (const l of unifiedDiffLines(f.leftText, f.rightText)) {
        if (l.kind === "add") added++;
        else if (l.kind === "del") removed++;
      }
    }
    return { added, removed, files: fileList.length };
  });

  function lineClass(kind: DiffLine["kind"]): string {
    if (kind === "add") return "added";
    if (kind === "del") return "removed";
    return "same";
  }

  function prefix(kind: DiffLine["kind"]): string {
    if (kind === "add") return "+";
    if (kind === "del") return "-";
    return " ";
  }
</script>

{#if open}
  <div
    class="modal-backdrop"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && onCancel?.()}
  >
    <div
      class="modal snbt-diff-modal"
      class:multi={fileList.length > 1}
      role="dialog"
      aria-modal="true"
      aria-labelledby="snbt-diff-title"
      use:trapFocus={{ onEscape: () => onCancel?.() }}
    >
      <div class="modal-header">
        <div>
          <h2 id="snbt-diff-title">{title}</h2>
          <p>
            {#if fileList.length > 1}
              {totalStats.files} files · +{totalStats.added} / −{totalStats.removed}
              {#if active}
                · viewing {active.leftLabel ?? "Disk"} → {active.rightLabel ?? "Editor"}
                (+{stats.added}/−{stats.removed})
              {/if}
            {:else}
              {active?.leftLabel ?? leftLabel} → {active?.rightLabel ?? rightLabel}
              · +{stats.added} / −{stats.removed}
            {/if}
          </p>
        </div>
        <button class="icon-btn" type="button" onclick={() => onCancel?.()} aria-label="Close">×</button>
      </div>
      <div class="diff-shell">
        {#if fileList.length > 1}
          <aside class="file-list">
            {#each fileList as f, i (f.id)}
              <button
                type="button"
                class="file-btn"
                class:active={i === activeIndex}
                onclick={() => (activeIndex = i)}
              >
                {f.label}
              </button>
            {/each}
          </aside>
        {/if}
        <pre class="diff-body"><code
          >{#each lines as line, i (`${i}-${line.kind}-${line.text.slice(0, 24)}`)}<span
              class={lineClass(line.kind)}>{prefix(line.kind)}{line.text}
</span>{/each}</code
        ></pre>
      </div>
      <div class="modal-actions">
        <button class="ghost" type="button" onclick={() => onCancel?.()}>Cancel</button>
        <button class="primary" type="button" onclick={() => onConfirm?.()}>{confirmLabel}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 90;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.55);
    padding: 16px;
  }
  .snbt-diff-modal {
    width: min(920px, 96vw);
    max-height: min(88vh, 900px);
    display: flex;
    flex-direction: column;
    gap: 12px;
    background: var(--bg-secondary, var(--ftbq-bg));
    border: 1px solid var(--border-color, var(--ftbq-border));
    border-radius: var(--border-radius-lg);
    padding: 16px;
    box-shadow: 0 18px 48px rgba(0, 0, 0, 0.45);
  }
  .snbt-diff-modal.multi {
    width: min(1100px, 96vw);
  }
  .modal-header {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    align-items: flex-start;
  }
  .modal-header h2 {
    margin: 0;
    font-size: 16px;
    color: var(--text-primary);
  }
  .modal-header p {
    margin: 4px 0 0;
    font-size: 12px;
    color: var(--text-muted);
  }
  .icon-btn {
    border: 0;
    background: transparent;
    color: var(--text-muted);
    font-size: 20px;
    line-height: 1;
    cursor: pointer;
    padding: 2px 6px;
  }
  .diff-shell {
    display: flex;
    gap: 10px;
    min-height: 0;
    flex: 1;
  }
  .file-list {
    width: 200px;
    flex-shrink: 0;
    overflow: auto;
    max-height: 58vh;
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-right: 1px solid var(--border-color, var(--ftbq-border));
    padding-right: 8px;
  }
  .file-btn {
    text-align: left;
    padding: 7px 8px;
    border: 1px solid transparent;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-secondary, #c4c4c8);
    font-size: 12px;
    cursor: pointer;
  }
  .file-btn:hover,
  .file-btn.active {
    background: var(--bg-tertiary, var(--ftbq-bg-panel));
    border-color: rgba(61, 184, 168, 0.35);
    color: var(--text-primary);
  }
  .diff-body {
    flex: 1;
    min-width: 0;
    min-height: 200px;
    max-height: 58vh;
    overflow: auto;
    margin: 0;
    padding: 12px;
    border-radius: var(--border-radius-md);
    background: var(--bg-elevated);
    border: 1px solid var(--border-color, var(--ftbq-border));
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
    line-height: 1.45;
    white-space: pre;
  }
  .diff-body code {
    display: block;
  }
  .same {
    display: block;
    color: var(--text-muted);
  }
  .added {
    display: block;
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
  }
  .removed {
    display: block;
    color: var(--accent-danger);
    background: color-mix(in srgb, var(--accent-danger) 8%, transparent);
  }
  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .modal-actions button {
    padding: 8px 14px;
    border-radius: var(--border-radius-sm);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    border: 1px solid var(--border-color, var(--ftbq-border));
  }
  .modal-actions .ghost {
    background: transparent;
    color: var(--text-secondary, #c4c4c8);
  }
  .modal-actions .primary {
    background: var(--accent);
    border-color: transparent;
    color: var(--ftbq-bg);
  }
</style>
