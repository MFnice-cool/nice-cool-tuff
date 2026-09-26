<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open as openShell } from "@tauri-apps/plugin-shell";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import {
    PlayCircle, RefreshCw, TimerReset,
    Square, Stethoscope, Activity,
    Terminal, Shield, Crosshair,
  } from "@lucide/svelte";
  import { onDestroy, onMount, tick } from "svelte";
  import {
    ideStageRequest,
    isProjectLaunching,
    isProjectRunning,
    launchSessions,
    openLaunchLog,
    projectPath,
    projectInfo,
    runningInstances,
  } from "../lib/store";
  import EmptyState from "./EmptyState.svelte";
  import TestHardwareCard from "./test/TestHardwareCard.svelte";
  import TestLoadChart from "./test/TestLoadChart.svelte";
  import TestLabConsole from "./test/TestLabConsole.svelte";
  import TestLabOptions from "./test/TestLabOptions.svelte";
  import { killWithFeedback, launchWithFeedback } from "../lib/launch";
  import type { TestRunRecord } from "../lib/api";
  import { gb1, peaksFromSamples, pushLoadSample, type LoadSample } from "../lib/testLoad";
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
    type GroupTestOutcome,
  } from "../lib/modBisect";

  type Profile = {
    id: string;
    name: string;
    side: string;
    memoryMb?: number | null;
    jvmArgs: string[];
  };

  type LiveDebugStats = {
    hostCpuPercent: number;
    hostMemoryUsedMb: number;
    hostMemoryTotalMb: number;
    instance: null | {
      pid: number;
      profile: string;
      startedAt: number;
      cpuPercent: number;
      memoryMb: number;
      virtualMemoryMb: number;
    };
  };

  type Verdict = "pass" | "fail" | "timedOut" | "crashed";
  type LivePhase = "idle" | "launching" | "bootstrapping" | Verdict;

  type MatrixRow = {
    profile: string;
    verdict: Verdict | "skipped" | "running";
    durationSeconds: number | null;
    reason?: string;
  };

  const PASS_SIGNALS = [
    "Sound engine started",
    "Reloading ResourceManager",
    "Done (",
    "Done loading",
    "Minecraft has been loaded",
    "Started serving on",
    "For help, type \"help\"",
    "Preparing spawn area: 100%",
  ];

  const FAIL_SIGNALS = [
    "# Launch error:",
    "Exception in thread",
    "Minecraft has crashed",
    "---- Minecraft Crash Report ----",
    "Failed to start the minecraft server",
    "A fatal error has been detected",
  ];

  const CLIENT_4G_MEMORY_MB = 4096;
  const DEFAULT_TIMEOUT_S = 180;

  let documentVisible = $state(true);

  let profiles = $state<Profile[]>([]);
  let selectedProfile = $state("client");
  let log = $state("");
  let running = $state(false);
  let watching = $state(false);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let message = $state<string | null>(null);
  let startedAt: number | null = null;
  let lastLoadedPath: string | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  let now = $state(Date.now())
  let validationReport = $state<any>(null);
  let validationLoading = $state(false);
  let validationError = $state<string | null>(null);
  let autoScroll = $state(true);
  let live = $state<LiveDebugStats | null>(null);
  let killing = $state(false);
  let launchStats = $state<any>(null);
  let forceRun = $state(false);
  let autoSnapshot = $state(false);
  let levelSeed = $state("");
  let onlineModeOff = $state(true);
  let timeoutSeconds = $state(DEFAULT_TIMEOUT_S);
  /** Memory preset applied to the main run button (null = profile default). */
  type MemPresetId = "auto" | "4g" | "8g" | "custom";
  const MEM_PRESETS: Array<{ id: MemPresetId; label: string; mb: number | null }> = [
    { id: "auto", label: "Profile default", mb: null },
    { id: "4g", label: "4 GB", mb: 4096 },
    { id: "8g", label: "8 GB", mb: 8192 },
    { id: "custom", label: "Custom JVM", mb: null },
  ];
  let memPreset = $state<MemPresetId>("auto");
  let livePhase = $state<LivePhase>("idle");
  let verdictReason: string | null = null;
  let startupSeconds = $state<number | null>(null);
  let activeRunId: string | null = null;
  let finalizeInFlight = $state(false);
  let sawProcess = $state(false);
  let historyFilter = $state<"all" | "pass" | "fail" | "crashed">("all");
  let worlds = $state<{ name: string }[]>([]);
  let quickPlayWorld = $state("");
  // Right-side panel tabs (replaced the three collapsed <details> accordions).
  type SideTabId = "options" | "matrix" | "history" | "bisect";
  const sideTabs: Array<{ id: SideTabId; label: string }> = [
    { id: "options", label: "Options" },
    { id: "bisect", label: "Find culprit" },
    { id: "matrix", label: "Profile matrix" },
    { id: "history", label: "Profiles & history" },
  ];
  let sideTab = $state<SideTabId>("options");
  let matrixIds = $state<Record<string, boolean>>({});
  let matrixRunning = $state(false);
  let matrixStopOnFail = $state(true);

  // ── Automatic mod isolation (binary exclusion via group testing) ──────
  // The backend (mod_group_test) owns the algorithm: it disables groups,
  // halves them per outcome and verifies the result. This driver chains
  // launches: launch → watch verdict → report outcome → next launch, until
  // the culprit is isolated (or the cap / an inconclusive verdict stops it).
  let bisect = $state<GroupTestSession | null>(null);
  let bisectBusy = $state(false);
  let bisectAuto = $state(true);
  let bisectRunInFlight = $state(false);
  let bisectLaunches = $state(0);
  let bisectLog = $state<string[]>([]);
  const bisectActive = $derived(sessionActive(bisect));
  const bisectPhase = $derived(phaseKey(bisect));
  const bisectStatusText = $derived(statusLine(bisect));
  let matrixSummary = $state<MatrixRow[]>([]);
  let matrixAbort = $state(false);
  let serverDir = $state("");
  let activeLogRoot = $state<string | null>(null);

  let runs = $state<TestRunRecord[]>([]);
  let capturedRunIds = $state<Record<string, boolean>>({});

  let loadSamples = $state<LoadSample[]>([]);
  let activeXmxMb = $state(4096);
  const potatoPc = typeof document !== "undefined"
    && document.documentElement.classList.contains("potato-pc");

  const selected = $derived(profiles.find((p) => p.id === selectedProfile));
  /** Resolved memory for the main run button (null = profile default). */
  const mainRunMb = $derived(MEM_PRESETS.find((m) => m.id === memPreset)?.mb ?? null);
  const sharedLaunch = $derived($launchSessions[$projectPath ?? ""] ?? null);
  const sharedLaunching = $derived(isProjectLaunching($projectPath, $launchSessions));
  const sharedRunning = $derived(isProjectRunning($projectPath, $runningInstances));
  const launchBusy = $derived(running || matrixRunning || sharedLaunching || sharedRunning);
  const elapsed = $derived(startedAt ? Math.floor((now - startedAt) / 1000) : 0);
  const validationCritical = $derived(!!validationReport && (
    !validationReport.passed
    || (validationReport.graphErrors ?? 0) > 0
    || (validationReport.jsonErrors?.length ?? 0) > 0
  ));
  const validationBadge = $derived(!validationReport
    ? null
    : validationReport.passed
      ? { ok: true, label: "OK" }
      : {
          ok: false,
          label: `${(validationReport.graphErrors ?? 0) + (validationReport.jsonErrors?.length ?? 0)} errors`,
        });
  const filteredRuns = $derived(runs.filter((r) => {
    if (historyFilter === "all") return true;
    const s = normalizeStatus(r.status);
    if (historyFilter === "pass") return s === "pass" || s === "finished";
    if (historyFilter === "fail") return s === "fail" || s === "failed" || s === "timedOut";
    if (historyFilter === "crashed") return s === "crashed";
    return true;
  }));
  /** Share of recorded runs that reached a healthy state. */
  const passRate = $derived.by(() => {
    if (runs.length === 0) return 100;
    const passed = runs.filter((r) => {
      const s = normalizeStatus(r.status);
      return s === "pass" || s === "finished";
    }).length;
    return Math.round((passed / runs.length) * 100);
  });
  const avgRunSeconds = $derived.by(() => {
    const withDur = runs.filter((r) => r.durationSeconds != null);
    if (withDur.length === 0) return null;
    return Math.round(withDur.reduce((acc, r) => acc + (r.durationSeconds ?? 0), 0) / withDur.length);
  });
  const statusLabel = $derived((() => {
    switch (livePhase) {
      case "launching": return sharedLaunch?.message || "Launching…";
      case "bootstrapping": return `Bootstrapping… ${elapsed}s`;
      case "pass": return `Pass (${startupSeconds ?? elapsed}s)`;
      case "fail": return "Fail";
      case "timedOut": return "TimedOut";
      case "crashed": return "Crashed";
      default: return live?.instance || running ? `${elapsed}s` : "idle";
    }
  })());

  // Process lifecycle is authoritative. The debug sampler below remains for
  // CPU/RAM and log metrics, but it no longer decides whether a Play action
  // became a running game or silently reset after invoke.
  $effect(() => {
    const lifecycle = sharedLaunch;
    if (!lifecycle) return;
    if (lifecycle.phase === "running") {
      sawProcess = true;
      if (!startedAt) startedAt = (lifecycle.startedAt || 0) * 1000 || Date.now();
      if (livePhase === "idle" || livePhase === "launching") livePhase = "bootstrapping";
      if (!watching) startPolling();
      return;
    }
    if (lifecycle.phase === "failed" && (livePhase === "launching" || livePhase === "bootstrapping")) {
      error = lifecycle.error?.message || lifecycle.message || "Launch failed.";
      verdictReason = error;
      livePhase = "fail";
      running = false;
      return;
    }
    if (lifecycle.phase === "exited" && live?.instance) {
      live = { ...live, instance: null };
    }
    if (lifecycle.phase === "exited" && running && !finalizeInFlight && livePhase === "bootstrapping") {
      void finalizeActive(
        lifecycle.error ? "crashed" : "fail",
        lifecycle.error?.message || lifecycle.message || "Process exited before pass signal",
      );
    }
  });

  $effect(() => {
    if ($projectPath && lastLoadedPath !== $projectPath) loadProfiles(true);
  });

  function formatRam(mb?: number | null): string {
    const v = mb ?? 4096;
    return v >= 1024 ? `${(v / 1024).toFixed(v % 1024 ? 1 : 0)} GB` : `${v} MB`;
  }

  function normalizeStatus(s: string) {
    return s;
  }

  /** Tailwind border tint per run verdict (used by history rows). */
  function runBorderClass(verdict: string): string {
    switch (verdict) {
      case "pass":
      case "finished":
        return "border-[color-mix(in_srgb,var(--accent-primary)_28%,transparent)]";
      case "fail":
      case "failed":
        return "border-[color-mix(in_srgb,var(--accent-danger)_40%,transparent)]";
      case "crashed":
        return "border-[color-mix(in_srgb,var(--accent-danger)_55%,transparent)]";
      case "timedOut":
        return "border-[color-mix(in_srgb,var(--accent-warning)_45%,transparent)]";
      case "started":
        return "border-[color-mix(in_srgb,var(--accent-warning)_28%,transparent)]";
      default:
        return "border-[var(--border-color)]";
    }
  }

  function logSliceForCurrentRun(text: string): string {
    const marker = "# TuffBox launching";
    const idx = text.lastIndexOf(marker);
    if (idx >= 0) return text.slice(idx);
    return text;
  }

  function detectPass(text: string): string | null {
    const slice = logSliceForCurrentRun(text);
    for (const sig of PASS_SIGNALS) {
      if (slice.includes(sig)) return sig;
    }
    return null;
  }

  function detectFail(text: string): string | null {
    const slice = logSliceForCurrentRun(text);
    for (const sig of FAIL_SIGNALS) {
      if (slice.includes(sig)) return sig;
    }
    // Early Stopping! before any pass is fail-ish
    if (slice.includes("Stopping!") && !detectPass(text)) return "Stopping!";
    return null;
  }

  function formatRunTime(value: string) {
    const seconds = Number(value);
    if (!Number.isFinite(seconds)) return value;
    return new Date(seconds * 1000).toLocaleString();
  }

  function verdictClass(status: string) {
    const s = status.toLowerCase();
    if (s === "pass" || s === "finished") return "pass";
    if (s === "crashed") return "crashed";
    if (s === "timedout" || s === "timeout") return "timedOut";
    if (s === "fail" || s === "failed") return "fail";
    if (s === "started") return "started";
    return status;
  }

  function verdictLabel(status: string) {
    const s = status.toLowerCase();
    if (s === "finished") return "Pass*";
    if (s === "failed") return "Fail";
    if (s === "timedout") return "TimedOut";
    if (s === "pass") return "Pass";
    if (s === "fail") return "Fail";
    if (s === "crashed") return "Crashed";
    if (s === "started") return "Running";
    return status;
  }

  async function loadStats() {
    if (!$projectPath) return;
    try {
      launchStats = await invoke("get_launch_stats", { path: $projectPath });
    } catch {
      launchStats = null;
    }
  }

  async function loadWorlds() {
    if (!$projectPath) return;
    try {
      worlds = await invoke("list_worlds", { path: $projectPath });
      if (!quickPlayWorld && worlds[0]) quickPlayWorld = worlds[0].name;
    } catch {
      worlds = [];
    }
  }

  async function runValidation() {
    if (!$projectPath) return;
    validationLoading = true;
    validationError = null;
    try {
      validationReport = await invoke("run_project_validation", { path: $projectPath });
    } catch (e) {
      validationError = String(e);
    } finally {
      validationLoading = false;
    }
  }

  async function loadProfiles(force = false) {
    if (!$projectPath) return;
    if (!force && lastLoadedPath === $projectPath && profiles.length > 0) return;
    loading = true;
    error = null;
    try {
      profiles = await invoke("list_profiles", { path: $projectPath });
      selectedProfile = profiles.find((p) => p.id === selectedProfile)?.id ?? profiles[0]?.id ?? "client";
      const defaults: Record<string, boolean> = {};
      for (const p of profiles) {
        defaults[p.id] = /client|server|low/i.test(p.id) || p.id === selectedProfile;
      }
      if (Object.keys(matrixIds).length === 0) matrixIds = defaults;
      lastLoadedPath = $projectPath;
      if (!serverDir.trim()) serverDir = defaultServerDir();
      await refreshLog();
      await loadRuns();
      await loadStats();
      await loadWorlds();
      await refreshLive();
      if (live?.instance) {
        running = true;
        sawProcess = true;
        livePhase = "bootstrapping";
        startedAt = (live.instance.startedAt || 0) * 1000 || Date.now();
        startPolling();
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function loadRuns() {
    if (!$projectPath) return;
    try {
      runs = await invoke("list_test_runs", { path: $projectPath });
      for (const r of runs) {
        if ((r.capturedPaths?.length ?? 0) > 0) {
          capturedRunIds = { ...capturedRunIds, [r.id]: true };
        }
      }
    } catch {
      runs = [];
    }
  }

  function projectDir(): string {
    if (!$projectPath) return "";
    return $projectPath.replace(/[/\\][^/\\]+$/, "");
  }

  function defaultServerDir(): string {
    const dir = projectDir();
    const name = ($projectInfo?.id || $projectInfo?.name || "modpack")
      .toString()
      .replace(/[<>:"/\\|?*]+/g, "-")
      .trim() || "modpack";
    return dir ? `${dir}/${name}-server` : `${name}-server`;
  }

  async function ensureServerDir(): Promise<string | null> {
    if (!$projectPath) return null;
    if (!serverDir.trim()) serverDir = defaultServerDir();
    const picked = await openDialog({
      directory: true,
      multiple: false,
      title: "Choose folder for the server instance",
      defaultPath: serverDir.trim() || projectDir() || undefined,
    });
    if (picked == null) return null;
    const path = Array.isArray(picked) ? picked[0] : picked;
    if (!path) return null;
    serverDir = path;
    return path;
  }

  async function maybeSnapshot(label: string) {
    if (!autoSnapshot || !$projectPath) return;
    const dir = projectDir();
    try {
      await invoke("create_snapshot", {
        projectDir: dir,
        name: `pre-test-${Date.now()}`,
        reason: `Auto snapshot before ${label}`,
      });
      message = `Snapshot taken before ${label}.`;
    } catch (e) {
      error = `Snapshot failed: ${e}`;
    }
  }

  function canLaunch(): boolean {
    if (!$projectPath || launchBusy) return false;
    if (validationCritical && !forceRun) {
      error = "Validation has critical issues. Enable Force run to launch anyway.";
      return false;
    }
    return true;
  }

  async function beginRun(opts: {
    profile: string;
    label: string;
    memoryMbOverride?: number | null;
    quickPlayType?: string | null;
    quickPlayValue?: string | null;
    prepareServer?: boolean;
    serverDir?: string | null;
    openServerConsole?: boolean;
  }): Promise<boolean> {
    if (!canLaunch()) return false;
    running = true;
    sawProcess = false;
    finalizeInFlight = false;
    startedAt = Date.now();
    loadSamples = [];
    const profile = profiles.find((p) => p.id === opts.profile);
    activeXmxMb = opts.memoryMbOverride ?? profile?.memoryMb ?? 4096;
    error = null;
    message = null;
    log = "";
    livePhase = "launching";
    verdictReason = null;
    startupSeconds = null;
    activeRunId = null;
    activeLogRoot = opts.serverDir
      ? `${opts.serverDir.replace(/[/\\]+$/, "")}/tuffbox.project.json`
      : $projectPath;
    try {
      await maybeSnapshot(opts.label);
      if (opts.prepareServer && opts.serverDir) {
        await invoke("generate_server_properties", {
          path: $projectPath,
          levelSeed: levelSeed.trim() || null,
          onlineMode: onlineModeOff ? false : true,
          targetDir: opts.serverDir,
        });
      }
      const res = await launchWithFeedback(
        {
          path: $projectPath!,
          profile: opts.profile,
          memoryMbOverride: opts.memoryMbOverride ?? null,
          quickPlayType: opts.quickPlayType ?? null,
          quickPlayValue: opts.quickPlayValue ?? null,
          serverDir: opts.serverDir ?? null,
          levelSeed: levelSeed.trim() || null,
          onlineMode: onlineModeOff ? false : true,
        },
        {
          openLog: false,
          logPath: activeLogRoot,
          logTitle: opts.openServerConsole ? "Server console" : null,
        },
      );
      if (!res) {
        const lifecycle = $launchSessions[$projectPath ?? ""];
        running = false;
        livePhase = lifecycle?.phase === "failed" ? "fail" : "idle";
        if (lifecycle?.error?.message) error = lifecycle.error.message;
        activeLogRoot = null;
        return false;
      }
      await loadStats();
      await loadRuns();
      activeRunId = runs[0]?.id ?? null;
      livePhase = "bootstrapping";
      message = opts.serverDir
        ? `${opts.label} started — server folder: ${opts.serverDir}`
        : `${opts.label} started — watching latest.log.`;
      startPolling();
      return true;
    } catch (e) {
      error = String(e);
      running = false;
      livePhase = "fail";
      verdictReason = String(e);
      activeLogRoot = null;
      return false;
    }
  }

  /** Main run button: launch the selected target with the active memory preset. */
  async function runFromBar() {
    const isServerTarget = String(selected?.side ?? "").toLowerCase() === "server" || selectedProfile === "server";
    if (isServerTarget) {
      const dir = await ensureServerDir();
      if (!dir) return;
      await beginRun({
        profile: selectedProfile,
        label: "Run server",
        prepareServer: true,
        serverDir: dir,
        openServerConsole: true,
        memoryMbOverride: mainRunMb,
      });
      return;
    }
    activeLogRoot = $projectPath;
    await beginRun({
      profile: selectedProfile,
      label: "Run test",
      openServerConsole: false,
      memoryMbOverride: mainRunMb,
    });
  }

  async function runServer() {
    const dir = await ensureServerDir();
    if (!dir) {
      message = "Server folder selection cancelled.";
      return;
    }
    await beginRun({
      profile: "server",
      label: "Run server",
      prepareServer: true,
      serverDir: dir,
      openServerConsole: true,
    });
  }

  async function runClient4Ram() {
    const client = profiles.find((p) => p.id === "client")
      ?? profiles.find((p) => String(p.side).toLowerCase() !== "server")
      ?? profiles.find((p) => p.id === selectedProfile);
    if (!client) {
      error = "No client profile found.";
      return;
    }
    await beginRun({
      profile: client.id,
      label: "Run client 4 RAM",
      memoryMbOverride: CLIENT_4G_MEMORY_MB,
      openServerConsole: false,
    });
  }

  async function quickPlay() {
    if (!quickPlayWorld) {
      error = "Pick a world in saves/ for Quick Play.";
      return;
    }
    await beginRun({
      profile: selectedProfile,
      label: `Quick Play · ${quickPlayWorld}`,
      quickPlayType: "singleplayer",
      quickPlayValue: quickPlayWorld,
    });
  }

  async function finalizeActive(verdict: Verdict, reason: string, kill = false) {
    if (finalizeInFlight) return;
    finalizeInFlight = true;
    livePhase = verdict;
    verdictReason = reason;
    if (verdict === "pass") startupSeconds = elapsed;
    const duration = elapsed;
    const runId = activeRunId;
    const shouldKill = kill || matrixRunning;
    if (shouldKill && $projectPath) {
      try {
        await killWithFeedback($projectPath);
      } catch {
        // ignore
      }
    }
    running = false;
    if ($projectPath && runId) {
      try {
        const peaks = peaksFromSamples(loadSamples);
        await invoke("finalize_test_run", {
          path: $projectPath,
          runId,
          status: verdict,
          durationSeconds: duration,
          verdictReason: reason,
          peakProcMb: peaks ? Math.round(peaks.peakProcMb) : null,
          peakHostMb: peaks ? Math.round(peaks.peakHostMb) : null,
          hostTotalMb: peaks
            ? Math.round(peaks.lastHostTotalMb)
            : (live?.hostMemoryTotalMb ?? null),
          xmxMb: Math.round(activeXmxMb),
        });
      } catch {
        // ignore
      }
      try {
        await captureRunLogs({ id: runId }, true);
      } catch {
        // ignore
      }
    }
    await loadRuns();
    await loadStats();
    // Automatic isolation: this launch belonged to a bisect step — feed the
    // verdict back into the group-test session (which prepares the next
    // mod layout) and chain the next launch when auto mode is on.
    if (bisectRunInFlight && bisect) {
      bisectRunInFlight = false;
      await advanceBisect(verdict);
    }
  }

  async function evaluateLogAndLifecycle() {
    if (!running || finalizeInFlight) return;
    if (livePhase === "launching" && (live?.instance || log.length > 40)) {
      livePhase = "bootstrapping";
    }
    if (live?.instance) sawProcess = true;

    const fail = detectFail(log);
    if (fail && livePhase !== "pass") {
      // Prefer crash check after exit; while alive treat as fail signal
      if (!live?.instance && sawProcess) {
        let crashed = false;
        try {
          crashed = await invoke<boolean>("has_crashed", { path: $projectPath });
        } catch {
          crashed = false;
        }
        await finalizeActive(crashed ? "crashed" : "fail", fail);
        return;
      }
      if (fail.includes("Launch error") || fail.includes("Crash Report") || fail.includes("fatal")) {
        await finalizeActive("fail", fail);
        return;
      }
    }

    const pass = detectPass(log);
    if (pass && livePhase !== "pass") {
      await finalizeActive("pass", pass);
      return;
    }

    if (timeoutSeconds > 0 && elapsed >= timeoutSeconds && livePhase === "bootstrapping") {
      await finalizeActive("timedOut", `No pass signal within ${timeoutSeconds}s`, true);
      return;
    }

    // Process exited without pass
    if (sawProcess && !live?.instance && watching && livePhase === "bootstrapping") {
      let crashed = false;
      try {
        crashed = await invoke<boolean>("has_crashed", { path: $projectPath });
      } catch {
        crashed = false;
      }
      if (crashed) {
        await finalizeActive("crashed", "Crash report detected after exit");
      } else if (fail) {
        await finalizeActive("fail", fail);
      } else if (log.includes("Process exited") || log.includes("Stopping!")) {
        await finalizeActive("fail", "Process exited before pass signal");
      }
    }
  }

  async function refreshLog() {
    if (!$projectPath) return;
    try {
      log = await invoke("get_launch_log", { path: activeLogRoot || $projectPath });
      await evaluateLogAndLifecycle();
    } catch {
      // latest.log may not exist before first run.
    }
  }

  async function refreshLive() {
    if (!$projectPath) return;
    try {
      live = await invoke("get_live_debug_stats", { instanceId: $projectPath });
      if (live?.instance) {
        running = true;
        sawProcess = true;
        if (!startedAt) startedAt = (live.instance.startedAt || 0) * 1000 || Date.now();
        if (livePhase === "idle" || livePhase === "launching") livePhase = "bootstrapping";
      }
    } catch {
      // ignore sampler failures
    }
  }

  async function captureRunLogs(run: { id: string }, silent = false) {
    if (!$projectPath) return;
    try {
      const dir: string = await invoke("capture_test_run_logs", { path: $projectPath, runId: run.id });
      capturedRunIds = { ...capturedRunIds, [run.id]: true };
      if (!silent) message = `Captured logs to ${dir}`;
      await loadRuns();
    } catch (e) {
      if (!silent) error = String(e);
    }
  }

  async function openRunLogs(run: TestRunRecord) {
    if (!$projectPath) return;
    const captureDir = `${projectDir()}/.tuffbox/test-runs/${run.id}`;
    try {
      if (!capturedRunIds[run.id]) await captureRunLogs(run, true);
      await openShell(captureDir);
    } catch {
      try {
        await openShell(run.logPath);
      } catch (e) {
        error = String(e);
      }
    }
  }

  function openDiagnose() {
    ideStageRequest.set("diagnose");
  }

  async function reRun(run: TestRunRecord) {
    selectedProfile = run.profile;
    if (run.profile === "server") await runServer();
    else await beginRun({ profile: run.profile, label: `Re-run · ${run.profile}`, openServerConsole: false });
  }

  async function killInstance() {
    if (!$projectPath || killing) return;
    killing = true;
    error = null;
    try {
      const stopped = await killWithFeedback($projectPath);
      if (!stopped) return;
      message = "Stopping game…";
      if (running && !finalizeInFlight && livePhase === "bootstrapping") {
        await finalizeActive("fail", "Stopped by user");
      } else {
        running = false;
        if (livePhase === "launching" || livePhase === "bootstrapping") livePhase = "idle";
      }
      await refreshLog();
      await loadRuns();
    } finally {
      killing = false;
    }
  }

  async function runMatrix() {
    if (!$projectPath || launchBusy) return;
    if (validationCritical && !forceRun) {
      error = "Validation has critical issues. Enable Force run to launch matrix.";
      return;
    }
    const queue = profiles.filter((p) => matrixIds[p.id]);
    if (queue.length === 0) {
      error = "Select at least one profile for the matrix.";
      return;
    }
    matrixRunning = true;
    matrixAbort = false;
    sideTab = "matrix";
    matrixSummary = queue.map((p) => ({
      profile: p.id,
      verdict: "running",
      durationSeconds: null,
    }));
    message = `Matrix: ${queue.length} profile(s), sequential.`;

    for (let i = 0; i < queue.length; i++) {
      if (matrixAbort) {
        for (let j = i; j < queue.length; j++) {
          matrixSummary[j] = { ...matrixSummary[j], verdict: "skipped", reason: "Aborted" };
        }
        break;
      }
      const profile = queue[i];
      matrixSummary[i] = { ...matrixSummary[i], verdict: "running" };
      const isServer = profile.id === "server" || String(profile.side).toLowerCase() === "server";
      let stagedServerDir: string | null = null;
      if (isServer) {
        stagedServerDir = serverDir.trim() || defaultServerDir();
        serverDir = stagedServerDir;
      }
      const ok = await beginRun({
        profile: profile.id,
        label: `Matrix · ${profile.name}`,
        prepareServer: isServer,
        serverDir: stagedServerDir,
        openServerConsole: isServer,
        memoryMbOverride: null,
      });
      if (!ok) {
        matrixSummary[i] = {
          profile: profile.id,
          verdict: "fail",
          durationSeconds: elapsed,
          reason: error ?? "Launch failed",
        };
        if (matrixStopOnFail) {
          for (let j = i + 1; j < queue.length; j++) {
            matrixSummary[j] = { ...matrixSummary[j], verdict: "skipped", reason: "Stop on fail" };
          }
          break;
        }
        continue;
      }

      // Wait until current run finalizes
      while (running && !matrixAbort) {
        await new Promise((r) => setTimeout(r, 500));
      }
      const verdict = (livePhase === "pass" || livePhase === "fail" || livePhase === "timedOut" || livePhase === "crashed")
        ? livePhase
        : "fail";
      matrixSummary[i] = {
        profile: profile.id,
        verdict,
        durationSeconds: startupSeconds ?? elapsed,
        reason: verdictReason ?? undefined,
      };
      // Brief pause between JVMs
      await new Promise((r) => setTimeout(r, 800));
      if (matrixStopOnFail && verdict !== "pass") {
        for (let j = i + 1; j < queue.length; j++) {
          matrixSummary[j] = { ...matrixSummary[j], verdict: "skipped", reason: "Stop on fail" };
        }
        break;
      }
    }

    matrixRunning = false;
    message = "Matrix finished.";
  }

  function stopMatrix() {
    matrixAbort = true;
  }

  function onSecondaryToggle(e: Event) {
    const el = e.currentTarget as HTMLElement;
    if (!el) return;
    // Expand downward into scroll space instead of compressing the log above.
    requestAnimationFrame(() => {
      el.scrollIntoView({ block: "nearest", behavior: "smooth" });
    });
  }

  function startPolling() {
    watching = true;
    now = Date.now();
    syncPollingTimer();
  }

  function syncPollingTimer() {
    if (timer) {
      clearInterval(timer);
      timer = null;
    }
    if (watching && documentVisible) {
      timer = setInterval(pollTick, 1000);
      pollTick();
    }
  }

  function pollTick() {
    if (!watching || !documentVisible) return;
    now = Date.now();
    refreshLog();
    void refreshLive().then(() => {
      if (!watching || startedAt == null || !live) return;
      pushLoadSample(loadSamples, {
        tSec: (Date.now() - startedAt) / 1000,
        hostUsedMb: live.hostMemoryUsedMb,
        hostTotalMb: live.hostMemoryTotalMb,
        procRssMb: live.instance?.memoryMb ?? 0,
        hostCpuPct: live.hostCpuPercent,
      });
    });
  }

  function stopWatching() {
    watching = false;
    if (timer) {
      clearInterval(timer);
      timer = null;
    }
  }

  function onVisibilityChange() {
    documentVisible = document.visibilityState === "visible";
    syncPollingTimer();
  }

  async function refreshBisect() {
    if (!$projectPath) return;
    try {
      bisect = await invoke<GroupTestSession | null>("get_mod_group_test", {
        path: $projectPath,
      });
    } catch {
      bisect = null;
    }
  }

  /** Client profile used for isolation launches (never the server one). */
  function bisectProfileId(): string {
    const client = profiles.find((p) => p.id === "client")
      ?? profiles.find((p) => String(p.side).toLowerCase() !== "server")
      ?? profiles[0];
    return client?.id ?? "client";
  }

  async function startBisect() {
    if (!$projectPath || bisectBusy || launchBusy) return;
    bisectBusy = true;
    error = null;
    try {
      bisect = await invoke<GroupTestSession>("start_mod_group_test", {
        path: $projectPath,
        suspected: null,
      });
      bisectLaunches = 0;
      bisectLog = [
        `Suspect pool: ${bisect.pool.length} mods (recently changed + suspected + content mods; protected mods excluded).`,
        `Estimated launches: ~${estimateLaunches(bisect.pool.length)}. A snapshot was taken — Restore brings everything back.`,
      ];
      sideTab = "bisect";
      if (bisectAuto) await launchBisectStep();
    } catch (e) {
      error = String(e);
    } finally {
      bisectBusy = false;
    }
  }

  async function launchBisectStep() {
    if (!bisect || !$projectPath) return;
    bisectLaunches += 1;
    bisectRunInFlight = true;
    bisectLog = [
      ...bisectLog,
      `Launch ${bisectLaunches} (step ${bisect.step + (bisectPhase === "needCovering" ? 0 : 1)}): ${statusLine(bisect)}`,
    ].slice(-40);
    const ok = await beginRun({
      profile: bisectProfileId(),
      label: `Find culprit · launch ${bisectLaunches}`,
      memoryMbOverride: mainRunMb,
    });
    if (!ok) {
      // Launch itself failed — stop the auto loop, keep the session for
      // manual reporting.
      bisectRunInFlight = false;
      bisectAuto = false;
      bisectLog = [...bisectLog, "Launch failed — automatic isolation paused. Fix the launch error or report the outcome manually."].slice(-40);
    }
  }

  /** Called from finalizeActive once a bisect launch reached a verdict. */
  async function advanceBisect(verdict: Verdict) {
    if (!bisect || !$projectPath) return;
    const outcome = outcomeFromVerdict(verdict);
    if (!outcome) {
      bisectAuto = false;
      bisectLog = [
        ...bisectLog,
        `Launch ${bisectLaunches}: TIMED OUT — inconclusive. Automatic isolation paused; re-run with a larger timeout or report the outcome manually.`,
      ].slice(-40);
      return;
    }
    bisectBusy = true;
    try {
      bisect = await invoke<GroupTestSession>("report_mod_group_test_outcome", {
        path: $projectPath,
        outcome: outcome as GroupTestOutcome,
      });
      const key = phaseKey(bisect);
      if (key === "done") {
        bisectLog = [
          ...bisectLog,
          `Isolated: ${bisect.defectives.join(", ") || "none"}${bisect.verified ? " (verified)" : ""}. Only the culprit mods stay disabled; use Restore to re-enable everything.`,
        ].slice(-40);
        return;
      }
      if (key === "failed") {
        bisectAuto = false;
        bisectLog = [...bisectLog, `Stopped: ${statusLine(bisect)}`].slice(-40);
        return;
      }
      bisectLog = [...bisectLog, `Launch ${bisectLaunches}: ${verdict.toUpperCase()} → ${statusLine(bisect)}`].slice(-40);
      if (bisectAuto && shouldAutoContinue(bisect, bisectLaunches)) {
        if ($projectPath && (running || live?.instance)) {
          try {
            await killWithFeedback($projectPath);
          } catch {
            /* the pass path leaves the game running — best effort */
          }
        }
        await launchBisectStep();
      } else if (!shouldAutoContinue(bisect, bisectLaunches)) {
        bisectAuto = false;
        bisectLog = [
          ...bisectLog,
          `Safety cap reached (${MAX_AUTO_BISECT_LAUNCHES} launches) — continue manually or start a new isolation.`,
        ].slice(-40);
      }
    } catch (e) {
      bisectAuto = false;
      error = String(e);
    } finally {
      bisectBusy = false;
    }
  }

  /** Manual override for inconclusive launches. */
  async function reportBisectManually(outcome: GroupTestOutcome) {
    if (!$projectPath || !bisect || bisectBusy) return;
    bisectBusy = true;
    try {
      bisect = await invoke<GroupTestSession>("report_mod_group_test_outcome", {
        path: $projectPath,
        outcome,
      });
      bisectLog = [...bisectLog, `Manual report: ${outcome.toUpperCase()} → ${statusLine(bisect)}`].slice(-40);
      if (bisectAuto && shouldAutoContinue(bisect, bisectLaunches)) {
        await launchBisectStep();
      }
    } catch (e) {
      error = String(e);
    } finally {
      bisectBusy = false;
    }
  }

  async function stopBisectAndRestore() {
    if (!$projectPath || bisectBusy) return;
    bisectBusy = true;
    try {
      await invoke("cancel_mod_group_test", { path: $projectPath });
      bisect = null;
      bisectAuto = true;
      bisectRunInFlight = false;
      bisectLog = [...bisectLog, "Cancelled — snapshot restored, all mods re-enabled."].slice(-40);
    } catch (e) {
      error = String(e);
    } finally {
      bisectBusy = false;
    }
  }

  onMount(() => {
    void refreshBisect();
    documentVisible = document.visibilityState === "visible";
    document.addEventListener("visibilitychange", onVisibilityChange);
  });

  onDestroy(() => {
    document.removeEventListener("visibilitychange", onVisibilityChange);
    if (timer) clearInterval(timer);
  });
</script>

<div class="flex flex-col h-full min-h-0 overflow-hidden w-full">
  <div class="flex items-center justify-between gap-4 px-1 pt-1 pb-3 shrink-0">
    <div class="flex items-center gap-2.5 font-bold text-[var(--text-secondary)]">
      <PlayCircle size={18} /> <span>Test · launch lab</span>
    </div>
    <div class="flex items-center">
      <button class="ghost" onclick={() => loadProfiles(true)} disabled={!$projectPath || loading} title="Reload profiles">
        <RefreshCw size={14} class={loading ? "spin" : ""} />
        Profiles
      </button>
      <button class="secondary" onclick={refreshLog} disabled={!$projectPath}>
        <Terminal size={14} /> Tail log
      </button>
      <button class="secondary" onclick={runValidation} disabled={!$projectPath || validationLoading}>
        <Shield size={14} />
        {validationLoading ? "Checking…" : "Validate"}
      </button>
      {#if validationBadge}
        <span class="val-badge" class:ok={validationBadge.ok} class:bad={!validationBadge.ok}>
          {validationBadge.label}
        </span>
      {/if}
      <button class="danger" onclick={killInstance} disabled={!$projectPath || (!live?.instance && !sharedRunning) || killing} title="Kill game/server process">
        <Square size={14} />
        {killing ? "Stopping…" : "Kill"}
      </button>
    </div>
  </div>

  {#if error}<div class="px-3.5 py-3 rounded-[var(--border-radius-lg)] mb-2 border shrink-0 text-[var(--accent-danger)] bg-[color-mix(in_srgb,var(--accent-danger)_8%,transparent)] border-[color-mix(in_srgb,var(--accent-danger)_28%,transparent)]">{error}</div>{/if}
  {#if message}<div class="px-3.5 py-3 rounded-[var(--border-radius-lg)] mb-2 border shrink-0 text-[var(--accent-primary)] bg-[color-mix(in_srgb,var(--accent-primary)_8%,transparent)] border-[color-mix(in_srgb,var(--accent-primary)_25%,transparent)]">{message}</div>{/if}
  {#if validationError}<div class="px-3.5 py-3 rounded-[var(--border-radius-lg)] mb-2 border shrink-0 text-[var(--accent-danger)] bg-[color-mix(in_srgb,var(--accent-danger)_8%,transparent)] border-[color-mix(in_srgb,var(--accent-danger)_28%,transparent)]">{validationError}</div>{/if}

  {#if !$projectPath}
    <EmptyState icon={PlayCircle} title="No project selected" description="Open a project to run test profiles." />
  {:else}
    <div class="flex-1 min-h-0 flex flex-col overflow-hidden gap-4">

      <!-- ── Launch toolbar: status + target + preset + run/kill ─── -->
      <div class="launch-bar glass-card shrink-0">
        <div class="flex items-center gap-3 flex-wrap">
          <div class="flex items-center gap-2 font-semibold rounded-lg px-3.5 py-2 transition-colors duration-150 status-chip {
            livePhase === "pass"
              ? "pass"
              : (livePhase === "fail" || livePhase === "crashed" || livePhase === "timedOut")
                ? "fail"
                : (livePhase === "launching" || livePhase === "bootstrapping")
                  ? "running"
                  : "idle"
          }">
            <TimerReset size={16} />
            <span>{statusLabel}</span>
            {#if live?.instance}
              <span class="chip-pid">PID {live.instance.pid}</span>
            {/if}
          </div>
          <label class="field">
            <span class="field-label">Target</span>
            <select bind:value={selectedProfile} disabled={running || matrixRunning} class="min-w-[180px]">
              {#each profiles as p (p.id)}
                <option value={p.id}>
                  {p.name} · {p.id === "server" ? "Dedicated Server" : "Client"}
                </option>
              {/each}
            </select>
          </label>
          <label class="field">
            <span class="field-label">Memory</span>
            <select bind:value={memPreset} disabled={running || matrixRunning} class="min-w-[150px]">
              {#each MEM_PRESETS as m (m.id)}
                <option value={m.id}>{m.label}</option>
              {/each}
            </select>
          </label>

          <button
            class="run-main"
            class:run={livePhase === "launching" || livePhase === "bootstrapping"}
            disabled={running || matrixRunning || !selectedProfile}
            onclick={() => void runFromBar()}
            title="Launch the selected target with the active memory preset"
          >
            {#if livePhase === "launching" || livePhase === "bootstrapping"}
              <span class="mini-spinner"></span>
              {livePhase === "launching" ? "Launching…" : `Running… ${elapsed}s`}
            {:else}
              <PlayCircle size={17} /> Run test
            {/if}
          </button>

          {#if running || live?.instance}
            <button class="kill-btn" onclick={killInstance} disabled={killing} title="Kill game/server process">
              <Square size={15} />
              {killing ? "Stopping…" : "Stop"}
            </button>
          {/if}
        </div>

        <div class="flex items-center gap-3 mt-3 pt-3 border-t border-[color:var(--border-color)]">
          <span class="launch-note text-[12.5px]">
            {#if running}
              Process active — watching <span class="font-mono text-[color:var(--text-muted)]">latest.log</span> for a pass/fail signal.
            {:else}
              Ready to launch. Pick a target and memory preset, then run.
            {/if}
          </span>
          <span class="launch-note text-[12px] ml-auto text-[color:var(--text-muted)]">
            {selected ? `${selected.name} · ${formatRam(selected.memoryMb)}` : ""}
          </span>
        </div>
      </div>

      <!-- ── Main area: log column + right settings column ──────── -->
      <div class="flex-[1_1_auto] min-h-0 min-w-0 grid grid-cols-[minmax(0,1fr)_minmax(300px,0.85fr)] gap-4 max-[1199px]:grid-cols-1 max-[1199px]:grid-rows-[minmax(220px,38vh)_minmax(0,1fr)]">

        <!-- Left: load + log (vertical stack) -->
        <div class="min-w-0 min-h-0 flex flex-col gap-4">
          <div class="shrink-0 flex flex-col overflow-hidden glass-card px-4 py-3 gap-2.5">
            <h3 class="m-0 text-[12px] font-bold text-[color:var(--text-secondary)] shrink-0 flex items-center gap-2">
              <Activity size={14} class="text-[var(--accent-primary)]" />
              Load
            </h3>
            <TestLoadChart samples={loadSamples} xmxMb={activeXmxMb} potato={potatoPc} />
            {#if loadSamples.length > 0}
              <TestHardwareCard samples={loadSamples} xmxMb={activeXmxMb} />
            {/if}
          </div>
          <TestLabConsole
            bind:log
            {running}
            {watching}
            bind:live
            {elapsed}
            bind:activeLogRoot
            projectPath={$projectPath}
            onclear={() => (log = "")}
            onopenserver={() => activeLogRoot && openLaunchLog(activeLogRoot, "Server console")}
            ondiagnose={openDiagnose}
            onpause={stopWatching}
            onwatch={startPolling}
          />
        </div>

        <!-- Right: tabbed panel (Options / Matrix / History) -->
        <div class="min-w-0 min-h-0 flex flex-col overflow-hidden glass-card">
          <div class="flex items-center gap-2 px-2.5 pt-2.5 pb-2 shrink-0 flex-wrap" role="tablist">
            <div class="seg-tabs" role="group">
              {#each sideTabs as tab (tab.id)}
                <button
                  type="button"
                  role="tab"
                  aria-selected={sideTab === tab.id}
                  class="seg-tab"
                  class:active={sideTab === tab.id}
                  onclick={() => (sideTab = tab.id)}
                >
                  {tab.label}
                  {#if tab.id === "options" && validationBadge}
                    <span class="side-tab-dot {validationBadge.ok ? "ok" : "bad"}"></span>
                  {/if}
                </button>
              {/each}
            </div>
          </div>

          <div class="flex-1 min-h-0 overflow-auto px-4 py-4">
            {#if sideTab === "options"}
              <TestLabOptions
              projectPath={$projectPath}
              bind:profiles
              bind:selectedProfile
              {running}
              {matrixRunning}
              {validationLoading}
              bind:validationReport
              {validationBadge}
              bind:forceRun
              bind:autoSnapshot
              bind:timeoutSeconds
              bind:worlds
              bind:quickPlayWorld
              bind:serverDir
              bind:levelSeed
              bind:onlineModeOff
              bind:startupSeconds
              bind:livePhase
              {formatRam}
              onquickplay={quickPlay}
              onbrowseserver={() => void ensureServerDir()}
              ondefaultserver={() => (serverDir = defaultServerDir())}
              onwriteserverprops={async () => {
                try {
                  const dir = serverDir.trim() || defaultServerDir();
                  await invoke("generate_server_properties", {
                    path: $projectPath,
                    levelSeed: levelSeed.trim() || null,
                    onlineMode: onlineModeOff ? false : true,
                    targetDir: dir,
                  });
                  message = `server.properties written to ${dir}`;
                } catch (e) { error = String(e); }
              }}
            />
            {:else if sideTab === "matrix"}
              <div class="grid gap-3.5">
                <div class="flex items-center gap-3 flex-wrap">
                  <label class="inline-flex items-center gap-2 cursor-pointer"><input type="checkbox" class="w-auto accent-[var(--accent-primary)]" bind:checked={matrixStopOnFail} /> Stop on fail</label>
                  <button class="secondary" onclick={runMatrix} disabled={running || matrixRunning}>Run matrix</button>
                  {#if matrixRunning}
                    <button class="danger" onclick={stopMatrix}>Stop queue</button>
                  {/if}
                </div>
                <div class="flex flex-wrap gap-2.5 gap-x-4">
                  {#each profiles as p (p.id)}
                    <label class="inline-flex items-center gap-2 cursor-pointer text-[var(--text-muted)] text-[12px]">
                      <input type="checkbox" class="w-auto accent-[var(--accent-primary)]" bind:checked={matrixIds[p.id]} />
                      {p.name} <small>({p.id})</small>
                    </label>
                  {/each}
                </div>
                {#if matrixSummary.length > 0}
                  <table class="w-full border-collapse text-[12px]">
                    <thead><tr><th class="text-left px-2 py-1.5 border-b border-[var(--border-color)]">Profile</th><th class="text-left px-2 py-1.5 border-b border-[var(--border-color)]">Verdict</th><th class="text-left px-2 py-1.5 border-b border-[var(--border-color)]">Time</th><th class="text-left px-2 py-1.5 border-b border-[var(--border-color)]">Reason</th></tr></thead>
                    <tbody>
                      {#each matrixSummary as row, i (row.profile + '-' + i)}
                        <tr>
                          <td class="px-2 py-1.5 border-b border-[var(--border-color)]">{row.profile}</td>
                          <td class="px-2 py-1.5 border-b border-[var(--border-color)]"><span class="vbadge {row.verdict}">{row.verdict}</span></td>
                          <td class="px-2 py-1.5 border-b border-[var(--border-color)]">{row.durationSeconds != null ? `${row.durationSeconds}s` : "—"}</td>
                          <td class="px-2 py-1.5 border-b border-[var(--border-color)] text-[var(--text-muted)]">{row.reason ?? ""}</td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                {/if}
              </div>
            {:else if sideTab === "bisect"}
              <div class="grid gap-3.5">
                <div class="flex items-start gap-2.5">
                  <Crosshair size={16} class="text-[var(--accent-primary)] shrink-0 mt-0.5" />
                  <div class="min-w-0">
                    <strong class="text-[var(--text-primary)]">Find the crashing mod automatically</strong>
                    <p class="m-0 text-[12px] leading-relaxed text-[var(--text-muted)]">
                      Binary exclusion: TuffBox disables groups of suspect mods, launches the pack, and reads the
                      verdict from the game log — halving the group after every launch until the culprit is isolated
                      and verified. A snapshot is taken first; <em>Restore</em> re-enables everything at any time.
                    </p>
                  </div>
                </div>

                {#if !bisect}
                  <div class="flex items-center gap-3 flex-wrap">
                    <button
                      type="button"
                      class="primary-action"
                      onclick={startBisect}
                      disabled={!$projectPath || bisectBusy || launchBusy}
                      title="Start automatic isolation with the current suspect pool"
                    >
                      <Crosshair size={14} /> Start isolation
                    </button>
                    <label class="inline-flex items-center gap-2 cursor-pointer text-[12px] text-[var(--text-muted)]">
                      <input type="checkbox" class="w-auto accent-[var(--accent-primary)]" bind:checked={bisectAuto} />
                      Launch automatically
                    </label>
                  </div>
                  {#if $projectPath}
                    <p class="m-0 text-[12px] text-[var(--text-muted)]">
                      The suspect pool is built from recently changed and suspected mods (protected essentials like
                      loaders and APIs are never disabled). Everything else stays untouched.
                    </p>
                  {:else}
                    <p class="m-0 text-[12px] text-[var(--text-muted)]">Open a project to start.</p>
                  {/if}
                {:else}
                  <div class="flex flex-col gap-2.5 p-3 rounded-[var(--border-radius-md)] border border-[var(--border-color)] bg-[var(--bg-tertiary)]">
                    <div class="flex items-center justify-between gap-2 flex-wrap">
                      <span class="vbadge {bisectPhase === 'done' ? 'pass' : bisectPhase === 'failed' ? 'fail' : bisectRunInFlight || running ? 'started' : 'timedOut'}">
                        {bisectPhase === "done" ? "isolated" : bisectPhase === "failed" ? "stopped" : bisectRunInFlight || running ? "testing" : "paused"}
                      </span>
                      <span class="text-[12px] text-[var(--text-muted)]">
                        launch {bisectLaunches}/{MAX_AUTO_BISECT_LAUNCHES} · step {bisect.step}
                      </span>
                    </div>
                    <p class="m-0 text-[12.5px] leading-relaxed text-[var(--text-primary)]">{bisectStatusText}</p>
                    <div class="h-1.5 rounded-full overflow-hidden bg-[var(--bg-elevated)]" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(bisectProgress(bisect) * 100)}>
                      <div class="h-full rounded-full bg-[var(--accent-primary)] transition-width duration-300" style={`width: ${Math.round(bisectProgress(bisect) * 100)}%`}></div>
                    </div>
                    {#if bisect.defectives.length > 0}
                      <div class="flex items-center gap-2 flex-wrap">
                        <span class="text-[12px] font-bold uppercase tracking-wider text-[var(--text-muted)]">Culprit{bisect.defectives.length > 1 ? "s" : ""}:</span>
                        {#each bisect.defectives as id (id)}
                          <span class="px-2 py-0.5 rounded-full text-[12px] font-semibold border border-[color-mix(in_srgb,var(--accent-danger)_45%,transparent)] bg-[color-mix(in_srgb,var(--accent-danger)_12%,transparent)] text-[var(--text-primary)]">{id}</span>
                        {/each}
                      </div>
                    {/if}
                    <div class="flex items-center gap-2 flex-wrap">
                      <label class="inline-flex items-center gap-2 cursor-pointer text-[12px] text-[var(--text-muted)]">
                        <input type="checkbox" class="w-auto accent-[var(--accent-primary)]" bind:checked={bisectAuto} />
                        Auto-launch
                      </label>
                      <button
                        type="button"
                        class="secondary"
                        onclick={() => void reportBisectManually("healthy")}
                        disabled={bisectBusy || bisectRunInFlight || running}
                        title="Use when a verdict was inconclusive (timeout) or you stopped the game yourself"
                      >Mark healthy</button>
                      <button
                        type="button"
                        class="secondary"
                        onclick={() => void reportBisectManually("crash")}
                        disabled={bisectBusy || bisectRunInFlight || running}
                        title="Report the current group as crashing"
                      >Mark crashed</button>
                      <button
                        type="button"
                        class="danger"
                        onclick={stopBisectAndRestore}
                        disabled={bisectBusy || bisectRunInFlight || running}
                        title="Cancel isolation and restore the snapshot (all mods re-enabled)"
                      >Stop · restore all mods</button>
                    </div>
                    {#if bisectPhase === "done"}
                      <p class="m-0 text-[12px] text-[var(--text-muted)]">
                        Only the culprit mod(s) above are disabled now — test the pack, then keep them disabled or
                        remove them in Mods. Restore re-enables everything.
                      </p>
                    {/if}
                  </div>

                  {#if bisectLog.length > 0}
                    <div class="flex flex-col gap-2 max-h-56 overflow-y-auto p-3 rounded-[var(--border-radius-md)] border border-[var(--border-color)] bg-[var(--bg-primary)] font-[family-name:var(--font-mono)] text-[12px] leading-relaxed text-[var(--text-secondary)]">
                      {#each bisectLog as line, i (i)}
                        <span class="whitespace-pre-wrap">{line}</span>
                      {/each}
                    </div>
                  {/if}
                {/if}
              </div>
            {:else if sideTab === "history"}
              <div class="grid gap-3">
                {#if profiles.length > 0}
                  <div class="grid gap-2 [grid-template-columns:repeat(auto-fill,minmax(200px,1fr))]">
                    {#each profiles as profile (profile.id)}
                      <button
                        class="w-full flex flex-col items-start gap-2 border p-3 text-left transition-colors duration-150 cursor-pointer { selectedProfile === profile.id
                          ? "border-[color-mix(in_srgb,var(--accent-primary)_40%,transparent)] bg-[color-mix(in_srgb,var(--accent-primary)_8%,transparent)]"
                          : "border-[var(--border-color)] bg-[var(--bg-tertiary)] hover:border-[color-mix(in_srgb,var(--accent-primary)_40%,transparent)] hover:bg-[color-mix(in_srgb,var(--accent-primary)_8%,transparent)]" }"
                        onclick={() => (selectedProfile = profile.id)}
                      >
                        <strong class="text-[var(--text-primary)]">{profile.name}</strong>
                        <span class="text-[var(--text-muted)]">{profile.id} · {profile.side}</span>
                        <small class="text-[var(--text-muted)]">{profile.memoryMb ?? 4096} MB · {profile.jvmArgs.length} JVM args</small>
                      </button>
                    {/each}
                  </div>
                {:else}
                  <div class="text-[var(--text-muted)]">No profiles found.</div>
                {/if}

                {#if launchStats}
                  <div class="p-3 border border-[var(--border-color)] rounded-[var(--border-radius-md)] bg-[var(--bg-tertiary)] grid gap-2">
                    <h3 class="text-[var(--text-secondary)] text-[12px] m-0 uppercase tracking-[0.04em]">Launch stats</h3>
                    <div class="grid grid-cols-3 gap-2 mb-1">
                      <div class="stat-tile">
                        <span class="text-[12px] uppercase tracking-wide text-[var(--text-muted)]">Launches</span>
                        <strong class="text-[15px] text-[var(--text-primary)] tabular-nums">{ launchStats.totalLaunches }</strong>
                      </div>
                      <div class="stat-tile" class:bad={launchStats.totalCrashes > 0} title={launchStats.totalCrashes > 0 ? "Crashes recorded for this pack — see Diagnose" : "No crashes recorded"}>
                        <span class="text-[12px] uppercase tracking-wide text-[var(--text-muted)]">Crashes</span>
                        <strong class="text-[15px] text-[var(--text-primary)] tabular-nums">{ launchStats.totalCrashes }</strong>
                      </div>
                      <div class="stat-tile" class:warn={passRate < 80 && runs.length > 0} title={passRate < 80 && runs.length > 0 ? "Fewer than 8 in 10 runs reach a healthy state" : "Share of runs that reached a healthy state"}>
                        <span class="text-[12px] uppercase tracking-wide text-[var(--text-muted)]">Pass rate</span>
                        <strong class="text-[15px] text-[var(--text-primary)] tabular-nums">{ passRate }%</strong>
                      </div>
                    </div>
                    {#if avgRunSeconds != null}
                      <div class="flex justify-between items-center text-[12px]"><span class="text-[var(--text-muted)]">Avg run duration</span><span class="text-[var(--text-secondary)] tabular-nums">{ avgRunSeconds }s</span></div>
                    {/if}
                    {#if launchStats.lastLaunch}<div class="flex justify-between items-center text-[12px]"><span class="text-[var(--text-muted)]">Last launch</span><span class="text-[12px] text-[var(--text-muted)]">{launchStats.lastLaunch}</span></div>{/if}
                  </div>
                {/if}

                <div class="flex items-center justify-between gap-2 flex-wrap">
                  <h2 class="m-0 text-[15px] text-[var(--text-primary)]">Run history</h2>
                  <div class="flex gap-2 flex-wrap">
                    <button class="ghost mini" class:active={historyFilter === "all"} onclick={() => (historyFilter = "all")}>All</button>
                    <button class="ghost mini" class:active={historyFilter === "pass"} onclick={() => (historyFilter = "pass")}>Pass</button>
                    <button class="ghost mini" class:active={historyFilter === "fail"} onclick={() => (historyFilter = "fail")}>Fail</button>
                    <button class="ghost mini" class:active={historyFilter === "crashed"} onclick={() => (historyFilter = "crashed")}>Crashed</button>
                  </div>
                </div>
                {#if filteredRuns.length === 0}
                  <div class="text-[var(--text-muted)]">No test runs recorded yet.</div>
                {:else}
                  <div class="grid gap-2">
                    {#each filteredRuns.slice(0, 12) as run (run.id)}
                      <div class="grid gap-[3px] p-2.5 rounded-[var(--border-radius-md)] bg-[var(--bg-tertiary)] border {runBorderClass(verdictClass(run.status))}">
                        <div class="flex justify-between items-center gap-2">
                          <strong class="text-[var(--text-primary)]">{run.profile}</strong>
                          <span class="vbadge {verdictClass(run.status)}">{verdictLabel(run.status)}</span>
                        </div>
                        <span class="text-[var(--text-muted)] text-[12px]">{formatRunTime(run.startedAt)}</span>
                        <small class="text-[var(--text-muted)] text-[12px]">
                          {run.durationSeconds != null ? `${run.durationSeconds}s` : "—"}
                          {#if run.peakProcMb}
                            · <span
                              class="text-[var(--text-secondary)] tabular-nums"
                              title={run.recommendedRamGb
                                ? `Players need ${run.recommendedRamGb} GB RAM`
                                : undefined}
                            >{gb1(run.peakProcMb)} GB peak</span>
                          {/if}
                          {#if run.verdictReason} · {run.verdictReason}{/if}
                        </small>
                        <div class="flex gap-2 flex-wrap mt-1">
                          <button class="ghost mini" onclick={() => openRunLogs(run)}>Open logs</button>
                          {#if !capturedRunIds[run.id]}
                            <button class="ghost mini" onclick={() => captureRunLogs(run)}>Capture</button>
                          {/if}
                          <button class="ghost mini" onclick={openDiagnose} title="Open Diagnose stage">
                            <Stethoscope size={12} /> Diagnose
                          </button>
                          <button class="ghost mini" onclick={() => reRun(run)} disabled={running || matrixRunning}>Re-run</button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  /* Layout comes from Tailwind utilities; scoped styles only cover what
     utilities cannot express: this view's tweaks to the shared button skins,
     the verdict badges, the launch controls and the two shell surfaces. */

  .mini { padding: 5px 9px; font-size: 12px; justify-self: start; }

  .danger {
    background: color-mix(in srgb, var(--accent-danger) 18%, transparent);
    border-color: color-mix(in srgb, var(--accent-danger) 40%, transparent);
    color: color-mix(in srgb, var(--accent-danger) 62%, var(--text-primary));
  }
  .danger:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-danger) 28%, transparent);
  }
  /* Disabled destructive actions must not read as live controls: without this
     "Kill" stayed fully saturated red while nothing was running. */
  .danger:disabled,
  .kill-btn:disabled,
  .run-main:disabled {
    opacity: 0.45;
    cursor: default;
    box-shadow: none;
    filter: grayscale(0.35);
  }

  /* Form fields: labeled column so selects align with the buttons next to
     them instead of jumping in height. */
  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }
  .field select {
    width: 100%;
    min-height: 36px;
    padding: 7px 30px 7px 12px;
    font-size: 13px;
    line-height: 1.2;
    appearance: none;
    background-image: linear-gradient(45deg, transparent 50%, var(--text-muted) 50%),
      linear-gradient(135deg, var(--text-muted) 50%, transparent 50%);
    background-position: calc(100% - 16px) 55%, calc(100% - 10px) 55%;
    background-size: 5px 5px;
    background-repeat: no-repeat;
  }
  .field select:focus {
    border-color: var(--accent-primary);
  }

  /* Stat tiles: the number always stays on the primary text colour so it is
     readable in every theme; a bad/warn state is carried by the tile tint.
     (Colouring the digits with --accent-warning/danger drops to ~2.2:1 on the
     win95 silver panel.) */
  .stat-tile {
    display: grid;
    gap: 8px;
    padding: 8px;
    border-radius: var(--border-radius-sm);
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
  }
  .stat-tile.bad {
    border-color: color-mix(in srgb, var(--accent-danger) 45%, var(--border-color));
    background: color-mix(in srgb, var(--accent-danger) 10%, var(--bg-secondary));
  }
  .stat-tile.warn {
    border-color: color-mix(in srgb, var(--accent-warning) 45%, var(--border-color));
    background: color-mix(in srgb, var(--accent-warning) 10%, var(--bg-secondary));
  }

  .side-tab-dot {
    width: 6px;
    height: 6px;
    border-radius: 999px;
  }
  .side-tab-dot.ok {
    background: var(--accent-primary);
  }
  .side-tab-dot.bad {
    background: var(--accent-danger);
  }

  /* Status chip: quiet themed pill, no gray Ore button skin. */
  .status-chip {
    border: 1px solid var(--border-color);
  }
  .status-chip.idle {
    color: var(--text-secondary);
    background: color-mix(in srgb, var(--accent-primary) 8%, var(--bg-secondary));
    border-color: color-mix(in srgb, var(--accent-primary) 22%, var(--border-color));
  }
  .status-chip.pass {
    color: color-mix(in srgb, var(--accent-primary) 62%, var(--text-primary));
    background: color-mix(in srgb, var(--accent-primary) 14%, transparent);
    border-color: color-mix(in srgb, var(--accent-primary) 40%, transparent);
  }
  .status-chip.fail {
    color: color-mix(in srgb, var(--accent-danger) 62%, var(--text-primary));
    background: color-mix(in srgb, var(--accent-danger) 14%, transparent);
    border-color: color-mix(in srgb, var(--accent-danger) 40%, transparent);
  }
  .status-chip.running {
    color: color-mix(in srgb, var(--accent-warning) 62%, var(--text-primary));
    background: color-mix(in srgb, var(--accent-warning) 14%, transparent);
    border-color: color-mix(in srgb, var(--accent-warning) 40%, transparent);
  }

  /* History filter chips: accent when active, quiet otherwise. */
  .ghost.mini {
    border-radius: 999px;
    border: 1px solid transparent;
  }
  .ghost.mini.active {
    color: color-mix(in srgb, var(--accent-primary) 62%, var(--text-primary));
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }

  .vbadge {
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
  }
  .vbadge.pass,
  .vbadge.finished {
    color: color-mix(in srgb, var(--accent-primary) 62%, var(--text-primary));
    border-color: color-mix(in srgb, var(--accent-primary) 40%, transparent);
  }
  .vbadge.fail,
  .vbadge.failed {
    color: color-mix(in srgb, var(--accent-danger) 62%, var(--text-primary));
    border-color: color-mix(in srgb, var(--accent-danger) 40%, transparent);
  }
  .vbadge.crashed {
    color: color-mix(in srgb, var(--accent-danger) 62%, var(--text-primary));
    background: color-mix(in srgb, var(--accent-danger) 12%, transparent);
  }
  .vbadge.timedOut {
    color: color-mix(in srgb, var(--accent-warning) 62%, var(--text-primary));
    border-color: color-mix(in srgb, var(--accent-warning) 40%, transparent);
  }
  .vbadge.started,
  .vbadge.running {
    color: var(--accent-secondary);
  }
  .vbadge.skipped {
    color: var(--text-muted);
  }

  .val-badge {
    font-size: 12px;
    font-weight: 700;
    padding: 4px 8px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
  }
  .val-badge.ok {
    color: color-mix(in srgb, var(--accent-primary) 62%, var(--text-primary));
    border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
  }
  .val-badge.bad {
    color: color-mix(in srgb, var(--accent-danger) 62%, var(--text-primary));
    border-color: color-mix(in srgb, var(--accent-danger) 35%, transparent);
    background: color-mix(in srgb, var(--accent-danger) 8%, transparent);
  }

  /* ── Glassmorphism shell for panels ──────────────────────────── */
  .glass-card {
    background: color-mix(in srgb, var(--bg-secondary) 50%, transparent);
    -webkit-backdrop-filter: blur(20px) saturate(140%);
    backdrop-filter: blur(20px) saturate(140%);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
    box-shadow:
      inset 0 1px 0 color-mix(in srgb, var(--text-muted) 10%, transparent),
      0 14px 44px rgba(0, 0, 0, 0.16);
  }

  .launch-bar {
    padding: 14px 16px;
  }
  .launch-note {
    color: var(--text-muted);
    line-height: 1.4;
  }

  .run-main {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    align-self: flex-end;
    min-height: 36px;
    padding: 0 18px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--accent-primary);
    border-bottom-color: color-mix(in srgb, var(--accent-primary) 60%, #000);
    border-bottom-width: 3px;
    background: var(--accent-primary);
    color: var(--on-accent);
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    box-shadow: 0 6px 18px color-mix(in srgb, var(--accent-primary) 30%, transparent);
  }
  .run-main:hover:not(:disabled) {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
    border-bottom-color: color-mix(in srgb, var(--accent-primary) 50%, #000);
  }
  .run-main:active:not(:disabled) {
    background: color-mix(in srgb, var(--accent-primary) 85%, #000);
    border-bottom-width: 1px;
    transform: translateY(2px);
  }
  .run-main:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .run-main.run {
    background: color-mix(in srgb, var(--accent-primary) 70%, #000);
    border-color: color-mix(in srgb, var(--accent-primary) 70%, #000);
  }

  .kill-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 36px;
    align-self: flex-end;
    padding: 0 16px;
    border-radius: var(--border-radius-md);
    border: 1px solid color-mix(in srgb, var(--accent-danger) 40%, transparent);
    border-bottom-color: color-mix(in srgb, var(--accent-danger) 70%, #000);
    border-bottom-width: 3px;
    background: color-mix(in srgb, var(--accent-danger) 16%, transparent);
    color: color-mix(in srgb, var(--accent-danger) 62%, var(--text-primary));
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: background var(--motion-fast, 160ms) ease;
  }
  .kill-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-danger) 28%, transparent);
  }

  .chip-pid {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
    font-weight: 600;
    padding: 2px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text-primary) 12%, transparent);
    color: var(--text-secondary);
    font-family: var(--font-mono, ui-monospace, monospace);
  }

  /* Segmented tabs (right panel header). */
  .seg-tabs {
    display: inline-flex;
    gap: 8px;
    padding: 3px;
    border-radius: var(--border-radius-md);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
  }
  .seg-tab {
    border: none;
    background: transparent;
    color: var(--text-muted);
    padding: 7px 13px;
    border-radius: var(--border-radius-sm);
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    transition: background var(--motion-fast, 160ms) ease, color var(--motion-fast, 160ms) ease;
  }
  /* Accent-on-accent-tint text (segmented tabs, verdict badges, filter chips)
     sits around 3:1 in the light themes. Mixing the accent toward the primary
     text colour keeps the hue readable: darker on light panels, lighter on dark
     ones, in every theme. */
  .seg-tab.active {
    background: color-mix(in srgb, var(--accent-primary) 16%, transparent);
    color: color-mix(in srgb, var(--accent-primary) 62%, var(--text-primary));
    box-shadow: 0 0 10px color-mix(in srgb, var(--accent-primary) 20%, transparent);
  }
  .seg-tab.active :global(svg) {
    color: inherit;
  }

  .mini-spinner {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid color-mix(in srgb, var(--on-accent) 30%, transparent);
    border-top-color: var(--on-accent);
    animation: spin 700ms linear infinite;
    flex-shrink: 0;
  }

  :global(.spin) { animation: spin 900ms linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
