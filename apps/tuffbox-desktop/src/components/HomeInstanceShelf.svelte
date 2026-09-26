<script lang="ts">
  import { Library, Plus, FolderInput } from "@lucide/svelte";
  import {
    recentProjects,
    runningInstances,
    isProjectRunning,
    openAddInstance,
    type RecentProject,
  } from "../lib/store";
  import { homeIcons } from "../lib/homeBootstrap";
  import { t } from "../lib/i18n";
  import HomeYoutubePlacementToggle from "./HomeYoutubePlacementToggle.svelte";

  let {
    selectedPath = null,
    potato = false,
    showPlacementToggle = false,
    onselect,
    onlibrary,
  }: {
    selectedPath?: string | null;
    potato?: boolean;
    showPlacementToggle?: boolean;
    onselect?: (path: string) => void;
    onlibrary?: () => void;
  } = $props();

  const icons = $derived($homeIcons);
  const packCount = $derived($recentProjects.length);

  function themeGradient(name: string): [string, string] {
    const pairs: [string, string][] = [
      [
        "color-mix(in srgb, var(--accent-primary) 32%, var(--bg-primary))",
        "color-mix(in srgb, var(--accent-primary) 82%, var(--bg-secondary))",
      ],
      [
        "color-mix(in srgb, var(--accent-secondary) 28%, var(--bg-primary))",
        "color-mix(in srgb, var(--accent-primary) 72%, var(--bg-tertiary))",
      ],
      [
        "color-mix(in srgb, var(--accent-primary) 18%, var(--bg-secondary))",
        "color-mix(in srgb, var(--accent-hover) 78%, var(--bg-primary))",
      ],
      [
        "color-mix(in srgb, var(--bg-tertiary) 45%, var(--accent-primary))",
        "color-mix(in srgb, var(--accent-primary) 88%, var(--accent-secondary))",
      ],
      [
        "color-mix(in srgb, var(--accent-secondary) 22%, var(--bg-primary))",
        "color-mix(in srgb, var(--accent-secondary) 65%, var(--accent-primary))",
      ],
    ];
    let hash = 0;
    for (let i = 0; i < name.length; i++) hash = name.charCodeAt(i) + ((hash << 5) - hash);
    return pairs[Math.abs(hash) % pairs.length];
  }

  function letter(project: RecentProject): string {
    return (project.info.name?.[0] ?? "?").toUpperCase();
  }
</script>

