import { SvelteMap } from "svelte/reactivity";

// ─── Crash-fix preference memory ────────────────────────────────────
//
// Remembers which side the user chose for a crash fingerprint so the next
// Diagnose run of the *same crash* recommends (and can auto-select) the same
// resolution instead of recomputing a fresh default every time. Keyed by crash
// fingerprint; value is the stable option key (`disable:<mod>` / `update:<mod>`
// / `keep:<mod>`), mirroring ChangeOption.keep/preferred from the backend plan.
//
// This module is intentionally dependency-free (no Tauri api) so it can be
// unit-tested under vitest.

const FIX_PREFS_KEY = "tuffbox.fix-prefs";

function readFixPreferences(): Map<string, string> {
  const m = new Map<string, string>();
  if (typeof localStorage === "undefined") return m;
  try {
    const raw = localStorage.getItem(FIX_PREFS_KEY);
    if (!raw) return m;
    const obj = JSON.parse(raw);
    if (obj && typeof obj === "object") {
      for (const [k, v] of Object.entries(obj)) {
        if (typeof v === "string") m.set(k, v);
      }
    }
  } catch {
    /* ignore */
  }
  return m;
}

function persistFixPreferences() {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(
      FIX_PREFS_KEY,
      JSON.stringify(Object.fromEntries(fixPreferenceByFingerprint)),
    );
  } catch {
    /* ignore */
  }
}

/** crash fingerprint → chosen fix option key (persisted in localStorage). */
export const fixPreferenceByFingerprint = new SvelteMap<string, string>(readFixPreferences());

export function setFixPreference(
  fingerprint: string | null | undefined,
  optionKey: string,
) {
  if (!fingerprint || !optionKey) return;
  fixPreferenceByFingerprint.set(fingerprint, optionKey);
  persistFixPreferences();
}

export function getFixPreference(
  fingerprint: string | null | undefined,
): string | null {
  if (!fingerprint) return null;
  return fixPreferenceByFingerprint.get(fingerprint) ?? null;
}

/** Stable key for a ChangeOption (used to persist the user's choice). */
export function changeOptionKey(option: {
  label?: string;
  keepMod?: string | null;
  actions?: { action?: string; nodeId?: string; modId?: string }[] | null;
}): string {
  const a = (option.actions ?? [])[0];
  const target =
    a?.nodeId?.replace(/^mod:/, "") || a?.modId || option.keepMod || option.label || "opt";
  const verb = (a?.action ?? "fix").toLowerCase();
  return `${verb}:${target}`;
}