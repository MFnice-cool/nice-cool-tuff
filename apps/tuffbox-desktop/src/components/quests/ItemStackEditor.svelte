<script lang="ts">
  import { Package } from "@lucide/svelte";
  import QuestItemIcon from "./QuestItemIcon.svelte";
  import ItemPicker from "./ItemPicker.svelte";
  import {
    applyTagJson,
    convertToMode,
    detectMode,
    filterKind,
    isFilterCompound,
    isItemObject,
    listFilterChildren,
    readCount,
    readFilterTagValue,
    readTagJson,
    setFilterChildren,
    setFilterTagValue,
    setStackCount,
    setStackId,
    stackDisplayId,
    type ItemEditMode,
    type ItemValue,
  } from "../../lib/itemStack";

  let {
    value = null,
    onChange,
    allowFilters = true,
    label = "Item",
    emphasizeEmpty = false,
    emptyCta = "Choose item",
  }: {
    value?: ItemValue | null;
    onChange: (next: ItemValue | null) => void;
    allowFilters?: boolean;
    label?: string;
    emphasizeEmpty?: boolean;
    emptyCta?: string;
  } = $props();

  let pickerOpen = $state(false);
  let pickerTarget = $state<"main" | number>("main");
  let tagLocal = $state<string | null>(null);
  let tagJsonError = $state("");
  let tagKey = $state<"tag" | "components">("tag");

  let mode = $derived(detectMode(value ?? null));
  let displayId = $derived(stackDisplayId(value ?? null));
  let isEmpty = $derived(!displayId);
  let isFilter = $derived(isFilterCompound(value ?? null));
  let children = $derived(
    isItemObject(value) ? listFilterChildren(value) : ([] as ItemValue[]),
  );
  let tagDisplay = $derived(tagLocal ?? readTagJson(value ?? null));
  let modeOptions = $derived(
    allowFilters
      ? ([
          { id: "simple", label: "Simple" },
          { id: "stack", label: "Stack" },
          { id: "filter_or", label: "Filter OR" },
          { id: "filter_and", label: "Filter AND" },
          { id: "filter_tag", label: "Filter tag" },
        ] as { id: ItemEditMode; label: string }[])
      : ([
          { id: "simple", label: "Simple" },
          { id: "stack", label: "Stack" },
        ] as { id: ItemEditMode; label: string }[]),
  );

  function emit(next: ItemValue | null) {
    onChange(next);
  }

  function setMode(next: ItemEditMode) {
    if (!allowFilters && next.startsWith("filter_")) return;
    tagLocal = null;
    tagJsonError = "";
    if (next === "stack") {
      const converted = convertToMode(value ?? null, next);
      tagKey =
        isItemObject(converted) &&
        converted.components != null &&
        converted.tag == null
          ? "components"
          : "tag";
      emit(converted);
      return;
    }
    emit(convertToMode(value ?? null, next));
  }

  function onIdInput(raw: string) {
    if (mode === "simple") {
      emit(raw.trim() || null);
      return;
    }
    if (mode === "stack") {
      emit(setStackId(value ?? null, raw));
    }
  }

  function onCountInput(n: number) {
    emit(setStackCount(value ?? null, n));
  }

  function openPicker(target: "main" | number) {
    pickerTarget = target;
    pickerOpen = true;
  }

  function onPick(itemId: string) {
    if (pickerTarget === "main") {
      if (mode === "simple") emit(itemId);
      else if (mode === "stack") emit(setStackId(value ?? null, itemId));
    } else if (typeof pickerTarget === "number" && isItemObject(value)) {
      const nextKids = [...listFilterChildren(value)];
      nextKids[pickerTarget] = itemId;
      emit(setFilterChildren(value, nextKids));
    }
  }

  function addChild() {
    if (!isItemObject(value)) return;
    const nextKids = [...listFilterChildren(value), "minecraft:stone"];
    emit(setFilterChildren(value, nextKids));
  }

  function removeChild(i: number) {
    if (!isItemObject(value)) return;
    const nextKids = listFilterChildren(value).filter((_, idx) => idx !== i);
    emit(setFilterChildren(value, nextKids.length ? nextKids : ["minecraft:stone"]));
  }

  function setChildId(i: number, id: string) {
    if (!isItemObject(value)) return;
    const nextKids = [...listFilterChildren(value)];
    nextKids[i] = id.trim() || "minecraft:stone";
    emit(setFilterChildren(value, nextKids));
  }

  function onTagFilterInput(raw: string) {
    if (!isItemObject(value)) {
      emit(setFilterTagValue({ id: "itemfilters:tag" }, raw));
      return;
    }
    emit(setFilterTagValue(value, raw));
  }

  function commitTagJson() {
    const next = applyTagJson(value ?? null, tagDisplay, tagKey);
    if (next == null) {
      tagJsonError = "Invalid JSON";
      return;
    }
    tagJsonError = "";
    tagLocal = null;
    emit(next);
  }
