/**
 * Auto-layout algorithms for quest arrangements (ported from quest-editor-web).
 */

import type { QuestData } from "./types";

export interface LayoutOptions {
  spacingX: number;
  spacingY: number;
  centerX: number;
  centerY: number;
}

const DEFAULT_OPTIONS: LayoutOptions = {
  spacingX: 3,
  spacingY: 3,
  centerX: 0,
  centerY: 0,
};

export type LayoutMode = "tree" | "grid" | "circle";

/**
 * Topological sort for dependency-based layout.
 * Cyclic leftovers are appended as a final layer.
 */
function topologicalLayers(quests: QuestData[]): QuestData[][] {
  const questMap = new Map(quests.map((q) => [q.id, q]));
  const inDegree = new Map<string, number>();
  const children = new Map<string, string[]>();

  for (const q of quests) {
    inDegree.set(q.id, 0);
    children.set(q.id, []);
  }

  for (const q of quests) {
    const deps = [...new Set(q.dependencies.filter((d) => questMap.has(d) && d !== q.id))];
    inDegree.set(q.id, deps.length);
    for (const d of deps) {
      const list = children.get(d)!;
      if (!list.includes(q.id)) list.push(q.id);
    }
  }

  const layers: QuestData[][] = [];
  const placed = new Set<string>();
  let currentLayer = quests.filter((q) => (inDegree.get(q.id) ?? 0) === 0);

  while (currentLayer.length > 0) {
    layers.push(currentLayer);
    for (const q of currentLayer) placed.add(q.id);

    const nextLayer: QuestData[] = [];
    for (const q of currentLayer) {
      for (const childId of children.get(q.id) ?? []) {
        if (placed.has(childId)) continue;
        const deg = (inDegree.get(childId) ?? 1) - 1;
        inDegree.set(childId, deg);
        if (deg === 0) {
          nextLayer.push(questMap.get(childId)!);
        }
      }
    }
    currentLayer = nextLayer;
  }

  const leftover = quests.filter((q) => !placed.has(q.id));
  if (leftover.length > 0) layers.push(leftover);

  return layers;
}

export function layoutTree(
  quests: QuestData[],
  options: Partial<LayoutOptions> = {},
): Map<string, { x: number; y: number }> {
  const opts = { ...DEFAULT_OPTIONS, ...options };
  const result = new Map<string, { x: number; y: number }>();
  if (quests.length === 0) return result;

  const layers = topologicalLayers(quests);
  if (layers.length === 0) return result;

  const maxLayerWidth = Math.max(...layers.map((l) => l.length), 1);
  const totalWidth = (maxLayerWidth - 1) * opts.spacingX;
  const totalHeight = (layers.length - 1) * opts.spacingY;
  const startY = opts.centerY - totalHeight / 2;
  void totalWidth;

  for (let layerIdx = 0; layerIdx < layers.length; layerIdx++) {
    const layer = layers[layerIdx]!;
    const layerWidth = (layer.length - 1) * opts.spacingX;
    const layerStartX = opts.centerX - layerWidth / 2;

    for (let questIdx = 0; questIdx < layer.length; questIdx++) {
      const q = layer[questIdx]!;
      result.set(q.id, {
        x: layerStartX + questIdx * opts.spacingX,
        y: startY + layerIdx * opts.spacingY,
      });
    }
  }

  return result;
}

export function layoutGrid(
  quests: QuestData[],
  options: Partial<LayoutOptions> = {},
): Map<string, { x: number; y: number }> {
  const opts = { ...DEFAULT_OPTIONS, ...options };
  const result = new Map<string, { x: number; y: number }>();
  if (quests.length === 0) return result;

  const cols = Math.ceil(Math.sqrt(quests.length));
  const rows = Math.ceil(quests.length / cols);
  const totalWidth = (cols - 1) * opts.spacingX;
  const totalHeight = (rows - 1) * opts.spacingY;
  const startX = opts.centerX - totalWidth / 2;
  const startY = opts.centerY - totalHeight / 2;

  for (let i = 0; i < quests.length; i++) {
    const col = i % cols;
    const row = Math.floor(i / cols);
    result.set(quests[i]!.id, {
      x: startX + col * opts.spacingX,
      y: startY + row * opts.spacingY,
    });
  }

  return result;
}

