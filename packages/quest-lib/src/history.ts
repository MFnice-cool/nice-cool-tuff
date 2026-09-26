/**
 * Undo/Redo history stack for quest editor state.
 * Per-chapter JSON strings with structural sharing across snapshots,
 * plus book meta and reward tables.
 */

export interface HistorySnapshot {
  /** chapter id → JSON.stringify(chapter); unchanged chapters reuse prior string refs */
  chapterJsonById: Record<string, string>;
  chapterOrder: string[];
  chapterGroups: string;
  selectedChapter: string;
  /** JSON.stringify({ title, subtitle, bookSettings }) */
  bookMetaJson: string;
  /** JSON.stringify(rewardTables) */
  rewardTablesJson: string;
}

export interface HistoryBookMeta {
  title: string | null;
  subtitle: string | null;
  bookSettings: Record<string, unknown>;
}

export interface HistoryExtras {
  bookMetaJson: string;
  rewardTablesJson: string;
}

export interface HistoryState {
  undoStack: HistorySnapshot[];
  redoStack: HistorySnapshot[];
  maxSize: number;
}

type ChapterLike = { id?: unknown };

export function createHistoryState(maxSize: number = 100): HistoryState {
  return {
    undoStack: [],
    redoStack: [],
    maxSize,
  };
}

export function serializeBookMeta(meta: HistoryBookMeta): string {
  return JSON.stringify({
    title: meta.title ?? null,
    subtitle: meta.subtitle ?? null,
    bookSettings: meta.bookSettings ?? {},
  });
}

export function parseBookMeta(json: string): HistoryBookMeta {
  try {
    const v = JSON.parse(json) as Partial<HistoryBookMeta>;
    return {
      title: (v.title as string | null | undefined) ?? null,
      subtitle: (v.subtitle as string | null | undefined) ?? null,
      bookSettings:
        v.bookSettings && typeof v.bookSettings === "object" && !Array.isArray(v.bookSettings)
          ? (v.bookSettings as Record<string, unknown>)
          : {},
    };
  } catch {
    return { title: null, subtitle: null, bookSettings: {} };
  }
}

function chapterId(ch: unknown, index: number): string {
  const id = (ch as ChapterLike)?.id;
  return typeof id === "string" && id.length > 0 ? id : `__idx_${index}`;
}

function buildSnapshot(
  chapters: unknown[],
  chapterGroups: unknown[],
  selectedChapter: string,
  extras: HistoryExtras,
  prevMap?: Record<string, string>,
): HistorySnapshot {
  const chapterJsonById: Record<string, string> = {};
  const chapterOrder: string[] = [];
  for (let i = 0; i < chapters.length; i++) {
    const ch = chapters[i];
    const id = chapterId(ch, i);
    chapterOrder.push(id);
    const next = JSON.stringify(ch);
    const prev = prevMap?.[id];
    chapterJsonById[id] = prev !== undefined && prev === next ? prev : next;
  }
  return {
    chapterJsonById,
    chapterOrder,
    chapterGroups: JSON.stringify(chapterGroups),
    selectedChapter,
    bookMetaJson: extras.bookMetaJson,
    rewardTablesJson: extras.rewardTablesJson,
  };
}

function snapshotsEqual(a: HistorySnapshot, b: HistorySnapshot): boolean {
  if (a.selectedChapter !== b.selectedChapter) return false;
  if (a.chapterGroups !== b.chapterGroups) return false;
  if (a.bookMetaJson !== b.bookMetaJson) return false;
  if (a.rewardTablesJson !== b.rewardTablesJson) return false;
  if (a.chapterOrder.length !== b.chapterOrder.length) return false;
  for (let i = 0; i < a.chapterOrder.length; i++) {
    if (a.chapterOrder[i] !== b.chapterOrder[i]) return false;
  }
  for (const id of a.chapterOrder) {
    if (a.chapterJsonById[id] !== b.chapterJsonById[id]) return false;
  }
  return true;
}

export function materializeChapters(snapshot: HistorySnapshot): unknown[] {
  return snapshot.chapterOrder.map((id) => {
    const raw = snapshot.chapterJsonById[id];
    if (raw === undefined) {
      throw new Error(`history snapshot missing chapter ${id}`);
    }
    return JSON.parse(raw);
  });
}

