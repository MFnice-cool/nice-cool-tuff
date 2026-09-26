<script lang="ts">
  import { X, Save, ChevronRight, ChevronDown, RefreshCw } from "@lucide/svelte";
  import type { ChunkEditorData, NbtNode } from "../lib/api";
  import { api } from "../lib/api";
  import { projectPath } from "../lib/store";

  let {
    worldName,
    dimension = "overworld",
    regionX,
    regionZ,
    index,
    onClose,
    onSaved = () => {},
  }: {
    worldName: string;
    dimension?: string;
    regionX: number;
    regionZ: number;
    index: number;
    onClose: () => void;
    onSaved?: () => void;
  } = $props();

  let data = $state<ChunkEditorData | null>(null);
  let layer = $state<"region" | "entities" | "poi">("region");
  let loading = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let expanded = $state(new Set<string>(["root"]));
  let editPath = $state<string | null>(null);
  let editValue = $state("");

  async function load() {
    if (!$projectPath || !worldName) return;
    loading = true;
    error = null;
    try {
      data = await api.worlds.readChunkEditor(
        worldName,
        regionX,
        regionZ,
        index,
        dimension,
        layer,
        $projectPath,
      );
      expanded = new Set(["root"]);
    } catch (e) {
      data = null;
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function save() {
    if (!data || !$projectPath) return;
    saving = true;
    error = null;
    try {
      await api.worlds.writeChunkEditor(worldName, data, dimension, $projectPath);
      onSaved();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  function pathKey(parts: string[]): string {
    return parts.join("/");
  }

  function toggle(path: string) {
    const next = new Set(expanded);
    if (next.has(path)) next.delete(path); else next.add(path);
    expanded = next;
  }

  function typeName(t: number): string {
    const names = ["End", "Byte", "Short", "Int", "Long", "Float", "Double", "ByteArray", "String", "List", "Compound", "IntArray", "LongArray"];
    return names[t] || `T${t}`;
  }

  function isEditableScalar(node: NbtNode): boolean {
    return [1, 2, 3, 4, 5, 6, 8].includes(node.tagType);
  }

  function startEdit(path: string, node: NbtNode) {
    if (!isEditableScalar(node)) return;
    editPath = path;
    editValue = node.value == null ? "" : String(node.value);
  }

  function applyEdit(node: NbtNode) {
    if (editPath == null) return;
    const t = node.tagType;
    if (t === 8) node.value = editValue;
    else if (t === 5 || t === 6) node.value = Number(editValue);
    else node.value = Math.trunc(Number(editValue));
    editPath = null;
    data = data; // trigger reactivity
  }

  function findNode(children: NbtNode[] | undefined, names: string[]): NbtNode | null {
    if (!children || names.length === 0) return null;
    const [head, ...rest] = names;
    const node = children.find((c) => c.name === head);
    if (!node) return null;
    if (rest.length === 0) return node;
    return findNode(node.children, rest);
  }

  $effect(() => {
    if (worldName) void load();
  });

  $effect(() => {
    if (layer) void load();
  });
</script>

<div class="editor-overlay" role="dialog" aria-modal="true">
  <div class="editor-panel">
    <div class="editor-head">
      <div>
        <strong>Chunk Editor</strong>
        <span class="meta">({regionX},{regionZ}:{index}) · chunk {data?.chunkX ?? "?"},{data?.chunkZ ?? "?"}</span>
      </div>
      <div class="head-actions">
        <select bind:value={layer} title="MCA layer">
          <option value="region">region</option>
          <option value="entities">entities</option>
          <option value="poi">poi</option>
        </select>
        <button class="ghost" onclick={load} disabled={loading} title="Reload"><RefreshCw size={14} /></button>
        <button class="ghost" onclick={save} disabled={!data || saving} title="Apply"><Save size={14} /> Apply</button>
        <button class="ghost" onclick={onClose} title="Close"><X size={14} /></button>
      </div>
    </div>

    {#if error}<div class="err">{error}</div>{/if}
    {#if loading}<div class="muted">Loading NBT…</div>{/if}

    {#if data}
      <div class="tree">
        {#each [data.root] as root (root)}
          {@const rootPath = "root"}
          <div class="node">
            <button class="row" onclick={() => toggle(rootPath)}>
              {#if expanded.has(rootPath)}<ChevronDown size={12} />{:else}<ChevronRight size={12} />{/if}
              <span class="type">Compound</span>
              <span class="name">{root.name || "(root)"}</span>
            </button>
            {#if expanded.has(rootPath)}
              <div class="children">
                {#each root.children || [] as child, i (pathKey([child.name || String(i)]))}
                  {@const p = pathKey([child.name || String(i)])}
                  {#if child.tagType === 10 || child.tagType === 9}
                    <div class="node">
                      <button class="row" onclick={() => toggle(p)}>
                        {#if expanded.has(p)}<ChevronDown size={12} />{:else}<ChevronRight size={12} />{/if}
                        <span class="type">{typeName(child.tagType)}</span>
                        <span class="name">{child.name}</span>
                        {#if child.tagType === 9}<span class="muted">[{(child.children || []).length}]</span>{/if}
                      </button>
                      {#if expanded.has(p)}
                        <div class="children">
                          {#each child.children || [] as sub, si (pathKey([child.name, sub.name || String(si)]))}
                            {@const sp = pathKey([child.name, sub.name || String(si)])}
                            <div class="node">
                              {#if sub.tagType === 10 || sub.tagType === 9}
                                <button class="row" onclick={() => toggle(sp)}>
                                  {#if expanded.has(sp)}<ChevronDown size={12} />{:else}<ChevronRight size={12} />{/if}
                                  <span class="type">{typeName(sub.tagType)}</span>
                                  <span class="name">{sub.name}</span>
                                </button>
                                {#if expanded.has(sp)}
                                  <div class="children">
                                    {#each sub.children || [] as leaf, li (pathKey([child.name, sub.name || String(si), leaf.name || String(li)]))}
                                      {@const lp = pathKey([child.name, sub.name || String(si), leaf.name || String(li)])}
                                      <div
                                        class="row leaf"
                                        role="button"
                                        tabindex="0"
                                        ondblclick={() => startEdit(lp, leaf)}
                                        onkeydown={(e) => e.key === "Enter" && startEdit(lp, leaf)}
                                      >
                                        <span class="pad"></span>
                                        <span class="type">{typeName(leaf.tagType)}</span>
                                        <span class="name">{leaf.name}</span>
                                        {#if editPath === lp}
                                          <input
                                            class="edit"
                                            bind:value={editValue}
                                            onkeydown={(e) => e.key === "Enter" && applyEdit(leaf)}
                                            onblur={() => applyEdit(leaf)}
                                          />
                                        {:else if leaf.value != null && !Array.isArray(leaf.value)}
                                          <span class="val">{String(leaf.value)}</span>
                                        {:else if leaf.tagType === 10 || leaf.tagType === 9}
                                          <span class="muted">{(leaf.children || []).length} items</span>
                                        {/if}
                                      </div>
                                    {/each}
                                  </div>
                                {/if}
                              {:else}
                                <div
                                  class="row leaf"
                                  role="button"
                                  tabindex="0"
                                  ondblclick={() => startEdit(sp, sub)}
                                  onkeydown={(e) => e.key === "Enter" && startEdit(sp, sub)}
                                >
                                  <span class="pad"></span>
                                  <span class="type">{typeName(sub.tagType)}</span>
                                  <span class="name">{sub.name}</span>
                                  {#if editPath === sp}
                                    <input
                                      class="edit"
                                      bind:value={editValue}
                                      onkeydown={(e) => e.key === "Enter" && applyEdit(sub)}
                                      onblur={() => applyEdit(sub)}
                                    />
                                  {:else if sub.value != null && !Array.isArray(sub.value)}
                                    <span class="val">{String(sub.value)}</span>
                                  {:else if Array.isArray(sub.value)}
                                    <span class="muted">array[{sub.value.length}]</span>
                                  {/if}
                                </div>
                              {/if}
                            </div>
                          {/each}
                        </div>
                      {/if}
                    </div>
                  {:else}
                    <div
                      class="row leaf"
                      role="button"
                      tabindex="0"
                      ondblclick={() => startEdit(p, child)}
                      onkeydown={(e) => e.key === "Enter" && startEdit(p, child)}
                    >
                      <span class="pad"></span>
                      <span class="type">{typeName(child.tagType)}</span>
                      <span class="name">{child.name}</span>
                      {#if editPath === p}
                        <input
                          class="edit"
                          bind:value={editValue}
                          onkeydown={(e) => e.key === "Enter" && applyEdit(child)}
                          onblur={() => applyEdit(child)}
                        />
                      {:else if child.value != null && !Array.isArray(child.value)}
                        <span class="val">{String(child.value)}</span>
                      {:else if Array.isArray(child.value)}
                        <span class="muted">array[{child.value.length}]</span>
                      {/if}
                    </div>
                  {/if}
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      </div>
      <div class="hint">Double-click scalar values to edit · Enter to apply · Apply writes to disk</div>
    {/if}
  </div>
</div>

<style>
  .editor-overlay {
    position: fixed; inset: 0; z-index: 80;
    background: rgba(0,0,0,.45);
    display: flex; align-items: center; justify-content: center;
    padding: 24px;
  }
  .editor-panel {
    width: min(720px, 96vw); max-height: 85vh;
    background: var(--bg-elevated); border: 1px solid var(--border-color);
    border-radius: 10px; display: flex; flex-direction: column; overflow: hidden;
    box-shadow: 0 16px 48px rgba(0,0,0,.45);
  }
  .editor-head {
    display: flex; justify-content: space-between; align-items: center; gap: 12px;
    padding: 12px 14px; border-bottom: 1px solid var(--border-color);
  }
  .meta { margin-left: 8px; font-size: 12px; color: var(--text-muted); font-weight: 400; }
  .head-actions { display: flex; gap: 8px; align-items: center; }
  .head-actions select {
    background: var(--bg-secondary); color: var(--text-primary);
    border: 1px solid var(--border-color); border-radius: 6px; padding: 4px 8px; font-size: 12px;
  }
  .tree { overflow: auto; padding: 8px 10px; flex: 1; font-family: var(--font-mono, ui-monospace, monospace); font-size: 12px; }
  .row {
    display: flex; align-items: center; gap: 8px; width: 100%;
    background: transparent; border: none; color: var(--text-secondary);
    padding: 3px 4px; border-radius: 4px; cursor: pointer; text-align: left;
  }
  .row:hover { background: var(--bg-tertiary); }
  .row.leaf { cursor: default; }
  .children { padding-left: 14px; }
  .type { color: var(--accent-secondary); min-width: 72px; }
  .name { color: var(--text-primary); font-weight: 600; }
  .val { color: var(--accent-primary); margin-left: 6px; word-break: break-all; }
  .muted { color: var(--text-muted); font-size: 12px; }
  .err { color: var(--accent-danger); padding: 8px 14px; font-size: 12px; }
  .pad { width: 12px; display: inline-block; }
  .edit {
    margin-left: 6px; flex: 1; min-width: 80px;
    background: var(--bg-secondary); color: var(--text-primary);
    border: 1px solid rgba(120,200,255,.5); border-radius: 4px; padding: 2px 6px; font-size: 12px;
  }
  .hint { padding: 8px 14px; font-size: 12px; color: var(--text-muted); border-top: 1px solid var(--border-color); }
  .ghost { display: inline-flex; align-items: center; gap: 8px; }
</style>
