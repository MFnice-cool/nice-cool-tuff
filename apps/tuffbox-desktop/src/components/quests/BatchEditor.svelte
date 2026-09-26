<script lang="ts">
  import type { QuestChapter, QuestData } from "../../lib/api";
  import { mcFormat, stripCodes } from "../../lib/mcformat";
  import { SHAPE_OPTIONS } from "../../lib/questTypeLabels";
  import ConfirmDialog from "../ConfirmDialog.svelte";

  interface Props {
    chapters: QuestChapter[];
    selectedIds?: Set<string>;
    focusToken?: number;
    onQuestUpdate: (chapterId: string, quest: QuestData) => void;
    onBatchApply?: (questIds: Set<string>, mutator: (q: QuestData) => QuestData) => void;
    onSaveChapter: (chapterId: string) => void;
    /** P1 mass generation: append quests built from a source list. */
    onAddQuests?: (quests: QuestData[]) => void;
    /** Item catalog for "get item" task generation (resolved by the editor). */
    itemCatalogProvider?: () => Promise<string[]>;
  }

  let {
    chapters,
    selectedIds = new Set(),
    focusToken = 0,
    onQuestUpdate,
    onBatchApply,
    onSaveChapter,
    onAddQuests = undefined,
    itemCatalogProvider = undefined,
  }: Props = $props();

  // Search state
  let query = $state("");
  let searchInputEl = $state<HTMLInputElement | null>(null);
  let lastFocusToken = $state(-1);
  let caseSensitive = $state(false);
  let noTitle = $state(false);
  let noSubtitle = $state(false);
  let noDescription = $state(false);
  let scopeChapter = $state("");
  let scopeSelection = $state(false);
  let prevSelectedCount = $state(0);
  let perPage = $state(10);
  let page = $state(0);

  // Mass-apply draft values
  let massOptional = $state<"keep" | "true" | "false">("keep");
  let massSize = $state("");
  let massShape = $state("__keep__");
  let massHideDep = $state<"keep" | "true" | "false" | "unset">("keep");
  let massHideDependent = $state<"keep" | "true" | "false" | "unset">("keep");
  let massStatus = $state<string | null>(null);
  let massConfirmOpen = $state(false);

  // P1 mass generation: build "get item" quests from a source list
  let genOpen = $state(false);
  let genSource = $state<"tag" | "mod" | "raw">("tag");
  let genTag = $state("");
  let genMod = $state("");
  let genRaw = $state("");
  let genCount = $state("");
  let genStatus = $state<string | null>(null);

  async function parseGenSource(): Promise<string[]> {
    const countCap = Math.max(1, Math.min(200, Number(genCount) || 200));
    const items: string[] = [];
    if (genSource === "tag") {
      // Tag tasks reference `#tag` ids in FTB item tasks.
      const tag = genTag.trim().replace(/^#/, "");
      if (tag) items.push(`#${tag}`);
    } else if (genSource === "mod") {
      const modNs = genMod.trim().toLowerCase();
      if (!modNs || !itemCatalogProvider) return items;
      const catalog = await itemCatalogProvider();
      return catalog.filter((id) => id.toLowerCase().startsWith(`${modNs}:`)).slice(0, countCap);
    } else {
      // Raw list: one id per line, comments (#) and blanks ignored.
      for (const line of genRaw.split("\n")) {
        const t = line.trim();
        if (t && !t.startsWith("#")) items.push(t);
        if (items.length >= countCap) break;
      }
    }
    return items.slice(0, countCap);
  }

  async function runGenerate() {
    genStatus = null;
    const source = await parseGenSource();
    if (source.length === 0) {
      genStatus = "Source resolved to zero items — check the tag/mod/list.";
      return;
    }
    if (!onAddQuests) return;
    const quests: QuestData[] = source.map((itemId, i) => ({
      id: crypto.randomUUID().replace(/-/g, "").slice(0, 16).toUpperCase(),
      title: itemId.startsWith("#") ? `Get any ${itemId.slice(1).split(":").pop()}` : `Get ${itemId.split(":").pop() ?? itemId}`,
      titleFromSnbt: true,
      description: [],
      // Laid out below/right of existing content so nothing overlaps.
      x: 2 + (i % 8),
      y: 2 + Math.floor(i / 8),
      dependencies: [],
      tasks: [
        {
          id: crypto.randomUUID().replace(/-/g, "").slice(0, 12).toUpperCase(),
          type: "item",
          value: itemId,
          properties: { count: 1, item: itemId },
        },
      ],
      rewards: [],
      optional: false,
      size: 1,
      extras: {},
    }));
    onAddQuests(quests);
    genStatus = `Added ${quests.length} quest(s) — position them on the canvas, Ctrl+Z to undo.`;
  }

  // Turn on when selection appears; clear when selection empties
  $effect(() => {
    const n = selectedIds.size;
    if (n > 0 && prevSelectedCount === 0) scopeSelection = true;
    if (n === 0) scopeSelection = false;
    prevSelectedCount = n;
  });

  $effect(() => {
    if (focusToken === lastFocusToken) return;
    lastFocusToken = focusToken;
    if (focusToken <= 0) return;
    queueMicrotask(() => searchInputEl?.focus({ preventScroll: true }));
  });

  // Editing state
  let editingId = $state<string | null>(null);
  let editTitle = $state("");
  let editSubtitle = $state("");
  let editDescription = $state("");
  let saveStatus = $state<Record<string, "saving" | "saved" | "error">>({});

  interface BatchResult {
    quest: QuestData;
    chapterId: string;
    chapterTitle: string;
    matchField: string;
  }

  let results = $derived.by<BatchResult[]>(() => {
    const terms = query.trim()
      ? query.trim().split(/\s+/).filter(Boolean)
      : [];
    const selectionOnly = scopeSelection && selectedIds.size > 0;
    if (!terms.length && !noTitle && !noSubtitle && !noDescription && !selectionOnly) {
      return [];
    }

    const filtered: BatchResult[] = [];

    for (const ch of chapters) {
      if (scopeChapter && ch.title !== scopeChapter && ch.id !== scopeChapter) continue;

      for (const q of ch.quests) {
        if (selectionOnly && !selectedIds.has(q.id)) continue;

        if (noTitle && q.title?.trim()) continue;
        if (noSubtitle && q.subtitle?.trim()) continue;
        if (noDescription && q.description?.length) continue;

        if (terms.length > 0) {
          const titleText = stripCodes(q.title ?? "");
          const subtitleText = stripCodes(q.subtitle ?? "");
          const descText = stripCodes(q.description?.join(" ") ?? "");

          const match = terms.every((term) => {
            const t = caseSensitive ? term : term.toLowerCase();
            return (
              (caseSensitive ? titleText : titleText.toLowerCase()).includes(t) ||
              (caseSensitive ? subtitleText : subtitleText.toLowerCase()).includes(t) ||
              (caseSensitive ? descText : descText.toLowerCase()).includes(t)
            );
          });

          if (!match) continue;
        }

        let matchField = "title";
        if (noTitle) matchField = "title (missing)";
        else if (noSubtitle) matchField = "subtitle (missing)";
        else if (noDescription) matchField = "description (missing)";
        else if (selectionOnly && !terms.length) matchField = "selected";

        filtered.push({
          quest: q,
          chapterId: ch.id,
          chapterTitle: ch.title,
          matchField,
        });
      }
    }

    return filtered;
  });

  let totalPages = $derived(Math.ceil(results.length / perPage));
  let pagedResults = $derived(results.slice(page * perPage, (page + 1) * perPage));
  let canMassApply = $derived(results.length > 0 && !!onBatchApply);

  // Reset page on search change
  $effect(() => {
    void query;
    void scopeChapter;
    void scopeSelection;
    void noTitle;
    void noSubtitle;
    void noDescription;
    page = 0;
  });

  function startEdit(q: QuestData) {
    editingId = q.id;
    editTitle = q.title ?? "";
    editSubtitle = q.subtitle ?? "";
    editDescription = q.description?.join("\n") ?? "";
  }

  function cancelEdit() {
    editingId = null;
  }

  async function saveEdit(chapterId: string, q: QuestData) {
    saveStatus[q.id] = "saving";

    const updated: QuestData = {
      ...q,
      title: editTitle,
      subtitle: editSubtitle || null,
      description: editDescription
        .split("\n")
        .map((l) => l.trimEnd())
        .filter((l) => l.length > 0),
    };

    try {
      onQuestUpdate(chapterId, updated);
      saveStatus[q.id] = "saved";
      setTimeout(() => {
        if (saveStatus[q.id] === "saved") {
          delete saveStatus[q.id];
          saveStatus = saveStatus; // trigger reactivity
        }
      }, 1500);
    } catch {
      saveStatus[q.id] = "error";
    }
  }

  function parseTri(
    mode: "keep" | "true" | "false" | "unset",
  ): boolean | null | undefined {
    if (mode === "keep") return undefined;
    if (mode === "unset") return null;
    return mode === "true";
  }

  function applyMass() {
    if (!onBatchApply || results.length === 0) return;
    const sizeNum = massSize.trim() === "" ? undefined : Number(massSize);
    if (sizeNum !== undefined && (!Number.isFinite(sizeNum) || sizeNum <= 0)) {
      massStatus = "Size must be a positive number";
      return;
    }
    const optional =
      massOptional === "keep" ? undefined : massOptional === "true";
    const shape = massShape === "__keep__" ? undefined : massShape || null;
    const hideDependencyLines = parseTri(massHideDep);
    const hideDependentLines = parseTri(massHideDependent);

    const hasAny =
      optional !== undefined ||
      sizeNum !== undefined ||
      shape !== undefined ||
      hideDependencyLines !== undefined ||
      hideDependentLines !== undefined;
    if (!hasAny) {
      massStatus = "Pick at least one field to apply";
      return;
    }

    if (results.length >= 10) {
      massConfirmOpen = true;
      return;
    }
    runMassApply(optional, sizeNum, shape, hideDependencyLines, hideDependentLines);
  }

  function runMassApply(
    optional: boolean | undefined,
    sizeNum: number | undefined,
    shape: string | null | undefined,
    hideDependencyLines: boolean | null | undefined,
    hideDependentLines: boolean | null | undefined,
  ) {
    if (!onBatchApply) return;
    const ids = new Set(results.map((r) => r.quest.id));
    onBatchApply(ids, (q) => {
      const next: QuestData = { ...q };
      if (optional !== undefined) next.optional = optional;
      if (sizeNum !== undefined) next.size = sizeNum;
      if (shape !== undefined) next.shape = shape;
      if (hideDependencyLines !== undefined) next.hideDependencyLines = hideDependencyLines;
      if (hideDependentLines !== undefined) next.hideDependentLines = hideDependentLines;
      return next;
    });
    massStatus = `Applied to ${ids.size} quest(s) · one Undo reverts all`;
    setTimeout(() => {
      if (massStatus?.startsWith("Applied")) massStatus = null;
    }, 2500);
  }

  function confirmMassApply() {
    massConfirmOpen = false;
    const sizeNum = massSize.trim() === "" ? undefined : Number(massSize);
    const optional =
      massOptional === "keep" ? undefined : massOptional === "true";
    const shape = massShape === "__keep__" ? undefined : massShape || null;
    runMassApply(
      optional,
      sizeNum,
      shape,
      parseTri(massHideDep),
      parseTri(massHideDependent),
    );
  }

  function matchColor(field: string): string {
    if (field.includes("missing")) return "var(--accent-danger)";
    return "var(--ftbq-text-muted, #9a9aa0)";
  }
</script>

<div class="batch-editor">
  {#if onAddQuests}
    <div class="gen-panel">
      <button
        type="button"
        class="gen-toggle"
        aria-expanded={genOpen}
        onclick={() => (genOpen = !genOpen)}
      >
        Generate quests from…
      </button>
      {#if genOpen}
        <div class="gen-body">
          <label class="gen-field">
            Source
            <select bind:value={genSource}>
              <option value="tag">Item tag (any item of tag)</option>
              <option value="mod">Mod namespace (all items of mod)</option>
              <option value="raw">Raw list (one item id per line)</option>
            </select>
          </label>
          {#if genSource === "tag"}
            <label class="gen-field">
              Tag
              <input bind:value={genTag} placeholder="c:ingots or minecraft:logs" />
            </label>
          {:else if genSource === "mod"}
            <label class="gen-field">
              Mod namespace
              <input bind:value={genMod} placeholder="botania" />
            </label>
          {:else}
            <label class="gen-field">
              Item ids (one per line)
              <textarea bind:value={genRaw} rows="5" placeholder="minecraft:diamond&#10;botania:mana_pearl"></textarea>
            </label>
          {/if}
          <label class="gen-field">
            Max quests
            <input type="number" min="1" max="200" bind:value={genCount} placeholder="200" />
          </label>
          <button type="button" class="gen-run" onclick={() => void runGenerate()}>
            Generate & add to chapter
          </button>
          {#if genStatus}
            <p class="gen-status">{genStatus}</p>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  <div class="batch-header">
    <h3 class="batch-title">Batch Editor</h3>
    <span class="batch-count">
      {results.length} result{results.length !== 1 ? "s" : ""}
    </span>
  </div>

  <div class="batch-search">
    <input
      type="text"
      class="search-input"
      placeholder="Search quests..."
      bind:this={searchInputEl}
      bind:value={query}
      onkeydown={(e) => {
        if (e.key === "Escape" && query) {
          e.stopPropagation();
          query = "";
        }
      }}
    />
    <div class="filters">
      <label class="filter">
        <input type="checkbox" bind:checked={caseSensitive} />
        Case sensitive
      </label>
      <label class="filter">
        <input type="checkbox" bind:checked={noTitle} />
        No title
      </label>
      <label class="filter">
        <input type="checkbox" bind:checked={noSubtitle} />
        No subtitle
      </label>
      <label class="filter">
        <input type="checkbox" bind:checked={noDescription} />
        No description
      </label>
    </div>
    <div class="scope-row">
      <select bind:value={scopeChapter}>
        <option value="">All chapters</option>
        {#each chapters as ch (ch.id)}
          <option value={ch.id}>{ch.title} ({ch.quests.length})</option>
        {/each}
      </select>
      <label class="filter" title="Limit batch rows to canvas multi-selection">
        <input type="checkbox" bind:checked={scopeSelection} disabled={selectedIds.size === 0} />
        Selected only ({selectedIds.size})
      </label>
      <select bind:value={perPage}>
        <option value={5}>5 per page</option>
        <option value={10}>10 per page</option>
        <option value={20}>20 per page</option>
        <option value={50}>50 per page</option>
      </select>
    </div>
  </div>

  {#if canMassApply}
    <div class="mass-bar">
      <span class="mass-label">Mass apply ({results.length})</span>
      <label class="mass-field">
        Optional
        <select bind:value={massOptional}>
          <option value="keep">keep</option>
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
      </label>
      <label class="mass-field">
        Size
        <input type="number" step="0.25" min="0.25" placeholder="keep" bind:value={massSize} />
      </label>
      <label class="mass-field">
        Shape
        <select bind:value={massShape}>
          <option value="__keep__">keep</option>
          {#each SHAPE_OPTIONS as s (s.id || "_default")}
            <option value={s.id}>{s.label}</option>
          {/each}
        </select>
      </label>
      <label class="mass-field">
        Hide dep lines
        <select bind:value={massHideDep}>
          <option value="keep">keep</option>
          <option value="true">true</option>
          <option value="false">false</option>
          <option value="unset">unset</option>
        </select>
      </label>
      <label class="mass-field">
        Hide dependent
        <select bind:value={massHideDependent}>
          <option value="keep">keep</option>
          <option value="true">true</option>
          <option value="false">false</option>
          <option value="unset">unset</option>
        </select>
      </label>
      <button type="button" class="btn primary small" onclick={applyMass}>Apply</button>
      {#if massStatus}
        <span class="mass-status">{massStatus}</span>
      {/if}
    </div>
  {/if}

  {#if results.length === 0}
    <div class="batch-empty">
      {#if query.trim() || noTitle || noSubtitle || noDescription || scopeSelection}
        <p>No quests match your search.</p>
      {:else}
        <p>Type a search query or check a filter to find quests.</p>
      {/if}
    </div>
  {:else}
    <div class="batch-results">
      {#each pagedResults as r (r.quest.id + r.chapterId)}
        <div class="quest-card" class:editing={editingId === r.quest.id}>
          <div class="quest-header" role="button" tabindex="0"
            onclick={() => editingId === r.quest.id ? null : startEdit(r.quest)}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                startEdit(r.quest);
              }
            }}
          >
            <span class="quest-chapter" style="color: {matchColor(r.matchField)}">
              {r.chapterTitle}
            </span>
            <span class="quest-id">{r.quest.id}</span>
            {#if saveStatus[r.quest.id]}
              <span class="save-badge" class:saving={saveStatus[r.quest.id] === "saving"}
                class:saved={saveStatus[r.quest.id] === "saved"}
                class:error={saveStatus[r.quest.id] === "error"}>
                {saveStatus[r.quest.id] === "saving" ? "Updating…" :
                 saveStatus[r.quest.id] === "saved" ? "Updated" : "Error"}
              </span>
            {/if}
          </div>

          {#if editingId === r.quest.id}
            <div class="edit-form">
              <label class="field">
                <span class="field-label">Title</span>
                <input type="text" bind:value={editTitle} />
                <span class="field-preview">{@html mcFormat(editTitle)}</span>
              </label>
              <label class="field">
                <span class="field-label">Subtitle</span>
                <input type="text" bind:value={editSubtitle} />
                <span class="field-preview">{@html mcFormat(editSubtitle)}</span>
              </label>
              <label class="field">
                <span class="field-label">Description</span>
                <textarea rows="4" bind:value={editDescription}></textarea>
                <span class="field-preview desc">{@html mcFormat(editDescription)}</span>
              </label>
              <div class="edit-actions">
                <button type="button" class="btn primary small"
                  onclick={() => saveEdit(r.chapterId, r.quest)}>
                  Update
                </button>
                <button type="button" class="btn ghost small" onclick={cancelEdit}>
                  Cancel
                </button>
                <button type="button" class="btn ghost small"
                  onclick={() => onSaveChapter(r.chapterId)}>
                  Save chapter
                </button>
              </div>
            </div>
          {:else}
            <div class="quest-preview">
                <div class="preview-row">
                  <span class="preview-label">Title:</span>
                  <span class="preview-value">{r.quest.title ?? ''}</span>
                </div>
              {#if r.quest.subtitle}
                <div class="preview-row">
                  <span class="preview-label">Subtitle:</span>
                  <span class="preview-value">{r.quest.subtitle}</span>
                </div>
              {/if}
              {#if r.quest.description?.length}
                <div class="preview-row">
                  <span class="preview-label">Description:</span>
                  <span class="preview-value desc">{r.quest.description.slice(0, 2).join(" ")}{r.quest.description.length > 2 ? "..." : ""}</span>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>

    {#if totalPages > 1}
      <div class="pagination">
        <button type="button" class="btn ghost small" disabled={page === 0}
          onclick={() => page--}>
          ← Prev
        </button>
        <span class="page-info">
          Page {page + 1} of {totalPages}
        </span>
        <button type="button" class="btn ghost small" disabled={page >= totalPages - 1}
          onclick={() => page++}>
          Next →
        </button>
      </div>
    {/if}
  {/if}
</div>

{#if massConfirmOpen}
  <ConfirmDialog
    title="Mass apply to many quests?"
    message={`Apply field changes to ${results.length} quests? One Undo (Ctrl+Z) reverts the whole batch.`}
    confirmLabel="Apply"
    onconfirm={confirmMassApply}
    oncancel={() => (massConfirmOpen = false)}
  />
{/if}

<style>
  .batch-editor {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    background: var(--bg-secondary, var(--ftbq-bg-panel));
    color: var(--text-primary, var(--ftbq-text));
  }
  .batch-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--ftbq-frame);
    flex-shrink: 0;
  }
  .gen-panel {
    margin: 10px 12px 0;
    border: 1px dashed var(--ftbq-frame);
    border-radius: var(--ftbq-radius-control);
    padding: 8px;
    flex-shrink: 0;
  }
  .gen-toggle {
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    color: var(--text-secondary, var(--ftbq-text-muted));
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
    padding: 2px 0;
  }
  .gen-toggle:hover {
    color: var(--text-primary, var(--ftbq-text));
  }
  .gen-body {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 8px;
  }
  .gen-field {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted, var(--ftbq-text-muted));
  }
  .gen-field input,
  .gen-field select,
  .gen-field textarea {
    font-size: 12px;
    padding: 5px 7px;
    background: var(--ftbq-input-bg);
    border: 1px solid var(--ftbq-frame);
    color: var(--text-primary, var(--ftbq-text));
    border-radius: var(--ftbq-radius-control);
    outline: none;
  }
  .gen-field textarea {
    font-family: var(--font-mono, ui-monospace, monospace);
    resize: vertical;
  }
  .gen-run {
    align-self: flex-start;
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
    border: 1px solid var(--ftbq-frame);
    border-radius: var(--ftbq-radius-control);
    background: color-mix(in srgb, var(--accent-primary) 18%, transparent);
    color: var(--text-primary, var(--ftbq-text));
  }
  .gen-run:hover {
    background: color-mix(in srgb, var(--accent-primary) 30%, transparent);
  }
  .gen-status {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted, var(--ftbq-text-muted));
  }
  .batch-title {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary, var(--ftbq-text));
  }
  .batch-count {
    font-size: 12px;
    color: var(--text-muted, var(--ftbq-text-muted));
    padding: 2px 6px;
    border: 1px solid var(--ftbq-frame);
    border-radius: 4px;
  }
  .batch-search {
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
  .filters {
    display: flex;
    flex-wrap: wrap;
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
  .scope-row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  .scope-row select {
    flex: 1;
    min-width: 0;
    font-size: 12px;
    padding: 5px 6px;
    background: var(--ftbq-input-bg);
    border: 1px solid var(--ftbq-frame);
    color: var(--text-primary, var(--ftbq-text));
    border-radius: var(--ftbq-radius-control);
    outline: none;
  }
  .scope-row select:focus {
    border-color: color-mix(in srgb, var(--accent-primary) 55%, var(--ftbq-frame));
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }

  .mass-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--ftbq-frame);
    background: color-mix(in srgb, var(--ftbq-bg) 55%, transparent);
    flex-shrink: 0;
  }
  .mass-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--ftbq-accent-teal);
    align-self: center;
  }
  .mass-field {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted, var(--ftbq-text-muted));
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .mass-field select,
  .mass-field input {
    min-width: 72px;
    max-width: 110px;
    font-size: 12px;
    padding: 4px 6px;
    background: var(--ftbq-input-bg);
    border: 1px solid var(--ftbq-frame);
    color: var(--text-primary, var(--ftbq-text));
    border-radius: var(--ftbq-radius-control);
    text-transform: none;
    outline: none;
  }
  .mass-field select:focus,
  .mass-field input:focus {
    border-color: color-mix(in srgb, var(--accent-primary) 55%, var(--ftbq-frame));
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }
  .mass-status {
    font-size: 12px;
    color: var(--ftbq-accent-green);
    align-self: center;
  }

  .batch-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted, var(--ftbq-text-muted));
    font-size: 12px;
    padding: 24px;
    text-align: center;
  }

  .batch-results {
    flex: 1;
    overflow-y: auto;
    padding: 8px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .quest-card {
    border: 1px solid var(--ftbq-frame);
    border-radius: var(--ftbq-radius-control);
    overflow: hidden;
    background: color-mix(in srgb, var(--ftbq-bg) 40%, transparent);
    transition: border-color 0.15s;
  }
  .quest-card:hover { border-color: color-mix(in srgb, var(--accent-primary) 40%, var(--ftbq-frame)); }
  .quest-card.editing {
    border-color: color-mix(in srgb, var(--accent-primary) 55%, var(--ftbq-frame));
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent-primary) 25%, transparent);
  }

  .quest-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 8px;
    background: transparent;
    cursor: pointer;
    font-size: 12px;
  }
  .quest-chapter {
    font-weight: 600;
    white-space: nowrap;
  }
  .quest-id {
    font-family: var(--font-mono, ui-monospace, monospace);
    color: var(--text-muted, var(--ftbq-text-muted));
    font-size: 12px;
  }
  .save-badge {
    margin-left: auto;
    font-size: 12px;
    padding: 1px 6px;
    border-radius: 4px;
    font-weight: 600;
  }
  .save-badge.saving { color: var(--text-muted, var(--ftbq-text-muted, #9a9aa0)); }
  .save-badge.saved { color: var(--ftbq-accent-green, #55c95a); background: rgba(85,201,90,0.12); }
  .save-badge.error { color: var(--accent-danger); background: color-mix(in srgb, var(--accent-danger) 12%, transparent); }

  .quest-preview {
    padding: 0 8px 8px;
    font-size: 12px;
  }
  .preview-row {
    display: flex;
    gap: 8px;
    margin-bottom: 2px;
  }
  .preview-label {
    color: var(--text-muted, var(--ftbq-text-muted));
    min-width: 70px;
    flex-shrink: 0;
  }
  .preview-value {
    color: var(--text-secondary, var(--ftbq-text));
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .preview-value.desc {
    white-space: normal;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .edit-form {
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-top: 1px solid var(--ftbq-frame);
    background: color-mix(in srgb, var(--ftbq-bg) 55%, transparent);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .field-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted, var(--ftbq-text-muted));
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .field input, .field textarea {
    width: 100%;
    font-size: 12px;
    padding: 5px 7px;
    font-family: inherit;
    background: var(--ftbq-input-bg);
    border: 1px solid var(--ftbq-frame);
    box-shadow: none;
    color: var(--text-primary, var(--ftbq-text));
    border-radius: var(--ftbq-radius-control);
    outline: none;
  }
  .field input:focus, .field textarea:focus {
    border-color: color-mix(in srgb, var(--accent-primary) 55%, var(--ftbq-frame));
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }
  .field textarea {
    resize: vertical;
    min-height: 60px;
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
  }
  .field-preview {
    font-size: 12px;
    padding: 4px 6px;
    background: color-mix(in srgb, var(--ftbq-bg) 70%, transparent);
    border: 1px solid var(--ftbq-frame);
    border-radius: 4px;
    min-height: 20px;
    line-height: 1.4;
    word-break: break-word;
  }
  .field-preview.desc {
    max-height: 80px;
    overflow-y: auto;
  }
  .edit-actions {
    display: flex;
    gap: 8px;
    margin-top: 4px;
    flex-wrap: wrap;
  }

  .pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 8px 12px;
    border-top: 1px solid var(--ftbq-frame);
    flex-shrink: 0;
  }
  .page-info {
    font-size: 12px;
    color: var(--text-muted, var(--ftbq-text-muted));
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
