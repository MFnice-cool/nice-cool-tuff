<script lang="ts">
  import { X } from "@lucide/svelte";
  import type { QuestChapterGroup } from "../../lib/api";

  let {
    chapterGroups,
    groupsDirty = false,
    saving = false,
    onclose,
    onsave,
    onadd,
    onremove,
    onmove,
    ontitlechange,
  }: {
    chapterGroups: QuestChapterGroup[];
    groupsDirty?: boolean;
    saving?: boolean;
    onclose: () => void;
    onsave: () => void;
    onadd: () => void;
    onremove: (id: string) => void;
    onmove: (id: string, dir: -1 | 1) => void;
    ontitlechange: (id: string, title: string) => void;
  } = $props();
</script>

<div class="drawer drawer-wide">
  <div class="drawer-h">
    <strong>Chapter groups</strong>
    <button type="button" class="ghost" onclick={onadd}>+ Group</button>
    <button type="button" class="ghost ico" aria-label="Close panel" onclick={onclose}><X size={14} /></button>
  </div>
  {#each chapterGroups as g, gi (g.id)}
    <div class="group-row">
      <code>{g.id}</code>
      <input
        value={g.title}
        oninput={(e) => ontitlechange(g.id, (e.currentTarget as HTMLInputElement).value)}
      />
      <button
        type="button"
        class="ghost"
        disabled={gi === 0}
        title="Move up"
        onclick={() => onmove(g.id, -1)}>↑</button
      >
      <button
        type="button"
        class="ghost"
        disabled={gi === chapterGroups.length - 1}
        title="Move down"
        onclick={() => onmove(g.id, 1)}>↓</button
      >
      <button type="button" class="ghost" onclick={() => onremove(g.id)}>Remove</button>
    </div>
  {/each}
  <p class="drawer-hint">Included in Save all</p>
  <button type="button" onclick={onsave} disabled={saving || !groupsDirty}>Save groups</button>
</div>

<style>
  .drawer {
    position: relative;
    top: auto;
    right: auto;
    z-index: auto;
    width: 100%;
    max-height: none;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: transparent;
    border: none;
    border-radius: 0;
    box-shadow: none;
    overflow: visible;
  }
  .drawer-wide {
    width: 100%;
    max-height: none;
    overflow: visible;
  }
  .drawer-h {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .drawer-h strong {
    flex: 1;
  }
  .drawer-hint {
    margin: 0;
    font-size: 12px;
    color: var(--ftbq-text-muted);
  }
  .group-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .group-row input {
    flex: 1;
    background: var(--ftbq-input-bg);
    border: 1px solid var(--ftbq-frame);
    color: inherit;
    border-radius: 3px;
    padding: 6px 8px;
    box-shadow: inset 1px 1px 3px rgba(0, 0, 0, 0.55);
  }
</style>
