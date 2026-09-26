<script lang="ts">
  import { onDestroy } from "svelte";
  import { Check } from "@lucide/svelte";
  import {
    THEMES,
    type ThemeId,
    previewTheme,
    restoreCommittedTheme,
    commitTheme,
  } from "../lib/themes";

  let {
    value = "tuffbox",
    onChange = () => {},
  }: {
    value?: ThemeId;
    onChange?: (id: ThemeId) => void;
  } = $props();

  const SHARP_THEMES = new Set(
    THEMES.filter((t) => t.badge === "Sharp").map((t) => t.id),
  );
  const MINIMAL_THEMES = new Set(
    THEMES.filter((t) => t.badge === "Minimal").map((t) => t.id),
  );

  /** Preview applies only after the pointer rests on a swatch — a fast sweep
      across the grid no longer re-themes the whole app (layout shift → leave →
      restore → re-enter → apply → flicker loop). */
  const PREVIEW_DELAY = 200;
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  let previewed: ThemeId | null = null;

  function schedulePreview(id: ThemeId) {
    clearTimeout(previewTimer);
    previewTimer = setTimeout(() => {
      if (previewed !== id) {
        previewed = id;
        previewTheme(id);
      }
    }, PREVIEW_DELAY);
  }

  function outPreview() {
    clearTimeout(previewTimer);
    if (previewed !== null) {
      previewed = null;
      restoreCommittedTheme();
    }
  }

  function select(id: ThemeId) {
    outPreview();
    commitTheme(id);
    onChange(id);
  }

  onDestroy(() => {
    // If the picker unmounts while a preview is pending (or already applied),
    // clear the timer and restore the committed theme — otherwise the preview
    // theme sticks forever after closing the picker.
    clearTimeout(previewTimer);
    if (previewed !== null) {
      previewed = null;
      restoreCommittedTheme();
    }
  });

  function badgeFor(id: ThemeId): string | null {
    return THEMES.find((t) => t.id === id)?.badge ?? null;
  }
</script>

