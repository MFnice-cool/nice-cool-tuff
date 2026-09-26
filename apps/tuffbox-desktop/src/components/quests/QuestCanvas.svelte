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
  import { Maximize2, Plus, LayoutGrid, ChevronDown, AlignLeft, Search } from "@lucide/svelte";
  import {
    iconDisplayId,
    type QuestChapter,
    type QuestData,
    type QuestValidationIssue,
    type QuestProgressStatus,
  } from "../../lib/api";
  import { projectPath } from "../../lib/store";
  import { preloadItemIcons } from "./iconCache";
  import QuestNode from "./QuestNode.svelte";
  import { getWorldCoordinates, rectsIntersect } from "./coords";
  import { alignQuests, distributeQuests } from "@tuffbox/quest-lib";

  let {
    quests,
    chapters = [],
    selectedId = null,
    selectedIds = new Set<string>(),
    issues,
    progressStatuses = {},
    progressOverlay = false,
    emptyHint = "Double-click to add a quest",
    showEmptyAddCta = false,
    onSelect,
    onMove,
    onAddAt,
    onLink,
    onUnlink = undefined,
    onEdgeSelect = undefined,
    onOpenChapter = undefined,
    onSelectMultiple = () => {},
    fitToken = 0,
    addQuestToken = 0,
    questFilter = "",
    filterTotal = 0,
    onQuestFilterChange = undefined,
    onApplyLayout = undefined,
    onAlign = undefined,
    onDistribute = undefined,
  }: {
    quests: QuestData[];
    chapters?: QuestChapter[];
    selectedId?: string | null;
    selectedIds?: Set<string>;
    issues: QuestValidationIssue[];
    progressStatuses?: Record<string, QuestProgressStatus>;
    progressOverlay?: boolean;
    emptyHint?: string;
    showEmptyAddCta?: boolean;
    onSelect: (q: QuestData | null, e?: MouseEvent) => void;
    onMove: (q: QuestData, x: number, y: number) => void;
    onAddAt: (x: number, y: number) => void;
    onLink: (fromId: string, toDepId: string) => void;
    onUnlink?: (questId: string, depId: string) => void;
    onEdgeSelect?: (edge: { questId: string; depId: string } | null) => void;
    onOpenChapter?: (chapterId: string, questId?: string) => void;
    onSelectMultiple?: (ids: string[]) => void;
    fitToken?: number;
    addQuestToken?: number;
    questFilter?: string;
    filterTotal?: number;
    onQuestFilterChange?: (value: string) => void;
    onApplyLayout?: (kind: "tree" | "grid" | "circle") => void;
    onAlign?: (mode: "left" | "right" | "top" | "bottom" | "centerX" | "centerY") => void;
    onDistribute?: (mode: "horizontally" | "vertically") => void;
  } = $props();

  const BASE = 24;
  const nodeTypes = { quest: QuestNode };

  let issueIds = $derived(new Set(issues.map((i) => i.questId)));
  let iconRevision = $state(0);
  let selectedEdgeId = $state<string | null>(null);
  let layoutMenuOpen = $state(false);
  let lastLayout = $state<"tree" | "grid" | "circle" | null>(null);
  let alignMenuOpen = $state(false);

  function doAlign(mode: "left" | "right" | "top" | "bottom" | "centerX" | "centerY") {
    alignMenuOpen = false;
    if (!onAlign) return;
    const selected = quests.filter((q) => selectedIds.has(q.id));
    if (selected.length < 2) return;
    const moves = alignQuests(
      selected.map((q) => ({ id: q.id, x: q.x, y: q.y })),
      mode,
    );
    for (const q of selected) {
      const m = moves.get(q.id);
      if (!m) continue;
      if (m.x !== undefined || m.y !== undefined) {
        onMove(q, m.x ?? q.x, m.y ?? q.y);
      }
    }
  }

  function doDistribute(mode: "horizontally" | "vertically") {
    alignMenuOpen = false;
    if (!onDistribute) return;
    const selected = quests.filter((q) => selectedIds.has(q.id));
    if (selected.length < 3) return;
    const moves = distributeQuests(
      selected.map((q) => ({ id: q.id, x: q.x, y: q.y })),
      mode,
    );
    for (const q of selected) {
      const m = moves.get(q.id);
      if (!m) continue;
      if (m.x !== undefined || m.y !== undefined) {
        onMove(q, m.x ?? q.x, m.y ?? q.y);
      }
    }
  }

  let zoomPercent = $state(100);
  $effect(() => {
    const el = document.querySelector(".svelte-flow__viewport");
    if (!el) return;
    const read = () => {
      const m = /scale\(([\d.]+)\)/.exec(el.getAttribute("style") ?? "");
      zoomPercent = m ? Math.round(parseFloat(m[1]) * 100) : 100;
    };
    read();
    const mo = new MutationObserver(read);
    mo.observe(el, { attributes: true, attributeFilter: ["style"] });
    return () => mo.disconnect();
  });

  $effect(() => {
    if (!layoutMenuOpen && !alignMenuOpen) return;
    const onPtr = (e: PointerEvent) => {
      const target = e.target as HTMLElement | null;
      if (!target?.closest?.(".layout-pop")) {
        layoutMenuOpen = false;
        alignMenuOpen = false;
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        layoutMenuOpen = false;
        alignMenuOpen = false;
      }
    };
    window.addEventListener("pointerdown", onPtr, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onPtr, true);
      window.removeEventListener("keydown", onKey, true);
    };
  });

  function pickLayout(mode: "tree" | "grid" | "circle") {
    lastLayout = mode;
    layoutMenuOpen = false;
    onApplyLayout?.(mode);
  }

  const { screenToFlowPosition, fitView: flowFitView, getViewport } = useSvelteFlow();

  let nodes = $state<Node[]>([]);
  let edges = $state<Edge[]>([]);
  let viewportEl = $state<HTMLDivElement | null>(null);

  let marqueeWorld = $state<null | { x1: number; y1: number; x2: number; y2: number }>(null);
  let marqueeScreen = $state<null | { left: number; top: number; width: number; height: number }>(null);
  let marqueeOriginScreen = $state<null | { x: number; y: number }>(null);
  let marqueeActive = $state(false);

  $effect(() => {
    if (quests && $projectPath) {
      void preloadChapterIcons(quests);
    }
  });

  async function preloadChapterIcons(list: QuestData[]) {
    const ids = list
      .map((q) => iconDisplayId(q.icon))
      .filter((id): id is string => !!id);
    if (!ids.length || !$projectPath) return;
    await preloadItemIcons(ids, $projectPath);
    iconRevision += 1;
  }

  function progressOf(q: QuestData): QuestProgressStatus | null {
    if (!progressOverlay) return null;
    return progressStatuses[q.id] ?? "unknown";
  }

  function findExternalDep(
    depId: string,
  ): { quest: QuestData; chapterTitle: string; chapterId: string } | null {
    for (const ch of chapters) {
      const direct = ch.quests.find((oq) => oq.id === depId);
      if (direct && !quests.some((q) => q.id === direct.id)) {
        return {
          quest: direct,
          chapterTitle: ch.title || ch.filename || ch.id.slice(0, 8),
          chapterId: ch.id,
        };
      }
      const owner = ch.quests.find((oq) => oq.tasks?.some((t) => t.id === depId));
      if (owner && !quests.some((q) => q.id === owner.id)) {
        return {
          quest: owner,
          chapterTitle: ch.title || ch.filename || ch.id.slice(0, 8),
          chapterId: ch.id,
        };
      }
    }
    return null;
  }

  $effect(() => {
    const rev = iconRevision;
    void chapters;
    const newNodes: Node[] = quests.map((q) => ({
      id: q.id,
      type: "quest",
      position: { x: q.x * BASE, y: q.y * BASE },
      data: {
        quest: q,
        isIssue: issueIds.has(q.id),
        isSelected: selectedId === q.id || selectedIds.has(q.id),
        baseSize: BASE,
        progress: progressOf(q),
        iconRevision: rev,
      },
      selected: selectedId === q.id || selectedIds.has(q.id),
    }));

    const ghosts = new Map<string, Node>();
    const newEdges: Edge[] = [];
    for (const q of quests) {
      for (const depId of q.dependencies) {
        let sourceId = depId;
        let targetExists = quests.some((oq) => oq.id === depId);
        let external = false;
        if (!targetExists) {
          const owner = quests.find((oq) => oq.tasks?.some((t) => t.id === depId));
          if (owner) {
            sourceId = owner.id;
            targetExists = true;
          }
        }

        if (!targetExists) {
          const ext = findExternalDep(depId);
          if (ext) {
            sourceId = `ext:${ext.quest.id}`;
            targetExists = true;
            external = true;
            if (!ghosts.has(sourceId)) {
              ghosts.set(sourceId, {
                id: sourceId,
                type: "quest",
                position: {
                  x: q.x * BASE - BASE * 3,
                  y: q.y * BASE - BASE * 3,
                },
                draggable: false,
                selectable: true,
                data: {
                  quest: ext.quest,
                  isIssue: false,
                  isSelected: false,
                  baseSize: BASE,
                  progress: null,
                  iconRevision: rev,
                  external: true,
                  chapterTitle: ext.chapterTitle,
                  chapterId: ext.chapterId,
                },
              });
            }
          }
        }

        const depDone =
          progressOverlay &&
          (progressStatuses[sourceId.replace(/^ext:/, "")] === "completed" ||
            progressStatuses[depId] === "completed");

        let style =
          "stroke: var(--accent-primary); stroke-width: 2.5; filter: drop-shadow(0 1px 2px rgba(0,0,0,0.4));";
        if (!targetExists || external) {
          style =
            "stroke: var(--accent-warning); stroke-width: 2; stroke-dasharray: 5 4; filter: drop-shadow(0 1px 2px rgba(0,0,0,0.4));";
        } else if (depDone) {
          style =
            "stroke: var(--accent-primary); stroke-width: 3.5; filter: drop-shadow(0 0 4px var(--accent-primary));";
        }

        newEdges.push({
          id: `e-${depId}-${q.id}`,
          source: sourceId,
          target: q.id,
          type: "smoothstep",
          style,
          markerEnd: { type: "arrowclosed", width: 14, height: 14, color: "var(--accent-primary)" },
          selectable: !external,
          data: { depId, dependentId: q.id },
          selected: selectedEdgeId === `e-${depId}-${q.id}`,
        });
      }
    }

    nodes = [...newNodes, ...ghosts.values()];
    edges = newEdges;
  });

  let lastMultiSelection = $state<readonly string[]>([]);
  $effect(() => {
    const selectedNodes = nodes.filter((n) => n.selected && !n.id.startsWith("ext:"));
    if (selectedNodes.length > 1) {
      const ids = selectedNodes.map((n) => n.id);
      const changed =
        ids.length !== lastMultiSelection.length ||
        ids.some((id) => !lastMultiSelection.includes(id));
      lastMultiSelection = ids;
      if (changed) onSelectMultiple(ids);
    } else {
      lastMultiSelection = [];
    }
  });

  let lastFitToken = $state(-1);
  $effect(() => {
    if (fitToken !== lastFitToken) {
      lastFitToken = fitToken;
      tick().then(() => flowFitView({ padding: 0.2 }));
    }
  });

  let lastAddQuestToken = $state(0);
  $effect(() => {
    if (addQuestToken !== lastAddQuestToken && addQuestToken > 0) {
      lastAddQuestToken = addQuestToken;
      tick().then(() => addAtCenter());
    }
  });

  function snap(v: number) {
    return Math.round(v * 2) / 2;
  }

  function flowContainer(): HTMLElement | null {
    if (!viewportEl) return null;
    return (
      (viewportEl.querySelector(".svelte-flow") as HTMLElement | null) ??
      (viewportEl.querySelector(".react-flow") as HTMLElement | null) ??
      viewportEl
    );
  }

  function isEmptyPaneTarget(target: EventTarget | null): boolean {
    if (!(target instanceof Element)) return false;
    if (target.closest(".xyflow__node, .svelte-flow__node, .react-flow__node")) return false;
    if (target.closest(".xyflow__edge, .svelte-flow__edge, .react-flow__edge")) return false;
    if (target.closest(".xyflow__controls, .svelte-flow__controls, .react-flow__controls"))
      return false;
    if (target.closest(".xyflow__minimap, .svelte-flow__minimap, button, a, input, textarea"))
      return false;
    return !!(
      target.closest(".xyflow__pane, .svelte-flow__pane, .react-flow__pane") ||
      target.closest(".svelte-flow, .react-flow") ||
      target === viewportEl
    );
  }

  function nodeFlowSize(n: Node): number {
    const q = (n.data as { quest?: QuestData; baseSize?: number } | undefined)?.quest;
    const base = (n.data as { baseSize?: number } | undefined)?.baseSize ?? BASE;
    const scale = q?.size && q.size > 0 ? q.size : 1;
    return base * scale;
  }

  function finishMarquee(additive: boolean) {
    if (!marqueeWorld) {
      marqueeActive = false;
      marqueeScreen = null;
      marqueeOriginScreen = null;
      return;
    }
    const box = marqueeWorld;
    const hit: string[] = [];
    for (const n of nodes) {
      if (n.id.startsWith("ext:")) continue;
      const size = nodeFlowSize(n);
      const nb = {
        x1: n.position.x,
        y1: n.position.y,
        x2: n.position.x + size,
        y2: n.position.y + size,
      };
      if (rectsIntersect(box, nb)) hit.push(n.id);
    }
    marqueeActive = false;
    marqueeWorld = null;
    marqueeScreen = null;
    marqueeOriginScreen = null;
    if (hit.length > 0) {
      onSelectMultiple(hit);
    } else if (!additive) {
      onSelect(null);
    }
  }

  function focusCanvas() {
    viewportEl?.focus({ preventScroll: true });
  }

  function onMarqueePointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    if (!isEmptyPaneTarget(e.target)) return;
    focusCanvas();
    const container = flowContainer();
    if (!container) return;
    const { x: panX, y: panY, zoom } = getViewport();
    const world = getWorldCoordinates(e, container, panX, panY, zoom);
    const rect = container.getBoundingClientRect();
    const sx = e.clientX - rect.left;
    const sy = e.clientY - rect.top;
    marqueeActive = true;
    marqueeWorld = { x1: world.x, y1: world.y, x2: world.x, y2: world.y };
    marqueeOriginScreen = { x: sx, y: sy };
    marqueeScreen = { left: sx, top: sy, width: 0, height: 0 };
    try {
      viewportEl?.setPointerCapture(e.pointerId);
    } catch {
      /* ignore */
    }
    e.preventDefault();
    e.stopPropagation();
  }

  function onMarqueePointerMove(e: PointerEvent) {
    if (!marqueeActive || !marqueeWorld || !marqueeOriginScreen) return;
    const container = flowContainer();
    if (!container) return;
    const { x: panX, y: panY, zoom } = getViewport();
    const world = getWorldCoordinates(e, container, panX, panY, zoom);
    const rect = container.getBoundingClientRect();
    const sx = e.clientX - rect.left;
    const sy = e.clientY - rect.top;
    const ox = marqueeOriginScreen.x;
    const oy = marqueeOriginScreen.y;
    marqueeWorld = { ...marqueeWorld, x2: world.x, y2: world.y };
    marqueeScreen = {
      left: Math.min(ox, sx),
      top: Math.min(oy, sy),
      width: Math.abs(sx - ox),
      height: Math.abs(sy - oy),
    };
  }

  function onMarqueePointerUp(e: PointerEvent) {
    if (!marqueeActive) return;
    try {
      viewportEl?.releasePointerCapture(e.pointerId);
    } catch {
      /* ignore */
    }
    const dragged =
      marqueeScreen != null && (marqueeScreen.width > 3 || marqueeScreen.height > 3);
    if (dragged) {
      finishMarquee(e.shiftKey || e.ctrlKey || e.metaKey);
    } else {
      marqueeActive = false;
      marqueeWorld = null;
      marqueeScreen = null;
      marqueeOriginScreen = null;
    }
  }

  function onCanvasDblClick(e: MouseEvent) {
    if (!isEmptyPaneTarget(e.target)) return;
    const container = flowContainer();
    if (!container) return;
    e.preventDefault();
    e.stopPropagation();
    const { x: panX, y: panY, zoom } = getViewport();
    const world = getWorldCoordinates(e, container, panX, panY, zoom);
    onAddAt(snap(world.x / BASE), snap(world.y / BASE));
  }

  function handleNodeDragStop({
    targetNode,
    nodes: flowNodes,
  }: {
    targetNode: Node | null;
    nodes: Node[];
    event: MouseEvent | TouchEvent;
  }) {
    const toPersist = new Set<string>();
    if (targetNode && !targetNode.id.startsWith("ext:")) toPersist.add(targetNode.id);
    for (const n of flowNodes) {
      if (n.selected && !n.id.startsWith("ext:")) toPersist.add(n.id);
    }
    for (const n of flowNodes) {
      if (!toPersist.has(n.id)) continue;
      const q = quests.find((item) => item.id === n.id);
      if (q) {
        onMove(q, snap(n.position.x / BASE), snap(n.position.y / BASE));
      }
    }
  }

  function handleConnect(connection: Connection) {
    if (connection.source && connection.target) {
      const src = connection.source.startsWith("ext:")
        ? connection.source.slice(4)
        : connection.source;
      const tgt = connection.target.startsWith("ext:")
        ? connection.target.slice(4)
        : connection.target;
      onLink(tgt, src);
    }
  }

  function isValidConnection(connection: Edge | Connection | null | undefined): boolean {
    if (!connection?.source || !connection?.target) return false;
    if (connection.source === connection.target) return false;
    if (connection.target.startsWith("ext:")) return false;
    const src = connection.source.startsWith("ext:")
      ? connection.source.slice(4)
      : connection.source;
    const dependentId = connection.target;
    if (!quests.some((q) => q.id === dependentId)) return false;
    if (src === dependentId) return false;
    const dependent = quests.find((q) => q.id === dependentId);
    if (dependent?.dependencies?.includes(src)) return false;
    return true;
  }

  function clearEdgeSelection() {
    if (!selectedEdgeId) return;
    selectedEdgeId = null;
    edges = edges.map((ed) => (ed.selected ? { ...ed, selected: false } : ed));
    onEdgeSelect?.(null);
  }

  function handleEdgeClick({ edge }: { edge: Edge }) {
    const depId = (edge.data as { depId?: string } | undefined)?.depId;
    const dependentId = (edge.data as { dependentId?: string } | undefined)?.dependentId;
    if (!depId || !dependentId) return;
    selectedEdgeId = edge.id;
    edges = edges.map((ed) => ({
      ...ed,
      selected: ed.id === edge.id,
      style:
        ed.id === edge.id
          ? `${String(ed.style ?? "").replace(/stroke:[^;]+;?/g, "").replace(/stroke-width:[^;]+;?/g, "")} stroke: var(--accent-primary); stroke-width: 4;`
          : ed.style,
    }));
    onEdgeSelect?.({ questId: dependentId, depId });
    onSelect(null);
  }

  function handlePaneClick({ event }: { event: MouseEvent }) {
    if (event.detail >= 2) return;
    if (marqueeActive) return;
    clearEdgeSelection();
    focusCanvas();
    onSelect(null, event);
  }

  function handleNodeClick({ node, event }: { node: Node; event: MouseEvent | TouchEvent }) {
    if (node.id.startsWith("ext:")) {
      const chapterId = (node.data as { chapterId?: string } | undefined)?.chapterId;
      const questId = (node.data as { quest?: QuestData } | undefined)?.quest?.id;
      if (chapterId && onOpenChapter) {
        onOpenChapter(chapterId, questId);
      }
      return;
    }
    clearEdgeSelection();
    const q = quests.find((item) => item.id === node.id);
    if (q) {
      focusCanvas();
      onSelect(q, event instanceof MouseEvent ? event : undefined);
    }
  }

  /** Clearance kept between a freshly added quest and every other quest, in
   *  FTBQ units (1 unit == BASE px). 2.5 units is wider than the node label
   *  (capped at 2 units), so two quests placed on the same row can never have
   *  their titles overlap. */
  const PLACE_CLEARANCE = 2.5;

  /** Nearest free cell around (x, y) on the half-unit quest grid. Without this
   *  every "+ Add Quest" dropped the quest on the viewport centre, so repeated
   *  adds stacked nodes (and their labels) on top of each other. */
  function findFreeSlot(x: number, y: number) {
    const free = (cx: number, cy: number) =>
      quests.every(
        (q) => Math.abs(q.x - cx) >= PLACE_CLEARANCE || Math.abs(q.y - cy) >= PLACE_CLEARANCE,
      );
    // Half-unit lattice == one grid step; ring 0 is the requested point itself,
    // so the quest stays as close to the viewport centre as space allows.
    const offsets: [number, number][] = [[0, 0]];
    for (let ring = 0.5; ring <= 12; ring += 0.5) {
      for (let i = -ring; i <= ring; i += 0.5) {
        if (Math.abs(i) !== ring) offsets.push([i, -ring], [i, ring]);
        offsets.push([-ring, i], [ring, i]);
      }
    }
    for (const [dx, dy] of offsets) {
      const cx = snap(x + dx);
      const cy = snap(y + dy);
      if (free(cx, cy)) return { x: cx, y: cy };
    }
    return { x, y };
  }

  function addAtCenter() {
    const container = flowContainer();
    if (container) {
      const rect = container.getBoundingClientRect();
      const cx = rect.left + rect.width / 2;
      const cy = rect.top + rect.height / 2;
      const { x: panX, y: panY, zoom } = getViewport();
      const world = getWorldCoordinates({ clientX: cx, clientY: cy }, container, panX, panY, zoom);
      const slot = findFreeSlot(snap(world.x / BASE), snap(world.y / BASE));
      onAddAt(slot.x, slot.y);
    } else {
      const pos = screenToFlowPosition({
        x: window.innerWidth / 2,
        y: window.innerHeight / 2,
      });
      const slot = findFreeSlot(snap(pos.x / BASE), snap(pos.y / BASE));
      onAddAt(slot.x, slot.y);
    }
  }

  function selectQuestByIndex(index: number) {
    if (!quests.length) return;
    const clamped = Math.max(0, Math.min(index, quests.length - 1));
    onSelect(quests[clamped]);
  }

  function handleCanvasKeydown(e: KeyboardEvent) {
    if (e.altKey || e.ctrlKey || e.metaKey) return;
    switch (e.key) {
      case "Escape": {
        e.preventDefault();
        e.stopPropagation();
        onSelect(null);
        return;
      }
      case "Home": {
        if (!quests.length) return;
        e.preventDefault();
        e.stopPropagation();
        selectQuestByIndex(0);
        return;
      }
      case "End": {
        if (!quests.length) return;
        e.preventDefault();
        e.stopPropagation();
        selectQuestByIndex(quests.length - 1);
        return;
      }
      case "ArrowRight":
      case "ArrowDown": {
        if (!quests.length) return;
        e.preventDefault();
        e.stopPropagation();
        {
          const idx = selectedId ? quests.findIndex((q) => q.id === selectedId) : -1;
          selectQuestByIndex(idx < 0 ? 0 : Math.min(idx + 1, quests.length - 1));
        }
        return;
      }
      case "ArrowLeft":
      case "ArrowUp": {
        if (!quests.length) return;
        e.preventDefault();
        e.stopPropagation();
        {
          const idx = selectedId ? quests.findIndex((q) => q.id === selectedId) : -1;
          selectQuestByIndex(idx < 0 ? quests.length - 1 : Math.max(idx - 1, 0));
        }
        return;
      }
      default:
        return;
    }
  }
