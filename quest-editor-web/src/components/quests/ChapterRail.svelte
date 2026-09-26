<script lang="ts">
  import type { QuestChapter, QuestChapterGroup } from "../../lib/store";

  let {
    chapters,
    chapterGroups = [],
    selectedChapter,
    dirtyIds,
    onSelect,
    onCreate,
    onDirty,
    onDelete,
    onMove,
    onExport,
    onRename,
  }: {
    chapters: QuestChapter[];
    chapterGroups?: QuestChapterGroup[];
    selectedChapter: string;
    dirtyIds: Set<string>;
    onSelect: (id: string) => void;
    onCreate: () => void;
    onDirty: (id: string) => void;
    onDelete: (id: string) => void;
    onMove: (id: string, dir: -1 | 1) => void;
    onExport: (id: string) => void;
    onRename: (id: string, title: string) => void;
  } = $props();

  let editingId = $state<string | null>(null);
  let menuId = $state<string | null>(null);

  function stripMc(s: string): string {
    return s.replace(/§[0-9a-fk-or]/gi, "").replace(/&[0-9a-fk-or]/gi, "").trim();
  }

  function glyph(ch: QuestChapter): string {
    const icon = ch.icon?.trim();
    if (icon) {
      const leaf = icon.includes(":") ? icon.split(":").pop()! : icon;
      return (leaf[0] || "?").toUpperCase();
    }
    return (stripMc(ch.title)[0] || "?").toUpperCase();
  }

  function commitTitle(ch: QuestChapter, value: string) {
    const next = value.trim();
    if (next && next !== ch.title) {
      onRename(ch.id, next);
    }
    editingId = null;
  }
</script>

<aside class="rail">
  <div class="rail-h">
    <h3>Chapters</h3>
    <button type="button" class="ico" title="Add chapter" onclick={onCreate}>+</button>
  </div>

  <div class="rail-list">
    {#each chapters as ch (ch.id)}
      <div class="ch-wrap" class:sel={selectedChapter === ch.id} class:dirty={dirtyIds.has(ch.id)}>
        <button
          type="button"
          class="ch-row"
          onclick={() => onSelect(ch.id)}
          ondblclick={() => (editingId = ch.id)}
        >
          <span class="glyph">{glyph(ch)}</span>
          <span class="ch-text">
            {#if editingId === ch.id}
              <input
                class="title-edit"
                value={ch.title}
                autofocus
                onclick={(e) => e.stopPropagation()}
                onkeydown={(e) => {
                  if (e.key === "Enter") commitTitle(ch, (e.target as HTMLInputElement).value);
                  if (e.key === "Escape") editingId = null;
                }}
                onblur={(e) => commitTitle(ch, (e.target as HTMLInputElement).value)}
              />
            {:else}
              <strong>{stripMc(ch.title)}</strong>
            {/if}
            <span>{ch.quests.length} quests</span>
          </span>
          {#if dirtyIds.has(ch.id)}<span class="dot">●</span>{/if}
        </button>
        <div class="ch-actions">
          <button type="button" class="ico-sm" title="Move up" onclick={() => onMove(ch.id, -1)}>↑</button>
          <button type="button" class="ico-sm" title="Move down" onclick={() => onMove(ch.id, 1)}>↓</button>
          <button type="button" class="ico-sm" title="Export" onclick={() => onExport(ch.id)}>↓</button>
          <button type="button" class="ico-sm danger" title="Delete" onclick={() => onDelete(ch.id)}>×</button>
        </div>
      </div>
    {/each}
  </div>
</aside>

<style>
  .rail {
    width: 200px;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
    border-right: 1px solid var(--border);
    flex-shrink: 0;
  }
  .rail-h {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
    background: rgba(0,0,0,0.15);
  }
  .rail-h h3 {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-muted);
    font-weight: 700;
  }
  .ico {
    width: 22px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--border);
    background: rgba(0,0,0,0.25);
    color: var(--text-muted);
    border-radius: 2px;
    font-size: 14px;
    cursor: pointer;
  }
  .ico:hover { color: var(--text-primary); border-color: var(--accent); }
  .rail-list {
    flex: 1;
    overflow: auto;
    padding: 4px 0;
  }
  .ch-wrap {
    display: flex;
    align-items: stretch;
    border-left: 3px solid transparent;
  }
  .ch-wrap.sel { background: rgba(85,201,90,0.12); border-left-color: var(--success); }
  .ch-wrap.dirty:not(.sel) { border-left-color: rgba(242,201,76,0.4); }
  .ch-row {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border: none;
    background: transparent;
    color: var(--text-primary);
    text-align: left;
    cursor: pointer;
    min-width: 0;
  }
  .ch-row:hover { background: rgba(255,255,255,0.04); }
  .glyph {
    width: 22px;
    height: 22px;
    flex-shrink: 0;
    border-radius: 2px;
    background: var(--node-bg);
    border: 2px solid var(--border);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    font-weight: 800;
    color: var(--text-primary);
  }
  .ch-wrap.sel .glyph { border-color: var(--success); }
  .ch-text {
    display: grid;
    gap: 1px;
    min-width: 0;
    flex: 1;
  }
  .ch-text strong {
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ch-text span { font-size: 9px; color: var(--text-muted); }
  .title-edit {
    font-size: 12px;
    font-weight: 600;
    padding: 2px 4px;
    width: 100%;
  }
  .dot { color: var(--warning); font-size: 10px; }
  .ch-actions {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 2px 4px;
    opacity: 0;
    transition: opacity 0.15s;
  }
  .ch-wrap:hover .ch-actions, .ch-wrap.sel .ch-actions { opacity: 1; }
  .ico-sm {
    width: 16px;
    height: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: 10px;
    cursor: pointer;
    border-radius: 2px;
    padding: 0;
  }
  .ico-sm:hover { color: var(--text-primary); background: rgba(255,255,255,0.08); }
  .ico-sm.danger:hover { color: var(--danger); }
</style>
