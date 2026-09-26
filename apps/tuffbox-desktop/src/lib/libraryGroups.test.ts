import { beforeEach, describe, expect, it } from "vitest";
import {
  DEFAULT_GROUP,
  folderFromDrop,
  getGroup,
  listGroupNames,
  loadCollapsedGroups,
  loadGroupMap,
  setGroup,
  suggestFolderName,
  toggleCollapsed,
  type GroupMap,
} from "./libraryGroups";

beforeEach(() => {
  localStorage.clear();
});

describe("group map persistence", () => {
  it("returns an empty map when nothing stored", () => {
    expect(loadGroupMap()).toEqual({});
  });

  it("setGroup assigns a named group and persists it", () => {
    const next = setGroup({}, "p1", "Skyblock");
    expect(getGroup(next, "p1")).toBe("Skyblock");
    expect(loadGroupMap()["p1"]).toBe("Skyblock");
  });

  it("setGroup with the default name removes the entry", () => {
    let map = setGroup({}, "p1", "Skyblock");
    map = setGroup(map, "p1", DEFAULT_GROUP);
    expect(map).toEqual({});
    expect(getGroup(map, "p1")).toBe(DEFAULT_GROUP);
  });

  it("whitespace-only names fall back to the default group", () => {
    const map = setGroup({}, "p1", "   ");
    expect(getGroup(map, "p1")).toBe(DEFAULT_GROUP);
  });
});

describe("listGroupNames", () => {
  it("always includes the default group first, then alphabetical", () => {
    const map: GroupMap = { b: "Zeta", a: "Alpha" };
    expect(listGroupNames(map, ["a", "b"])).toEqual([
      DEFAULT_GROUP,
      "Alpha",
      "Zeta",
    ]);
  });

  it("includes named groups even when they currently hold no visible project", () => {
    const map: GroupMap = { hidden: "Empty group" };
    expect(listGroupNames(map, [])).toContain("Empty group");
  });
});

describe("collapsed groups", () => {
  it("toggle adds then removes, persisting each step", () => {
    let collapsed = loadCollapsedGroups();
    collapsed = toggleCollapsed(collapsed, "G");
    expect(collapsed.has("G")).toBe(true);
    expect(loadCollapsedGroups().has("G")).toBe(true);
    collapsed = toggleCollapsed(collapsed, "G");
    expect(collapsed.has("G")).toBe(false);
  });
});

describe("suggestFolderName", () => {
  it("returns the cleaned base name when free", () => {
    expect(suggestFolderName({}, "My Folder")).toBe("My Folder");
  });

  it("appends a counter when the name is taken", () => {
    const map = setGroup({}, "p1", "Pack");
    expect(suggestFolderName(map, "Pack")).toBe("Pack 2");
  });

  it("never collides with the default group name", () => {
    expect(suggestFolderName({}, DEFAULT_GROUP)).not.toBe(DEFAULT_GROUP);
  });
});

describe("folderFromDrop", () => {
  it("returns null for the same instance", () => {
    expect(folderFromDrop({}, "p1", "p1", "Alpha")).toBeNull();
  });

  it("creates a folder named after the target holding both instances", () => {
    const result = folderFromDrop({}, "p-src", "p-tgt", "Alpha");
    expect(result).not.toBeNull();
    expect(result!.created).toBe(true);
    expect(result!.groupName).toBe("Alpha");
    expect(getGroup(result!.map, "p-src")).toBe("Alpha");
    expect(getGroup(result!.map, "p-tgt")).toBe("Alpha");
  });

  it("moves the source into the target's existing group without creating a new one", () => {
    const map = setGroup({}, "p-tgt", "Skyblock");
    const result = folderFromDrop(map, "p-src", "p-tgt", "Alpha");
    expect(result!.created).toBe(false);
    expect(result!.groupName).toBe("Skyblock");
    expect(getGroup(result!.map, "p-src")).toBe("Skyblock");
  });

  it("returns null when both instances already share a group", () => {
    let map = setGroup({}, "p-src", "Skyblock");
    map = setGroup(map, "p-tgt", "Skyblock");
    expect(folderFromDrop(map, "p-src", "p-tgt", "Alpha")).toBeNull();
  });
});