<section class="shelf glass-panel" class:potato>
  <header class="shelf-head">
    <h2>
      Modpacks
      {#if packCount > 0}
        <span class="shelf-count">{packCount}</span>
      {/if}
    </h2>
    <div class="shelf-actions">
      {#if showPlacementToggle}
        <HomeYoutubePlacementToggle compact={false} />
      {/if}
      <button type="button" class="text-btn" onclick={() => onlibrary?.()}>
        <Library size={14} />
        All in Library
      </button>
    </div>
  </header>

  {#if packCount === 0}
    <div class="shelf-empty">
      <div class="ghost-row" aria-hidden="true">
        <span class="ghost-pack"></span>
        <span class="ghost-pack"></span>
        <span class="ghost-pack"></span>
      </div>
      <p>{$t("home.shelfEmpty")}</p>
      <div class="shelf-empty-actions">
        <button type="button" class="text-btn accent" onclick={() => openAddInstance("blank")}>
          <Plus size={14} />
          {$t("home.create")}
        </button>
        <button type="button" class="text-btn" onclick={() => openAddInstance("import")}>
          <FolderInput size={14} />
          {$t("home.import")}
        </button>
      </div>
    </div>
  {:else}
    <div class="shelf-grid tb-stagger">
      {#each $recentProjects as project, i (project.path)}
        {@const icon = icons[project.path]}
        {@const running = isProjectRunning(project.path, $runningInstances)}
        {@const [g0, g1] = themeGradient(project.info.name)}
        <button
          type="button"
          class="pack-tile"
          class:selected={project.path === selectedPath}
          class:running
          style:--i={i}
          title={project.info.name}
          onclick={() => onselect?.(project.path)}
          aria-current={project.path === selectedPath ? "true" : undefined}
        >
          <span class="pack-icon-wrap">
            <span
              class="pack-icon"
              class:has-icon={!!icon}
              style={icon ? undefined : `background: linear-gradient(135deg, ${g0}, ${g1})`}
            >
              {#if icon}
                <img src={icon} alt="" draggable="false" />
              {:else}
                <span class="pack-letter">{letter(project)}</span>
              {/if}
            </span>
            {#if running}
              <span class="running-dot" title={$t("home.running")}></span>
            {/if}
          </span>
          <span class="pack-name">{project.info.name}</span>
        </button>
      {/each}
    </div>
  {/if}
</section>

<style>
  .shelf {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 18px 16px 16px;
    border-radius: var(--border-radius-xl);
  }
  .shelf-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .shelf-head h2 {
    margin: 0;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    font-weight: 700;
    letter-spacing: 0.02em;
    color: var(--text-secondary);
  }
  .shelf-count {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 1.4em;
    padding: 1px 7px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 700;
    color: var(--text-muted);
    background: color-mix(in srgb, var(--bg-hover) 80%, transparent);
    border: 1px solid color-mix(in srgb, var(--glass-border) 70%, transparent);
  }
  .shelf-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .text-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 5px 10px;
    border: 0;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .text-btn:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }
  .text-btn.accent {
    color: var(--accent-primary);
  }
  .shelf-empty {
    display: grid;
    justify-items: start;
    gap: 10px;
    padding: 4px 0 6px;
  }
  .ghost-row {
    display: flex;
    gap: 12px;
  }
  .ghost-pack {
    width: 72px;
    height: 72px;
    border-radius: 50%;
    border: 1px dashed color-mix(in srgb, var(--text-muted) 35%, transparent);
    background:
      radial-gradient(circle at 35% 28%, color-mix(in srgb, #fff 12%, transparent), transparent 46%),
      color-mix(in srgb, var(--bg-tertiary) 70%, transparent);
  }
  .shelf-empty p {
    margin: 0;
    font-size: 13px;
    color: var(--text-muted);
  }
  .shelf-empty-actions {
    display: flex;
    gap: 8px;
  }
  .shelf-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 16px 14px;
    padding-bottom: 4px;
  }
  .shelf-grid::after {
    content: "";
    flex: 1 0 100%;
    height: 1px;
    background: linear-gradient(
      90deg,
      transparent,
      color-mix(in srgb, var(--glass-highlight) 70%, transparent) 12%,
      color-mix(in srgb, var(--glass-border) 90%, transparent) 50%,
      transparent
    );
  }
  .pack-tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    width: 104px;
    padding: 6px 2px 4px;
    border: 0;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    border-radius: var(--border-radius-md);
  }
  .pack-tile:hover .pack-icon-wrap {
    transform: translateY(-3px);
  }
  .pack-tile:hover .pack-icon {
    /* No `filter` on hover. The shelf is a .glass-panel (backdrop-filter);
       a hover-toggled filter on a descendant forces Chromium/WebView2 to
       re-snapshot the panel's backdrop, so the glass background visibly
       flickers/jumps on every hover — only when transparency is on (that is
       when the panel actually blurs). The ring + shadow below are
       compositing-only and give the same affordance without a filter. */
    box-shadow:
      0 10px 22px color-mix(in srgb, var(--bg-primary) 45%, transparent),
      0 0 0 1px color-mix(in srgb, var(--accent-primary) 38%, transparent);
  }
  .pack-tile.selected .pack-icon {
    box-shadow:
      0 0 0 2px color-mix(in srgb, var(--accent-primary) 72%, transparent),
      0 8px 22px color-mix(in srgb, var(--accent-primary) 32%, transparent);
  }
  .pack-tile.selected .pack-name {
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 16%, transparent);
    border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }
  .pack-tile:hover:not(.selected) .pack-name {
    background: color-mix(in srgb, var(--bg-hover) 90%, transparent);
    color: var(--text-primary);
  }
  /* Transparency modes: the 90%-opaque pill above wipes the glass out under
     the hovered icon ("background turns solid on hover"). Keep the highlight
     but let the glass/acrylic show through. Sharp themes keep solid fills —
     glass contradicts their identity (same rule as themes.css). */
  :global(html[data-glass="on"]) .pack-tile:hover:not(.selected) .pack-name,
  :global(html.glass-os) .pack-tile:hover:not(.selected) .pack-name {
    background: color-mix(in srgb, var(--bg-hover) 46%, transparent);
  }
  :global(html[data-glass="on"]:is([data-theme="win95"], [data-theme="pixelato"]))
    .pack-tile:hover:not(.selected)
    .pack-name {
    background: var(--bg-hover);
  }
  .pack-tile:focus-visible {
    outline: 2px solid var(--accent-primary);
    outline-offset: 2px;
  }
  .pack-icon-wrap {
    position: relative;
    width: 72px;
    height: 72px;
    flex-shrink: 0;
    transition: transform var(--motion-fast, 160ms) var(--ease-spring, ease);
    /* Own compositor layer: the hover lift animates without repainting the
       glass panel's backdrop (extra flicker guard for WebView2). */
    will-change: transform;
  }
  .pack-icon {
    position: relative;
    width: 72px;
    height: 72px;
    border-radius: 50%;
    overflow: hidden;
    display: grid;
    place-items: center;
    box-shadow: 0 6px 14px color-mix(in srgb, var(--bg-primary) 35%, transparent);
    transition:
      box-shadow var(--motion-fast, 160ms) var(--ease-out, ease);
  }
  .pack-icon::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: linear-gradient(
      160deg,
      color-mix(in srgb, #fff 30%, transparent) 0%,
      transparent 42%
    );
    pointer-events: none;
  }
  .pack-icon.has-icon {
    background: transparent;
  }
  .pack-icon img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    pointer-events: none;
  }
  .pack-letter {
    font-weight: 900;
    font-size: 26px;
    line-height: 1;
    text-transform: uppercase;
    color: color-mix(in srgb, var(--accent-primary) 18%, var(--text-primary));
    text-shadow: 0 1px 2px color-mix(in srgb, var(--bg-primary) 55%, transparent);
  }
  .running-dot {
    position: absolute;
    right: 0;
    bottom: 0;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: var(--accent-primary);
    border: 2px solid var(--bg-primary);
    box-shadow: 0 0 8px color-mix(in srgb, var(--accent-primary) 55%, transparent);
    pointer-events: none;
    animation: shelf-dot-pulse 1.8s ease-out infinite;
  }
  @keyframes shelf-dot-pulse {
    0% {
      box-shadow: 0 0 0 0 color-mix(in srgb, var(--accent-primary) 50%, transparent);
    }
    70% {
      box-shadow: 0 0 0 8px transparent;
    }
    100% {
      box-shadow: 0 0 0 0 transparent;
    }
  }
  .pack-name {
    font-size: 12px;
    font-weight: 600;
    line-height: 1.3;
    text-align: center;
    max-width: 100%;
    /* Clamp to two lines WITHOUT a fixed-height pill: the old fixed box left a
       visible empty sliver under one-line names (hover pill rendered the
       artifact). -webkit-line-clamp keeps the height content-driven. */
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    padding: 2px 6px;
    border-radius: var(--border-radius-sm);
    border: 1px solid transparent;
  }
  /* TuffBox dark: the section heading, count, actions and empty state
     were lost in dim gray on the dark glass — lift them to primary/secondary
     ink. Other themes keep their tuned palettes. */
  :global(html[data-theme="tuffbox"]) .shelf-head h2 {
    color: var(--text-primary);
  }
  :global(html[data-theme="tuffbox"]) .shelf-count {
    color: var(--text-secondary);
  }
  :global(html[data-theme="tuffbox"]) .text-btn {
    color: var(--text-secondary);
  }
  :global(html[data-theme="tuffbox"]) .shelf-empty p {
    color: var(--text-secondary);
  }

  .potato .pack-tile:hover .pack-icon-wrap {
    transform: none;
  }
  .potato .pack-tile:hover:not(.selected) .pack-icon {
    box-shadow: 0 6px 14px color-mix(in srgb, var(--bg-primary) 35%, transparent);
  }
  .potato .pack-tile.selected .pack-icon {
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent-primary) 72%, transparent);
  }
  .potato .running-dot {
    animation: none;
    box-shadow: none;
  }
</style>
