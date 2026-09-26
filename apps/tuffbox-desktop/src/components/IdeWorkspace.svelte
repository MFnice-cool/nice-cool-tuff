<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    ClipboardList,
    SlidersHorizontal,
    Package,
    GitGraph,
    FileCode2,
    PlayCircle,
    Stethoscope,
    History,
    Camera,
    UploadCloud,
    Rocket,
    Mountain,
    PackageOpen,
    ScrollText,
    Circle,
    Map as MapIcon,
    Plus,
    RotateCcw,
    X,
  } from "@lucide/svelte";
  import {
    projectPath,
    ideStageRequest,
    ideActiveStage,
    autoHideWorkflowRail,
    tuneDirty,
    briefDirty,
    questDirty,
    requestIdeNextAction,
    requestIdePlay,
    pushIdeRecent,
  } from "../lib/store";
  import { onDestroy, onMount } from "svelte";
  import type { Component } from "svelte";
  // Stage canvases load on demand — mounting the workspace must not parse the
  // whole production suite (Mods/Quests/Graph/…) at once; each stage arrives
  // when first opened (mirrors App.svelte VIEW_LOADERS).
  const STAGE_LOADERS: Record<StageId, () => Promise<{ default: Component }>> = {
    brief: () => import("./BriefEditor.svelte"),
    setup: () => import("./ProjectSettings.svelte"),
    content: () => import("./Mods.svelte"),
    quests: () => import("./QuestEditor.svelte"),
    recipes: () => import("./RecipeBrowser.svelte"),
    "world-map": () => import("./World.svelte"),
    "ore-gen": () => import("./OreGenVisualizer.svelte"),
    resolve: () => import("./Graph.svelte"),
    configs: () => import("./ConfigEditor.svelte"),
    history: () => import("./ChangeHistory.svelte"),
    test: () => import("./TestRuns.svelte"),
    diagnose: () => import("./Diagnostics.svelte"),
    snapshots: () => import("./Snapshots.svelte"),
    export: () => import("./ExportBuilder.svelte"),
    release: () => import("./ReleaseRoom.svelte"),
  };
  const stageCache = new Map<StageId, Component>();
  let stageComp = $state<Component | null>(null);
  let stageCompFor = $state<StageId | null>(null);
  let stageLoadError = $state<string | null>(null);

  async function mountStage(id: StageId) {
    const cached = stageCache.get(id);
    if (cached) {
      stageComp = cached;
      stageCompFor = id;
      stageLoadError = null;
      return;
    }
    stageComp = null;
    stageCompFor = id;
    stageLoadError = null;
    try {
      const mod = await STAGE_LOADERS[id]();
      stageCache.set(id, mod.default);
      if (stageCompFor === id) stageComp = mod.default;
    } catch (e) {
      if (stageCompFor === id) stageLoadError = String(e);
    }
  }
  $effect(() => {
    void mountStage(activeStage);
  });
  import GithubPackUpdateBanner from "./GithubPackUpdateBanner.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import { portal } from "../lib/portal";
  import IdeNextBar from "./IdeNextBar.svelte";

  type StageId =
    | "brief"
    | "setup"
    | "content"
    | "world-map"
    | "ore-gen"
    | "recipes"
    | "quests"
    | "resolve"
    | "configs"
    | "history"
    | "test"
    | "diagnose"
    | "snapshots"
    | "export"
    | "release";

  type Stage = {
    id: StageId;
    label: string;
    short: string;
    icon: any;
    goal: string;
    outputs: string[];
  };

  const stages: Stage[] = [
    {
      id: "brief",
      label: "Brief",
      short: "Idea",
      icon: ClipboardList,
      goal: "Shape the storefront listing: icon, summary, markdown description, and live Modrinth/CurseForge card preview.",
      outputs: ["listing card", "summary + icon", "author notes"],
    },
    {
      id: "setup",
      label: "Setup",
      short: "Project",
      icon: SlidersHorizontal,
      goal: "Choose Minecraft/loader/Java, memory budget and base project settings.",
      outputs: ["manifest", "profiles", "runtime settings"],
    },
    {
      id: "content",
      label: "Content",
      short: "Mods",
      icon: Package,
      goal: "Add, update and remove mods as managed dependencies, not loose files.",
      outputs: ["mod list", "source metadata", "auto snapshots"],
    },
    {
      id: "quests",
      label: "Quests",
      short: "Lore",
      icon: ScrollText,
      goal: "Author FTB Quests lines with AI sidebar (20+ quests, lore, tasks/rewards) and edit SNBT visually.",
      outputs: ["quest tree", "AI QuestPlan", "SNBT files", "validation report"],
    },
    {
      id: "recipes",
      label: "Recipes",
      short: "Craft",
      icon: PackageOpen,
      goal: "JEI-style recipe browser: search, Recipes/Uses, KubeJS remove scripts.",
      outputs: ["recipe list", "disable scripts", "ingredient search"],
    },
    {
      id: "world-map",
      label: "World",
      short: "Map",
      icon: MapIcon,
      goal: "MCA Selector-style chunk map: select, delete, export/import, NBT edit.",
      outputs: ["chunk map", "selection", "export / backup"],
    },
    {
      id: "ore-gen",
      label: "Ores",
      short: "Heights",
      icon: Mountain,
      goal: "Visualize ore generation heights, vein sizes and toggle worldgen from configs.",
      outputs: ["ore layers", "generation config", "spawn rates"],
    },
    {
      id: "resolve",
      label: "Resolve",
      short: "Graph",
      icon: GitGraph,
      goal: "Inspect dependency graph, missing dependencies, conflicts and side mismatches.",
      outputs: ["diagnostics", "change plan", "lockfile graph"],
    },
    {
      id: "configs",
      label: "Tune",
      short: "Configs",
      icon: FileCode2,
      goal: "Edit configs, scripts and overrides with rollback-safe saves.",
      outputs: ["configs", "KubeJS/scripts", "tracked changes"],
    },
    {
      id: "history",
      label: "History",
      short: "Changes",
      icon: History,
      goal: "Chronological pack activity: launcher ops, external disk edits, AI fixes.",
      outputs: ["timeline", "delta scan", "AI context"],
    },
    {
      id: "test",
      label: "Test",
      short: "Runs",
      icon: PlayCircle,
      goal: "Launch client/server profiles, collect logs and measure startup stability.",
      outputs: ["latest.log", "run history", "performance notes"],
    },
    {
      id: "diagnose",
      label: "Diagnose",
      short: "Health",
      icon: Stethoscope,
      goal: "Turn errors, crashes and graph diagnostics into clear next actions.",
      outputs: ["suspected mods", "fix hypotheses", "safe plan"],
    },
    {
      id: "snapshots",
      label: "Snapshots",
      short: "Checkpoints",
      icon: Camera,
      goal: "Checkpoint risky edits, compare states and rollback — not the activity feed (see History).",
      outputs: ["snapshots", "diff", "rollback point"],
    },
    {
      id: "export",
      label: "Export",
      short: "Build",
      icon: UploadCloud,
      goal: "Package the pack into .mrpack, Prism zip, server pack and changelog.",
      outputs: ["artifacts", "server pack", "changelog"],
    },
    {
      id: "release",
      label: "Release",
      short: "Ship",
      icon: Rocket,
      goal: "Prepare release notes, publish draft and track post-release hotfixes.",
      outputs: ["release snapshot", "publish draft", "support checklist"],
    },
  ];

  // ── Closable tabs: the rail hides stages on request; navigation onto a
  // hidden stage reopens it automatically. Persisted per app, not per project.
  const HIDDEN_STAGES_KEY = "tuffbox.ide.hiddenStages";

  function loadHiddenStages(): Set<StageId> {
    try {
      const raw = localStorage.getItem(HIDDEN_STAGES_KEY);
      const list: unknown = raw ? JSON.parse(raw) : [];
      const valid = Array.isArray(list)
        ? (list as unknown[]).filter((id): id is StageId =>
            stages.some((s) => s.id === id),
          )
        : [];
      // At least one tab must stay visible — reset a corrupt "all hidden" state.
      if (valid.length >= stages.length) return new Set();
      return new Set(valid);
    } catch {
      return new Set();
    }
  }

  let hiddenStages = $state<Set<StageId>>(loadHiddenStages());
  $effect(() => {
    try {
      localStorage.setItem(HIDDEN_STAGES_KEY, JSON.stringify([...hiddenStages]));
    } catch {
      /* ignore quota / private mode */
    }
  });

  const visibleStages = $derived(stages.filter((s) => !hiddenStages.has(s.id)));

  function closeStage(id: StageId) {
    if (hiddenStages.size >= stages.length - 1) return; // keep one tab visible
    const next = new Set(hiddenStages);
    next.add(id);
    hiddenStages = next;
    if (activeStage === id) {
      // Jump to the nearest still-visible neighbour (dirty-leave guards run
      // inside goToStage, so unsaved Tune/Brief/Quests edits stay protected).
      const order = stages.filter((s) => !next.has(s.id));
      const after = order.find((s) => stages.findIndex((x) => x.id === s.id) > stages.findIndex((x) => x.id === id));
      goToStage((after ?? order[0]).id);
    }
  }

  function reopenStage(id: StageId) {
    if (!hiddenStages.has(id)) return;
    const next = new Set(hiddenStages);
    next.delete(id);
    hiddenStages = next;
  }

  function reopenAllStages() {
    hiddenStages = new Set();
  }

  /** Stage chords when IDE focused (avoid App Ctrl+1… Home shortcuts). */
  const STAGE_CHORD: Record<string, StageId> = {
    "1": "content",
    "2": "resolve",
    "3": "history",
    "4": "test",
    "5": "diagnose",
    "6": "snapshots",
    "7": "configs",
    "8": "quests",
    "9": "export",
    "0": "brief",
  };

  // Reverse lookup for tooltip / chord badge — keeps discoverability of the
  // non-sequential mapping (1→Content … 0→Brief) without cluttering the label.
  const CHORD_FOR_STAGE: Record<StageId, string> = Object.fromEntries(
    Object.entries(STAGE_CHORD).map(([k, v]) => [v, k]),
  ) as Record<StageId, string>;
  function stageTooltip(stage: Stage): string {
    const chord = CHORD_FOR_STAGE[stage.id];
    const chordHint = chord ? ` • Ctrl+${chord}` : "";
    return `${stage.goal}${chordHint} • [ / ] соседние • Right-click to hide`;
  }

  let activeStage = $state<StageId>("content");
  let leaveConfirmOpen = $state(false);
  // Expose for browser preview / e2e — set synchronously so puppeteer can call immediately after navigation
  if (typeof window !== "undefined") {
    (window as any).__setIdeStage = (s: string) => goToStage(s as StageId);
    (window as any).__getIdeStage = () => activeStage;
  }
  $effect(() => {
    if (typeof window !== "undefined") {
      (window as any).__setIdeStage = (s: string) => goToStage(s as StageId);
      (window as any).__getIdeStage = () => activeStage;
    }
  });
  let pendingStage = $state<StageId | null>(null);
  let leaveKind = $state<"tune" | "brief" | "quests">("tune");

  function goAdjacentStage(dir: -1 | 1) {
    const visible = visibleStages;
    if (visible.length === 0) return;
    const index = visible.findIndex((s) => s.id === activeStage);
    if (index < 0) {
      goToStage(visible[0].id);
      return;
    }
    const next = visible[index + dir];
    if (next) goToStage(next.id);
  }

  function goToStage(id: StageId) {
    if (hiddenStages.has(id)) {
      // Navigating onto a closed tab (chord, IdeNextBar, library links) reopens it.
      const next = new Set(hiddenStages);
      next.delete(id);
      hiddenStages = next;
    }
    if (id === activeStage) return;
    if (activeStage === "configs" && $tuneDirty) {
      leaveKind = "tune";
      pendingStage = id;
      leaveConfirmOpen = true;
      return;
    }
    if (activeStage === "brief" && $briefDirty) {
      leaveKind = "brief";
      pendingStage = id;
      leaveConfirmOpen = true;
      return;
    }
    if (activeStage === "quests" && $questDirty) {
      leaveKind = "quests";
      pendingStage = id;
      leaveConfirmOpen = true;
      return;
    }
    activeStage = id;
    const stage = stages.find((s) => s.id === id);
    if (stage) pushIdeRecent(`ide:${id}`, `IDE · ${stage.label}`);
  }

  function confirmLeaveStage() {
    leaveConfirmOpen = false;
    if (leaveKind === "tune") tuneDirty.set(false);
    else if (leaveKind === "brief") briefDirty.set(false);
    else questDirty.set(false);
    if (pendingStage) {
      const id = pendingStage;
      activeStage = id;
      pendingStage = null;
      const stage = stages.find((s) => s.id === id);
      if (stage) pushIdeRecent(`ide:${id}`, `IDE · ${stage.label}`);
    }
  }

  function cancelLeaveStage() {
    leaveConfirmOpen = false;
    pendingStage = null;
  }

  // ── Rail tab context menus (right-click: close / reopen) ──
  /** Menu anchored at the pointer; `stageId` set → per-tab menu, null → rail menu. */
  let tabMenu = $state<{ x: number; y: number; stageId: StageId | null } | null>(null);

  const MENU_W = 220;
  const MENU_H = 260;
  function menuPosition(e: MouseEvent): { x: number; y: number } {
    const pad = 8;
    let x = e.clientX;
    let y = e.clientY;
    if (x + MENU_W > window.innerWidth - pad) x = window.innerWidth - MENU_W - pad;
    // The rail hugs the bottom edge — open the menu upward from the click.
    if (y + MENU_H > window.innerHeight - pad) y = window.innerHeight - MENU_H - pad;
    return { x: Math.max(pad, x), y: Math.max(pad, y) };
  }

  function openTabMenu(e: MouseEvent, id: StageId) {
    e.preventDefault();
    e.stopPropagation();
    const { x, y } = menuPosition(e);
    tabMenu = { x, y, stageId: id };
  }

  function openRailMenu(e: MouseEvent) {
    if ((e.target as HTMLElement | null)?.closest?.(".stage-tab, .stage-add")) return;
    e.preventDefault();
    const { x, y } = menuPosition(e);
    tabMenu = { x, y, stageId: null };
  }

  function closeTabMenu() {
    tabMenu = null;
  }

  $effect(() => {
    function onGlobalPointerDown(e: MouseEvent) {
      if (!tabMenu) return;
      if ((e.target as HTMLElement | null)?.closest?.(".ide-ctx-menu")) return;
      tabMenu = null;
    }
    function onGlobalKey(e: KeyboardEvent) {
      if (e.key === "Escape" && tabMenu) {
        e.stopPropagation();
        tabMenu = null;
      }
    }
    window.addEventListener("pointerdown", onGlobalPointerDown, true);
    window.addEventListener("keydown", onGlobalKey, true);
    return () => {
      window.removeEventListener("pointerdown", onGlobalPointerDown, true);
      window.removeEventListener("keydown", onGlobalKey, true);
    };
  });

  $effect(() => {
    ideActiveStage.set(activeStage);
  });

  $effect(() => {
    if ($ideStageRequest) {
        const req = $ideStageRequest;
        ideStageRequest.set(null);
        if (stages.some((s) => s.id === req)) {
          goToStage(req as StageId);
        }
        revealRailFromNavigation();
      }
  });

  let focusedScanTimer: ReturnType<typeof setInterval> | null = null;
  /** Background delta scan is useful on History/Diagnose; skip on other stages to save CPU. */
  const FOCUSED_SCAN_STAGES: StageId[] = ["history", "diagnose"];

  function stopFocusedScanLoop() {
    if (focusedScanTimer) {
      clearInterval(focusedScanTimer);
      focusedScanTimer = null;
    }
  }

  async function refreshFocusedScanLoop() {
    stopFocusedScanLoop();
    if (!$projectPath) return;
    if (!FOCUSED_SCAN_STAGES.includes(activeStage)) return;
    try {
      const settings: { focusedScan?: boolean } = await invoke("get_history_settings", {
        path: $projectPath,
      });
      if (!settings?.focusedScan) return;
      focusedScanTimer = setInterval(() => {
        if (!$projectPath) return;
        if (!FOCUSED_SCAN_STAGES.includes(activeStage)) return;
        void invoke("scan_project_changes", { path: $projectPath }).catch(() => {});
      }, 60_000);
    } catch {
      // ignore
    }
  }

  $effect(() => {
    if ($projectPath && FOCUSED_SCAN_STAGES.includes(activeStage)) {
        void refreshFocusedScanLoop();
      } else {
        stopFocusedScanLoop();
      }
  });

  onMount(() => {
    revealRailFromNavigation();
    const onVis = () => {
      if (document.visibilityState === "visible") void refreshFocusedScanLoop();
      else stopFocusedScanLoop();
    };
    const onSettings = () => void refreshFocusedScanLoop();
    const onKey = (e: KeyboardEvent) => {
      const t = e.target;
      if (t instanceof HTMLElement) {
        const tag = t.tagName;
        if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || t.isContentEditable) return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
        e.preventDefault();
        requestIdeNextAction();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && (e.key === "P" || e.key === "p")) {
        e.preventDefault();
        e.stopPropagation();
        requestIdePlay();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && STAGE_CHORD[e.key]) {
        e.preventDefault();
        e.stopPropagation();
        goToStage(STAGE_CHORD[e.key]);
        revealRailFromNavigation();
        return;
      }
      if (!e.ctrlKey && !e.metaKey && !e.altKey && e.key === "[") {
        e.preventDefault();
        goAdjacentStage(-1);
        revealRailFromNavigation();
        return;
      }
      if (!e.ctrlKey && !e.metaKey && !e.altKey && e.key === "]") {
        e.preventDefault();
        goAdjacentStage(1);
        revealRailFromNavigation();
      }
    };
    document.addEventListener("visibilitychange", onVis);
    window.addEventListener("tuffbox:history-settings-changed", onSettings);
    window.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("visibilitychange", onVis);
      window.removeEventListener("tuffbox:history-settings-changed", onSettings);
      window.removeEventListener("keydown", onKey, true);
    };
  });

  let railRevealed = $state(false);
  let railHideTimer: ReturnType<typeof setTimeout> | null = null;
  /** Grace before hide — enough to move from hotzone onto the rail. */
  const RAIL_HIDE_MS = 280;
  /** Keep the rail visible longer after left-nav / stage jumps so it feels opened. */
  const RAIL_NAV_HOLD_MS = 2800;

  function clearRailHideTimer() {
    if (railHideTimer) {
      clearTimeout(railHideTimer);
      railHideTimer = null;
    }
  }

  function revealRail() {
    if (!$autoHideWorkflowRail) return;
    clearRailHideTimer();
    railRevealed = true;
  }

  /** Programmatic open (sidebar / shortcuts) — show rail, then auto-hide after a beat. */
  function revealRailFromNavigation() {
    if (!$autoHideWorkflowRail) return;
    clearRailHideTimer();
    railRevealed = true;
    scheduleHideRail(RAIL_NAV_HOLD_MS);
  }

  function scheduleHideRail(delay = RAIL_HIDE_MS) {
    if (!$autoHideWorkflowRail) return;
    clearRailHideTimer();
    railHideTimer = setTimeout(() => {
      railRevealed = false;
      railHideTimer = null;
    }, delay);
  }

  function onRailFocusOut(e: FocusEvent) {
    if (!$autoHideWorkflowRail) return;
    const next = e.relatedTarget;
    if (next instanceof Node && e.currentTarget instanceof Node && e.currentTarget.contains(next)) {
      return;
    }
    scheduleHideRail();
  }

  $effect(() => {
    if (!$autoHideWorkflowRail) {
        railRevealed = false;
        clearRailHideTimer();
      }
  });

  onDestroy(() => {
    stopFocusedScanLoop();
    clearRailHideTimer();
    ideActiveStage.set(null);
  });
