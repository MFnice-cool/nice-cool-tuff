<script lang="ts">
  import {
    SvelteFlow,
    Background,
    Controls,
    MiniMap,
    BackgroundVariant,
    useSvelteFlow,
    type Node,
    type Edge,
    type Connection,
  } from "@xyflow/svelte";
  import "@xyflow/svelte/dist/style.css";
  import { tick } from "svelte";
  import type { QuestData, QuestValidationIssue } from "../../lib/store";
  import QuestNode from "./QuestNode.svelte";

  let {
    quests,
    selectedId = null,
    selectedIds = new Set(),
    issues,
    emptyHint = "Double-click to add a quest",
    onSelect,
    onMove,
    onAddAt,
    onLink,
    onSelectMultiple,
    fitToken = 0,
  }: {
    quests: QuestData[];
    selectedId?: string | null;
    selectedIds?: Set<string>;
    issues: QuestValidationIssue[];
    emptyHint?: string;
    onSelect: (q: QuestData | null, e?: MouseEvent) => void;
    onMove: (q: QuestData, x: number, y: number) => void;
    onAddAt: (x: number, y: number) => void;
    onLink: (fromId: string, toDepId: string) => void;
    onSelectMultiple: (ids: string[]) => void;
    fitToken?: number;
  } = $props();

  const BASE = 24;
  const nodeTypes = { quest: QuestNode };

  let issueIds = $derived(new Set(issues.map((i) => i.questId)));

  // Svelte Flow internal state
  const { screenToFlowPosition, fitView: flowFitView } = useSvelteFlow();

  // Convert quests → Svelte Flow nodes/edges
  let nodes = $state<Node[]>([]);
  let edges = $state<Edge[]>([]);

  $effect(() => {
    const newNodes: Node[] = quests.map((q) => ({
      id: q.id,
      type: "quest",
      position: { x: q.x * BASE, y: q.y * BASE },
      data: {
        quest: q,
        isIssue: issueIds.has(q.id),
        isSelected: selectedId === q.id || selectedIds.has(q.id),
        baseSize: BASE,
      },
      selected: selectedId === q.id || selectedIds.has(q.id),
    }));

    const newEdges: Edge[] = [];
    for (const q of quests) {
      for (const depId of q.dependencies) {
        // Check if the dependency target exists (quest or task id)
        const targetExists = quests.some((oq) => oq.id === depId) ||
          quests.some((oq) => oq.tasks?.some((t) => t.id === depId));

        newEdges.push({
          id: `e-${depId}-${q.id}`,
          source: depId,
          target: q.id,
          type: "step",
          style: targetExists
            ? "stroke: #5c8a9e; stroke-width: 3;"
            : "stroke: var(--warning); stroke-width: 2.5; stroke-dasharray: 6 4;",
        });
      }
    }

    nodes = newNodes;
    edges = newEdges;
  });

  // Sync selection back from Svelte Flow
  $effect(() => {
    const selectedNodes = nodes.filter((n) => n.selected);
    if (selectedNodes.length > 0) {
      const ids = selectedNodes.map((n) => n.id);
      onSelectMultiple(ids);
    }
  });

  // Fit view when fitToken changes
  let lastFitToken = $state(-1);
  $effect(() => {
    if (fitToken !== lastFitToken) {
      lastFitToken = fitToken;
      tick().then(() => flowFitView({ padding: 0.2 }));
    }
  });

  // Event handlers
  function handleNodeDragStop(_event: MouseEvent, node: Node) {
    const q = quests.find((q) => q.id === node.id);
    if (q) {
      onMove(q, Math.round(node.position.x / BASE), Math.round(node.position.y / BASE));
    }
  }

  function handleConnect(connection: Connection) {
    if (connection.source && connection.target) {
      onLink(connection.source, connection.target);
    }
  }

  function handlePaneClick(event: MouseEvent) {
    onSelect(null);
  }

  function handlePaneDoubleClick(event: MouseEvent) {
    const pos = screenToFlowPosition({ x: event.clientX, y: event.clientY });
    onAddAt(Math.round(pos.x / BASE), Math.round(pos.y / BASE));
  }

  function handleNodeClick(_event: MouseEvent, node: Node) {
    const q = quests.find((q) => q.id === node.id);
    if (q) onSelect(q, _event);
  }

  /** Canvas MiniMap cannot resolve CSS custom properties — use computed hex. */
  function cssColor(name: string, fallback: string): string {
    if (typeof document === "undefined") return fallback;
    const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    return v || fallback;
  }

  function miniNodeColor(n: Node): string {
    if (n.type !== "quest") return "#666666";
    const q = quests.find((item) => item.id === n.id);
    if (!q) return "#666666";
    if (issueIds.has(q.id)) return cssColor("--warning", "#fbbf24");
    return cssColor("--node-bg", "#18181c");
  }
