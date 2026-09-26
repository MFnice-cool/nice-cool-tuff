import { describe, expect, it } from "vitest";
import {
  createHistoryState,
  dirtyIdsAgainstBaseline,
  materializeChapters,
  patchSavedBaseline,
  pushSnapshot,
} from "./history";

describe("history structural sharing", () => {
  it("reuses string refs for unchanged chapters", () => {
    let state = createHistoryState();
    const chA = { id: "A", title: "Alpha", quests: [] };
    const chB = { id: "B", title: "Beta", quests: [] };
    state = pushSnapshot(state, [chA, chB], [], "A");
    const first = state.undoStack[0]!;
    expect(first.chapterOrder).toEqual(["A", "B"]);

    const chA2 = { id: "A", title: "Alpha changed", quests: [] };
    state = pushSnapshot(state, [chA2, chB], [], "A");
    const second = state.undoStack[1]!;

    expect(Object.is(second.chapterJsonById.B, first.chapterJsonById.B)).toBe(
      true,
    );
    expect(Object.is(second.chapterJsonById.A, first.chapterJsonById.A)).toBe(
      false,
    );

    const restored = materializeChapters(second) as Array<{ id: string; title: string }>;
    expect(restored.map((c) => c.id)).toEqual(["A", "B"]);
    expect(restored[0]?.title).toBe("Alpha changed");
    expect(restored[1]?.title).toBe("Beta");
  });

  it("skips no-op pushes", () => {
    let state = createHistoryState();
    const chapters = [{ id: "A", title: "T", quests: [] }];
    state = pushSnapshot(state, chapters, [], "A");
    const afterFirst = state.undoStack.length;
    state = pushSnapshot(state, chapters, [], "A");
    expect(state.undoStack.length).toBe(afterFirst);
  });
});

describe("dirtyIdsAgainstBaseline", () => {
  it("marks edited and new chapters dirty; clean after revert", () => {
    const saved = {
      A: JSON.stringify({ id: "A", title: "Alpha", quests: [] }),
      B: JSON.stringify({ id: "B", title: "Beta", quests: [] }),
    };
    const edited = {
      A: JSON.stringify({ id: "A", title: "Alpha changed", quests: [] }),
      B: saved.B,
      C: JSON.stringify({ id: "C", title: "New", quests: [] }),
    };
    expect(dirtyIdsAgainstBaseline(edited, saved).sort()).toEqual(["A", "C"]);
    expect(dirtyIdsAgainstBaseline(saved, saved)).toEqual([]);
  });

  it("patchSavedBaseline clears dirtiness for that chapter", () => {
    let baseline = {
      A: JSON.stringify({ id: "A", title: "Old", quests: [] }),
    };
    const next = JSON.stringify({ id: "A", title: "New", quests: [] });
    baseline = patchSavedBaseline(baseline, "A", next);
    expect(dirtyIdsAgainstBaseline({ A: next }, baseline)).toEqual([]);
  });
});
