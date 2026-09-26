<script lang="ts">
  import { trapFocus } from "../../lib/focusTrap";

  let {
    isOpen,
    onClose,
  }: {
    isOpen: boolean;
    onClose: () => void;
  } = $props();

  const shortcuts = [
    { category: "General", items: [
      { keys: ["Ctrl", "S"], action: "Save all" },
      { keys: ["Ctrl", "/"], action: "This shortcuts dialog" },
      { keys: ["Ctrl", "Z"], action: "Undo" },
      { keys: ["Ctrl", "Shift", "Z"], action: "Redo" },
      { keys: ["Ctrl", "F"], action: "Search quests (/regex/ or re:…)" },
      { keys: ["Ctrl", "0"], action: "Zoom to fit" },
      { keys: ["Esc"], action: "Close menus / drawers / AI / search / deselect" },
    ]},
    { category: "Editing", items: [
      { keys: ["Ctrl", "C"], action: "Copy selected quests" },
      { keys: ["Ctrl", "V"], action: "Paste quests" },
      { keys: ["Ctrl", "A"], action: "Select all quests" },
      { keys: ["Del"], action: "Delete selected quests" },
      { keys: ["Arrow keys"], action: "Nudge selected (1px) when canvas not focused" },
      { keys: ["Shift", "Arrow"], action: "Nudge selected (5px)" },
    ]},
    { category: "Canvas", items: [
      { keys: ["Arrow keys"], action: "Select next/previous quest (canvas focused)" },
      { keys: ["Home", "End"], action: "Jump to first/last quest" },
      { keys: ["Esc"], action: "Clear canvas selection" },
      { keys: ["Alt", "Click"], action: "Toggle simulate complete (Simulate mode)" },
      { keys: ["Space", "Drag"], action: "Pan canvas" },
      { keys: ["Scroll"], action: "Zoom in/out" },
      { keys: ["N"], action: "Add quest at view center" },
      { keys: ["Double-click"], action: "Add new quest" },
      { keys: ["Drag handle"], action: "Link quests (bottom → top handle)" },
      { keys: ["Click edge", "Del"], action: "Unlink selected dependency" },
      { keys: ["Shift", "Click"], action: "Add/remove from multi-select" },
      { keys: ["Click + Drag"], action: "Marquee select" },
    ]},
    { category: "Quest AI", items: [
      { keys: ["Ctrl", "Enter"], action: "Send / Generate (composer)" },
      { keys: ["Arrow keys"], action: "Move between intent radios" },
    ]},
  ];
</script>

{#if isOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="overlay" role="presentation" onclick={onClose}>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="modal"
      role="dialog"
      aria-modal="true"
      aria-labelledby="shortcuts-title"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      use:trapFocus={{ onEscape: onClose }}
    >
      <div class="modal-header">
        <h2 id="shortcuts-title">Keyboard Shortcuts</h2>
        <button type="button" class="close" onclick={onClose} aria-label="Close">×</button>
      </div>
      <div class="modal-body">
        {#each shortcuts as section (section.category)}
          <div class="section">
            <h3>{section.category}</h3>
            <div class="shortcut-list">
              {#each section.items as item (item.action)}
                <div class="shortcut-row">
                  <div class="keys">
                    {#each item.keys as key, i (`${item.action}-${i}`)}
                      {#if i > 0}<span class="plus">+</span>{/if}
                      <kbd>{key}</kbd>
                    {/each}
                  </div>
                  <span class="action">{item.action}</span>
                </div>
              {/each}
            </div>
          </div>
        {/each}
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0,0,0,0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .modal {
    background: var(--ftbq-bg-panel, #212126);
    border: 1px solid var(--ftbq-border, #3a3a42);
    border-radius: 3px;
    width: min(640px, 96vw);
    max-height: 80vh;
    overflow: auto;
    box-shadow: 0 8px 32px rgba(0,0,0,0.5);
  }
  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--ftbq-border, #3a3a42);
  }
  .modal-header h2 {
    font-size: 16px;
    font-weight: 700;
    color: var(--ftbq-title-gold, #f2c94c);
    margin: 0;
  }
  .close {
    border: none;
    background: transparent;
    color: var(--ftbq-text-muted, #9a9aa0);
    font-size: 20px;
    cursor: pointer;
    padding: 4px 8px;
    border-radius: 3px;
  }
  .close:hover {
    background: rgba(255,255,255,0.1);
    color: var(--ftbq-text, #e8e8e8);
  }
  .modal-body {
    padding: 16px 20px;
  }
  .section {
    margin-bottom: 20px;
  }
  .section:last-child {
    margin-bottom: 0;
  }
  .section h3 {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--ftbq-accent-teal, #3db8a8);
    margin-bottom: 8px;
    font-weight: 700;
  }
  .shortcut-list {
    display: grid;
    gap: 8px;
  }
  .shortcut-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 8px;
    border-radius: 3px;
    background: rgba(0,0,0,0.15);
  }
  .shortcut-row:hover {
    background: rgba(0,0,0,0.25);
  }
  .keys {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  kbd {
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    display: inline-block;
    padding: 3px 8px;
    font-size: 12px;
    font-family: var(--font-mono, ui-monospace, monospace);
    background: var(--ftbq-bg, #1a1a1e);
    border: 1px solid var(--ftbq-border, #3a3a42);
    border-radius: 3px;
    color: var(--ftbq-text, #e8e8e8);
    min-width: 24px;
    text-align: center;
  }
  .plus {
    font-size: 12px;
    color: var(--ftbq-text-muted, #9a9aa0);
  }
  .action {
    font-size: 12px;
    color: var(--ftbq-text-muted, #9a9aa0);
  }
</style>