</script>

<div class="ide-workspace" class:auto-hide-rail={$autoHideWorkflowRail}>
  <section class="stage-shell">
    <IdeNextBar onGoStage={(s) => {
      goToStage(s as StageId);
      revealRailFromNavigation();
    }} />
    <div
      class="stage-content"
      class:fill-stage={
        activeStage === "configs" ||
        activeStage === "world-map" ||
        activeStage === "brief" ||
        activeStage === "setup" ||
        activeStage === "quests" ||
        activeStage === "content" ||
        activeStage === "resolve" ||
        activeStage === "test" ||
        activeStage === "diagnose" ||
        activeStage === "recipes" ||
        activeStage === "snapshots"
      }
    >
      {#if $projectPath}
        <GithubPackUpdateBanner />
      {/if}
      {#if activeStage === "setup" && !$projectPath}
        <div class="skeleton-page">
          <h2>No project opened</h2>
          <p>Go to Home, create or open an instance, then return to the IDE workflow.</p>
        </div>
      {:else if stageComp && stageCompFor === activeStage}
        {#if activeStage === "setup"}
          {@const StageView = stageComp}
          <StageView showBack={false} stayAfterSave={true} />
        {:else}
          {@const StageView = stageComp}
          <StageView />
        {/if}
      {:else if stageLoadError}
        <div class="skeleton-page">
          <h2>Stage failed to load</h2>
          <p>{stageLoadError}</p>
        </div>
      {:else}
        <div class="skeleton-page">
          <p>Loading…</p>
        </div>
      {/if}
    </div>
  </section>

  {#if $autoHideWorkflowRail}
    <div
      class="rail-hotzone"
      aria-hidden="true"
      onmouseenter={revealRail}
      onmouseleave={() => scheduleHideRail()}
    ></div>
  {/if}
  <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
  <nav
    class="workflow-rail"
    class:revealed={railRevealed || !$autoHideWorkflowRail}
    aria-label="Modpack production workflow"
    role="tablist"
    onmouseenter={revealRail}
    onmouseleave={() => scheduleHideRail()}
    onfocusin={revealRail}
    onfocusout={onRailFocusOut}
    oncontextmenu={openRailMenu}
  >
    {#each visibleStages as stage (stage.id)}
      {@const StageIcon = stage.icon}
      {@const chord = CHORD_FOR_STAGE[stage.id]}
      <button
        class="stage-tab"
        class:active={activeStage === stage.id}
        role="tab"
        aria-selected={activeStage === stage.id}
        aria-current={activeStage === stage.id ? "step" : undefined}
        data-chord={chord ?? undefined}
        onclick={(e) => {
          goToStage(stage.id);
          if (e.currentTarget instanceof HTMLElement) e.currentTarget.blur();
          scheduleHideRail(320);
        }}
        oncontextmenu={(e) => openTabMenu(e, stage.id)}
        title={stageTooltip(stage)}
      >
        <span class="stage-status" aria-hidden="true">
          <Circle size={12} fill={activeStage === stage.id ? "currentColor" : "none"} />
        </span>
        <StageIcon size={20} />
        <span class="stage-text">
          <strong>{stage.label}</strong>
          <small>{stage.short}</small>
        </span>
        {#if chord}
          <span class="stage-chord" aria-hidden="true">{chord}</span>
        {/if}
      </button>
    {/each}
    {#if hiddenStages.size > 0}
      <button
        class="stage-add"
        title="Reopen closed tabs"
        aria-label={`Reopen closed tabs (${hiddenStages.size})`}
        onclick={(e) => {
          const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
          tabMenu = {
            x: Math.max(8, Math.min(rect.left, window.innerWidth - MENU_W - 8)),
            y: Math.max(8, rect.top - MENU_H),
            stageId: null,
          };
        }}
      >
        <Plus size={16} />
      </button>
    {/if}
  </nav>
</div>

{#if tabMenu}
  <div
    class="ide-ctx-menu"
    use:portal
    style={`position:fixed; left:${tabMenu.x}px; top:${tabMenu.y}px; z-index:10000`}
    role="menu"
  >
    {#if tabMenu.stageId}
      {@const menuStage = stages.find((s) => s.id === tabMenu?.stageId)}
      {#if menuStage}
        {@const MenuIcon = menuStage.icon}
        <button
          type="button"
          role="menuitem"
          class="danger"
          disabled={visibleStages.length <= 1}
          title={visibleStages.length <= 1 ? "At least one tab stays open" : `Hide ${menuStage.label} from the rail`}
          onclick={() => {
            const id = menuStage.id;
            closeTabMenu();
            closeStage(id);
          }}
        >
          <X size={14} /> Close {menuStage.label}
        </button>
      {/if}
    {/if}
    {#if hiddenStages.size > 0}
      {#if tabMenu.stageId}
        <div class="menu-sep"></div>
      {/if}
      <div class="menu-title">Reopen</div>
      {#each stages.filter((s) => hiddenStages.has(s.id)) as hidden (hidden.id)}
        {@const HiddenIcon = hidden.icon}
        <button
          type="button"
          role="menuitem"
          onclick={() => {
            closeTabMenu();
            reopenStage(hidden.id);
          }}
        >
          <HiddenIcon size={14} /> {hidden.label}
        </button>
      {/each}
      <div class="menu-sep"></div>
      <button type="button" role="menuitem" onclick={() => { closeTabMenu(); reopenAllStages(); }}>
        <RotateCcw size={14} /> Reopen all
      </button>
    {:else if !tabMenu.stageId}
      <div class="menu-empty">No closed tabs</div>
    {/if}
  </div>
{/if}

{#if leaveConfirmOpen}
  <ConfirmDialog
    title={leaveKind === "tune"
      ? "Discard Tune changes?"
      : leaveKind === "brief"
        ? "Discard Brief changes?"
        : "Discard Quests changes?"}
    message={leaveKind === "tune"
      ? "You have unsaved config edits. Leave Tune and discard them?"
      : leaveKind === "brief"
        ? "You have unsaved listing edits. Leave Brief and discard them?"
        : "You have unsaved quest edits. Leave Quests and discard them?"}
    danger={false}
    confirmLabel="Discard & leave"
    onconfirm={confirmLeaveStage}
    oncancel={cancelLeaveStage}
  />
{/if}

<style>
  .ide-workspace {
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-rows: minmax(0, 1fr) auto;
    position: relative;
  }

  .ide-workspace.auto-hide-rail {
    grid-template-rows: minmax(0, 1fr);
  }

  .skeleton-page {
    width: min(1120px, 100%);
    margin: 0 auto;
  }

  .stage-shell {
    min-width: 0;
    min-height: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    background:
      radial-gradient(circle at top right, color-mix(in srgb, var(--accent-primary) 6%, transparent), transparent 32%),
      rgba(255, 255, 255, 0.015);
    overflow: hidden;
  }

  .stage-content {
    flex: 1;
    min-width: 0;
    min-height: 0;
    width: 100%;
    overflow: auto;
    padding: 20px 24px;
    scrollbar-gutter: stable;
  }

  .stage-content.fill-stage {
    /* One scroll owner = child; avoid outer scrollbar over edge-to-edge content. */
    overflow: hidden;
    display: flex;
    flex-direction: column;
    padding: 0;
  }

  .stage-content.fill-stage > :global(.config-editor) {
    flex: 1;
    min-height: 0;
    height: 100%;
    padding: 16px 20px;
    box-sizing: border-box;
  }

  .stage-content.fill-stage > :global(.worlds-view) {
    flex: 1;
    min-height: 0;
  }

  .stage-content.fill-stage > :global(.qe.ftbq) {
    flex: 1;
    min-height: 0;
    height: 100%;
  }

  .stage-content.fill-stage > :global(.brief-editor),
  .stage-content.fill-stage > :global(.settings-page) {
    flex: 1;
    min-height: 0;
    height: 100%;
  }

  /* Priority stages: one primary canvas fills the stage (DaVinci-style work
     area). The stage only provides sizing — each canvas keeps its own
     centered max-width cap on 1440p+ (.mods 1680 / .graph 1840 /
     .diagnostics 1320 / .snapshots 1440). Do NOT set max-width/margin here:
     the previous `max-width: none; margin: 0` clobbered those caps and
     stretched every stage edge-to-edge across the window. */
  .stage-content.fill-stage > :global(.mods),
  .stage-content.fill-stage > :global(.graph),
  .stage-content.fill-stage > :global(.test-runs),
  .stage-content.fill-stage > :global(.diagnostics),
  .stage-content.fill-stage > :global(.recipe-workspace),
  .stage-content.fill-stage > :global(.snapshots) {
    flex: 1;
    min-height: 0;
    height: 100%;
    box-sizing: border-box;
  }

  /* Banner is a direct child of the zero-padding canvas stage — inset it so
     the bordered card does not hug the window edges. Document stages render
     inside .stage-content padding and need no override. */
  .stage-content.fill-stage > :global(.update-card) {
    margin: 12px 16px 0;
  }

  .stage-content.fill-stage > :global(.mods),
  .stage-content.fill-stage > :global(.test-runs),
  .stage-content.fill-stage > :global(.diagnostics),
  .stage-content.fill-stage > :global(.recipe-workspace) {
    display: flex;
    flex-direction: column;
    padding: 12px 16px;
    overflow: hidden;
  }

  /* Graph: long lists under the canvas — this root owns vertical scroll.
     Snapshots keep overflow:hidden (internal panes scroll). */
  .stage-content.fill-stage > :global(.graph) {
    display: flex;
    flex-direction: column;
    padding: 12px 16px;
    overflow-x: hidden;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .stage-content.fill-stage > :global(.snapshots) {
    display: flex;
    flex-direction: column;
    padding: 12px 16px;
    overflow: hidden;
  }

  .rail-hotzone {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    /* Visible handle when rail is hidden — 2px accent line hint that the
       workflow rail lives at the bottom. The 12px invisible zone is kept
       for hover, but the handle itself is a subtle line the user can see. */
    height: 14px;
    z-index: 6;
    background: linear-gradient(
      to top,
      color-mix(in srgb, var(--accent-primary) 10%, transparent) 0%,
      transparent 60%
    );
    border-top: 2px solid color-mix(in srgb, var(--accent-primary) 28%, transparent);
    opacity: 0.95;
    transition: opacity 0.16s ease, border-color 0.16s ease;
  }
  .ide-workspace.auto-hide-rail:has(.workflow-rail.revealed) .rail-hotzone {
    opacity: 0;
    pointer-events: none;
  }

  .workflow-rail {
    flex: 0 0 auto;
    display: flex;
    flex-wrap: wrap;
    align-items: stretch;
    gap: 8px;
    min-width: 0;
    padding: 10px 12px;
    overflow: visible;
    border-top: 1px solid var(--border-color);
    background: var(--bg-secondary);
    z-index: 5;
  }

  .ide-workspace.auto-hide-rail .workflow-rail {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    transform: translateY(calc(100% + 2px));
    opacity: 0;
    visibility: hidden;
    transition:
      transform 0.2s cubic-bezier(0.22, 1, 0.36, 1),
      opacity 0.16s ease,
      visibility 0s linear 0.2s;
    box-shadow: 0 -12px 32px rgba(0, 0, 0, 0.32);
    pointer-events: none;
    will-change: transform, opacity;
  }

  .ide-workspace.auto-hide-rail .workflow-rail.revealed {
    transform: translateY(0);
    opacity: 1;
    visibility: visible;
    pointer-events: auto;
    transition:
      transform 0.2s cubic-bezier(0.22, 1, 0.36, 1),
      opacity 0.14s ease,
      visibility 0s linear 0s;
  }

  .stage-tab {
    min-width: 0;
    min-height: 52px;
    flex: 1 1 auto;
    justify-content: flex-start;
    gap: 8px;
    padding: 9px 10px;
    background: transparent;
    color: var(--text-secondary);
    border: 1px solid transparent;
    position: relative;
  }

  .stage-tab:hover {
    transform: none;
    background: var(--bg-tertiary);
    border-color: color-mix(in srgb, var(--accent-primary) 25%, transparent);
    color: var(--text-primary);
  }

  .stage-tab.active {
    transform: none;
    background: color-mix(in srgb, var(--accent-primary) 14%, var(--bg-tertiary));
    border-color: color-mix(in srgb, var(--accent-primary) 45%, transparent);
    color: var(--text-primary);
    box-shadow:
      inset 0 -2px 0 var(--accent-primary),
      0 0 0 1px color-mix(in srgb, var(--accent-primary) 18%, transparent);
  }

  .stage-tab.active .stage-status {
    color: var(--accent-primary);
  }

  .stage-tab:focus-visible {
    outline: 2px solid var(--accent-primary);
    outline-offset: 2px;
  }

  .stage-status {
    display: grid;
    flex: 0 0 auto;
    place-items: center;
    color: var(--text-muted);
  }

  /* Shortcut badge — tiny number showing the Ctrl+chord, visible on hover/active
     and always for screen-reader discoverability via title. Keeps the rail
     scannable without adding a permanent label column. */
  .stage-chord {
    position: absolute;
    top: 4px;
    right: 6px;
    min-width: 14px;
    height: 14px;
    display: grid;
    place-items: center;
    padding: 0 3px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--accent-primary) 18%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 30%, transparent);
    color: var(--accent-primary);
    font: 700 12px/1 var(--font-mono, monospace);
    opacity: 0;
    transform: scale(0.9);
    transition:
      opacity 0.14s ease,
      transform 0.14s ease;
    pointer-events: none;
  }
  .stage-tab:hover .stage-chord,
  .stage-tab.active .stage-chord,
  .stage-tab:focus-visible .stage-chord {
    opacity: 1;
    transform: scale(1);
  }
  /* On wider rails the chord sits inline after the label to avoid overlap
     with the icon on narrow columns — but absolute is cleaner for the bottom
     rail, so keep it pinned to the corner across breakpoints. */
  @media (max-width: 720px) {
    .stage-chord {
      top: 2px;
      right: 4px;
      font-size: 12px;
      min-width: 14px;
      height: 14px;
    }
  }

  /* "+" reopen pill at the rail end — visible while tabs are closed. */
  .stage-add {
    flex: 0 0 auto;
    align-self: center;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    margin-left: 2px;
    padding: 0;
    border: 1px dashed color-mix(in srgb, var(--accent-primary) 45%, transparent);
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--accent-primary);
    cursor: pointer;
    transform: none;
  }
  .stage-add:hover {
    background: var(--bg-tertiary);
    border-style: solid;
  }

  /* Right-click menu for the rail tabs (opens upward, clamped). */
  .ide-ctx-menu {
    display: flex;
    flex-direction: column;
    min-width: 210px;
    max-height: min(320px, 46vh);
    overflow-y: auto;
    padding: 4px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-primary);
    box-shadow: 0 14px 36px rgba(0, 0, 0, 0.4);
  }
  .ide-ctx-menu button[role="menuitem"] {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 9px;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    text-align: left;
    cursor: pointer;
    white-space: nowrap;
    transform: none;
  }
  .ide-ctx-menu button[role="menuitem"]:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .ide-ctx-menu button[role="menuitem"]:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .ide-ctx-menu button[role="menuitem"].danger {
    color: var(--accent-danger);
  }
  .ide-ctx-menu .menu-sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--border-color);
  }
  .ide-ctx-menu .menu-title {
    padding: 3px 9px 4px;
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .ide-ctx-menu .menu-empty {
    padding: 7px 9px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .stage-text {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    min-width: 0;
    line-height: 1.1;
  }

  .stage-text strong,
  .stage-text small {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .stage-text small {
    color: var(--text-muted);
    font-size: 12px;
  }

  .skeleton-page {
    min-height: 100%;
  }

  .skeleton-page h2 {
    margin-bottom: 8px;
  }

  .skeleton-page p {
    color: var(--text-muted);
  }

  @media (max-width: 1100px) {
    .stage-content {
      padding: 16px;
    }

    /* Keep every stage tab visible (no horizontal scrollbar): show the short
       label + icon and drop the long descriptive name on narrower windows. */
    .stage-tab {
      justify-content: center;
      flex: 1 1 auto;
    }

    .stage-text strong {
      display: none;
    }
  }

  @media (max-width: 720px) {
    .workflow-rail {
      padding-inline: 8px;
    }

    .stage-tab {
      flex-direction: column;
      gap: 8px;
      padding-inline: 6px;
    }

    .stage-text small {
      font-size: 12px;
    }
  }
</style>
