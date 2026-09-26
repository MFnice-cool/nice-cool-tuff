import { describe, expect, it } from "vitest";
import { fuzzySubsequence, parseSearchRegex, searchQuests } from "./search";
import type { QuestChapter } from "./types";

const chapters: QuestChapter[] = [
  {
    id: "ch1",
    title: "Starter",
    quests: [
      {
        id: "q1",
        title: "Gather Cobblestone",
        description: ["Mine some stone"],
        x: 0,
        y: 0,
        dependencies: [],
      },
      {
        id: "q2",
        title: "Craft a Pickaxe",
        description: ["Wood tools first"],
        x: 1,
        y: 0,
        dependencies: [],
      },
    ],
  },
];

describe("searchQuests", () => {
  it("matches multi-term AND on title", () => {
    const hits = searchQuests("gather cobble", chapters);
    expect(hits).toHaveLength(1);
    expect(hits[0]?.quest.id).toBe("q1");
    expect(hits[0]?.matchField).toBe("title");
  });

  it("falls back to fuzzy subsequence when exact fails", () => {
    const hits = searchQuests("gthr cbbl", chapters);
    expect(hits.some((h) => h.quest.id === "q1")).toBe(true);
  });

  it("fuzzySubsequence requires order", () => {
    expect(fuzzySubsequence("abc", "aXbYc")).toBe(true);
    expect(fuzzySubsequence("acb", "abc")).toBe(false);
  });

  it("supports /regex/ and re: patterns", () => {
    expect(parseSearchRegex("/cobble/i")?.source).toBe("cobble");
    const slash = searchQuests("/Craft.*Pick/", chapters);
    expect(slash.some((h) => h.quest.id === "q2")).toBe(true);
    const re = searchQuests("re:gather", chapters);
    expect(re.some((h) => h.quest.id === "q1")).toBe(true);
  });

  it("returns empty for invalid regex", () => {
    expect(parseSearchRegex("/(/")).toBeNull();
    expect(searchQuests("/(/", chapters)).toEqual([]);
  });
});
