<script lang="ts">
  let {
    isOpen,
    onClose,
  }: {
    isOpen: boolean;
    onClose: () => void;
  } = $props();

  const shortcuts = [
    { category: "General", items: [
      { keys: ["Ctrl", "S"], action: "Export all chapters" },
      { keys: ["Ctrl", "Z"], action: "Undo" },
      { keys: ["Ctrl", "Shift", "Z"], action: "Redo" },
      { keys: ["Ctrl", "F"], action: "Search quests" },
      { keys: ["Ctrl", "0"], action: "Zoom to fit" },
      { keys: ["Esc"], action: "Deselect / Close panel" },
    ]},
    { category: "Editing", items: [
      { keys: ["Ctrl", "C"], action: "Copy selected quests" },
      { keys: ["Ctrl", "V"], action: "Paste quests" },
      { keys: ["Ctrl", "A"], action: "Select all quests" },
      { keys: ["Del"], action: "Delete selected quests" },
      { keys: ["Arrow keys"], action: "Nudge selected quests (1px)" },
      { keys: ["Shift", "Arrow"], action: "Nudge selected quests (5px)" },
    ]},
    { category: "Canvas", items: [
      { keys: ["Space", "Drag"], action: "Pan canvas" },
      { keys: ["Scroll"], action: "Zoom in/out" },
      { keys: ["Double-click"], action: "Add new quest" },
      { keys: ["Shift", "Drag"], action: "Link quests (create dependency)" },
      { keys: ["Click + Drag"], action: "Marquee select" },
    ]},
  ];
</script>

{#if isOpen}
  <div class="overlay" onclick={onClose} role="dialog" aria-modal="true">
    <div class="modal" onclick={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <h2>Keyboard Shortcuts</h2>
        <button type="button" class="close" onclick={onClose}>×</button>
      </div>
      <div class="modal-body">
        {#each shortcuts as section}
          <div class="section">
            <h3>{section.category}</h3>
            <div class="shortcut-list">
              {#each section.items as item}
                <div class="shortcut-row">
                  <div class="keys">
                    {#each item.keys as key, i}
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
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 8px;
    width: 480px;
    max-height: 80vh;
    overflow: auto;
    box-shadow: 0 8px 32px rgba(0,0,0,0.5);
  }
  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--border);
  }
  .modal-header h2 {
    font-size: 16px;
    font-weight: 700;
    color: var(--text-primary);
  }
  .close {
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: 20px;
    cursor: pointer;
    padding: 4px 8px;
    border-radius: 4px;
  }
  .close:hover { background: rgba(255,255,255,0.1); color: var(--text-primary); }
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
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--accent);
    margin-bottom: 8px;
    font-weight: 700;
  }
  .shortcut-list {
    display: grid;
    gap: 6px;
  }
  .shortcut-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 8px;
    border-radius: 4px;
    background: rgba(0,0,0,0.15);
  }
  .shortcut-row:hover {
    background: rgba(0,0,0,0.25);
  }
  .keys {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  kbd {
    display: inline-block;
    padding: 3px 8px;
    font-size: 11px;
    font-family: monospace;
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-primary);
    min-width: 24px;
    text-align: center;
  }
  .plus {
    font-size: 10px;
    color: var(--text-muted);
  }
  .action {
    font-size: 12px;
    color: var(--text-secondary);
  }
</style>
