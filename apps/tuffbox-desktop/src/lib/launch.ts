import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-shell";
import { writable, get } from "svelte/store";
import { toasts } from "./toast";
import type {
  LaunchCrashEvent,
  LaunchErrorInfo,
  LaunchLifecycleEvent,
  LaunchResult,
  ProcessExitedEvent,
  RunningInstance,
} from "./api";
import { api } from "./api";
import {
  applyLaunchLifecycle,
  beginLaunchSession,
  failLaunchSession,
  isProjectLaunching,
  isProjectRunning,
  launchSessions,
  markLaunchRunning,
  openLaunchLog,
  projectPath,
  removeRunning,
  runningInstances,
  upsertRunning,
  authState,
  loginModalOpen,
  openLauncherSettings,
  isLaunching,
  launchProgress,
} from "./store";
import { shareCrashLogWithFeedback } from "./mclogs";
import { reportSoftVerifyCrash } from "./softVerify";

export type { LaunchErrorInfo, LaunchLifecycleEvent };

/** Unified launch-phase type used across the desktop UI. */
export type LaunchPhase =
  | "idle"
  | "preparing"
  | "preflight"
  | "resolving_java"
  | "downloading"
  | "starting"
  | "running"
  | "stopping"
  | "exited"
  | "failed";

export interface LaunchParams {
  path: string;
  /** "client" (default) | "server" | a custom profile id */
  profile?: string;
  quickPlayType?: string | null;
  quickPlayValue?: string | null;
  /** One-shot memory override for smoke/low-end runs (does not mutate manifest). */
  memoryMbOverride?: number | null;
  /** Staged server instance directory (required for profile === "server"). */
  serverDir?: string | null;
  levelSeed?: string | null;
  onlineMode?: boolean | null;
}

export interface LaunchFeedbackOptions {
  onStarted?: (result: LaunchResult) => void;
  showSuccess?: boolean;
  openLog?: boolean;
  /** Manifest path for the log modal (for example, a staged server dir). */
  logPath?: string | null;
  logTitle?: string | null;
}

// ─── Shared launch state machine ─────────────────────────────────────
//
// The backend exposes an explicit lifecycle through Tauri events so every Play
// button in the app can render the same accurate state instead of each keeping
// its own local `launching` flag that is reset the instant `invoke` returns
// (BUG_REPORT Bug 2). Phases flow:
//
//   (user clicks Play)
//     → "preparing"   (launchWithFeedback starts the invoke)
//     → "resolving_java" / "downloading"  (optional, from `launch-phase` events)
//     → "starting"    (JVM spawn begins)
//     → "running"     (`process-started` / `launch-phase` running)
//     → "exited"      (`process-exited` / `launch-crashed`)
//
// A path is considered "launching" for phases preparing…starting and stays that
// way until the backend confirms `running` or the run ends — never reset by the
// return of the invoke alone.

export interface LaunchPhaseState {
  path: string;
  phase: LaunchPhase;
  message: string | null;
  /** Set on launch failure / crash (kept for deep-link UI). */
  error?: LaunchErrorInfo | null;
  /** The LaunchResult when the invoke succeeded (running/exited). */
  result?: LaunchResult | null;
}

/** Per-path launch phase map, keyed by manifest path (`running.id`). */
export const launchStates = writable<Record<string, LaunchPhaseState>>({});

/**
 * The single path whose launch is currently in a pre-run phase
 * (preparing…starting). Cleared once the game is `running` or `exited`.
 * This is the shared replacement for the per-component `launching` flags.
 */
export const launchingPath = writable<string | null>(null);

// `isLaunching` and `launchProgress` live in store.ts as the single shared
// source (components subscribe there). This module only writes progress and
// reads the derived busy flag — it must not keep private duplicates; they used
// to diverge and left the header progress bar dead.

/**
 * The last LaunchParams passed to `launchWithFeedback`. Retained so the
 * crash-retry flow can reconstruct the same launch call without asking the
 * user to pick the instance again.
 */
let lastLaunch: LaunchParams | null = null;
let lastOnStarted: ((result: LaunchResult) => void) | null = null;

const LAUNCHING_PHASES: ReadonlySet<LaunchPhase> = new Set<LaunchPhase>([
  "preparing",
  "resolving_java",
  "downloading",
  "starting",
]);

export function isLaunchPhase(phase: LaunchPhase | null | undefined): boolean {
  return phase != null && LAUNCHING_PHASES.has(phase);
}

