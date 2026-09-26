<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { killWithFeedback, launchWithFeedback, launchingPath } from "../lib/launch";
  import { onMount, tick } from "svelte";
  import {
    Stethoscope,
    Play,
    Square,
    FolderOpen,
    RefreshCw,
    AlertCircle,
    AlertTriangle,
    Info,
    History,
    Trash2,
    Database,
    ArrowDownToLine,
    FileText,
    Bug,
    Terminal,
    Shield,
  } from "@lucide/svelte";
  import {
    diagnoseFocus,
    diagnoseFocusPaths,
    historyFocusEventId,
    ideStageRequest,
    projectPath,
    pushWorkTrail,
    ideNeedsHealth,
    requestIdeIssuesRefresh,
    historyFocusFingerprintKey,
    launchSessions,
    isProjectLaunching,
    isProjectRunning,
    openLaunchLog,
    runningInstances,
    changeOptionKey,
    getFixPreference,
    setFixPreference,
    type DiagnoseFocus,
  } from "../lib/store";
  import { shareCrashLogWithFeedback } from "../lib/mclogs";
  import {
    DIAGNOSE_BUSY_CAP_MS,
    UnifiedBusyTracker,
    shouldAutoScheduleAi,
    shouldTripWatchdog,
  } from "../lib/diagnoseAutoSchedule";
  import EmptyState from "./EmptyState.svelte";
  import AiConnectionModal from "./AiConnectionModal.svelte";
  import DiagnoseTriagePanels from "./diagnostics/DiagnoseTriagePanels.svelte";
  import DiagnosePlanReviewModal from "./diagnostics/DiagnosePlanReviewModal.svelte";
  import DiagnoseLogViewer from "./diagnostics/DiagnoseLogViewer.svelte";
  import DiagnoseAnalysisTabs from "./diagnostics/DiagnoseAnalysisTabs.svelte";
  import DiagnoseVerdictHero from "./diagnostics/DiagnoseVerdictHero.svelte";
  import DiagnoseProblemsList from "./diagnostics/DiagnoseProblemsList.svelte";
  import DiagnoseStatusBar from "./diagnostics/DiagnoseStatusBar.svelte";
  import DiagnoseAdvanced from "./diagnostics/DiagnoseAdvanced.svelte";
  import { formatCascadeLabel } from "./diagnostics/cascadeLabel";
  import {
    buildUnifiedProblems,
    collectFixAllActions,
    hasBlockingProblems,
    type FixAction as ProblemFixAction,
    type Problem,
  } from "./diagnostics/problemModel";
  import { open as openShell } from "@tauri-apps/plugin-shell";
  import {
    fetchCrashFixBanner,
    type SoftVerifyOutcome,
  } from "../lib/softVerify";

  type Diagnostic = {
    severity: string;
    code: string;
    message: string;
    relatedNodes: any[];
  };

  type Snapshot = {
    id: string;
    name: string;
    createdAt: string;
    reason: string;
    changedFiles?: string[];
  };

  type Evidence = {
    source: string;
    lineNumber: number;
    kind: string;
    text: string;
  };

  type SuspectedMod = {
    id: string;
    name: string;
    version?: string | null;
    fileName?: string | null;
    knownInManifest: boolean;
    confidence: number;
    evidence: Evidence[];
    authors?: string[];
    blameRole?: "primary" | "secondary" | "related" | string;
    matchSources?: string[];
  };

  type CrashReportSummary = {
    id: string;
    name: string;
    path: string;
    size: number;
    modified?: number | null;
  };

  type CrashReportAnalysis = {
    summary: CrashReportSummary;
    content: string;
    sections?: { title: string; startLine: number; endLine: number; preview: string }[];
    modEntries?: { id: string; name?: string | null; version?: string | null; raw: string }[];
    signals: Evidence[];
    suspectedMods: SuspectedMod[];
  };

  type LatestLogAnalysis = {
    path: string;
    exists: boolean;
    tail: string;
    signals: Evidence[];
    suspectedMods: SuspectedMod[];
    hints: DiagnosisHint[];
  };

  type DiagnosisHint = {
    id: string;
    title: string;
    severity: string;
    detail: string;
    steps: string[];
    relatedMods: string[];
    fix: FixAction | null;
    fixes: FixAction[];
  };

  type FixAction = {
    kind: string;
    label: string;
    modId: string | null;
  };

  type CrashDiagnosis = {
    reports: CrashReportSummary[];
    selectedReport?: CrashReportAnalysis | null;
    latestLog: LatestLogAnalysis;
    launcherLog: LatestLogAnalysis;
    suspectedMods: SuspectedMod[];
    hints: DiagnosisHint[];
    recentSnapshots: Snapshot[];
    graphDiagnostics: Diagnostic[];
    fixPlan: any;
    analysisSource?: string;
    crashReportStale?: boolean;
    sessionHealthy?: boolean;
    hsErrLogs?: {
      id: string;
      name: string;
      kind: string;
      problematicFrame?: string | null;
      preview: string;
    }[];
    worldCoords?: { x: number; y: number; z: number; label: string } | null;
    memoryHint?: string | null;
  };

  let diagnosis = $state<CrashDiagnosis | null>(null);
  /** Aggregated HealthReport (diagnostics + crash + export blockers + missing). */
  let health = $state<{
    errorCount?: number;
    warningCount?: number;
    hasCrash?: boolean;
    crashReports?: string[];
    exportBlockers?: { code: string; message: string; target?: string | null }[];
    missingCount?: number;
    hashMismatchCount?: number;
  } | null>(null);
  let healthLoading = $state(false);
  let selectedReportId = $state("");
  let preferLatestLog = $state(true);
  let preferLauncherLog = $state(false);
  /// Sentinel: force latest.log analysis (never auto-pick a crash file).
  const LATEST_LOG_SOURCE = "__latest_log__";
  const LAUNCHER_LOG_SOURCE = "__launcher_log__";
  let analysisBusy = $state(false);
  // Monotonic client-side generation. Backend calls cannot always be
  // cancelled (Ollama/network), so late results from an older refresh must
  // never overwrite the currently selected report.
  let analysisGeneration = 0;
  // Ownership of `analysisBusy` — see UnifiedBusyTracker. The generation
  // counter is bumped by MANUAL AI runs too, which used to strand the flag.
  const unifiedBusy = new UnifiedBusyTracker();
  let analysisKickoff: ReturnType<typeof setTimeout> | undefined;
  let diagnoseTimings = $state<Record<string, { elapsedMs: number; cacheHit: boolean }>>({});
  const isCurrentAnalysis = (generation: number) => generation === analysisGeneration;

  // Coalesce load-triggered enrichments into one post-paint job. Source/path
  // changes can otherwise schedule multiple Crash Assistant + AI cascades
  // before the first result has even reached the UI.
  function scheduleUnifiedAnalysis(includeAi = false) {
    if (analysisKickoff) clearTimeout(analysisKickoff);
    analysisKickoff = setTimeout(() => {
      analysisKickoff = undefined;
      void runUnifiedAnalysis({ includeAi });
    }, 32);
  }
  /** Separate dedupe keys let base rules stay cheap while AI remains opt-in. */
  let lastRulesSource = $state<string | null>(null);
  let lastAiSource = $state<string | null>(null);
  /** Main Health canvas tab. */
  let mainTab = $state<"problems" | "evidence" | "ai" | "advanced">("problems");
  /** After a fix: ask user to verify with Test launch. */
  let verifyPrompt = $state(false);

  $effect(() => {
    if (verifyPrompt) {
      pushWorkTrail("Fix applied — verify with a Test launch", [
        { id: "test", label: "Test launch", kind: "play" },
        { id: "dismiss", label: "Dismiss", kind: "dismiss" },
      ]);
    }
  });
  let applyingProblemId = $state<string | null>(null);
  let fixAllBusy = $state(false);
  let appliedProblemIds = $state<Set<string>>(new Set());
  /** Detail panel under the verdict: rules findings vs AI explanation. */
  let aiSoftError = $state<string | null>(null);
  let sharingLog = $state(false);
  let loading = $state(false);
  let planning = $state(false);
  let applying = $state(false);
  let applyingHintId = $state<string | null>(null);
  const launchSession = $derived($launchSessions[$projectPath ?? ""] ?? null);
  const launching = $derived(isProjectLaunching($projectPath, $launchSessions));
  const projectRunning = $derived(isProjectRunning($projectPath, $runningInstances));
  // launching is derived from the shared launch store so the button stays in the
  // launching state until the game is running or the run ends.
  const launchingFromPath = $derived($launchingPath === $projectPath);
  let fixingIdx = $state<number | null>(null);
  let disablingModId = $state<string | null>(null);
  let error = $state<string | null>(null);
  let message = $state<string | null>(null);
  let plan = $state<any | null>(null);
  /** Radio choice for plans that carry `options` (conflict resolutions). */
  let selectedFixOption = $state<number | null>(null);
  let lastLoadedPath = $state<string | null>(null);
  let advancedLoadedPath: string | null = null;
  // Prevent the project-path effect, onMount and the open-diagnostics event
  // from starting identical IPC pipelines at the same time.
  let activeLoadPath: string | null = null;

  function onSourceChange(e: Event) {
    const el = e.currentTarget;
    if (!(el instanceof HTMLSelectElement)) return;
    if (el.value === LATEST_LOG_SOURCE) chooseLatestLog();
    else if (el.value === LAUNCHER_LOG_SOURCE) chooseLauncherLog();
    else if (el.value.startsWith("session/")) chooseArchivedSession(el.value);
    else chooseReport(el.value);
  }

  // Diagnose can appear to hang forever when an IPC never settles (stuck
  // Ollama/endpoint, p2p swarm lookup). Watchdog: after a hard cap, clear the
  // busy flags so the Health view stops spinning and tells the user what was stuck.
  let diagnoseWatch: ReturnType<typeof setInterval> | undefined;
  let diagnoseBusySince = 0;
  // Set when the watchdog force-stops a stuck run. Blocks the AI tab's
  // AUTOMATIC re-scheduling until the user explicitly retries (Re-analyze /
  // Retry AI / Refresh / source switch). Without it, the watchdog clearing
  // the busy flags + the AI-tab auto-effect re-arming formed an infinite
  // "Analyzing…" loop: every reset spawned another backend cascade that
  // piled onto the still-running one.
  let diagnoseWatchdogTripped = $state(false);

  function syncDiagnoseWatchdog() {
    const busy = loading || analysisBusy || crashLoading || aiLoading || healthLoading;
    if (!busy) {
      if (diagnoseWatch) {
        clearInterval(diagnoseWatch);
        diagnoseWatch = undefined;
      }
      diagnoseBusySince = 0;
      return;
    }
    if (!diagnoseBusySince) diagnoseBusySince = Date.now();
    if (diagnoseWatch) return;
    diagnoseWatch = setInterval(() => {
      if (
        !shouldTripWatchdog({
          busySinceMs: diagnoseBusySince,
          nowMs: Date.now(),
          capMs: DIAGNOSE_BUSY_CAP_MS,
        })
      )
        return;
      const stage =
        cascadeLiveStage ||
        (aiLoading
          ? "AI analysis (Ollama / endpoint)"
          : crashLoading
            ? "crash rules"
            : loading
              ? "reading logs"
              : healthLoading
                ? "health report"
                : "pack scan");
      analysisBusy = false;
      crashLoading = false;
      aiLoading = false;
      loading = false;
      healthLoading = false;
      cascadeLiveStage = null;
      // Break the auto-restart loop: block the AI tab's automatic
      // re-scheduling until the user explicitly retries.
      diagnoseWatchdogTripped = true;
      // The backend cascade may still be running (no client-side cancel).
      // Forget the source stamp so a refresh re-runs analysis instead of
      // trusting state from a run whose results will land out of order.
      lastRulesSource = null;
      lastAiSource = null;
      clearInterval(diagnoseWatch);
      diagnoseWatch = undefined;
      diagnoseBusySince = 0;
      error = `Diagnose stayed in "${stage}" for ${Math.round(DIAGNOSE_BUSY_CAP_MS / 1000)}s and was stopped. Check Settings → AI / Ollama, then Refresh.`;
    }, 500);
  }

  $effect(() => {
    void loading;
    void analysisBusy;
    void crashLoading;
    void aiLoading;
    void healthLoading;
    void diagnoseWatchdogTripped;
    syncDiagnoseWatchdog();
    return () => {
      if (diagnoseWatch) {
        clearInterval(diagnoseWatch);
        diagnoseWatch = undefined;
      }
      // Never let a coalesced analysis kickoff fire after destroy.
      if (analysisKickoff) {
        clearTimeout(analysisKickoff);
        analysisKickoff = undefined;
      }
    };
  });

  async function chooseArchivedSession(reportId: string) {
    preferLatestLog = false;
    preferLauncherLog = false;
    selectedReportId = reportId;
    await load(true);
  }

  async function shareCurrentLog() {
    if (!$projectPath || sharingLog) return;
    sharingLog = true;
    try {
      const name = selectedReport?.summary?.name;
      await shareCrashLogWithFeedback($projectPath, preferLatestLog ? "latest.log" : name ?? null);
    } finally {
      sharingLog = false;
    }
  }

  async function loadHealth() {
    if (!$projectPath) return;
    healthLoading = true;
    try {
      health = await invoke("get_health_report", { path: $projectPath });
    } catch {
      health = null;
    } finally {
      healthLoading = false;
    }
  }

  async function load(force = false) {
    if (!$projectPath) return;
    // Explicit reload: re-arm automatic AI scheduling after a watchdog stop.
    if (force) diagnoseWatchdogTripped = false;
    const requestedPath = $projectPath;
    if (activeLoadPath === requestedPath) return;
    if (!force && lastLoadedPath === requestedPath && diagnosis) return;
    activeLoadPath = requestedPath;
    loading = true;
    error = null;
    const requestedLatest = preferLatestLog;
    try {
      const reportId = preferLatestLog
        ? LATEST_LOG_SOURCE
        : preferLauncherLog
          ? LAUNCHER_LOG_SOURCE
          : selectedReportId || null;
      const data: CrashDiagnosis = await invoke("get_crash_diagnosis", {
        path: requestedPath,
        reportId,
      });
      diagnosis = data;
      if (requestedLatest || data.analysisSource === "latest_log") {
        preferLatestLog = true;
        preferLauncherLog = false;
        selectedReportId = "";
      } else if (data.analysisSource === "launcher_log") {
        preferLatestLog = false;
        preferLauncherLog = true;
        selectedReportId = "";
      } else {
        preferLatestLog = false;
        preferLauncherLog = false;
        selectedReportId = data.selectedReport?.summary.id ?? selectedReportId;
      }
      plan = data.fixPlan ?? null;
      preselectFixOption();
      void loadHealth();
      detectWrongLoaderMods();
      detectDuplicateModJars();
      if (data.sessionHealthy && preferLatestLog) {
        crashFindings = [];
        crashMcreator = [];
        crashClassFinder = [];
        aiAnalysis = null;
        aiContext = null;
        aiSoftError = null;
        ideNeedsHealth.set(false);
        void invoke("confirm_crash_resolution_from_diagnose", { path: $projectPath }).catch(() => {});
      } else {
        if (!data.sessionHealthy) ideNeedsHealth.set(true);
        scheduleUnifiedAnalysis();
      }
    } catch (e) {
      error = String(e);
    } finally {
      if (activeLoadPath === requestedPath) {
        activeLoadPath = null;
        loading = false;
      }
    }
  }

  function onProjectPathChange(path: string | null) {
    if (!path || path === lastLoadedPath) return;
    lastLoadedPath = path;
    diagnoseTimings = {};
    preferLatestLog = true;
    selectedReportId = "";
    appliedProblemIds = new Set();
    lastRulesSource = null;
    lastAiSource = null; // new project → force a fresh unified analysis
    crashFindings = [];
    aiAnalysis = null;
    void load(true);
    void refreshSoftVerifyStatus(path);
  }

  async function chooseReport(reportId: string) {
    preferLatestLog = false;
    preferLauncherLog = false;
    selectedReportId = reportId;
    await load(true);
  }

  async function chooseLatestLog() {
    preferLatestLog = true;
    preferLauncherLog = false;
    selectedReportId = "";
    await load(true);
  }

  async function chooseLauncherLog() {
    preferLatestLog = false;
    preferLauncherLog = true;
    selectedReportId = "";
    await load(true);
  }

  function activeReportId(): string | null {
    if (preferLatestLog) return LATEST_LOG_SOURCE;
    if (preferLauncherLog) return LAUNCHER_LOG_SOURCE;
    return selectedReportId || null;
  }

  async function createFixPlan() {
    if (!$projectPath) return;
    planning = true;
    error = null;
    try {
      plan = await invoke("create_crash_fix_plan", {
        path: $projectPath,
        reportId: activeReportId(),
      });
    } catch (e) {
      error = String(e);
    } finally {
      planning = false;
    }
  }

  /// Per-diagnostic fix: install a missing mod dependency via Modrinth.
  async function fixMissingDependency(modId: string, idx: number) {
    if (!$projectPath) return;
    fixingIdx = idx;
    error = null;
    message = null;
    try {
      const summary = await applyFixBatchOrThrow([
        { kind: "installDependency", label: `Install ${modId}`, modId },
      ]);
      message = `${summary}. Reloading...`;
    } catch (e) {
      error = String(e);
    } finally {
      fixingIdx = null;
    }
  }

  /// Soft-disable a mod by renaming jar → `.jar.disabled` (keeps manifest entry).
  async function fixDisableMod(modId: string, idx: number | null = null) {
    if (!$projectPath) return;
    if (idx !== null) fixingIdx = idx;
    disablingModId = modId;
    error = null;
    message = null;
    try {
      const summary = await applyFixBatchOrThrow([
        { kind: "disableMod", label: `Disable ${modId}`, modId },
      ]);
      message = `${summary}. Rerun the Test profile to verify.`;
    } catch (e) {
      error = String(e);
    } finally {
      fixingIdx = null;
      disablingModId = null;
    }
  }

  async function fixEnableMod(modId: string) {
    if (!$projectPath) return;
    disablingModId = modId;
    error = null;
    message = null;
    try {
      const result: any = await invoke("enable_project_mod", {
        path: $projectPath,
        modId,
      });
      message = `Re-enabled ${result?.name ?? modId}.`;
      await load(true);
    } catch (e) {
      error = String(e);
    } finally {
      disablingModId = null;
    }
  }

  /// One-click update of the top suspect to the latest compatible version.
  async function applyTopSuspectUpdate() {
    if (!$projectPath || !topSuspect) return;
    fixingIdx = -1;
    error = null;
    message = null;
    try {
      const summary = await applyFixBatchOrThrow([
        { kind: "updateMod", label: `Update ${topSuspect.name}`, modId: topSuspect.id },
      ]);
      message = summary || `Updated ${topSuspect.name}`;
    } catch (e) {
      error = String(e);
    } finally {
      fixingIdx = null;
    }
  }

  /// Per-diagnostic fix: remove a conflicting mod from the project.
  async function fixRemoveMod(modId: string, idx: number) {
    if (!$projectPath) return;
    fixingIdx = idx;
    error = null;
    message = null;
    try {
      const summary = await applyFixBatchOrThrow([
        { kind: "removeMod", label: `Remove ${modId}`, modId },
      ]);
      message = `${summary}. Reloading...`;
    } catch (e) {
      error = String(e);
    } finally {
      fixingIdx = null;
    }
  }

  /// Per-diagnostic fix: keep newest jar for this mod id when disk duplicates exist.
  async function fixDeduplicate(idx: number) {
    fixingIdx = idx;
    const d = graphDiagnostics[idx];
    const fromMsg = (d?.message ?? "").match(/Duplicate mod node:\s*(.+)$/i)?.[1]?.trim();
    const group =
      duplicateJarGroups.find((g) => g.modId === fromMsg) ??
      (fromMsg
        ? duplicateJarGroups.find((g) => g.modId.toLowerCase() === fromMsg.toLowerCase())
        : undefined);
    try {
      if (group?.keepCandidate) {
        await keepOneDuplicateJar(group.modId, group.keepCandidate);
      } else {
        await detectDuplicateModJars();
        message = duplicateJarGroups.length
          ? "Duplicate jars listed under Conflicts & jars — pick which jar to keep."
          : "No duplicate jars on disk for this graph warning.";
      }
    } finally {
      fixingIdx = null;
    }
  }

  // --- Wrong-loader jar detection ---
  type WrongLoaderJar = {
    fileName: string;
    detectedLoader: string;
    projectLoader: string;
    recommendation: string;
    reason: string;
  };
  let wrongLoaderJars = $state<WrongLoaderJar[]>([]);
  let wrongLoaderLoading = $state(false);
  let wrongLoaderFixing = $state<string | null>(null);

  type DupJar = {
    fileName: string;
    modId: string;
    mtimeMs: number;
    size: number;
    inManifest: boolean;
  };
  type DupJarGroup = { modId: string; keepCandidate: string; jars: DupJar[] };
  let duplicateJarGroups = $state<DupJarGroup[]>([]);
  let duplicateJarLoading = $state(false);
  let duplicateJarFixing = $state<string | null>(null);

  // Ore generation scanner state
  let oreFindings = $state<any[]>([]);
  let oreLoading = $state(false);

  // Duplicate items / unification state
  let duplicateFindings = $state<any[]>([]);
  let duplicateLoading = $state(false);
  let unifyConfigResult = $state<any>(null);
  let unifyLoading = $state(false);

  // Crash Assistant state
  let crashLoading = $state(false);
  let crashFindings = $state<any[]>([]);
  let crashMcreator = $state<string[]>([]);
  let crashClassFinder = $state<any[]>([]);
  let analysisToolsOpen = $state(false);
  let classQuery = $state("");
  let classBusy = $state(false);
  let classResults = $state<{ className: string; modId: string; modName: string }[]>([]);
  let dependentResults = $state<{ className: string; modId: string; modName: string }[]>([]);
  let bisectMods = $state<string[]>([]);
  type GroupTestSession = {
    pool: string[];
    covering: string[];
    knownClean: string[];
    defectives: string[];
    testGroup: string[];
    phase: string | { verifyOne?: { index: number }; failed?: { reason: string } };
    snapshotId?: string | null;
    verified: boolean;
    step: number;
  };
  let groupTest = $state<GroupTestSession | null>(null);
  let groupTestBusy = $state(false);
  let groupTestAuto = $state(true);
  let supportBusy = $state(false);
  let importBusy = $state(false);
  let importUrl = $state("");

  async function runClassFinder(q: string) {
    if (!$projectPath || !q.trim()) return;
    classBusy = true;
    classResults = [];
    try {
      classResults = await invoke("find_class_in_mods", {
        path: $projectPath,
        className: q.trim(),
      });
      message = classResults.length
        ? `Found ${classResults.length} mod(s) containing «${q.trim()}»`
        : `No mod jar contains «${q.trim()}»`;
    } catch (e) {
      error = String(e);
    } finally {
      classBusy = false;
    }
  }

  async function runFindDependents(q: string) {
    if (!$projectPath || !q.trim()) return;
    classBusy = true;
    dependentResults = [];
    try {
      dependentResults = await invoke("find_dependents_on_class", {
        path: $projectPath,
        className: q.trim(),
      });
      message = dependentResults.length
        ? `${dependentResults.length} mod(s) depend on «${q.trim()}»`
        : `No dependents for «${q.trim()}»`;
    } catch (e) {
      error = String(e);
    } finally {
      classBusy = false;
    }
  }

  function toggleBisect(modId: string) {
    if (bisectMods.includes(modId)) bisectMods = bisectMods.filter((id) => id !== modId);
    else bisectMods = [...bisectMods, modId];
  }

  function groupTestPhaseKey(session: GroupTestSession | null): string {
    if (!session) return "";
    const p = session.phase;
    if (typeof p === "string") return p;
    if (p && typeof p === "object" && "failed" in p) return "failed";
    if (p && typeof p === "object" && "verifyOne" in p) return "verifyOne";
    return "";
  }

  function groupTestStatus(session: GroupTestSession | null): string {
    if (!session) return "";
    const key = groupTestPhaseKey(session);
    if (key === "needCovering") {
      return `Disable all ${session.covering.length} suspects, then test launch.`;
    }
    if (key === "testing") {
      return `Enable [${session.testGroup.join(", ")}]; keep the rest of the covering disabled.`;
    }
    if (key === "verifyAll") {
      return `Verify: disable only [${session.defectives.join(", ")}].`;
    }
    if (key === "verifyOne") {
      const idx =
        typeof session.phase === "object" && session.phase && "verifyOne" in session.phase
          ? session.phase.verifyOne?.index ?? 0
          : 0;
      return `Verify: re-enable ${session.defectives[idx] ?? "?"} (should crash).`;
    }
    if (key === "done") {
      return `Isolated: [${session.defectives.join(", ") || "none"}].`;
    }
    if (key === "failed") {
      const p = session.phase;
      return typeof p === "object" && p && "failed" in p
        ? (p.failed?.reason ?? "Group test failed")
        : "Group test failed";
    }
    return "";
  }

  const groupTestActive = $derived(
    !!groupTest && !["done", "failed"].includes(groupTestPhaseKey(groupTest)),
  );

  async function refreshGroupTest() {
    if (!$projectPath) return;
    try {
      groupTest = await invoke<GroupTestSession | null>("get_mod_group_test", {
        path: $projectPath,
      });
    } catch {
      groupTest = null;
    }
  }

  async function startGroupTest() {
    if (!$projectPath || groupTestBusy) return;
    groupTestBusy = true;
    error = null;
    try {
      const poolIds = [
        ...new Set([
          ...bisectMods,
          ...suspected.filter((s) => s.knownInManifest).map((s) => s.id),
        ]),
      ];
      groupTest = await invoke<GroupTestSession>("start_mod_group_test", {
        path: $projectPath,
        suspected: poolIds,
      });
      message = groupTestStatus(groupTest);
      if (groupTestAuto) await runTest();
    } catch (e) {
      error = String(e);
    } finally {
      groupTestBusy = false;
    }
  }

  async function reportGroupTest(outcome: "healthy" | "crash") {
    if (!$projectPath || !groupTest || groupTestBusy) return;
    groupTestBusy = true;
    error = null;
    try {
      groupTest = await invoke<GroupTestSession>("report_mod_group_test_outcome", {
        path: $projectPath,
        outcome,
      });
      message = groupTestStatus(groupTest);
      const key = groupTestPhaseKey(groupTest);
      if (groupTestAuto && key !== "done" && key !== "failed") {
        await runTest();
      }
    } catch (e) {
      error = String(e);
    } finally {
      groupTestBusy = false;
    }
  }

  async function cancelGroupTest() {
    if (!$projectPath) return;
    groupTestBusy = true;
    try {
      await invoke("cancel_mod_group_test", { path: $projectPath });
      groupTest = null;
      message = "Group test cancelled; snapshot restored.";
    } catch (e) {
      error = String(e);
    } finally {
      groupTestBusy = false;
    }
  }

  async function exportSupportPack() {
    if (!$projectPath || supportBusy) return;
    supportBusy = true;
    try {
      const findings = [
        ...(topFinding ? [`${topFinding.title}: ${topFinding.description}`] : []),
        ...crashFindings.slice(0, 8).map((f: any) => `${f.code}: ${f.title}`),
      ].join("\n");
      const events = recentPackEvents
        .slice(0, 8)
        .map((e) => `${e.ts} [${e.actor}] ${e.summary}`)
        .join("\n");
      const planJson = aiAnalysis
        ? JSON.stringify(
            {
              explanation: aiAnalysis.humanExplanation ?? aiAnalysis.human_explanation,
              actions: aiPlanActions(aiAnalysis),
            },
            null,
            2,
          )
        : null;
      const result: { path: string; fileCount: number } = await invoke(
        "export_diagnose_support_pack",
        {
          path: $projectPath,
          reportId: activeReportId(),
          findingsSummary: findings,
          recentEventsSummary: events,
          actionPlanJson: planJson,
        },
      );
      message = `Support pack ready (${result.fileCount} files): ${result.path}`;
      try {
        await navigator.clipboard.writeText(result.path);
        message += " — path copied.";
      } catch {
        /* ignore */
      }
    } catch (e) {
      error = String(e);
    } finally {
      supportBusy = false;
    }
  }

  async function importExternalCrashFile(file: File) {
    if (!$projectPath) return;
    importBusy = true;
    try {
      const content = await file.text();
      const id: string = await invoke("import_external_crash", {
        path: $projectPath,
        fileName: file.name,
        content,
      });
      message = `Imported player crash → ${id}`;
      preferLatestLog = false;
      preferLauncherLog = false;
      selectedReportId = id;
      await load(true);
      await runUnifiedAnalysis({ force: true });
    } catch (e) {
      error = String(e);
    } finally {
      importBusy = false;
    }
  }

  async function importFromMclogsUrl() {
    if (!$projectPath || !importUrl.trim()) return;
    importBusy = true;
    try {
      const url = importUrl.trim();
      if (!/^https:\/\/(api\.)?mclo\.gs\//i.test(url) && !/^https:\/\/mclo\.gs\//i.test(url)) {
        throw new Error("Only https://mclo.gs/… URLs are allowed");
      }
      const rawId = url.replace(/\/+$/, "").split("/").pop() || "";
      const fetchUrl = `https://api.mclo.gs/1/raw/${rawId}`;
      const res = await fetch(fetchUrl);
      if (!res.ok) throw new Error(`mclo.gs fetch failed (${res.status})`);
      const content = await res.text();
      const id: string = await invoke("import_external_crash", {
        path: $projectPath,
        fileName: `mclogs-${rawId}.txt`,
        content,
      });
      message = `Imported from mclo.gs → ${id}`;
      preferLatestLog = false;
      preferLauncherLog = false;
      selectedReportId = id;
      importUrl = "";
      await load(true);
      await runUnifiedAnalysis({ force: true });
    } catch (e) {
      error = String(e);
    } finally {
      importBusy = false;
    }
  }

  function onDropCrash(e: DragEvent) {
    e.preventDefault();
    const file = e.dataTransfer?.files?.[0];
    if (file) void importExternalCrashFile(file);
  }

  async function runCrashAssistant(generation?: number) {
    if (!$projectPath) return;
    const run = generation ?? ++analysisGeneration;
    crashLoading = true;
    try {
      const result: any = await invoke("run_crash_assistant_full", {
        path: $projectPath,
        reportId: activeReportId(),
      });
      if (!isCurrentAnalysis(run)) return;
      crashFindings = result.findings ?? [];
      crashMcreator = result.mcreatorMods ?? [];
      crashClassFinder = result.classFinderResults ?? [];
      // No enrich here: aiAnalysis still belongs to the PREVIOUS source (or is
      // null) at this point — runUnifiedAnalysis re-enriches after the fresh
      // runAiExplain completes, matching hints to the actual source.
    } catch (e) {
      error = String(e);
    } finally {
      if (isCurrentAnalysis(run)) crashLoading = false;
    }
  }

  /** Crash Assistant first, then AI — equal analysis cards.
   * Task #66: with force=false (tab open / reload) reuse the previous run's
   * results when the log source hasn't changed — re-running the full AI
   * cascade on every tab visit made the tab appear stuck in "Analyzing…". */
  async function runUnifiedAnalysis(opts: { force?: boolean; includeAi?: boolean } = {}) {
    if (!$projectPath || analysisBusy) return;
    // Manual Re-analyze: re-arm automatic AI scheduling after a watchdog stop.
    if (opts.force) diagnoseWatchdogTripped = false;
    const source = activeReportId();
    const includeAi = opts.includeAi ?? true;
    if (!opts.force && !includeAi && lastRulesSource === source) return;
    if (!opts.force && includeAi && lastAiSource === source && (aiAnalysis || aiSoftError)) return;
    lastRulesSource = source;
    const run = ++analysisGeneration;
    // Claim busy-flag ownership BEFORE any await: a manual "Retry AI" during
    // this run bumps analysisGeneration, and the old
    // `if (isCurrentAnalysis(run))` settle condition would then never match,
    // leaving analysisBusy stuck on until the watchdog tripped.
    const settle = unifiedBusy.begin();
    analysisBusy = true;
    aiSoftError = null;
    try {
      await runCrashAssistant(run);
      if (!isCurrentAnalysis(run)) return;
      if (!includeAi) return;
      try {
        await runAiExplain({ quiet: true, runId: run });
      } catch (aiErr) {
        if (!isCurrentAnalysis(run)) return;
        aiSoftError = String(aiErr);
        lastAiSource = source;
        console.warn("[Diagnose] AI explain soft-fail:", aiErr);
      }
      if (!isCurrentAnalysis(run)) return;
      // Single enrichment point, AFTER fresh AI results (fix #5).
      enrichCrashFindingsWithAi();
      lastAiSource = source;
    } finally {
      // Settle by unified-run ownership (not AI generation): the latest
      // unified run owns the flag even if a manual AI run bumped the
      // generation counter mid-flight.
      if (unifiedBusy.shouldSettle(settle)) analysisBusy = false;
    }
  }

  function enrichCrashFindingsWithAi() {
    if (!aiAnalysis || !crashFindings.length) return;
    const actions = aiPlanActions(aiAnalysis);
    const suspected = new Set(
      (aiAnalysis.suspectedMods ?? aiAnalysis.suspected_mods ?? []).map((m: string) =>
        String(m).toLowerCase(),
      ),
    );
    crashFindings = crashFindings.map((f: any) => {
      const fixIds = (f.fixes ?? [])
        .map((x: any) => String(x.modId ?? "").toLowerCase())
        .filter(Boolean);
      const blob = `${f.title ?? ""} ${f.description ?? ""} ${f.code ?? ""}`.toLowerCase();
      const matched = actions.find((a: any) => {
        const mid = String(a.modId ?? a.mod_id ?? "").toLowerCase();
        if (!mid) return false;
        return fixIds.includes(mid) || blob.includes(mid) || suspected.has(mid);
      });
      if (!matched && !fixIds.some((id: string) => suspected.has(id))) {
        return { ...f, aiAgree: false, aiHint: null };
      }
      return {
        ...f,
        aiAgree: true,
        aiHint:
          matched?.reason ??
          matched?.description ??
          (aiAnalysis.humanExplanation ?? aiAnalysis.human_explanation ?? null),
      };
    });
  }

  // AI context state
  let aiLoading = $state(false);
  let cascadeLiveStage = $state<string | null>(null);
  // Live backend stage label from the diagnose-progress event stream (task bus
  // mirror). Shown instead of the static "Loading crash diagnosis…" text.
  let liveDiagnoseStage = $state<string | null>(null);
  let aiContext = $state<any>(null);
  let aiPrompt = $state("");
  let aiShowPrompt = $state(false);
  let aiAnalysis = $state<any>(null);
  let aiFeedbackBusy = $state(false);
  let aiFeedbackMsg = $state<string | null>(null);
  let aiModalOpen = $state(false);
  let aiApplyBusy = $state(false);
  let pendingPlan = $state<any>(null);
  let pendingBusy = $state(false);
  let swarmEnabled = $state(false);
  let softVerifyStatus = $state<null | {
    kind: "pending" | "confirmed" | "rejected";
    detail?: string;
  }>(null);

  /** ActionPlan review modal (replaces window.confirm). */
  let planReviewOpen = $state(false);
  let planReviewSource = $state<"ai" | "network" | "fix-all">("ai");
  let planReviewRows = $state<{
    key: string;
    selected: boolean;
    op: string;
    modId: string | null;
    path: string | null;
    patchPreview: string | null;
    reason: string;
    risk: string;
    diffKind?: "add" | "remove" | "change" | "other";
    destructive?: boolean;
    problemId?: string;
    problemTitle?: string | null;
    raw: any;
  }[]>([]);
  let planReviewAcknowledged = $state(false);
  let planReviewNeedsAck = $state(false);
  let planReviewExplanation = $state("");
  let showApplyTrail = $state(false);
  const planReviewSelectedCount = $derived(planReviewRows.filter((r) => r.selected).length);
  const planReviewCanApply = $derived(
    planReviewSelectedCount > 0 && (!planReviewNeedsAck || planReviewAcknowledged),
  );

  // Author KB case form (pack author — crash + resolution)
  let authorOpen = $state(false);
  let authorBusy = $state(false);
  let authorMsg = $state<string | null>(null);
  let authorId = $state("");
  let authorSolution = $state("");
  let authorSymptoms = $state("");
  let authorSuspected = $state("");
  let authorNotes = $state("");
  let authorActionsJson = $state("[]");
  let authorFingerprint = $state<any>(null);
  let authorCases = $state<any[]>([]);
  let authorExportPreview = $state("");

  let recentPackEvents = $state<{
    id: string;
    ts: string;
    actor: string;
    op: string;
    summary: string;
    paths?: string[];
    meta?: { fingerprintKey?: string; episodeId?: string } | null;
  }[]>([]);

  async function loadRecentPackEvents() {
    if (!$projectPath) return;
    try {
      recentPackEvents = await invoke("list_recent_pack_events", {
        path: $projectPath,
        limit: 12,
      });
    } catch {
      recentPackEvents = [];
    }
  }

  /** Compact relative time for the recent-changes strip ("5m ago", "2h ago"). */
  function relTime(ts: string | undefined): string {
    if (!ts) return "";
    const then = Date.parse(ts.endsWith("Z") || ts.includes("T") ? ts : ts.replace(" ", "T") + "Z");
    if (!Number.isFinite(then)) return ts.slice(11, 16);
    const mins = Math.max(0, Math.round((Date.now() - then) / 60000));
    if (mins < 1) return "now";
    if (mins < 60) return `${mins}m ago`;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return `${hours}h ago`;
    return `${Math.floor(hours / 24)}d ago`;
  }

  function openHistoryEvent(eventId: string, fingerprintKey?: string | null) {
    historyFocusEventId.set(eventId);
    if (fingerprintKey) historyFocusFingerprintKey.set(fingerprintKey);
    ideStageRequest.set("history");
  }

  let lastRecentPackPath = $state<string | null>(null);
  $effect(() => {
    if ($projectPath && $projectPath !== lastRecentPackPath) {
      lastRecentPackPath = $projectPath;
      void loadRecentPackEvents();
    }
  });
  $effect(() => {
    const focus = $diagnoseFocus;
    if (!focus) return;
    diagnoseFocus.set(null);
    void applyHistoryFocus(focus);
  });

  $effect(() => {
    if ($diagnoseFocusPaths?.length) {
      const paths = $diagnoseFocusPaths;
      diagnoseFocusPaths.set(null);
      void applyHistoryFocus({ paths });
    }
  });

  async function applyHistoryFocus(focus: DiagnoseFocus) {
    const paths = focus.paths?.filter(Boolean) ?? [];
    const bits = [
      paths.length ? `paths ${paths.join(", ")}` : null,
      focus.fingerprintKey ? `fp ${focus.fingerprintKey.slice(0, 24)}` : null,
      focus.logPath ? `log ${focus.logPath}` : null,
    ].filter(Boolean);
    message = bits.length ? `Focus from History: ${bits.join(" · ")}` : "Focus from History";
    if (!diagnosis && $projectPath) await load(true);
    const d = diagnosis;
    if (!d) return;

    if (focus.logPath) {
      const lp = focus.logPath.toLowerCase().replace(/\\/g, "/");
      const matchReport = d.reports.find((r) => {
        const path = (r.path || "").toLowerCase().replace(/\\/g, "/");
        const name = r.name.toLowerCase();
        return path.includes(lp) || lp.includes(path) || lp.endsWith(name) || (name && lp.includes(name));
      });
      if (matchReport) {
        await chooseReport(matchReport.id);
        setTimeout(() => void openEvidence(), 250);
        return;
      }
      if (lp.includes("launcher")) {
        await chooseLauncherLog();
        setTimeout(() => void openEvidence(), 250);
        return;
      }
      if (lp.includes("latest.log") || lp.includes("/logs/")) {
        await chooseLatestLog();
        setTimeout(() => void openEvidence(), 250);
        return;
      }
    }

    if (paths.length) {
      const joined = paths.join(" ").toLowerCase().replace(/\\/g, "/");
      const matchReport = d.reports.find((r) => {
        const id = r.id.toLowerCase();
        const name = r.name.toLowerCase();
        const path = (r.path || "").toLowerCase().replace(/\\/g, "/");
        return paths.some((p) => {
          const n = p.toLowerCase().replace(/\\/g, "/");
          return id.includes(n) || name.includes(n) || path.includes(n) || n.includes(name);
        });
      });
      if (matchReport) {
        await chooseReport(matchReport.id);
      } else if (joined.includes("launcher")) {
        await chooseLauncherLog();
      } else if (joined.includes("latest.log") || joined.includes("/logs/")) {
        await chooseLatestLog();
      }
    }

    setTimeout(() => void openEvidence(), 250);
  }

  async function runAiExplain(opts: { quiet?: boolean; runId?: number } = {}) {
    if (!$projectPath) return;
    // Manual AI run (Retry AI / Explain): re-arm automatic scheduling.
    if (!opts.quiet) diagnoseWatchdogTripped = false;
    const run = opts.runId ?? ++analysisGeneration;
    aiLoading = true;
    cascadeLiveStage = "l1_searching";
    if (!opts.quiet) error = null;
    aiSoftError = null;
    aiFeedbackMsg = null;
    try {
      // Catch external disk edits before building AI context.
      try {
        void invoke("scan_project_changes", { path: $projectPath }).catch(() => {});
        await loadRecentPackEvents();
      } catch {
        // non-fatal
      }
      // Daemon warm-up and the heavy context build are independent — run
      // them concurrently. Serializing them stacked the Ollama probe time
      // (up to tens of seconds on a sick daemon) on top of the context
      // build before the AI cascade even started.
      const prepPromise = invoke<{ ok?: boolean; model?: string; skipped?: boolean }>(
        "ensure_ollama_model",
      ).catch((prepErr) => {
        console.warn("[AI] ensure_ollama_model:", prepErr);
        return null;
      });
      if (!opts.quiet) message = "Preparing local AI…";
      const reportId = activeReportId();
      const context: any = await invoke("build_ai_crash_context", {
        path: $projectPath,
        reportId,
      });
      if (!isCurrentAnalysis(run)) return;
      aiContext = context;
      aiPrompt = context.prompt ?? "";
      aiShowPrompt = false;
      const prep = await prepPromise;
      if (!opts.quiet && prep?.model) {
        message = `AI ready (${prep.model}). Analyzing crash…`;
      }
      const result = await invoke("analyze_crash_with_ai", {
        path: $projectPath,
        reportId,
      });
      if (!isCurrentAnalysis(run)) return;
      aiAnalysis = result;
      swarmEnabled = !!aiAnalysis?.swarmEnabled;
      enrichCrashFindingsWithAi();
      await loadPendingPlan();
      if (!opts.quiet) {
        const similar = context.similarCaseCount ?? 0;
        const model = context.aiModel ?? aiAnalysis?.model ?? "AI";
        const cascade = formatCascadeLabel(
          aiAnalysis?.cascadeStage ?? aiAnalysis?.source,
          Array.isArray(aiAnalysis?.cascadeTried) ? aiAnalysis.cascadeTried.map(String) : [],
        );
        const stage = cascade.label ? ` · ${cascade.label}` : "";
        const miss = cascade.detail ? ` · ${cascade.detail}` : "";
        const spec = aiAnalysis?.speculativeUsed
          ? ` · draft→verify${aiAnalysis?.speculativeDraftModel ? ` (${aiAnalysis.speculativeDraftModel})` : ""}`
          : "";
        message = `AI analysis ready (${model}${similar ? `, ${similar} KB hit(s)` : ""}${stage}${miss}${spec}). Review before applying.`;
      }
    } catch (e) {
      if (!isCurrentAnalysis(run)) return;
      const msg = String(e);
      aiAnalysis = null;
      if (opts.quiet) {
        aiSoftError = msg;
        throw e;
      }
      if (/not installed|Install model|no model|Settings → AI/i.test(msg)) {
        error = `${msg} Open Settings → AI to install a model.`;
        aiModalOpen = true;
      } else if (/model.*(not found)|pull|download/i.test(msg)) {
        error = `Local AI model missing: ${msg}`;
      } else if (/ollama|connection refused|failed to fetch|tcp|unreachable/i.test(msg)) {
        error = `Ollama unavailable — install from https://ollama.com, set the path in Settings → AI, then install a model there. ${msg}`;
      } else {
        error = msg;
      }
    } finally {
      if (isCurrentAnalysis(run)) {
        aiLoading = false;
        cascadeLiveStage = null;
      }
    }
  }

  async function sendAiFeedback(helped: boolean) {
    if (!$projectPath || !aiAnalysis) return;
    aiFeedbackBusy = true;
    aiFeedbackMsg = null;
    try {
      const path = await invoke<string>("record_crash_ai_feedback", {
        path: $projectPath,
        feedback: {
          helped,
          fingerprintKey: aiAnalysis.fingerprintKey ?? aiContext?.fingerprintKey ?? null,
          humanExplanation: aiAnalysis.human_explanation ?? aiAnalysis.humanExplanation ?? null,
          suspectedMods: aiAnalysis.suspected_mods ?? aiAnalysis.suspectedMods ?? [],
          recommendedActions: aiAnalysis.recommended_actions ?? aiAnalysis.recommendedActions ?? [],
          reportId: activeReportId(),
        },
      });
      aiFeedbackMsg = helped
        ? `Thanks — saved to knowledge base (${path}).`
        : `Marked as unhelpful — recorded in KB (${path}).`;
    } catch (e) {
      error = String(e);
    } finally {
      aiFeedbackBusy = false;
    }
  }

  function aiPlanActions(analysis: any): any[] {
    return analysis?.actions ?? analysis?.recommended_actions ?? analysis?.recommendedActions ?? [];
  }

  function aiActionLabel(action: any): string {
    const op = String(action?.op ?? action?.action_type ?? action?.actionType ?? "").toLowerCase();
    switch (op) {
      case "install_mod":
      case "install":
        return "Install";
      case "remove_mod":
      case "remove":
        return "Remove";
      case "disable_mod":
      case "disable":
        return "Disable";
      case "update_mod":
      case "update":
        return "Update";
      case "change_mod_version":
        return "Change version";
      case "reinstall_mod":
      case "reinstall":
        return "Reinstall";
      case "edit_config":
      case "config_change":
        return "Edit config";
      default:
        return op || "Action";
    }
  }

  function aiActionVersion(action: any): string | null {
    const v = String(action?.version ?? "").trim();
    if (!v) return null;
    const fake = new Set(["1.2.3", "0.0.0", "x.y.z", "latest", "version", "unknown", "null", "string"]);
    if (fake.has(v.toLowerCase()) || v === "X.Y.Z" || v === "<version>" || v === "{{version}}") return null;
    return v;
  }

  type MergedRec = {
    id: string;
    source: "rules" | "ai";
    label: string;
    detail: string;
    risk: string;
    modId: string | null;
    apply: () => void;
  };

  const mergedRecommendations = $derived(buildMergedRecommendations(crashFindings, aiAnalysis));
  const primaryRec = $derived(mergedRecommendations[0] ?? null);
  const sessionOk = $derived(!!(diagnosis?.sessionHealthy && preferLatestLog));

  function buildMergedRecommendations(findings: any[], analysis: any): MergedRec[] {
    const out: MergedRec[] = [];
    const seen = new Set<string>();
    for (const f of findings ?? []) {
      for (const fix of f.fixes ?? []) {
        const key = `rules:${fix.kind}:${fix.modId ?? fix.label}`;
        if (seen.has(key)) continue;
        seen.add(key);
        out.push({
          id: key,
          source: "rules",
          label: fix.label ?? fix.kind,
          detail: f.aiHint ? `${f.title} — AI: ${f.aiHint}` : f.title ?? f.code ?? "",
          risk: f.severity === "error" || f.severity === "critical" ? "high" : "medium",
          modId: fix.modId ?? null,
          apply: () => void applyCrashFindingFix(f, fix),
        });
      }
    }
    for (const a of aiPlanActions(analysis)) {
      const mid = a.modId ?? a.mod_id ?? null;
      const op = a.op ?? a.action_type ?? a.actionType ?? "action";
      const key = `ai:${op}:${mid ?? a.reason ?? ""}`;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push({
        id: key,
        source: "ai",
        label: `${aiActionLabel(a)}${mid ? ` ${mid}` : ""}`,
        detail: a.reason ?? a.description ?? "",
        risk: a.risk ?? "medium",
        modId: mid,
        apply: () => void applyAiPlan(),
      });
    }
    return out.slice(0, 12);
  }

  function buildPlanRows(actions: any[]) {
    const destructiveOps = new Set([
      "disable_mod",
      "remove_mod",
      "remove_file",
      "uninstall_mod",
    ]);
    return actions.map((a: any, idx: number) => {
      const op = String(a.op ?? a.action_type ?? a.actionType ?? "action");
      const modId = a.modId ?? a.mod_id ?? null;
      const path = a.path ?? null;
      let patchPreview: string | null = null;
      if (op === "edit_config" || path) {
        const patch = a.patch ?? null;
        patchPreview = patch
          ? `${path ?? "?"}\n${typeof patch === "string" ? patch : JSON.stringify(patch, null, 2)}`.slice(0, 800)
          : path
            ? String(path)
            : null;
      }
      const diffKind: "add" | "remove" | "change" | "other" = destructiveOps.has(op)
        ? "remove"
        : op === "edit_config" || op === "update_config" || op === "update_mod" || op === "change_mod_version"
          ? "change"
          : op.includes("install") || op.includes("download") || op === "reinstall_mod"
            ? "add"
            : "other";
      return {
        key: `${op}:${modId ?? path ?? idx}`,
        selected: true,
        op,
        modId: modId != null ? String(modId) : null,
        path: path != null ? String(path) : null,
        patchPreview,
        reason: String(a.reason ?? a.description ?? ""),
        risk: String(a.risk ?? "medium"),
        diffKind,
        destructive: destructiveOps.has(op) || (op === "edit_config" && String(a.risk ?? "") === "high"),
        raw: a,
      };
    });
  }

  function parseNetworkTrust(plan: any): {
    trustPercent: number | null;
    keeps: number | null;
    discards: number | null;
    mc: string | null;
    loader: string | null;
  } {
    const ctx = String(plan?.additionalContext ?? plan?.additional_context ?? "");
    const m = ctx.match(
      /trust:(\d+)\|keeps:(\d+)\|discards:(\d+)\|mc:([^|]*)\|loader:([^|]*)/,
    );
    if (!m) {
      return { trustPercent: null, keeps: null, discards: null, mc: null, loader: null };
    }
    return {
      trustPercent: Number(m[1]),
      keeps: Number(m[2]),
      discards: Number(m[3]),
      mc: m[4] && m[4] !== "-" ? m[4] : null,
      loader: m[5] && m[5] !== "-" ? m[5] : null,
    };
  }

  const networkTrust = $derived(pendingPlan ? parseNetworkTrust(pendingPlan) : null);
  const hasNetworkTrust = $derived(
    !!networkTrust &&
      (networkTrust.trustPercent != null ||
        networkTrust.keeps != null ||
        networkTrust.discards != null ||
        networkTrust.mc != null ||
        networkTrust.loader != null),
  );
  const cascadeFormatted = $derived(
    aiAnalysis
      ? formatCascadeLabel(
          aiAnalysis.cascadeStage ?? aiAnalysis.source,
          Array.isArray(aiAnalysis.cascadeTried) ? aiAnalysis.cascadeTried.map(String) : [],
        )
      : { label: "", detail: null as string | null },
  );
  const planReviewHasDestructive = $derived(planReviewRows.some((r: any) => r.destructive));

  async function refreshSoftVerifyStatus(path: string | null) {
    if (!path) {
      softVerifyStatus = null;
      return;
    }
    const banner = await fetchCrashFixBanner(path);
    if (!banner) {
      softVerifyStatus = null;
      return;
    }
    if (banner.rolledBack) {
      softVerifyStatus = { kind: "rejected", detail: "rolled back" };
    } else if (banner.resolved) {
      softVerifyStatus = { kind: "confirmed" };
    } else {
      softVerifyStatus = { kind: "pending" };
    }
  }

  function openAiPlanReview() {
    if (!$projectPath || !aiAnalysis) return;
    const actions = aiPlanActions(aiAnalysis);
    if (!actions.length) {
      error = "No actions in the AI plan to apply.";
      return;
    }
    if (aiAnalysis.validation && aiAnalysis.validation.ok === false) {
      error = `Plan invalid: ${(aiAnalysis.validation.errors ?? []).join("; ")}`;
      return;
    }
    planReviewSource = "ai";
    planReviewRows = buildPlanRows(actions);
    planReviewNeedsAck = !!(aiAnalysis.needsUserReview ?? aiAnalysis.needs_user_review ?? true);
    planReviewAcknowledged = !planReviewNeedsAck;
    planReviewExplanation =
      aiAnalysis.humanExplanation ?? aiAnalysis.human_explanation ?? "AI / KB ActionPlan";
    planReviewOpen = true;
  }

  function openNetworkPlanReview() {
    if (!$projectPath || !pendingPlan) return;
    const actions = pendingPlan.actions ?? [];
    if (!actions.length) {
      error = "Pending network plan has no actions.";
      return;
    }
    planReviewSource = "network";
    planReviewRows = buildPlanRows(actions);
    planReviewNeedsAck = !!(pendingPlan.needsUserReview ?? pendingPlan.needs_user_review ?? true);
    planReviewAcknowledged = !planReviewNeedsAck;
    planReviewExplanation =
      pendingPlan.humanExplanation ?? pendingPlan.human_explanation ?? "Network ActionPlan";
    planReviewOpen = true;
  }

  async function applyAiPlan() {
    openAiPlanReview();
  }

  async function confirmPlanReviewApply() {
    if (!$projectPath || !planReviewCanApply) return;
    const selected = planReviewRows.filter((r) => r.selected);
    if (!selected.length) return;
    planReviewOpen = false;
    if (planReviewSource === "fix-all") {
      await executeFixAllApply(selected);
    } else if (planReviewSource === "network") {
      await executeNetworkPlanApply(selected.map((r) => r.raw));
    } else {
      await executeAiPlanApply(selected.map((r) => r.raw));
    }
  }

  function openFixAllReview() {
    const rows = collectFixAllActions(unifiedProblems);
    if (!rows.length) {
      message = "No automatic fixes available — use per-problem buttons or Resolve.";
      return;
    }
    planReviewSource = "fix-all";
    planReviewRows = rows.map((r) => ({
      key: r.key,
      selected: !r.destructive,
      op: r.action.label,
      modId: r.action.modId,
      path: null,
      patchPreview: null,
      reason: r.action.label,
      risk: r.destructive ? "high" : "low",
      diffKind: r.destructive ? ("remove" as const) : ("add" as const),
      destructive: r.destructive,
      problemId: r.problemId,
      problemTitle: r.problemTitle,
      raw: r.action,
    }));
    const destructive = rows.some((r) => r.destructive);
    planReviewNeedsAck = destructive;
    planReviewAcknowledged = !destructive;
    planReviewExplanation = `Apply ${rows.length} fix(es) from the Problems list. Snapshot is created once before the batch.`;
    planReviewOpen = true;
  }

  async function invokeFixBatch(actions: ProblemFixAction[]) {
    if (!$projectPath || !actions.length) return null;
    return invoke<{ summary: string; stopped: boolean; applied: { ok: boolean; error?: string }[] }>(
      "apply_fix_actions",
      {
        path: $projectPath,
        actions: actions.map((a) => normalizeFixAction(a)),
      },
    );
  }

  async function applyFixBatchOrThrow(
    actions: Array<ProblemFixAction | FixAction>,
    opts?: { problemId?: string; reload?: boolean },
  ): Promise<string> {
    const result = await invokeFixBatch(actions.map((a) => normalizeFixAction(a)));
    if (!result) throw new Error("Fix batch returned no result");
    if (result.stopped) {
      const failed = result.applied.find((a) => !a.ok);
      throw new Error(failed?.error ?? "Fix failed");
    }
    if (opts?.problemId) {
      appliedProblemIds = new Set([...appliedProblemIds, opts.problemId]);
    }
    verifyPrompt = true;
    showApplyTrail = true;
    requestIdeIssuesRefresh();
    if (opts?.reload !== false) {
      await load(true);
      void detectWrongLoaderMods();
      void detectDuplicateModJars();
    }
    return result.summary;
  }

  async function executeFixAllApply(selected: typeof planReviewRows) {
    if (!$projectPath) return;
    fixAllBusy = true;
    error = null;
    try {
      const actions = selected.map((r) => r.raw as ProblemFixAction);
      const result = await invokeFixBatch(actions);
      if (!result) return;
      if (result.stopped) {
        const failed = result.applied.find((a) => !a.ok);
        error = failed?.error ?? "Batch stopped after an error.";
      }
      message = `${result.summary}. Snapshot saved — Test launch to verify.`;
      verifyPrompt = true;
      showApplyTrail = true;
      requestIdeIssuesRefresh();
      for (const row of selected) {
        if (row.problemId) {
          appliedProblemIds = new Set([...appliedProblemIds, row.problemId]);
        }
      }
      await load(true);
      void detectWrongLoaderMods();
      void detectDuplicateModJars();
    } catch (e) {
      error = String(e);
    } finally {
      fixAllBusy = false;
    }
  }

  async function executeAiPlanApply(actionsRaw: any[]) {
    if (!$projectPath || !aiAnalysis) return;
    aiApplyBusy = true;
    error = null;
    showApplyTrail = false;
    try {
      const plan = {
        schemaVersion: aiAnalysis.schemaVersion ?? 1,
        humanExplanation: aiAnalysis.humanExplanation ?? aiAnalysis.human_explanation ?? "",
        confidence: aiAnalysis.confidence ?? 0.5,
        suspectedMods: aiAnalysis.suspectedMods ?? aiAnalysis.suspected_mods ?? [],
        needsUserReview: false,
        source: aiAnalysis.source ?? null,
        matchedCaseIds: aiAnalysis.matchedCaseIds ?? [],
        actions: actionsRaw.map((a: any) => ({
          op: a.op ?? a.action_type ?? a.actionType,
          modId: a.modId ?? a.mod_id ?? null,
          provider: a.provider ?? null,
          projectId: a.projectId ?? a.project_id ?? null,
          version: a.version ?? null,
          path: a.path ?? null,
          patchType: a.patchType ?? a.patch_type ?? null,
          patch: a.patch ?? null,
          reason: a.reason ?? a.description ?? null,
          risk: a.risk ?? "medium",
        })),
        additionalContext: aiAnalysis.additionalContext ?? aiAnalysis.additional_context ?? null,
      };
      if (!plan.actions.length) {
        plan.actions = (aiAnalysis.recommended_actions ?? aiAnalysis.recommendedActions ?? []).map(
          (a: any) => ({
            op: a.action_type ?? a.actionType ?? "unknown",
            modId: a.mod_id ?? a.modId ?? null,
            provider: null,
            projectId: null,
            version: null,
            path: null,
            patchType: null,
            patch: null,
            reason: a.description ?? null,
            risk: a.risk ?? "medium",
          }),
        );
      }
      const result: any = await invoke("apply_action_plan", {
        path: $projectPath,
        plan,
        fingerprintKey: aiAnalysis.fingerprintKey ?? aiContext?.fingerprintKey ?? null,
      });
      const applied = result?.applied ?? [];
      const errs = result?.errors ?? [];
      message = `Applied ${applied.length} action(s).${errs.length ? ` Errors: ${errs.join("; ")}` : ""} Snapshot saved — Test launch to verify.`;
      showApplyTrail = applied.length > 0;
      verifyPrompt = applied.length > 0 && !errs.length;
      if (applied.length > 0 && !errs.length) {
        ideNeedsHealth.set(false);
        requestIdeIssuesRefresh();
      }
      if (errs.length) error = errs.join("; ");
      await load(true);
      if (applied.length && !errs.length) {
        window.dispatchEvent(new CustomEvent("tuffbox:crash-fix-applied"));
        mainTab = "problems";
      }
    } catch (e) {
      error = String(e);
    } finally {
      aiApplyBusy = false;
    }
  }

  async function executeNetworkPlanApply(actionsRaw: any[]) {
    if (!$projectPath || !pendingPlan) return;
    if (!swarmEnabled) {
      error = "Enable TuffSwarm network in Settings to apply network fixes.";
      return;
    }
    pendingBusy = true;
    error = null;
    showApplyTrail = false;
    try {
      const plan = { ...pendingPlan, needsUserReview: false, actions: actionsRaw };
      const result: any = await invoke("apply_action_plan", {
        path: $projectPath,
        plan,
        fingerprintKey: pendingPlan.matchedCaseIds?.[0] ?? null,
      });
      const applied = result?.applied ?? [];
      const errs = result?.errors ?? [];
      message = `Network fix applied (${applied.length}).${errs.length ? ` Errors: ${errs.join("; ")}` : ""}`;
      showApplyTrail = applied.length > 0;
      if (errs.length) error = errs.join("; ");
      await invoke("clear_pending_network_plan", { path: $projectPath });
      pendingPlan = null;
      window.dispatchEvent(new CustomEvent("tuffbox:crash-fix-applied"));
    } catch (e) {
      error = String(e);
    } finally {
      pendingBusy = false;
    }
  }

  async function loadPendingPlan() {
    pendingPlan = null;
    if (!$projectPath) return;
    try {
      const swarm = await invoke<{ enabled?: boolean }>("get_swarm_settings");
      swarmEnabled = !!swarm?.enabled;
      if (!swarmEnabled) return;
      pendingPlan = await invoke("get_pending_action_plan", { path: $projectPath });
    } catch {
      pendingPlan = null;
    }
  }

  async function applyPendingNetworkFix() {
    openNetworkPlanReview();
  }

  function parseAuthorActions(): any[] {
    try {
      const parsed = JSON.parse(authorActionsJson || "[]");
      return Array.isArray(parsed) ? parsed : [];
    } catch {
      throw new Error("Actions JSON is invalid — expect an array of {op, modId, …}");
    }
  }

  async function refreshAuthorCases() {
    if (!$projectPath) return;
    try {
      authorCases = await invoke("list_authored_crash_cases", { path: $projectPath });
    } catch {
      authorCases = [];
    }
  }

  async function openAuthorForm(opts?: { fromAnalysis?: boolean }) {
    if (!$projectPath) return;
    authorOpen = true;
    authorMsg = null;
    authorExportPreview = "";
    authorBusy = true;
    try {
      const draft: any = await invoke("draft_authored_crash_case", {
        path: $projectPath,
        reportId: activeReportId(),
      });
      authorFingerprint = draft.fingerprint;
      authorSymptoms = (draft.symptoms ?? []).join("\n");
      if (opts?.fromAnalysis && aiAnalysis) {
        authorSolution =
          aiAnalysis.humanExplanation ?? aiAnalysis.human_explanation ?? authorSolution;
        authorSuspected = (
          aiAnalysis.suspectedMods ??
          aiAnalysis.suspected_mods ??
          []
        ).join(", ");
        const actions = aiPlanActions(aiAnalysis).map((a: any) => ({
          op: a.op ?? a.action_type ?? a.actionType,
          modId: a.modId ?? a.mod_id ?? null,
          provider: a.provider ?? null,
          projectId: a.projectId ?? a.project_id ?? null,
          version: a.version ?? null,
          path: a.path ?? null,
          patchType: a.patchType ?? a.patch_type ?? null,
          patch: a.patch ?? null,
          reason: a.reason ?? a.description ?? null,
          risk: a.risk ?? "medium",
        }));
        authorActionsJson = JSON.stringify(actions, null, 2);
        if (!authorId) {
          const ex = (draft.fingerprint?.exception ?? "case")
            .replace(/[^a-zA-Z0-9-]+/g, "-")
            .slice(0, 40)
            .toLowerCase();
          authorId = `authored-${ex || "case"}`;
        }
      } else if (!authorActionsJson || authorActionsJson === "[]") {
        authorActionsJson = JSON.stringify(
          [
            {
              op: "disable_mod",
              modId: "examplemod",
              reason: "Describe the fix",
              risk: "low",
            },
          ],
          null,
          2,
        );
      }
      await refreshAuthorCases();
    } catch (e) {
      error = String(e);
    } finally {
      authorBusy = false;
    }
  }

  async function saveAuthorCase() {
    if (!$projectPath || !authorFingerprint) return;
    authorBusy = true;
    authorMsg = null;
    error = null;
    try {
      const launcherActions = parseAuthorActions();
      const result: any = await invoke("save_authored_crash_case", {
        path: $projectPath,
        input: {
          id: authorId.trim() || null,
          fingerprint: authorFingerprint,
          solution: authorSolution.trim(),
          symptoms: authorSymptoms
            .split("\n")
            .map((s) => s.trim())
            .filter(Boolean),
          suspectedMods: authorSuspected
            .split(/[,;\n]/)
            .map((s) => s.trim())
            .filter(Boolean),
          launcherActions,
          actions: [],
          notes: authorNotes.trim() || null,
        },
      });
      authorMsg = `Saved «${result.caseId}» → KB + export ${result.exportPath}`;
      authorExportPreview = JSON.stringify(
        {
          id: result.case?.id,
          fingerprint: result.case?.fingerprint,
          solution: result.case?.solution,
          actions: result.case?.launcherActions ?? result.case?.launcher_actions,
        },
        null,
        2,
      );
      await refreshAuthorCases();
      message = `KB case saved: ${result.caseId}`;
    } catch (e) {
      error = String(e);
    } finally {
      authorBusy = false;
    }
  }

  async function copyAuthorExport(caseId?: string) {
    if (!$projectPath) return;
    try {
      let text = authorExportPreview;
      if (caseId) {
        text = await invoke<string>("get_authored_case_export", {
          path: $projectPath,
          caseId,
        });
      }
      if (!text) throw new Error("Nothing to copy");
      await navigator.clipboard.writeText(text);
      authorMsg = "Export JSON copied (notes stripped — safe for remote KB).";
    } catch (e) {
      error = String(e);
    }
  }

  async function openAuthorExportFolder() {
    if (!$projectPath) return;
    try {
      await invoke("open_authored_kb_folder", { path: $projectPath });
    } catch (e) {
      error = String(e);
    }
  }

  async function copyAiPrompt() {
    try { await navigator.clipboard.writeText(aiPrompt); message = "AI prompt copied."; }
    catch { message = "Failed to copy prompt."; }
  }

  async function scanOreGen() {
    if (!$projectPath) return;
    oreLoading = true;
    try {
      oreFindings = await invoke("scan_ore_generation", { path: $projectPath });
    } catch (e) {
      error = String(e);
    } finally {
      oreLoading = false;
    }
  }

  async function scanDuplicateItems() {
    if (!$projectPath) return;
    duplicateLoading = true;
    try {
      duplicateFindings = await invoke("detect_duplicate_items", { path: $projectPath });
    } catch (e) {
      error = String(e);
    } finally {
      duplicateLoading = false;
    }
  }

  async function generateUnify() {
    if (!$projectPath) return;
    unifyLoading = true;
    try {
      unifyConfigResult = await invoke("generate_unify_config", { path: $projectPath, save: true });
      message = `Unify config saved with ${unifyConfigResult.materials?.length ?? 0} materials.`;
    } catch (e) {
      error = String(e);
    } finally {
      unifyLoading = false;
    }
  }

  async function detectWrongLoaderMods() {
    if (!$projectPath) return;
    wrongLoaderLoading = true;
    try {
      wrongLoaderJars = await invoke("detect_wrong_loader_mods", { path: $projectPath });
    } catch {
      wrongLoaderJars = [];
    } finally {
      wrongLoaderLoading = false;
    }
  }

  async function detectDuplicateModJars() {
    if (!$projectPath) return;
    duplicateJarLoading = true;
    try {
      duplicateJarGroups = await invoke("detect_duplicate_mod_jars", { path: $projectPath });
    } catch {
      duplicateJarGroups = [];
    } finally {
      duplicateJarLoading = false;
    }
  }

  async function keepOneDuplicateJar(modId: string, keepFileName: string) {
    if (!$projectPath) return;
    duplicateJarFixing = `${modId}::${keepFileName}`;
    error = null;
    try {
      const result: string = await invoke("keep_one_duplicate_mod_jar", {
        path: $projectPath,
        modId,
        keepFileName,
      });
      message = result;
      await detectDuplicateModJars();
      await load(true);
    } catch (e) {
      error = String(e);
    } finally {
      duplicateJarFixing = null;
    }
  }

  async function disableWrongJar(fileName: string) {
    if (!$projectPath) return;
    wrongLoaderFixing = fileName;
    error = null;
    try {
      const result: string = await invoke("disable_wrong_loader_jar", { path: $projectPath, fileName });
      message = result;
      await detectWrongLoaderMods();
    } catch (e) {
      error = String(e);
    } finally {
      wrongLoaderFixing = null;
    }
  }

  async function removeWrongJar(fileName: string) {
    if (!$projectPath) return;
    wrongLoaderFixing = fileName;
    error = null;
    try {
      const result: string = await invoke("remove_loose_jar", { path: $projectPath, fileName });
      message = result;
      await detectWrongLoaderMods();
    } catch (e) {
      error = String(e);
    } finally {
      wrongLoaderFixing = null;
    }
  }

  function icon(severity: string) {
    if (severity === "Error") return AlertCircle;
    if (severity === "Warning") return AlertTriangle;
    return Info;
  }

  function formatBytes(size: number) {
    if (size < 1024) return `${size} B`;
    if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
    return `${(size / 1024 / 1024).toFixed(1)} MB`;
  }

  function formatDate(seconds?: number | null) {
    if (!seconds) return "unknown time";
    return new Date(seconds * 1000).toLocaleString();
  }

  function actionLabel(action: any) {
    if (!action || typeof action !== "object") return String(action);
    const [kind, payload] = Object.entries(action)[0] ?? ["Action", {}];
    return `${kind}: ${JSON.stringify(payload)}`;
  }

  function hypothesisForGroup(title: string) {
    if (title === "Entrypoint") return "Likely a mod init failure. Check the provided-by mod, then its libraries and loader version.";
    if (title === "Loader mismatch") return "Likely a loader/API mismatch. Check the loader port and update its matching libraries.";
    if (title === "Render/OpenGL") return "Likely a render conflict. Disable shaders and test render mods (Sodium, Iris, Voxy…) in groups.";
    if (title === "Performance") return "Likely overload, not a crash cause. Lower view distance, profile heavy entities, rerun.";
    return "Review this signal group and compare it with recent snapshots.";
  }

  const topFinding = $derived(
    [...(crashFindings ?? [])].sort((a, b) => {
      const rank = (s: string) =>
        s === "critical" ? 4 : s === "error" ? 3 : s === "warning" ? 2 : 1;
      return rank(String(b.severity ?? "")) - rank(String(a.severity ?? ""));
    })[0] ?? null,
  );

  function severityChip(sev: string): string {
    if (sev === "critical") return "Fix this first";
    if (sev === "error") return "Needs a fix";
    if (sev === "warning") return "Worth checking";
    return "FYI";
  }

  const selectedReport = $derived(diagnosis?.selectedReport ?? null);
  const suspected = $derived(diagnosis?.suspectedMods ?? []);
  const primarySuspects = $derived(suspected.filter((m) => m.blameRole === "primary"));
  const topSuspect = $derived(primarySuspects[0] ?? suspected[0] ?? null);
  const heroCulpritLabel = $derived(
    primarySuspects.length > 1
      ? primarySuspects.map((m) => m.name).join(" + ")
      : topSuspect?.name ?? "",
  );
  const strongestEvidence = $derived(topSuspect?.evidence?.[0] ?? null);
  const providedByEvidence = $derived(
    topSuspect?.evidence?.find((item) => item.text.toLowerCase().includes("provided by")) ?? null,
  );

  /// Remember/restore the radio "which side to fix" choice for a conflict plan.
  /// Prefers a saved crash-fingerprint preference; else the recommended option.
  function preselectFixOption() {
    selectedFixOption = null;
    const opts = plan?.options ?? [];
    if (opts.length <= 1) return;
    type OptionLike = {
      label?: string;
      keepMod?: string | null;
      preferred?: boolean;
      actions?: { action?: string; nodeId?: string; modId?: string }[] | null;
    };
    const isOption = (o: unknown): o is OptionLike =>
      !!o && typeof o === "object";
    const fp = aiContext?.fingerprintKey ?? activeReportId() ?? null;
    const saved = fp ? getFixPreference(fp) : null;
    let idx = -1;
    if (saved != null) {
      idx = opts.findIndex((o: unknown) => isOption(o) && changeOptionKey(o) === saved);
    }
    if (idx < 0) idx = opts.findIndex((o: unknown) => isOption(o) && o.preferred === true);
    selectedFixOption = idx >= 0 ? idx : 0;
  }

  /// Actually applies the crash-diagnosis fix plan on the backend (snapshot
  /// + update/disable suspected mod / install missing dependency) and
  /// reports what really happened. Previously this only faked a success
  /// message in the UI without calling into the backend, so "fixing" a
  /// conflict did nothing to the project.
  async function applyFix() {
    if (!$projectPath) return;
    applying = true;
    error = null;
    message = null;
    try {
      const hasOptions = (plan?.options?.length ?? 0) > 0;
      const optIdx = hasOptions ? selectedFixOption : null;
      const applied: string[] = await invoke("apply_crash_fix_plan", {
        path: $projectPath,
        reportId: activeReportId(),
        optionIndex: optIdx,
      });
      if (optIdx != null && optIdx >= 0) {
        const opt = plan?.options?.[optIdx];
        const fp = aiAnalysis?.fingerprintKey ?? aiContext?.fingerprintKey ?? activeReportId();
        if (opt && fp) setFixPreference(fp, changeOptionKey(opt));
      }
      message = applied.length
        ? `Applied: ${applied.join(", ")}`
        : "No deterministic action was available for this plan. Review the notes manually.";
      await load(true);
    } catch (e) {
      error = String(e);
    } finally {
      applying = false;
    }
  }

  /// Applies a machine-actionable fix from a diagnosis hint (raise memory,
  /// accept EULA, change port, update/reinstall/disable a mod, etc.).
  async function applyHintFix(hint: DiagnosisHint) {
    if (hint.fix) await applyHintFixAction(hint, hint.fix);
  }

  /// Applies a specific fix action (used for per-mod buttons on a hint).
  async function applyHintFixAction(hint: DiagnosisHint, action: FixAction) {
    if (!$projectPath) return;
    const normalized = normalizeFixAction(action);
    applyingHintId = hint.id;
    error = null;
    message = null;
    try {
      const summary = await applyFixBatchOrThrow([normalized]);
      message = summary || `Applied fix: ${hint.title}`;
    } catch (e) {
      error = String(e);
    } finally {
      applyingHintId = null;
    }
  }

  /// One-by-one fix from a Crash Assistant finding card.
  async function applyCrashFindingFix(finding: any, action: FixAction) {
    await applyHintFixAction(
      {
        id: `ca:${finding.code}`,
        title: finding.title,
        severity: finding.severity,
        detail: finding.description,
        steps: finding.autoFix ? [finding.autoFix] : [],
        relatedMods: [],
        fix: null,
        fixes: finding.fixes ?? [],
      },
      action,
    );
  }

  /// Launches the client (Test) profile so the user can reproduce a crash.
  /// Soft-verify is started by `launchWithFeedback` → `confirm_crash_resolution_after_launch`
  /// (do not invoke confirm again here).
  async function runTest() {
    if (!$projectPath || launching || projectRunning) return;
    error = null;
    message = "Preparing Test launch — reproduce the crash, then come back.";
    // launchWithFeedback drives the shared launch store; no local reset.
    const result = await launchWithFeedback(
      { path: $projectPath, profile: "client" },
      {
        onStarted: () => {
          message =
            "Test launch started. Soft-verify runs in the background (≥180s healthy play + latest.log). Watch Home banner countdown.";
        },
      },
    );
    if (result) {
      message =
        "Test launch started. Soft-verify continues until a healthy post-fix session passes the playtime gate (or the game crashes / you Restore).";
      // The invoke returns once the JVM spawns; keep the spinner until this
      // instance's process-exited event (grace fallback covers edge cases).
      const path = $projectPath;
      let exited = false;
      let unlisten: () => void = () => {};
      const onExited = (event: { payload?: { id?: string } }) => {
        if (event.payload?.id === path) {
          exited = true;
          unlisten();
        }
      };
      listen<{ id: string; code?: number | null }>("process-exited", onExited).then((fn) => {
        if (exited) fn();
        else unlisten = fn;
      });
      setTimeout(() => {
        if (!exited) {
          unlisten();
        }
      }, 15000);
      return;
    }
  }

  async function stopTest() {
    if (!$projectPath || !projectRunning) return;
    await killWithFeedback($projectPath);
  }

  /// Opens the project folder in the OS file manager (quick access to
  /// crash-reports / logs without leaving Diagnose).
  async function openFolder() {
    if (!$projectPath) return;
    try {
      await invoke("open_project_folder", { path: $projectPath });
    } catch (e) {
      error = String(e);
    }
  }

  const allHints = $derived([
    ...(diagnosis?.hints ?? []),
    ...(diagnosis?.latestLog?.hints ?? []),
    ...(diagnosis?.launcherLog?.hints ?? []),
  ]);
  const dedupedHints = $derived(
    Array.from(new Map(allHints.filter((h) => h && h.id).map((h) => [h.id, h])).values()),
  );

  // Per-line detection highlights for the open crash report: lineNumber -> kind.
  // Drives the inline signal marker so crashes are visible at a glance.
  const signalLineMap = $derived.by(() => {
    const m = new Map<number, string>();
    const signals = preferLatestLog
      ? (diagnosis?.latestLog?.signals ?? [])
      : (selectedReport?.signals ?? []);
    for (const s of signals) {
      if (s.lineNumber && s.lineNumber > 0) {
        const prev = m.get(s.lineNumber);
        m.set(s.lineNumber, prev ?? s.kind);
      }
    }
    return m;
  });

  // --- Log source text (viewer itself lives in DiagnoseLogViewer.svelte) ---
  let logViewerRef = $state<{ scrollToLine: (line: number) => void } | null>(null);

  const currentLogText = $derived(
    preferLatestLog
      ? (diagnosis?.latestLog?.tail ?? "")
      : preferLauncherLog
        ? (diagnosis?.launcherLog?.tail ?? "")
        : (selectedReport?.content ?? ""),
  );
  const logDisplayText = $derived(
    currentLogText.length > 160_000 ? currentLogText.slice(currentLogText.length - 160_000) : currentLogText,
  );
  const logSourceKey = $derived(
    preferLatestLog ? LATEST_LOG_SOURCE : preferLauncherLog ? LAUNCHER_LOG_SOURCE : selectedReportId,
  );
  const logSourceLabel = $derived(preferLatestLog ? "latest.log" : (selectedReport?.summary?.name ?? "log"));

  /** Delegates to the log viewer child, which owns the scroll container and
   *  its own truncation window (see DiagnoseLogViewer.scrollToLine). Called
   *  from the verdict hero, tools strip, and the triage panel's jumpLine event. */
  function scrollLogToLine(line: number) {
    logViewerRef?.scrollToLine(line);
  }

  /** Wait for DiagnoseLogViewer to mount after switching to the Evidence tab. */
  async function scrollLogToLineWhenReady(line: number, maxAttempts = 10): Promise<boolean> {
    for (let i = 0; i < maxAttempts; i++) {
      if (logViewerRef) {
        logViewerRef.scrollToLine(line);
        return true;
      }
      await tick();
    }
    return false;
  }

  function fixActionModId(action: { modId?: string | null; mod_id?: string | null }): string | null {
    const id = action.modId ?? action.mod_id ?? null;
    return id ? String(id) : null;
  }

  function normalizeFixAction(action: FixAction | ProblemFixAction): FixAction {
    return {
      kind: action.kind,
      label: action.label,
      modId: fixActionModId(action),
    };
  }

  /** Switch to Evidence and scroll once the log viewer is mounted. */
  async function openEvidence(opts?: { line?: number }) {
    mainTab = "evidence";
    await tick();
    if (opts?.line != null && opts.line > 0) {
      await scrollLogToLineWhenReady(Math.max(0, opts.line - 1));
      return;
    }
    await jumpToFirstErrorAsync();
  }

  const LOG_ERROR_RE = /\b(FATAL|ERROR|SEVERE)\b|Exception|Caused by:|Crash Report/i;
  let activeErrorHit = $state(-1);

  const errorHits = $derived(
    (logDisplayText ? logDisplayText.split("\n") : [])
      .map((l, i) => (LOG_ERROR_RE.test(l) ? i : -1))
      .filter((i) => i >= 0),
  );

  /** Cycle through every ERROR/FATAL/Exception line (wraps). */
  function jumpToNextError() {
    if (!errorHits.length) {
      message = "No ERROR/FATAL/Exception lines found in this log view.";
      return;
    }
    activeErrorHit = ((activeErrorHit + 1) % errorHits.length + errorHits.length) % errorHits.length;
    const idx = errorHits[activeErrorHit];
    scrollLogToLine(idx);
    message = `Error ${activeErrorHit + 1}/${errorHits.length} · line ${idx + 1}`;
  }

  async function jumpToNextErrorAsync() {
    if (!errorHits.length) {
      message = "No ERROR/FATAL/Exception lines found in this log view.";
      return;
    }
    activeErrorHit = ((activeErrorHit + 1) % errorHits.length + errorHits.length) % errorHits.length;
    const idx = errorHits[activeErrorHit];
    if (await scrollLogToLineWhenReady(idx)) {
      message = `Error ${activeErrorHit + 1}/${errorHits.length} · line ${idx + 1}`;
    }
  }

  /** Jump to first ERROR line in the log (resets cycle). */
  function jumpToFirstError() {
    activeErrorHit = -1;
    jumpToNextError();
  }

  async function jumpToFirstErrorAsync() {
    activeErrorHit = -1;
    await jumpToNextErrorAsync();
  }

  async function copyCurrentLog() {
    const text = currentLogText;
    if (!text) {
      error = "Nothing to copy — load a log first.";
      return;
    }
    try {
      await navigator.clipboard.writeText(text);
      message = `Copied ${text.length.toLocaleString()} characters.`;
    } catch {
      error = "Clipboard copy failed.";
    }
  }

  function projectDir(): string {
    const p = $projectPath ?? "";
    const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
    return i >= 0 ? p.slice(0, i) : p;
  }

  async function openSubdir(name: string) {
    if (!$projectPath) return;
    try {
      await openShell(`${projectDir()}\\${name}`.replace(/\//g, "\\"));
    } catch {
      try {
        await openShell(`${projectDir()}/${name}`);
      } catch (e) {
        error = String(e);
      }
    }
  }

  // --- Unified Problems panel (Health Check feed) ---
  const unifiedProblems = $derived(
    buildUnifiedProblems({
      hints: [
        ...(diagnosis?.hints ?? []),
        ...(diagnosis?.latestLog?.hints ?? []),
        ...(diagnosis?.launcherLog?.hints ?? []),
      ],
      findings: crashFindings,
      graphDiagnostics: diagnosis?.graphDiagnostics ?? [],
      wrongLoaderJars,
      duplicateJarGroups,
      memoryHint: diagnosis?.sessionHealthy && preferLatestLog ? null : diagnosis?.memoryHint,
      worldCoords: diagnosis?.sessionHealthy && preferLatestLog ? null : diagnosis?.worldCoords,
      aiAnalysis,
    }),
  );
  const problemsBlocking = $derived(hasBlockingProblems(unifiedProblems));
  const fixAllCount = $derived(collectFixAllActions(unifiedProblems).length);

  const graphDiagnostics = $derived(diagnosis?.graphDiagnostics ?? []);
  const sourceLabel = $derived((() => {
    if (preferLatestLog) return "Game log";
    if (preferLauncherLog) return "Launcher log";
    if (selectedReportId?.startsWith("session/")) {
      const rep = diagnosis?.reports?.find((r) => r.id === selectedReportId);
      return rep?.name ?? "Archived session";
    }
    if (diagnosis?.analysisSource === "archived_session") return "Archived crash session";
    if (selectedReportId?.startsWith("hs_err")) return "Native crash";
    if (selectedReportId) return "Crash report";
    return "Log";
  })());

  async function applyProblemAction(problem: Problem, action: ProblemFixAction) {
    if (!$projectPath) return;
    const kind = action.kind;
    if (kind === "openResolve") {
      ideStageRequest.set("resolve");
      return;
    }
    if (kind === "openSetup") {
      ideStageRequest.set("setup");
      return;
    }
    if (kind === "openEvidence") {
      void openEvidence(
        problem.evidence?.line ? { line: problem.evidence.line } : undefined,
      );
      return;
    }
    if (kind === "reviewAiPlan") {
      await applyAiPlan();
      return;
    }
    if (kind === "installAllMissing") {
      applyingProblemId = problem.id;
      error = null;
      try {
        const plan = await invoke<any>("get_resolve_change_plan", { path: $projectPath });
        if (plan?.actions?.length) {
          const applied = await invoke<string[]>("apply_resolve_change_plan", { path: $projectPath });
          message = `Installed dependencies (${applied?.length ?? plan.actions.length}). Snapshot saved — Test launch to verify.`;
          verifyPrompt = true;
          showApplyTrail = true;
          requestIdeIssuesRefresh();
          await load(true);
          void detectWrongLoaderMods();
          void detectDuplicateModJars();
        } else {
          ideStageRequest.set("resolve");
          message = "Open Resolve to install missing dependencies.";
        }
      } catch (e) {
        error = String(e);
        ideStageRequest.set("resolve");
      } finally {
        applyingProblemId = null;
      }
      return;
    }
    if (kind === "installDependency") {
      const modId = fixActionModId(action);
      if (!modId) {
        error = "Install action is missing a mod id.";
        return;
      }
      applyingProblemId = problem.id;
      applyingHintId = problem.id;
      error = null;
      try {
        await applyFixBatchOrThrow(
          [{ kind: "installDependency", label: action.label, modId }],
          { problemId: problem.id },
        );
        message = `Installed ${modId}. Snapshot saved — Test launch to verify.`;
      } catch (e) {
        error = String(e);
      } finally {
        applyingProblemId = null;
        applyingHintId = null;
      }
      return;
    }
    if (kind === "installMissingForMod" && action.modId) {
      applyingProblemId = problem.id;
      error = null;
      try {
        const requester = action.modId;
        const missingIds = [
          ...new Set(
            (diagnosis?.graphDiagnostics ?? [])
              .filter((d) => String(d.code ?? "").toUpperCase().includes("MISSING"))
              .filter((d) => {
                const from = d.relatedNodes?.[0];
                const slug =
                  typeof from === "string"
                    ? from.replace(/^mod:/i, "")
                    : from && typeof from === "object" && "0" in from
                      ? String((from as { 0: unknown })[0] ?? "").replace(/^mod:/i, "")
                      : "";
                return slug === requester;
              })
              .map((d) => {
                const to = d.relatedNodes?.[1];
                if (typeof to === "string") return to.replace(/^mod:/i, "");
                if (to && typeof to === "object" && "0" in to) {
                  return String((to as { 0: unknown })[0] ?? "").replace(/^mod:/i, "");
                }
                const m = String(d.message ?? "").match(/missing dependency\s+mod:([a-z0-9_-]+)/i);
                return m?.[1] ?? "";
              })
              .filter(Boolean),
          ),
        ];
        if (missingIds.length === 0) {
          // Fall back to modIds on the problem card (skip requester itself).
          for (const mid of problem.modIds) {
            if (mid !== requester) missingIds.push(mid);
          }
        }
        if (missingIds.length === 0) {
          message = "No missing dependencies found for this mod.";
          return;
        }
        await applyFixBatchOrThrow(
          missingIds.map((mid) => ({
            kind: "installDependency",
            label: `Install ${mid}`,
            modId: mid,
          })),
          { problemId: problem.id },
        );
        message = `Installed ${missingIds.length} dep(s) for ${requester}. Snapshot saved — Test launch to verify.`;
      } catch (e) {
        error = String(e);
      } finally {
        applyingProblemId = null;
      }
      return;
    }
    if (kind === "removeWrongJar" && action.modId) {
      await removeWrongJar(action.modId);
      verifyPrompt = true;
      return;
    }
    if (kind === "raiseMemory") {
      applyingProblemId = problem.id;
      try {
        await applyFixBatchOrThrow(
          [{ kind: "raiseMemory", label: action.label, modId: null }],
          { problemId: problem.id },
        );
        message = "Memory raised. Snapshot saved — Test launch to verify.";
      } catch (e) {
        error = String(e);
      } finally {
        applyingProblemId = null;
      }
      return;
    }

    applyingProblemId = problem.id;
    applyingHintId = problem.id;
    error = null;
    try {
      const normalized = normalizeFixAction(action);
      const summary = await applyFixBatchOrThrow([normalized], { problemId: problem.id });
      message = `${summary}. Snapshot saved — Test launch to verify.`;
    } catch (e) {
      error = String(e);
    } finally {
      applyingProblemId = null;
      applyingHintId = null;
    }
  }

  function onProblemWhy(problem: Problem) {
    void openEvidence(problem.evidence?.line ? { line: problem.evidence.line } : undefined);
  }

  function scrollToPackWarnings() {
    mainTab = "problems";
    queueMicrotask(() => {
      document.getElementById("dx-pack-warnings")?.scrollIntoView({ behavior: "smooth", block: "start" });
    });
  }

  const allSignals = $derived(
    diagnosis?.sessionHealthy && preferLatestLog
      ? []
      : [
          ...(diagnosis?.selectedReport?.signals ?? []),
          ...(diagnosis?.latestLog?.signals ?? []),
          ...(diagnosis?.launcherLog?.signals ?? []),
        ],
  );
  const signalGroups = $derived(
    (() => {
      const groups = [
        { title: "Entrypoint", hint: "Fabric/Quilt entrypoint failures", kinds: ["Entrypoint"] },
        { title: "Loader mismatch", hint: "Wrong loader/API/version bridge", kinds: ["LoaderMismatch", "WrongLoader"] },
        { title: "Mixin", hint: "Mixin apply / inject conflicts", kinds: ["Mixin"] },
        { title: "OutOfMemory", hint: "Java heap / native OOM", kinds: ["OutOfMemory"] },
        { title: "Render/OpenGL", hint: "Renderer, shader or GPU pipeline", kinds: ["OpenGl"] },
        { title: "Ticking / world", hint: "Ticking entity or world corruption signals", kinds: ["TickingEntity"] },
        { title: "Performance", hint: "Tick stalls and overload", kinds: ["Performance"] },
      ].map((group) => ({ ...group, items: [] as Evidence[] }));
      const byKind = new Map<string, Evidence[]>();
      for (const signal of allSignals) {
        const bucket = byKind.get(signal.kind) ?? [];
        bucket.push(signal);
        byKind.set(signal.kind, bucket);
      }
      return groups
        .map(({ title, hint, kinds }) => ({
          title,
          hint,
          items: kinds.flatMap((kind) => byKind.get(kind) ?? []),
        }))
        .filter((group) => group.items.length > 0);
    })(),
  );

  const cascadingFinding = $derived(
    crashFindings.find((f: any) => String(f.code ?? "").toUpperCase() === "CASCADING_CONFIG_ERROR"),
  );
  const mixinFinding = $derived(
    crashFindings.find((f: any) => /mixin/i.test(String(f.code ?? "") + String(f.title ?? ""))),
  );
  const sideMismatchFinding = $derived(
    crashFindings.find((f: any) => /client.?only|side.?mismatch|SERVER/i.test(String(f.code ?? "") + String(f.title ?? ""))),
  );
  const isHsErr = $derived(
    diagnosis?.analysisSource === "hs_err" || (selectedReportId?.startsWith("hs_err/") ?? false),
  );
  const hsErrKind = $derived(
    diagnosis?.hsErrLogs?.find((h) => h.id === selectedReportId)?.kind ?? (isHsErr ? "native" : null),
  );
  $effect(() => {
    const path = $projectPath;
    if (!path) {
      softVerifyStatus = null;
      advancedLoadedPath = null;
      if (analysisKickoff) {
        clearTimeout(analysisKickoff);
        analysisKickoff = undefined;
      }
    }
    onProjectPathChange(path);
  });

  // AI is CPU-heavy (especially with local Ollama). Base rule diagnostics
  // run on load, while AI enrichment starts only when the AI tab is opened or
  // the user explicitly presses AI explain.
  $effect(() => {
    const path = $projectPath;
    if (
      shouldAutoScheduleAi({
        mainTabAi: mainTab === "ai",
        hasProject: !!path,
        hasDiagnosis: !!diagnosis,
        sessionOk,
        hasAiAnalysis: !!aiAnalysis,
        hasAiSoftError: !!aiSoftError,
        analysisBusy,
        watchdogTripped: diagnoseWatchdogTripped,
      })
    ) {
      scheduleUnifiedAnalysis(true);
    }
  });

  // Advanced tools are independent of the base diagnosis. Do not query the
  // group-test session on every Diagnose open; load it only when the user
  // actually enters the Advanced tab.
  $effect(() => {
    const path = $projectPath;
    if (mainTab !== "advanced" || !path || advancedLoadedPath === path) return;
    advancedLoadedPath = path;
    void refreshGroupTest();
  });

  onMount(() => {
    // Refresh whenever the Diagnose tab is (re)opened so the user always sees
    // fresh crash-report / log data rather than a stale snapshot from a
    // previous visit. Without this the panel could appear "stuck" / empty.
    const reload = () => {
      lastLoadedPath = null;
      void load(true);
    };
    window.addEventListener("tuffbox:open-diagnostics", reload);
    if ($projectPath) {
      void load(true);
      void loadPendingPlan();
      void refreshSoftVerifyStatus($projectPath);
    }
    let unlistenCascade: UnlistenFn | undefined;
    let unlistenSoftVerify: UnlistenFn | undefined;
    let unlistenCrash: UnlistenFn | undefined;
    let unlistenProgress: UnlistenFn | undefined;
    let unlistenTiming: UnlistenFn | undefined;
    void listen<{ stage?: string }>("diagnose-cascade", (ev) => {
      const stage = ev.payload?.stage;
      if (stage) cascadeLiveStage = stage;
    }).then((u) => {
      unlistenCascade = u;
    });
    // Live backend stage for the whole Diagnose pipeline (log reading, jar
    // scanning, AI stages) — replaces the static "Loading crash diagnosis…".
    void listen<{ stage?: string }>("diagnose-progress", (ev) => {
      const stage = ev.payload?.stage;
      if (stage) liveDiagnoseStage = stage;
    }).then((u) => {
      unlistenProgress = u;
    });
    void listen<{ phase?: string; elapsedMs?: number; cacheHit?: boolean }>("diagnose-timing", (ev) => {
      const phase = ev.payload?.phase;
      const elapsedMs = Number(ev.payload?.elapsedMs);
      if (!phase || !Number.isFinite(elapsedMs)) return;
      diagnoseTimings = {
        ...diagnoseTimings,
        [phase]: { elapsedMs, cacheHit: !!ev.payload?.cacheHit },
      };
    }).then((u) => {
      unlistenTiming = u;
    });
    void listen<SoftVerifyOutcome>("tuffbox:soft-verify-outcome", (ev) => {
      const payload = ev.payload ?? {};
      const path = $projectPath;
      if (!path) return;
      if (payload.path && payload.path !== path) return;
      const outcome = String(payload.outcome ?? "").toLowerCase();
      if (outcome === "confirm") {
        softVerifyStatus = { kind: "confirmed" };
      } else if (outcome === "reject") {
        softVerifyStatus = {
          kind: "rejected",
          detail: payload.reason ? String(payload.reason) : undefined,
        };
      }
    }).then((u) => {
      unlistenSoftVerify = u;
    });
    void listen("launch-crashed", () => {
      if (groupTest && !["done", "failed"].includes(groupTestPhaseKey(groupTest))) {
        void reportGroupTest("crash");
      }
    }).then((u) => {
      unlistenCrash = u;
    });
    return () => {
      window.removeEventListener("tuffbox:open-diagnostics", reload);
      unlistenCascade?.();
      unlistenSoftVerify?.();
      unlistenCrash?.();
      unlistenProgress?.();
      unlistenTiming?.();
    };
  });
</script>


<div class="diagnostics">
  <div class="dx-top">
    <div class="toolbar dx-chrome">
      <div class="title">
        <Stethoscope size={18} />
        <span>Health</span>
        {#if analysisBusy || crashLoading || aiLoading}
          <span class="analyzing-pill">Analyzing…</span>
        {/if}
      </div>
      <div class="dx-chrome-actions">
        {#if diagnosis}
          <select
            class="dx-source-select chrome-source"
            value={preferLatestLog ? LATEST_LOG_SOURCE : preferLauncherLog ? LAUNCHER_LOG_SOURCE : selectedReportId}
            onchange={onSourceChange}
            title="Log source"
          >
            <option value={LATEST_LOG_SOURCE}>
              Game log{diagnosis.latestLog.exists ? "" : " (missing)"}
            </option>
            <option value={LAUNCHER_LOG_SOURCE}>
              Launcher log{diagnosis.launcherLog?.exists ? "" : " (missing)"}
            </option>
            {#each diagnosis.reports as report (report.id)}
              <option value={report.id}>
                {report.id.startsWith("session/")
                  ? report.name
                  : report.id.startsWith("hs_err")
                    ? `Native crash · ${report.name}`
                    : `Crash report · ${report.name}`}
              </option>
            {/each}
          </select>
        {/if}
        <button class="ghost" onclick={() => load(true)} disabled={!$projectPath || loading} title="Reload logs & pack graph">
          <RefreshCw size={15} class={loading ? "spin" : ""} /> Refresh
        </button>
        <button
          class="secondary"
          onclick={() => runUnifiedAnalysis({ force: true })}
          disabled={!$projectPath || analysisBusy || loading || sessionOk}
          title="Re-run rules + AI"
        >
          <RefreshCw size={15} class={analysisBusy ? "spin" : ""} />
          {analysisBusy ? "Analyzing…" : "Re-analyze"}
        </button>
        {#if projectRunning}
          <button class="primary" onclick={stopTest} disabled={!$projectPath}>
            <Square size={14} fill="currentColor" /> Stop test
          </button>
        {:else}
          <button class="primary" onclick={runTest} disabled={!$projectPath || launching || loading}>
            <Play size={15} class={launching ? "spin" : ""} />
            {launching ? (launchSession?.message || "Launching…") : "Test launch"}
          </button>
        {/if}
        {#if launchSession && (launching || projectRunning || launchSession.phase === "failed")}
          <button class="ghost" type="button" onclick={() => $projectPath && openLaunchLog($projectPath)}>
            <FileText size={15} /> Live log
          </button>
        {/if}
      </div>
    </div>

    {#if error}<div class="notice error">{error}</div>{/if}
    {#if launchSession && (launching || launchSession.phase === "failed" || launchSession.phase === "exited")}
      <div class="notice" class:error={launchSession.phase === "failed"} class:warning={launching}>
        <span>{launchSession.message || (launching ? "Preparing Test launch…" : "Test launch ended.")}</span>
        <div class="trail-links">
          <button class="ghost mini" type="button" onclick={() => $projectPath && openLaunchLog($projectPath)}>
            <FileText size={12} /> Live log
          </button>
          {#if launchSession.phase === "failed"}
            <button class="ghost mini" type="button" onclick={() => ideStageRequest.set("diagnose")}>
              <Bug size={12} /> Crash findings
            </button>
          {/if}
        </div>
      </div>
    {/if}
    {#if message}
      <div class="notice success trail-notice">
        <span>{message}</span>
        {#if showApplyTrail}
          <div class="trail-links">
            <button class="ghost mini" type="button" onclick={() => ideStageRequest.set("history")}><History size={12} /> History</button>
            <button class="ghost mini" type="button" onclick={() => ideStageRequest.set("snapshots")}><Database size={12} /> Snapshots</button>
          </div>
        {/if}
      </div>
    {/if}
    {#if verifyPrompt}
      <div class="notice warning verify-banner">
        <span>Fix applied. Run a Test launch to confirm it worked?</span>
        <div class="trail-links">
          <button class="primary small" type="button" onclick={() => { verifyPrompt = false; void runTest(); }} disabled={launching || projectRunning}>
            Test launch
          </button>
          <button class="ghost mini" type="button" onclick={() => (verifyPrompt = false)}>Later</button>
          <button
            class="ghost mini"
            type="button"
            onclick={async () => {
              verifyPrompt = false;
              if ($projectPath) {
                try {
                  await invoke("confirm_crash_resolution_from_diagnose", { path: $projectPath });
                  message = "Marked as looks fixed.";
                  await load(true);
                } catch {
                  /* ignore */
                }
              }
            }}
          >
            Looks fixed
          </button>
        </div>
      </div>
    {/if}
    {#if aiSoftError}
      <div class="notice warning">
        AI unavailable — rules still work on Problems.
        <span class="soft-ai-detail">{aiSoftError}</span>
        <button class="ghost mini" type="button" onclick={() => (aiModalOpen = true)}>AI settings</button>
      </div>
    {/if}
    {#if pendingPlan && swarmEnabled}
      <div class="notice warning network-pending">
        <div class="network-pending-head">
          <span>Network fix ready ({(pendingPlan.actions ?? []).length} action(s)).</span>
          <button class="secondary small" onclick={applyPendingNetworkFix} disabled={pendingBusy}>
            {pendingBusy ? "Applying…" : "Review & apply"}
          </button>
        </div>
        {#if hasNetworkTrust && networkTrust}
          <div class="trust-card-line">
            {#if networkTrust.trustPercent != null}
              <span class="diff-chip">Trust {networkTrust.trustPercent}%</span>
            {/if}
            {#if networkTrust.keeps != null}
              <span class="diff-chip">Keeps {networkTrust.keeps}</span>
            {/if}
            {#if networkTrust.discards != null}
              <span class="diff-chip">Discards {networkTrust.discards}</span>
            {/if}
            {#if networkTrust.mc}
              <span class="diff-chip">MC {networkTrust.mc}</span>
            {/if}
            {#if networkTrust.loader}
              <span class="diff-chip">{networkTrust.loader}</span>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
    {#if softVerifyStatus}
      <div
        class="notice soft-verify-notice"
        class:warning={softVerifyStatus.kind === "pending"}
        class:success={softVerifyStatus.kind === "confirmed"}
        class:error={softVerifyStatus.kind === "rejected"}
      >
        Soft-verify: {softVerifyStatus.kind}{#if softVerifyStatus.detail} ({softVerifyStatus.detail}){/if}
      </div>
    {/if}
  </div>

  <div class="dx-scroll">
  {#if recentPackEvents.length > 0}
    <section class="panel recent-pack-panel">
      <div class="recent-head">
        <strong><History size={14} /> Recent pack changes</strong>
        <span class="recent-meta">{recentPackEvents.length} event{recentPackEvents.length === 1 ? "" : "s"}</span>
        <button class="ghost mini" onclick={() => ideStageRequest.set("history")}>Open History</button>
      </div>
      <div class="recent-list">
        {#each recentPackEvents as ev (ev.id)}
          <button type="button" class="recent-row" onclick={() => openHistoryEvent(ev.id, ev.meta?.fingerprintKey ?? null)} title={ev.summary}>
            <span class="recent-actor">{ev.actor}</span>
            <span class="recent-op">{ev.op}</span>
            <span class="recent-sum">{ev.summary}</span>
            <small>{relTime(ev.ts)}</small>
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if loading && !diagnosis}
    <div class="loading">
      <div class="loading-title">Loading crash diagnosis…</div>
      {#if liveDiagnoseStage}
        <div class="loading-stage">{liveDiagnoseStage}</div>
      {/if}
    </div>
  {:else if !$projectPath}
    <EmptyState icon={Stethoscope} title="Pick a pack first" description="Open a project — we'll read the crash log and tell you what to click next." />
  {:else if diagnosis}
    <DiagnoseStatusBar
      problems={unifiedProblems}
      sessionOk={sessionOk}
      loading={loading}
      analyzing={analysisBusy || crashLoading || aiLoading}
      liveStage={liveDiagnoseStage}
      cascadeStage={aiLoading
        ? (cascadeLiveStage ?? "l1_searching")
        : (aiAnalysis?.cascadeStage ?? null)}
      cascadeLabel={aiAnalysis && cascadeFormatted.label ? cascadeFormatted.label : null}
      cascadeDetail={aiAnalysis ? cascadeFormatted.detail : null}
      sourceLabel={sourceLabel}
      fixAllCount={fixAllCount}
      fixAllBusy={fixAllBusy}
      onScrollToWarnings={scrollToPackWarnings}
      onFixAll={openFixAllReview}
    />

    {#if health}
      <section class="health-strip panel" aria-label="Pack health summary">
        {#if health.hasCrash}
          <span class="health-chip bad" title={(health.crashReports ?? []).join("\n")}>
            ⚠ {health.crashReports?.length ?? 1} crash report{((health.crashReports?.length ?? 1) !== 1) ? "s" : ""}
          </span>
        {/if}
        <span class="health-chip" class:bad={(health.errorCount ?? 0) > 0}>
          {health.errorCount ?? 0} errors
        </span>
        <span class="health-chip" class:bad={(health.warningCount ?? 0) > 0}>
          {health.warningCount ?? 0} warnings
        </span>
        <span class="health-chip" class:bad={(health.missingCount ?? 0) > 0}>
          {health.missingCount ?? 0} missing
        </span>
        {#if (health.hashMismatchCount ?? 0) > 0}
          <span class="health-chip bad">{health.hashMismatchCount} hash mismatch</span>
        {/if}
        <span class="health-chip" class:bad={(health.exportBlockers?.length ?? 0) > 0}>
          {health.exportBlockers?.length ?? 0} export blocker{((health.exportBlockers?.length ?? 0) !== 1) ? "s" : ""}
        </span>
        {#if (health.exportBlockers?.length ?? 0) > 0}
          <div class="health-blockers">
            {#each health.exportBlockers ?? [] as b (b.code)}
              <code>{b.code}</code><span>{b.message}</span>
            {/each}
          </div>
        {/if}
      </section>
    {:else if healthLoading}
      <div class="loading compact health-strip-loading">Loading health…</div>
    {/if}

    {#if !sessionOk}
      <DiagnoseVerdictHero
        sessionOk={sessionOk}
        topSuspect={topSuspect}
        topFinding={topFinding}
        heroCulpritLabel={heroCulpritLabel}
        strongestEvidence={strongestEvidence}
        analysisBusy={analysisBusy}
        primaryRec={primaryRec}
        mergedRecommendations={[]}
        aiApplyBusy={aiApplyBusy}
        applyingHintId={applyingHintId}
        disablingModId={disablingModId}
        fixingIdx={fixingIdx}
        aiAnalysis={aiAnalysis}
        logDisplayText={logDisplayText}
        isHsErr={isHsErr}
        hsErrKind={hsErrKind}
        memoryHint={null}
        worldCoords={null}
        cascadingFinding={null}
        mixinFinding={null}
        sideMismatchFinding={null}
        suspected={suspected}
        onFixDisableMod={fixDisableMod}
        onApplyTopSuspectUpdate={applyTopSuspectUpdate}
        onApplyAiPlan={applyAiPlan}
        onJumpToFirstError={() => void openEvidence()}
        warningCount={unifiedProblems.filter((p) => p.severity === "warning" || p.severity === "info").length}
        onShowWarnings={scrollToPackWarnings}
      />
    {/if}

    {#if !diagnosis.reports.some((r) => r.id.startsWith("crash-reports/")) && !problemsBlocking && !sessionOk}
      <div class="dx-empty-sources panel">
        <span>No crash reports yet.</span>
        <button type="button" class="ghost mini" onclick={chooseLatestLog}>Game log</button>
        <button type="button" class="ghost mini" onclick={runTest} disabled={launching || projectRunning}>Test launch</button>
      </div>
    {/if}

    <div
      class="dx-drop panel"
      role="region"
      aria-label="Import player crash"
      ondragover={(e) => e.preventDefault()}
      ondrop={onDropCrash}
    >
      <span>Drop a player crash file, or paste mclo.gs URL:</span>
      <div class="dx-drop-row">
        <input type="url" placeholder="https://mclo.gs/…" bind:value={importUrl} />
        <button type="button" class="ghost mini" disabled={importBusy || !importUrl.trim()} onclick={importFromMclogsUrl}>
          {importBusy ? "…" : "Import"}
        </button>
      </div>
    </div>

    <div class="dx-main-tabs" role="tablist">
      <button type="button" role="tab" class="dx-main-tab" class:active={mainTab === "problems"} aria-selected={mainTab === "problems"} onclick={() => (mainTab = "problems")}>
        Problems{#if unifiedProblems.length}<span class="count">{unifiedProblems.length}</span>{/if}
      </button>
      <button type="button" role="tab" class="dx-main-tab" class:active={mainTab === "evidence"} aria-selected={mainTab === "evidence"} onclick={() => (mainTab = "evidence")}>
        Evidence
      </button>
      <button type="button" role="tab" class="dx-main-tab" class:active={mainTab === "ai"} aria-selected={mainTab === "ai"} onclick={() => (mainTab = "ai")}>
        AI plan{#if aiAnalysis}<span class="count">1</span>{/if}
      </button>
      <button type="button" role="tab" class="dx-main-tab" class:active={mainTab === "advanced"} aria-selected={mainTab === "advanced"} onclick={() => (mainTab = "advanced")}>
        Advanced
      </button>
    </div>

    {#if mainTab === "problems"}
      <DiagnoseProblemsList
        problems={unifiedProblems}
        sessionOk={sessionOk}
        applyingId={applyingProblemId}
        appliedIds={appliedProblemIds}
        onApply={applyProblemAction}
        onWhy={onProblemWhy}
      />
      {#if problemsBlocking}
        <div class="dx-resolve-bridge">
          <button type="button" class="secondary" onclick={() => ideStageRequest.set("resolve")}>
            Fix pack graph in Resolve
          </button>
        </div>
      {/if}
    {:else if mainTab === "evidence"}
      <DiagnoseTriagePanels
        signalGroups={signalGroups}
        sections={selectedReport?.sections ?? []}
        suspected={suspected}
        recentSnapshots={diagnosis.recentSnapshots ?? []}
        mcreatorMods={[]}
        classFinderResults={[]}
        bind:classQuery
        classBusy={false}
        classResults={[]}
        dependentResults={[]}
        bind:toolsOpen={analysisToolsOpen}
        disablingModId={disablingModId}
        bisectMods={[]}
        worldCoords={diagnosis.worldCoords ?? null}
        memoryHint={diagnosis.memoryHint ?? null}
        cascadingBanner={cascadingFinding ? cascadingFinding.description : null}
        sourceHint={sourceLabel}
        onJumpLine={(ln) => {
          const n = Number(ln) || 0;
          if (n <= 0) void openEvidence();
          else scrollLogToLine(Math.max(0, n - 1));
        }}
        onDisableMod={fixDisableMod}
        onUpdateMod={async (id: string) => {
          if (!id) return;
          fixingIdx = -1;
          try {
            await applyFixBatchOrThrow([
              { kind: "updateMod", label: `Update ${id}`, modId: id },
            ]);
            message = `Update requested for ${id}`;
          } catch (err) {
            error = String(err);
          } finally {
            fixingIdx = null;
          }
        }}
        onToggleBisect={() => {}}
        onFindClass={() => {}}
        onFindDependents={() => {}}
        onOpenSnapshots={() => ideStageRequest.set("snapshots")}
      />
      <DiagnoseLogViewer
        bind:this={logViewerRef}
        logDisplayText={logDisplayText}
        currentLogTextLength={currentLogText.length}
        sourceLabel={logSourceLabel}
        signalLineMap={signalLineMap}
        errorHits={errorHits}
        activeErrorHit={activeErrorHit}
        sharingLog={sharingLog}
        hasLogText={!!currentLogText}
        sourceKey={logSourceKey}
        onJumpNextError={jumpToNextError}
        onCopy={copyCurrentLog}
        onShare={shareCurrentLog}
      />
    {:else if mainTab === "ai"}
      <DiagnoseAnalysisTabs
        crashFindings={crashFindings}
        crashLoading={crashLoading}
        aiAnalysis={aiAnalysis}
        aiLoading={aiLoading}
        aiSoftError={aiSoftError}
        aiApplyBusy={aiApplyBusy}
        aiFeedbackBusy={aiFeedbackBusy}
        aiFeedbackMsg={aiFeedbackMsg}
        applyingHintId={applyingHintId}
        cascadeLabel={aiAnalysis && cascadeFormatted.label ? cascadeFormatted.label : null}
        onApplyFindingFix={({ finding, action }) => applyCrashFindingFix(finding, action)}
        onRetryAi={() => runAiExplain()}
        onApplyAiPlan={applyAiPlan}
        onFeedback={sendAiFeedback}
      />
    {:else}
      <DiagnoseAdvanced
        projectPath={$projectPath}
        aiLoading={aiLoading}
        sessionOk={sessionOk}
        sharingLog={sharingLog}
        currentLogText={currentLogText}
        supportBusy={supportBusy}
        planning={planning}
        oreLoading={oreLoading}
        duplicateLoading={duplicateLoading}
        unifyLoading={unifyLoading}
        wrongLoaderLoading={wrongLoaderLoading}
        duplicateJarLoading={duplicateJarLoading}
        authorBusy={authorBusy}
        timings={diagnoseTimings}
        bind:aiPrompt
        bind:aiShowPrompt
        runAiExplain={() => void runAiExplain()}
        shareCurrentLog={shareCurrentLog}
        exportSupportPack={exportSupportPack}
        copyCurrentLog={copyCurrentLog}
        openFolder={openFolder}
        openSubdir={openSubdir}
        createFixPlan={() => void createFixPlan()}
        scanOreGen={() => void scanOreGen()}
        scanDuplicateItems={() => void scanDuplicateItems()}
        generateUnify={() => void generateUnify()}
        detectWrongLoaderMods={() => void detectWrongLoaderMods()}
        detectDuplicateModJars={() => void detectDuplicateModJars()}
        onOpenAuthorForm={() => void openAuthorForm({ fromAnalysis: !!aiAnalysis })}
        onOpenAiSettings={() => (aiModalOpen = true)}
        signalGroups={[]}
        recentSnapshots={diagnosis.recentSnapshots ?? []}
        suspected={suspected}
        crashMcreator={crashMcreator}
        crashClassFinder={crashClassFinder}
        bind:classQuery
        classBusy={classBusy}
        classResults={classResults}
        dependentResults={dependentResults}
        bind:analysisToolsOpen
        disablingModId={disablingModId}
        bisectMods={bisectMods}
        onJumpLine={() => void openEvidence()}
        onDisableMod={fixDisableMod}
        onUpdateMod={async (id: string) => {
          if (!id) return;
          try { await applyFixBatchOrThrow([{ kind: "updateMod", label: `Update ${id}`, modId: id }]); message = `Update requested for ${id}`; }
          catch (err) { error = String(err); }
        }}
        onToggleBisect={toggleBisect}
        onFindClass={runClassFinder}
        onFindDependents={runFindDependents}
        onOpenSnapshots={() => ideStageRequest.set("snapshots")}
        groupTest={groupTest}
        groupTestActive={groupTestActive}
        groupTestStatus={groupTestStatus(groupTest)}
        groupTestPhaseKey={groupTestPhaseKey(groupTest)}
        groupTestBusy={groupTestBusy}
        bind:groupTestAuto
        launching={launching}
        onStartGroupTest={() => void startGroupTest()}
        onGroupTestLaunch={() => void runTest()}
        onReportGroupTest={(outcome: "healthy" | "crash") => void reportGroupTest(outcome)}
        onCancelGroupTest={() => void cancelGroupTest()}
        graphDiagnostics={graphDiagnostics}
        duplicateJarGroups={duplicateJarGroups}
        wrongLoaderJars={wrongLoaderJars}
        fixingIdx={fixingIdx}
        duplicateJarFixing={duplicateJarFixing}
        wrongLoaderFixing={wrongLoaderFixing}
        onFixMissingDependency={({ modId, idx }: { modId: string; idx: number }) => fixMissingDependency(modId, idx)}
        onFixDeduplicate={fixDeduplicate}
        onKeepOneDuplicateJar={({ modId, fileName }: { modId: string; fileName: string }) => keepOneDuplicateJar(modId, fileName)}
        onDisableWrongJar={disableWrongJar}
        onRemoveWrongJar={removeWrongJar}
        plan={plan}
        bind:selectedFixOption
        applying={applying}
        onApplyPlan={() => void applyFix()}
        bind:authorOpen
        bind:authorId
        bind:authorSolution
        bind:authorSymptoms
        bind:authorSuspected
        bind:authorActionsJson
        bind:authorNotes
        authorExportPreview={authorExportPreview}
        authorMessage={authorMsg}
        onSaveAuthor={() => void saveAuthorCase()}
        onCopyAuthorExport={() => void copyAuthorExport()}
        onOpenAuthorFolder={() => void openAuthorExportFolder()}
      />
    {/if}
  {:else}
    <div class="empty">Press Refresh to load diagnosis.</div>
  {/if}
  </div>
</div>

<DiagnosePlanReviewModal
  bind:open={planReviewOpen}
  source={planReviewSource}
  explanation={planReviewExplanation}
  hasDestructive={planReviewHasDestructive}
  bind:rows={planReviewRows}
  needsAck={planReviewNeedsAck}
  bind:acknowledged={planReviewAcknowledged}
  canApply={planReviewCanApply}
  selectedCount={planReviewSelectedCount}
  busy={aiApplyBusy || pendingBusy || fixAllBusy}
  networkTrust={planReviewSource === "network" ? networkTrust : null}
  onCancel={() => (planReviewOpen = false)}
  onConfirm={confirmPlanReviewApply}
/>

<AiConnectionModal bind:open={aiModalOpen} />

<style>
  .diagnostics {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    overflow: hidden;
    /* Responsive: centered column, grows on 1440p without edge-to-edge. */
    max-width: min(1320px, 100%);
    width: 100%;
    margin: 0 auto;
  }
  .dx-top {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 0;
  }
  .dx-scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    scrollbar-gutter: stable;
  }
  .toolbar,
 .title { display: flex; align-items: center; }
  .toolbar { justify-content: space-between; gap: 16px; margin-bottom: 10px; flex-wrap: wrap; }
  .dx-chrome {
    position: sticky;
    top: 0;
    z-index: 5;
    padding: 8px 0 10px;
    background: color-mix(in srgb, var(--bg-primary) 92%, transparent);
    backdrop-filter: blur(8px);
  }
  .dx-chrome-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .chrome-source {
    max-width: 220px;
    padding: 6px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-size: 12px;
  }
  .verify-banner {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .dx-main-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 12px 0 14px;
    padding: 4px;
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
  }
  .dx-main-tab {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
  }
  .dx-main-tab.active {
    background: var(--bg-primary);
    color: var(--text-primary);
    box-shadow: 0 1px 2px rgba(0,0,0,0.12);
  }
  .dx-main-tab .count {
    padding: 1px 6px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent-primary) 15%, transparent);
    color: var(--accent-primary);
    font-size: 12px;
  }
  .dx-resolve-bridge {
    margin-top: 12px;
    display: flex;
    justify-content: flex-end;
  }
  .dx-empty-sources {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    padding: 10px 12px;
    margin-bottom: 10px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .dx-drop {
    margin-bottom: 12px;
    padding: 10px 12px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .dx-drop-row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 8px;
  }
  .dx-drop-row input {
    flex: 1;
    min-width: 160px;
    padding: 6px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-primary);
    color: var(--text-primary);
  }
  .primary.small, .secondary.small {
    padding: 6px 10px;
    font-size: 12px;
  }
  .title{ gap: 10px; color: var(--text-secondary); font-weight: 700; }
  .health-strip {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-bottom: 14px;
    padding: 10px 12px;
  }
  .health-chip {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
    font-weight: 600;
    padding: 3px 10px;
    border-radius: 999px;
    color: var(--text-secondary);
    background: color-mix(in srgb, var(--bg-secondary) 80%, transparent);
    border: 1px solid var(--border-color);
  }
  .health-chip.bad {
    color: #fca5a5;
    border-color: rgba(239, 68, 68, 0.5);
    background: rgba(239, 68, 68, 0.08);
  }
  .health-blockers {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 100%;
    margin-top: 6px;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    background: rgba(251, 191, 36, 0.08);
    border: 1px solid rgba(251, 191, 36, 0.25);
  }
  .health-blockers code {
    font-size: 12px;
    color: #fbbf24;
  }
  .health-blockers span {
    font-size: 12px;
    color: var(--text-secondary);
  }
  .health-strip-loading {
    margin-bottom: 14px;
  }
  .recent-pack-panel {
    margin-bottom: 14px;
  }
  .recent-meta {
    flex: 1;
    font-size: 12px;
    color: var(--text-muted);
  }
  .recent-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 8px;
  }
  .recent-head strong {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .recent-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .recent-row {
    display: grid;
    grid-template-columns: 64px 68px 1fr auto;
    gap: 8px;
    align-items: center;
    width: 100%;
    text-align: left;
    padding: 5px 8px;
    border-radius: var(--border-radius-sm);
    border: 1px solid transparent;
    background: var(--bg-primary);
    color: inherit;
    cursor: pointer;
    font: inherit;
  }
  .recent-op {
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.3px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .recent-row:hover {
    border-color: var(--border-color);
  }
  .recent-actor {
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .recent-sum {
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .recent-row small {
    color: var(--text-muted);
    font-size: 12px;
  }

  .dx-source-select {
    width: 100%;
    padding: 10px 12px;
    border-radius: 10px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-primary);
    font-size: 13px;
  }
  .notice { padding: 12px 14px; border-radius: var(--border-radius-lg); margin-bottom: 14px; border: 1px solid var(--border-color); }
  .notice.error { color: var(--accent-danger); background: color-mix(in srgb, var(--accent-danger) 8%, transparent); border-color: color-mix(in srgb, var(--accent-danger) 28%, transparent); }
  .notice.success { color: var(--accent-primary); background: color-mix(in srgb, var(--accent-primary) 8%, transparent); border-color: color-mix(in srgb, var(--accent-primary) 25%, transparent); }
  .panel, .empty, .loading { background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: var(--border-radius-lg); }
  .panel { padding: 16px; min-width: 0; }
  .analyzing-pill {
    display: inline-flex;
    align-items: center;
    padding: 2px 8px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 700;
  }
  .notice.warning { color: var(--accent-warning); background: color-mix(in srgb, var(--accent-warning) 8%, transparent); border-color: color-mix(in srgb, var(--accent-warning) 28%, transparent); }
  .soft-ai-detail {
    display: block;
    margin-top: 4px;
    font-size: 12px;
    opacity: 0.9;
    word-break: break-word;
  }
  .soft-verify-notice { font-size: 12px; padding: 8px 12px; }
  .network-pending { display: flex; flex-direction: column; gap: 8px; }
  .network-pending-head { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; justify-content: space-between; }
  .trust-card-line { font-size: 12px; color: var(--text-secondary); display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .diff-chip {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 700;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }
  .loading, .empty { padding: 24px; text-align: center; color: var(--text-muted); }
  .loading-title { font-weight: 700; color: var(--text-primary); }
  .loading-stage {
    margin-top: 6px;
    font-size: 12px;
    color: var(--accent-primary);
    /* Backend stage labels stream in; a soft pulse shows liveness. */
    animation: dx-stage-pulse 1.6s ease-in-out infinite;
  }
  @keyframes dx-stage-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.55; }
  }
  :global(.spin) { animation: dx-spin 0.9s linear infinite; }
  @keyframes dx-spin { to { transform: rotate(360deg); } }
  .trail-notice {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .trail-links { display: inline-flex; flex-wrap: wrap; gap: 8px; }
  .dx-empty-sources {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    font-size: 12px;
    color: var(--text-muted);
  }
  .dx-drop {
    margin-top: 4px;
    padding: 10px;
    border: 1px dashed var(--border-color);
    border-radius: var(--border-radius-sm);
    font-size: 12px;
    color: var(--text-muted);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .dx-drop-row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  .dx-drop-row input {
    flex: 1;
    min-width: 160px;
    padding: 6px 8px;
    border-radius: 6px;
    border: 1px solid var(--border-color);
    background: var(--bg-primary);
    color: inherit;
  }
</style>
