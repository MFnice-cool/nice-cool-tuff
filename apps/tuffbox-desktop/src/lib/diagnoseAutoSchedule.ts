/**
 * Diagnose auto-analysis gating — extracted from Diagnostics.svelte so the
 * decision table can be regression-tested.
 *
 * Context (2026-09 "infinite Analyzing…" bug): the AI tab used to re-schedule
 * a full unified analysis whenever it saw `no aiAnalysis && no aiSoftError &&
 * !analysisBusy`. The busy watchdog (which force-clears the busy flags after
 * DIAGNOSE_BUSY_CAP_S) therefore created an endless loop:
 *
 *   IPC settles late / hangs → watchdog clears flags + nulls the source
 *   stamps → AI-tab effect schedules a new run → new run joins/starts another
 *   backend cascade → watchdog again → …
 *
 * The user saw "Analyzing…" / "AI is reading this crash…" forever. The fix:
 * once the watchdog TRIPS, automatic scheduling is blocked until a *manual*
 * action (Re-analyze, Retry AI, Refresh, source switch) resets the trip.
 */

/** Default hard cap after which the Diagnose watchdog stops the spinners. */
export const DIAGNOSE_BUSY_CAP_MS = 180_000;

export interface AutoScheduleInput {
  /** AI tab is the active Health canvas tab. */
  mainTabAi: boolean;
  /** A project is open. */
  hasProject: boolean;
  /** Base diagnosis (get_crash_diagnosis) has loaded. */
  hasDiagnosis: boolean;
  /** Last session healthy on latest.log — nothing to analyze. */
  sessionOk: boolean;
  /** A previous AI result exists (any source). */
  hasAiAnalysis: boolean;
  /** A previous AI soft error exists (AI ran and failed with a message). */
  hasAiSoftError: boolean;
  /** A unified analysis run is currently in flight. */
  analysisBusy: boolean;
  /** The busy watchdog tripped — manual retry required before auto-scheduling again. */
  watchdogTripped: boolean;
}

/**
 * Whether the AI tab may *automatically* kick off a unified analysis run.
 * Every condition doubles as a loop breaker; see the test suite for the
 * exact regression cases (notably: watchdogTripped must block rescheduling).
 */
export function shouldAutoScheduleAi(input: AutoScheduleInput): boolean {
  return (
    input.mainTabAi &&
    input.hasProject &&
    input.hasDiagnosis &&
    !input.sessionOk &&
    !input.hasAiAnalysis &&
    !input.hasAiSoftError &&
    !input.analysisBusy &&
    // Loop breaker: after a watchdog stop, only an explicit user action
    // (Re-analyze / Retry AI / Refresh / source switch) starts a new run.
    !input.watchdogTripped
  );
}

export interface WatchdogTickInput {
  /** Epoch ms when the current busy stretch started (0 = idle). */
  busySinceMs: number;
  /** Current epoch ms. */
  nowMs: number;
  /** Hard cap for a continuous busy stretch. */
  capMs?: number;
}

/**
 * Whether the watchdog should trip on this tick. Pure time arithmetic so the
 * exact boundary (== cap) is testable.
 */
export function shouldTripWatchdog(input: WatchdogTickInput): boolean {
  if (!input.busySinceMs) return false;
  const cap = input.capMs ?? DIAGNOSE_BUSY_CAP_MS;
  return input.nowMs - input.busySinceMs >= cap;
}

/**
 * Ownership tracker for the Diagnose `analysisBusy` flag.
 *
 * Regression (2026-09): a manual "Retry AI" click while a unified analysis
 * run was in flight bumped `analysisGeneration`, so the in-flight run's
 * `finally { if (isCurrentAnalysis(run)) analysisBusy = false; }` no longer
 * matched — and since manual AI runs set `aiLoading` but never
 * `analysisBusy`, NOBODY cleared the flag. The UI stayed on "Analyzing…"
 * until the watchdog tripped (previously: forever).
 *
 * The busy flag is owned by the LATEST unified run, independent of the
 * AI-generation counter: `shouldSettle(token)` is true exactly when `token`
 * is the most recent run that called `begin()`, so an older run finishing
 * after a generation bump still settles the flag, while a genuinely newer
 * unified run keeps it until it settles itself.
 */
export class UnifiedBusyTracker {
  #latest = 0;

  /** Start a unified run; returns its settle token. */
  begin(): number {
    return ++this.#latest;
  }

  /** True when `token` is the latest unified run and may clear the flag. */
  shouldSettle(token: number): boolean {
    return token === this.#latest;
  }
}