</script>

<div class="canvas-wrap">
  <div class="canvas-toolbar">
    <button type="button" class="tb" onclick={() => flowFitView({ padding: 0.2 })}>⊞ Fit</button>
    <button
      type="button"
      class="tb"
      onclick={() => {
        // Add quest at center of viewport
        const viewportEl = document.querySelector(".xyflow__viewport");
        if (viewportEl) {
          const rect = viewportEl.getBoundingClientRect();
          const cx = rect.left + rect.width / 2;
          const cy = rect.top + rect.height / 2;
          const pos = screenToFlowPosition({ x: cx, y: cy });
          onAddAt(Math.round(pos.x / BASE), Math.round(pos.y / BASE));
        } else {
          onAddAt(0, 0);
        }
      }}
    >+ Add</button>
    <span class="hint">Drag · Scroll zoom · Shift+drag link · Dbl-click add · Ctrl+click multi-select</span>
  </div>

  <div class="viewport">
    {#if quests.length === 0}
      <div class="empty-hint">{emptyHint}</div>
    {/if}

    <SvelteFlow
      {nodes}
      {edges}
      {nodeTypes}
      panOnScroll
      selectionOnDrag
      panOnDrag={[1, 2]}
      deleteKey={null}
      onnodeclick={handleNodeClick}
      onpaneclick={handlePaneClick}
      onpanedoubleclick={handlePaneDoubleClick}
      onnodedragstop={handleNodeDragStop}
      onconnect={handleConnect}
      fitView
      fitViewOptions={{ padding: 0.2 }}
      defaultEdgeOptions={{ type: "step", style: "stroke: #5c8a9e; stroke-width: 3;" }}
    >
      <Background variant={BackgroundVariant.Dots} gap={15} size={1} />
      <Controls />
      {#if quests.length > 0}
        <MiniMap nodeColor={miniNodeColor} />
      {/if}
    </SvelteFlow>
  </div>
</div>

<style>
  .canvas-wrap {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-tertiary);
    overflow: hidden;
  }
  .canvas-toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 8px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-secondary);
    flex-shrink: 0;
  }
  .tb {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 10px;
    border-radius: 2px;
    border: 1px solid var(--border);
    background: rgba(0,0,0,0.25);
    color: var(--text-primary);
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }
  .tb:hover { border-color: var(--accent); background: rgba(61,184,168,0.12); }
  .hint { margin-left: auto; font-size: 9px; color: var(--text-muted); }
  .viewport {
    position: relative;
    flex: 1;
    overflow: hidden;
  }
  .empty-hint {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    font-size: 12px;
    pointer-events: none;
    z-index: 10;
  }

  /* Svelte Flow theme overrides */
  :global(.svelte-flow__background) {
    background-color: var(--bg-canvas);
  }
  :global(.svelte-flow__edge path) {
    stroke-width: 3;
  }
  :global(.svelte-flow__controls) {
    box-shadow: 0 1px 4px rgba(0,0,0,0.3);
  }
  :global(.svelte-flow__controls button) {
    background: var(--bg-secondary);
    border-color: var(--border);
    color: var(--text-primary);
  }
  :global(.svelte-flow__controls button:hover) {
    background: rgba(61,184,168,0.12);
  }
  :global(.svelte-flow__minimap) {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 4px;
  }
  :global(.svelte-flow__attribution) {
    display: none;
  }
</style>
