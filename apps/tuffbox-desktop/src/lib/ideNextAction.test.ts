import { describe, expect, it } from "vitest";
import { computeIdeNextAction, packStatusLabel } from "./ideNextAction";

/** Launcher jargon must not reach a first-time user through this bar. */
const JARGON = ["health check", "open health", "pack graph", "brief", "tune", "verify the pack"];

describe("computeIdeNextAction — first-run guidance copy", () => {
  it("every branch has a verb-first label and an explanatory sentence", () => {
    const branches = [
      computeIdeNextAction({ issueCount: 3, needsHealth: false, briefDirty: false, tuneDirty: false, questDirty: false }),
      computeIdeNextAction({ issueCount: 1, needsHealth: true, briefDirty: true, tuneDirty: true, questDirty: true }),
      computeIdeNextAction({ issueCount: 0, needsHealth: true, briefDirty: false, tuneDirty: false, questDirty: false }),
      computeIdeNextAction({ issueCount: 0, needsHealth: false, briefDirty: true, tuneDirty: false, questDirty: false }),
      computeIdeNextAction({ issueCount: 0, needsHealth: false, briefDirty: false, tuneDirty: true, questDirty: false }),
      computeIdeNextAction({ issueCount: 0, needsHealth: false, briefDirty: false, tuneDirty: false, questDirty: true }),
      computeIdeNextAction({ issueCount: 0, needsHealth: false, briefDirty: false, tuneDirty: false, questDirty: false }),
    ];
    for (const b of branches) {
      expect(b.label.length).toBeGreaterThan(3);
      expect(b.label[0]).toMatch(/[A-Z]/);
      expect(b.detail).toBeTruthy();
      expect(b.detail!.endsWith(".")).toBe(true);
      expect(b.detail!.length).toBeGreaterThan(15);
    }
  });

  it("REGRESSION: no launcher jargon in the user-facing copy", () => {
    const all = [
      ...Object.values(computeIdeNextAction({ issueCount: 1, needsHealth: true, briefDirty: true, tuneDirty: true, questDirty: true })),
      ...Object.values(computeIdeNextAction({ issueCount: 0, needsHealth: false, briefDirty: false, tuneDirty: false, questDirty: false })),
    ].filter((v): v is string => typeof v === "string");
    for (const text of all) {
      const lower = text.toLowerCase();
      for (const word of JARGON) {
        expect(lower, `"${text}" must not contain jargon "${word}"`).not.toContain(word);
      }
    }
  });

  it("prioritizes mod problems over everything else", () => {
    const next = computeIdeNextAction({ issueCount: 5, needsHealth: true, briefDirty: true, tuneDirty: true, questDirty: true });
    expect(next.stage).toBe("resolve");
    expect(next.label).toContain("5 mod problems");
  });

  it("pluralizes the mod problem count", () => {
    expect(computeIdeNextAction({ issueCount: 1, needsHealth: false, briefDirty: false, tuneDirty: false, questDirty: false }).label).toBe(
      "Fix 1 mod problem",
    );
  });

  it("crash guidance explains what the Health check does", () => {
    const next = computeIdeNextAction({ issueCount: 0, needsHealth: true, briefDirty: false, tuneDirty: false, questDirty: false });
    expect(next.label).toBe("Find out why it crashed");
    expect(next.detail).toContain("crash report");
    expect(next.stage).toBe("diagnose");
  });
});

describe("packStatusLabel — plain-language status pill", () => {
  it("describes each state without jargon", () => {
    expect(packStatusLabel(0, false)).toBe("Pack checks out");
    expect(packStatusLabel(0, true)).toBe("Crash report found");
    expect(packStatusLabel(1, false)).toBe("1 mod problem");
    expect(packStatusLabel(4, true)).toBe("4 mod problems");
  });
});
