import { describe, expect, it } from "vitest";
import {
  bisectProgress,
  estimateLaunches,
  MAX_AUTO_BISECT_LAUNCHES,
  outcomeFromVerdict,
  phaseKey,
  sessionActive,
  shouldAutoContinue,
  statusLine,
  type GroupTestSession,
} from "./modBisect";

function session(partial: Partial<GroupTestSession>): GroupTestSession {
  return {
    pool: ["a", "b", "c", "d"],
    covering: ["a", "b", "c", "d"],
    knownClean: [],
    defectives: [],
    testGroup: [],
    phase: "needCovering",
    verified: false,
    step: 0,
    ...partial,
  };
}

describe("outcomeFromVerdict", () => {
  it("maps conclusive verdicts", () => {
    expect(outcomeFromVerdict("pass")).toBe("healthy");
    expect(outcomeFromVerdict("crashed")).toBe("crash");
    // A launch that never reached a healthy state breaks the pack.
    expect(outcomeFromVerdict("fail")).toBe("crash");
  });

  it("REGRESSION: a timeout is inconclusive — reporting it would corrupt the bisection", () => {
    expect(outcomeFromVerdict("timedOut")).toBeNull();
  });
});

describe("phase parsing", () => {
  it("reads string, object and failed phases", () => {
    expect(phaseKey(session({ phase: "testing" }))).toBe("testing");
    expect(phaseKey(session({ phase: { verifyOne: { index: 2 } } }))).toBe("verifyOne");
    expect(phaseKey(session({ phase: { failed: { reason: "boom" } } }))).toBe("failed");
    expect(phaseKey(null)).toBe("");
  });

  it("session is active unless done or failed", () => {
    expect(sessionActive(session({ phase: "testing" }))).toBe(true);
    expect(sessionActive(session({ phase: "done" }))).toBe(false);
    expect(sessionActive(session({ phase: { failed: { reason: "x" } } }))).toBe(false);
    expect(sessionActive(null)).toBe(false);
  });
});

describe("statusLine", () => {
  it("describes each phase", () => {
    expect(statusLine(session({ phase: "needCovering" }))).toContain("4 suspects disabled");
    expect(
      statusLine(session({ phase: "testing", testGroup: ["a", "b"], covering: ["a", "b", "c", "d"] })),
    ).toContain("Testing [a, b]");
    expect(statusLine(session({ phase: "done", defectives: ["c"], verified: true }))).toContain("Isolated: c");
    expect(statusLine(session({ phase: { failed: { reason: "not in the pool" } } }))).toContain("not in the pool");
  });
});

describe("launch estimation and the auto-loop guard", () => {
  it("estimates ~log2 launches for a pool", () => {
    // 16 mods → 1 covering + 4 bisection + 1 verify-all + 1 verify = 7
    expect(estimateLaunches(16)).toBe(7);
    // 2 mods → covering + 1 test + verify-all + verify-one = 4
    expect(estimateLaunches(2)).toBe(4);
    expect(estimateLaunches(1)).toBe(0);
  });

  it("continues while active and under the cap", () => {
    const s = session({ phase: "testing" });
    expect(shouldAutoContinue(s, 0)).toBe(true);
    expect(shouldAutoContinue(s, MAX_AUTO_BISECT_LAUNCHES)).toBe(false);
    expect(shouldAutoContinue(session({ phase: "done" }), 0)).toBe(false);
  });

  it("progress grows with steps and saturates below done", () => {
    const s = session({ phase: "testing", step: 3, pool: Array.from({ length: 16 }, (_, i) => `m${i}`) });
    expect(bisectProgress(s)).toBeGreaterThan(0);
    expect(bisectProgress(s)).toBeLessThan(1);
    expect(bisectProgress(session({ phase: "done", defectives: ["x"] }))).toBe(1);
  });
});