export function pushSnapshot(
  state: HistoryState,
  chapters: unknown[],
  chapterGroups: unknown[],
  selectedChapter: string,
  extras: HistoryExtras = { bookMetaJson: "{}", rewardTablesJson: "[]" },
): HistoryState {
  const top = state.undoStack[state.undoStack.length - 1];
  const snapshot = buildSnapshot(
    chapters,
    chapterGroups,
    selectedChapter,
    extras,
    top?.chapterJsonById,
  );

  if (top && snapshotsEqual(top, snapshot)) {
    return state;
  }

  const newUndo = [...state.undoStack, snapshot];
  if (newUndo.length > state.maxSize) {
    newUndo.shift();
  }

  return {
    ...state,
    undoStack: newUndo,
    redoStack: [],
  };
}

export function undo(
  state: HistoryState,
  currentChapters: unknown[],
  currentChapterGroups: unknown[],
  currentSelectedChapter: string,
  currentExtras: HistoryExtras,
): { state: HistoryState; snapshot: HistorySnapshot | null } {
  if (state.undoStack.length === 0) {
    return { state, snapshot: null };
  }

  const top = state.undoStack[state.undoStack.length - 1];
  const currentSnapshot = buildSnapshot(
    currentChapters,
    currentChapterGroups,
    currentSelectedChapter,
    currentExtras,
    top.chapterJsonById,
  );

  const newUndo = [...state.undoStack];
  const snapshot = newUndo.pop()!;

  return {
    state: {
      ...state,
      undoStack: newUndo,
      redoStack: [...state.redoStack, currentSnapshot],
    },
    snapshot,
  };
}

export function redo(
  state: HistoryState,
  currentChapters: unknown[],
  currentChapterGroups: unknown[],
  currentSelectedChapter: string,
  currentExtras: HistoryExtras,
): { state: HistoryState; snapshot: HistorySnapshot | null } {
  if (state.redoStack.length === 0) {
    return { state, snapshot: null };
  }

  const topRedo = state.redoStack[state.redoStack.length - 1];
  const currentSnapshot = buildSnapshot(
    currentChapters,
    currentChapterGroups,
    currentSelectedChapter,
    currentExtras,
    state.undoStack[state.undoStack.length - 1]?.chapterJsonById ??
      topRedo.chapterJsonById,
  );

  const newRedo = [...state.redoStack];
  const snapshot = newRedo.pop()!;

  return {
    state: {
      ...state,
      undoStack: [...state.undoStack, currentSnapshot],
      redoStack: newRedo,
    },
    snapshot,
  };
}

export function canUndo(state: HistoryState): boolean {
  return state.undoStack.length > 0;
}

export function canRedo(state: HistoryState): boolean {
  return state.redoStack.length > 0;
}

export function clearHistory(): HistoryState {
  return {
    undoStack: [],
    redoStack: [],
    maxSize: 100,
  };
}

/** Chapter ids whose JSON changed between two snapshots (added/removed/edited). */
export function diffDirtyChapterIds(
  before: HistorySnapshot | { chapterJsonById: Record<string, string> },
  after: HistorySnapshot | { chapterJsonById: Record<string, string> },
): string[] {
  const a = before.chapterJsonById;
  const b = after.chapterJsonById;
  const ids = new Set([...Object.keys(a), ...Object.keys(b)]);
  const dirty: string[] = [];
  for (const id of ids) {
    if (a[id] !== b[id]) dirty.push(id);
  }
  return dirty;
}

/**
 * Dirty chapter ids relative to a saved baseline (disk / last successful save).
 * New chapters (missing from baseline) and edited chapters are dirty.
 * Chapters only in baseline (deleted in editor) are not listed — callers track deletes separately.
 */
export function dirtyIdsAgainstBaseline(
  currentChapterJsonById: Record<string, string>,
  savedBaseline: Record<string, string>,
): string[] {
  const dirty: string[] = [];
  for (const id of Object.keys(currentChapterJsonById)) {
    if (currentChapterJsonById[id] !== savedBaseline[id]) dirty.push(id);
  }
  return dirty;
}

/** Merge one chapter's JSON into a baseline map (after successful save). */
export function patchSavedBaseline(
  baseline: Record<string, string>,
  chapterId: string,
  chapterJson: string,
): Record<string, string> {
  return { ...baseline, [chapterId]: chapterJson };
}

/** Build chapterJsonById map for the current editor chapters (no structural sharing). */
export function chapterJsonMap(chapters: unknown[]): Record<string, string> {
  const map: Record<string, string> = {};
  chapters.forEach((ch, i) => {
    map[chapterId(ch, i)] = JSON.stringify(ch);
  });
  return map;
}
