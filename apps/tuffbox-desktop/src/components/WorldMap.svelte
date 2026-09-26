<script lang="ts">
  import { onMount, onDestroy, tick, untrack } from "svelte";
  import {
    Map as MapIcon, RefreshCw, Trash2, MousePointer2, Square, Layers, Download,
    CalendarRange, CheckSquare, XSquare, Copy, Scissors, Clipboard, Circle,
    ZoomIn, ZoomOut, Minimize2, Eraser, ArrowLeftRight, Filter, FolderOutput, FolderInput,
    FileDown, FileUp, Wrench, Pencil, Crosshair, List, Globe2,
  } from "@lucide/svelte";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { projectPath } from "../lib/store";
  import EmptyState from "./EmptyState.svelte";
  import ChunkNbtEditor from "./ChunkNbtEditor.svelte";
  import { api } from "../lib/api";
  import type {
    WorldMap as WorldMapData,
    ChunkCell,
    NbtChangeRequest,
    AdvancedChunkFilter,
    WorldListItem,
  } from "../lib/api";
  import { biomeCss, heightShade } from "../lib/biomePalette";
  import {
    worldMapClipboard,
    setWorldMapClipboard,
    clearWorldMapClipboard,
  } from "../lib/worldMapClipboard";
  import { get } from "svelte/store";

  const STATUS_EMPTY = 1;
  const STATUS_PARTIAL = 2;
  const STATUS_FULL = 3;

  type ColorMode = "status" | "date" | "inhabited" | "biome" | "height";
  type Tool = "pan" | "click" | "box" | "radius" | "region" | "poly";

  let {
    worldName = "",
    layout = "top",
    readOnly = false,
  }: {
    worldName?: string;
    layout?: "top" | "dock";
    readOnly?: boolean;
  } = $props();

  let map = $state<WorldMapData | null>(null);
  let filtersOpen = $state(false);
  let toolsDrawerOpen = $state(false);
  let regionRailOpen = $state(false);
  type ToolsTab = "select" | "edit" | "export" | "filters";
  let toolsTab = $state<ToolsTab>("edit");

  $effect(() => {
    if (readOnly && (toolsTab === "edit" || toolsTab === "export")) {
      toolsTab = "select";
    }
  });
  let loading = $state(false);
  let error = $state<string | null>(null);

  let dimensions = $state<string[]>(["overworld"]);
  let dimension = $state("overworld");

  let showRegions = $state(true);
  let showChunkGrid = $state(true);
  let showSpawn = $state(true);
  let spawnChunk = $state<{ cx: number; cz: number } | null>(null);
  let colorMode = $state<ColorMode>("biome");
  let tool = $state<Tool>("box");
  let selection = $state(new Set<string>());
  let statusFilter = $state<"all" | "empty" | "partial" | "full">("all");
  let polyPoints = $state<{ x: number; y: number }[]>([]);
  let polyAdd = $state(true);

  /** Height range filter (MCA-style Y slider); chunks outside are dimmed. */
  let heightMin = $state(-64);
  let heightMax = $state(319);

  let gotoOpen = $state(false);
  let gotoMode = $state<"block" | "chunk">("block");
  let gotoX = $state("0");
  let gotoZ = $state("0");
  let gotoXInput = $state<HTMLInputElement | null>(null);

  let hover = $state<{
    rx: number; rz: number; cx: number; cz: number;
    blockX: number; blockZ: number;
    status: string; modified: number;
    inhabitedTime: number; dataVersion: number;
    biomeId?: number; surfaceY?: number;
    entityCount?: number; structureCount?: number;
  } | null>(null);
  let tipX = $state(0);
  let tipY = $state(0);
  let visibleRegionCount = $state(0);

  let pasteOffsetX = $state(0);
  let pasteOffsetZ = $state(0);

  let fromWorldOpen = $state(false);
  let fromWorldLoading = $state(false);
  let fromWorldList = $state<WorldListItem[]>([]);
  let fromWorldName = $state("");
  let fromWorldDim = $state("overworld");
  let fromWorldDims = $state<string[]>(["overworld"]);
  let fromWorldBannerDismissed = $state(false);
  let busyLabel = $state<string | null>(null);

  let filterFrom = $state("");
  let filterTo = $state("");
  let filterActive = $state(false);
  let radiusChunks = $state(8);

  let inhabitedMin = $state("");
  let inhabitedMax = $state("");
  let dataVersionMin = $state("");
  let dataVersionMax = $state("");
  let xposMin = $state("");
  let xposMax = $state("");
  let zposMin = $state("");
  let zposMax = $state("");
  let borderEmpty = $state("");
  let entityCountMin = $state("");
  let structureCountMin = $state("");
  let filtEntityNames = $state("");
  let filtStructureNames = $state("");
  let filtPaletteNames = $state("");
  let filterQuery = $state("");
  let importOverwrite = $state(true);
  let importIntoSelection = $state(false);
  let importYOffset = $state(0);
  let importSections = $state("");

  let chgInhabited = $state("");
  let chgStatus = $state("");
  let chgDataVersion = $state("");
  let chgLightPopulated = $state("");
  let chgBiome = $state("");
  let chgDeleteSections = $state("");
  let chgReplaceBlocks = $state("");
  let chgDeleteStructureRefs = $state("");
  let chgPreventRetrogen = $state(false);
  let chgForceBlend = $state(false);
  let chgDeleteEntities = $state(false);
  let chgFixStatus = $state(false);
  let chgForce = $state(false);
  let nbtPanelOpen = $state(true);

  let editorOpen = $state(false);
  let editorRx = $state(0);
  let editorRz = $state(0);
  let editorIdx = $state(0);

  let csvInput = $state<HTMLInputElement | undefined>(undefined);

  const CELL = 8;
  const GRID = 32;
  let canvas = $state<HTMLCanvasElement | undefined>(undefined);
  let viewport = $state<HTMLDivElement | undefined>(undefined);

  // View transform (screen = world * zoom + pan)
  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let panning = $state(false);
  let panLast = $state<{ x: number; y: number } | null>(null);

  let dragStart = $state<{ x: number; y: number } | null>(null);
  let dragCurrent = $state<{ x: number; y: number } | null>(null);
  let dragAdd = $state(true);

  let flashMsg = $state<string | null>(null);
  let flashTimer = $state<ReturnType<typeof setTimeout> | undefined>(undefined);
  // Plain flags — scheduleDraw reads+writes these; must not be $state or
  // $effect(if map → scheduleDraw) thrash-loops at ~60 Hz.
  let drawScheduled = false;
  let lastDrawAt = 0;
  const POTATO_DRAW_MIN_MS = 100;

  function isPotatoPc() {
    return document.documentElement.classList.contains("potato-pc");
  }

  function scheduleDraw() {
    if (drawScheduled) return;
    if (isPotatoPc()) {
      const elapsed = performance.now() - lastDrawAt;
      if (elapsed < POTATO_DRAW_MIN_MS) {
        drawScheduled = true;
        setTimeout(() => {
          drawScheduled = false;
          lastDrawAt = performance.now();
          draw();
        }, POTATO_DRAW_MIN_MS - elapsed);
        return;
      }
    }
    drawScheduled = true;
    requestAnimationFrame(() => {
      drawScheduled = false;
      lastDrawAt = performance.now();
      draw();
    });
  }

  function statusLabel(code: number): string {
    return code === STATUS_EMPTY ? "empty"
      : code === STATUS_PARTIAL ? "partial"
      : code === STATUS_FULL ? "full" : "unknown";
  }

  function worldChunkX(rx: number, local: number) { return rx * GRID + local; }
  function worldChunkZ(rz: number, local: number) { return rz * GRID + local; }

  function dimLabel(d: string): string {
    if (d === "nether") return "Nether";
    if (d === "end") return "The End";
    return "Overworld";
  }

  function flash(msg: string, ms = 2800) {
    flashMsg = msg;
    clearTimeout(flashTimer);
    flashTimer = setTimeout(() => (flashMsg = null), ms);
  }

  function flashError(msg: string) {
    error = msg;
    flash(msg, 4500);
  }

  const crossWorldClip = $derived(
    $worldMapClipboard != null &&
    $worldMapClipboard.sourceWorld !== worldName &&
    ($worldMapClipboard.clipboard.chunks?.length ?? 0) > 0,
  );

  const showClipBanner = $derived(
    layout === "dock" &&
    !readOnly &&
    crossWorldClip &&
    !fromWorldBannerDismissed &&
    !fromWorldOpen &&
    !gotoOpen,
  );

  let lastClipAt = $state(0);
  $effect(() => {
    if ($worldMapClipboard?.copiedAt && $worldMapClipboard.copiedAt !== lastClipAt) {
      lastClipAt = $worldMapClipboard.copiedAt;
      fromWorldBannerDismissed = false;
    }
  });
  $effect(() => {
    if (!$worldMapClipboard) {
      lastClipAt = 0;
    }
  });

  function parseOptNum(s: string): number | null {
    if (s === "" || s == null) return null;
    const n = Number(s);
    return Number.isFinite(n) ? n : null;
  }

  function cycleColorMode() {
    colorMode = colorMode === "status" ? "date"
      : colorMode === "date" ? "inhabited"
      : colorMode === "inhabited" ? "biome"
      : colorMode === "biome" ? "height"
      : "status";
    draw();
  }

  async function loadDimensions() {
    if (!$projectPath || !worldName) return;
    try {
      dimensions = await api.worlds.dimensions(worldName, $projectPath);
      if (!dimensions.includes(dimension)) {
        dimension = dimensions[0] || "overworld";
      }
    } catch {
      dimensions = ["overworld"];
    }
  }

  async function load() {
    if (!$projectPath || !worldName) return;
    loading = true;
    error = null;
    selection = new Set();
    filterActive = false;
    polyPoints = [];
    spawnChunk = null;
    try {
      await loadDimensions();
      map = await api.worlds.map(worldName, dimension, $projectPath);
      if (mapHasSparseBiomes(map)) {
        colorMode = "status";
      }
      try {
        const info = await api.worlds.readInfo(worldName, $projectPath);
        if (typeof info.spawnX === "number" && typeof info.spawnZ === "number") {
          spawnChunk = {
            cx: Math.floor(info.spawnX / 16),
            cz: Math.floor(info.spawnZ / 16),
          };
        }
      } catch {
        spawnChunk = null;
      }
      loading = false;
      // Canvas mounts with {#if map}; wait for layout so viewport has non-zero size.
      await tick();
      await tick();
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      fitView();
      draw();
    } catch (e) {
      map = null;
      error = errText(e);
      loading = false;
    }
  }

  function errText(e: unknown): string {
    if (typeof e === "string") return e;
    if (e && typeof e === "object" && "message" in e) {
      return String((e as { message: unknown }).message);
    }
    return String(e);
  }

  /** True when most present chunks lack biome ids (biome paint would look blank). */
  function mapHasSparseBiomes(m: WorldMapData): boolean {
    let present = 0;
    let unknown = 0;
    for (const r of m.regions) {
      for (const cell of r.chunks) {
        if (!cell.present) continue;
        present++;
        if ((cell.biomeId ?? -1) < 0) unknown++;
      }
    }
    if (present === 0) return false;
    return unknown / present >= 0.5;
  }

  function statusColor(
    cell: ChunkCell,
    minMod: number,
    maxMod: number,
    shade: number,
  ): string {
    switch (cell.status) {
      case STATUS_EMPTY: return "#3b4252";
      case STATUS_PARTIAL: return "#b08968";
      case STATUS_FULL: {
        const span = Math.max(1, maxMod - minMod);
        const t = Math.max(0, Math.min(1, (cell.lastModified - minMod) / span));
        let r = Math.round(27 + t * 12);
        let g = Math.round(120 + t * 60);
        let b = Math.round(70 + t * 60);
        r = Math.max(0, Math.min(255, Math.round(r * shade)));
        g = Math.max(0, Math.min(255, Math.round(g * shade)));
        b = Math.max(0, Math.min(255, Math.round(b * shade)));
        return `rgb(${r},${g},${b})`;
      }
      default: return "#4a8c5a";
    }
  }

  function fitView() {
    if (!map || !viewport) return;
    const regionW = map.maxRegionX - map.minRegionX + 1;
    const regionH = map.maxRegionZ - map.minRegionZ + 1;
    const W = regionW * GRID * CELL;
    const H = regionH * GRID * CELL;
    const vw = viewport.clientWidth || 800;
    const vh = viewport.clientHeight || 400;
    zoom = Math.max(0.25, Math.min(4, Math.min(vw / Math.max(W, 1), vh / Math.max(H, 1)) * 0.92));
    panX = (vw - W * zoom) / 2;
    panY = (vh - H * zoom) / 2;
  }

  function dateToEpoch(d: string): number {
    if (!d) return 0;
    const t = new Date(d + "T00:00:00").getTime() / 1000;
    return isNaN(t) ? 0 : t;
  }

  function globalMinMax(): [number, number] {
    if (!map) return [0, 1];
    let min = Infinity, max = 0;
    for (const r of map.regions) {
      if (r.present > 0) {
        min = Math.min(min, r.minModified);
        max = Math.max(max, r.maxModified);
      }
    }
    if (!isFinite(min)) return [0, 1];
    return [min, max];
  }

  function globalInhabitedMinMax(): [number, number] {
    if (!map) return [0, 1];
    let min = Infinity, max = 0;
    for (const r of map.regions) {
      for (const cell of r.chunks) {
        if (!cell.present) continue;
        const v = cell.inhabitedTime ?? 0;
        min = Math.min(min, v);
        max = Math.max(max, v);
      }
    }
    if (!isFinite(min)) return [0, 1];
    return [min, max];
  }

  function globalSurfaceMinMax(): [number, number] {
    if (!map) return [0, 1];
    let min = Infinity, max = -Infinity;
    for (const r of map.regions) {
      for (const cell of r.chunks) {
        if (!cell.present) continue;
        const y = cell.surfaceY;
        if (y == null || y === -9999) continue;
        min = Math.min(min, y);
        max = Math.max(max, y);
      }
    }
    if (!isFinite(min) || !isFinite(max)) return [0, 1];
    return [min, max];
  }

  function heatColor(t: number): string {
    const r = Math.round(40 + t * 200);
    const g = Math.round(90 + t * 120);
    const b = Math.round(200 - t * 170);
    return `rgb(${r},${g},${b})`;
  }

  function inHeightRange(cell: ChunkCell): boolean {
    const y = cell.surfaceY;
    if (y == null || y === -9999) return true;
    return y >= heightMin && y <= heightMax;
  }

  function chunkColor(
    cell: ChunkCell,
    mode: ColorMode,
    minMod: number,
    maxMod: number,
    minInh: number,
    maxInh: number,
    minSurf: number,
    maxSurf: number,
  ): string {
    if (!cell.present) return "#15171c";
    const shade = heightShade(cell.surfaceY, minSurf, maxSurf);
    if (mode === "date") {
      const span = Math.max(1, maxMod - minMod);
      const t = Math.max(0, Math.min(1, (cell.lastModified - minMod) / span));
      return heatColor(t);
    }
    if (mode === "inhabited") {
      const span = Math.max(1, maxInh - minInh);
      const v = cell.inhabitedTime ?? 0;
      const t = Math.max(0, Math.min(1, (v - minInh) / span));
      return heatColor(t);
    }
    if (mode === "biome") {
      const id = cell.biomeId ?? -1;
      if (id < 0) {
        // Unknown biomes: use status colors so the map stays readable.
        return statusColor(cell, minMod, maxMod, shade);
      }
      return biomeCss(id, shade);
    }
    if (mode === "height") {
      const y = cell.surfaceY ?? -9999;
      if (y === -9999) return "#15171c";
      const span = Math.max(1, maxSurf - minSurf);
      const t = Math.max(0, Math.min(1, (y - minSurf) / span));
      return heatColor(t);
    }
    return statusColor(cell, minMod, maxMod, shade);
  }

  function mapSize(): { W: number; H: number } {
    if (!map) return { W: 0, H: 0 };
    const regionW = map.maxRegionX - map.minRegionX + 1;
    const regionH = map.maxRegionZ - map.minRegionZ + 1;
    return { W: regionW * GRID * CELL, H: regionH * GRID * CELL };
  }

  function regionInViewport(
    ox: number,
    oy: number,
    cssW: number,
    cssH: number,
  ): boolean {
    const rw = GRID * CELL;
    const rh = GRID * CELL;
    const left = ox * zoom + panX;
    const top = oy * zoom + panY;
    const right = left + rw * zoom;
    const bottom = top + rh * zoom;
    return right >= -8 && bottom >= -8 && left <= cssW + 8 && top <= cssH + 8;
  }

  function draw() {
    if (!canvas || !map) return;
    const { W, H } = mapSize();
    const dpr = window.devicePixelRatio || 1;
    const viewW = Math.max(1, Math.round((viewport?.clientWidth || W) * dpr));
    const viewH = Math.max(1, Math.round((viewport?.clientHeight || H) * dpr));
    canvas.width = viewW;
    canvas.height = viewH;
    canvas.style.width = (viewport?.clientWidth || W) + "px";
    canvas.style.height = (viewport?.clientHeight || H) + "px";

    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const cssW = viewport?.clientWidth || W;
    const cssH = viewport?.clientHeight || H;
    ctx.clearRect(0, 0, cssW, cssH);
    ctx.fillStyle = "#0a0b0e";
    ctx.fillRect(0, 0, cssW, cssH);

    ctx.save();
    ctx.translate(panX, panY);
    ctx.scale(zoom, zoom);

    const [minMod, maxMod] = globalMinMax();
    const [minInh, maxInh] = globalInhabitedMinMax();
    const [minSurf, maxSurf] = globalSurfaceMinMax();

    let visible = 0;
    const drawChunkGrid = showChunkGrid && zoom >= 0.55;
    const lw = 1 / zoom;

    for (const r of map.regions) {
      const ox = (r.regionX - map.minRegionX) * GRID * CELL;
      const oy = (r.regionZ - map.minRegionZ) * GRID * CELL;
      if (!regionInViewport(ox, oy, cssW, cssH)) continue;
      visible++;

      for (let i = 0; i < r.chunks.length; i++) {
        const cell = r.chunks[i];
        const lx = i % GRID;
        const lz = Math.floor(i / GRID);
        const key = `${r.regionX}:${r.regionZ}:${i}`;
        const x = ox + lx * CELL;
        const y = oy + lz * CELL;
        if (!cell.present) {
          ctx.fillStyle = "#12141a";
          ctx.fillRect(x, y, CELL, CELL);
          continue;
        }
        const inRange = inHeightRange(cell);
        ctx.globalAlpha = inRange ? 1 : 0.18;
        ctx.fillStyle = chunkColor(cell, colorMode, minMod, maxMod, minInh, maxInh, minSurf, maxSurf);
        ctx.fillRect(x, y, CELL, CELL);
        ctx.globalAlpha = 1;
        if (selection.has(key)) {
          ctx.fillStyle = "rgba(255, 90, 95, 0.42)";
          ctx.fillRect(x, y, CELL, CELL);
        }
        if (zoom >= 2.2 && (cell.entityCount || cell.structureCount)) {
          if ((cell.entityCount ?? 0) > 0) {
            ctx.fillStyle = "rgba(255, 220, 80, 0.85)";
            ctx.fillRect(x + CELL * 0.15, y + CELL * 0.15, CELL * 0.22, CELL * 0.22);
          }
          if ((cell.structureCount ?? 0) > 0) {
            ctx.fillStyle = "rgba(120, 200, 255, 0.9)";
            ctx.fillRect(x + CELL * 0.6, y + CELL * 0.6, CELL * 0.22, CELL * 0.22);
          }
        }
      }

      if (drawChunkGrid) {
        ctx.strokeStyle = "rgba(255, 255, 255, 0.14)";
        ctx.lineWidth = lw;
        for (let g = 0; g <= GRID; g++) {
          const gx = ox + g * CELL;
          const gy = oy + g * CELL;
          ctx.beginPath();
          ctx.moveTo(gx + 0.5 * lw, oy);
          ctx.lineTo(gx + 0.5 * lw, oy + GRID * CELL);
          ctx.stroke();
          ctx.beginPath();
          ctx.moveTo(ox, gy + 0.5 * lw);
          ctx.lineTo(ox + GRID * CELL, gy + 0.5 * lw);
          ctx.stroke();
        }
      }

      if (showRegions) {
        ctx.strokeStyle = "rgba(0, 0, 0, 0.85)";
        ctx.lineWidth = Math.max(1.5 / zoom, 1 / zoom);
        ctx.strokeRect(ox + 0.5, oy + 0.5, GRID * CELL - 1, GRID * CELL - 1);
        ctx.strokeStyle = "rgba(220, 220, 230, 0.35)";
        ctx.lineWidth = 0.75 / zoom;
        ctx.strokeRect(ox + 1, oy + 1, GRID * CELL - 2, GRID * CELL - 2);
        if (zoom < 0.85) {
          ctx.fillStyle = "rgba(255, 255, 255, 0.55)";
          ctx.font = `${Math.max(9 / zoom, 8)}px ui-monospace, monospace`;
          ctx.fillText(`r.${r.regionX}.${r.regionZ}`, ox + 3 / zoom, oy + 12 / zoom);
        }
      }
    }
    visibleRegionCount = visible;

    if (tool === "box" && dragStart && dragCurrent) {
      const x = Math.min(dragStart.x, dragCurrent.x);
      const y = Math.min(dragStart.y, dragCurrent.y);
      const w = Math.abs(dragCurrent.x - dragStart.x);
      const h = Math.abs(dragCurrent.y - dragStart.y);
      ctx.strokeStyle = dragAdd ? "rgba(120, 200, 255, 0.9)" : "rgba(255, 90, 95, 0.9)";
      ctx.fillStyle = dragAdd ? "rgba(120, 200, 255, 0.15)" : "rgba(255, 90, 95, 0.15)";
      ctx.lineWidth = 1 / zoom;
      ctx.fillRect(x, y, w, h);
      ctx.strokeRect(x + 0.5, y + 0.5, w, h);
    }

    if (tool === "radius" && dragStart && dragCurrent) {
      const dx = dragCurrent.x - dragStart.x;
      const dy = dragCurrent.y - dragStart.y;
      const r = Math.sqrt(dx * dx + dy * dy);
      ctx.beginPath();
      ctx.arc(dragStart.x, dragStart.y, r, 0, Math.PI * 2);
      ctx.fillStyle = dragAdd ? "rgba(120, 200, 255, 0.12)" : "rgba(255, 90, 95, 0.12)";
      ctx.fill();
      ctx.strokeStyle = dragAdd ? "rgba(120, 200, 255, 0.9)" : "rgba(255, 90, 95, 0.9)";
      ctx.lineWidth = 1 / zoom;
      ctx.stroke();
    }

    if (tool === "poly" && polyPoints.length > 0) {
      ctx.beginPath();
      ctx.moveTo(polyPoints[0].x, polyPoints[0].y);
      for (let i = 1; i < polyPoints.length; i++) {
        ctx.lineTo(polyPoints[i].x, polyPoints[i].y);
      }
      ctx.strokeStyle = polyAdd ? "rgba(120, 200, 255, 0.95)" : "rgba(255, 90, 95, 0.95)";
      ctx.lineWidth = 1.5 / zoom;
      ctx.stroke();
      if (polyPoints.length >= 3) {
        ctx.closePath();
        ctx.fillStyle = polyAdd ? "rgba(120, 200, 255, 0.12)" : "rgba(255, 90, 95, 0.12)";
        ctx.fill();
      }
      for (const p of polyPoints) {
        ctx.fillStyle = "#fff";
        ctx.beginPath();
        ctx.arc(p.x, p.y, 2.2 / zoom, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    if (showSpawn && spawnChunk && map) {
      const wx =
        (spawnChunk.cx - map.minRegionX * GRID) * CELL + CELL / 2;
      const wy =
        (spawnChunk.cz - map.minRegionZ * GRID) * CELL + CELL / 2;
      const r = Math.max(5 / zoom, 3);
      ctx.beginPath();
      ctx.arc(wx, wy, r, 0, Math.PI * 2);
      ctx.fillStyle = "rgba(34, 197, 94, 0.95)";
      ctx.fill();
      ctx.strokeStyle = "#fff";
      ctx.lineWidth = 1.5 / zoom;
      ctx.stroke();
      ctx.fillStyle = "rgba(255,255,255,0.95)";
      ctx.font = `${Math.max(10 / zoom, 8)}px sans-serif`;
      ctx.fillText("Spawn", wx + r + 2 / zoom, wy + 3 / zoom);
    }

    ctx.restore();
  }

  /**
   * Map viewport pointer → canvas CSS-pixel space.
   * Under `html { zoom }`, `clientX`/`getBoundingClientRect` are visual while
   * `clientWidth` / pan / 2d drawing use layout CSS px — mix them and selection shifts.
   */
  function pointerToCanvasCss(clientX: number, clientY: number): { x: number; y: number } | null {
    if (!canvas) return null;
    const rect = canvas.getBoundingClientRect();
    const rw = rect.width || 1;
    const rh = rect.height || 1;
    const sx = canvas.clientWidth / rw;
    const sy = canvas.clientHeight / rh;
    return {
      x: (clientX - rect.left) * sx,
      y: (clientY - rect.top) * sy,
    };
  }

  function screenToWorld(clientX: number, clientY: number): { x: number; y: number } | null {
    const p = pointerToCanvasCss(clientX, clientY);
    if (!p) return null;
    return {
      x: (p.x - panX) / zoom,
      y: (p.y - panY) / zoom,
    };
  }

  function cellAtWorld(wx: number, wy: number): { rx: number; rz: number; lx: number; lz: number; idx: number; cell: ChunkCell } | null {
    if (!map) return null;
    const localX = Math.floor(wx / CELL);
    const localZ = Math.floor(wy / CELL);
    const rx = map.minRegionX + Math.floor(localX / GRID);
    const rz = map.minRegionZ + Math.floor(localZ / GRID);
    const r = map.regions.find((rr) => rr.regionX === rx && rr.regionZ === rz);
    if (!r) return null;
    const lx = localX - (rx - map.minRegionX) * GRID;
    const lz = localZ - (rz - map.minRegionZ) * GRID;
    if (lx < 0 || lx >= GRID || lz < 0 || lz >= GRID) return null;
    const idx = lz * GRID + lx;
    return { rx, rz, lx, lz, idx, cell: r.chunks[idx] };
  }

  function cellAtChunk(cx: number, cz: number): ChunkCell | null {
    if (!map) return null;
    const rx = Math.floor(cx / GRID);
    const rz = Math.floor(cz / GRID);
    const lx = cx - rx * GRID;
    const lz = cz - rz * GRID;
    const r = map.regions.find((rr) => rr.regionX === rx && rr.regionZ === rz);
    if (!r) return null;
    return r.chunks[lz * GRID + lx] || null;
  }

  function isEmptyNeighbor(cell: ChunkCell | null): boolean {
    return !cell || !cell.present || cell.status === STATUS_EMPTY;
  }

  function emptyNeighborCount(cx: number, cz: number): number {
    let n = 0;
    if (isEmptyNeighbor(cellAtChunk(cx + 1, cz))) n++;
    if (isEmptyNeighbor(cellAtChunk(cx - 1, cz))) n++;
    if (isEmptyNeighbor(cellAtChunk(cx, cz + 1))) n++;
    if (isEmptyNeighbor(cellAtChunk(cx, cz - 1))) n++;
    return n;
  }

  function cellAt(evt: MouseEvent) {
    const w = screenToWorld(evt.clientX, evt.clientY);
    if (!w) return null;
    return cellAtWorld(w.x, w.y);
  }

  function onMove(evt: MouseEvent) {
    tipX = evt.clientX + 12;
    tipY = evt.clientY + 12;
    const w = screenToWorld(evt.clientX, evt.clientY);
    const hit = w ? cellAtWorld(w.x, w.y) : null;
    if (!hit || !w) hover = null;
    else {
      const cx = worldChunkX(hit.rx, hit.lx);
      const cz = worldChunkZ(hit.rz, hit.lz);
      const fracX = w.x / CELL - Math.floor(w.x / CELL);
      const fracZ = w.y / CELL - Math.floor(w.y / CELL);
      const blockX = cx * 16 + Math.min(15, Math.max(0, Math.floor(fracX * 16)));
      const blockZ = cz * 16 + Math.min(15, Math.max(0, Math.floor(fracZ * 16)));
      hover = {
        rx: hit.rx, rz: hit.rz,
        cx, cz,
        blockX, blockZ,
        status: statusLabel(hit.cell.status),
        modified: hit.cell.lastModified,
        inhabitedTime: hit.cell.inhabitedTime ?? 0,
        dataVersion: hit.cell.dataVersion ?? 0,
        biomeId: hit.cell.biomeId,
        surfaceY: hit.cell.surfaceY,
        entityCount: hit.cell.entityCount,
        structureCount: hit.cell.structureCount,
      };
    }

    if (panning && panLast) {
      const p = pointerToCanvasCss(evt.clientX, evt.clientY);
      if (p) {
        panX += p.x - panLast.x;
        panY += p.y - panLast.y;
        panLast = p;
        draw();
      }
      return;
    }

    if ((tool === "box" || tool === "radius") && dragStart) {
      const w = screenToWorld(evt.clientX, evt.clientY);
      if (w) {
        dragCurrent = w;
        draw();
      }
    }
  }

  function selectRegion(rx: number, rz: number, add: boolean) {
    if (!map) return;
    const r = map.regions.find((rr) => rr.regionX === rx && rr.regionZ === rz);
    if (!r) return;
    const next = new Set(selection);
    r.chunks.forEach((cell, i) => {
      if (!cell.present) return;
      const key = `${rx}:${rz}:${i}`;
      if (add) next.add(key); else next.delete(key);
    });
    selection = next;
    draw();
  }

  function onClick(evt: MouseEvent) {
    if (tool === "poly") {
      const w = screenToWorld(evt.clientX, evt.clientY);
      if (!w) return;
      if (polyPoints.length === 0) {
        polyAdd = !(evt.shiftKey || evt.ctrlKey || evt.metaKey);
      }
      polyPoints = [...polyPoints, w];
      draw();
      return;
    }
    if (tool === "region") {
      const hit = cellAt(evt);
      if (!hit) return;
      selectRegion(hit.rx, hit.rz, !(evt.shiftKey || evt.ctrlKey || evt.metaKey));
      return;
    }
    if (tool !== "click") return;
    const hit = cellAt(evt);
    if (!hit) return;
    const key = `${hit.rx}:${hit.rz}:${hit.idx}`;
    const next = new Set(selection);
    if (next.has(key)) next.delete(key); else next.add(key);
    selection = next;
    draw();
  }

  function pointInPoly(px: number, py: number, pts: { x: number; y: number }[]): boolean {
    let inside = false;
    for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
      const xi = pts[i].x, yi = pts[i].y;
      const xj = pts[j].x, yj = pts[j].y;
      const intersect =
        yi > py !== yj > py &&
        px < ((xj - xi) * (py - yi)) / (yj - yi + 1e-12) + xi;
      if (intersect) inside = !inside;
    }
    return inside;
  }

  function applyPolySelection() {
    if (!map || polyPoints.length < 3) {
      polyPoints = [];
      draw();
      return;
    }
    const next = new Set(selection);
    const xs = polyPoints.map((p) => p.x);
    const ys = polyPoints.map((p) => p.y);
    const minX = Math.min(...xs);
    const maxX = Math.max(...xs);
    const minY = Math.min(...ys);
    const maxY = Math.max(...ys);
    const lx1 = Math.floor(minX / CELL);
    const lz1 = Math.floor(minY / CELL);
    const lx2 = Math.floor(maxX / CELL);
    const lz2 = Math.floor(maxY / CELL);
    for (let lz = lz1; lz <= lz2; lz++) {
      for (let lx = lx1; lx <= lx2; lx++) {
        const wx = lx * CELL + CELL / 2;
        const wy = lz * CELL + CELL / 2;
        if (!pointInPoly(wx, wy, polyPoints)) continue;
        const hit = cellAtWorld(wx, wy);
        if (!hit || !hit.cell.present) continue;
        const key = `${hit.rx}:${hit.rz}:${hit.idx}`;
        if (polyAdd) next.add(key); else next.delete(key);
      }
    }
    selection = next;
    polyPoints = [];
    draw();
  }

  function onDblClick(evt: MouseEvent) {
    if (tool !== "poly") return;
    evt.preventDefault();
    applyPolySelection();
  }

  function onDown(evt: MouseEvent) {
    focusMapViewport();
    if (evt.button === 1 || tool === "pan" || (evt.button === 0 && evt.altKey)) {
      evt.preventDefault();
      panning = true;
      panLast = pointerToCanvasCss(evt.clientX, evt.clientY);
      return;
    }
    if (tool !== "box" && tool !== "radius") return;
    if (evt.button !== 0) return;
    evt.preventDefault();
    const w = screenToWorld(evt.clientX, evt.clientY);
    if (!w) return;
    dragStart = w;
    dragCurrent = { ...w };
    dragAdd = !(evt.shiftKey || evt.ctrlKey || evt.metaKey);
  }

  function onUp(_evt: MouseEvent) {
    if (panning) {
      panning = false;
      panLast = null;
      return;
    }
    if (!dragStart || !dragCurrent || !map) {
      dragStart = null;
      dragCurrent = null;
      return;
    }

    const next = new Set(selection);

    if (tool === "box") {
      const x1 = Math.min(dragStart.x, dragCurrent.x);
      const y1 = Math.min(dragStart.y, dragCurrent.y);
      const xMax = Math.max(dragStart.x, dragCurrent.x);
      const yMax = Math.max(dragStart.y, dragCurrent.y);
      const lx1 = Math.floor(x1 / CELL);
      const lz1 = Math.floor(y1 / CELL);
      const lx2 = Math.floor(xMax / CELL);
      const lz2 = Math.floor(yMax / CELL);
      for (let lz = lz1; lz <= lz2; lz++) {
        for (let lx = lx1; lx <= lx2; lx++) {
          const wx = lx * CELL + CELL / 2;
          const wy = lz * CELL + CELL / 2;
          const hit = cellAtWorld(wx, wy);
          if (!hit || !hit.cell.present) continue;
          if (!matchesStatus(hit.cell)) continue;
          const key = `${hit.rx}:${hit.rz}:${hit.idx}`;
          if (dragAdd) next.add(key); else next.delete(key);
        }
      }
    } else if (tool === "radius") {
      const dx = dragCurrent.x - dragStart.x;
      const dy = dragCurrent.y - dragStart.y;
      let rPx = Math.sqrt(dx * dx + dy * dy);
      if (rPx < CELL) rPx = radiusChunks * CELL;
      const rCells = rPx / CELL;
      const cx = dragStart.x / CELL;
      const cz = dragStart.y / CELL;
      const minLx = Math.floor(cx - rCells);
      const maxLx = Math.ceil(cx + rCells);
      const minLz = Math.floor(cz - rCells);
      const maxLz = Math.ceil(cz + rCells);
      for (let lz = minLz; lz <= maxLz; lz++) {
        for (let lx = minLx; lx <= maxLx; lx++) {
          const ddx = lx + 0.5 - cx;
          const ddz = lz + 0.5 - cz;
          if (ddx * ddx + ddz * ddz > rCells * rCells) continue;
          const hit = cellAtWorld(lx * CELL + CELL / 2, lz * CELL + CELL / 2);
          if (!hit || !hit.cell.present) continue;
          if (!matchesStatus(hit.cell)) continue;
          const key = `${hit.rx}:${hit.rz}:${hit.idx}`;
          if (dragAdd) next.add(key); else next.delete(key);
        }
      }
    }

    selection = next;
    dragStart = null;
    dragCurrent = null;
    draw();
  }

  function matchesStatus(cell: ChunkCell): boolean {
    if (statusFilter === "all") return true;
    if (statusFilter === "empty") return cell.status === STATUS_EMPTY;
    if (statusFilter === "partial") return cell.status === STATUS_PARTIAL;
    if (statusFilter === "full") return cell.status === STATUS_FULL;
    return true;
  }

  function matchesChunkFilter(cell: ChunkCell, rx: number, rz: number, idx: number): boolean {
    if (!cell.present) return false;
    if (!matchesStatus(cell)) return false;

    const from = dateToEpoch(filterFrom);
    const to = filterTo ? dateToEpoch(filterTo) + 86399 : Infinity;
    if (filterFrom || filterTo) {
      const m = cell.lastModified;
      if (m < from || m > to) return false;
    }

    const inh = cell.inhabitedTime ?? 0;
    const inhMin = parseOptNum(inhabitedMin);
    const inhMax = parseOptNum(inhabitedMax);
    if (inhMin != null && inh < inhMin) return false;
    if (inhMax != null && inh > inhMax) return false;

    const dv = cell.dataVersion ?? 0;
    const dvMin = parseOptNum(dataVersionMin);
    const dvMax = parseOptNum(dataVersionMax);
    if (dvMin != null && dv < dvMin) return false;
    if (dvMax != null && dv > dvMax) return false;

    const lx = idx % GRID;
    const lz = Math.floor(idx / GRID);
    const cx = worldChunkX(rx, lx);
    const cz = worldChunkZ(rz, lz);

    const xMin = parseOptNum(xposMin);
    const xMax = parseOptNum(xposMax);
    const zMin = parseOptNum(zposMin);
    const zMax = parseOptNum(zposMax);
    if (xMin != null && cx < xMin) return false;
    if (xMax != null && cx > xMax) return false;
    if (zMin != null && cz < zMin) return false;
    if (zMax != null && cz > zMax) return false;

    const borderN = parseOptNum(borderEmpty);
    if (borderN != null && emptyNeighborCount(cx, cz) < borderN) return false;

    const entMin = parseOptNum(entityCountMin);
    if (entMin != null && (cell.entityCount ?? 0) < entMin) return false;

    const structMin = parseOptNum(structureCountMin);
    if (structMin != null && (cell.structureCount ?? 0) < structMin) return false;

    return true;
  }

  function onWheel(evt: WheelEvent) {
    evt.preventDefault();
    if (!canvas) return;
    const p = pointerToCanvasCss(evt.clientX, evt.clientY);
    if (!p) return;
    const mx = p.x;
    const my = p.y;
    const beforeX = (mx - panX) / zoom;
    const beforeY = (my - panY) / zoom;
    const factor = evt.deltaY < 0 ? 1.12 : 1 / 1.12;
    zoom = Math.max(0.15, Math.min(8, zoom * factor));
    panX = mx - beforeX * zoom;
    panY = my - beforeY * zoom;
    draw();
  }

  function zoomBy(factor: number) {
    if (!viewport) return;
    const mx = viewport.clientWidth / 2;
    const my = viewport.clientHeight / 2;
    const beforeX = (mx - panX) / zoom;
    const beforeY = (my - panY) / zoom;
    zoom = Math.max(0.15, Math.min(8, zoom * factor));
    panX = mx - beforeX * zoom;
    panY = my - beforeY * zoom;
    draw();
  }

  function selectByDate() {
    if (!map) return;
    const from = dateToEpoch(filterFrom);
    const to = filterTo ? dateToEpoch(filterTo) + 86399 : Infinity;
    const next = new Set<string>();
    for (const r of map.regions) {
      r.chunks.forEach((cell, i) => {
        if (!cell.present) return;
        if (!matchesStatus(cell)) return;
        const m = cell.lastModified;
        if (m >= from && m <= to) next.add(`${r.regionX}:${r.regionZ}:${i}`);
      });
    }
    selection = next;
    filterActive = next.size > 0;
    draw();
  }

  function applyChunkFilter() {
    if (!map) return;
    const next = new Set<string>();
    for (const r of map.regions) {
      r.chunks.forEach((cell, i) => {
        if (matchesChunkFilter(cell, r.regionX, r.regionZ, i)) {
          next.add(`${r.regionX}:${r.regionZ}:${i}`);
        }
      });
    }
    selection = next;
    filterActive = next.size > 0;
    draw();
  }

  function selectAll() {
    if (!map) return;
    const next = new Set<string>();
    for (const r of map.regions) {
      r.chunks.forEach((cell, i) => {
        if (cell.present && matchesStatus(cell)) next.add(`${r.regionX}:${r.regionZ}:${i}`);
      });
    }
    selection = next;
    draw();
  }

  function clearSelection() {
    selection = new Set();
    filterActive = false;
    draw();
  }

  function invertSelection() {
    if (!map) return;
    const next = new Set<string>();
    for (const r of map.regions) {
      r.chunks.forEach((cell, i) => {
        const key = `${r.regionX}:${r.regionZ}:${i}`;
        if (cell.present && matchesStatus(cell) && !selection.has(key)) next.add(key);
      });
    }
    selection = next;
    draw();
  }

  function invertSelectedRegions() {
    if (!map) return;
    const touched = new Set<string>();
    for (const key of selection) {
      const parts = key.split(":");
      touched.add(`${parts[0]}:${parts[1]}`);
    }
    if (touched.size === 0) return;
    const next = new Set(selection);
    for (const r of map.regions) {
      const rk = `${r.regionX}:${r.regionZ}`;
      if (!touched.has(rk)) continue;
      r.chunks.forEach((cell, i) => {
        if (!cell.present) return;
        const key = `${r.regionX}:${r.regionZ}:${i}`;
        if (next.has(key)) next.delete(key); else next.add(key);
      });
    }
    selection = next;
    draw();
  }

  /** Expand selection by Chebyshev radius `radiusChunks` (only present map cells). */
  function expandSelection() {
    if (!map || selection.size === 0) return;
    const r = Math.max(0, Math.trunc(Number(radiusChunks)) || 0);
    if (r <= 0) {
      flash(`Selection: ${selection.size} chunk(s)`);
      return;
    }
    const next = new Set(selection);
    for (const key of selection) {
      const parts = key.split(":");
      const rx = Number(parts[0]);
      const rz = Number(parts[1]);
      const idx = Number(parts[2]);
      const lx = idx % GRID;
      const lz = Math.floor(idx / GRID);
      const cx = worldChunkX(rx, lx);
      const cz = worldChunkZ(rz, lz);
      for (let dz = -r; dz <= r; dz++) {
        for (let dx = -r; dx <= r; dx++) {
          if (dx === 0 && dz === 0) continue;
          const ncx = cx + dx;
          const ncz = cz + dz;
          // rem_euclid-style region/local for negatives (Math.floor + remainder)
          const nrx = Math.floor(ncx / GRID);
          const nrz = Math.floor(ncz / GRID);
          const nlx = ncx - nrx * GRID;
          const nlz = ncz - nrz * GRID;
          const region = map.regions.find((rr) => rr.regionX === nrx && rr.regionZ === nrz);
          if (!region) continue;
          const nidx = nlz * GRID + nlx;
          const cell = region.chunks[nidx];
          if (!cell?.present) continue;
          next.add(`${nrx}:${nrz}:${nidx}`);
        }
      }
    }
    selection = next;
    draw();
    flash(`Expanded to ${selection.size} chunk(s)`);
  }

  function selectionPayload() {
    const byRegion = new Map<string, { regionX: number; regionZ: number; indices: number[] }>();
    for (const key of selection) {
      const parts = key.split(":");
      const rx = Number(parts[0]);
      const rz = Number(parts[1]);
      const idx = Number(parts[2]);
      const k = `${rx}:${rz}`;
      if (!byRegion.has(k)) byRegion.set(k, { regionX: rx, regionZ: rz, indices: [] });
      byRegion.get(k)!.indices.push(idx);
    }
    return Array.from(byRegion.values());
  }

  function exportSelectionCsv() {
    if (!map || selection.size === 0) return;
    const byRegion = new Map<string, { rx: number; rz: number; indices: Set<number>; present: number }>();
    for (const r of map.regions) {
      byRegion.set(`${r.regionX}:${r.regionZ}`, {
        rx: r.regionX, rz: r.regionZ, indices: new Set(), present: r.present,
      });
    }
    for (const key of selection) {
      const parts = key.split(":");
      const rk = `${parts[0]}:${parts[1]}`;
      const entry = byRegion.get(rk);
      if (entry) entry.indices.add(Number(parts[2]));
    }
    const lines: string[] = [];
    for (const entry of byRegion.values()) {
      if (entry.indices.size === 0) continue;
      if (entry.present > 0 && entry.indices.size === entry.present) {
        lines.push(`${entry.rx};${entry.rz}`);
      } else {
        for (const idx of entry.indices) {
          const lx = idx % GRID;
          const lz = Math.floor(idx / GRID);
          lines.push(`${entry.rx};${entry.rz};${lx};${lz}`);
        }
      }
    }
    const blob = new Blob([lines.join("\n") + "\n"], { type: "text/csv;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `selection-${worldName || "world"}-${dimension}.csv`;
    document.body.appendChild(a);
    a.click();
    a.remove();
    URL.revokeObjectURL(url);
    flash(`Exported ${selection.size} chunk(s) to CSV`);
  }

  function triggerCsvImport() {
    csvInput?.click();
  }

  async function onCsvImport(evt: Event) {
    const input = evt.target as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file || !map) return;
    try {
      const text = await file.text();
      const lines = text.split(/\r?\n/).map((l) => l.trim()).filter((l) => l.length > 0);
      let doInvert = false;
      const next = new Set<string>();
      for (const line of lines) {
        if (line.toLowerCase() === "inverted") {
          doInvert = true;
          continue;
        }
        const parts = line.split(";").map((p) => p.trim());
        if (parts.length === 2) {
          const rx = Number(parts[0]);
          const rz = Number(parts[1]);
          if (!Number.isFinite(rx) || !Number.isFinite(rz)) continue;
          const r = map.regions.find((rr) => rr.regionX === rx && rr.regionZ === rz);
          if (!r) continue;
          r.chunks.forEach((cell, i) => {
            if (cell.present) next.add(`${rx}:${rz}:${i}`);
          });
        } else if (parts.length >= 4) {
          const rx = Number(parts[0]);
          const rz = Number(parts[1]);
          const lx = Number(parts[2]);
          const lz = Number(parts[3]);
          if (![rx, rz, lx, lz].every(Number.isFinite)) continue;
          if (lx < 0 || lx >= GRID || lz < 0 || lz >= GRID) continue;
          const idx = lz * GRID + lx;
          const r = map.regions.find((rr) => rr.regionX === rx && rr.regionZ === rz);
          if (!r || !r.chunks[idx]?.present) continue;
          next.add(`${rx}:${rz}:${idx}`);
        }
      }
      if (doInvert) {
        const inverted = new Set<string>();
        for (const r of map.regions) {
          r.chunks.forEach((cell, i) => {
            if (!cell.present) return;
            const key = `${r.regionX}:${r.regionZ}:${i}`;
            if (!next.has(key)) inverted.add(key);
          });
        }
        selection = inverted;
      } else {
        selection = next;
      }
      filterActive = selection.size > 0;
      draw();
      flash(`Imported ${selection.size} chunk(s) from CSV`);
    } catch (e) {
      error = String(e);
    }
  }

  async function exportSelectedFolder() {
    if (!map || selection.size === 0 || !$projectPath || !worldName) return;
    error = null;
    try {
      const dest = await open({ directory: true, multiple: false });
      if (!dest || typeof dest !== "string") return;
      const n = await api.worlds.exportChunks(
        worldName,
        selectionPayload(),
        dest,
        dimension,
        $projectPath,
      );
      flash(`Exported ${n} chunk(s) to folder`);
    } catch (e) {
      error = String(e);
    }
  }

  async function importFromFolder() {
    if (readOnly) return;
    if (!map || !$projectPath || !worldName) return;
    error = null;
    try {
      const src = await open({
        directory: true,
        multiple: false,
        title: "Import chunks from world / export folder",
      });
      if (!src || typeof src !== "string") return;
      const n = await api.worlds.importChunks(
        worldName,
        src,
        {
          offsetX: Number(pasteOffsetX) || 0,
          offsetZ: Number(pasteOffsetZ) || 0,
          overwrite: importOverwrite,
          yOffset: Number(importYOffset) || 0,
          sections: importSections.trim() || undefined,
          targetSelections:
            importIntoSelection && selection.size > 0 ? selectionPayload() : undefined,
          dimension,
          sourceDimension: dimension,
        },
        $projectPath,
      );
      await load();
      flash(`Imported ${n} chunk entries (Δ ${pasteOffsetX},${pasteOffsetZ}, Ysec ${importYOffset})`);
    } catch (e) {
      error = String(e);
    }
  }

  async function exportFullMapPng() {
    if (!map || !$projectPath || !worldName) return;
    error = null;
    try {
      const dest = await save({
        defaultPath: `worldmap-${worldName}-${dimension}.png`,
        filters: [{ name: "PNG", extensions: ["png"] }],
      });
      if (!dest || typeof dest !== "string") return;
      const [w, h] = await api.worlds.renderMapPng(
        worldName,
        dest,
        {
          colorMode,
          scale: 4,
          selections: selection.size > 0 ? selectionPayload() : [],
          dimension,
        },
        $projectPath,
      );
      flash(`Saved map PNG ${w}×${h}${selection.size ? " (selection)" : ""}`);
    } catch (e) {
      error = String(e);
    }
  }

  async function applyQuerySelect() {
    if (!map || !$projectPath || !worldName) return;
    const q = filterQuery.trim();
    if (!q) {
      flash("Enter a filter query (e.g. InhabitedTime < 100)");
      return;
    }
    error = null;
    try {
      const hits = await api.worlds.selectByQuery(worldName, q, dimension, $projectPath);
      const next = new Set<string>();
      for (const h of hits) {
        next.add(`${h.regionX}:${h.regionZ}:${h.index}`);
      }
      selection = next;
      filterActive = true;
      draw();
      flash(`Query: ${selection.size} chunk(s)`);
    } catch (e) {
      error = String(e);
    }
  }

  async function swapTwoSelected() {
    if (readOnly) return;
    if (!map || selection.size !== 2 || !$projectPath || !worldName) return;
    const keys = Array.from(selection);
    const parseKey = (key: string) => {
      const parts = key.split(":");
      return {
        regionX: Number(parts[0]),
        regionZ: Number(parts[1]),
        indices: [Number(parts[2])],
      };
    };
    const a = parseKey(keys[0]);
    const b = parseKey(keys[1]);
    error = null;
    try {
      await api.worlds.swapChunks(worldName, a, b, dimension, $projectPath);
      selection = new Set();
      await load();
      flash("Swapped 2 chunks");
    } catch (e) {
      error = String(e);
    }
  }

  function exportPng() {
    if (!canvas) return;
    const url = canvas.toDataURL("image/png");
    const a = document.createElement("a");
    a.href = url;
    a.download = `worldmap-${worldName || "world"}-${dimension}.png`;
    document.body.appendChild(a);
    a.click();
    a.remove();
  }

  async function deleteSelected() {
    if (readOnly) return;
    if (!map || selection.size === 0 || !$projectPath || !worldName) return;
    const n = selection.size;
    if (
      !confirm(
        `Delete ${n} selected chunk(s) in ${dimLabel(dimension)}?\n\n` +
          `This permanently removes chunk data from the save (like MCA Selector).\n` +
          `Make a Backup first — this cannot be undone.`,
      )
    ) {
      return;
    }
    error = null;
    try {
      const cleared = await api.worlds.deleteChunks(worldName, selectionPayload(), dimension, $projectPath);
      selection = new Set();
      filterActive = false;
      await load();
      flash(`Deleted ${cleared} chunks`);
    } catch (e) {
      error = String(e);
    }
  }

  async function purgeRegions() {
    if (readOnly) return;
    if (!$projectPath || !worldName) return;
    if (!confirm(`Purge/compact region files in ${dimLabel(dimension)}?\n\nRemoves empty sectors after deletes and deletes empty .mca files (region + entities + poi).`)) {
      return;
    }
    error = null;
    try {
      const n = await api.worlds.purge(worldName, dimension, $projectPath);
      await load();
      flash(`Purged ${n} region file(s)`);
    } catch (e) {
      error = String(e);
    }
  }

  async function clearMapCache() {
    if (!$projectPath || !worldName) return;
    error = null;
    try {
      const n = await api.worlds.clearCache(worldName, dimension, $projectPath);
      await load();
      flash(`Cleared ${n} map cache file(s)`);
    } catch (e) {
      error = String(e);
    }
  }

  async function warmMapCache() {
    if (!$projectPath || !worldName) return;
    error = null;
    try {
      const n = await api.worlds.warmCache(worldName, dimension, $projectPath);
      flash(`Warmed cache for ${n} region(s)`);
    } catch (e) {
      error = String(e);
    }
  }

  async function copySelected(): Promise<boolean> {
    const path = get(projectPath);
    if (!map || selection.size === 0 || !path || !worldName) {
      if (selection.size === 0) flash("Select chunks to copy (box/click tools, then Ctrl+C)");
      return false;
    }
    error = null;
    try {
      const clip = await api.worlds.copyChunks(worldName, selectionPayload(), dimension, path);
      setWorldMapClipboard(clip, worldName, dimension);
      pasteOffsetX = 0;
      pasteOffsetZ = 0;
      const ents = clip.entities?.length ?? 0;
      const pois = clip.poi?.length ?? 0;
      let msg = `Copied ${clip.chunks.length} chunks`;
      if (ents || pois) {
        const parts: string[] = [];
        if (ents) parts.push(`${ents} entities`);
        if (pois) parts.push(`${pois} poi`);
        msg += ` (+${parts.join(", ")})`;
      }
      flash(msg);
      return true;
    } catch (e) {
      error = String(e);
      return false;
    }
  }

  async function cutSelected() {
    if (readOnly) return;
    if (!map || selection.size === 0 || !$projectPath || !worldName) return;
    const beforeAt = get(worldMapClipboard)?.copiedAt ?? null;
    const ok = await copySelected();
    if (!ok) return;
    // Require a freshly written clipboard — never delete on a stale prior copy.
    const after = get(worldMapClipboard);
    if (!after || after.copiedAt === beforeAt) return;
    if (!confirm(`Cut: also delete ${selection.size} chunks from the world?`)) {
      flash("Copied (cut cancelled — chunks kept)");
      return;
    }
    try {
      await api.worlds.deleteChunks(worldName, selectionPayload(), dimension, $projectPath);
      selection = new Set();
      await load();
      flash("Cut complete");
    } catch (e) {
      error = String(e);
    }
  }

  async function pasteFromClipboard() {
    if (readOnly) return;
    const clipState = get(worldMapClipboard);
    const path = get(projectPath);
    if (!clipState || !path || !worldName) {
      if (!clipState) flash("Clipboard empty — copy chunks first (Ctrl+C)");
      return;
    }
    if (
      clipState.sourceDimension &&
      clipState.sourceDimension !== dimension
    ) {
      if (
        !confirm(
          `Clipboard is from dimension “${clipState.sourceDimension}”, current is “${dimension}”. Paste anyway?`,
        )
      ) {
        return;
      }
    }
    error = null;
    busyLabel = "Pasting…";
    try {
      const pasted = await api.worlds.pasteChunks(
        worldName,
        clipState.clipboard,
        Number(pasteOffsetX) || 0,
        Number(pasteOffsetZ) || 0,
        dimension,
        path,
        importOverwrite,
      );
      await load();
      const from =
        clipState.sourceWorld !== worldName
          ? ` from ${clipState.sourceWorld}`
          : "";
      flash(`Pasted ${pasted} chunks${from} (offset ${pasteOffsetX}, ${pasteOffsetZ})`);
      fromWorldBannerDismissed = true;
    } catch (e) {
      flashError(String(e));
    } finally {
      busyLabel = null;
    }
  }

  function clearClipboard() {
    clearWorldMapClipboard();
    fromWorldBannerDismissed = false;
  }

  async function openFromWorld() {
    if (readOnly) return;
    if (!$projectPath) return;
    fromWorldLoading = true;
    fromWorldOpen = true;
    error = null;
    try {
      fromWorldList = (await api.worlds.list($projectPath)).filter((w) => w.name !== worldName);
      if (!fromWorldName || !fromWorldList.some((w) => w.name === fromWorldName)) {
        fromWorldName = fromWorldList[0]?.name ?? "";
      }
      await loadFromWorldDims();
    } catch (e) {
      flashError(String(e));
      fromWorldOpen = false;
    } finally {
      fromWorldLoading = false;
    }
  }

  async function loadFromWorldDims() {
    if (!$projectPath || !fromWorldName) {
      fromWorldDims = ["overworld"];
      fromWorldDim = "overworld";
      return;
    }
    try {
      fromWorldDims = await api.worlds.dimensions(fromWorldName, $projectPath);
      if (!fromWorldDims.includes(fromWorldDim)) {
        fromWorldDim = fromWorldDims[0] || "overworld";
      }
    } catch {
      fromWorldDims = ["overworld"];
      fromWorldDim = "overworld";
    }
  }

  async function fromWorldCopyOnly() {
    if (readOnly) return;
    if (!map || selection.size === 0 || !$projectPath || !fromWorldName) {
      flash("Select chunks on the map first");
      return;
    }
    fromWorldLoading = true;
    busyLabel = "Copying…";
    error = null;
    try {
      const clip = await api.worlds.copyChunks(
        fromWorldName,
        selectionPayload(),
        fromWorldDim,
        $projectPath,
      );
      setWorldMapClipboard(clip, fromWorldName, fromWorldDim);
      pasteOffsetX = 0;
      pasteOffsetZ = 0;
      flash(`Copied ${clip.chunks.length} from ${fromWorldName} — Paste (Ctrl+V) anytime`);
      fromWorldOpen = false;
    } catch (e) {
      flashError(String(e));
    } finally {
      fromWorldLoading = false;
      busyLabel = null;
    }
  }

  async function fromWorldReplaceSelection() {
    if (readOnly) return;
    if (!map || selection.size === 0 || !$projectPath || !worldName || !fromWorldName) {
      flash("Select target chunks first");
      return;
    }
    fromWorldLoading = true;
    busyLabel = "Replacing…";
    error = null;
    try {
      const clip = await api.worlds.copyChunks(
        fromWorldName,
        selectionPayload(),
        fromWorldDim,
        $projectPath,
      );
      setWorldMapClipboard(clip, fromWorldName, fromWorldDim);
      const pasted = await api.worlds.pasteChunks(
        worldName,
        clip,
        Number(pasteOffsetX) || 0,
        Number(pasteOffsetZ) || 0,
        dimension,
        $projectPath,
        true,
      );
      await load();
      fromWorldOpen = false;
      flash(`Replaced ${pasted} chunk(s) from ${fromWorldName}`);
    } catch (e) {
      flashError(String(e));
    } finally {
      fromWorldLoading = false;
      busyLabel = null;
    }
  }

  function sourceWorldDir(name: string): string {
    const root = $projectPath ?? "";
    const sep = root.includes("\\") ? "\\" : "/";
    return `${root.replace(/[/\\]$/, "")}${sep}saves${sep}${name}`;
  }

  /** Import entire source dimension (or into current selection) via folder import. */
  async function fromWorldImportFolder() {
    if (readOnly) return;
    if (!map || !$projectPath || !worldName || !fromWorldName) return;
    const intoSel = importIntoSelection && selection.size > 0;
    const msg = intoSel
      ? `Import present chunks from "${fromWorldName}" (${fromWorldDim}) into the current selection (${selection.size})?\nUses paste ΔX/ΔZ and overwrite settings.`
      : `Import ALL present chunks from "${fromWorldName}" (${fromWorldDim}) into "${worldName}" (${dimension})?\nThis can be large. Uses paste ΔX/ΔZ and overwrite settings.`;
    if (!confirm(msg)) return;
    fromWorldLoading = true;
    busyLabel = "Importing…";
    error = null;
    try {
      const n = await api.worlds.importChunks(
        worldName,
        sourceWorldDir(fromWorldName),
        {
          offsetX: Number(pasteOffsetX) || 0,
          offsetZ: Number(pasteOffsetZ) || 0,
          overwrite: importOverwrite,
          yOffset: Number(importYOffset) || 0,
          sections: importSections.trim() || undefined,
          targetSelections: intoSel ? selectionPayload() : undefined,
          dimension,
          sourceDimension: fromWorldDim,
        },
        $projectPath,
      );
      await load();
      fromWorldOpen = false;
      flash(`Imported ${n} chunk entries from ${fromWorldName}`);
    } catch (e) {
      flashError(String(e));
    } finally {
      fromWorldLoading = false;
      busyLabel = null;
    }
  }

  function openChunkEditor() {
    if (readOnly) return;
    if (selection.size !== 1) return;
    const key = Array.from(selection)[0];
    const parts = key.split(":");
    editorRx = Number(parts[0]);
    editorRz = Number(parts[1]);
    editorIdx = Number(parts[2]);
    editorOpen = true;
  }

  function closeChunkEditor() {
    editorOpen = false;
  }

  function buildNbtChange(): NbtChangeRequest {
    const change: NbtChangeRequest = {};
    if (chgInhabited !== "") {
      const n = Number(chgInhabited);
      if (Number.isFinite(n)) change.inhabitedTime = n;
    }
    if (chgStatus !== "") change.status = chgStatus;
    if (chgDataVersion !== "") {
      const n = Number(chgDataVersion);
      if (Number.isFinite(n)) change.dataVersion = Math.trunc(n);
    }
    if (chgLightPopulated !== "") {
      const n = Number(chgLightPopulated);
      if (Number.isFinite(n)) change.lightPopulated = Math.trunc(n);
    }
    if (chgBiome !== "") change.biome = chgBiome;
    if (chgDeleteSections !== "") change.deleteSections = chgDeleteSections;
    if (chgReplaceBlocks !== "") change.replaceBlocks = chgReplaceBlocks;
    if (chgDeleteStructureRefs !== "") change.deleteStructureRefs = chgDeleteStructureRefs;
    if (chgPreventRetrogen) change.preventRetrogen = true;
    if (chgForceBlend) change.forceBlend = true;
    if (chgDeleteEntities) change.deleteEntities = true;
    if (chgFixStatus) change.fixStatus = true;
    if (chgForce) change.force = true;
    return change;
  }

  function buildAdvancedFilter(): AdvancedChunkFilter | null {
    const filter: AdvancedChunkFilter = {};
    if (filtEntityNames !== "") filter.entityNames = filtEntityNames;
    if (filtStructureNames !== "") filter.structureNames = filtStructureNames;
    if (filtPaletteNames !== "") filter.paletteNames = filtPaletteNames;
    const minEnt = parseOptNum(entityCountMin);
    if (minEnt != null) filter.minEntities = minEnt;
    if (
      filter.entityNames == null &&
      filter.structureNames == null &&
      filter.paletteNames == null &&
      filter.minEntities == null
    ) {
      return null;
    }
    return filter;
  }

  async function applyContentFilter() {
    if (!$projectPath || !worldName) return;
    const filter = buildAdvancedFilter();
    if (!filter) {
      flash("Fill entity/structure/palette names or ents min");
      return;
    }
    error = null;
    try {
      const refs = await api.worlds.filterAdvanced(
        worldName,
        filter,
        selection.size ? selectionPayload() : undefined,
        dimension,
        $projectPath,
      );
      const next = new Set<string>();
      for (const ref of refs) {
        next.add(`${ref.regionX}:${ref.regionZ}:${ref.index}`);
      }
      selection = next;
      filterActive = next.size > 0;
      draw();
      flash(`Content filter: ${next.size} chunk(s)`);
    } catch (e) {
      error = String(e);
    }
  }

  async function applyNbtChange() {
    if (readOnly) return;
    if (!map || selection.size === 0 || !$projectPath || !worldName) return;
    const change = buildNbtChange();
    const n = selection.size;
    if (!confirm(`Apply NBT change to ${n} selected chunk(s) in ${dimLabel(dimension)}?\n\nThis writes chunk data (make a Backup first).`)) {
      return;
    }
    error = null;
    try {
      const changed = await api.worlds.changeChunks(
        worldName,
        selectionPayload(),
        change,
        dimension,
        $projectPath,
      );
      flash(`Changed ${changed} chunk(s)`);
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  function isEditableTarget(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    if (target.isContentEditable) return true;
    const tag = target.tagName;
    if (tag === "TEXTAREA" || tag === "SELECT") return true;
    if (tag === "INPUT") {
      const type = ((target as HTMLInputElement).type || "text").toLowerCase();
      // Number/checkbox fields (paste ΔX/ΔZ etc.) must not block chunk Ctrl+C/V.
      if (
        type === "number" ||
        type === "range" ||
        type === "checkbox" ||
        type === "radio" ||
        type === "button" ||
        type === "submit" ||
        type === "reset"
      ) {
        return false;
      }
      return true;
    }
    return false;
  }

  function focusMapViewport() {
    viewport?.focus({ preventScroll: true });
  }

  function handleKeydown(e: KeyboardEvent) {
    const ctrl = e.ctrlKey || e.metaKey;
    // Chunk clipboard always wins over generic editable handling when not typing text.
    if (ctrl && (e.code === "KeyC" || e.code === "KeyX" || e.code === "KeyV" || e.code === "KeyA")) {
      if (!isEditableTarget(e.target)) {
        e.preventDefault();
        e.stopPropagation();
        if (e.code === "KeyC") {
          if (!readOnly) void copySelected();
        } else if (e.code === "KeyX") {
          if (!readOnly) void cutSelected();
        } else if (e.code === "KeyV") {
          if (!readOnly) void pasteFromClipboard();
        } else selectAll();
        return;
      }
    }
    if (isEditableTarget(e.target)) return;
    if (e.key === "Enter" && tool === "poly") { e.preventDefault(); applyPolySelection(); }
    else if (e.key === "Escape") {
      if (gotoOpen) { gotoOpen = false; return; }
      if (fromWorldOpen) { fromWorldOpen = false; return; }
      if (toolsDrawerOpen) { toolsDrawerOpen = false; return; }
      if (polyPoints.length > 0) { polyPoints = []; draw(); return; }
      clearSelection();
    }
    else if ((e.key === "Delete" || e.key === "Backspace") && !readOnly) { e.preventDefault(); deleteSelected(); }
    else if (e.key === "+" || e.key === "=") zoomBy(1.15);
    else if (e.key === "-") zoomBy(1 / 1.15);
    else if (e.key === "0") { fitView(); draw(); }
    else if (e.key === "n" || e.key === "N") { e.preventDefault(); cycleColorMode(); }
    else if (e.key === "g" || e.key === "G") { e.preventDefault(); openGoto(); }
    else if ((e.key === "f" || e.key === "F") && !readOnly) {
      e.preventDefault();
      openFromWorld();
    }
    else if (e.key === "t" || e.key === "T") {
      if (layout === "dock") {
        e.preventDefault();
        toolsDrawerOpen = !toolsDrawerOpen;
        if (toolsDrawerOpen && toolsTab === "filters") filtersOpen = true;
      }
    }
  }

  function panToChunk(cx: number, cz: number, zoomTarget?: number) {
    if (!map || !viewport) return;
    if (zoomTarget != null) zoom = zoomTarget;
    const wx = (cx - map.minRegionX * GRID) * CELL + CELL / 2;
    const wy = (cz - map.minRegionZ * GRID) * CELL + CELL / 2;
    panX = viewport.clientWidth / 2 - wx * zoom;
    panY = viewport.clientHeight / 2 - wy * zoom;
    draw();
  }

  function panToRegion(rx: number, rz: number) {
    const cx = rx * GRID + GRID / 2;
    const cz = rz * GRID + GRID / 2;
    panToChunk(cx, cz, Math.max(zoom, 0.6));
  }

  async function openGoto() {
    if (hover) {
      if (gotoMode === "block") {
        gotoX = String(hover.blockX);
        gotoZ = String(hover.blockZ);
      } else {
        gotoX = String(hover.cx);
        gotoZ = String(hover.cz);
      }
    }
    gotoOpen = true;
    await tick();
    gotoXInput?.focus();
    gotoXInput?.select();
  }

  function resetHeightRange() {
    heightMin = -64;
    heightMax = 319;
    draw();
  }

  function applyGoto() {
    const x = Number(gotoX);
    const z = Number(gotoZ);
    if (!Number.isFinite(x) || !Number.isFinite(z)) {
      flash("Invalid coordinates");
      return;
    }
    const cx = gotoMode === "block" ? Math.floor(x / 16) : Math.trunc(x);
    const cz = gotoMode === "block" ? Math.floor(z / 16) : Math.trunc(z);
    panToChunk(cx, cz, Math.max(zoom, 1.2));
    gotoOpen = false;
    flash(`Went to chunk ${cx}, ${cz}`);
  }

  const regionList = $derived(map
    ? [...map.regions]
        .map((r) => ({
          rx: r.regionX,
          rz: r.regionZ,
          present: r.present,
          label: `r.${r.regionX}.${r.regionZ}`,
        }))
        .sort((a, b) => a.rx - b.rx || a.rz - b.rz)
    : []);

  $effect(() => {
    if (heightMin > heightMax) {
      const t = heightMin;
      heightMin = heightMax;
      heightMax = t;
    }
  });

  function onLeave(evt: MouseEvent) {
    hover = null;
    onUp(evt);
  }

  function onResize() {
    draw();
  }

  let viewportRo: ResizeObserver | null = null;

  // Capture phase so chunk clipboard shortcuts work even when a child stops bubbling
  // (e.g. modal backdrop) and before Tauri/webview default handling.
  $effect(() => {
    const onKey = (e: KeyboardEvent) => handleKeydown(e);
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  onMount(() => {
    window.addEventListener("resize", onResize);
  });

  onDestroy(() => {
    window.removeEventListener("resize", onResize);
    viewportRo?.disconnect();
    viewportRo = null;
    clearTimeout(flashTimer);
  });

  // Attach ResizeObserver when viewport binds (not only first onMount).
  $effect(() => {
    const el = viewport;
    if (!el || typeof ResizeObserver === "undefined") return;
    viewportRo?.disconnect();
    let sawSize = false;
    const ro = new ResizeObserver(() => {
      const h = el.clientHeight;
      const w = el.clientWidth;
      if (w < 2 || h < 2) {
        sawSize = false;
        return;
      }
      untrack(() => {
        if (map && !sawSize) fitView();
        sawSize = true;
        draw();
      });
    });
    ro.observe(el);
    viewportRo = ro;
    return () => {
      ro.disconnect();
      if (viewportRo === ro) viewportRo = null;
    };
  });

  $effect(() => {
    if (worldName && $projectPath) load();
  });
  $effect(() => {
    if (!map) return;
    untrack(() => scheduleDraw());
  });
  const canvasCursor = $derived(tool === "pan" ? "grab" : (tool === "click" || tool === "region" || tool === "poly") ? "pointer" : "crosshair");
  $effect(() => {
    // Track filter bounds only; do not track drawScheduled.
    const _lo = heightMin;
    const _hi = heightMax;
    if (_lo === undefined || _hi === undefined) return;
    untrack(() => scheduleDraw());
  });
</script>

<div
  class="world-map"
  class:layout-dock={layout === "dock"}
  class:layout-top={layout === "top"}
  class:tools-open={layout === "dock" && toolsDrawerOpen}
>
  {#if layout === "dock"}
    <div class="mca-topbar">
      <select class="ghost select slim dim-select" bind:value={dimension} onchange={load} title="Dimension">
        {#each dimensions as d (d)}
          <option value={d}>{dimLabel(d)}</option>
        {/each}
      </select>
      {#if readOnly}
        <span class="view-only-badge" title="Map is view-only — mutations disabled">View only</span>
      {/if}
      <span class="mca-sep" aria-hidden="true"></span>
      <div class="tool-group compact tool-segment" role="toolbar" aria-label="Selection tools">
        <button type="button" class="ghost tool-ico" class:active={tool === "pan"} onclick={() => (tool = "pan")} title="Pan"><Minimize2 size={14} /></button>
        <button type="button" class="ghost tool-ico" class:active={tool === "click"} onclick={() => (tool = "click")} title="Click"><MousePointer2 size={14} /></button>
        <button type="button" class="ghost tool-ico" class:active={tool === "box"} onclick={() => (tool = "box")} title="Box"><Square size={14} /></button>
        <button type="button" class="ghost tool-ico" class:active={tool === "radius"} onclick={() => (tool = "radius")} title="Radius"><Circle size={14} /></button>
        <button type="button" class="ghost tool-ico" class:active={tool === "poly"} onclick={() => { tool = "poly"; polyPoints = []; draw(); }} title="Poly"><Pencil size={14} /></button>
        <button type="button" class="ghost tool-ico" class:active={tool === "region"} onclick={() => (tool = "region")} title="Region"><Layers size={14} /></button>
      </div>
      <span class="mca-sep" aria-hidden="true"></span>
      <select class="ghost select slim" bind:value={colorMode} onchange={draw} title="Color mode (N)">
        <option value="biome">biome</option>
        <option value="height">height</option>
        <option value="status">status</option>
        <option value="date">date</option>
        <option value="inhabited">inhabited</option>
      </select>
      <span class="mca-sep" aria-hidden="true"></span>
      <div class="tool-group compact tool-segment">
        <button type="button" class="ghost tool-ico" onclick={() => zoomBy(1.2)} title="Zoom in"><ZoomIn size={14} /></button>
        <button type="button" class="ghost tool-ico" onclick={() => zoomBy(1 / 1.2)} title="Zoom out"><ZoomOut size={14} /></button>
        <button type="button" class="ghost tool-lbl" onclick={() => { fitView(); draw(); }} title="Fit (0)">Fit</button>
        <button type="button" class="ghost tool-lbl" onclick={openGoto} title="Go to… (G)"><Crosshair size={14} /><span>Go</span></button>
      </div>
      {#if !readOnly}
        <span class="mca-sep" aria-hidden="true"></span>
        <div class="tool-group compact tool-segment" role="toolbar" aria-label="Chunk clipboard">
          <button
            type="button"
            class="ghost tool-ico"
            onclick={() => void copySelected()}
            disabled={selection.size === 0 || !$projectPath || !worldName}
            title="Copy selection (Ctrl+C)"
          ><Copy size={14} /></button>
          <button
            type="button"
            class="ghost tool-ico"
            onclick={() => void pasteFromClipboard()}
            disabled={!$worldMapClipboard || !$projectPath || !worldName}
            title="Paste clipboard (Ctrl+V)"
          ><Clipboard size={14} /></button>
        </div>
      {/if}
      <div class="tool-group compact grow-end view-toggles">
        <label class="toggle tight" title="Region borders"><input type="checkbox" bind:checked={showRegions} onchange={draw} /><span>R</span></label>
        <label class="toggle tight" title="Chunk grid"><input type="checkbox" bind:checked={showChunkGrid} onchange={draw} /><span>G</span></label>
        <label class="toggle tight" title="Spawn"><input type="checkbox" bind:checked={showSpawn} onchange={draw} /><span>S</span></label>
        <button type="button" class="ghost tool-ico" class:active={regionRailOpen} onclick={() => (regionRailOpen = !regionRailOpen)} title="Region files"><List size={14} /></button>
        <button
          type="button"
          class="ghost tool-lbl accent-btn"
          class:active={toolsDrawerOpen}
          onclick={() => { toolsDrawerOpen = !toolsDrawerOpen; }}
          title="Tools (T)"
        >
          <Wrench size={14} /><span>Tools</span>
        </button>
        <button type="button" class="ghost tool-ico" onclick={load} disabled={loading} title="Reload"><RefreshCw size={14} class={loading ? "spin" : ""} /></button>
      </div>
    </div>
  {/if}

  <div class="mca-body" class:has-rail={layout === "dock" && regionRailOpen}>
    {#if layout === "dock" && regionRailOpen}
      <aside class="region-rail" aria-label="Region files">
        <div class="region-rail-title">Regions</div>
        <div class="region-rail-list">
          {#each regionList as r (r.label)}
            <button
              type="button"
              class="region-item"
              title="Jump to {r.label}"
              onclick={() => panToRegion(r.rx, r.rz)}
            >
              <span class="r-name">{r.label}</span>
              <span class="r-count">{r.present}</span>
            </button>
          {:else}
            <div class="region-empty">No regions</div>
          {/each}
        </div>
      </aside>
    {/if}

  <aside class="tools-panel" class:drawer={layout === "dock"}>
    {#if layout === "dock"}
      <div class="title tools-title">
        <Wrench size={16} /> Tools
        <button type="button" class="ghost tool-ico drawer-close" onclick={() => (toolsDrawerOpen = false)} title="Close">×</button>
      </div>
      <div class="tools-tabs" role="tablist">
        <button type="button" role="tab" class:active={toolsTab === "select"} aria-selected={toolsTab === "select"} onclick={() => (toolsTab = "select")}>Select</button>
        {#if !readOnly}
          <button type="button" role="tab" class:active={toolsTab === "edit"} aria-selected={toolsTab === "edit"} onclick={() => (toolsTab = "edit")}>Edit</button>
          <button type="button" role="tab" class:active={toolsTab === "export"} aria-selected={toolsTab === "export"} onclick={() => (toolsTab = "export")}>Export</button>
        {/if}
        <button type="button" role="tab" class:active={toolsTab === "filters"} aria-selected={toolsTab === "filters"} onclick={() => { toolsTab = "filters"; filtersOpen = true; }}>Filters</button>
      </div>

      {#if toolsTab === "select"}
        <div class="tools-tab-body">
          <div class="tool-group">
            <button class="ghost" onclick={selectAll} disabled={!map} title="Select all present chunks (Ctrl+A)">
              <CheckSquare size={14} /> All
            </button>
            <button class="ghost" onclick={invertSelection} disabled={!map} title="Invert selection">
              <CheckSquare size={14} /> Invert
            </button>
            <button class="ghost" onclick={invertSelectedRegions} disabled={!map || selection.size === 0} title="Invert only regions that have selection">
              <CheckSquare size={14} /> Invert regions
            </button>
            <button class="ghost" onclick={clearSelection} disabled={selection.size === 0} title="Clear selection (Esc)">
              <XSquare size={14} /> Clear
            </button>
            <button class="ghost" onclick={expandSelection} disabled={!map || selection.size === 0} title="Expand selection by Chebyshev ±r chunks">
              Expand ±r
            </button>
          </div>
          <label class="field-row">
            <span>Radius (chunks)</span>
            <input class="num" type="number" min="1" max="128" bind:value={radiusChunks} title="Default radius for radius tool / expand" />
          </label>
          <p class="hint">Poly: click vertices, Enter or double-click to apply (Shift = subtract).</p>
        </div>
      {:else if toolsTab === "edit" && !readOnly}
        <div class="tools-tab-body">
          <div class="tool-group">
            <button class="ghost" onclick={copySelected} disabled={selection.size === 0 || !$projectPath || !worldName} title="Copy (Ctrl+C)">
              <Copy size={14} /> Copy
            </button>
            <button class="ghost" onclick={cutSelected} disabled={selection.size === 0 || !$projectPath || !worldName} title="Cut (Ctrl+X)">
              <Scissors size={14} /> Cut
            </button>
            <button class="ghost" onclick={pasteFromClipboard} disabled={!$worldMapClipboard || !$projectPath || !worldName} title="Paste (Ctrl+V)">
              <Clipboard size={14} /> Paste {$worldMapClipboard ? `(${$worldMapClipboard.clipboard.chunks.length})` : ""}
            </button>
            <button class="ghost" onclick={clearClipboard} disabled={!$worldMapClipboard} title="Clear clipboard">
              Clear clip
            </button>
            <button class="ghost" onclick={openFromWorld} disabled={!map || !$projectPath} title="From another world (F)">
              <Globe2 size={14} /> From world…
            </button>
            <button class="ghost" onclick={swapTwoSelected} disabled={selection.size !== 2 || !$projectPath || !worldName} title="Swap two selected chunks">
              <ArrowLeftRight size={14} /> Swap
            </button>
            <button class="ghost" onclick={openChunkEditor} disabled={selection.size !== 1} title="Edit NBT of selected chunk">
              <Pencil size={14} /> Edit NBT
            </button>
            <button class="ghost danger" onclick={deleteSelected} disabled={selection.size === 0 || !$projectPath || !worldName} title="Delete selected (Del)">
              <Trash2 size={14} /> Delete {selection.size || ""}
            </button>
          </div>
          <div class="field-grid">
            <label class="field-row"><span>paste ΔX</span><input class="num" type="number" bind:value={pasteOffsetX} /></label>
            <label class="field-row"><span>ΔZ</span><input class="num" type="number" bind:value={pasteOffsetZ} /></label>
          </div>
          <label class="chk" title="Overwrite existing chunks on paste/import destinations">
            <input type="checkbox" bind:checked={importOverwrite} /> Overwrite existing chunks on paste/import
          </label>
          <label class="chk" title="Only import into current selection">
            <input type="checkbox" bind:checked={importIntoSelection} /> Import into selection only
          </label>
          <div class="field-grid">
            <label class="field-row"><span>Ysec</span><input class="num" type="number" bind:value={importYOffset} title="Import vertical section offset" /></label>
            <label class="field-row"><span>secs</span><input class="num wide" type="text" bind:value={importSections} placeholder="all / :-4" /></label>
          </div>
        </div>
      {:else if toolsTab === "export" && !readOnly}
        <div class="tools-tab-body">
          <div class="tool-group">
            <button class="ghost" onclick={exportSelectedFolder} disabled={selection.size === 0} title="Export selected chunks to folder">
              <FolderOutput size={14} /> Folder export
            </button>
            <button class="ghost" onclick={importFromFolder} disabled={!map} title="Import chunks from folder">
              <FolderInput size={14} /> Folder import
            </button>
            <button class="ghost" onclick={exportSelectionCsv} disabled={selection.size === 0} title="Export selection CSV">
              <FileDown size={14} /> CSV ↓
            </button>
            <button class="ghost" onclick={triggerCsvImport} disabled={!map} title="Import selection CSV">
              <FileUp size={14} /> CSV ↑
            </button>
            <button class="ghost" onclick={exportPng} title="Export viewport PNG"><Download size={14} /> Viewport PNG</button>
            <button class="ghost" onclick={exportFullMapPng} disabled={!map} title="Save full map PNG">
              <Download size={14} /> Map PNG
            </button>
            <button class="ghost" onclick={purgeRegions} title="Compact region files after deletes">
              <Eraser size={14} /> Purge
            </button>
            <button class="ghost" onclick={warmMapCache} disabled={!map} title="Warm region metadata cache">Cache</button>
            <button class="ghost" onclick={clearMapCache} disabled={!map} title="Clear region metadata cache">Clear cache</button>
          </div>
        </div>
      {:else}
        <div class="tools-tab-body filters-tab">
          <div class="height-range stacked" title="Height range filter (surfaceY)">
            <span class="hr-label">Y height</span>
            <input type="range" min="-64" max="319" bind:value={heightMin} oninput={draw} />
            <code>{heightMin}</code>
            <span class="hr-dots">…</span>
            <input type="range" min="-64" max="319" bind:value={heightMax} oninput={draw} />
            <code>{heightMax}</code>
            {#if heightMin !== -64 || heightMax !== 319}
              <button type="button" class="y-reset" onclick={resetHeightRange} title="Reset Y to −64…319">↺</button>
            {/if}
          </div>
          <div class="filter-bar stacked">
            <CalendarRange size={14} />
            <span>from</span>
            <input type="date" bind:value={filterFrom} />
            <span>to</span>
            <input type="date" bind:value={filterTo} />
            <button class="mini" onclick={selectByDate} disabled={!map || (!filterFrom && !filterTo)}>Select by date</button>
            <span>status</span>
            <select class="mini-select" bind:value={statusFilter}>
              <option value="all">all</option>
              <option value="empty">empty</option>
              <option value="partial">partial</option>
              <option value="full">full</option>
            </select>
            <span>inh</span>
            <input class="num" type="text" bind:value={inhabitedMin} placeholder="min" />
            <input class="num" type="text" bind:value={inhabitedMax} placeholder="max" />
            <span>dv</span>
            <input class="num" type="text" bind:value={dataVersionMin} placeholder="min" />
            <input class="num" type="text" bind:value={dataVersionMax} placeholder="max" />
            <span>X</span>
            <input class="num" type="text" bind:value={xposMin} placeholder="min" />
            <input class="num" type="text" bind:value={xposMax} placeholder="max" />
            <span>Z</span>
            <input class="num" type="text" bind:value={zposMin} placeholder="min" />
            <input class="num" type="text" bind:value={zposMax} placeholder="max" />
            <span>border</span>
            <input class="num" type="text" bind:value={borderEmpty} placeholder="≥N" />
            <span>ents</span>
            <input class="num" type="text" bind:value={entityCountMin} placeholder="min" />
            <span>structs</span>
            <input class="num" type="text" bind:value={structureCountMin} placeholder="min" />
            <span>ent names</span>
            <input class="num wide" type="text" bind:value={filtEntityNames} placeholder="zombie,…" />
            <span>struct names</span>
            <input class="num wide" type="text" bind:value={filtStructureNames} placeholder="village,…" />
            <span>palette</span>
            <input class="num wide" type="text" bind:value={filtPaletteNames} placeholder="stone,…" />
            <button class="mini" onclick={applyChunkFilter} disabled={!map}><Filter size={12} /> Select by filter</button>
            <button class="mini" onclick={applyContentFilter} disabled={!map || !$projectPath}><Filter size={12} /> Content filter</button>
            <span>query</span>
            <input class="num wide" type="text" bind:value={filterQuery} placeholder="InhabitedTime < 100 AND Status = full" />
            <button class="mini" onclick={applyQuerySelect} disabled={!map || !$projectPath}><Filter size={12} /> Query</button>
            {#if filterActive}<span class="filttag">filter active</span>{/if}
          </div>
          <div class="nbt-bar stacked">
            {#if !readOnly}
              <button class="mini" onclick={() => (nbtPanelOpen = !nbtPanelOpen)} title="Toggle NBT changer">
                <Wrench size={12} /> NBT Changer
              </button>
              {#if nbtPanelOpen}
                <span>inhabited</span>
                <input class="num" type="text" bind:value={chgInhabited} placeholder="ticks" />
                <span>status</span>
                <input class="num wide" type="text" bind:value={chgStatus} placeholder="e.g. full" />
                <span>dataVersion</span>
                <input class="num" type="text" bind:value={chgDataVersion} placeholder="dv" />
                <span>light</span>
                <input class="num" type="text" bind:value={chgLightPopulated} placeholder="0/1" />
                <span>biome</span>
                <input class="num wide" type="text" bind:value={chgBiome} placeholder="plains" />
                <span>del secs</span>
                <input class="num wide" type="text" bind:value={chgDeleteSections} placeholder="all / :-4" />
                <span>replace</span>
                <input class="num wide" type="text" bind:value={chgReplaceBlocks} placeholder="stone=deepslate" />
                <span>del structs</span>
                <input class="num wide" type="text" bind:value={chgDeleteStructureRefs} placeholder="names" />
                <label class="chk"><input type="checkbox" bind:checked={chgPreventRetrogen} /> no retrogen</label>
                <label class="chk"><input type="checkbox" bind:checked={chgForceBlend} /> force blend</label>
                <label class="chk"><input type="checkbox" bind:checked={chgDeleteEntities} /> del ents</label>
                <label class="chk"><input type="checkbox" bind:checked={chgFixStatus} /> fix status</label>
                <label class="chk"><input type="checkbox" bind:checked={chgForce} /> force</label>
                <button class="mini" onclick={applyNbtChange} disabled={selection.size === 0}><Wrench size={12} /> NBT Change</button>
              {/if}
            {/if}
          </div>
        </div>
      {/if}
    {:else}
      <div class="title"><MapIcon size={16} /> MCA map · {worldName}</div>
      <div class="tools">
        <select class="ghost select" bind:value={dimension} onchange={load} title="Dimension">
          {#each dimensions as d (d)}
            <option value={d}>{dimLabel(d)}</option>
          {/each}
        </select>
        <label class="toggle" title="Overlay region boundaries">
          <Layers size={14} /> Regions
          <input type="checkbox" bind:checked={showRegions} onchange={draw} />
        </label>
        <label class="toggle" title="Show world spawn">
          <MapIcon size={14} /> Spawn
          <input type="checkbox" bind:checked={showSpawn} onchange={draw} />
        </label>
        <select class="ghost select" bind:value={colorMode} onchange={draw}>
          <option value="status">by status</option>
          <option value="date">by date</option>
          <option value="inhabited">by inhabited</option>
          <option value="biome">by biome</option>
          <option value="height">by height</option>
        </select>
        <div class="tool-group">
          <button class="ghost" class:active={tool === "pan"} onclick={() => (tool = "pan")}><Minimize2 size={14} /> Pan</button>
          <button class="ghost" class:active={tool === "click"} onclick={() => (tool = "click")}><MousePointer2 size={14} /> Click</button>
          <button class="ghost" class:active={tool === "box"} onclick={() => (tool = "box")}><Square size={14} /> Box</button>
          <button class="ghost" class:active={tool === "radius"} onclick={() => (tool = "radius")}><Circle size={14} /> Radius</button>
          <button class="ghost" class:active={tool === "poly"} onclick={() => { tool = "poly"; polyPoints = []; draw(); }}><Pencil size={14} /> Poly</button>
          <button class="ghost" class:active={tool === "region"} onclick={() => (tool = "region")}><Layers size={14} /> Region</button>
        </div>
        <div class="tool-group">
          <button class="ghost" onclick={selectAll} disabled={!map}><CheckSquare size={14} /> All</button>
          <button class="ghost" onclick={invertSelection} disabled={!map}><CheckSquare size={14} /> Invert</button>
          <button class="ghost" onclick={clearSelection} disabled={selection.size === 0}><XSquare size={14} /> Clear</button>
        </div>
        {#if !readOnly}
          <div class="tool-group">
            <button class="ghost" onclick={copySelected} disabled={selection.size === 0 || !$projectPath || !worldName}><Copy size={14} /> Copy</button>
            <button class="ghost" onclick={cutSelected} disabled={selection.size === 0 || !$projectPath || !worldName}><Scissors size={14} /> Cut</button>
            <button class="ghost" onclick={pasteFromClipboard} disabled={!$worldMapClipboard || !$projectPath || !worldName}><Clipboard size={14} /> Paste</button>
            <button class="ghost" onclick={openFromWorld} disabled={!map}><Globe2 size={14} /> From world…</button>
            <button class="ghost danger" onclick={deleteSelected} disabled={selection.size === 0 || !$projectPath || !worldName}><Trash2 size={14} /> Delete</button>
          </div>
        {/if}
        <div class="tool-group">
          <button class="ghost" onclick={exportPng}><Download size={14} /> PNG</button>
          <button class="ghost" onclick={load} disabled={loading}><RefreshCw size={14} class={loading ? "spin" : ""} /></button>
        </div>
      </div>
      <div class="filter-bar">
        <CalendarRange size={14} />
        <span>from</span>
        <input type="date" bind:value={filterFrom} />
        <span>to</span>
        <input type="date" bind:value={filterTo} />
        <button class="mini" onclick={selectByDate} disabled={!map || (!filterFrom && !filterTo)}>Select by date</button>
        {#if !readOnly}
          <label class="chk" title="Overwrite existing chunks on paste/import">
            <input type="checkbox" bind:checked={importOverwrite} /> overwrite
          </label>
        {/if}
      </div>
    {/if}
  </aside>

  <input
    bind:this={csvInput}
    type="file"
    accept=".csv,text/csv,text/plain"
    style="display:none"
    onchange={onCsvImport}
  />

  <div class="viewport-col">
    {#if layout === "top"}
      <div class="stats">
        {#if map}
          <span>{dimLabel(dimension)}</span>
          <span>{map.regionCount} regions</span>
          <span>{map.totalPresent.toLocaleString()} chunks</span>
          <span>RX {map.minRegionX}…{map.maxRegionX}</span>
          <span>RZ {map.minRegionZ}…{map.maxRegionZ}</span>
          <span>zoom {(zoom * 100).toFixed(0)}%</span>
          <span class="sel">selected: {selection.size}</span>
          {#if $worldMapClipboard}
            <span class="clip">clipboard: {$worldMapClipboard.clipboard.chunks.length} from {$worldMapClipboard.sourceWorld}</span>
          {/if}
          {#if flashMsg}<span class="ok">{flashMsg}</span>{/if}
        {:else if error}
          <span class="err">{error}</span>
        {:else if loading}
          <span>loading…</span>
        {:else}
          <span>no world map</span>
        {/if}
      </div>
    {/if}

    <div
      class="map-scroll"
      bind:this={viewport}
      tabindex="-1"
      role="application"
      aria-label="World chunk map"
      onpointerdown={focusMapViewport}
    >
      {#if map}
        <canvas
          bind:this={canvas}
          style="cursor: {canvasCursor}"
          onmousemove={onMove}
          onclick={onClick}
          ondblclick={onDblClick}
          onmousedown={onDown}
          onmouseup={onUp}
          onmouseleave={onLeave}
          onwheel={(e) => { e.preventDefault(); onWheel(e); }}
          oncontextmenu={(e) => e.preventDefault()}
        ></canvas>
        {#if hover && layout === "top"}
          <div class="hover-tip" style="left: {tipX}px; top: {tipY}px">
            chunk <code>{hover.cx}, {hover.cz}</code> · region {hover.rx},{hover.rz}<br />
            {hover.status}{#if hover.modified} · {new Date(hover.modified * 1000).toLocaleDateString()}{/if}<br />
            inhabited {hover.inhabitedTime} · dataVersion {hover.dataVersion}
            {#if hover.biomeId != null}<br />biome {hover.biomeId}{/if}
            {#if hover.surfaceY != null}<br />surfaceY {hover.surfaceY}{/if}
            {#if hover.entityCount != null}<br />entities {hover.entityCount}{/if}
            {#if hover.structureCount != null}<br />structures {hover.structureCount}{/if}
          </div>
        {/if}
        {#if showClipBanner && $worldMapClipboard}
          <div class="clip-banner" role="status">
            <div class="clip-banner-text">
              <Clipboard size={14} />
              <span>
                <strong>{$worldMapClipboard.clipboard.chunks.length}</strong> chunks from
                <strong>{$worldMapClipboard.sourceWorld}</strong>
                ready to paste
              </span>
            </div>
            <div class="clip-banner-offsets" title="Paste offset (chunks)">
              <label>ΔX <input type="number" bind:value={pasteOffsetX} /></label>
              <label>ΔZ <input type="number" bind:value={pasteOffsetZ} /></label>
            </div>
            <div class="clip-banner-actions">
              <button type="button" class="primary" disabled={!!busyLabel} onclick={pasteFromClipboard}>
                Paste here
              </button>
              <button type="button" class="ghost" onclick={() => (fromWorldBannerDismissed = true)}>Later</button>
              <button type="button" class="ghost" onclick={clearClipboard} title="Clear clipboard">Clear</button>
            </div>
          </div>
        {/if}
        {#if flashMsg && layout === "dock"}
          <div class="flash-toast" class:err={error && flashMsg === error}>{flashMsg}</div>
        {/if}
        {#if loading || busyLabel}
          <div class="map-busy" aria-live="polite">
            <RefreshCw size={16} class="spin" />
            {busyLabel || "Loading map…"}
          </div>
        {/if}
      {:else if loading}
        <div class="map-busy center" aria-live="polite">
          <RefreshCw size={18} class="spin" />
          Loading map…
        </div>
      {:else if error}
        {@const noRegions = /no region/i.test(error) || /not generated yet/i.test(error)}
        <EmptyState
          icon={MapIcon}
          title={noRegions ? "No map yet" : "Map unavailable"}
          description={noRegions
            ? `${error} Launch the pack, explore a bit, then refresh. Try Nether/End if you only visited those. If .mca files exist, clear the map cache and reload.`
            : error}
          actionLabel="Clear cache & reload"
          onaction={() => void clearMapCache()}
        />
      {:else if !loading}
        <EmptyState icon={MapIcon} title="No world selected" description="Open a world to view its 2D map." />
      {/if}
    </div>

    {#if layout === "dock"}
      <div class="mca-status" role="status">
        <span class="st"><span class="st-k">block</span> <strong>{hover ? `${hover.blockX}, ${hover.blockZ}` : "—"}</strong></span>
        <span class="st"><span class="st-k">chunk</span> <strong>{hover ? `${hover.cx}, ${hover.cz}` : "—"}</strong></span>
        <span class="st"><span class="st-k">region</span> <strong>{hover ? `${hover.rx}, ${hover.rz}` : "—"}</strong></span>
        <span class="st"><span class="st-k">selected</span> <strong class:has-sel={selection.size > 0}>{selection.size}</strong></span>
        <span class="st"><span class="st-k">visible</span> <strong>{visibleRegionCount}</strong></span>
        <span class="st"><span class="st-k">total</span> <strong>{map?.regionCount ?? 0}</strong></span>
        {#if $worldMapClipboard}
          <button
            type="button"
            class="clip-status"
            class:cross={crossWorldClip}
            title={crossWorldClip ? "Click to paste from another world" : "Shared clipboard"}
            onclick={() => {
              if (crossWorldClip) pasteFromClipboard();
              else openFromWorld();
            }}
          >
            <span class="st-k">clip</span>
            <strong>{$worldMapClipboard.clipboard.chunks.length}</strong>
            <span class="clip-from">{$worldMapClipboard.sourceWorld}</span>
            {#if crossWorldClip}<span class="clip-cta">Paste</span>{/if}
          </button>
        {/if}
        {#if hover?.surfaceY != null && hover.surfaceY !== -9999}
          <span class="st"><span class="st-k">Y</span> <strong>{hover.surfaceY}</strong></span>
        {/if}
        {#if busyLabel}<span class="busy-status">{busyLabel}</span>{/if}
        <span class="status-zoom">{(zoom * 100).toFixed(0)}%</span>
      </div>
    {:else}
      <div class="legend">
        <span><i style="background:#15171c"></i> absent</span>
        <span><i style="background:#3b4252"></i> empty</span>
        <span><i style="background:#b08968"></i> partial</span>
        <span><i style="background:#2d8c8c"></i> {colorMode === "date" || colorMode === "inhabited" || colorMode === "height" ? "old→new / low→high" : colorMode === "biome" ? "biome" : "full (old→new)"}</span>
        <span><i style="background:rgba(255,90,95,0.7)"></i> selected</span>
        {#if $worldMapClipboard}
          <span class="clip">clip: {$worldMapClipboard.clipboard.chunks.length} from {$worldMapClipboard.sourceWorld}</span>
        {/if}
        <span class="hint">Wheel zoom · Alt/middle pan · Shift subtract · N color · G goto · Del delete</span>
      </div>
    {/if}
  </div>
  </div><!-- /.mca-body -->
</div>

{#if gotoOpen}
  <div class="goto-backdrop" role="presentation" onclick={() => (gotoOpen = false)}>
    <div
      class="goto-dialog"
      role="dialog"
      aria-labelledby="goto-title"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <h3 id="goto-title">Go to</h3>
      <div class="goto-mode">
        <button type="button" class:active={gotoMode === "block"} onclick={() => (gotoMode = "block")}>Block</button>
        <button type="button" class:active={gotoMode === "chunk"} onclick={() => (gotoMode = "chunk")}>Chunk</button>
      </div>
      <div class="goto-fields">
        <label>X <input bind:this={gotoXInput} bind:value={gotoX} onkeydown={(e) => e.key === "Enter" && applyGoto()} /></label>
        <label>Z <input bind:value={gotoZ} onkeydown={(e) => e.key === "Enter" && applyGoto()} /></label>
      </div>
      <div class="goto-actions">
        <button type="button" class="ghost" onclick={() => (gotoOpen = false)}>Cancel</button>
        <button type="button" class="primary" onclick={applyGoto}>Go</button>
      </div>
    </div>
  </div>
{/if}

{#if fromWorldOpen}
  <div class="goto-backdrop" role="presentation" onclick={() => !fromWorldLoading && (fromWorldOpen = false)}>
    <div
      class="goto-dialog from-world-dialog"
      role="dialog"
      aria-labelledby="from-world-title"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <h3 id="from-world-title">From world</h3>
      {#if fromWorldLoading && fromWorldList.length === 0}
        <p class="from-world-hint"><RefreshCw size={14} class="spin" /> Loading worlds…</p>
      {:else if fromWorldList.length === 0}
        <p class="from-world-hint">No other worlds in this pack. Create or copy a world into <code>saves/</code> first.</p>
        <div class="goto-actions">
          <button type="button" class="ghost" onclick={() => (fromWorldOpen = false)}>Close</button>
        </div>
      {:else}
        <div class="from-world-fields">
          <label>
            Source world
            <select
              class="select"
              bind:value={fromWorldName}
              onchange={loadFromWorldDims}
              disabled={fromWorldLoading}
            >
              {#each fromWorldList as w (w.name)}
                <option value={w.name}>{w.name} · {w.sizeFormatted}</option>
              {/each}
            </select>
          </label>
          <label>
            Source dimension
            <select class="select" bind:value={fromWorldDim} disabled={fromWorldLoading}>
              {#each fromWorldDims as d (d)}
                <option value={d}>{dimLabel(d)}</option>
              {/each}
            </select>
          </label>
          <div class="from-world-offsets">
            <label>Paste ΔX <input type="number" bind:value={pasteOffsetX} disabled={fromWorldLoading} /></label>
            <label>Paste ΔZ <input type="number" bind:value={pasteOffsetZ} disabled={fromWorldLoading} /></label>
          </div>
        </div>

        <div class="from-world-steps">
          <div class="fw-step" class:ready={selection.size > 0} class:dim={selection.size === 0}>
            <span class="fw-num">1</span>
            <div>
              <strong>Selection on this map</strong>
              <p>{selection.size > 0 ? `${selection.size} chunk(s) selected` : "Select chunks to replace or copy from source at the same coordinates"}</p>
            </div>
          </div>
          <div class="fw-step" class:ready={!!$worldMapClipboard} class:dim={!$worldMapClipboard}>
            <span class="fw-num">2</span>
            <div>
              <strong>Shared clipboard</strong>
              <p>
                {#if $worldMapClipboard}
                  {$worldMapClipboard.clipboard.chunks.length} from {$worldMapClipboard.sourceWorld}
                {:else}
                  Empty — Copy only, or Copy (Ctrl+C) in another world
                {/if}
              </p>
            </div>
          </div>
        </div>

        <div class="from-world-actions-grid">
          <button
            type="button"
            class="fw-card primary"
            disabled={fromWorldLoading || !fromWorldName || selection.size === 0 || !worldName}
            onclick={fromWorldReplaceSelection}
          >
            <strong>Replace selection</strong>
            <span>Copy source at selected coords → paste here</span>
          </button>
          <button
            type="button"
            class="fw-card"
            disabled={fromWorldLoading || !fromWorldName || selection.size === 0}
            onclick={fromWorldCopyOnly}
          >
            <strong>Copy only</strong>
            <span>Fill clipboard, paste later (Ctrl+V)</span>
          </button>
          <button
            type="button"
            class="fw-card"
            class:recommended={!!$worldMapClipboard && selection.size === 0}
            disabled={fromWorldLoading || !$worldMapClipboard || !worldName}
            onclick={async () => {
              await pasteFromClipboard();
              fromWorldOpen = false;
            }}
          >
            <strong>Paste clipboard</strong>
            <span>Use shared clipboard with Δ offsets</span>
          </button>
          <button
            type="button"
            class="fw-card dangerish"
            disabled={fromWorldLoading || !fromWorldName || !worldName}
            onclick={fromWorldImportFolder}
          >
            <strong>Import dimension</strong>
            <span>All present chunks from source (confirm)</span>
          </button>
        </div>

        <div class="goto-actions">
          <button type="button" class="ghost" disabled={fromWorldLoading} onclick={() => (fromWorldOpen = false)}>Cancel</button>
          {#if fromWorldLoading}
            <span class="from-world-busy"><RefreshCw size={14} class="spin" /> Working…</span>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}

{#if editorOpen}
  <ChunkNbtEditor
    {worldName}
    {dimension}
    regionX={editorRx}
    regionZ={editorRz}
    index={editorIdx}
    onClose={closeChunkEditor}
    onSaved={load}
  />
{/if}

<style>
  .world-map {
    display: flex;
    flex-direction: column;
    gap: 0;
    height: 100%;
    min-height: 0;
    flex: 1;
    width: 100%;
    position: relative;
    overflow: hidden;
  }

  .world-map.layout-top {
    gap: 10px;
  }

  .world-map.layout-dock {
    flex-direction: column;
    gap: 0;
    align-items: stretch;
    background: color-mix(in srgb, var(--bg-primary) 88%, #000 12%);
    --mca-chrome: color-mix(in srgb, var(--bg-secondary) 92%, var(--bg-primary) 8%);
    --mca-chip: color-mix(in srgb, var(--bg-tertiary) 85%, var(--bg-elevated) 15%);
    --mca-ink: var(--text-secondary);
    --mca-ink-strong: var(--text-primary);
    --mca-line: var(--border-color);
  }

  .mca-topbar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    padding: 5px 8px;
    background: var(--mca-chrome);
    border-bottom: 1px solid var(--mca-line);
    flex-shrink: 0;
    z-index: 5;
  }
  .mca-sep {
    width: 1px;
    height: 22px;
    background: var(--mca-line);
    flex-shrink: 0;
    opacity: 0.9;
  }
  .mca-topbar .slim {
    min-width: 0;
    width: auto;
    max-width: 128px;
    padding: 4px 8px;
    font-size: 12px;
    height: 28px;
  }
  .view-only-badge {
    display: inline-flex;
    align-items: center;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    padding: 3px 8px;
    border-radius: 6px;
    color: var(--text-secondary);
    background: color-mix(in srgb, var(--bg-tertiary) 80%, var(--accent-primary) 20%);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 28%, var(--border-color));
    white-space: nowrap;
    flex-shrink: 0;
  }
  .mca-topbar .tool-group.compact {
    flex-direction: row;
    flex-wrap: nowrap;
    gap: 0;
  }
  .mca-topbar .tool-segment {
    display: inline-flex;
    align-items: stretch;
    padding: 2px;
    border: 1px solid var(--mca-line);
    border-radius: var(--border-radius-sm);
    background: var(--mca-chip);
  }
  .mca-topbar .tool-segment .ghost {
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--mca-ink);
    min-height: 26px;
    padding: 0 8px;
  }
  .mca-topbar .tool-segment .tool-ico {
    width: 28px;
    padding: 0;
    justify-content: center;
  }
  .mca-topbar .tool-segment .tool-lbl {
    gap: 8px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.01em;
  }
  .mca-topbar .tool-segment .ghost:hover {
    background: var(--bg-hover);
    color: var(--mca-ink-strong);
  }
  .mca-topbar .tool-segment .ghost.active {
    background: color-mix(in srgb, var(--accent-primary) 16%, transparent);
    color: var(--accent-primary);
  }
  .mca-topbar .grow-end {
    margin-left: auto;
    gap: 8px !important;
    align-items: center;
  }
  .view-toggles .toggle.tight {
    padding: 3px 7px;
    border: 1px solid transparent;
    border-radius: 6px;
    color: var(--mca-ink);
  }
  .view-toggles .toggle.tight:hover {
    background: var(--bg-hover);
    color: var(--mca-ink-strong);
  }
  .view-toggles .toggle.tight:has(input:checked) {
    border-color: color-mix(in srgb, var(--accent-primary) 30%, var(--mca-line));
    background: color-mix(in srgb, var(--accent-primary) 10%, transparent);
    color: var(--accent-primary);
  }
  .view-toggles .toggle.tight input {
    accent-color: var(--accent-primary);
  }
  .height-range {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--mca-ink);
    padding: 2px 8px;
    border: 1px solid var(--mca-line);
    border-radius: var(--border-radius-sm);
    background: var(--mca-chip);
    height: 30px;
  }
  .height-range .hr-label {
    font-weight: 700;
    font-size: 12px;
    letter-spacing: 0.06em;
    color: var(--text-muted);
  }
  .height-range .hr-dots { opacity: 0.5; }
  .height-range input[type="range"] {
    width: 68px;
    padding: 0;
    background: transparent;
    border: none;
    accent-color: var(--accent-primary);
  }
  .height-range code {
    font-size: 12px;
    font-family: var(--font-mono, ui-monospace, monospace);
    color: var(--mca-ink-strong);
    min-width: 28px;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }
  .y-reset {
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    padding: 0 2px;
    font-size: 13px;
    line-height: 1;
  }
  .y-reset:hover { color: var(--mca-ink-strong); }
  .toggle.tight {
    font-size: 12px;
    gap: 8px;
  }
  .accent-btn.active {
    border-color: color-mix(in srgb, var(--accent-primary) 40%, transparent);
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
  }

  .mca-body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    position: relative;
  }
  .layout-dock .mca-body {
    flex-direction: row;
    align-items: stretch;
  }
  .layout-top .mca-body {
    flex-direction: column;
    gap: 10px;
  }

  .region-rail {
    width: 140px;
    flex-shrink: 0;
    border-right: 1px solid var(--mca-line);
    background: var(--mca-chrome);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .region-rail-title {
    padding: 9px 10px 8px;
    font-size: 12px;
    font-weight: 700;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    border-bottom: 1px solid var(--mca-line);
  }
  .region-rail-list {
    flex: 1;
    overflow-y: auto;
    padding: 4px;
  }
  .region-item {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    padding: 6px 8px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text-secondary);
    font-size: 12px;
    font-family: var(--font-mono, ui-monospace, monospace);
    cursor: pointer;
    text-align: left;
  }
  .region-item:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .r-count {
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }
  .region-empty {
    padding: 12px 8px;
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
  }

  .tools-panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex-shrink: 0;
  }

  .layout-top .tools-panel {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 10px 12px;
    align-items: center;
  }
  .layout-top .tools-panel > .title { grid-column: 1; }
  .layout-top .tools-panel > .tools { grid-column: 2; justify-content: flex-end; }
  .layout-top .tools-panel > .filter-bar { grid-column: 1 / -1; }

  .layout-dock .tools-panel.drawer {
    display: none;
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: min(360px, 94vw);
    z-index: 30;
    height: auto;
    max-width: 380px;
    min-width: 280px;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 10px 10px 14px;
    background: color-mix(in srgb, var(--bg-secondary) 94%, var(--bg-elevated) 6%);
    border-left: 1px solid var(--border-color);
    box-shadow: -8px 0 24px color-mix(in srgb, #000 35%, transparent);
    gap: 8px;
    backdrop-filter: blur(8px);
  }
  .layout-dock.tools-open .tools-panel.drawer {
    display: flex;
  }

  .tools-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 700;
  }
  .tools-title .drawer-close {
    margin-left: auto;
    width: 28px;
    height: 28px;
    font-size: 18px;
    line-height: 1;
  }

  .tools-tabs {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
    flex-shrink: 0;
  }
  .tools-tabs button {
    padding: 6px 4px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
  }
  .tools-tabs button.active {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, var(--border-color));
    background: color-mix(in srgb, var(--accent-primary) 12%, var(--bg-tertiary));
    color: var(--text-primary);
  }

  .tools-tab-body {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-height: 0;
  }
  .tools-tab-body .hint {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.4;
  }
  .field-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-secondary);
  }
  .field-row span { flex-shrink: 0; min-width: 4.5em; }
  .field-row .num { flex: 1; min-width: 0; }
  .field-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }

  .filter-bar.stacked,
  .nbt-bar.stacked,
  .height-range.stacked {
    flex-wrap: wrap;
    width: 100%;
  }
  .height-range.stacked {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 0;
  }

  .layout-dock .tools {
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
  }
  .layout-dock .tool-group {
    flex-direction: column;
    align-items: stretch;
  }
  .layout-dock .tool-group .ghost {
    width: 100%;
    justify-content: flex-start;
  }
  .layout-dock .select {
    width: 100%;
  }

  .mca-status {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 8px;
    align-items: center;
    padding: 5px 10px;
    background: color-mix(in srgb, var(--bg-secondary) 80%, #000 20%);
    border-top: 1px solid var(--border-color);
    font-size: 12px;
    font-family: var(--font-mono, ui-monospace, monospace);
    color: var(--text-muted);
    flex-shrink: 0;
    letter-spacing: 0.01em;
  }
  .mca-status .st {
    display: inline-flex;
    align-items: baseline;
    gap: 8px;
    padding: 2px 8px;
    border-right: 1px solid color-mix(in srgb, var(--border-color) 80%, transparent);
  }
  .mca-status .st:last-of-type {
    border-right: none;
  }
  .mca-status .st-k {
    color: var(--text-muted);
    font-size: 12px;
    text-transform: lowercase;
    opacity: 0.85;
  }
  .mca-status strong {
    color: var(--text-primary);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .mca-status strong.has-sel {
    color: color-mix(in srgb, var(--accent-danger) 70%, #fff 30%);
  }
  .status-zoom {
    margin-left: auto;
    padding-left: 8px;
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }

  .flash-toast {
    position: absolute;
    top: 12px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 8;
    padding: 7px 14px;
    border-radius: var(--border-radius-sm);
    background: color-mix(in srgb, var(--bg-elevated) 88%, var(--accent-primary) 12%);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 40%, var(--border-color));
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 600;
    pointer-events: none;
    max-width: min(520px, 90%);
    text-align: center;
    box-shadow: var(--shadow-md);
  }
  .flash-toast.err {
    background: color-mix(in srgb, var(--bg-elevated) 85%, var(--accent-danger) 15%);
    border-color: color-mix(in srgb, var(--accent-danger) 45%, var(--border-color));
    color: color-mix(in srgb, var(--accent-danger) 70%, #fff 30%);
  }

  .clip-banner {
    position: absolute;
    top: 12px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 9;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px 14px;
    padding: 10px 12px;
    max-width: min(640px, calc(100% - 24px));
    border-radius: 10px;
    background: color-mix(in srgb, var(--bg-elevated) 92%, var(--bg-primary) 8%);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 28%, var(--border-color));
    box-shadow: var(--shadow-lg);
    backdrop-filter: blur(10px);
  }
  .clip-banner-text {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-secondary);
  }
  .clip-banner-text strong { color: var(--accent-primary); }
  .clip-banner-offsets {
    display: flex;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .clip-banner-offsets label {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .clip-banner-offsets input {
    width: 56px;
    height: 28px;
    padding: 0 6px;
    border-radius: 6px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-primary);
    font-family: var(--font-mono, ui-monospace, monospace);
  }
  .clip-banner-actions {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }
  .clip-banner-actions .primary {
    padding: 6px 12px;
    border: none;
    border-radius: 7px;
    background: var(--accent-primary);
    color: var(--on-accent);
    font-weight: 700;
    font-size: 12px;
    cursor: pointer;
  }
  .clip-banner-actions .primary:disabled { opacity: 0.5; cursor: default; }
  .clip-banner-actions .ghost {
    height: 28px;
    padding: 0 10px;
    font-size: 12px;
  }

  .map-busy {
    position: absolute;
    bottom: 12px;
    left: 12px;
    z-index: 7;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: var(--border-radius-sm);
    background: color-mix(in srgb, var(--bg-elevated) 90%, transparent);
    border: 1px solid var(--border-color);
    color: var(--text-secondary);
    font-size: 12px;
    backdrop-filter: blur(6px);
  }
  .map-busy.center {
    inset: 0;
    bottom: auto;
    left: auto;
    justify-content: center;
    border: none;
    border-radius: 0;
    background: color-mix(in srgb, var(--bg-primary) 72%, transparent);
  }

  .goto-backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    background: color-mix(in srgb, #000 55%, transparent);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    backdrop-filter: blur(2px);
  }
  .goto-dialog {
    width: min(360px, 100%);
    padding: 18px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
    box-shadow: var(--shadow-lg);
  }
  .goto-dialog h3 {
    margin: 0 0 14px;
    font-size: 15px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }
  .goto-mode {
    display: flex;
    gap: 0;
    margin-bottom: 12px;
    padding: 2px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
    background: var(--bg-tertiary);
  }
  .goto-mode button {
    flex: 1;
    padding: 7px;
    border-radius: 6px;
    border: none;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    font-weight: 600;
    font-size: 12px;
  }
  .goto-mode button.active {
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 14%, transparent);
  }
  .goto-fields {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }
  .goto-fields label {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .goto-fields input {
    height: 36px;
    padding: 0 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-primary);
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 13px;
  }
  .goto-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 14px;
    align-items: center;
  }
  .goto-actions .primary {
    padding: 8px 14px;
    border: none;
    border-radius: var(--border-radius-sm);
    background: var(--accent-primary);
    color: var(--on-accent);
    font-weight: 700;
    cursor: pointer;
  }
  .goto-actions .primary:disabled,
  .goto-actions .ghost:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .from-world-dialog {
    width: min(500px, 100%);
  }
  .from-world-hint {
    margin: 0 0 12px;
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.45;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .from-world-fields {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-bottom: 12px;
  }
  .from-world-fields label {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
  }
  .from-world-fields .select {
    width: 100%;
    height: 36px;
  }
  .from-world-offsets {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }
  .from-world-offsets input {
    height: 32px;
    padding: 0 8px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-primary);
    font-family: var(--font-mono, ui-monospace, monospace);
  }
  .from-world-steps {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 12px;
  }
  .fw-step {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 9px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
  }
  .fw-step.ready {
    border-color: color-mix(in srgb, var(--accent-primary) 35%, var(--border-color));
    background: color-mix(in srgb, var(--accent-primary) 8%, var(--bg-tertiary));
  }
  .fw-step.dim { opacity: 0.7; }
  .fw-step strong {
    display: block;
    font-size: 12px;
    color: var(--text-primary);
  }
  .fw-step p {
    margin: 2px 0 0;
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.35;
  }
  .fw-num {
    flex-shrink: 0;
    width: 22px;
    height: 22px;
    border-radius: 6px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    font-weight: 700;
    background: var(--bg-elevated);
    color: var(--text-secondary);
  }
  .fw-step.ready .fw-num {
    background: color-mix(in srgb, var(--accent-primary) 22%, transparent);
    color: var(--accent-primary);
  }
  .from-world-actions-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin-bottom: 12px;
  }
  .fw-card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    text-align: left;
    padding: 11px;
    border-radius: 9px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    cursor: pointer;
    transition: border-color 120ms ease, background 120ms ease;
  }
  .fw-card strong {
    font-size: 12px;
    color: var(--text-primary);
  }
  .fw-card span {
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.35;
  }
  .fw-card:hover:not(:disabled) {
    background: var(--bg-hover);
    border-color: color-mix(in srgb, var(--text-primary) 18%, var(--border-color));
  }
  .fw-card.primary {
    border-color: color-mix(in srgb, var(--accent-primary) 40%, var(--border-color));
    background: color-mix(in srgb, var(--accent-primary) 10%, var(--bg-tertiary));
  }
  .fw-card.primary strong { color: var(--accent-primary); }
  .fw-card.recommended {
    border-color: color-mix(in srgb, var(--accent-primary) 30%, var(--border-color));
  }
  .fw-card.dangerish:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent-danger) 40%, var(--border-color));
  }
  .fw-card:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .from-world-busy {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
    margin-right: auto;
  }
  .mca-status .clip-status {
    color: var(--text-secondary);
    background: transparent;
    border: none;
    border-right: 1px solid color-mix(in srgb, var(--border-color) 80%, transparent);
    padding: 2px 8px;
    font: inherit;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .mca-status .clip-status.cross {
    color: var(--accent-primary);
  }
  .mca-status .clip-status:hover {
    color: var(--text-primary);
  }
  .mca-status .clip-from {
    max-width: 96px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-muted);
  }
  .clip-cta {
    padding: 1px 6px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--accent-primary) 18%, transparent);
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .busy-status {
    color: var(--text-muted);
    padding: 0 8px;
  }

  .title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 700;
    font-size: 13px;
    color: var(--text-primary);
    flex-shrink: 0;
  }

  .tools {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }

  .tool-group {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    align-items: center;
  }

  .toggle { display: inline-flex; align-items: center; gap: 8px; font-size: 12px; color: var(--text-muted); cursor: pointer; }
  .ghost { display: inline-flex; align-items: center; gap: 8px; }
  .ghost.active { background: rgba(120,200,255,0.15); border-color: rgba(120,200,255,0.4); color: #8fd3ff; }
  .ghost.danger:not(:disabled):hover { background: rgba(255,90,95,0.15); border-color: rgba(255,90,95,0.4); color: #ff7a7e; }
  .select { padding: 5px 8px; background: var(--bg-elevated); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: var(--border-radius-md); font-size: 12px; }

  .viewport-col {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
    min-height: 0;
    flex: 1;
  }

  .layout-dock .viewport-col {
    padding: 0;
    flex: 1;
    min-width: 0;
    min-height: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  .stats { display: flex; gap: 14px; flex-wrap: wrap; font-size: 12px; color: var(--text-muted); flex-shrink: 0; }
  .stats .sel { color: #ff7a7e; }
  .stats .clip { color: #8fd3ff; }
  .stats .ok { color: var(--accent-primary); }
  .stats .err { color: var(--accent-danger); }

  .filter-bar, .nbt-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    font-size: 12px;
    color: var(--text-muted);
  }

  .layout-dock .filter-bar,
  .layout-dock .nbt-bar {
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
  }

  .layout-dock .filter-bar .num,
  .layout-dock .nbt-bar .num,
  .layout-dock .filter-bar .num.wide,
  .layout-dock .nbt-bar .num.wide,
  .layout-dock .filter-bar input[type="date"],
  .layout-dock .mini-select {
    width: 100%;
    box-sizing: border-box;
  }

  .filter-bar input[type="date"] { background: var(--bg-elevated); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: var(--border-radius-md); padding: 4px 6px; font-size: 12px; color-scheme: dark; }
  .filter-bar .num, .nbt-bar .num { width: 64px; background: var(--bg-elevated); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: var(--border-radius-md); padding: 4px 6px; font-size: 12px; }
  .filter-bar .num.wide, .nbt-bar .num.wide { width: 88px; }
  .nbt-bar .chk, .filter-bar .chk { display: inline-flex; align-items: center; gap: 8px; font-size: 12px; cursor: pointer; }
  .mini-select { background: var(--bg-elevated); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: var(--border-radius-md); padding: 4px 6px; font-size: 12px; }
  .mini { display: inline-flex; align-items: center; gap: 8px; font-size: 12px; padding: 4px 8px; border: 1px solid var(--border-color); border-radius: var(--border-radius-md); background: transparent; color: var(--text-secondary); cursor: pointer; }
  .mini:hover:not(:disabled) { background: var(--bg-tertiary); }
  .mini:disabled { opacity: .4; cursor: default; }
  .filttag { color: #8fd3ff; font-size: 12px; }

  .map-scroll {
    position: relative;
    overflow: hidden;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: #0e0f13;
    flex: 1;
    min-height: 0;
  }

  .layout-top .map-scroll {
    height: min(60vh, 560px);
    min-height: 280px;
    flex: 1 1 auto;
  }

  .layout-dock .map-scroll {
    flex: 1 1 0;
    min-height: 240px;
    height: auto;
    border: none;
    border-radius: 0;
    outline: none;
    background: color-mix(in srgb, var(--bg-primary) 90%, #000 10%);
  }
  .map-scroll:focus-visible {
    outline: 2px solid color-mix(in srgb, var(--accent-primary) 55%, transparent);
    outline-offset: -2px;
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
    image-rendering: pixelated;
  }
  .hover-tip { position: fixed; pointer-events: none; z-index: 30; background: var(--bg-elevated); border: 1px solid var(--border-color); border-radius: 6px; padding: 6px 8px; font-size: 12px; color: var(--text-secondary); box-shadow: 0 4px 16px rgba(0,0,0,.4); }
  .hover-tip code { color: var(--accent-primary); }
  .legend { display: flex; gap: 14px; flex-wrap: wrap; font-size: 12px; color: var(--text-muted); align-items: center; flex-shrink: 0; }
  .legend span { display: inline-flex; align-items: center; gap: 8px; }
  .legend i { width: 11px; height: 11px; border-radius: 2px; display: inline-block; border: 1px solid rgba(255,255,255,.1); }
  .legend .hint { opacity: 0.7; margin-left: auto; }
</style>
