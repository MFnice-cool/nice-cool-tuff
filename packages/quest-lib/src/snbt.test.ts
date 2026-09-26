import { describe, expect, it } from "vitest";
import {
  isSnbtTypedArray,
  parseSnbt,
  serializeSnbt,
  type SnbtTypedArray,
} from "./snbt";

describe("snbt serialize booleans", () => {
  it("emits true/false not 1b/0b", () => {
    const out = serializeSnbt({ a: true, b: false });
    expect(out).toContain("true");
    expect(out).toContain("false");
    expect(out).not.toContain("1b");
    expect(out).not.toContain("0b");
  });
});

describe("snbt typed arrays", () => {
  it("parses [I; ...] into tagged AST", () => {
    const v = parseSnbt(`{ id: [I; 1 -2 3] }`) as Record<string, unknown>;
    expect(isSnbtTypedArray(v.id)).toBe(true);
    const tagged = v.id as SnbtTypedArray;
    expect(tagged.__snbtArray).toBe("I");
    expect(tagged.values).toEqual([1, -2, 3]);
  });

  it("round-trips [I;] [B;] [L;]", () => {
    for (const type of ["I", "B", "L"] as const) {
      const src = `{ x: [${type}; 1 2 3] }`;
      const parsed = parseSnbt(src);
      const out = serializeSnbt(parsed);
      expect(out).toContain(`[${type};`);
      const again = parseSnbt(out) as Record<string, unknown>;
      expect(isSnbtTypedArray(again.x)).toBe(true);
      expect((again.x as SnbtTypedArray).__snbtArray).toBe(type);
      expect((again.x as SnbtTypedArray).values).toEqual([1, 2, 3]);
    }
  });

  it("round-trips empty typed array", () => {
    const parsed = parseSnbt("{ x: [I;] }") as Record<string, unknown>;
    expect(isSnbtTypedArray(parsed.x)).toBe(true);
    expect((parsed.x as SnbtTypedArray).values).toEqual([]);
    const out = serializeSnbt(parsed);
    expect(out).toContain("[I;]");
  });
});
