<script lang="ts">
  import { Handle, Position } from "@xyflow/svelte";
  import { iconDisplayId, type QuestData, type QuestProgressStatus } from "../../lib/api";
  import QuestItemIcon from "./QuestItemIcon.svelte";

  let {
    data,
  }: {
    data: {
      quest: QuestData;
      isIssue: boolean;
      isSelected: boolean;
      baseSize: number;
      progress: QuestProgressStatus | null;
      iconRevision: number;
      external?: boolean;
      chapterTitle?: string;
      chapterId?: string;
    };
  } = $props();

  let q = $derived(data.quest);
  let size = $derived(data.baseSize * (q.size && q.size > 0 ? q.size : 1));
  let prog = $derived(data.progress);
  let external = $derived(!!data.external);

  function glyph(quest: QuestData): string {
    const icon = iconDisplayId(quest.icon);
    if (icon) {
      const leaf = icon.includes(":") ? icon.split(":").pop()! : icon;
      return (leaf[0] || "?").toUpperCase();
    }
    return (quest.title[0] || "?").toUpperCase();
  }

  let iconId = $derived(iconDisplayId(q.icon));

  function nodeShape(quest: QuestData): string {
    const s = quest.shape?.trim();
    if (s && s !== "none") return s;
    return "rsquare";
  }

  let shape = $derived(nodeShape(q));
  let clipped = $derived(
    shape === "diamond" || shape === "hexagon" || shape === "pentagon" || shape === "gear",
  );
</script>

<div
  class="node-root relative flex flex-col items-center"
  class:sel={data.isSelected}
  class:issue={data.isIssue}
  class:external
