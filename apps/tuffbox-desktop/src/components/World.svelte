<script lang="ts">
  import {
    Globe,
    Download,
    RefreshCw,
    Database,
    PanelLeftClose,
    PanelLeftOpen,
    Clipboard,
    Map,
    Eye,
    Layers,
  } from "@lucide/svelte";
  import { projectPath } from "../lib/store";
  import { api } from "../lib/api";
  import type { WorldListItem, WorldDetail } from "../lib/api";
  import WorldMap from "./WorldMap.svelte";
  import EmptyState from "./EmptyState.svelte";
  import { worldMapClipboard, clearWorldMapClipboard } from "../lib/worldMapClipboard";

  type MapMode = null | "builtin" | "mca";

  let worlds = $state<WorldListItem[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let selectedWorld = $state<string | null>(null);
  let worldDetail = $state<WorldDetail | null>(null);
  let detailLoading = $state(false);
  let backupMsg = $state<string | null>(null);
  let mcaMsg = $state<string | null>(null);
  let mcaOpening = $state(false);
  let railOpen = $state(true);
  let mapMode = $state<MapMode>(null);

  const worldMetaTitle = $derived(
    !worldDetail || !selectedWorld
      ? (selectedWorld ?? "")
      : [
          `Seed ${worldDetail.seed}`,
          gameTypeLabel(worldDetail.gameType),
          difficultyLabel(worldDetail.difficulty),
          worldDetail.sizeFormatted,
          formatTime(worldDetail.time || 0),
          `Spawn ${worldDetail.spawnX}, ${worldDetail.spawnY}, ${worldDetail.spawnZ}`,
        ].join(" · "),
  );

  const modeLabel = $derived(
    mapMode === "builtin" ? "Built-in viewer" : mapMode === "mca" ? "MCA Selector" : "Choose mode",
  );

  function setMapMode(mode: MapMode) {
    mapMode = mode;
  }

  async function loadWorlds() {
    const p = $projectPath;
    if (!p) return;
    loading = true;
    error = null;
    try {
      worlds = await api.worlds.list(p);
      if (worlds.length > 0 && !selectedWorld) {
        selectWorld(worlds[0].name);
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function selectWorld(name: string) {
    const p = $projectPath;
    if (!p) return;
    selectedWorld = name;
    railOpen = false;
    worldDetail = null;
    detailLoading = true;
    // Always offer the chooser when opening / switching worlds.
    mapMode = null;
    try {
      worldDetail = await api.worlds.readInfo(name, p);
      error = null;
    } catch (e) {
      worldDetail = null;
      error = String(e);
    } finally {
      detailLoading = false;
    }
  }

  async function backupWorld() {
    const p = $projectPath;
    if (!selectedWorld || !p) return;
    try {
      const path = await api.worlds.backup(selectedWorld, p);
      backupMsg = `Backed up to ${path.split(/[\\/]/).pop()}`;
      setTimeout(() => (backupMsg = null), 3000);
    } catch {
      backupMsg = null;
    }
  }

  async function openMcaSelector() {
    const p = $projectPath;
    if (!selectedWorld || !p || mcaOpening) return;
    mcaOpening = true;
    mcaMsg = null;
    error = null;
    try {
      await api.worlds.openMcaSelector(selectedWorld, p);
      mcaMsg = "MCA Selector launched — File → Open Recent";
      setTimeout(() => (mcaMsg = null), 5000);
    } catch (e) {
      error = String(e);
    } finally {
      mcaOpening = false;
    }
  }

  function chooseBuiltin() {
    setMapMode("builtin");
  }

  async function chooseMca() {
    setMapMode("mca");
    await openMcaSelector();
  }

  function changeMode() {
    mapMode = null;
  }

  function useBuiltinFromMca() {
    setMapMode("builtin");
  }

  function gameTypeLabel(t: string | number): string {
    if (typeof t === "string" && t.length > 0 && isNaN(Number(t))) return t;
    const n = typeof t === "number" ? t : Number(t);
    switch (n) {
      case 0: return "Survival";
      case 1: return "Creative";
      case 2: return "Adventure";
      case 3: return "Spectator";
      default: return String(t ?? "—");
    }
  }

  function difficultyLabel(d: string | number): string {
    if (typeof d === "string" && d.length > 0 && isNaN(Number(d))) return d;
    const n = typeof d === "number" ? d : Number(d);
    switch (n) {
      case 0: return "Peaceful";
      case 1: return "Easy";
      case 2: return "Normal";
      case 3: return "Hard";
      default: return String(d ?? "—");
    }
  }

  function formatTime(ticks: number): string {
    const totalMinutes = Math.floor(ticks / 1200);
    const days = Math.floor(totalMinutes / 1440);
    const hours = Math.floor((totalMinutes % 1440) / 60);
    const mins = totalMinutes % 60;
    if (days > 0) return `${days}d ${hours}h ${mins}m`;
    if (hours > 0) return `${hours}h ${mins}m`;
    return `${mins}m`;
  }

  $effect(() => {
    if ($projectPath) {
        loadWorlds();
      }
  });
</script>

<div class="worlds-view" class:rail-collapsed={!railOpen}>
  <aside class="worlds-rail" class:open={railOpen}>
    <div class="rail-header">
      <Globe size={16} />
      {#if railOpen}
        <span>Worlds</span>
        <button class="icon-btn" type="button" onclick={loadWorlds} disabled={loading} title="Refresh">
          <RefreshCw size={13} class={loading ? "spin" : ""} />
        </button>
      {/if}
      <button
        class="icon-btn rail-toggle"
        type="button"
        title={railOpen ? "Collapse world list" : "Expand world list"}
        onclick={() => (railOpen = !railOpen)}
      >
        {#if railOpen}
          <PanelLeftClose size={14} />
        {:else}
          <PanelLeftOpen size={14} />
        {/if}
      </button>
    </div>

    {#if railOpen}
      <div class="world-list">
        {#if !$projectPath}
          <EmptyState
            icon={Globe}
            title="No project open"
            description="Open a pack first, then worlds from its saves folder will appear here."
          />
        {:else if error && worlds.length === 0 && !loading}
          <EmptyState
            icon={Globe}
            title="Couldn’t list worlds"
            description={error}
          />
        {:else if worlds.length === 0 && !loading}
          <EmptyState
            icon={Globe}
            title="No worlds found"
            description="Generate a world by launching the game, then refresh."
          />
        {:else}
          {#each worlds as w (w.name)}
            <button
              class="world-item"
              class:active={selectedWorld === w.name}
              type="button"
              onclick={() => selectWorld(w.name)}
            >
              <div class="world-icon"><Database size={14} /></div>
              <div class="world-info">
                <span class="world-name">{w.name}</span>
                <span class="world-meta">{w.sizeFormatted}</span>
              </div>
            </button>
          {/each}
        {/if}
      </div>
    {/if}
  </aside>

  <div class="worlds-main">
    {#if selectedWorld}
      <div class="compact-bar">
        <div class="compact-left">
          <strong class="world-title" title={worldMetaTitle}>{selectedWorld}</strong>
          {#if detailLoading}
            <span class="meta-muted"><RefreshCw size={11} class="spin" /> Loading…</span>
          {/if}
          {#if mapMode}
            <span class="meta-pill mode-pill" title="Current world map mode">{modeLabel}</span>
          {/if}
        </div>
        <div class="compact-right">
          {#if $worldMapClipboard}
            <span
              class="meta-pill clip-pill"
              class:cross={$worldMapClipboard.sourceWorld !== selectedWorld}
              title={
                $worldMapClipboard.sourceWorld !== selectedWorld
                  ? "Clipboard from another world — use Paste on the map (Ctrl+V)"
                  : "Shared MCA clipboard"
              }
            >
              <Clipboard size={10} />
              {#if $worldMapClipboard.sourceWorld !== selectedWorld}
                Paste {$worldMapClipboard.clipboard.chunks.length} from {$worldMapClipboard.sourceWorld}
              {:else}
                {$worldMapClipboard.clipboard.chunks.length} copied
              {/if}
              <button
                type="button"
                class="clip-clear"
                title="Clear clipboard"
                onclick={clearWorldMapClipboard}
              >×</button>
            </span>
          {/if}
          {#if backupMsg}<span class="backup-msg">{backupMsg}</span>{/if}
          {#if mcaMsg}<span class="backup-msg">{mcaMsg}</span>{/if}
          {#if mapMode}
            <button
              class="mode-change"
              type="button"
              onclick={changeMode}
              title="Choose how to open the world map"
            >
              <Layers size={12} />
              Change mode
            </button>
          {/if}
          <button class="ghost" type="button" onclick={backupWorld} title="Backup this world">
            <Download size={13} /> Backup
          </button>
        </div>
      </div>

      <div class="map-stage">
        {#if mapMode === null}
          <div class="mode-chooser" role="dialog" aria-labelledby="map-mode-title">
            <div class="mode-chooser-inner">
              <h2 id="map-mode-title" class="mode-chooser-title">How do you want to open the world map?</h2>
              <div class="mode-cards">
                <button type="button" class="mode-card" onclick={chooseBuiltin}>
                  <span class="mode-card-icon"><Eye size={22} /></span>
                  <span class="mode-card-title">Built-in viewer</span>
                  <span class="mode-card-sub">View-only map inside TuffBox</span>
                </button>
                <button
                  type="button"
                  class="mode-card"
                  disabled={mcaOpening}
                  onclick={chooseMca}
                >
                  <span class="mode-card-icon"><Map size={22} /></span>
                  <span class="mode-card-title">MCA Selector</span>
                  <span class="mode-card-sub">Full Querz editor (bundled, no download)</span>
                  {#if mcaOpening}
                    <span class="mode-card-busy">
                      <RefreshCw size={12} class="spin" /> Opening…
                    </span>
                  {/if}
                </button>
              </div>
            </div>
          </div>
        {:else if mapMode === "builtin"}
          <WorldMap worldName={selectedWorld} layout="dock" readOnly={true} />
        {:else}
          <div class="mca-status-panel">
            <div class="mca-status-card">
              <Map size={28} />
              <h3>MCA Selector launched</h3>
              <p>
                The bundled Querz MCA Selector should be open with this world.
                If it isn’t focused, use <strong>File → Open Recent</strong> in MCA Selector.
              </p>
              {#if mcaOpening}
                <p class="mca-status-busy"><RefreshCw size={13} class="spin" /> Opening…</p>
              {/if}
              <div class="mca-status-actions">
                <button
                  type="button"
                  class="mca-relaunch"
                  disabled={mcaOpening}
                  onclick={openMcaSelector}
                >
                  {#if mcaOpening}
                    <RefreshCw size={13} class="spin" /> Opening…
                  {:else}
                    <Map size={13} /> Open MCA Selector again
                  {/if}
                </button>
                <button type="button" class="ghost" onclick={useBuiltinFromMca}>
                  <Eye size={13} /> Use built-in viewer
                </button>
                <button type="button" class="ghost" onclick={changeMode}>
                  Choose again
                </button>
              </div>
            </div>
          </div>
        {/if}
      </div>
    {:else}
      <EmptyState
        icon={Globe}
        title="Select a world"
        description="Choose a world from the rail to open the MCA map."
      />
    {/if}
    {#if error}
      <div class="err-banner">{error}</div>
    {/if}
  </div>
</div>

<style>
  .worlds-view {
    height: 100%;
    min-height: 0;
    flex: 1;
    display: flex;
    overflow: hidden;
    /* Responsive: centered cap on 1440p+. */
    max-width: min(1720px, 100%);
    margin: 0 auto;
    width: 100%;
  }

  .worlds-rail {
    width: 44px;
    flex-shrink: 0;
    background: color-mix(in srgb, var(--bg-secondary) 94%, var(--bg-primary) 6%);
    border-right: 1px solid var(--border-color);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    transition: width 0.2s var(--ease-out, ease);
  }
  .worlds-rail.open {
    width: 208px;
  }

  .rail-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    font-weight: 700;
    font-size: 12px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-muted);
    border-bottom: 1px solid var(--border-color);
    min-height: 40px;
  }
  .rail-header span { flex: 1; color: var(--text-primary); text-transform: none; letter-spacing: 0; font-size: 12px; }
  .icon-btn {
    background: transparent;
    border: 1px solid transparent;
    border-radius: 7px;
    padding: 5px;
    cursor: pointer;
    color: var(--text-muted);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .icon-btn:hover { background: var(--bg-hover); color: var(--text-primary); border-color: var(--border-color); }
  .rail-toggle { margin-left: auto; }

  .world-list {
    flex: 1;
    overflow-y: auto;
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .world-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px;
    border: 1px solid transparent;
    border-radius: var(--border-radius-sm);
    background: transparent;
    cursor: pointer;
    text-align: left;
    width: 100%;
    transition: background 120ms ease, border-color 120ms ease;
  }
  .world-item:hover { background: var(--bg-hover); }
  .world-item.active {
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    border-color: color-mix(in srgb, var(--accent-primary) 28%, transparent);
  }
  .world-item.active .world-name { color: var(--accent-primary); }
  .world-icon {
    width: 28px;
    height: 28px;
    border-radius: 7px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    color: var(--text-secondary);
  }
  .world-item.active .world-icon {
    color: var(--accent-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 30%, var(--border-color));
  }
  .world-info { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 8px; }
  .world-name {
    font-weight: 600;
    font-size: 12px;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .world-meta { font-size: 12px; color: var(--text-muted); font-variant-numeric: tabular-nums; }

  .worlds-main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--bg-primary);
  }

  .compact-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 5px 12px;
    border-bottom: 1px solid var(--border-color);
    background: color-mix(in srgb, var(--bg-secondary) 96%, var(--bg-primary) 4%);
    flex-shrink: 0;
    flex-wrap: nowrap;
    min-height: 36px;
  }
  .compact-left {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    min-width: 0;
  }
  .world-title {
    font-size: 13px;
    font-weight: 700;
    color: var(--text-primary);
    letter-spacing: -0.01em;
    margin-right: 4px;
  }
  .meta-pill {
    min-width: 0;
    line-height: 1.4;
    vertical-align: baseline;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-secondary);
    padding: 3px 8px;
    border-radius: 6px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mode-pill {
    max-width: 160px;
    color: var(--accent-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 30%, var(--border-color));
    background: color-mix(in srgb, var(--accent-primary) 10%, var(--bg-tertiary));
  }
  .meta-muted {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .compact-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .backup-msg {
    font-size: 12px;
    color: var(--accent-primary);
  }
  .clip-pill {
    color: var(--accent-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 35%, var(--border-color));
    background: color-mix(in srgb, var(--accent-primary) 10%, var(--bg-tertiary));
    max-width: 280px;
  }
  .clip-pill.cross {
    animation: clip-pulse 1.6s ease-in-out 2;
  }
  @keyframes clip-pulse {
    0%, 100% { border-color: color-mix(in srgb, var(--accent-primary) 35%, var(--border-color)); }
    50% { border-color: color-mix(in srgb, var(--accent-primary) 70%, var(--border-color)); }
  }
  .clip-clear {
    margin-left: 2px;
    border: none;
    background: transparent;
    color: inherit;
    cursor: pointer;
    font-size: 14px;
    line-height: 1;
    padding: 0 2px;
    opacity: 0.7;
  }
  .clip-clear:hover { opacity: 1; }
  .ghost {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }

  .mode-change {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    font-weight: 600;
    padding: 4px 8px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    cursor: pointer;
    white-space: nowrap;
    transition: background 120ms ease, border-color 120ms ease, color 120ms ease;
  }
  .mode-change:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 35%, var(--border-color));
  }

  .map-stage {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    position: relative;
  }
  .map-stage > :global(.world-map) {
    flex: 1;
    min-height: 0;
    min-width: 0;
  }

  .mode-chooser {
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background:
      radial-gradient(ellipse 70% 50% at 50% 40%, color-mix(in srgb, var(--accent-primary) 8%, transparent), transparent 70%),
      var(--bg-primary);
  }
  .mode-chooser-inner {
    width: min(560px, 100%);
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  .mode-chooser-title {
    margin: 0;
    text-align: center;
    font-size: 16px;
    font-weight: 700;
    color: var(--text-primary);
    letter-spacing: -0.01em;
  }
  .mode-cards {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  @media (max-width: 560px) {
    .mode-cards { grid-template-columns: 1fr; }
  }
  .mode-card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    text-align: left;
    padding: 18px 16px;
    border-radius: var(--border-radius-lg);
    border: 1px solid var(--border-color);
    background: color-mix(in srgb, var(--bg-secondary) 92%, var(--bg-primary) 8%);
    color: var(--text-primary);
    cursor: pointer;
    transition: background 140ms ease, border-color 140ms ease, transform 140ms ease;
  }
  .mode-card:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-primary) 10%, var(--bg-secondary));
    border-color: color-mix(in srgb, var(--accent-primary) 40%, var(--border-color));
    transform: translateY(-1px);
  }
  .mode-card:disabled {
    opacity: 0.7;
    cursor: wait;
  }
  .mode-card-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    border-radius: var(--border-radius-md);
    background: color-mix(in srgb, var(--accent-primary) 14%, var(--bg-tertiary));
    border: 1px solid color-mix(in srgb, var(--accent-primary) 28%, var(--border-color));
    color: var(--accent-primary);
    margin-bottom: 4px;
  }
  .mode-card-title {
    font-size: 14px;
    font-weight: 700;
  }
  .mode-card-sub {
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.35;
  }
  .mode-card-busy {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
    font-size: 12px;
    color: var(--accent-primary);
  }

  .mca-status-panel {
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: var(--bg-primary);
  }
  .mca-status-card {
    width: min(420px, 100%);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    text-align: center;
    padding: 28px 24px;
    border-radius: var(--border-radius-lg);
    border: 1px solid var(--border-color);
    background: color-mix(in srgb, var(--bg-secondary) 94%, var(--bg-primary) 6%);
    color: var(--text-secondary);
  }
  .mca-status-card h3 {
    margin: 0;
    font-size: 15px;
    font-weight: 700;
    color: var(--text-primary);
  }
  .mca-status-card p {
    margin: 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--text-muted);
  }
  .mca-status-busy {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--accent-primary) !important;
  }
  .mca-status-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    justify-content: center;
    margin-top: 8px;
  }
  .mca-relaunch {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    font-weight: 600;
    padding: 6px 12px;
    border-radius: var(--border-radius-sm);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 40%, var(--border-color));
    background: color-mix(in srgb, var(--accent-primary) 16%, var(--bg-tertiary));
    color: var(--text-primary);
    cursor: pointer;
    white-space: nowrap;
  }
  .mca-relaunch:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-primary) 28%, var(--bg-tertiary));
  }
  .mca-relaunch:disabled {
    opacity: 0.65;
    cursor: wait;
  }

  .err-banner {
    padding: 6px 10px;
    font-size: 12px;
    color: color-mix(in srgb, var(--accent-danger) 75%, #fff 25%);
    background: color-mix(in srgb, var(--accent-danger) 12%, transparent);
    border-top: 1px solid color-mix(in srgb, var(--accent-danger) 30%, transparent);
  }

  :global(.spin) {
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
