<script lang="ts">
  import { FolderOpen, ChevronRight, Terminal } from "@lucide/svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { onMount, onDestroy } from "svelte";
  import { fly } from "svelte/transition";
  import { quintOut } from "svelte/easing";
  import { projectPath, projectInfo, openLaunchLog, recentProjects, launcherSettingsLive } from "../lib/store";
  import { toasts } from "../lib/toast";
  import { api } from "../lib/api";

  import type { View } from "../lib/types";

  let { currentView = $bindable(), children = null }: { currentView: View; children?: any } = $props();

  let onlineCount = $state(0);
  let onlineOk = $state(false);
  let onlineTimer: ReturnType<typeof setInterval> | null = null;

  async function refreshOnline() {
    try {
      if (!isTauri()) {
        onlineOk = false;
        onlineCount = 0;
        return;
      }
      const stats: any = await invoke("get_launcher_online");
      onlineCount = Number(stats?.onlineCount ?? 0);
      onlineOk = true;
    } catch {
      onlineOk = false;
    }
  }

  onMount(() => {
    void refreshOnline();
    // Cosmetic "online now" counter: 30s cadence, and no Supabase
    // round-trip at all while the window is minimized / occluded.
    onlineTimer = setInterval(() => {
      if (document.hidden) return;
      void refreshOnline();
    }, 30000);
  });

  onDestroy(() => {
    if (onlineTimer) clearInterval(onlineTimer);
  });

  const titles: Record<View, string> = {
    dashboard: "Home",
    ide: "IDE Workflow",
    mods: "Mods",
    graph: "Dependency Graph",
    world: "World · MCA map",
    library: "Library",
    chats: "Chats",
    diagnostics: "Health Check",
    "crash-votes": "Crash Votes",
    snapshots: "Snapshot History",
    configs: "Config Editor",
    settings: "Settings",
    "project-settings": "Instance Settings",
    "ore-gen": "Ore Generation",
    recipes: "Recipe Browser",
    quests: "Quest Editor",
    me: "Me",
  };

  /** The pack name lives in the Home hero — the header just names the section. */
  const pageTitle = $derived.by(() => {
    const mcHome =
      $launcherSettingsLive?.theme === "minecraft" &&
      (currentView === "dashboard" || currentView === "library" || currentView === "me");
    if (mcHome) return "Minecraft: Java Edition";
    return titles[currentView] ?? "";
  });

  function prefersReducedMotion(): boolean {
    if (typeof document === "undefined") return true;
    if (document.documentElement.classList.contains("potato-pc")) return true;
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  function titleIntro(node: Element) {
    if (prefersReducedMotion()) return { duration: 0 };
    return fly(node, { y: 12, duration: 320, opacity: 0, easing: quintOut });
  }

  async function selectProject() {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "TuffBox Project", extensions: ["tuffbox.json"] }],
    });
    if (selected && typeof selected === "string") {
      try {
        const info = await invoke("validate_project", { path: selected }) as import("../lib/api").ProjectSummary;
        const manifestPath = info.manifestPath || selected;
        recentProjects.add({ path: manifestPath, info: info as any }, { reorder: false });
        projectPath.set(manifestPath);
        projectInfo.set(info as any);
        void api.session.setLastOpened(manifestPath).catch(() => {});
      } catch (e) {
        toasts.error(String(e));
      }
    }
  }
</script>

