<script lang="ts">
  import type { QuestChapter, QuestData } from "../../lib/store";
  import { exportChapterSnbt } from "../../lib/store";
  import { mcFormat, stripCodes } from "../../lib/mcformat";

  interface Props {
    chapters: QuestChapter[];
    onQuestUpdate: (chapterId: string, quest: QuestData) => void;
    onExportChapter: (chapterId: string) => void;
  }

  let { chapters, onQuestUpdate, onExportChapter }: Props = $props();

  // Search state
  let query = $state("");
  let caseSensitive = $state(false);
  let noTitle = $state(false);
  let noSubtitle = $state(false);
  let noDescription = $state(false);
  let scopeChapter = $state("");
  let perPage = $state(10);
  let page = $state(0);

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
    if (!query.trim() && !noTitle && !noSubtitle && !noDescription) return [];

    const terms = query.trim()
      ? query.trim().split(/\s+/).filter(Boolean)
      : [];
    const searchLower = caseSensitive ? undefined : query.toLowerCase();

    const filtered: BatchResult[] = [];

    for (const ch of chapters) {
      if (scopeChapter && ch.title !== scopeChapter && ch.id !== scopeChapter) continue;

      for (const q of ch.quests) {
        // Filter: quests missing specific fields
        if (noTitle && q.title?.trim()) continue;
        if (noSubtitle && q.subtitle?.trim()) continue;
        if (noDescription && q.description?.length) continue;

        // Text search with color code stripping
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

  // Reset page on search change
  $effect(() => {
    void query;
    void scopeChapter;
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

  function matchColor(field: string): string {
    if (field.includes("missing")) return "var(--danger)";
    return "var(--text-muted)";
  }
</script>

<div class="batch-editor">
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
      bind:value={query}
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
        {#each chapters as ch}
          <option value={ch.id}>{ch.title} ({ch.quests.length})</option>
        {/each}
      </select>
      <select bind:value={perPage}>
        <option value={5}>5 per page</option>
        <option value={10}>10 per page</option>
        <option value={20}>20 per page</option>
        <option value={50}>50 per page</option>
      </select>
    </div>
  </div>

  {#if results.length === 0}
    <div class="batch-empty">
      {#if query.trim() || noTitle || noSubtitle || noDescription}
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
            onkeydown={(e) => { if (e.key === "Enter") startEdit(r.quest); }}
          >
            <span class="quest-chapter" style="color: {matchColor(r.matchField)}">
              {r.chapterTitle}
            </span>
            <span class="quest-id">{r.quest.id}</span>
            {#if saveStatus[r.quest.id]}
              <span class="save-badge" class:saving={saveStatus[r.quest.id] === "saving"}
                class:saved={saveStatus[r.quest.id] === "saved"}
                class:error={saveStatus[r.quest.id] === "error"}>
                {saveStatus[r.quest.id] === "saving" ? "Saving..." :
                 saveStatus[r.quest.id] === "saved" ? "Saved" : "Error"}
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
                  Save
                </button>
                <button type="button" class="btn ghost small" onclick={cancelEdit}>
                  Cancel
                </button>
                <button type="button" class="btn ghost small"
                  onclick={() => onExportChapter(r.chapterId)}>
                  Export chapter
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

<style>
  .batch-editor {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }
  .batch-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .batch-title {
    font-size: 12px;
    font-weight: 700;
    color: var(--accent);
  }
  .batch-count {
    font-size: 10px;
    color: var(--text-muted);
  }
  .batch-search {
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
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
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
  .scope-row {
    display: flex;
    gap: 8px;
  }
  .scope-row select {
    flex: 1;
    font-size: 11px;
    padding: 4px 6px;
  }

  .batch-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    font-size: 12px;
    padding: 24px;
  }

  .batch-results {
    flex: 1;
    overflow-y: auto;
    padding: 8px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .quest-card {
    border: 1px solid var(--border);
    border-radius: 3px;
    overflow: hidden;
    transition: border-color 0.15s;
  }
  .quest-card:hover { border-color: var(--accent); }
  .quest-card.editing { border-color: var(--accent); }

  .quest-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    background: var(--bg-secondary);
    cursor: pointer;
    font-size: 11px;
  }
  .quest-chapter {
    font-weight: 600;
    white-space: nowrap;
  }
  .quest-id {
    font-family: monospace;
    color: var(--text-muted);
    font-size: 10px;
  }
  .save-badge {
    margin-left: auto;
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 2px;
    font-weight: 600;
  }
  .save-badge.saving { color: var(--text-muted); }
  .save-badge.saved { color: var(--success); background: rgba(85,201,90,0.15); }
  .save-badge.error { color: var(--danger); background: rgba(239,68,68,0.15); }

  .quest-preview {
    padding: 6px 8px;
    font-size: 11px;
  }
  .preview-row {
    display: flex;
    gap: 6px;
    margin-bottom: 2px;
  }
  .preview-label {
    color: var(--text-muted);
    min-width: 70px;
    flex-shrink: 0;
  }
  .preview-value {
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .preview-value.desc {
    white-space: normal;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .edit-form {
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    background: var(--bg-primary);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .field-label {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .field input, .field textarea {
    width: 100%;
    font-size: 12px;
    padding: 4px 6px;
    font-family: inherit;
  }
  .field textarea {
    resize: vertical;
    min-height: 60px;
    font-family: monospace;
    font-size: 11px;
  }
  .field-preview {
    font-size: 11px;
    padding: 4px 6px;
    background: rgba(0,0,0,0.15);
    border-radius: 2px;
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
    gap: 6px;
    margin-top: 4px;
  }

  .pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 8px 12px;
    border-top: 1px solid var(--border);
    flex-shrink: 0;
  }
  .page-info {
    font-size: 11px;
    color: var(--text-muted);
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
