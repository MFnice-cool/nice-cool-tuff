/**
 * Automatic mod isolation (binary exclusion / group testing) driver — pure
 * decision logic for the Test tab.
 *
 * The bisection ALGORITHM lives in `tuffbox-core::mod_group_test`
 * (disable the suspect pool → enable half → launch → crash keeps the half,
 * healthy discards it → verify). This module owns the FRONTEND decisions:
 * mapping a launch verdict to a group-test outcome, deciding whether the
 * auto loop may continue, and summarizing the session for the UI. Kept pure
 * so the loop contract is unit-testable without a backend.
 */

/** Verdicts produced by the Test tab's launch watcher. */
export type LaunchVerdict = "pass" | "fail" | "timedOut" | "crashed";

/** Outcome accepted by `report_mod_group_test_outcome`. */
export type GroupTestOutcome = "healthy" | "crash";

/** Wire shape of `GroupTestSession` (serde camelCase). */
export interface GroupTestSession {
  pool: string[];
  covering: string[];
  knownClean: string[];
  defectives: string[];
  testGroup: string[];
  phase: string | { verifyOne?: { index?: number } } | { failed?: { reason?: string } };
  snapshotId?: string | null;
  verified: boolean;
  step: number;
}

/** Hard cap on automatic launches per isolation run (safety valve). */
export const MAX_AUTO_BISECT_LAUNCHES = 24;

/**
 * Map a launch verdict to a group-test outcome.
 *
 * - `pass` → healthy (the game reached a playable state with this group on)
 * - `crash` → crash
 * - `fail` → crash: the pack did not reach a healthy state with this group
 *   enabled, which is exactly what "this group breaks the pack" means
 * - `timedOut` → null (INCONCLUSIVE): reporting a timeout as either side
 *   would corrupt the bisection, so the auto loop must pause and let the
 *   user decide (or re-run with a larger timeout).
 */
export function outcomeFromVerdict(verdict: LaunchVerdict): GroupTestOutcome | null {
  switch (verdict) {
    case "pass":
      return "healthy";
    case "crashed":
    case "fail":
      return "crash";
    default:
      return null;
  }
}

/** Stable key for the session phase (`"done"`, `"failed"`, `"testing"`, …). */
export function phaseKey(session: GroupTestSession | null | undefined): string {
  if (!session) return "";
  const p = session.phase;
  if (typeof p === "string") return p;
  if (p && typeof p === "object") {
    if ("failed" in p) return "failed";
    if ("verifyOne" in p) return "verifyOne";
  }
  return "";
}

/** True while the session still expects launches. */
export function sessionActive(session: GroupTestSession | null | undefined): boolean {
  const key = phaseKey(session);
  return !!session && key !== "done" && key !== "failed";
}

/** One-line human status for the current phase. */
export function statusLine(session: GroupTestSession | null | undefined): string {
  if (!session) return "";
  const key = phaseKey(session);
  switch (key) {
    case "needCovering":
      return `Launch 1: all ${session.covering.length} suspects disabled — the pack must start healthy.`;
    case "testing":
      return `Testing [${session.testGroup.join(", ")}] — ${session.covering.length - session.testGroup.length + session.defectives.length} other suspects stay disabled.`;
    case "verifyAll":
      return `Verify: only [${session.defectives.join(", ")}] disabled — everything else enabled.`;
    case "verifyOne": {
      const idx =
        typeof session.phase === "object" && session.phase && "verifyOne" in session.phase
          ? session.phase.verifyOne?.index ?? 0
          : 0;
      return `Verify: re-enable ${session.defectives[idx] ?? "?"} alone — it should crash.`;
    }
    case "done":
      return `Isolated: ${session.defectives.join(", ") || "none"}.`;
    case "failed":
      return typeof session.phase === "object" && session.phase && "failed" in session.phase
        ? (session.phase.failed?.reason ?? "Group test failed")
        : "Group test failed";
    default:
      return "";
  }
}

/**
 * Estimated number of launches for a pool: 1 covering + ⌈log2(pool)⌉
 * bisection steps + 1 verify-all + one verify per isolated defective.
 */
export function estimateLaunches(poolSize: number, defectivesEstimate = 1): number {
  if (poolSize < 2) return 0;
  const bits = Math.ceil(Math.log2(poolSize));
  return 1 + bits + 1 + Math.max(0, defectivesEstimate);
}

/** Whether the auto loop may fire another launch. */
export function shouldAutoContinue(
  session: GroupTestSession | null | undefined,
  launches: number,
  cap: number = MAX_AUTO_BISECT_LAUNCHES,
): boolean {
  return sessionActive(session) && launches < cap;
}

/** Progress fraction (0..1) of the bisection for a progress bar. */
export function bisectProgress(session: GroupTestSession | null | undefined): number {
  if (!session || session.pool.length < 2) return 0;
  const key = phaseKey(session);
  if (key === "done") return 1;
  if (key === "failed") return 1;
  const total = estimateLaunches(session.pool.length);
  if (total <= 0) return 0;
  // step counts applied outcomes; the in-flight launch is step+1 but never
  // exceeds the (rough) total.
  return Math.min(0.98, session.step / total);
}
