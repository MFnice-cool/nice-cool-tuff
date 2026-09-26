<script lang="ts">
  import { X } from "@lucide/svelte";
  import { trapFocus } from "../lib/focusTrap";

  let { onclose }: { onclose?: () => void } = $props();

  const shortcuts = [
    { keys: ["Ctrl", "K"], label: "Command palette (instances, stages, actions)" },
    { keys: ["Ctrl", "1"], label: "Home" },
    { keys: ["Ctrl", "2"], label: "Open IDE" },
    { keys: ["Ctrl", "3"], label: "IDE · Content" },
    { keys: ["Ctrl", "4"], label: "IDE · Resolve" },
    { keys: ["Ctrl", "5"], label: "IDE · Configs" },
    { keys: ["Ctrl", "6"], label: "IDE · Diagnose" },
    { keys: ["Ctrl", "7"], label: "IDE · Snapshots" },
    { keys: ["Ctrl", "8"], label: "IDE · World map" },
    { keys: ["Ctrl", "9"], label: "Library" },
    { keys: ["Ctrl", ","], label: "Settings" },
    { keys: ["Ctrl", "Shift", "P"], label: "IDE · Play / Test launch" },
    { keys: ["["], label: "IDE · Prev stage in phase" },
    { keys: ["]"], label: "IDE · Next stage in phase" },
    { keys: ["?"], label: "Show shortcuts" },
  ];
</script>

<div class="kh-backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose?.()}>
  <div class="kh-dialog" role="dialog" aria-modal="true" aria-label="Keyboard shortcuts" use:trapFocus={{ onEscape: () => onclose?.() }}>
    <h3>Keyboard Shortcuts</h3>
    <div class="kh-list">
      {#each shortcuts as s}
        <div class="kh-row">
          <span class="kh-label">{s.label}</span>
          <span class="kh-keys">
            {#each s.keys as k, i}{#if i > 0}<span class="kh-plus">+</span>{/if}<kbd>{k}</kbd>{/each}
          </span>
        </div>
      {/each}
    </div>
    <p class="kh-hint">Press <kbd>?</kbd> anywhere to toggle this panel</p>
    <button class="kh-close" onclick={() => onclose?.()} aria-label="Close shortcuts">
      <X size={16} />
    </button>
  </div>
</div>

<style>
  .kh-backdrop {
    position: fixed; inset: 0; background: rgba(0, 0, 0, 0.55);
    display: flex; align-items: center; justify-content: center;
    z-index: 210; backdrop-filter: blur(8px);
  }

  .kh-dialog {
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-xl);
    padding: 28px 32px;
    width: 400px;
    box-shadow: var(--shadow-lg);
    position: relative;
  }

  .kh-dialog h3 {
    font-size: 18px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0 0 18px;
  }

  .kh-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .kh-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 0;
    border-bottom: 1px solid rgba(255, 255, 255, 0.04);
  }

  .kh-row:last-child {
    border-bottom: none;
  }

  .kh-label {
    font-size: 14px;
    color: var(--text-secondary);
  }

  .kh-keys {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 28px;
    height: 26px;
    padding: 0 8px;
    background: var(--bg-primary);
    border: 1px solid var(--border-color);
    border-radius: 6px;
    font-family: inherit;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
  }

  .kh-plus {
    font-size: 12px;
    color: var(--text-muted);
    margin: 0 1px;
  }

  .kh-hint {
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
    margin: 16px 0 0;
  }

  .kh-hint kbd {
    min-width: 20px;
    height: 20px;
    padding: 0 5px;
    font-size: 12px;
    vertical-align: middle;
  }

  .kh-close {
    position: absolute;
    top: 12px;
    right: 12px;
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    border-radius: var(--border-radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .kh-close:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }
</style>