export function layoutCircle(
  quests: QuestData[],
  options: Partial<LayoutOptions> = {},
): Map<string, { x: number; y: number }> {
  const opts = { ...DEFAULT_OPTIONS, ...options };
  const result = new Map<string, { x: number; y: number }>();
  if (quests.length === 0) return result;
  if (quests.length === 1) {
    result.set(quests[0]!.id, { x: opts.centerX, y: opts.centerY });
    return result;
  }

  const radius = Math.max(quests.length * 0.5, 3);

  for (let i = 0; i < quests.length; i++) {
    const angle = (2 * Math.PI * i) / quests.length - Math.PI / 2;
    result.set(quests[i]!.id, {
      x: opts.centerX + radius * Math.cos(angle),
      y: opts.centerY + radius * Math.sin(angle),
    });
  }

  return result;
}

export function applyLayout<T extends { id: string; x: number; y: number }>(
  quests: T[],
  positions: Map<string, { x: number; y: number }>,
): T[] {
  return quests.map((q) => {
    const pos = positions.get(q.id);
    if (pos) {
      return { ...q, x: Math.round(pos.x * 2) / 2, y: Math.round(pos.y * 2) / 2 };
    }
    return q;
  });
}

export function positionsForMode(
  quests: QuestData[],
  mode: LayoutMode,
): Map<string, { x: number; y: number }> {
  switch (mode) {
    case "grid":
      return layoutGrid(quests);
    case "circle":
      return layoutCircle(quests);
    case "tree":
    default:
      return layoutTree(quests);
  }
}

/** Snap a coordinate to the half-unit quest grid (same rounding as applyLayout). */
function snap(v: number): number {
  return Math.round(v * 2) / 2;
}

export type AlignMode = "left" | "right" | "top" | "bottom" | "centerX" | "centerY";
export type DistributeMode = "horizontally" | "vertically";

/** Minimum spacing between quest centers so 1-size quests never overlap. */
const MIN_SPACING = 1;

/** Align selected quests to the extreme/center line of their current bounding box. */
export function alignQuests(
  quests: { id: string; x: number; y: number }[],
  mode: AlignMode,
): Map<string, { x?: number; y?: number }> {
  const result = new Map<string, { x?: number; y?: number }>();
  if (quests.length < 2) return result;
  const xs = quests.map((q) => q.x);
  const ys = quests.map((q) => q.y);
  const minX = Math.min(...xs);
  const maxX = Math.max(...xs);
  const minY = Math.min(...ys);
  const maxY = Math.max(...ys);
  const centerX = (minX + maxX) / 2;
  const centerY = (minY + maxY) / 2;
  for (const q of quests) {
    switch (mode) {
      case "left":
        result.set(q.id, { x: snap(minX) });
        break;
      case "right":
        result.set(q.id, { x: snap(maxX) });
        break;
      case "top":
        result.set(q.id, { y: snap(minY) });
        break;
      case "bottom":
        result.set(q.id, { y: snap(maxY) });
        break;
      case "centerX":
        result.set(q.id, { x: snap(centerX) });
        break;
      case "centerY":
        result.set(q.id, { y: snap(centerY) });
        break;
    }
  }
  return result;
}

/** Distribute selected quests evenly between the first and the last one
 *  (in current positional order) along one axis. */
export function distributeQuests(
  quests: { id: string; x: number; y: number }[],
  mode: DistributeMode,
): Map<string, { x?: number; y?: number }> {
  const result = new Map<string, { x?: number; y?: number }>();
  if (quests.length < 3) return result;
  const sorted = [...quests].sort((a, b) =>
    mode === "horizontally" ? a.x - b.x : a.y - b.y,
  );
  const first = sorted[0]!;
  const last = sorted[sorted.length - 1]!;
  if (mode === "horizontally") {
    const span = last.x - first.x;
    if (span < MIN_SPACING * (sorted.length - 1)) return result;
    const step = span / (sorted.length - 1);
    sorted.forEach((q, i) => {
      if (i === 0 || i === sorted.length - 1) return;
      result.set(q.id, { x: snap(first.x + step * i) });
    });
  } else {
    const span = last.y - first.y;
    if (span < MIN_SPACING * (sorted.length - 1)) return result;
    const step = span / (sorted.length - 1);
    sorted.forEach((q, i) => {
      if (i === 0 || i === sorted.length - 1) return;
      result.set(q.id, { y: snap(first.y + step * i) });
    });
  }
  return result;
}

/** Exported for unit tests */
export { topologicalLayers as _topologicalLayersForTest };