</script>

<div class="canvas-wrap flex flex-col h-full min-h-0 bg-[var(--bg-primary)] overflow-hidden">
  <!-- Top HUD Toolbar -->
  <div class="canvas-toolbar flex items-center justify-between gap-2 px-3 py-2 bg-[var(--bg-secondary)] border-b border-[var(--border-color)] flex-shrink-0 z-10">
    <div class="flex items-center gap-2 flex-1 min-w-0">
      {#if onQuestFilterChange}
        <div class="relative flex items-center min-w-[140px] max-w-[240px] flex-1">
          <Search size={13} class="absolute left-2.5 text-[var(--text-muted)] pointer-events-none" />
          <input
            type="search"
            class="tb-filter w-full pl-7 pr-2 py-1 text-xs bg-[var(--bg-primary)] border border-[var(--border-color)] rounded-md text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent-primary)]"
            placeholder="Filter canvas nodes…"
            value={questFilter}
            oninput={(e) => onQuestFilterChange?.((e.currentTarget as HTMLInputElement).value)}
            onkeydown={(e) => {
              if (e.key === "Escape") onQuestFilterChange?.("");
              if (e.key === "Enter") {
                e.preventDefault();
                const first = quests[0];
                if (first) onSelect(first);
              }
            }}
          />
        </div>
        {#if questFilter}
          <span class="text-[12px] font-mono font-semibold text-[var(--text-muted)] bg-[var(--bg-card)] px-2 py-0.5 rounded border border-[var(--border-color)]">
            {quests.length}/{filterTotal}
          </span>
        {/if}
      {/if}

      <button
        type="button"
        class="hud-btn flex items-center gap-2 px-2.5 py-1 text-xs font-semibold rounded-md border border-[var(--border-color)] bg-[var(--bg-card)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] transition"
        title="Add quest at center (N or double-click)"
        onclick={addAtCenter}
      >
        <Plus size={13} class="text-[var(--accent-primary)]" />
        <span>Add Quest</span>
      </button>
    </div>

    <!-- Right Controls: Fit, Zoom, Layout, Align -->
    <div class="flex items-center gap-2 flex-shrink-0">
      <button
        type="button"
        class="hud-btn flex items-center gap-2 px-2.5 py-1 text-xs font-semibold rounded-md border border-[var(--border-color)] bg-[var(--bg-card)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] transition"
        title="Fit canvas to content (Ctrl+0)"
        onclick={() => flowFitView({ padding: 0.2 })}
      >
        <Maximize2 size={13} />
        <span>Fit</span>
      </button>

      <span class="zoom-pct text-[12px] font-mono font-bold text-[var(--text-muted)] px-1.5 py-0.5 rounded bg-[var(--bg-card)] border border-[var(--border-color)]" title="Current zoom level">
        {zoomPercent}%
      </span>

      {#if onApplyLayout}
        <div class="layout-pop relative">
          <button
            type="button"
            class="hud-btn flex items-center gap-2 px-2.5 py-1 text-xs font-semibold rounded-md border border-[var(--border-color)] bg-[var(--bg-card)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] transition {layoutMenuOpen ? 'border-[var(--accent-primary)] bg-[var(--bg-hover)]' : ''}"
            title="Auto-layout nodes"
            onclick={() => (layoutMenuOpen = !layoutMenuOpen)}
          >
            <LayoutGrid size={13} class="text-[var(--accent-primary)]" />
            <span>Layout</span>
            <ChevronDown size={11} />
          </button>
          {#if layoutMenuOpen}
            <div class="layout-menu absolute top-full right-0 mt-1.5 w-32 p-1 bg-[var(--bg-card)] border border-[var(--border-color)] rounded-lg shadow-lg z-50 flex flex-col gap-0.5" role="menu">
              <button
                type="button"
                class="w-full text-left px-2.5 py-1.5 text-xs font-medium rounded hover:bg-[var(--bg-hover)] text-[var(--text-primary)] {lastLayout === 'tree' ? 'text-[var(--accent-primary)] font-bold' : ''}"
                onclick={() => pickLayout("tree")}
              >
                Hierarchical Tree
              </button>
              <button
                type="button"
                class="w-full text-left px-2.5 py-1.5 text-xs font-medium rounded hover:bg-[var(--bg-hover)] text-[var(--text-primary)] {lastLayout === 'grid' ? 'text-[var(--accent-primary)] font-bold' : ''}"
                onclick={() => pickLayout("grid")}
              >
                Compact Grid
              </button>
              <button
                type="button"
                class="w-full text-left px-2.5 py-1.5 text-xs font-medium rounded hover:bg-[var(--bg-hover)] text-[var(--text-primary)] {lastLayout === 'circle' ? 'text-[var(--accent-primary)] font-bold' : ''}"
                onclick={() => pickLayout("circle")}
              >
                Radial Circle
              </button>
            </div>
          {/if}
        </div>
      {/if}

      {#if onAlign && onDistribute && selectedIds.size >= 2}
        <div class="layout-pop relative">
          <button
            type="button"
            class="hud-btn flex items-center gap-2 px-2.5 py-1 text-xs font-semibold rounded-md border border-[var(--border-color)] bg-[var(--bg-card)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] transition {alignMenuOpen ? 'border-[var(--accent-primary)] bg-[var(--bg-hover)]' : ''}"
            title="Align or distribute selected quests"
            onclick={() => (alignMenuOpen = !alignMenuOpen)}
          >
            <AlignLeft size={13} class="text-[var(--accent-primary)]" />
            <span>Align ({selectedIds.size})</span>
            <ChevronDown size={11} />
          </button>
          {#if alignMenuOpen}
            <div class="layout-menu absolute top-full right-0 mt-1.5 w-44 p-1.5 bg-[var(--bg-card)] border border-[var(--border-color)] rounded-lg shadow-lg z-50 flex flex-col gap-2" role="menu">
              <span class="text-[12px] uppercase font-bold text-[var(--text-muted)] px-2 py-0.5">Align Nodes</span>
              <button type="button" class="w-full text-left px-2 py-1 text-xs rounded hover:bg-[var(--bg-hover)] text-[var(--text-primary)]" onclick={() => doAlign("left")}>Align Left</button>
              <button type="button" class="w-full text-left px-2 py-1 text-xs rounded hover:bg-[var(--bg-hover)] text-[var(--text-primary)]" onclick={() => doAlign("right")}>Align Right</button>
              <button type="button" class="w-full text-left px-2 py-1 text-xs rounded hover:bg-[var(--bg-hover)] text-[var(--text-primary)]" onclick={() => doAlign("top")}>Align Top</button>
              <button type="button" class="w-full text-left px-2 py-1 text-xs rounded hover:bg-[var(--bg-hover)] text-[var(--text-primary)]" onclick={() => doAlign("bottom")}>Align Bottom</button>
              <button type="button" class="w-full text-left px-2 py-1 text-xs rounded hover:bg-[var(--bg-hover)] text-[var(--text-primary)]" onclick={() => doAlign("centerX")}>Center Horizontally</button>
              <button type="button" class="w-full text-left px-2 py-1 text-xs rounded hover:bg-[var(--bg-hover)] text-[var(--text-primary)]" onclick={() => doAlign("centerY")}>Center Vertically</button>
              <span class="text-[12px] uppercase font-bold text-[var(--text-muted)] px-2 py-0.5 border-t border-[var(--border-color)] mt-1">Distribute</span>
              <button type="button" class="w-full text-left px-2 py-1 text-xs rounded hover:bg-[var(--bg-hover)] text-[var(--text-primary)]" onclick={() => doDistribute("horizontally")}>Evenly Horizontal</button>
              <button type="button" class="w-full text-left px-2 py-1 text-xs rounded hover:bg-[var(--bg-hover)] text-[var(--text-primary)]" onclick={() => doDistribute("vertically")}>Evenly Vertical</button>
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  <!-- SvelteFlow Canvas Stage -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_no_noninteractive_tabindex -->
  <div
    class="viewport flex-1 min-h-0 relative focus:outline-none"
    role="application"
    tabindex="0"
    aria-label="Quest canvas"
    bind:this={viewportEl}
    onpointerdown={onMarqueePointerDown}
    onpointermove={onMarqueePointerMove}
    onpointerup={onMarqueePointerUp}
    onpointercancel={onMarqueePointerUp}
    ondblclick={onCanvasDblClick}
    onkeydown={handleCanvasKeydown}
  >
    {#if quests.length === 0}
      <div class="empty-hint absolute inset-0 flex flex-col items-center justify-center gap-3 z-10 pointer-events-none text-center">
        {#if showEmptyAddCta}
          <button
            type="button"
            class="empty-add pointer-events-auto px-4 py-2 bg-[var(--accent-primary)] text-[var(--on-accent)] font-bold text-xs rounded-lg hover:brightness-110 shadow-md transition"
            onclick={(e) => { e.stopPropagation(); addAtCenter(); }}
          >
            + Add First Quest
          </button>
          <span class="text-xs text-[var(--text-muted)]">Double-click canvas · Press N · Use toolbar button</span>
        {:else}
          <span class="text-xs text-[var(--text-muted)]">{emptyHint}</span>
        {/if}
      </div>
    {/if}

    <SvelteFlow
      bind:nodes
      bind:edges
      {nodeTypes}
      panOnScroll
      selectionOnDrag={false}
      panOnDrag={[1, 2]}
      deleteKey={null}
      onnodeclick={handleNodeClick}
      onpaneclick={handlePaneClick}
      onnodedragstop={handleNodeDragStop}
      onconnect={handleConnect}
      isValidConnection={isValidConnection}
      onedgeclick={handleEdgeClick}
      fitView
      fitViewOptions={{ padding: 0.2 }}
      defaultEdgeOptions={{
        type: "smoothstep",
        style: "stroke: var(--accent-primary); stroke-width: 2.5; filter: drop-shadow(0 1px 2px rgba(0,0,0,0.4));",
      }}
    >
      <Background
        variant={BackgroundVariant.Dots}
        gap={20}
        size={1.5}
        patternColor="var(--border-color)"
      />
      <Controls />
      <MiniMap
        pannable
        zoomable
        nodeStrokeWidth={2}
        maskColor="rgba(0, 0, 0, 0.45)"
        bgColor="var(--bg-secondary)"
        nodeColor={() => "var(--accent-primary)"}
        ariaLabel="Chapter minimap"
      />
    </SvelteFlow>

    {#if marqueeScreen && (marqueeScreen.width > 0 || marqueeScreen.height > 0)}
      <div
        class="marquee-box absolute pointer-events-none z-20 border border-[var(--accent-primary)] bg-[var(--accent-primary)]/15"
        style="left:{marqueeScreen.left}px; top:{marqueeScreen.top}px; width:{marqueeScreen.width}px; height:{marqueeScreen.height}px;"
      ></div>
    {/if}
  </div>
</div>

<style>
  :global(.svelte-flow__background) {
    background-color: var(--bg-primary) !important;
  }
  :global(.svelte-flow__edge .svelte-flow__edge-path) {
    stroke-width: 2.5;
  }
  :global(.svelte-flow__handle.connectionindicator),
  :global(.svelte-flow__node:hover .svelte-flow__handle) {
    pointer-events: all !important;
  }
  :global(.svelte-flow__edge:hover path) {
    stroke: var(--accent-primary) !important;
    stroke-width: 3.5 !important;
  }
  :global(.svelte-flow__minimap) {
    display: none !important;
  }
  :global(.svelte-flow__controls) {
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm, 6px);
    overflow: hidden;
    background: var(--bg-card);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.25);
  }
  :global(.svelte-flow__controls button) {
    background: var(--bg-card);
    border: none;
    border-bottom: 1px solid var(--border-color);
    color: var(--text-primary);
    transition: background 0.15s ease;
  }
  :global(.svelte-flow__controls button:hover) {
    background: var(--bg-hover);
  }
  :global(.svelte-flow__attribution) {
    display: none;
  }
</style>