/** True while `path` is in a pre-run phase (preparing…starting). */
export function isPathLaunching(
  path: string | null | undefined,
  states?: Record<string, LaunchPhaseState>,
): boolean {
  if (!path) return false;
  const s = (states ?? get(launchStates))[path];
  return s ? isLaunchPhase(s.phase) : false;
}

export function setLaunchPhase(
  path: string,
  phase: LaunchPhase,
  message?: string | null,
  result?: LaunchResult | null,
): void {
  launchStates.update((map) => {
    const prev = map[path];
    return {
      ...map,
      [path]: {
        path,
        phase,
        message: message ?? prev?.message ?? null,
        error: prev?.error ?? null,
        result: result ?? prev?.result ?? null,
      },
    };
  });
}

export function setLaunchError(path: string, error: LaunchErrorInfo): void {
  launchStates.update((map) => ({
    ...map,
    [path]: {
      path,
      phase: "exited",
      message: error.message,
      error,
      result: map[path]?.result ?? null,
    },
  }));
}

/** Mark a run as running (game is up) — clears the launching flag. */
function markRunning(path: string): void {
  setLaunchPhase(path, "running", "Running");
  launchingPath.update((p) => (p === path ? null : p));
}

/** Mark a run as exited — clears the launching flag for this path. */
function markExited(path: string): void {
  setLaunchPhase(path, "exited", "Exited");
  launchingPath.update((p) => (p === path ? null : p));
}

// Retryable error categories — mirrors `LaunchErrorKind::retryable` on the Rust
// side. Only these get a Retry button; fundamental config errors do not.
const RETRYABLE = new Set<string>([
  "offline",
  "host_unreachable",
  "version_resolve",
  "mod_download",
  "java_missing",
  "install",
  "launch_crash",
]);

const STARTUP_PHASES = new Set<LaunchPhase>([
  "preflight",
  "resolving_java",
  "downloading",
  "starting",
]);

/** Human-friendly text for buttons / status chips. The backend message is more
 * specific when available; this is a stable fallback for every Play surface. */
export function launchPhaseLabel(phase: LaunchPhase | null | undefined): string {
  switch (phase) {
    case "preflight": return "Checking…";
    case "resolving_java": return "Resolving Java…";
    case "downloading": return "Preparing files…";
    case "starting": return "Starting…";
    case "running": return "Running";
    case "stopping": return "Stopping…";
    case "exited": return "Exited";
    case "failed": return "Launch failed";
    default: return "Launch";
  }
}

export function isLaunchError(error: unknown): error is LaunchErrorInfo {
  return (
    typeof error === "object"
    && error !== null
    && "kind" in error
    && "message" in error
    && typeof (error as { kind?: unknown }).kind === "string"
    && typeof (error as { message?: unknown }).message === "string"
  );
}

function asLaunchError(error: unknown): LaunchErrorInfo {
  return isLaunchError(error)
    ? error
    : { kind: "unknown", message: String(error) };
}

function isRetryable(info: LaunchErrorInfo): boolean {
  return RETRYABLE.has(info.kind);
}

type LaunchProgressPayload = {
  phase?: string;
  message?: string;
  percent?: number | null;
};

async function pollDownloadOverlay() {
  try {
    const phase = get(launchProgress)?.phase ?? "";
    // Skip host work while not downloading assets/mods.
    if (phase && !/mod|asset|download|install|java|librar/i.test(phase)) return;
    const items = await api.system.getDownloadProgress();
    if (!Array.isArray(items) || items.length === 0) return;
    let downloaded = 0;
    let total = 0;
    for (const raw of items) {
      const it = raw as { downloaded?: number; total?: number; percent?: number };
      downloaded += Number(it.downloaded) || 0;
      total += Number(it.total) || 0;
    }
    const percent =
      total > 0 ? Math.min(99, Math.round((downloaded / total) * 100)) : null;
    const current = get(launchProgress);
    const baseMsg = current?.message?.replace(/\s·\s\d+%.*/, "") || "Downloading…";
    launchProgress.set({
      phase: current?.phase || "mods",
      message: percent != null ? `${baseMsg} · ${percent}%` : baseMsg,
      percent: percent ?? current?.percent ?? null,
    });
  } catch {
    /* optional overlay */
  }
}


