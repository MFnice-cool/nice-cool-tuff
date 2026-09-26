<script lang="ts">
  import { Handle, Position } from "@xyflow/svelte";
  import type { QuestData } from "../../lib/store";

  let {
    data,
  }: {
    data: {
      quest: QuestData;
      isIssue: boolean;
      isSelected: boolean;
      baseSize: number;
    };
  } = $props();

  let q = $derived(data.quest);
  let size = $derived(data.baseSize * (q.size && q.size > 0 ? q.size : 1));

  function glyph(quest: QuestData): string {
    const icon = quest.icon?.trim();
    if (icon) {
      const leaf = icon.includes(":") ? icon.split(":").pop()! : icon;
      return (leaf[0] || "?").toUpperCase();
    }
    return (quest.title[0] || "?").toUpperCase();
  }

  /** Resolve FTB shape; empty → rsquare. `none` is a real shape (no frame). */
  function nodeShape(quest: QuestData): string {
    const s = quest.shape?.trim();
    if (!s) return "rsquare";
    return s;
  }

  let shape = $derived(nodeShape(q));
  let clipped = $derived(
    shape === "diamond" || shape === "hexagon" || shape === "pentagon" || shape === "gear",
  );
</script>

<Handle type="target" position={Position.Top} style="opacity: 0; width: 6px; height: 6px;" />
<Handle type="source" position={Position.Bottom} style="opacity: 0; width: 6px; height: 6px;" />

<div
  class="node-wrap"
  class:sel={data.isSelected}
  class:issue={data.isIssue}
  title={q.title}
>
  <div
    class="node-icon shape-{shape}"
    class:clipped
    class:optional={q.optional}
    style="width:{size}px; height:{size}px;"
  >
    <div class="node-face shape-{shape}">
      <span class="glyph" style="font-size:{Math.max(10, Math.floor(size * 0.5))}px">{glyph(q)}</span>
    </div>
    {#if q.optional}<span class="opt">?</span>{/if}
  </div>
  <span class="node-label">{q.title}</span>
</div>

<style>
  .node-wrap {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    cursor: grab;
    user-select: none;
  }
  .node-wrap.sel .node-icon {
    outline: 2px solid var(--node-selected);
    outline-offset: 1px;
  }
  .node-wrap.issue .node-icon {
    border-color: var(--warning);
  }
  .node-icon {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    border: 2px solid var(--node-border);
    background: transparent;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.4);
  }
  .node-icon.optional:not(.clipped):not(.shape-none) {
    border-style: dashed;
  }
  .node-icon.clipped {
    border-color: transparent;
    box-shadow: none;
    background: transparent;
    filter: drop-shadow(0 0 1px var(--node-border)) drop-shadow(0 1px 3px rgba(0, 0, 0, 0.45));
  }
  .node-icon.shape-none {
    border-color: transparent;
    box-shadow: none;
    background: transparent;
  }
  .node-icon.shape-circle,
  .node-face.shape-circle {
    border-radius: 50%;
  }
  .node-icon.shape-square,
  .node-face.shape-square {
    border-radius: 0;
  }
  .node-icon.shape-rsquare,
  .node-face.shape-rsquare {
    border-radius: 4px;
  }
  .node-face {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--node-bg);
    box-shadow: inset 0 2px 6px rgba(0, 0, 0, 0.5);
  }
  .node-face.shape-none {
    background: transparent;
    box-shadow: none;
  }
  .node-face.shape-diamond {
    border-radius: 0;
    clip-path: polygon(50% 0%, 100% 50%, 50% 100%, 0% 50%);
  }
  .node-face.shape-hexagon {
    clip-path: polygon(25% 0%, 75% 0%, 100% 50%, 75% 100%, 25% 100%, 0% 50%);
    border-radius: 0;
  }
  .node-face.shape-pentagon {
    clip-path: polygon(50% 0%, 100% 38%, 82% 100%, 18% 100%, 0% 38%);
    border-radius: 0;
  }
  .node-face.shape-gear {
    border-radius: 3px;
    clip-path: polygon(
      50% 0%,
      61% 8%,
      75% 4%,
      82% 18%,
      96% 25%,
      92% 39%,
      100% 50%,
      92% 61%,
      96% 75%,
      82% 82%,
      75% 96%,
      61% 92%,
      50% 100%,
      39% 92%,
      25% 96%,
      18% 82%,
      4% 75%,
      8% 61%,
      0% 50%,
      8% 39%,
      4% 25%,
      18% 18%,
      25% 4%,
      39% 8%
    );
  }
  .glyph {
    color: var(--text-primary);
    font-weight: 800;
  }
  .node-label {
    font-size: clamp(8px, 10px, 11px);
    line-height: 1.15;
    max-width: calc(100% + 24px);
    min-width: 100%;
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-muted);
    pointer-events: none;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.8);
  }
  .node-wrap.sel .node-label {
    color: var(--text-primary);
  }
  .opt {
    position: absolute;
    top: -3px;
    right: -3px;
    font-size: 9px;
    color: var(--warning);
    font-weight: 900;
  }
</style>
