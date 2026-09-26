/**
 * Soft-verify bridge: after a crash-fix apply, the Rust watcher emits
 * `tuffbox:soft-verify-outcome`. When the user is signed in we cast a passive
 * Keep/Discard vote; optionally show a cooldown toast for explicit thumbs.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { get } from "svelte/store";
import { getAuthSnapshot } from "./supabaseAuth";
import { projectPath } from "./store";
import { toasts } from "./toast";

export type SoftVerifyOutcome = {
  path?: string;
  outcome?: "confirm" | "reject" | string;
  reason?: string;
  snapshotId?: string;
  fingerprintKey?: string;
  matchedCaseIds?: string[];
  planSource?: string;
  humanExplanation?: string;
};

const TOAST_COOLDOWN_MS = 24 * 60 * 60 * 1000;
const TOAST_KEY = "tuffbox.softVerify.toastAt";

let registered = false;
let unlistenOutcome: UnlistenFn | null = null;
let unlistenCrash: UnlistenFn | null = null;

function toastAllowed(): boolean {
  try {
    const raw = localStorage.getItem(TOAST_KEY);
    const last = raw ? Number(raw) : 0;
    if (!Number.isFinite(last) || last <= 0) return true;
    return Date.now() - last >= TOAST_COOLDOWN_MS;
  } catch {
    return true;
  }
}

function markToastShown() {
  try {
    localStorage.setItem(TOAST_KEY, String(Date.now()));
  } catch {
    // ignore
  }
}

function capsuleIds(payload: SoftVerifyOutcome): string[] {
  const ids = (payload.matchedCaseIds ?? []).map((s) => String(s).trim()).filter(Boolean);
  // Only vote on capsule content hashes (64 hex) or cap-… ids — skip local KB case ids.
  return [
    ...new Set(
      ids.filter(
        (id) => /^[0-9a-f]{64}$/i.test(id) || /^cap-/i.test(id) || id.startsWith("capsule-"),
      ),
    ),
  ];
}

async function castVotes(ids: string[], vote: "confirm" | "reject") {
  if (!ids.length) return;
  let accessToken = "";
  try {
    const snap = await getAuthSnapshot();
    accessToken = snap.session?.access_token ?? "";
  } catch {
    return;
  }
  if (!accessToken) return;

  for (const contentHash of ids) {
    try {
      await invoke("vote_community_crash_capsule", {
        contentHash,
        vote,
        accessToken,
      });
    } catch {
      // already voted / not signed in / capsule missing — non-fatal
    }
  }
}

async function onOutcome(payload: SoftVerifyOutcome) {
  const outcome = (payload.outcome ?? "").toLowerCase();
  const vote = outcome === "confirm" ? "confirm" : outcome === "reject" ? "reject" : null;
  if (!vote) return;

  const ids = capsuleIds(payload);
  const planSource = (payload.planSource ?? "").toLowerCase();
  const isNetwork = planSource.includes("swarm") || ids.length > 0;

  if (isNetwork && ids.length) {
    await castVotes(ids, vote);
  }

  if (vote === "confirm" && toastAllowed()) {
    markToastShown();
    toasts.success(
      "Soft-verify confirmed this fix (passive Keep if signed in).",
      10000,
      [
        {
          label: "Open Crash Votes",
          run: () => {
            window.dispatchEvent(new CustomEvent("tuffbox:open-crash-votes"));
          },
        },
      ],
    );
  } else if (vote === "reject" && payload.reason === "rollback") {
    toasts.warning("Crash fix rolled back — recorded as Discard for the network.");
  } else if (vote === "reject" && payload.reason === "post_fix_crash") {
    toasts.warning("Game crashed after the fix — soft-verify marked it as Discard.");
  }
}

async function onLaunchCrash(payload?: { path?: string; id?: string } | null) {
  // New lifecycle events carry the manifest id. Keep `path` for compatibility
  // with older emitters and only fall back to the active project as a last resort.
  const path =
    (payload?.path && String(payload.path).trim()) ||
    (payload?.id && String(payload.id).trim()) ||
    get(projectPath)?.trim() ||
    "";
  if (!path) return;
  await reportSoftVerifyCrash(path);
}

/** Register global soft-verify listeners once (call from App onMount). */
export function registerSoftVerifyListeners(): () => void {
  if (registered) {
    return () => {};
  }
  registered = true;
  void listen<SoftVerifyOutcome>("tuffbox:soft-verify-outcome", (event) => {
    void onOutcome(event.payload ?? {});
  }).then((u) => {
    unlistenOutcome = u;
  });
  void listen("launch-crashed", (event) => {
    const payload = (event.payload ?? {}) as { path?: string; id?: string };
    void onLaunchCrash(payload);
  }).then((u) => {
    unlistenCrash = u;
  });
  return () => {
    unlistenOutcome?.();
    unlistenCrash?.();
    unlistenOutcome = null;
    unlistenCrash = null;
    registered = false;
  };
}

/** Report launch crash against the active crash-fix soft-verify for a project. */
export async function reportSoftVerifyCrash(path: string): Promise<void> {
  if (!path) return;
  try {
    await invoke("report_soft_verify_failure", {
      path,
      reason: "launch_crash",
    });
  } catch {
    // no pending marker
  }
}

export async function fetchCrashFixBanner(path: string) {
  if (!path) return null;
  try {
    return await invoke<{
      snapshotId: string;
      fingerprintKey: string;
      planSource?: string | null;
      humanExplanation: string;
      matchedCaseIds: string[];
      actionsSummary: string[];
      createdAt: string;
      resolved: boolean;
      rolledBack: boolean;
      softVerifyStartedUnix?: number | null;
      minPlaytimeSecs: number;
    } | null>("get_crash_fix_banner", { path });
  } catch {
    return null;
  }
}

export async function rollbackLastCrashFix(path: string): Promise<boolean> {
  try {
    await invoke("rollback_last_crash_fix", { path });
    toasts.success("Restored snapshot from before the crash fix");
    return true;
  } catch (e) {
    toasts.error(String(e));
    return false;
  }
}
