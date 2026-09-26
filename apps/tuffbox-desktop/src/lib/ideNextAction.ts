/**
 * The launcher's "what do I do next" guidance — copy included.
 *
 * This bar is the FIRST thing a new user reads after opening a pack, so the
 * wording follows three rules:
 *  1. Plain language, zero launcher jargon — not "Health", "pack graph",
 *     "Brief" or "Tune", but what happened and what the button will do.
 *  2. Status → explanation → action: the user always learns WHAT state the
 *     pack is in, WHY it matters, and gets one verb-first button.
 *  3. Every branch carries both a label (button) and a detail (sentence),
 *     so the bar teaches instead of hinting.
 */

export interface IdeNextAction {
  /** Verb-first button label, e.g. "Find out why it crashed". */
  label: string;
  /** IDE stage to navigate to, or null when there is nothing to do. */
  stage: string | null;
  kind: "stage" | "none";
  /** One plain sentence explaining the situation. */
  detail?: string;
}

export function computeIdeNextAction(opts: {
  issueCount: number;
  needsHealth: boolean;
  briefDirty: boolean;
  tuneDirty: boolean;
  questDirty: boolean;
}): IdeNextAction {
  if (opts.issueCount > 0) {
    return {
      label: `Fix ${opts.issueCount} mod problem${opts.issueCount === 1 ? "" : "s"}`,
      stage: "resolve",
      kind: "stage",
      detail: `${opts.issueCount === 1 ? "A mod is" : "Some mods are"} missing or conflicting — the pack may not start until it's fixed.`,
    };
  }
  if (opts.needsHealth) {
    return {
      label: "Find out why it crashed",
      stage: "diagnose",
      kind: "stage",
      detail: "A crash report was found. The Health check reads it and suggests what to fix.",
    };
  }
  if (opts.briefDirty) {
    return {
      label: "Save your pack listing",
      stage: "brief",
      kind: "stage",
      detail: "You have unsaved changes to the pack's store page.",
    };
  }
  if (opts.tuneDirty) {
    return {
      label: "Save your config changes",
      stage: "configs",
      kind: "stage",
      detail: "You have unsaved edits in the config editor.",
    };
  }
  if (opts.questDirty) {
    return {
      label: "Save your quests",
      stage: "quests",
      kind: "stage",
      detail: "You have unsaved edits in the quest editor.",
    };
  }
  return {
    label: "Test the pack",
    stage: "test",
    kind: "stage",
    detail: "Launch the game once to confirm the pack starts and plays.",
  };
}

/** Plain-language status for the pack state pill (left side of the bar). */
export function packStatusLabel(issueCount: number, needsHealth: boolean): string {
  if (issueCount > 0) {
    return `${issueCount} mod problem${issueCount === 1 ? "" : "s"}`;
  }
  if (needsHealth) return "Crash report found";
  return "Pack checks out";
}
