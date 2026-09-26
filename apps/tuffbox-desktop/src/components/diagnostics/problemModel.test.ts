import { describe, expect, it } from "vitest";
import { mergeProblems, type Problem } from "./problemModel";

function row(over: Partial<Problem>): Problem {
  return {
    id: "graph:missing:a",
    severity: "error",
    category: "dependency",
    title: "Missing dependency",
    summary: "",
    source: "graph",
    code: "MISSING_DEPENDENCY",
    modIds: [],
    actions: [],
    risk: "safe",
    layer: "pack",
    ...over,
  } as Problem;
}

describe("mergeProblems", () => {
  it("dedupes modIds within a single row (keyed #each would throw)", () => {
    const [p] = mergeProblems([row({ modIds: ["create", "flywheel", "create"] })]);
    expect(p.modIds).toEqual(["create", "flywheel"]);
  });
});
