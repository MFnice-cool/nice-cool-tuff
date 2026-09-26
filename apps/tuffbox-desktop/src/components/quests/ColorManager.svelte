<script lang="ts">
  import type { QuestChapter, QuestData } from "../../lib/api";
  import { searchColorHits, aggregateColors, recolorAtPosition, type ColorHit, type ColorAggregation } from "../../lib/color-manager";
  import { mcFormat, MC_COLORS, FORMATTING_NAMES } from "../../lib/mcformat";
  import { trapFocus } from "../../lib/focusTrap";

  interface Props {
    chapters: QuestChapter[];
    onQuestUpdate: (chapterId: string, quest: QuestData) => void;
  }

  let { chapters, onQuestUpdate }: Props = $props();

  let query = $state("");
  let caseSensitive = $state(false);
  let hits = $state<ColorHit[]>([]);
  let aggregations = $state<ColorAggregation[]>([]);
  let selectedHit = $state<ColorHit | null>(null);
  let popupColor = $state("");
  let showPopup = $state(false);
  let popupPos = $state({ x: 0, y: 0 });
  let message = $state<string | null>(null);

  function doSearch() {
    hits = searchColorHits(chapters, query, {
      caseSensitive,
      stripCodesBeforeMatch: true,
    });
    aggregations = aggregateColors(hits);
    selectedHit = null;
    showPopup = false;
  }

  function handleSearchKey(e: KeyboardEvent) {
    if (e.key === "Enter") doSearch();
  }

  function selectHit(hit: ColorHit) {
    selectedHit = hit;
  }

  function openRecolorPopup(hit: ColorHit, e: MouseEvent) {
    e.stopPropagation();
    selectedHit = hit;
    popupColor = hit.colorCode;
    popupPos = { x: e.clientX, y: e.clientY };
    showPopup = true;
  }

  function closePopup() {
    showPopup = false;
  }

  $effect(() => {
    if (!showPopup) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        closePopup();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  function applyRecolor() {
    if (!selectedHit || !popupColor) return;

    const ch = chapters.find((c) => c.id === selectedHit!.chapterId);
    if (!ch) return;
    const q = ch.quests.find((x) => x.id === selectedHit!.questId);
    if (!q) return;

    // Create a new quest object (don't mutate original) so undo works correctly
    const updated = { ...q };
    if (selectedHit.field === "title") {
      updated.title = recolorAtPosition(q.title ?? "", popupColor, selectedHit.position);
    } else if (selectedHit.field === "subtitle") {
      updated.subtitle = recolorAtPosition(q.subtitle ?? "", popupColor, selectedHit.position);
    } else {
      const lines = [...(q.description ?? [])];
      if (lines[selectedHit.lineIndex]) {
        lines[selectedHit.lineIndex] = recolorAtPosition(
          lines[selectedHit.lineIndex]!,
          popupColor,
          selectedHit.position
        );
      }
      updated.description = lines;
    }

    onQuestUpdate(selectedHit.chapterId, updated);
    showPopup = false;
    message = `Recolored to &${popupColor} in ${selectedHit.field}`;
    setTimeout(() => (message = null), 2000);

    // Re-search to refresh hits
    doSearch();
  }

  function bulkRecolor(code: string, newColor: string) {
    // Group hits by quest to handle multiple hits in the same quest
    const hitsByQuest = new Map<string, { quest: QuestData; chapterId: string; hits: ColorHit[] }>();

    for (const hit of hits) {
      if (hit.colorCode !== code) continue;
      const key = `${hit.chapterId}:${hit.questId}`;
      const ch = chapters.find((c) => c.id === hit.chapterId);
      if (!ch) continue;
      const q = ch.quests.find((x) => x.id === hit.questId);
      if (!q) continue;

      if (!hitsByQuest.has(key)) {
        hitsByQuest.set(key, { quest: q, chapterId: hit.chapterId, hits: [] });
      }
      hitsByQuest.get(key)!.hits.push(hit);
    }

    let count = 0;
    for (const { quest: q, chapterId, hits: questHits } of hitsByQuest.values()) {
      // Create a new quest object (don't mutate original)
      const updated = { ...q };

      // Process hits in reverse position order to avoid position shifts
      const sortedHits = questHits.sort((a, b) => b.position - a.position);

      for (const hit of sortedHits) {
        let rawStr: string;
        if (hit.field === "title") {
          rawStr = updated.title ?? "";
          updated.title = recolorAtPosition(rawStr, newColor, hit.position);
        } else if (hit.field === "subtitle") {
          rawStr = updated.subtitle ?? "";
          updated.subtitle = recolorAtPosition(rawStr, newColor, hit.position);
        } else {
          const lines = [...(updated.description ?? [])];
          if (lines[hit.lineIndex]) {
            lines[hit.lineIndex] = recolorAtPosition(
              lines[hit.lineIndex]!,
              newColor,
              hit.position
            );
          }
          updated.description = lines;
        }
        count++;
      }

      onQuestUpdate(chapterId, updated);
    }

    message = `Recolored ${count} occurrence(s) of &${code} to &${newColor}`;
    setTimeout(() => (message = null), 2000);
    doSearch();
  }

  function swatchColor(code: string): string {
    return MC_COLORS.find((c) => c.code === code)?.color ?? "#AAAAAA";
  }
</script>

<div class="color-manager">
  <div class="cm-header">
    <h3 class="cm-title">Color Manager</h3>
    <span class="cm-count">
      {hits.length} hit{hits.length !== 1 ? "s" : ""}
    </span>
  </div>

  <div class="cm-search">
    <input
      type="text"
      class="search-input"
      placeholder="Search term to find colors..."
      bind:value={query}
      onkeydown={handleSearchKey}
    />
    <div class="cm-actions">
      <label class="filter">
        <input type="checkbox" bind:checked={caseSensitive} />
        Case sensitive
      </label>
      <button type="button" class="btn primary small" onclick={doSearch}>
        Search
      </button>
    </div>
  </div>

  {#if message}
    <div class="cm-message">{message}</div>
  {/if}

  {#if aggregations.length > 0}
    <div class="color-palette">
      <div class="palette-label">Color codes found:</div>
      <div class="palette-chips">
        {#each aggregations as agg (agg.code)}
          <button
            type="button"
            class="color-chip"
            title="&{agg.code} — {agg.count} occurrence(s) in {agg.quests.size} quest(s). Click to bulk recolor."
            onclick={() => {
              const newColor = prompt(
                `Recolor all &${agg.code} (${FORMATTING_NAMES[agg.code] ?? agg.code}) to:`,
                agg.code
              );
              if (newColor && newColor !== agg.code) bulkRecolor(agg.code, newColor);
            }}
          >
            <span class="chip-swatch" style="background: {swatchColor(agg.code)}"></span>
            <span class="chip-code">&{agg.code}</span>
            <span class="chip-count">{agg.count}</span>
          </button>
        {/each}
      </div>
    </div>
  {/if}

  {#if hits.length === 0}
    <div class="cm-empty">
      {#if query.trim()}
        <p>No color hits found for "{query}".</p>
      {:else}
        <p>Type a search term and press Enter to find color codes.</p>
      {/if}
    </div>
  {:else}
    <div class="hit-list">
      {#each hits as hit, i (`${hit.chapterId}:${hit.questId}:${hit.field}:${hit.lineIndex}:${hit.position}:${i}`)}
        <button
          type="button"
          class="hit-item"
          class:selected={selectedHit === hit}
          onclick={() => selectHit(hit)}
          oncontextmenu={(e) => { e.preventDefault(); openRecolorPopup(hit, e); }}
        >
          <div class="hit-meta">
            <span class="hit-chapter">{hit.chapterTitle}</span>
            <span class="hit-quest">{hit.questTitle}</span>
            <span class="hit-field">{hit.field}{hit.lineIndex >= 0 ? `[${hit.lineIndex}]` : ""}</span>
            <span class="hit-color">
              <span class="mini-swatch" style="background: {swatchColor(hit.colorCode)}"></span>
              &{hit.colorCode}
            </span>
          </div>
          <div class="hit-segment">
            {@html mcFormat("..." + hit.segment + "...")}
          </div>
        </button>
      {/each}
    </div>
  {/if}
</div>

{#if showPopup}
  <div
    class="popup-overlay"
    role="presentation"
    onclick={closePopup}
    oncontextmenu={(e) => e.preventDefault()}
  >
    <div
      class="popup"
      role="dialog"
      aria-modal="true"
      aria-label="Recolor"
      tabindex="-1"
      style="left: {popupPos.x}px; top: {popupPos.y}px"
      use:trapFocus={{ onEscape: closePopup }}
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          closePopup();
        }
      }}
      oncontextmenu={(e) => e.preventDefault()}
    >
      <div class="popup-title">Recolor to:</div>
      <div class="popup-colors">
        {#each MC_COLORS as mc (mc.code)}
          <button
            type="button"
            class="popup-swatch"
            class:active={popupColor === mc.code}
            style="background: {mc.color}"
            title="&{mc.code} — {FORMATTING_NAMES[mc.code]}"
            aria-label="Recolor to &{mc.code} — {FORMATTING_NAMES[mc.code]}"
            onclick={() => { popupColor = mc.code; }}
          ></button>
        {/each}
      </div>
      <div class="popup-preview">
        Preview: {@html mcFormat(`&${popupColor}Sample text&r`)}
      </div>
      <div class="popup-actions">
        <button type="button" class="btn primary small" onclick={applyRecolor}>
          Apply
        </button>
        <button type="button" class="btn ghost small" onclick={closePopup}>
          Cancel
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .color-manager {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    background: var(--bg-secondary, var(--ftbq-bg-panel));
    color: var(--text-primary, var(--ftbq-text));
  }
  .cm-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--ftbq-frame);
    flex-shrink: 0;
  }
  .cm-title {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary, var(--ftbq-text));
  }
  .cm-count {
    font-size: 12px;
    color: var(--text-muted, var(--ftbq-text-muted));
    padding: 2px 6px;
    border: 1px solid var(--ftbq-frame);
    border-radius: 4px;
  }
  .cm-search {
    padding: 8px 12px;
    border-bottom: 1px solid var(--ftbq-frame);
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex-shrink: 0;
  }
  .search-input {
    width: 100%;
    padding: 6px 8px;
    font-size: 12px;
    background: var(--ftbq-input-bg);
    border: 1px solid var(--ftbq-frame);
    box-shadow: none;
    color: var(--text-primary, var(--ftbq-text));
    border-radius: var(--ftbq-radius-control);
    outline: none;
  }
  .search-input:focus {
    border-color: color-mix(in srgb, var(--accent-primary) 55%, var(--ftbq-frame));
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }
  .cm-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .filter {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted, var(--ftbq-text-muted));
    cursor: pointer;
  }
  .filter input { width: 12px; height: 12px; }

  .cm-message {
    padding: 6px 12px;
    font-size: 12px;
    color: var(--ftbq-accent-green);
    background: color-mix(in srgb, var(--ftbq-accent-green) 10%, transparent);
    border-bottom: 1px solid color-mix(in srgb, var(--ftbq-accent-green) 28%, var(--ftbq-frame));
    flex-shrink: 0;
  }

  .color-palette {
    padding: 8px 12px;
    border-bottom: 1px solid var(--ftbq-frame);
    flex-shrink: 0;
  }
  .palette-label {
    font-size: 12px;
    color: var(--ftbq-accent-teal);
    font-weight: 600;
    margin-bottom: 6px;
  }
  .palette-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .color-chip {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 7px;
    border: 1px solid var(--ftbq-frame);
    border-radius: var(--ftbq-radius-control);
    background: color-mix(in srgb, var(--ftbq-bg) 45%, transparent);
    cursor: pointer;
    font-size: 12px;
    color: var(--text-primary, var(--ftbq-text));
    transition: border-color 0.15s, background 0.15s;
  }
  .color-chip:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, var(--ftbq-frame));
    background: var(--bg-hover, var(--ftbq-btn-hover-top));
  }
  .chip-swatch {
    width: 10px;
    height: 10px;
    border-radius: 2px;
    flex-shrink: 0;
    border: 1px solid color-mix(in srgb, #000 25%, transparent);
  }
  .chip-code {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-weight: 600;
  }
  .chip-count {
    color: var(--text-muted, var(--ftbq-text-muted));
  }

  .cm-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted, var(--ftbq-text-muted));
    font-size: 12px;
    padding: 24px;
    text-align: center;
  }

  .hit-list {
    flex: 1;
    overflow-y: auto;
    padding: 8px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .hit-item {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 7px 8px;
    border: 1px solid var(--ftbq-frame);
    border-radius: var(--ftbq-radius-control);
    background: color-mix(in srgb, var(--ftbq-bg) 40%, transparent);
    cursor: pointer;
    text-align: left;
    transition: border-color 0.15s, background 0.15s;
    font-size: 12px;
    color: var(--text-primary, var(--ftbq-text));
    width: 100%;
  }
  .hit-item:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 40%, var(--ftbq-frame));
  }
  .hit-item.selected {
    border-color: color-mix(in srgb, var(--accent-primary) 55%, var(--ftbq-frame));
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
  }
  .hit-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .hit-chapter {
    font-weight: 600;
    color: var(--ftbq-accent-teal);
  }
  .hit-quest {
    color: var(--text-primary, var(--ftbq-text));
  }
  .hit-field {
    font-size: 12px;
    color: var(--text-muted, var(--ftbq-text-muted));
    font-family: var(--font-mono, ui-monospace, monospace);
  }
  .hit-color {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    font-family: var(--font-mono, ui-monospace, monospace);
    font-weight: 600;
    font-size: 12px;
  }
  .mini-swatch {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    flex-shrink: 0;
  }
  .hit-segment {
    font-size: 12px;
    color: var(--text-muted, var(--ftbq-text-muted));
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    line-height: 1.4;
  }

  /* Popup */
  .popup-overlay {
    position: fixed;
    inset: 0;
    z-index: 200;
    background: rgba(0,0,0,0.28);
  }
  .popup {
    position: fixed;
    background: var(--bg-elevated, var(--ftbq-bg-panel));
    border: 1px solid var(--ftbq-frame);
    border-radius: var(--ftbq-radius-control);
    padding: 12px;
    box-shadow: 0 10px 28px rgba(0,0,0,0.28);
    z-index: 201;
    min-width: 200px;
  }
  .popup-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary, var(--ftbq-text));
    margin-bottom: 8px;
  }
  .popup-colors {
    display: grid;
    grid-template-columns: repeat(8, 1fr);
    gap: 8px;
    margin-bottom: 8px;
  }
  .popup-swatch {
    width: 24px;
    height: 24px;
    border-radius: 4px;
    border: 2px solid transparent;
    cursor: pointer;
    transition: border-color 0.15s, transform 0.1s;
  }
  .popup-swatch:hover { transform: scale(1.1); }
  .popup-swatch.active {
    border-color: var(--ftbq-accent-teal);
    transform: scale(1.1);
  }
  .popup-preview {
    font-size: 12px;
    padding: 6px 8px;
    background: color-mix(in srgb, var(--ftbq-bg) 55%, transparent);
    border: 1px solid var(--ftbq-frame);
    border-radius: var(--ftbq-radius-control);
    margin-bottom: 8px;
    min-height: 28px;
  }
  .popup-actions {
    display: flex;
    gap: 8px;
  }

  .btn {
    padding: 6px 10px;
    border: 1px solid var(--ftbq-frame);
    background: var(--bg-secondary, var(--ftbq-bg-panel));
    color: var(--text-primary, var(--ftbq-text));
    font-size: 12px;
    font-weight: 600;
    border-radius: var(--ftbq-radius-control);
    cursor: pointer;
  }
  .btn:hover:not(:disabled) {
    background: var(--bg-hover, var(--ftbq-btn-hover-top));
    border-color: color-mix(in srgb, var(--accent-primary) 40%, var(--ftbq-frame));
  }
  .btn:disabled { opacity: 0.4; cursor: default; }
  .btn.primary {
    border-color: color-mix(in srgb, var(--ftbq-accent-green) 55%, var(--ftbq-frame));
    background: color-mix(in srgb, var(--ftbq-accent-green) 14%, transparent);
    color: var(--ftbq-accent-green);
  }
  .btn.ghost { background: transparent; }
  .btn.small { padding: 4px 8px; font-size: 12px; }
</style>