async function doLaunch(params: LaunchParams): Promise<LaunchResult> {
  const profile = params.profile ?? "client";
  const memoryMbOverride = params.memoryMbOverride ?? null;
  if (params.quickPlayType || params.quickPlayValue) {
    return invoke<LaunchResult>("launch_with_quick_play", {
      path: params.path,
      profile,
      quickPlayType: params.quickPlayType ?? null,
      quickPlayValue: params.quickPlayValue ?? null,
      memoryMbOverride,
    });
  }
  if (profile === "server") {
    const serverDir = params.serverDir?.trim();
    if (!serverDir) {
      throw { kind: "install", message: "Pick a server folder before Run server." } satisfies LaunchErrorInfo;
    }
    return invoke<LaunchResult>("launch_server", {
      path: params.path,
      serverDir,
      levelSeed: params.levelSeed ?? null,
      onlineMode: params.onlineMode ?? null,
    });
  }
  return invoke<LaunchResult>("launch_profile", {
    path: params.path,
    profile,
    memoryMbOverride,
  });
}

type RememberedLaunch = {
  params: LaunchParams;
  options?: LaunchFeedbackOptions;
};

// A JVM can crash long after invoke returned. Keep the launch request per
// instance (not as one global "last launch") so its Retry action is always
// attached to the correct Play surface when several instances are used.
const rememberedLaunches = new Map<string, RememberedLaunch>();

function normaliseRunningResult(result: LaunchResult, fallback: LaunchParams): RunningInstance | null {
  const id = result.instanceId || fallback.path;
  const pid = Number(result.pid);
  const startedAt = Number(result.startedAt);
  if (!id || !Number.isFinite(pid) || pid <= 0 || !Number.isFinite(startedAt)) return null;
  return {
    id,
    pid,
    profile: result.profile || fallback.profile || "client",
    startedAt,
  };
}

/**
 * The one public path for starting Minecraft from the UI.
 *
 * It begins a shared lifecycle session before invoke, promotes it to Running
 * from the returned process identity / Tauri events, and leaves completion to
 * process-exited / launch-crashed. It intentionally has no `finally` that
 * clears UI state: invoke timing is not game lifecycle timing.
 */
/// Launch a profile and surface a categorized, optionally-retryable toast on
/// failure. Returns the `LaunchResult` on success, or `null` after the error
/// toast has been shown.
///
/// The `launching`/`isLaunching` state is **not** reset here — it is driven by
/// the backend lifecycle events (`process-started` / `process-exited` /
/// `launch-phase` / `launch-crashed`) so the spinner stays up until the game is
/// actually running or the run has ended/failed. Callers must not clear it in a
/// `finally` block.
export async function launchWithFeedback(
  params: LaunchParams,
  options?: LaunchFeedbackOptions,
): Promise<LaunchResult | null> {
  const profile = params.profile ?? "client";
  const sessions = get(launchSessions);
  if (isProjectLaunching(params.path, sessions)) {
    toasts.info("This instance is already launching.");
    return null;
  }
  if (isProjectRunning(params.path, get(runningInstances))) {
    toasts.info("This instance is already running.", 6000, [
      {
        label: "Stop",
        run: () => {
          void killWithFeedback(params.path);
        },
      },
    ]);
    return null;
  }

  if (profile !== "server") {
    const auth = get(authState);
    if (!auth.loggedIn || !auth.profile) {
      toasts.warning(
        "Sign in to play with your Minecraft account, or continue offline.",
        12000,
        [
          {
            label: "Sign in",
            run: () => loginModalOpen.set(true),
          },
          {
            label: "Play offline",
            // Bypass the auth guard — a plain recursive call would hit the
            // same not-signed-in branch and warn forever instead of playing.
            run: () => {
              void attemptLaunch(params, options, true);
            },
          },
        ],
      );
      return null;
    }
  }
  return attemptLaunch(params, options, false);
}

/** Guarded body of launchWithFeedback; `skipAuthCheck` implements the
 * user-confirmed offline launch. */