</script>

<div class="ise">
  <div class="ise-h">
    <span class="lbl">{label}</span>
    {#if isFilter}
      <span class="badge" title={typeof value === "object" && value && "id" in value ? String(value.id) : ""}
        >filter{filterKind(value) ? `: ${filterKind(value)}` : ""}</span
      >
    {/if}
    <select
      class="mode"
      value={mode}
      onchange={(e) => setMode((e.currentTarget as HTMLSelectElement).value as ItemEditMode)}
    >
      {#each modeOptions as opt (opt.id)}
        <option value={opt.id}>{opt.label}</option>
      {/each}
      {#if !modeOptions.some((o) => o.id === mode)}
        <option value={mode}>{mode}</option>
      {/if}
    </select>
  </div>

  {#if mode === "simple"}
    <div class="item-row">
      <QuestItemIcon itemId={displayId} fallback="?" size={28} />
      <input
        value={typeof value === "string" ? value : (displayId ?? "")}
        oninput={(e) => onIdInput((e.currentTarget as HTMLInputElement).value)}
        placeholder="Pick item"
      />
      <button type="button" class="pick" onclick={() => openPicker("main")} title="Pick item" aria-label="Pick item"
        ><Package size={12} /></button
      >
    </div>
    {#if emphasizeEmpty && isEmpty}
      <button type="button" class="choose-cta" onclick={() => openPicker("main")}>{emptyCta}</button>
    {/if}
  {:else if mode === "stack"}
    <div class="item-row">
      <QuestItemIcon itemId={displayId} fallback="?" size={28} />
      <input
        value={displayId ?? ""}
        oninput={(e) => onIdInput((e.currentTarget as HTMLInputElement).value)}
        placeholder="Pick item"
      />
      <button type="button" class="pick" onclick={() => openPicker("main")} title="Pick item" aria-label="Pick item"
        ><Package size={12} /></button
      >
    </div>
    {#if emphasizeEmpty && isEmpty}
      <button type="button" class="choose-cta" onclick={() => openPicker("main")}>{emptyCta}</button>
    {/if}
    <label class="field"
      >Count<input
        type="number"
        min="1"
        value={readCount(value)}
        oninput={(e) => onCountInput(Number((e.currentTarget as HTMLInputElement).value) || 1)}
      /></label
    >
    <details class="extra">
      <summary>tag / components JSON</summary>
      <div class="tag-row">
        <select bind:value={tagKey}>
          <option value="tag">tag</option>
          <option value="components">components</option>
        </select>
        <button type="button" class="mini" onclick={commitTagJson}>Apply</button>
      </div>
      <textarea
        rows="4"
        value={tagDisplay}
        placeholder={'{"Damage":0}'}
        oninput={(e) => {
          tagLocal = (e.currentTarget as HTMLTextAreaElement).value;
        }}
        onchange={commitTagJson}
      ></textarea>
      {#if tagJsonError}<p class="err">{tagJsonError}</p>{/if}
    </details>
  {:else if mode === "filter_or" || mode === "filter_and"}
    <div class="kids">
      {#each children as child, i (i)}
        <div class="item-row">
          <QuestItemIcon itemId={stackDisplayId(child)} fallback="?" size={22} />
          <input
            value={typeof child === "string" ? child : (stackDisplayId(child) ?? "")}
            oninput={(e) => setChildId(i, (e.currentTarget as HTMLInputElement).value)}
            placeholder="modid:item"
          />
          <button type="button" class="pick" onclick={() => openPicker(i)} title="Pick item" aria-label="Pick item"
            ><Package size={12} /></button
          >
          <button type="button" class="mini danger" onclick={() => removeChild(i)}>×</button>
        </div>
      {/each}
      <button type="button" class="mini add" onclick={addChild}>+ item</button>
    </div>
  {:else if mode === "filter_tag"}
    <label class="field"
      >Tag
      <input
        value={isItemObject(value) ? readFilterTagValue(value) : ""}
        oninput={(e) => onTagFilterInput((e.currentTarget as HTMLInputElement).value)}
        placeholder="#minecraft:logs"
      />
    </label>
  {/if}
</div>

<ItemPicker
  open={pickerOpen}
  onPick={onPick}
  onClose={() => {
    pickerOpen = false;
  }}
/>

<style>
  .ise {
    display: grid;
    gap: 8px;
  }
  .ise-h {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .lbl {
    font-size: 12px;
    text-transform: none;
    letter-spacing: 0;
    color: var(--ftbq-text-muted);
  }
  .badge {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
    padding: 1px 6px;
    border-radius: 2px;
    background: rgba(61, 184, 168, 0.15);
    color: var(--ftbq-accent-teal);
    border: 1px solid rgba(61, 184, 168, 0.35);
  }
  .mode {
    margin-left: auto;
    font-size: 12px;
    background: var(--ftbq-bg);
    border: 1px solid var(--ftbq-border);
    color: inherit;
    border-radius: 2px;
    padding: 3px 6px;
  }
  .item-row {
    display: grid;
    grid-template-columns: auto 1fr auto auto;
    gap: 8px;
    align-items: center;
  }
  .item-row input,
  .field input,
  textarea,
  select {
    font-size: 12px;
    background: var(--ftbq-bg);
    border: 1px solid var(--ftbq-border);
    color: inherit;
    border-radius: 2px;
    padding: 6px 8px;
  }
  .field {
    display: grid;
    gap: 8px;
    font-size: 12px;
    text-transform: none;
    color: var(--ftbq-text-muted);
  }
  .choose-cta {
    width: 100%;
    margin-top: 4px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--accent-primary) 55%, var(--ftbq-border));
    border-radius: 4px;
    background: color-mix(in srgb, var(--accent-primary) 18%, transparent);
    color: var(--text-primary, var(--ftbq-text));
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .choose-cta:hover {
    background: color-mix(in srgb, var(--accent-primary) 28%, transparent);
  }
  .pick,
  .mini {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 2px;
    border: 1px solid var(--ftbq-border);
    background: transparent;
    color: var(--ftbq-text-muted);
    cursor: pointer;
    padding: 6px;
    font-size: 12px;
  }
  .mini.add {
    justify-self: start;
    color: var(--ftbq-accent-green);
  }
  .mini.danger {
    color: var(--accent-danger);
  }
  .kids {
    display: grid;
    gap: 8px;
  }
  .extra {
    font-size: 12px;
    color: var(--ftbq-text-muted);
  }
  .extra summary {
    cursor: pointer;
  }
  .tag-row {
    display: flex;
    gap: 8px;
    margin: 6px 0;
  }
  textarea {
    width: 100%;
    resize: vertical;
    font-family: var(--font-mono, ui-monospace, monospace);
  }
  .err {
    margin: 0;
    color: var(--accent-danger);
    font-size: 12px;
  }
</style>
