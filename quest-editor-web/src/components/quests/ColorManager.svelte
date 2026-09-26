<script lang="ts">
  import type { QuestChapter, QuestData } from "../../lib/store";
  import { searchColorHits, aggregateColors, recolorAtPosition, type ColorHit, type ColorAggregation } from "../../lib/color-manager";
  import { mcFormat, MC_COLORS, FORMATTING_NAMES } from "../../lib/mcformat";

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
        {#each aggregations as agg}
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
            <span class="chip-swatch" style="background: var(--mc-c{agg.code})"></span>
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
      {#each hits as hit, i}
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
              <span class="mini-swatch" style="background: var(--mc-c{hit.colorCode})"></span>
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
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="popup-overlay" onclick={closePopup} oncontextmenu={(e) => e.preventDefault()}>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div class="popup" style="left: {popupPos.x}px; top: {popupPos.y}px"
        onclick={(e) => e.stopPropagation()} oncontextmenu={(e) => e.preventDefault()}>
      <div class="popup-title">Recolor to:</div>
      <div class="popup-colors">
        {#each MC_COLORS as mc}
          <button
            type="button"
            class="popup-swatch"
            class:active={popupColor === mc.code}
            style="background: {mc.color}"
            title="&{mc.code} — {FORMATTING_NAMES[mc.code]}"
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
  }
  .cm-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .cm-title {
    font-size: 12px;
    font-weight: 700;
    color: var(--accent);
  }
  .cm-count {
    font-size: 10px;
    color: var(--text-muted);
  }
  .cm-search {
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 6px;
    flex-shrink: 0;
  }
  .search-input {
    width: 100%;
    padding: 6px 8px;
    font-size: 12px;
  }
  .cm-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .filter {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--text-muted);
    cursor: pointer;
  }
  .filter input { width: 12px; height: 12px; }

  .cm-message {
    padding: 6px 12px;
    font-size: 11px;
    color: var(--success);
    background: rgba(85,201,90,0.1);
    border-bottom: 1px solid rgba(85,201,90,0.3);
    flex-shrink: 0;
  }

  .color-palette {
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .palette-label {
    font-size: 10px;
    color: var(--text-muted);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 6px;
  }
  .palette-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .color-chip {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px 6px;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--bg-primary);
    cursor: pointer;
    font-size: 10px;
    color: var(--text-primary);
    transition: border-color 0.15s;
  }
  .color-chip:hover { border-color: var(--accent); }
  .chip-swatch {
    width: 10px;
    height: 10px;
    border-radius: 2px;
    flex-shrink: 0;
  }
  .chip-code {
    font-family: monospace;
    font-weight: 600;
  }
  .chip-count {
    color: var(--text-muted);
  }

  .cm-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    font-size: 12px;
    padding: 24px;
  }

  .hit-list {
    flex: 1;
    overflow-y: auto;
    padding: 8px 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .hit-item {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--bg-primary);
    cursor: pointer;
    text-align: left;
    transition: border-color 0.15s;
    font-size: 11px;
    color: var(--text-primary);
    width: 100%;
  }
  .hit-item:hover { border-color: var(--accent); }
  .hit-item.selected { border-color: var(--accent); background: rgba(61,184,168,0.08); }
  .hit-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .hit-chapter {
    font-weight: 600;
    color: var(--accent);
  }
  .hit-quest {
    color: var(--text-primary);
  }
  .hit-field {
    font-size: 10px;
    color: var(--text-muted);
    font-family: monospace;
  }
  .hit-color {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 3px;
    font-family: monospace;
    font-weight: 600;
    font-size: 10px;
  }
  .mini-swatch {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    flex-shrink: 0;
  }
  .hit-segment {
    font-size: 11px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    line-height: 1.4;
  }

  /* Color palette swatches */
  :global(.mc-c0) { background: #000000; }
  :global(.mc-c1) { background: #0000AA; }
  :global(.mc-c2) { background: #00AA00; }
  :global(.mc-c3) { background: #00AAAA; }
  :global(.mc-c4) { background: #AA0000; }
  :global(.mc-c5) { background: #AA00AA; }
  :global(.mc-c6) { background: #FFAA00; }
  :global(.mc-c7) { background: #AAAAAA; }
  :global(.mc-c8) { background: #555555; }
  :global(.mc-c9) { background: #5555FF; }
  :global(.mc-ca) { background: #55FF55; }
  :global(.mc-cb) { background: #55FFFF; }
  :global(.mc-cc) { background: #FF5555; }
  :global(.mc-cd) { background: #FF55FF; }
  :global(.mc-ce) { background: #FFFF55; }
  :global(.mc-cf) { background: #FFFFFF; }

  /* Popup */
  .popup-overlay {
    position: fixed;
    inset: 0;
    z-index: 200;
    background: rgba(0,0,0,0.3);
  }
  .popup {
    position: fixed;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 12px;
    box-shadow: 0 8px 32px rgba(0,0,0,0.4);
    z-index: 201;
    min-width: 200px;
  }
  .popup-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 8px;
  }
  .popup-colors {
    display: grid;
    grid-template-columns: repeat(8, 1fr);
    gap: 4px;
    margin-bottom: 8px;
  }
  .popup-swatch {
    width: 24px;
    height: 24px;
    border-radius: 3px;
    border: 2px solid transparent;
    cursor: pointer;
    transition: border-color 0.15s, transform 0.1s;
  }
  .popup-swatch:hover { transform: scale(1.15); }
  .popup-swatch.active { border-color: var(--accent); transform: scale(1.15); }
  .popup-preview {
    font-size: 12px;
    padding: 6px 8px;
    background: var(--bg-primary);
    border-radius: 3px;
    margin-bottom: 8px;
    min-height: 28px;
  }
  .popup-actions {
    display: flex;
    gap: 6px;
  }

  .btn {
    padding: 6px 12px;
    border: 1px solid var(--border);
    background: rgba(0,0,0,0.25);
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 600;
    border-radius: 2px;
    cursor: pointer;
  }
  .btn:hover:not(:disabled) { border-color: var(--accent); }
  .btn:disabled { opacity: 0.4; cursor: default; }
  .btn.primary { border-color: var(--success); background: rgba(85,201,90,0.18); color: var(--success); }
  .btn.ghost { background: transparent; }
  .btn.small { padding: 4px 8px; font-size: 11px; }
</style>