async function attemptLaunch(
  params: LaunchParams,
  options: LaunchFeedbackOptions | undefined,
  skipAuthCheck: boolean,
): Promise<LaunchResult | null> {
  const profile = params.profile ?? "client";
  if (!skipAuthCheck && profile !== "server") {
    const auth = get(authState);
    if (!auth.loggedIn || !auth.profile) {
      // Lost sign-in between the guard and here — surface the same warning.
      toasts.warning(
        "Sign in to play with your Minecraft account, or continue offline.",
        12000,
        [
          {
            label: "Sign in",
            run: () => loginModalOpen.set(true),
          },
          {
            label: "Play offline",
            run: () => {
              void attemptLaunch(params, options, true);
            },
          },
        ],
      );
      return null;
    }
  }

  rememberedLaunches.set(params.path, { params, options });
  beginLaunchSession(params.path, profile);
  launchProgress.set({ phase: "preparing", message: "Preparing…", percent: 0 });
  void invoke("set_last_opened_project", { path: params.path }).catch(() => {});

  lastLaunch = params;
  lastOnStarted = options?.onStarted ?? null;
  const showLog = options?.openLog !== false;
  if (showLog) openLaunchLog(options?.logPath ?? params.path, options?.logTitle ?? null);
  // Enter the pre-run launch phase. Kept until process-started / exit events.
  // (isLaunching derives from launchSessions — beginLaunchSession flipped it.)
  launchingPath.set(params.path);
  setLaunchPhase(params.path, "preparing", "Preparing…");

  let pollTimer: ReturnType<typeof setInterval> | null = null;
  try {
    void ensureLaunchProgressListener();
    pollTimer = setInterval(() => {
      void pollDownloadOverlay();
    }, 450);

    const result = await doLaunch(params);
    const running = normaliseRunningResult(result, params);
    if (running) {
      // A very short-lived JVM can emit `exited` before invoke returns. Never
      // resurrect that terminal state from the result; the process event stays
      // authoritative in that race.
      const phase = get(launchSessions)[running.id]?.phase;
      if (phase !== "exited" && phase !== "failed") {
        upsertRunning(running);
      }
      rememberedLaunches.set(running.id, { params, options });
    }
    // Successfully spawned — the game is starting. Keep `launching` true until
    // the backend confirms `running` via process-started, or the run ends.
    // Only advance a pre-run phase to "starting"; never downgrade a phase the
    // backend already advanced (the process-started event may beat the invoke).
    const cur = get(launchStates)[params.path];
    if (cur && isLaunchPhase(cur.phase)) {
      setLaunchPhase(params.path, "starting", "Starting game…", result);
    } else if (cur) {
      setLaunchPhase(params.path, cur.phase, cur.message, result);
    }
    if (options?.showSuccess) toasts.success("Launch started");
    options?.onStarted?.(result);
    // After a successful start, confirm any pending crash-fix as resolved when
    // latest.log looks healthy. On verified resolution the backend emits
    // `tuffbox:distill-resolution` for the Confirm → publish UI (no auto-upload).
    void invoke("confirm_crash_resolution_after_launch", { path: params.path }).catch(() => {
      // Optional bookkeeping must never change the launch lifecycle.
    });
    return result;
  } catch (error) {
    const info = asLaunchError(error);
    failLaunchSession(params.path, info);
    setLaunchError(params.path, info);
    launchingPath.update((p) => (p === params.path ? null : p));
    showLaunchError(info, () => void launchWithFeedback(params, options), { path: params.path });
    if (showLog) openLaunchLog(options?.logPath ?? params.path, options?.logTitle ?? null);
    return null;
  } finally {
    if (pollTimer) clearInterval(pollTimer);
    launchProgress.set(null);
  }
}

/** Stop a tracked game. The backend keeps it in `list_running_instances` until
 * Child::wait observes exit and then emits `process-exited`; do not optimistically
 * remove it here or the UI can claim the game stopped while it is still alive. */
/// Kill the Minecraft process for a project. Backend emits `process-exited`
/// (which clears `launching` via the shared state machine).
export async function killWithFeedback(path: string): Promise<boolean> {
  const running = get(runningInstances).find((instance) => instance.id === path);
  if (running) {
    applyLaunchLifecycle({
      id: path,
      profile: running.profile,
      phase: "stopping",
      message: "Stopping game…",
      pid: running.pid,
      startedAt: running.startedAt,
      stopped: true,
    });
  }
  try {
    await invoke("kill_running_instance", { instanceId: path });
    toasts.info("Stopping game…");
    removeRunning(path);
    markExited(path);
    return true;
  } catch (e) {
    const msg = String(e).toLowerCase();
    // Stale UI / race: process already gone — treat as stopped.
    if (msg.includes("no running instance") || msg.includes("not found") || msg.includes("not running")) {
      removeRunning(path);
      toasts.info("Game already stopped");
      return true;
    }
    // If delivery of the kill signal failed, restore the known running state.
    if (running) markLaunchRunning(running);
    toasts.error(`Stop failed: ${e}`);
    return false;
  }
}

/** Reconcile initial app state with the backend process registry. Events keep
 * it current afterwards; this is only the startup/reconnect safety net. */