<div class="theme-grid">
  {#each THEMES as theme (theme.id)}
    {@const badge = badgeFor(theme.id)}
    {@const minimal = MINIMAL_THEMES.has(theme.id)}
    {@const sharp = SHARP_THEMES.has(theme.id)}
    <button
      type="button"
      class="theme-swatch"
      class:active={value === theme.id}
      class:sharp
      class:minimal
      style={minimal
        ? `background:
            radial-gradient(ellipse 70% 55% at 18% 12%, ${theme.shades[1]}55, transparent 58%),
            radial-gradient(ellipse 60% 50% at 88% 18%, ${theme.shades[2]}40, transparent 55%),
            linear-gradient(160deg, ${theme.shades[0]} 0%, color-mix(in srgb, ${theme.shades[0]} 70%, ${theme.shades[1]}) 55%, ${theme.shades[0]} 100%)`
        : sharp
          ? `background:
              linear-gradient(90deg, ${theme.shades[1]} 0 22%, transparent 22%),
              linear-gradient(180deg, ${theme.shades[0]} 0%, ${theme.shades[0]} 100%);
            background-color: ${theme.shades[0]}`
          : `background: ${theme.shades[0]}`}
      onclick={() => select(theme.id)}
      onmouseenter={() => schedulePreview(theme.id)}
      onmouseleave={outPreview}
      onfocus={() => schedulePreview(theme.id)}
      onblur={outPreview}
    >
      <div class="mini-ui" aria-hidden="true">
        <div
          class="bar"
          style={minimal
            ? `background: linear-gradient(90deg, ${theme.shades[1]}33, ${theme.shades[0]}cc); backdrop-filter: blur(6px)`
            : sharp
              ? `background: ${theme.shades[2]}; opacity: 1`
              : `background: ${theme.shades[1]}`}
        ></div>
        <div class="body">
          <div
            class="sidebar"
            style={minimal
              ? `background: linear-gradient(180deg, ${theme.shades[1]}40, ${theme.shades[0]}99)`
              : sharp
                ? `background: ${theme.shades[1]}; box-shadow: inset -2px 0 0 ${theme.shades[2]}66`
                : `background: ${theme.shades[1]}`}
          ></div>
          <div
            class="panel"
            style={minimal
              ? `background: color-mix(in srgb, ${theme.shades[0]} 70%, transparent)`
              : `background: ${theme.shades[1]}`}
          >
            <span
              class="dot"
              style={minimal
                ? `background: linear-gradient(135deg, ${theme.shades[1]}, ${theme.shades[2]})`
                : sharp
                  ? `background: ${theme.shades[2]}; box-shadow: 2px 2px 0 #0006`
                  : `background: ${theme.shades[2]}`}
            ></span>
            <span
              class="line"
              style={minimal
                ? `background: linear-gradient(90deg, ${theme.shades[1]}, ${theme.shades[2]}); opacity: 0.7`
                : sharp
                  ? `background: ${theme.shades[2]}; opacity: 0.85`
                  : `background: ${theme.shades[2]}; opacity: 0.45`}
            ></span>
            <span
              class="line short"
              style={minimal
                ? `background: ${theme.shades[2]}; opacity: 0.35`
                : sharp
                  ? `background: ${theme.shades[2]}; opacity: 0.5`
                  : `background: ${theme.shades[2]}; opacity: 0.25`}
            ></span>
          </div>
        </div>
      </div>
      {#if value === theme.id}
        <div
          class="check"
          style={minimal
            ? `background: linear-gradient(135deg, ${theme.shades[1]}, ${theme.shades[2]})`
            : sharp
              ? `background: ${theme.shades[2]}; box-shadow: 3px 3px 0 #0008`
              : `background: ${theme.shades[2]}`}
        >
          <Check size={14} />
        </div>
      {/if}
      <span class="label-row">
        <span class="label">{theme.label}</span>
        {#if badge}<span class="badge">{badge}</span>{/if}
      </span>
    </button>
  {/each}
</div>

<style>
  .theme-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 14px;
  }

  .theme-swatch {
    position: relative;
    width: 168px;
    padding: 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    cursor: pointer;
    color: inherit;
    text-align: left;
    display: flex;
    flex-direction: column;
    gap: 10px;
    transition:
      border-color var(--motion-fast, 160ms) ease,
      box-shadow var(--motion-fast, 160ms) ease,
      transform var(--motion-fast, 160ms) ease;
  }

  .theme-swatch.sharp {
    border-radius: 0;
    box-shadow: 3px 3px 0 color-mix(in srgb, var(--text-primary) 18%, transparent);
  }

  .theme-swatch.minimal {
    border-radius: 18px;
    border-color: color-mix(in srgb, var(--accent-primary) 18%, var(--border-color));
  }

  .theme-swatch:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, var(--border-color));
  }

  .theme-swatch.active {
    border-color: var(--accent-primary);
    box-shadow:
      0 0 0 2px color-mix(in srgb, var(--accent-primary) 55%, transparent),
      var(--shadow-md);
  }

  .theme-swatch.sharp.active {
    box-shadow:
      0 0 0 2px var(--accent-primary),
      4px 4px 0 color-mix(in srgb, var(--accent-primary) 40%, transparent);
  }

  .mini-ui {
    height: 92px;
    border-radius: var(--border-radius-sm);
    overflow: hidden;
    border: 1px solid rgba(255, 255, 255, 0.08);
  }

  .theme-swatch.sharp .mini-ui {
    border-radius: 0;
    border-color: rgba(255, 255, 255, 0.16);
  }

  .theme-swatch.minimal .mini-ui {
    border-radius: var(--border-radius-md);
    border-color: rgba(255, 255, 255, 0.12);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.06);
  }

  .bar {
    height: 12px;
    opacity: 0.9;
  }

  .body {
    display: flex;
    height: calc(100% - 12px);
  }

  .sidebar {
    width: 22%;
    opacity: 0.85;
  }

  .panel {
    flex: 1;
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .dot {
    width: 11px;
    height: 11px;
    border-radius: 50%;
  }

  .theme-swatch.sharp .dot {
    border-radius: 0;
  }

  .theme-swatch.minimal .dot {
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.35);
  }

  .line {
    height: 6px;
    border-radius: 3px;
    width: 80%;
  }

  .theme-swatch.sharp .line {
    border-radius: 0;
  }

  .theme-swatch.minimal .line {
    border-radius: 999px;
  }

  .line.short {
    width: 55%;
  }

  .check {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -70%);
    width: 30px;
    height: 30px;
    border-radius: 999px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #000;
    box-shadow: var(--shadow-md);
  }

  .theme-swatch.sharp .check {
    border-radius: 0;
  }

  .label-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 0 2px 2px;
  }

  .label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .badge {
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-muted);
    border: 1px solid var(--border-color);
    border-radius: 999px;
    padding: 1px 6px;
    white-space: nowrap;
  }

  .theme-swatch.sharp .badge {
    border-radius: 0;
  }

  .theme-swatch.active .label {
    color: var(--text-primary);
  }

  .theme-swatch.active .badge {
    color: var(--accent-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 40%, var(--border-color));
  }
</style>
