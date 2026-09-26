<script lang="ts">
  import type { QuestChapter, QuestChapterGroup } from "../../lib/api";
  import { SHAPE_OPTIONS } from "../../lib/questTypeLabels";
  import ItemStackEditor from "./ItemStackEditor.svelte";

  let {
    chapter,
    chapterGroups = [],
    onDirty,
  }: {
    chapter: QuestChapter;
    chapterGroups?: QuestChapterGroup[];
    onDirty: () => void;
  } = $props();

  let hideDefValue = $derived(
    chapter.defaultHideDependencyLines === true
      ? "true"
      : chapter.defaultHideDependencyLines === false
        ? "false"
        : "",
  );

  function setHideDef(s: string) {
    if (s === "true") chapter.defaultHideDependencyLines = true;
    else if (s === "false") chapter.defaultHideDependencyLines = false;
    else chapter.defaultHideDependencyLines = null;
    onDirty();
  }

  let extraKey = $state("");
  let extraVal = $state("");

  function ensureExtras() {
    if (!chapter.extras) chapter.extras = {};
    return chapter.extras;
  }

  function addExtra() {
    const k = extraKey.trim();
    if (!k) return;
    let parsed: unknown = extraVal;
    try {
      parsed = JSON.parse(extraVal);
    } catch {
      /* keep string */
    }
    ensureExtras()[k] = parsed as never;
    chapter.extras = { ...chapter.extras };
    extraKey = "";
    extraVal = "";
    onDirty();
  }

  function removeExtra(k: string) {
    if (!chapter.extras) return;
    delete chapter.extras[k];
    chapter.extras = { ...chapter.extras };
    onDirty();
  }

  function selectVal(e: Event): string {
    return (e.currentTarget as HTMLSelectElement).value;
  }
  function inputVal(e: Event): string {
    return (e.currentTarget as HTMLInputElement).value;
  }
</script>

