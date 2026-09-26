/**
 * Multi-selection state management for quest canvas.
 * Supports Shift+click, Ctrl+A, and marquee selection.
 */

export interface SelectionState {
  selectedIds: Set<string>;
  lastSelectedId: string | null;
}

export function createSelectionState(): SelectionState {
  return {
    selectedIds: new Set(),
    lastSelectedId: null,
  };
}

export function selectSingle(state: SelectionState, id: string | null): SelectionState {
  if (id === null) {
    return { selectedIds: new Set(), lastSelectedId: null };
  }
  return {
    selectedIds: new Set([id]),
    lastSelectedId: id,
  };
}

export function toggleSelect(state: SelectionState, id: string): SelectionState {
  const newSet = new Set(state.selectedIds);
  if (newSet.has(id)) {
    newSet.delete(id);
  } else {
    newSet.add(id);
  }
  return {
    selectedIds: newSet,
    lastSelectedId: id,
  };
}

export function addToSelection(state: SelectionState, id: string): SelectionState {
  const newSet = new Set(state.selectedIds);
  newSet.add(id);
  return {
    selectedIds: newSet,
    lastSelectedId: id,
  };
}

export function selectAll(questIds: string[]): SelectionState {
  return {
    selectedIds: new Set(questIds),
    lastSelectedId: questIds[questIds.length - 1] ?? null,
  };
}

export function selectRange(
  state: SelectionState,
  id: string,
  allIds: string[]
): SelectionState {
  const lastIdx = allIds.indexOf(state.lastSelectedId ?? "");
  const currentIdx = allIds.indexOf(id);
  if (lastIdx === -1 || currentIdx === -1) {
    return addToSelection(state, id);
  }

  const start = Math.min(lastIdx, currentIdx);
  const end = Math.max(lastIdx, currentIdx);
  const newSet = new Set(state.selectedIds);
  for (let i = start; i <= end; i++) {
    newSet.add(allIds[i]);
  }
  return {
    selectedIds: newSet,
    lastSelectedId: id,
  };
}

export function selectInRect(
  rect: { x1: number; y1: number; x2: number; y2: number },
  quests: Array<{ id: string; x: number; y: number; size?: number }>
): Set<string> {
  const minX = Math.min(rect.x1, rect.x2);
  const maxX = Math.max(rect.x1, rect.x2);
  const minY = Math.min(rect.y1, rect.y2);
  const maxY = Math.max(rect.y1, rect.y2);

  const result = new Set<string>();
  for (const q of quests) {
    const half = (q.size && q.size > 0 ? q.size : 1) / 2;
    const qx = q.x;
    const qy = q.y;
    if (qx >= minX && qx <= maxX && qy >= minY && qy <= maxY) {
      result.add(q.id);
    }
  }
  return result;
}

export function clearSelection(): SelectionState {
  return {
    selectedIds: new Set(),
    lastSelectedId: null,
  };
}