>
  <div
    class="node-wrap flex flex-col items-center gap-2 select-none"
    class:sel={data.isSelected}
    class:issue={data.isIssue}
    class:external
    title={external && data.chapterTitle ? `${q.title} (${data.chapterTitle})` : q.title}
  >
    <div
      class="node-icon shape-{shape}"
      class:clipped
      class:optional={q.optional || external}
      class:prog-completed={prog === "completed"}
      class:prog-started={prog === "started"}
      class:prog-available={prog === "available"}
      class:prog-locked={prog === "locked"}
      style="width:{size}px; height:{size}px;"
    >
      <div class="node-face shape-{shape}">
        <QuestItemIcon
          itemId={iconId}
          fallback={glyph(q)}
          size={Math.max(12, Math.floor(size * 0.62))}
          revision={data.iconRevision}
        />
      </div>
      <!-- Handles live inside .node-icon so they anchor to the icon edges on
           any node size / label width — not to the measured root box. -->
      <Handle
        type="target"
        position={Position.Top}
        class="conn-handle"
        title={external ? undefined : "Drop here: this quest depends on the source"}
      />
      <Handle
        type="source"
        position={Position.Bottom}
        class="conn-handle"
        title={external ? undefined : "Drag to another quest’s top handle to create a dependency"}
      />
      {#if q.optional}<span class="opt">?</span>{/if}
      {#if prog === "completed"}<span class="check" title="Completed">✓</span>{/if}
    </div>
    <span class="node-label" title={q.title}>{q.title}</span>
    {#if external && data.chapterTitle}
      <span class="ch-badge">{data.chapterTitle}</span>
    {/if}
  </div>
</div>

<style>
  .node-root :global(.conn-handle) {
    width: 8px !important;
    height: 8px !important;
    min-width: 8px;
    min-height: 8px;
    background: var(--ftbq-accent-teal) !important;
    border: 2px solid var(--ftbq-frame, #3a3a42) !important;
    border-radius: 50%;
    /* Always faintly visible so connection points are discoverable;
       full opacity on hover/selection. */
    opacity: 0.35;
    transition: opacity 0.12s ease, transform 0.12s ease;
    z-index: 3;
  }
  /* Handles are pointer-events:none by default in xyflow until a connection
     drag starts; the canvas adds pointer-events during drags and hover so the
     dots are actually grabbable / droppable. */
  .node-root:hover:not(.external) :global(.conn-handle),
  .node-root.sel:not(.external) :global(.conn-handle) {
    opacity: 0.95;
  }
  .node-root:hover:not(.external) :global(.conn-handle):hover,
  .node-root.sel:not(.external) :global(.conn-handle):hover {
    transform: scale(1.25);
    background: var(--ftbq-accent-green) !important;
  }
  .node-root.external :global(.conn-handle) {
    pointer-events: none;
    opacity: 0 !important;
  }
  .node-wrap {
    /* layout moved to Tailwind utilities */
    cursor: grab;
    transition: transform 0.12s ease;
  }
  .node-wrap.external {
    cursor: default;
    opacity: 0.72;
  }
  .node-wrap:hover:not(.external) {
    transform: scale(1.06);
  }
  .node-wrap:active:not(.external) {
    cursor: grabbing;
  }
  .node-icon {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    border: 2px solid var(--ftbq-quest-default, #ffffff);
    background: transparent;
    color: var(--ftbq-text);
    box-shadow:
      0 0 0 2px rgba(0, 0, 0, 0.65),
      0 3px 8px rgba(0, 0, 0, 0.55);
  }
  .node-wrap.external .node-icon:not(.clipped) {
    border-style: dashed;
    border-color: var(--ftbq-quest-locked, #6b6b6b);
  }
  .node-wrap.sel .node-icon {
    border-color: var(--ftbq-title-gold);
    box-shadow:
      0 0 0 2px rgba(0, 0, 0, 0.65),
      0 0 12px 2px rgba(242, 201, 76, 0.5),
      0 3px 8px rgba(0, 0, 0, 0.55);
    animation: sel-pulse 1.6s ease-in-out infinite;
  }
  @keyframes sel-pulse {
    0%,
    100% {
      box-shadow:
        0 0 0 2px rgba(0, 0, 0, 0.65),
        0 0 8px 1px rgba(242, 201, 76, 0.35),
        0 3px 8px rgba(0, 0, 0, 0.55);
    }
    50% {
      box-shadow:
        0 0 0 2px rgba(0, 0, 0, 0.65),
        0 0 14px 3px rgba(242, 201, 76, 0.6),
        0 3px 8px rgba(0, 0, 0, 0.55);
    }
  }
  .node-wrap.issue .node-icon {
    border-color: var(--accent-danger);
    box-shadow:
      0 0 0 2px rgba(0, 0, 0, 0.65),
      0 0 10px 2px rgba(248, 113, 113, 0.45),
      0 3px 8px rgba(0, 0, 0, 0.55);
  }
  .node-icon.clipped {
    border-color: transparent;
    box-shadow: none;
    background: transparent;
    filter: drop-shadow(0 0 2px var(--ftbq-quest-default, #ffffff)) drop-shadow(0 3px 4px rgba(0, 0, 0, 0.6));
  }
  .node-wrap.sel .node-icon.clipped {
    border-color: transparent;
    animation: none;
    filter: drop-shadow(0 0 3px var(--ftbq-title-gold)) drop-shadow(0 3px 4px rgba(0, 0, 0, 0.6));
  }
  .node-icon.clipped.prog-completed {
    border-color: transparent;
    filter: drop-shadow(0 0 3px var(--ftbq-quest-completed)) drop-shadow(0 3px 4px rgba(0, 0, 0, 0.6));
  }
  .node-icon.clipped.prog-started {
    border-color: transparent;
    filter: drop-shadow(0 0 3px var(--ftbq-quest-started)) drop-shadow(0 3px 4px rgba(0, 0, 0, 0.6));
  }
  .node-icon.clipped.prog-locked {
    border-color: transparent;
    filter: drop-shadow(0 0 2px var(--ftbq-quest-locked, #6b6b6b)) drop-shadow(0 3px 4px rgba(0, 0, 0, 0.6));
  }
  .node-face {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background:
      radial-gradient(circle at 50% 32%, rgba(255, 255, 255, 0.07), transparent 62%),
      var(--ftbq-node-fill);
    box-shadow:
      inset 2px 2px 0 rgba(0, 0, 0, 0.55),
      inset -1px -1px 0 rgba(255, 255, 255, 0.09);
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
    border-radius: var(--ftbq-radius-control);
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
  .node-icon.optional:not(.clipped) {
    border-style: dashed;
  }
  .node-face :global(.qii),
  .node-face :global(.qii-ph) {
    max-width: 70%;
    max-height: 70%;
  }
  /* The label sizes .node-wrap (the wrap shrinks to its content), so an
     unbounded max-content width made long titles stretch the node far past its
     24px cell and collide with the neighbouring quests. Cap it at two layout
     units (2 * BASE 24px) — the clearance the auto-placer keeps between nodes
     and the narrowest chip that is still readable — so titles never overlap in
     hand-placed or laid-out books (the 3-unit default layouts then keep a
     comfortable gutter). Any tighter and the chip is unreadable, so the full
     title stays available through the node/label tooltip. */
  .node-label {
    font-size: clamp(8px, 10px, 11px);
    line-height: 1.15;
    max-width: 48px;
    min-width: 0;
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-primary, var(--ftbq-text));
    font-weight: 500;
    pointer-events: none;
    text-shadow: none;
    padding: 2px 8px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--bg-secondary, #fff) 80%, transparent);
    -webkit-backdrop-filter: blur(4px);
    backdrop-filter: blur(4px);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--border-color, #dee2e6) 55%, transparent);
  }
  .ch-badge {
    display: inline-block;
    vertical-align: baseline;
    font-size: 12px;
    line-height: 1.1;
    /* Same cap as the label: this span also sizes the wrap, so a percentage
       width only ever resolved against the already-stretched wrap. */
    max-width: 48px;
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ftbq-accent-teal);
    font-weight: 500;
    pointer-events: none;
    text-shadow: none;
    padding: 1px 6px;
    border: 1px dashed var(--ftbq-border);
    border-radius: 999px;
    background: color-mix(in srgb, var(--bg-secondary, #fff) 75%, transparent);
    -webkit-backdrop-filter: blur(4px);
    backdrop-filter: blur(4px);
  }
  .node-wrap:hover .node-label {
    color: var(--text-primary, var(--ftbq-text));
    background: color-mix(in srgb, var(--bg-secondary, #fff) 92%, transparent);
  }
  .node-wrap.sel .node-label {
    color: var(--text-primary, var(--ftbq-text));
    font-weight: 600;
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent-primary) 45%, transparent);
  }
  .opt {
    position: absolute;
    top: -3px;
    right: -3px;
    font-size: 12px;
    color: var(--ftbq-quest-started);
    font-weight: 900;
    text-shadow: none;
  }
  .check {
    position: absolute;
    bottom: -5px;
    right: -5px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: linear-gradient(180deg, var(--ftbq-accent-green), color-mix(in srgb, var(--ftbq-accent-green) 70%, #000));
    border: 1px solid var(--ftbq-bg);
    color: var(--ftbq-bg);
    font-size: 12px;
    font-weight: 900;
    display: flex;
    align-items: center;
    justify-content: center;
    line-height: 1;
    box-shadow:
      0 0 0 1px rgba(0, 0, 0, 0.6),
      0 0 6px rgba(85, 201, 90, 0.55),
      inset 0 1px 0 rgba(255, 255, 255, 0.35);
  }
  .node-icon.prog-completed {
    border-color: var(--ftbq-quest-completed);
    box-shadow:
      0 0 0 2px rgba(0, 0, 0, 0.65),
      0 0 10px 2px rgba(85, 201, 90, 0.4),
      0 3px 8px rgba(0, 0, 0, 0.55);
  }
  .node-icon.prog-started {
    border-color: var(--ftbq-quest-started);
    box-shadow:
      0 0 0 2px rgba(0, 0, 0, 0.65),
      0 0 10px 2px rgba(242, 201, 76, 0.4),
      0 3px 8px rgba(0, 0, 0, 0.55);
  }
  .node-icon.prog-available {
    border-color: var(--ftbq-quest-default, #ffffff);
  }
  .node-icon.prog-locked {
    border-color: var(--ftbq-quest-locked, #6b6b6b);
    opacity: 0.5;
    filter: grayscale(0.5);
  }
</style>