<header class="header" data-view={currentView}>
  <div class="header-top">
  <div class="left">
    {#key currentView + ($launcherSettingsLive?.theme ?? "")}
      <div class="title-swap" in:titleIntro>
        <div class="breadcrumb">
          <span class="crumb">TuffBox</span>
          <ChevronRight size={14} class="separator" />
          <span class="crumb active">{titles[currentView]}</span>
        </div>
        <h1 class="page-title">{pageTitle}</h1>
      </div>
    {/key}
  </div>

  <div class="right">
    {@render children?.()}
    <div
      class="online-chip"
      class:live={onlineOk}
      title={onlineOk
        ? "Users with TuffBox open right now"
        : isTauri()
          ? "Community presence unavailable (no network / Supabase)"
          : "Presence requires the Tauri app (browser preview is offline)"}
    >
      <span class="online-dot" class:on={onlineOk && onlineCount > 0}></span>
      <span class="online-label">{onlineOk ? onlineCount : "—"}</span>
      <span class="online-hint">{onlineOk ? "online" : isTauri() ? "offline" : "preview"}</span>
    </div>

    <button class="secondary switch-quiet" onclick={selectProject}>
      <FolderOpen size={16} />
      {$projectPath ? "Switch" : "Open"}
    </button>

    {#if currentView !== "dashboard" && currentView !== "library" && currentView !== "me"}
      <button
        class="secondary"
        disabled={!$projectPath}
        title="Live logs of the running build"
        onclick={() => $projectPath && openLaunchLog($projectPath)}
      >
        <Terminal size={16} />
        Logs
      </button>
    {/if}
  </div>
  </div>
  <nav class="mc-tabs" aria-label="Launcher sections">
    <button
      type="button"
      class={["mc-tab", { active: currentView === "dashboard" }]}
      onclick={() => (currentView = "dashboard")}
    >
      Play
    </button>
    <button
      type="button"
      class={["mc-tab", { active: currentView === "library" }]}
      onclick={() => (currentView = "library")}
    >
      Installations
    </button>
    <button
      type="button"
      class={["mc-tab", { active: currentView === "me" }]}
      onclick={() => (currentView = "me")}
    >
      Skins
    </button>
  </nav>
</header>

<style>
  .header {
    height: 72px;
    padding: 0 32px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--header-border, var(--border-color));
    background: var(--header-bg, rgba(18, 18, 20, 0.8));
    -webkit-backdrop-filter: var(--header-backdrop, blur(12px));
    backdrop-filter: var(--header-backdrop, blur(12px));
    /* Full-width top bar — square corners in every theme / corner mode. */
    border-radius: 0;
    flex-shrink: 0;
  }

  .header-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex: 1;
    min-width: 0;
    gap: 16px;
    /* Match the home backdrop panel (Dashboard .home): on wide windows the
       chips sit flush with the panel's edge instead of drifting to the
       window border. Keep both max-widths in sync. */
    max-width: 1520px;
    margin: 0 auto;
  }

  .mc-tabs {
    display: none;
  }

  .left {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
    position: relative;
  }

  .title-swap {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .breadcrumb {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .breadcrumb :global(.separator) {
    color: var(--text-muted);
  }

  .crumb.active {
    color: var(--text-secondary);
  }

  .page-title {
    font-size: 24px;
    font-weight: 800;
    letter-spacing: -0.3px;
    line-height: 1.15;
    margin: 0;
    max-width: 52vw;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .right {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .online-chip {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    background: rgba(255, 255, 255, 0.03);
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 600;
    user-select: none;
  }
  .online-chip.live {
    color: var(--text-secondary);
  }
  .online-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-muted);
    box-shadow: none;
  }
  .online-dot.on {
    background: var(--accent-primary);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent-primary) 22%, transparent);
  }
  .online-label {
    font-variant-numeric: tabular-nums;
    color: var(--text-primary);
    min-width: 1ch;
  }

  /* Switch/Open project: quiet ghost — visible affordance, not a loud CTA. */
  .switch-quiet {
    background: transparent;
    border-color: transparent;
    color: var(--text-muted);
    font-weight: 500;
    box-shadow: none;
    text-shadow: none;
    transition:
      color var(--motion-fast, 160ms) ease,
      background var(--motion-fast, 160ms) ease;
  }
  .switch-quiet:hover:not(:disabled) {
    background: var(--bg-hover, color-mix(in srgb, var(--text-primary) 8%, transparent));
    border-color: transparent;
    color: var(--text-secondary);
  }
  .switch-quiet:active:not(:disabled) {
    background: var(--bg-active, color-mix(in srgb, var(--text-primary) 12%, transparent));
  }
  .online-hint {
    text-transform: lowercase;
    letter-spacing: 0.02em;
  }

  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  button:disabled:hover {
    transform: none;
    background: inherit;
  }
</style>
