import { derived, get, writable } from "svelte/store";
import type { LaunchErrorInfo, LaunchLifecycleEvent, LaunchPhase, RunningInstance } from "./api";
import { api } from "./api";
import { readStoredTheme, commitTheme, type ThemeId } from "./themes";
export {
  changeOptionKey,
  fixPreferenceByFingerprint,
  getFixPreference,
  setFixPreference,
} from "./fixPreferences";
// Screen-aware YouTube modal sizing — video counterpart of suggestUiScalePercent.
export { suggestYouTubeModalWidth, YOUTUBE_MODAL_MIN_WIDTH } from "./youtubeSizing";

export interface ProjectInfo {
  id: string;
  name: string;
  version: string;
  minecraftVersion: string;
  loaderKind: string;
  loaderVersion: string;
  javaPath: string | null;
  memoryMb: number;
  jvmArgs: string[];
  playerName: string;
}

export interface RecentProject {
  path: string;
  info: ProjectInfo;
}

function persistRecent(projects: RecentProject[]) {
  try {
    localStorage.setItem("recentProjects", JSON.stringify(projects));
  } catch {
    /* ignore quota / private mode */
  }
  // Disk copy survives WebView2 profile wipes (EBWebView clears).
  void api.session.saveRecentProjects(projects).catch(() => {});
}

function createRecentProjects() {
  let initial: RecentProject[] = [];
  try {
    const stored = typeof window !== "undefined" ? localStorage.getItem("recentProjects") : null;
    if (stored) initial = JSON.parse(stored);
  } catch {
    initial = [];
  }
  const { subscribe, set, update } = writable<RecentProject[]>(initial);

  return {
    subscribe,
    add: (project: RecentProject, opts?: { reorder?: boolean; replacePath?: string }) => {
      const reorder = opts?.reorder !== false;
      const replacePath = opts?.replacePath;
      update((projects) => {
        const matchIdx = projects.findIndex(
          (p) =>
            p.path === project.path ||
            (replacePath != null && p.path === replacePath),
        );
        let next: RecentProject[];
        if (!reorder && matchIdx >= 0) {
          next = projects
            .map((p, i) => (i === matchIdx ? project : p))
            .filter((p, i) => i === matchIdx || p.path !== project.path);
        } else if (!reorder) {
          const filtered = projects.filter(
            (p) =>
              p.path !== project.path &&
              (replacePath == null || p.path !== replacePath),
          );
          next = [...filtered, project].slice(0, 20);
        } else {
          const filtered = projects.filter(
            (p) =>
              p.path !== project.path &&
              (replacePath == null || p.path !== replacePath),
          );
          next = [project, ...filtered].slice(0, 20);
        }
        persistRecent(next);
        return next;
      });
    },
    updateInfo: (path: string, info: ProjectInfo) => {
      update((projects) => {
        const next = projects.map((p) =>
          p.path === path ? { ...p, info } : p
        );
        persistRecent(next);
        return next;
      });
    },
    remove: (path: string) => {
      update((projects) => {
        const next = projects.filter((p) => p.path !== path);
        persistRecent(next);
        return next;
      });
    },
    set: (projects: RecentProject[]) => {
      persistRecent(projects);
      set(projects);
    },
    /** Merge disk-backed list after WebView localStorage was wiped. */
    async hydrateFromDisk() {
      try {
        const disk = await api.session.loadRecentProjects();
        if (!disk?.length) return;
        update((projects) => {
          const fromDisk = disk as RecentProject[];
          if (projects.length > 0) {
            // Keep local order; fill gaps from disk.
            const have = new Set(projects.map((p) => p.path));
            const merged = [...projects];
            for (const p of fromDisk) {
              if (!have.has(p.path)) merged.push(p);
            }
            const next = merged.slice(0, 20);
            persistRecent(next);
            return next;
          }
          const next = fromDisk.slice(0, 20);
          persistRecent(next);
          return next;
        });
      } catch {
        /* offline / old binary */
      }
    },
  };
}

export const recentProjects = createRecentProjects();
export const projectPath = writable<string | null>(null);
export const projectInfo = writable<ProjectInfo | null>(null);

// ─── Minecraft Auth ──────────────────────────────────────────────

export type SkinSource = "mojang" | "elyby" | "tlauncher" | "offline";
export type LoginType = "microsoft" | "offline" | "yggdrasil";
export type CapeProvider = "mojang" | "optifine" | "tlauncher" | "none";

export interface McCapeEntry {
  id: string;
  alias: string | null;
  url: string;
  state: string;
}

export interface McProfile {
  uuid: string;
  name: string;
  skinUrl: string | null;
  capeUrl: string | null;
  capes: McCapeEntry[];
}

export interface AccountEntry {
  uuid: string;
  name: string;
  loginType: LoginType;
  skinSource: SkinSource;
  addedAt: number;
  /** Yggdrasil / authlib-injector API root (Ely.by, LittleSkin, custom). */
  authority?: string | null;
  /** Microsoft OAuth backend for token refresh (`azure` | `live`). */
  msOauthBackend?: "azure" | "live" | null;
}

export interface YggdrasilPreset {
  id: string;
  label: string;
  authority: string;
}

export interface PresenceSettings {
  discordRpcEnabled: boolean;
  discordClientId: string;
}

export interface GameResolution {
  width: number;
  height: number;
}

