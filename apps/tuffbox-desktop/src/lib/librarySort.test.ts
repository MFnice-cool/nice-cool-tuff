import { describe, expect, it } from "vitest";
import {
  isValidSortMode,
  matchesInstanceFilter,
  sortInstances,
  type SortMode,
} from "./librarySort";
import type { RecentProject } from "./store";

function proj(path: string, name: string, mc = "1.21.1", loader = "fabric"): RecentProject {
  return {
    path,
    info: {
      id: path,
      name,
      minecraftVersion: mc,
      loaderKind: loader,
    } as RecentProject["info"],
  };
}

const STATS = {
  "p-a": { playtime: 120, lastLaunch: "2026-08-28T10:00:00Z" },
  "p-b": { playtime: 900, lastLaunch: "2026-08-27T10:00:00Z" },
  "p-c": { playtime: 30, lastLaunch: "2026-08-29T10:00:00Z" },
} as const;

describe("matchesInstanceFilter", () => {
  const p = proj("p1", "Fabulously Optimized", "1.21.1", "fabric");

  it("empty / whitespace query matches everything", () => {
    expect(matchesInstanceFilter(p, "")).toBe(true);
    expect(matchesInstanceFilter(p, "   ")).toBe(true);
  });

  it("matches by name, case-insensitively", () => {
    expect(matchesInstanceFilter(p, "fabul")).toBe(true);
    expect(matchesInstanceFilter(p, "OPTIMIZED")).toBe(true);
  });

  it("matches by Minecraft version and loader", () => {
    expect(matchesInstanceFilter(p, "1.21")).toBe(true);
    expect(matchesInstanceFilter(p, "forge")).toBe(false);
    expect(matchesInstanceFilter(p, "fabric")).toBe(true);
  });

  it("rejects non-matching queries", () => {
    expect(matchesInstanceFilter(p, "origins")).toBe(false);
  });

  it("matches group names when provided", () => {
    const pack = { path: "/p", info: { name: "Sky Industrial", minecraftVersion: "1.20.1", loaderKind: "fabric" } } as never;
    expect(matchesInstanceFilter(pack, "serve", "My Servers")).toBe(true);
    expect(matchesInstanceFilter(pack, "servers", "My Servers")).toBe(true);
    expect(matchesInstanceFilter(pack, "creative", "My Servers")).toBe(false);
    // without a group the same query only checks instance fields
    expect(matchesInstanceFilter(pack, "serve")).toBe(false);
  });
});

describe("sortInstances", () => {
  const list = [proj("p-a", "Alpha"), proj("p-b", "Bravo"), proj("p-c", "Charlie")];

  it("recent: most recent lastLaunch first, never-played last", () => {
    const sorted = sortInstances(list, "recent", STATS);
    expect(sorted.map((p) => p.path)).toEqual(["p-c", "p-a", "p-b"]);
  });

  it("recent: without stats keeps the store order (stable, all zero keys)", () => {
    const sorted = sortInstances(list, "recent", {});
    expect(sorted.map((p) => p.path)).toEqual(["p-a", "p-b", "p-c"]);
  });

  it("name: alphabetical regardless of stats", () => {
    const shuffled = [list[2], list[0], list[1]];
    expect(sortInstances(shuffled, "name").map((p) => p.path)).toEqual([
      "p-a",
      "p-b",
      "p-c",
    ]);
  });

  it("playtime: descending playtime", () => {
    expect(sortInstances(list, "playtime", STATS).map((p) => p.path)).toEqual([
      "p-b",
      "p-a",
      "p-c",
    ]);
  });

  it("does not mutate the input array", () => {
    const original = [...list];
    sortInstances(list, "name");
    expect(list).toEqual(original);
  });

  it("handles missing stats gracefully (no throw, deterministic order)", () => {
    expect(sortInstances(list, "recent", undefined).map((p) => p.path)).toEqual([
      "p-a",
      "p-b",
      "p-c",
    ]);
  });
});

describe("isValidSortMode", () => {
  it("accepts the three known modes and rejects everything else", () => {
    expect(isValidSortMode("recent")).toBe(true);
    expect(isValidSortMode("name")).toBe(true);
    expect(isValidSortMode("playtime")).toBe(true);
    expect(isValidSortMode("weird")).toBe(false);
    expect(isValidSortMode(null)).toBe(false);
  });
});
