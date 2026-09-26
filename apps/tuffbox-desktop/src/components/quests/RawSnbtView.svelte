<script lang="ts">
  import {
    type QuestChapter,
    chapterToSnbtJson,
    stripLocaleOverlay,
  } from "../../lib/api";
  import { serializeSnbt, type SnbtValue } from "../../lib/snbtSerialize";

  interface Props {
    chapter: QuestChapter;
    selectedQuestId?: string | null;
    onDiffVsDisk?: () => void;
  }

  let { chapter, selectedQuestId = null, onDiffVsDisk }: Props = $props();

  let copied = $state(false);
  let findQuery = $state("");
  let matchIndex = $state(0);
  let preEl = $state<HTMLPreElement | null>(null);

  let raw = $derived(
    serializeSnbt(stripLocaleOverlay(chapterToSnbtJson(chapter)) as SnbtValue)
  );

  let lines = $derived(raw.split("\n"));

  let matchLineIndexes = $derived.by(() => {
    const q = findQuery.trim().toLowerCase();
    if (!q) return [] as number[];
    const hits: number[] = [];
    for (let i = 0; i < lines.length; i++) {
      if (lines[i]!.toLowerCase().includes(q)) hits.push(i);
    }
    return hits;
  });

  $effect(() => {
    void findQuery;
    matchIndex = 0;
  });

  $effect(() => {
    const hits = matchLineIndexes;
    if (!hits.length || !preEl) return;
    const line = hits[Math.min(matchIndex, hits.length - 1)]!;
    const lineHeight = 16.5; // approx 11px * 1.5
    preEl.scrollTop = Math.max(0, line * lineHeight - 40);
  });

  function copyToClipboard() {
    navigator.clipboard.writeText(raw).then(() => {
      copied = true;
      setTimeout(() => (copied = false), 1500);
    });
  }

  function jumpToQuest() {
    if (!selectedQuestId || !preEl) return;
    const needle = selectedQuestId.toLowerCase();
    const idx = lines.findIndex((ln) => ln.toLowerCase().includes(needle));
    if (idx < 0) return;
    findQuery = selectedQuestId;
    matchIndex = 0;
    const lineHeight = 16.5;
    preEl.scrollTop = Math.max(0, idx * lineHeight - 40);
  }

  function nextMatch(dir: 1 | -1) {
    const n = matchLineIndexes.length;
    if (!n) return;
    matchIndex = (matchIndex + dir + n) % n;
  }
</script>

<div class="raw-view">
  <div class="raw-header">
    <div class="raw-header-text">
      <span class="raw-title">{chapter.title} — Raw SNBT</span>
      <span class="raw-note">Locale title/description stripped (match save). Copy button.</span>
    </div>
    <div class="raw-actions">
      {#if onDiffVsDisk}
        <button type="button" class="btn ghost small" onclick={() => onDiffVsDisk()}>
          Diff vs disk
        </button>
      {/if}
      <button
        type="button"
        class="btn ghost small"
        disabled={!selectedQuestId}
        onclick={jumpToQuest}
        title={selectedQuestId ? `Jump to ${selectedQuestId}` : "Select a quest first"}
      >
        Jump to quest
      </button>
      <button type="button" class="btn ghost small" onclick={copyToClipboard}>
        {copied ? "Copied!" : "Copy"}
      </button>
    </div>
  </div>
  <div class="raw-find">
    <input
      type="search"
      placeholder="Find in SNBT…"
      aria-label="Find in SNBT"
      bind:value={findQuery}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          nextMatch(e.shiftKey ? -1 : 1);
        }
      }}
    />
    <span class="find-count">
      {matchLineIndexes.length
        ? `${Math.min(matchIndex + 1, matchLineIndexes.length)}/${matchLineIndexes.length}`
        : findQuery.trim()
          ? "0"
          : ""}
    </span>
    <button type="button" class="btn ghost small" disabled={!matchLineIndexes.length} onclick={() => nextMatch(-1)}>Prev</button>
    <button type="button" class="btn ghost small" disabled={!matchLineIndexes.length} onclick={() => nextMatch(1)}>Next</button>
  </div>
  <pre class="raw-code" bind:this={preEl}><code>{raw}</code></pre>
</div>

<style>
  .raw-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    background: var(--bg-secondary, var(--ftbq-bg-panel));
  }
  .raw-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--ftbq-frame);
    flex-shrink: 0;
  }
  .raw-header-text {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }
  .raw-title {
    font-size: 14px;
    color: var(--text-primary, var(--ftbq-text));
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .raw-note {
    font-size: 12px;
    color: var(--text-muted, var(--ftbq-text-muted));
    line-height: 1.35;
  }
  .raw-actions {
    display: flex;
    gap: 8px;
    flex-shrink: 0;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .raw-find {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--ftbq-frame);
    background: color-mix(in srgb, var(--ftbq-bg) 55%, transparent);
    flex-shrink: 0;
  }
  .raw-find input {
    flex: 1;
    min-width: 0;
    padding: 5px 8px;
    font-size: 12px;
    background: var(--ftbq-input-bg);
    border: 1px solid var(--ftbq-frame);
    border-radius: var(--ftbq-radius-control);
    color: var(--text-primary, var(--ftbq-text));
    outline: none;
  }
  .raw-find input:focus {
    border-color: color-mix(in srgb, var(--accent-primary) 55%, var(--ftbq-frame));
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }
  .find-count {
    font-size: 12px;
    color: var(--text-muted, var(--ftbq-text-muted));
    min-width: 2.5em;
    text-align: right;
  }
  .raw-code {
    flex: 1;
    overflow: auto;
    padding: 12px;
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    font-family: var(--font-mono, ui-monospace, monospace);
    background: color-mix(in srgb, var(--ftbq-bg) 70%, transparent);
    color: var(--text-secondary, var(--ftbq-text));
    white-space: pre;
    tab-size: 2;
  }
  .btn {
    padding: 4px 8px;
    border: 1px solid var(--ftbq-frame);
    background: var(--bg-secondary, var(--ftbq-bg-panel));
    color: var(--text-primary, var(--ftbq-text));
    font-size: 12px;
    font-weight: 600;
    border-radius: var(--ftbq-radius-control);
    cursor: pointer;
    flex-shrink: 0;
  }
  .btn:hover:not(:disabled) {
    background: var(--bg-hover, var(--ftbq-btn-hover-top));
    border-color: color-mix(in srgb, var(--accent-primary) 40%, var(--ftbq-frame));
  }
  .btn:disabled { opacity: 0.45; cursor: default; }
  .btn.ghost { background: transparent; }
  .btn.small { padding: 3px 7px; font-size: 12px; }
</style>
