import { describe, expect, it } from "vitest";
import { layoutTree, _topologicalLayersForTest, applyLayout, alignQuests, distributeQuests } from "./layout";
import type { QuestData } from "./types";

function q(id: string, deps: string[] = []): QuestData {
  return {
    id,
    title: id,
    description: [],
    x: 0,
    y: 0,
    dependencies: deps,
    tasks: [],
    rewards: [],
    optional: false,
  };
}

describe("topologicalLayers / layoutTree", () => {
  it("layers a simple chain", () => {
    const quests = [q("a"), q("b", ["a"]), q("c", ["b"])];
    const layers = _topologicalLayersForTest(quests);
    expect(layers.map((l) => l.map((x) => x.id))).toEqual([["a"], ["b"], ["c"]]);
  });

  it("terminates on cyclic dependencies and still places all quests", () => {
    const quests = [q("a", ["b"]), q("b", ["a"]), q("c")];
    const layers = _topologicalLayersForTest(quests);
    const ids = layers.flat().map((x) => x.id).sort();
    expect(ids).toEqual(["a", "b", "c"]);
    // c is a root; a/b are cycle leftovers in a later layer
    expect(layers[0].map((x) => x.id)).toEqual(["c"]);
  });

  it("layoutTree returns finite positions for a pure cycle", () => {
    const quests = [q("a", ["b"]), q("b", ["a"])];
    const positions = layoutTree(quests);
    expect(positions.size).toBe(2);
    for (const pos of positions.values()) {
      expect(Number.isFinite(pos.x)).toBe(true);
      expect(Number.isFinite(pos.y)).toBe(true);
    }
    const applied = applyLayout(quests, positions);
    expect(applied.every((x) => Number.isFinite(x.x) && Number.isFinite(x.y))).toBe(true);
  });

  it("ignores self-dependencies", () => {
    const quests = [q("a", ["a"]), q("b", ["a"])];
    const layers = _topologicalLayersForTest(quests);
    expect(layers[0].map((x) => x.id)).toContain("a");
    expect(layers.flat().map((x) => x.id).sort()).toEqual(["a", "b"]);
  });
});

describe("alignQuests", () => {
  const pts = [
    { id: "a", x: 0, y: 5 },
    { id: "b", x: 4, y: 2 },
    { id: "c", x: 8, y: 7 },
  ];

  it("aligns left to min x", () => {
    const r = alignQuests(pts, "left");
    expect(r.get("c")!.x).toBe(0);
    expect(r.get("a")!.x).toBe(0);
    expect(r.get("a")!.y).toBeUndefined();
  });

  it("aligns centerX to bounding box middle", () => {
    const r = alignQuests(pts, "centerX");
    expect(r.get("a")!.x).toBe(4);
  });

  it("aligns top to min y", () => {
    const r = alignQuests(pts, "top");
    expect(r.get("c")!.y).toBe(2);
  });

  it("returns empty for fewer than 2 quests", () => {
    expect(alignQuests([pts[0]!], "left").size).toBe(0);
  });
});

describe("distributeQuests", () => {
  it("distributes evenly between first and last", () => {
    const pts = [
      { id: "a", x: 0, y: 0 },
      { id: "b", x: 1, y: 0 },
      { id: "c", x: 4, y: 0 },
    ];
    const r = distributeQuests(pts, "horizontally");
    expect(r.get("b")!.x).toBe(2);
    expect(r.has("a")).toBe(false);
    expect(r.has("c")).toBe(false);
  });

  it("distributes vertically", () => {
    const pts = [
      { id: "a", x: 0, y: 0 },
      { id: "b", x: 0, y: 3 },
      { id: "c", x: 0, y: 9 },
    ];
    const r = distributeQuests(pts, "vertically");
    expect(r.get("b")!.y).toBe(4.5);
  });

  it("no-op for fewer than 3 quests", () => {
    expect(distributeQuests([{ id: "a", x: 0, y: 0 }, { id: "b", x: 5, y: 0 }], "horizontally").size).toBe(0);
  });
});