export async function refreshRunningInstances(): Promise<void> {
  try {
    const list = await invoke<RunningInstance[]>("list_running_instances");
    const instances = Array.isArray(list) ? list : [];
    runningInstances.set(instances);
    for (const instance of instances) markLaunchRunning(instance);
    // Reconcile phase map with the source of truth: any running id should read
    // "running"; anything no longer in the list that was running → exited.
    const ids = new Set(instances.map((r) => r.id));
    const states = get(launchStates);
    for (const id of ids) {
      const s = states[id];
      if (!s || s.phase === "running") continue;
      setLaunchPhase(id, "running", "Running");
    }
    for (const id of Object.keys(states)) {
      if (ids.has(id)) continue;
      if (states[id] && isLaunchPhase(states[id].phase)) {
        setLaunchPhase(id, "exited", "Exited");
      }
    }
  } catch {
    // Backend not ready / optional in web preview.
  }
}

/** Keep running state truthful: prune dead PIDs on an adaptive interval + focus. */
export function startRunningInstancesWatch(): () => void {
  let timer: ReturnType<typeof setInterval> | null = null;

  const tick = () => {
    void refreshRunningInstances();
  };

  const schedule = () => {
    if (timer) clearInterval(timer);
    timer = null;
    if (document.visibilityState !== "visible") return; // hidden: no idle polling
    const busy = get(isLaunching) || get(runningInstances).length > 0;
    // Hot while launching / game open; cool when idle (less CPU for the launcher UI).
    timer = setInterval(tick, busy ? 2500 : 12000);
  };

  schedule();
  const unsubLaunch = isLaunching.subscribe(() => schedule());
  const unsubRun = runningInstances.subscribe(() => schedule());
  const onFocus = () => tick();
  window.addEventListener("focus", onFocus);
  document.addEventListener("visibilitychange", onVisChange);

  function onVisChange() {
    if (document.visibilityState === "visible") {
      tick();
      schedule(); // resume the watch after the hidden pause
    } else {
      schedule(); // pauses (clears) the interval while hidden
    }
  }

  return () => {
    if (timer) clearInterval(timer);
    unsubLaunch();
    unsubRun();
    window.removeEventListener("focus", onFocus);
    document.removeEventListener("visibilitychange", onVisChange);
  };
}

/// Display a launch error as a toast with Retry / View log actions when
/// appropriate.
export function showLaunchError(
  error: unknown,
  retry?: () => void,
  context?: { path?: string | null },
): void {
  const info = asLaunchError(error);
  const actions: { label: string; run: () => void }[] = [];
  if (retry && isRetryable(info)) {
    actions.push({ label: "Retry", run: retry });
  }
  if (context?.path) {
    actions.push({
      label: "Live log",
      run: () => openLaunchLog(context.path!),
    });
  }
  if (info.logPath) {
    actions.push({
      label: "Open file",
      run: () => { void open(info.logPath!).catch(() => {}); },
    });
  }
  // A JVM crash produced a fresh latest.log / crash-report — jump straight into
  // the existing Crash Assistant report so the user can investigate without
  // hunting for a hidden log. The Live log action remains available alongside it.
  if (info.kind === "launch_crash") {
    actions.push({
      label: "Fix it",
      run: () => window.dispatchEvent(new Event("tuffbox:open-diagnostics")),
    });
    actions.push({
      label: "Share log",
      run: () => {
        const path = context?.path ?? get(projectPath);
        if (!path) {
          toasts.warning("Open a project to share the crash log");
          return;
        }
        void shareCrashLogWithFeedback(path);
      },
    });
  }
  if (info.kind === "java_missing") {
    actions.push({
      label: "Java settings",
      run: () => openLauncherSettings("java"),
    });
  }
  toasts.error(info.message, 16000, actions);
}

let crashListener: Promise<UnlistenFn> | null = null;
let processListeners: Promise<UnlistenFn[]> | null = null;
let progressListener: Promise<UnlistenFn> | null = null;

function ensureLaunchProgressListener(): Promise<UnlistenFn> {
  if (!progressListener) {
    progressListener = listen<LaunchProgressPayload>("launch-progress", (ev) => {
      if (!get(isLaunching)) return;
      const p = ev.payload ?? {};
      const phase = String(p.phase || "preparing");
      const message = String(p.message || "Launching…");
      const percent =
        typeof p.percent === "number" && Number.isFinite(p.percent)
          ? Math.max(0, Math.min(100, Math.round(p.percent)))
          : null;
      launchProgress.set({ phase, message, percent });
    });
  }
  return progressListener;
}