export interface LauncherSettings {
  theme: string;
  potatoPc: boolean;
  /** Set once the one-time weak-hardware check (see detectWeakHardware) has run. */
  perfAutoDetected: boolean;
  concurrentDownloads: number;
  gameResolution: GameResolution | null;
  preLaunchHook: string | null;
  postExitHook: string | null;
  wrapperCommand: string | null;
  runtimePath: string | null;
  instancesPath: string | null;
  defaultJavaPath: string | null;
  javaCustomArgs: string | null;
  defaultMemoryMb: number;
  /** Litube-style in-app player (default on). false = thumbnail preview → system browser. */
  youtubeInlinePlayer: boolean;
  /** Show the YouTube feed on the home dashboard. Off by default; enable in Settings. */
  showYoutubeOnHome: boolean;
  /** Show Minecraft game updates (snapshots & releases) in the home news feed. */
  newsShowUpdates: boolean;
  /** Inject the in-game overlay bridge (YouTube player + friends/chat) on launch. */
  ingameOverlay: boolean;
  /** CPU affinity for the game process: off | performance | manual. */
  cpuAffinityMode: "off" | "performance" | "manual";
  /** Hex bitmask used when mode is "manual" (e.g. "0xFF0"). */
  cpuAffinityMask: string | null;
  /** GPU used for the game: auto (discrete when present) | discrete | integrated. */
  gpuPreference: "auto" | "discrete" | "integrated";
  /** Hide IDE bottom workflow rail until cursor hits the window bottom edge. */
  autoHideWorkflowRail: boolean;
  /** Hide the IDE top panel (status / next-step / health strip). */
  hideIdeNextBar: boolean;
  /** Left nav: full labels | icons (button toggle) | autoHide (left-edge hover). */
  sidebarMode: SidebarMode;
  /** Interface zoom percent (75–150). */
  uiScalePercent: number;
  /** `auto` follows screen/window size; `manual` locks uiScalePercent. */
  uiScaleMode: UiScaleMode;
  /** Round corners on panels/cards/buttons everywhere. */
  roundedCorners: boolean;
  /** Quartz backdrop panel behind the home dashboard. Home-only; toggle in Settings. */
  homeBackdrop: boolean;
}

export type SidebarMode = "full" | "icons" | "autoHide";
export type UiScaleMode = "auto" | "manual";

export const UI_SCALE_STEPS = [75, 90, 100, 110, 125, 150] as const;
export type UiScaleStep = (typeof UI_SCALE_STEPS)[number];

/** Live applied UI zoom percent — App binds `data-ui-scaled` / zoom from this. */
export const uiScalePercentLive = writable(100);

export function normalizeSidebarMode(raw: unknown): SidebarMode {
  if (raw === "icons" || raw === "autoHide" || raw === "full") return raw;
  return "full";
}

export function normalizeUiScalePercent(raw: unknown): number {
  const n = typeof raw === "number" ? raw : Number(raw);
  if (!Number.isFinite(n)) return 100;
  return Math.min(150, Math.max(75, Math.round(n)));
}

export function normalizeUiScaleMode(raw: unknown): UiScaleMode {
  return raw === "manual" ? "manual" : "auto";
}

/** Migrate unset mode: preserve non-100% as manual. */
export function resolveUiScaleMode(settings: {
  uiScaleMode?: unknown;
  uiScalePercent?: unknown;
}): UiScaleMode {
  const raw = settings.uiScaleMode;
  if (raw === "auto" || raw === "manual") return raw;
  return normalizeUiScalePercent(settings.uiScalePercent) !== 100 ? "manual" : "auto";
}

function snapUiScalePercent(raw: number): UiScaleStep {
  let best: UiScaleStep = UI_SCALE_STEPS[0];
  let bestDist = Math.abs(raw - best);
  for (const step of UI_SCALE_STEPS) {
    const d = Math.abs(raw - step);
    if (d < bestDist) {
      best = step;
      bestDist = d;
    }
  }
  return best;
}

/**
 * Suggest UI zoom from screen/window size + devicePixelRatio.
 * Snaps to the same chips used in Settings (75–150).
 *
 * Tuned for laptop panels (1366×768, 1440×900, 1600×900) that feel cramped at 100%.
 */
export function suggestUiScalePercent(): number {
  if (typeof window === "undefined") return 100;
  const screenW = window.screen?.availWidth || window.screen?.width || 1920;
  const screenH = window.screen?.availHeight || window.screen?.height || 1080;
  const innerW = window.innerWidth || screenW;
  const innerH = window.innerHeight || screenH;
  // Tighter of screen vs window — a small window on a big monitor still needs denser UI.
  const width = Math.min(screenW, innerW);
  const height = Math.min(screenH, innerH);
  const dpr = window.devicePixelRatio || 1;
  const shortSide = Math.min(width, height);

  let suggested = 100;
  // Tiny / phone-like or very short height
  if (width < 1100 || height < 720 || shortSide < 700) suggested = 110;
  // Classic laptops: 1366×768, 1400×900, 1440×900, 1600×900 — task #57: users
  // found 110 too large here; suggest a denser 90 for this band.
  else if (width <= 1440 || height <= 900) suggested = 90;
  else if (dpr >= 2 && width < 1800) suggested = 90;
  else if (width < 1600 && height < 1000) suggested = 90;
  else if (width >= 1920 && height >= 1080 && dpr <= 1) suggested = 100;
  else suggested = 100;

  return snapUiScalePercent(suggested);
}