<div class="ch-set ftbq-panel">
  <h4>Chapter settings</h4>
  <label
    >Title<input
      bind:value={chapter.title}
      oninput={() => {
        chapter.titleFromSnbt = true;
        onDirty();
      }}
    /></label
  >
  <div class="icon-edit">
    <ItemStackEditor
      label="Icon"
      value={chapter.icon ?? null}
      allowFilters={false}
      onChange={(v) => {
        chapter.icon = v;
        onDirty();
      }}
    />
  </div>
  <label
    >Group
    <select
      value={chapter.group ?? ""}
      onchange={(e) => {
        chapter.group = selectVal(e) || null;
        onDirty();
      }}
    >
      <option value="">(none)</option>
      {#each chapterGroups as g (g.id)}
        <option value={g.id}>{g.title}</option>
      {/each}
    </select>
  </label>
  <label
    >Order index<input
      type="number"
      value={chapter.orderIndex ?? ""}
      oninput={(e) => {
        const v = inputVal(e);
        chapter.orderIndex = v === "" ? null : Number(v);
        onDirty();
      }}
    /></label
  >
  <label>Filename<input bind:value={chapter.filename} oninput={onDirty} placeholder="my_chapter" /></label>
  <label
    >Default quest shape
    <select
      value={chapter.defaultQuestShape ?? ""}
      onchange={(e) => {
        chapter.defaultQuestShape = selectVal(e) || null;
        onDirty();
      }}
    >
      {#each SHAPE_OPTIONS as s (s.id || "_default")}
        <option value={s.id}>{s.label}</option>
      {/each}
    </select>
  </label>
  <label
    >Default hide dependency lines
    <select
      value={hideDefValue}
      onchange={(e) => setHideDef(selectVal(e))}
    >
      <option value="">unset</option>
      <option value="true">true</option>
      <option value="false">false</option>
    </select>
  </label>

  <h4>Extra SNBT fields</h4>
  {#each Object.entries(chapter.extras ?? {}) as [k, v] (k)}
    <div class="extra-row">
      <code>{k}</code>
      <span>{typeof v === "string" ? v : JSON.stringify(v)}</span>
      <button type="button" onclick={() => removeExtra(k)}>×</button>
    </div>
  {/each}
  <div class="extra-add">
    <input placeholder="key" bind:value={extraKey} />
    <input placeholder='value or JSON' bind:value={extraVal} />
    <button type="button" onclick={addExtra}>Add</button>
  </div>
</div>

<style>
  .ch-set {
    display: grid;
    gap: 8px;
    padding: 0 0 16px;
    font-size: 12px;
    background: var(--bg-secondary, var(--ftbq-bg-panel));
    max-height: 100%;
    overflow: auto;
    overflow-x: hidden;
    min-height: 0;
    min-width: 0;
  }
  .ch-set h4 {
    margin: 0;
    padding: 10px 12px 8px;
    font-size: 13px;
    font-weight: 650;
    letter-spacing: 0;
    color: var(--text-primary, var(--ftbq-text));
    background: color-mix(in srgb, var(--ftbq-bg) 55%, transparent);
    border-bottom: 1px solid var(--ftbq-frame);
  }
  .ch-set h4:not(:first-child) {
    margin-top: 4px;
    border-top: 1px solid var(--ftbq-frame);
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary, var(--ftbq-text));
  }
  .ch-set label {
    display: grid;
    gap: 8px;
    color: var(--text-muted, var(--ftbq-text-muted));
    font-size: 12px;
    letter-spacing: 0;
    padding: 0 12px;
  }
  .ch-set label:has(> select),
  .ch-set label:has(> input) {
    font-weight: 500;
  }
  .icon-edit {
    padding: 0 12px;
  }
  .ch-set input,
  .ch-set select {
    padding: 6px 8px;
    font-size: 12px;
    background: var(--ftbq-input-bg);
    border: 1px solid var(--ftbq-frame);
    box-shadow: none;
    color: var(--text-primary, var(--ftbq-text));
    border-radius: var(--ftbq-radius-control);
    text-transform: none;
    outline: none;
    color-scheme: inherit;
  }
  .ch-set input:focus,
  .ch-set select:focus {
    border-color: color-mix(in srgb, var(--accent-primary) 55%, var(--ftbq-frame));
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }
  .extra-row {
    display: grid;
    grid-template-columns: 1fr 1fr auto;
    gap: 8px;
    align-items: center;
    padding: 0 12px;
    font-size: 12px;
    color: var(--text-secondary, var(--ftbq-text));
  }
  .extra-row code {
    font-size: 12px;
    color: var(--ftbq-accent-teal);
  }
  .extra-row button {
    width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid var(--ftbq-frame);
    border-radius: var(--ftbq-radius-control);
    background: var(--bg-secondary, var(--ftbq-bg-panel));
    color: var(--text-muted, var(--ftbq-text-muted));
    cursor: pointer;
  }
  .extra-row button:hover {
    color: var(--accent-danger);
    border-color: color-mix(in srgb, var(--accent-danger) 45%, var(--ftbq-frame));
    background: var(--bg-hover, var(--ftbq-btn-hover-top));
  }
  .extra-add {
    display: grid;
    grid-template-columns: 1fr 1fr auto;
    gap: 8px;
    padding: 0 12px 12px;
  }
  .extra-add input,
  .extra-add button {
    font-size: 12px;
    background: var(--ftbq-input-bg);
    border: 1px solid var(--ftbq-frame);
    color: var(--text-primary, var(--ftbq-text));
    border-radius: var(--ftbq-radius-control);
    box-shadow: none;
  }
  .extra-add button {
    padding: 0 10px;
    background: var(--bg-secondary, var(--ftbq-bg-panel));
    cursor: pointer;
  }
  .extra-add button:hover {
    background: var(--bg-hover, var(--ftbq-btn-hover-top));
    color: var(--ftbq-accent-green);
  }
</style>