function parseCrashPayload(payload: LaunchCrashEvent | LaunchErrorInfo): LaunchCrashEvent {
  if (typeof payload === "object" && payload !== null && "error" in payload) {
    const event = payload as LaunchCrashEvent;
    return {
      id: event.id || event.path || get(projectPath) || "",
      profile: event.profile || "client",
      error: asLaunchError(event.error),
      exitCode: event.exitCode,
    };
  }
  // Compatibility with older desktop binaries that emitted LaunchErrorInfo
  // directly. New backend events always include the instance id.
  return {
    id: get(projectPath) ?? "",
    profile: "client",
    error: asLaunchError(payload),
  };
}

/** Register the global `launch-crashed` handler exactly once. */
export function registerLaunchCrashListener(): Promise<UnlistenFn> {
  void ensureLaunchProgressListener();
  if (!crashListener) {
    crashListener = listen<LaunchCrashEvent | LaunchErrorInfo>("launch-crashed", (event) => {
      const crash = parseCrashPayload(event.payload);
      const path = crash.id || get(projectPath);
      if (path) {
        failLaunchSession(path, crash.error);
        setLaunchError(path, crash.error);
        launchingPath.update((p) => (p === path ? null : p));
        void reportSoftVerifyCrash(path);
        // Keep the live log modal open on the crashed session.
        openLaunchLog(path);
      }
      const remembered = path ? rememberedLaunches.get(path) : undefined;
      const retry = remembered
        ? () => void launchWithFeedback(remembered.params, remembered.options)
        : undefined;
      showLaunchError(crash.error, retry, { path });
    });
  }
  return crashListener;
}

/**
 * Keep running process truth and shared lifecycle sessions in sync with Tauri.
 * `process-exited` is deliberately event-driven; stats polling is optional
 * observability and never decides whether a Play button becomes Stop.
 */
/// Keep `runningInstances` + the shared launch state machine in sync with
/// backend process-started / process-exited / launch-phase events.
export function registerProcessListeners(): Promise<UnlistenFn[]> {
  if (!processListeners) {
    processListeners = Promise.all([
      listen<LaunchLifecycleEvent>("launch-phase", (event) => {
        const lifecycle = event.payload;
        if (!lifecycle?.id) return;
        applyLaunchLifecycle(lifecycle);
        if (lifecycle.phase === "running" && lifecycle.pid != null && lifecycle.startedAt != null) {
          upsertRunning({
            id: lifecycle.id,
            pid: lifecycle.pid,
            profile: lifecycle.profile || "client",
            startedAt: lifecycle.startedAt,
          });
        } else if (lifecycle.phase === "exited") {
          removeRunning(lifecycle.id, {
            profile: lifecycle.profile,
            startedAt: lifecycle.startedAt,
            code: lifecycle.exitCode,
            stopped: lifecycle.stopped,
            error: lifecycle.error,
          });
        } else if (lifecycle.phase === "failed" && lifecycle.error) {
          failLaunchSession(lifecycle.id, lifecycle.error);
        }
        // Also drive the shared launch phase state machine.
        const known: Record<string, LaunchPhase> = {
          preparing: "preparing",
          resolving_java: "resolving_java",
          downloading: "downloading",
          starting: "starting",
          running: "running",
          exited: "exited",
        };
        const mapped = known[(lifecycle.phase ?? "").toLowerCase()];
        if (mapped) {
          if (mapped === "running") markRunning(lifecycle.id);
          else if (mapped === "exited") markExited(lifecycle.id);
          else {
            setLaunchPhase(lifecycle.id, mapped, lifecycle.message ?? null);
            launchingPath.set(lifecycle.id);
          }
        }
      }),
      listen<RunningInstance>("process-started", (event) => {
        if (event.payload?.id) upsertRunning(event.payload);
      }),
      listen<ProcessExitedEvent>("process-exited", (event) => {
        const exited = event.payload;
        if (exited?.id) {
          removeRunning(exited.id, {
            profile: exited.profile,
            startedAt: exited.startedAt,
            code: exited.code,
            stopped: exited.stopped,
          });
        }
      }),
    ]);
  }
  return processListeners;
}

/** Used by controls that need a short phase label without maintaining local
 * booleans. Kept exported to make the shared pattern easy to reuse. */
export function isStartupPhase(phase: LaunchPhase | null | undefined): boolean {
  return phase != null && STARTUP_PHASES.has(phase);
}