/** Current CSS zoom factor from `--ui-scale` (1 = 100%). */
export function getUiScale(): number {
  if (typeof document === "undefined") return 1;
  const el = document.documentElement as HTMLElement & { currentCSSZoom?: number };
  if (typeof el.currentCSSZoom === "number" && el.currentCSSZoom > 0) {
    return el.currentCSSZoom;
  }
  const raw = getComputedStyle(el).getPropertyValue("--ui-scale").trim();
  const n = Number(raw);
  return Number.isFinite(n) && n > 0 ? n : 1;
}

/**
 * Map viewport (clientX/Y) → CSS `position: fixed` left/top.
 * Zoom lives on `html`, so Chromium uses one coordinate frame — return as-is.
 * Kept for callers / WebKit edge cases that report visual px vs layout px.
 */
export function viewportToShellFixed(clientX: number, clientY: number): { x: number; y: number } {
  return { x: clientX, y: clientY };
}

/** Apply Chromium zoom on <html> so fixed menus, drag, and hit-tests share one coord space. */
export function applyUiScale(percent: unknown) {
  const p = normalizeUiScalePercent(percent);
  if (typeof document === "undefined") {
    uiScalePercentLive.set(p);
    return p;
  }
  const scale = p / 100;
  const root = document.documentElement;
  root.style.setProperty("--ui-scale", String(scale));
  uiScalePercentLive.set(p);

  // Zoom on html (not .app-shell): shell-only zoom desyncs clientX from
  // getBoundingClientRect / fixed overlays and breaks Library drag + context menus.
  if (p === 100) {
    root.removeAttribute("data-ui-scaled");
    root.style.removeProperty("zoom");
  } else {
    root.setAttribute("data-ui-scaled", "1");
    root.style.zoom = String(scale);
  }

  // Clear legacy shell zoom from older sessions / HMR.
  const shell = document.querySelector(".app-shell");
  if (shell instanceof HTMLElement) {
    shell.removeAttribute("data-ui-scaled");
  }
  return p;
}

/** Resolve mode + apply (auto → suggest, manual → stored percent). */
export function applyUiScaleFromSettings(settings: {
  uiScaleMode?: unknown;
  uiScalePercent?: unknown;
}): number {
  const mode = resolveUiScaleMode(settings);
  const pct =
    mode === "auto" ? suggestUiScalePercent() : normalizeUiScalePercent(settings.uiScalePercent);
  return applyUiScale(pct);
}

/**
 * Cheap, synchronous heuristic for "this machine is probably weak" — used
 * once on first launch to decide whether to auto-enable potato-pc (reduced
 * motion) mode. Deliberately conservative (only obviously low-end hardware
 * trips it) since it can only ever be overridden by the user afterwards.
 *
 * `navigator.deviceMemory` is Chromium-only (undefined on the WebKit
 * webview Tauri uses on macOS/Linux) so it's treated as an optional signal,
 * not a requirement.
 */
export function detectWeakHardware(): boolean {
  if (typeof navigator === "undefined") return false;
  const cores = navigator.hardwareConcurrency;
  if (typeof cores === "number" && cores > 0 && cores <= 2) return true;
  const memoryGb = (navigator as Navigator & { deviceMemory?: number }).deviceMemory;
  if (typeof memoryGb === "number" && memoryGb > 0 && memoryGb <= 2) return true;
  return false;
}

/** Toggle rounded-corners appearance mode (`data-rounded-corners` on <html>). */
export function applyRoundedCorners(enabled: unknown) {
  const on = enabled !== false;
  if (typeof document === "undefined") return on;
  document.documentElement.setAttribute("data-rounded-corners", on ? "on" : "off");
  try {
    localStorage.setItem("tuffbox-rounded-corners", on ? "1" : "0");
  } catch {
    /* ignore */
  }
  return on;
}

/** Human label for account provider badges. */
export function loginTypeLabel(type: LoginType, authority?: string | null): string {
  if (type === "microsoft") return "Mojang";
  if (type === "offline") return "Offline";
  const a = (authority ?? "").toLowerCase();
  if (a.includes("ely.by")) return "Ely.by";
  if (a.includes("littleskin")) return "LittleSkin";
  return "Yggdrasil";
}

export function formatPlaytime(secs: number): string {
  const s = Math.max(0, Math.floor(secs || 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m`;
  return `${s}s`;
}

export interface CapeOffer {
  provider: CapeProvider;
  id: string;
  label: string;
  url: string;
  canActivate: boolean;
  active: boolean;
}

export interface CapeCatalog {
  selectedProvider: CapeProvider;
  displayUrl: string | null;
  offers: CapeOffer[];
}

export interface AuthState {
  loggedIn: boolean;
  profile: McProfile | null;
  expiresAt: number | null;
  loginType: LoginType;
  skinSource: SkinSource;
  capeProvider: CapeProvider;
  accounts: AccountEntry[];
  activeAccountUuid: string | null;
}

export interface DeviceCodeInfo {
  userCode: string;
  verificationUri: string;
  /** One-click browser login URL (`…?otc=CODE`). */
  loginUrl: string;
  message: string;
  expiresIn: number;
  /** Suggested poll interval in seconds (from Microsoft). */
  interval: number;
}

export const authState = writable<AuthState>({
  loggedIn: false,
  profile: null,
  expiresAt: null,
  loginType: "offline",
  skinSource: "mojang",
  capeProvider: "mojang",
  accounts: [],
  activeAccountUuid: null,
});

export const skinPath = writable<string | null>(null);

// ─── UI / navigation state ───────────────────────────────────────

// Drives the "New instance" (AddInstanceModal) from anywhere in the app,
// including the sidebar's + button which lives outside the Dashboard tree.
export const newProjectOpen = writable<boolean>(false);

/** Global Minecraft login modal (Home skin panel, Play auth-gate, etc.). */
export const loginModalOpen = writable<boolean>(false);

/** Mode for AddInstanceModal when `newProjectOpen` is set. `"catalog"` redirects to Library Discover. */
export type AddInstanceMode = "blank" | "import" | "catalog";
export const addInstanceMode = writable<AddInstanceMode>("blank");

/** Open a Library tab (`discover` / `yours` / `create`). Cleared by Library when applied. */
export const libraryTabRequest = writable<"yours" | "discover" | "create" | null>(null);

export function openAddInstance(mode: AddInstanceMode = "blank") {
  if (mode === "catalog") {
    libraryTabRequest.set("discover");
    if (typeof window !== "undefined") {
      window.dispatchEvent(new CustomEvent("tuffbox:open-library"));
    }
    return;
  }
  addInstanceMode.set(mode);
  newProjectOpen.set(true);
}

/** Deep-link into Settings (consumed once by Settings.svelte). */
export type SettingsNavRequest = {
  tab: "appearance" | "launcher" | "ai" | "integrations" | "about";
  launcherSub?: "general" | "java" | "commands" | "runtime";
};
export const settingsNavRequest = writable<SettingsNavRequest | null>(null);

/** Open Settings → Launcher (optionally a sub-tab such as Java). */
export function openLauncherSettings(
  launcherSub: NonNullable<SettingsNavRequest["launcherSub"]> = "general",
) {
  settingsNavRequest.set({ tab: "launcher", launcherSub });
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent("tuffbox:open-settings"));
  }
}

// Single source of truth for the app theme. Initialized from localStorage
// (legacy ids migrated by readStoredTheme); every change re-applies the
// data-theme attribute and persists, keeping all subscribers in sync.
export const theme = writable<ThemeId>(readStoredTheme());
theme.subscribe((id) => commitTheme(id));

// ─── Shared launch lifecycle ─────────────────────────────────────
//
// `launch_profile` is deliberately fire-and-forget: a successful invoke only
// says the backend has spawned (or is about to spawn) Java. These per-instance
// sessions are driven by the backend `launch-phase` / process events, so every
// Play surface renders the same truth rather than resetting a local `finally`
// flag as soon as invoke resolves.
export interface LaunchSession {
  id: string;
  profile: string;
  phase: LaunchPhase;
  message: string;
  logPath?: string;
  pid?: number;
  startedAt?: number;
  exitCode?: number | null;
  stopped?: boolean;
  error?: LaunchErrorInfo;
  updatedAt: number;
}

export const launchSessions = writable<Record<string, LaunchSession>>({});

const ACTIVE_LAUNCH_PHASES = new Set<LaunchPhase>([
  "preflight",
  "resolving_java",
  "downloading",
  "starting",
  "stopping",
]);

/** A Play request is still being prepared (not yet a running process). */
export function isProjectLaunching(
  path: string | null | undefined,
  sessions: Record<string, LaunchSession>,
): boolean {
  if (!path) return false;
  const phase = sessions[path]?.phase;
  return phase != null && ACTIVE_LAUNCH_PHASES.has(phase);
}

export function isProjectStopping(
  path: string | null | undefined,
  sessions: Record<string, LaunchSession>,
): boolean {
  return !!path && sessions[path]?.phase === "stopping";
}

export function getLaunchSession(
  path: string | null | undefined,
  sessions: Record<string, LaunchSession>,
): LaunchSession | null {
  return path ? sessions[path] ?? null : null;
}

function patchLaunchSession(
  id: string,
  patch: Omit<Partial<LaunchSession>, "id" | "updatedAt"> & { phase: LaunchPhase },
) {
  if (!id) return;
  launchSessions.update((sessions) => {
    const current = sessions[id];
    const resetsAttempt = patch.phase === "preflight";
    const next: LaunchSession = {
      id,
      profile: patch.profile ?? current?.profile ?? "client",
      phase: patch.phase,
      message: patch.message ?? current?.message ?? "",
      logPath: patch.logPath ?? current?.logPath,
      pid: patch.pid ?? (resetsAttempt ? undefined : current?.pid),
      startedAt: patch.startedAt ?? (resetsAttempt ? undefined : current?.startedAt),
      exitCode: patch.exitCode ?? (resetsAttempt ? undefined : current?.exitCode),
      stopped: patch.stopped ?? (resetsAttempt ? false : current?.stopped),
      error: patch.error ?? (resetsAttempt ? undefined : current?.error),
      updatedAt: Date.now(),
    };
    return { ...sessions, [id]: next };
  });
}

/** Start an optimistic session before invoke so a fast click is visible even
 * before the first Tauri event arrives. */
export function beginLaunchSession(id: string, profile: string, message = "Preparing launch…") {
  patchLaunchSession(id, { profile, phase: "preflight", message, stopped: false });
}

function isTerminalSessionFor(
  instance: Pick<RunningInstance, "id" | "startedAt">,
  session = get(launchSessions)[instance.id],
): boolean {
  return !!session
    && (session.phase === "exited" || session.phase === "failed")
    && session.startedAt != null
    && session.startedAt === instance.startedAt;
}

export function applyLaunchLifecycle(event: LaunchLifecycleEvent) {
  if (!event?.id || !event.phase) return;
  // A JVM can exit so quickly that its wait thread emits Exited before the
  // caller finishes emitting Running. Do not resurrect that terminal session
  // when both events carry the same process start timestamp.
  if (
    event.phase === "running"
    && event.startedAt != null
    && isTerminalSessionFor({ id: event.id, startedAt: event.startedAt })
  ) {
    return;
  }
  patchLaunchSession(event.id, {
    profile: event.profile,
    phase: event.phase,
    message: event.message,
    logPath: event.logPath,
    pid: event.pid,
    startedAt: event.startedAt,
    exitCode: event.exitCode,
    stopped: event.stopped,
    error: event.error,
  });
}

export function failLaunchSession(id: string, error: LaunchErrorInfo) {
  patchLaunchSession(id, {
    phase: "failed",
    message: error.message,
    logPath: error.logPath,
    error,
    stopped: false,
  });
}

export function markLaunchRunning(instance: RunningInstance, message = "Game is running.") {
  if (isTerminalSessionFor(instance)) return;
  patchLaunchSession(instance.id, {
    profile: instance.profile,
    phase: "running",
    message,
    pid: instance.pid,
    startedAt: instance.startedAt,
    stopped: false,
  });
}

export function markLaunchExited(
  id: string,
  opts?: {
    profile?: string;
    startedAt?: number;
    code?: number | null;
    stopped?: boolean;
    error?: LaunchErrorInfo;
  },
) {
  patchLaunchSession(id, {
    profile: opts?.profile,
    phase: "exited",
    startedAt: opts?.startedAt,
    message: opts?.stopped ? "Game stopped." : opts?.error ? "Game exited unexpectedly." : "Game exited.",
    exitCode: opts?.code,
    stopped: !!opts?.stopped,
    error: opts?.error,
  });
}

// Global busy indicator remains useful for the header, but it is now derived
// from the same lifecycle source rather than being manually flipped around an
// invoke call.
export const isLaunching = derived(launchSessions, (sessions) =>
  Object.values(sessions).some((session) => ACTIVE_LAUNCH_PHASES.has(session.phase)),
);

/** Structured launch phase while `isLaunching` (Java → mods → install → starting). */
export type LaunchProgressState = {
  phase: string;
  message: string;
  percent: number | null;
};

export const launchProgress = writable<LaunchProgressState | null>(null);

/** Currently running Minecraft processes, keyed by project manifest path (`id`). */
export const runningInstances = writable<RunningInstance[]>([]);

export function normalizeInstancePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

export function isProjectRunning(path: string | null | undefined, list: RunningInstance[]): boolean {
  if (!path) return false;
  const key = normalizeInstancePath(path);
  return list.some((r) => normalizeInstancePath(r.id) === key);
}

export function upsertRunning(inst: RunningInstance) {
  if (isTerminalSessionFor(inst)) return;
  runningInstances.update((list) => {
    const key = normalizeInstancePath(inst.id);
    const without = list.filter((r) => normalizeInstancePath(r.id) !== key);
    return [...without, inst];
  });
  markLaunchRunning(inst);
}

export function removeRunning(
  id: string,
  opts?: {
    profile?: string;
    startedAt?: number;
    code?: number | null;
    stopped?: boolean;
    error?: LaunchErrorInfo;
  },
) {
  const key = normalizeInstancePath(id);
  runningInstances.update((list) => list.filter((r) => normalizeInstancePath(r.id) !== key));
  markLaunchExited(id, opts);
}

/** Opens the live launch-log modal for the given project manifest path. */
export const launchLogPath = writable<string | null>(null);
export const launchLogTitle = writable<string | null>(null);

export function openLaunchLog(path: string, title?: string | null) {
  launchLogTitle.set(title ?? null);
  launchLogPath.set(path);
}

export function closeLaunchLog() {
  launchLogPath.set(null);
  launchLogTitle.set(null);
}

/** One-shot: open IDE on this stage id (e.g. "content" = Mods). Cleared by IdeWorkspace. */
export const ideStageRequest = writable<string | null>(null);

/**
 * One-shot: after creating a fresh instance, open the Optimize pack wizard
 * for this project path. Set by AddInstanceModal ("Install performance mods"),
 * consumed by Mods (opens the wizard once Content mounts) — Dashboard only
 * navigates to IDE → Content. Cleared by Mods after applying.
 */
export const optimizeAfterCreate = writable<string | null>(null);

/** Live IDE stage while IdeWorkspace is mounted (for left-nav active highlight). */
export const ideActiveStage = writable<string | null>(null);

/** Suggested IDE stage when opening from Home (updated by IdeNextBar / diagnostics refresh). */
export const ideSuggestedStage = writable<string>("content");

/** Blocking pack issues count (missing deps + conflicts) for Home badge / Next bar. */
export const ideIssueCount = writable(0);

/** Optional crash/needs-fix hint for Next Action priority. */
export const ideNeedsHealth = writable(false);

export type WorkTrailAction = {
  id: string;
  label: string;
  /** Navigate to IDE stage, launch test, or dismiss. */
  kind: "stage" | "play" | "dismiss";
  stage?: string;
};

export type WorkTrail = {
  message: string;
  actions: WorkTrailAction[];
  createdAt: number;
  /** True after ignore-timeout escalation to pack problems / verify nudge. */
  escalated?: boolean;
};

/** How long a contextual work trail may sit ignored before escalating. */
export const WORK_TRAIL_ESCALATE_MS = 5 * 60 * 1000;

/** Contextual continue strip after Content / Resolve / Diagnose mutations. */
export const workTrail = writable<WorkTrail | null>(null);

export function pushWorkTrail(
  message: string,
  actions: WorkTrailAction[],
  opts?: { escalated?: boolean },
) {
  workTrail.set({
    message,
    actions,
    createdAt: Date.now(),
    escalated: opts?.escalated ?? false,
  });
}

export function clearWorkTrail() {
  workTrail.set(null);
}

/**
 * If a work trail was ignored long enough, replace it with the current
 * main pack problems (or a soft Test-launch nudge when the graph is clean).
 * Returns true when the trail was replaced.
 */
export function escalateIgnoredWorkTrail(opts: {
  issueCount: number;
  needsHealth: boolean;
}): boolean {
  const current = get(workTrail);
  if (!current || current.escalated) return false;
  if (Date.now() - current.createdAt < WORK_TRAIL_ESCALATE_MS) return false;

  if (opts.issueCount > 0) {
    pushWorkTrail(
      `${opts.issueCount} pack issue${opts.issueCount === 1 ? "" : "s"} still need attention`,
      [
        { id: "resolve", label: "Fix in Resolve", kind: "stage", stage: "resolve" },
        { id: "test", label: "Test launch", kind: "play" },
        { id: "dismiss", label: "Dismiss", kind: "dismiss" },
      ],
      { escalated: true },
    );
    return true;
  }

  if (opts.needsHealth) {
    pushWorkTrail(
      "Crash still needs a Health check",
      [
        { id: "diagnose", label: "Open Health", kind: "stage", stage: "diagnose" },
        { id: "test", label: "Test launch", kind: "play" },
        { id: "dismiss", label: "Dismiss", kind: "dismiss" },
      ],
      { escalated: true },
    );
    return true;
  }

  pushWorkTrail(
    "Still waiting — verify with a Test launch?",
    [
      { id: "test", label: "Test launch", kind: "play" },
      { id: "dismiss", label: "Dismiss", kind: "dismiss" },
    ],
    { escalated: true },
  );
  return true;
}

/** Bump to ask IdeNextBar to run its Next action (Ctrl+Enter). */
export const ideNextTrigger = writable(0);

export function requestIdeNextAction() {
  ideNextTrigger.update((n) => n + 1);
}

/** Bump to refresh issue counts on IdeNextBar. */
export const ideIssuesRefresh = writable(0);

export function requestIdeIssuesRefresh() {
  ideIssuesRefresh.update((n) => n + 1);
}

/** Bump to ask IdeNextBar to run Play (Ctrl+Shift+P). */
export const idePlayTrigger = writable(0);

export function requestIdePlay() {
  idePlayTrigger.update((n) => n + 1);
}

/** Recent IDE stages / commands for the command palette (max 5). */
export const ideRecentCommands = writable<{ id: string; label: string }[]>([]);

export function pushIdeRecent(id: string, label: string) {
  ideRecentCommands.update((list) => {
    const next = [{ id, label }, ...list.filter((x) => x.id !== id)];
    return next.slice(0, 5);
  });
}

// Next-action guidance copy lives in its own pure module (unit-tested
// there): plain language for first-time users, no launcher jargon.
export { computeIdeNextAction, packStatusLabel } from "./ideNextAction";

/** One-shot: open Quests AI sidebar on this quest chat session id. Cleared by QuestAiSidebar. */
export const questChatFocusId = writable<string | null>(null);

/** One-shot: open Tune Config AI sidebar on this tune chat session id. Cleared by TuneAiSidebar. */
export const tuneChatFocusId = writable<string | null>(null);

/** Focus a History event id after navigating to History stage. Cleared by ChangeHistory. */
export const historyFocusEventId = writable<string | null>(null);

/** Focus History episode by crash fingerprint (cleared by ChangeHistory). */
export const historyFocusFingerprintKey = writable<string | null>(null);

/** Focus History entries linked to this snapshot id (cleared by ChangeHistory). */
export const historyFocusSnapshotId = writable<string | null>(null);

/** History → Content (Mods): focus mod by manifest id/slug. Cleared by Mods. */
export const modsFocusId = writable<string | null>(null);

/** History → Content (Mods): fallback match by jar fileName. Cleared by Mods. */
export const modsFocusFileName = writable<string | null>(null);

/** History → Tune (ConfigEditor): open relative config path. Cleared by ConfigEditor. */
export const configFocusPath = writable<string | null>(null);

/** Optional paths to highlight when opening Diagnose from History. */
export const diagnoseFocusPaths = writable<string[] | null>(null);

/** Rich History → Diagnose handoff (paths + crash fingerprint / log). Cleared by Diagnostics. */
export type DiagnoseFocus = {
  paths?: string[] | null;
  fingerprintKey?: string | null;
  logPath?: string | null;
  episodeId?: string | null;
};
export const diagnoseFocus = writable<DiagnoseFocus | null>(null);

/** True while Tune/ConfigEditor has unsaved edits — IdeWorkspace confirms leave. */
export const tuneDirty = writable(false);

/** True while Brief listing editor has unsaved edits — IdeWorkspace confirms leave. */
export const briefDirty = writable(false);

/** True while Quests editor has unsaved edits — IdeWorkspace confirms leave. */
export const questDirty = writable(false);

/** Live mirror of launcherSettings.autoHideWorkflowRail for IDE rail. */
export const autoHideWorkflowRail = writable(false);

/** Live mirror of launcherSettings.hideIdeNextBar for the IDE top panel. */
export const hideIdeNextBar = writable(false);

/** Live mirror of launcherSettings.sidebarMode. */
export const sidebarMode = writable<SidebarMode>("full");

/** Latest launcher settings — updated when Settings persists (keeps App auto-scale in sync). */
export const launcherSettingsLive = writable<LauncherSettings | null>(null);

export function notifyLauncherSettingsChanged(settings: LauncherSettings) {
  launcherSettingsLive.set(settings);
  if (typeof window !== "undefined") {
    window.dispatchEvent(
      new CustomEvent("tuffbox:launcher-settings", { detail: settings }),
    );
  }
}

/** Icons mode only: true = labels hidden (icon rail). Persisted in localStorage. */
function createSidebarIconsCollapsed() {
  let initial = false;
  try {
    initial =
      typeof localStorage !== "undefined" &&
      localStorage.getItem("tuffbox.sidebar.icons-collapsed") === "true";
  } catch {
    initial = false;
  }
  const { subscribe, set, update } = writable(initial);
  return {
    subscribe,
    set: (v: boolean) => {
      try {
        localStorage.setItem("tuffbox.sidebar.icons-collapsed", String(v));
      } catch {
        /* ignore */
      }
      set(v);
    },
    toggle: () =>
      update((v) => {
        const next = !v;
        try {
          localStorage.setItem("tuffbox.sidebar.icons-collapsed", String(next));
        } catch {
          /* ignore */
        }
        return next;
      }),
  };
}
export const sidebarIconsCollapsed = createSidebarIconsCollapsed();

/** Home YouTube strip: under the pack shelf, or in the right column under the skin. */
export type HomeYoutubePlacement = "below" | "right";
const HOME_YT_PLACEMENT_KEY = "tuffbox-home-youtube-placement";

function readHomeYoutubePlacement(): HomeYoutubePlacement {
  try {
    const raw =
      typeof localStorage !== "undefined" ? localStorage.getItem(HOME_YT_PLACEMENT_KEY) : null;
    if (raw === "right" || raw === "below") return raw;
  } catch {
    /* ignore */
  }
  return "below";
}

function createHomeYoutubePlacement() {
  const { subscribe, set, update } = writable<HomeYoutubePlacement>(readHomeYoutubePlacement());
  const persist = (v: HomeYoutubePlacement) => {
    try {
      localStorage.setItem(HOME_YT_PLACEMENT_KEY, v);
    } catch {
      /* ignore */
    }
  };
  return {
    subscribe,
    set: (v: HomeYoutubePlacement) => {
      persist(v);
      set(v);
    },
    toggle: () => {
      update((cur) => {
        const next: HomeYoutubePlacement = cur === "right" ? "below" : "right";
        persist(next);
        return next;
      });
    },
  };
}
export const homeYoutubePlacement = createHomeYoutubePlacement();

/** App chrome brand mark: classic amber "T" or creeper-in-a-box. */
export type BrandIconId = "classic" | "creeper";

const BRAND_ICON_KEY = "tuffbox.brand-icon";

function readBrandIcon(): BrandIconId {
  try {
    const v = localStorage.getItem(BRAND_ICON_KEY);
    if (v === "creeper" || v === "classic") return v;
  } catch {
    /* ignore */
  }
  return "classic";
}

function createBrandIcon() {
  const { subscribe, set } = writable<BrandIconId>(
    typeof window !== "undefined" ? readBrandIcon() : "classic",
  );
  return {
    subscribe,
    set: (id: BrandIconId) => {
      try {
        localStorage.setItem(BRAND_ICON_KEY, id);
      } catch {
        /* ignore */
      }
      set(id);
    },
  };
}

export const brandIcon = createBrandIcon();

export const BRAND_ICON_CREEPER_SRC = "/brand/creeper-box.png";
export const BRAND_ICON_CREEPER_SRC_SM = "/brand/creeper-box-128.png";

/** Global YouTube player session — survives view switches so mini player stays on any page. */
export type YoutubePlayerSession = {
  videoId: string;
  title: string;
  originRect: DOMRect | null;
  startMini: boolean;
  /**
   * Resume playback from this offset (seconds) instead of the beginning.
   * The queue window's preview tracks currentTime and passes it here so the
   * mini/full player continues where the embed preview left off.
   */
  start?: number;
};

export const youtubePlayerSession = writable<YoutubePlayerSession | null>(null);

export function openYoutubePlayer(session: YoutubePlayerSession) {
  youtubePlayerSession.set(session);
}

export function closeYoutubePlayer() {
  youtubePlayerSession.set(null);
}

/**
 * Keeps the last known playback position (seconds) per video id, learned from
 * YouTube embed "infoDelivery" postMessages. Used to resume a video in the
 * mini/full player at the same offset the user was at in a previous embed
 * (e.g. the queue window preview) instead of restarting from 0:00.
 */
const progressById = new Map<string, number>();

export function setYoutubeVideoProgress(videoId: string, seconds: number) {
  if (!videoId || !Number.isFinite(seconds) || seconds < 0) return;
  progressById.set(videoId, seconds);
}

export function getYoutubeVideoProgress(videoId: string): number | undefined {
  return progressById.get(videoId);
}

/** Personal YouTube queue (playlist) — survives restarts. */
export type YoutubeQueueItem = {
  videoId: string;
  title: string;
  thumbnailUrl: string | null;
};

const YT_QUEUE_STORAGE_KEY = "tuffbox-youtube-queue";
type YoutubeQueuePersisted = { items: YoutubeQueueItem[]; index: number };

function readYoutubeQueuePersisted(): YoutubeQueuePersisted {
  if (typeof localStorage === "undefined") return { items: [], index: 0 };
  try {
    const raw = localStorage.getItem(YT_QUEUE_STORAGE_KEY);
    if (!raw) return { items: [], index: 0 };
    const parsed = JSON.parse(raw) as Partial<YoutubeQueuePersisted>;
    if (!parsed || !Array.isArray(parsed.items)) return { items: [], index: 0 };
    const items: YoutubeQueueItem[] = parsed.items
      .filter((v) => !!v && typeof v?.videoId === "string" && v.videoId.length > 0)
      .map((v) => ({
        videoId: v.videoId as string,
        title: typeof v.title === "string" ? v.title : "",
        thumbnailUrl: typeof v.thumbnailUrl === "string" ? v.thumbnailUrl : null,
      }));
    const index =
      typeof parsed.index === "number" && Number.isFinite(parsed.index)
        ? Math.min(Math.max(0, Math.round(parsed.index)), Math.max(0, items.length - 1))
        : 0;
    return { items, index };
  } catch {
    return { items: [], index: 0 };
  }
}

const _queueStorage = readYoutubeQueuePersisted();

export const youtubeQueueItems = writable<YoutubeQueueItem[]>(_queueStorage.items);
export const youtubeQueueIndex = writable<number>(_queueStorage.index);
export const youtubeQueueOpen = writable(false);

function persistYoutubeQueue() {
  try {
    localStorage.setItem(
      YT_QUEUE_STORAGE_KEY,
      JSON.stringify({ items: get(youtubeQueueItems), index: get(youtubeQueueIndex) }),
    );
  } catch {
    /* ignore */
  }
}

/** Append to the end; no-op if the video is already queued. */
export function youtubeQueueAdd(item: YoutubeQueueItem) {
  youtubeQueueItems.update((list) => {
    if (list.some((v) => v.videoId === item.videoId)) return list;
    return [...list, item];
  });
  persistYoutubeQueue();
}

/** Add and start playing it right away. */
export function youtubeQueueAddAndPlay(item: YoutubeQueueItem) {
  youtubeQueueItems.update((list) => {
    const existing = list.findIndex((v) => v.videoId === item.videoId);
    if (existing >= 0) {
      youtubeQueueIndex.set(existing);
      return list;
    }
    const next = [...list, item];
    youtubeQueueIndex.set(next.length - 1);
    return next;
  });
  persistYoutubeQueue();
}

export function youtubeQueuePlayAt(i: number) {
  const list = get(youtubeQueueItems);
  if (i >= 0 && i < list.length) {
    youtubeQueueIndex.set(i);
    persistYoutubeQueue();
  }
}

export function youtubeQueueNext() {
  const list = get(youtubeQueueItems);
  if (list.length === 0) return;
  youtubeQueueIndex.set((get(youtubeQueueIndex) + 1) % list.length);
  persistYoutubeQueue();
}

export function youtubeQueuePrevious() {
  const list = get(youtubeQueueItems);
  if (list.length === 0) return;
  youtubeQueueIndex.set((get(youtubeQueueIndex) - 1 + list.length) % list.length);
  persistYoutubeQueue();
}

export function youtubeQueueRemoveAt(i: number) {
  const cur = get(youtubeQueueIndex);
  youtubeQueueItems.update((list) => {
    if (i < 0 || i >= list.length) return list;
    const next = list.filter((_, k) => k !== i);
    let newIndex = cur;
    if (i < cur) newIndex = cur - 1;
    else if (i === cur) newIndex = Math.min(cur, next.length - 1);
    youtubeQueueIndex.set(newIndex);
    return next;
  });
  persistYoutubeQueue();
}

export function youtubeQueueMoveUp(i: number) {
  if (i <= 0) return;
  const cur = get(youtubeQueueIndex);
  youtubeQueueItems.update((list) => {
    const next = [...list];
    [next[i - 1], next[i]] = [next[i], next[i - 1]];
    if (cur === i) youtubeQueueIndex.set(i - 1);
    else if (cur === i - 1) youtubeQueueIndex.set(i);
    return next;
  });
  persistYoutubeQueue();
}

export function youtubeQueueMoveDown(i: number) {
  const list = get(youtubeQueueItems);
  if (i >= list.length - 1) return;
  const cur = get(youtubeQueueIndex);
  youtubeQueueItems.update((l) => {
    const next = [...l];
    [next[i], next[i + 1]] = [next[i + 1], next[i]];
    if (cur === i) youtubeQueueIndex.set(i + 1);
    else if (cur === i + 1) youtubeQueueIndex.set(i);
    return next;
  });
  persistYoutubeQueue();
}

export function youtubeQueueClear() {
  youtubeQueueItems.set([]);
  youtubeQueueIndex.set(0);
  persistYoutubeQueue();
}

export function openYoutubeQueue() {
  youtubeQueueOpen.set(true);
}

export function closeYoutubeQueue() {
  youtubeQueueOpen.set(false);
}
