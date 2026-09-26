<script lang="ts">
  import { Plus, Save, Trash2 } from "@lucide/svelte";
  import {
    rewardEntryId,
    rewardEntryWeight,
    type QuestRewardTable,
  } from "../../lib/api";
  import { REWARD_TYPE_OPTIONS } from "../../lib/questTypeLabels";
  import type { ItemValue } from "../../lib/itemStack";
  import ItemStackEditor from "./ItemStackEditor.svelte";

  const KNOWN_REWARD_KEYS = new Set([
    "id",
    "weight",
    "type",
    "item",
    "table",
    "loot_crate",
    "Count",
    "count",
    "title",
  ]);

  let {
    tables,
    dirty = false,
    saving = false,
    tableIds = [],
    onChange,
    onSave,
    onCreate,
  }: {
    tables: QuestRewardTable[];
    dirty?: boolean;
    saving?: boolean;
    tableIds?: string[];
    onChange: () => void;
    onSave: (table: QuestRewardTable) => void;
    onCreate: () => void;
  } = $props();

  let selectedId = $state("");
  let extrasDraft = $state<Record<number, string>>({});

  $effect(() => {
    if (tables.length === 0) {
      selectedId = "";
    } else if (!tables.some((t) => t.id === selectedId)) {
      selectedId = tables[0].id;
    }
  });

  let selected = $derived(tables.find((t) => t.id === selectedId) ?? null);

  function newEntryId(): string {
    return `r_${Date.now().toString(16).slice(-6)}`;
  }

  function addEntry() {
    if (!selected) return;
    selected.rewards = [
      ...selected.rewards,
      {
        id: newEntryId(),
        type: "item",
        item: "minecraft:stone",
        Count: 1,
        weight: 1,
      },
    ];
    extrasDraft = {}; // indices shifted — drop stale drafts
    onChange();
  }

  function removeEntry(i: number) {
    if (!selected) return;
    selected.rewards = selected.rewards.filter((_, idx) => idx !== i);
    extrasDraft = {}; // indices shifted — drop stale drafts
    onChange();
  }

  function setField(entry: Record<string, unknown>, key: string, value: unknown) {
    if (value === "" || value == null) delete entry[key];
    else entry[key] = value;
    onChange();
  }

  function entryType(entry: Record<string, unknown>): string {
    return typeof entry.type === "string" ? entry.type : "item";
  }

  function entryItem(entry: Record<string, unknown>): ItemValue | null {
    const v = entry.item;
    if (typeof v === "string") return v;
    if (v && typeof v === "object" && !Array.isArray(v)) return v as Record<string, unknown>;
    return null;
  }

  function setEntryItem(entry: Record<string, unknown>, next: ItemValue | null) {
    if (next == null || next === "") delete entry.item;
    else entry.item = next;
    onChange();
  }

  function extrasOf(entry: Record<string, unknown>): Record<string, unknown> {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(entry)) {
      if (!KNOWN_REWARD_KEYS.has(k)) out[k] = v;
    }
    return out;
  }

  function extrasText(entry: Record<string, unknown>, i: number): string {
    if (extrasDraft[i] !== undefined) return extrasDraft[i];
    const ex = extrasOf(entry);
    return Object.keys(ex).length ? JSON.stringify(ex, null, 2) : "";
  }

  function applyExtras(entry: Record<string, unknown>, i: number) {
    const raw = extrasDraft[i] ?? extrasText(entry, i);
    let parsed: Record<string, unknown> = {};
    const trimmed = raw.trim();
    if (trimmed) {
      try {
        const v = JSON.parse(trimmed) as unknown;
        if (!v || typeof v !== "object" || Array.isArray(v)) return;
        parsed = v as Record<string, unknown>;
      } catch {
        return;
      }
    }
    for (const k of Object.keys(entry)) {
      if (!KNOWN_REWARD_KEYS.has(k)) delete entry[k];
    }
    for (const [k, v] of Object.entries(parsed)) {
      if (!KNOWN_REWARD_KEYS.has(k)) entry[k] = v;
    }
    const next = { ...extrasDraft };
    delete next[i];
    extrasDraft = next;
    onChange();
  }

  function needsTable(type: string): boolean {
    return type === "random" || type === "choice" || type === "table";
  }
</script>

<details class="rt">
  <summary>Reward tables ({tables.length})</summary>
  <div class="rt-body">
    <div class="rt-side">
      {#if tables.length === 0}
        <div class="rt-empty">
          <p>No reward tables yet.</p>
          <button type="button" class="add primary" onclick={onCreate}><Plus size={12} /> Create table</button>
        </div>
      {:else}
        {#each tables as t (t.id)}
          <button
            type="button"
            class:sel={selected?.id === t.id}
            onclick={() => (selectedId = t.id)}>{t.title || t.id}</button
          >
        {/each}
        <button type="button" class="add" onclick={onCreate}><Plus size={12} /> New table</button>
      {/if}
    </div>
    {#if selected}
      <div class="rt-edit">
        <label>Id<input bind:value={selected.id} oninput={onChange} /></label>
        <label>Title<input bind:value={selected.title} oninput={onChange} placeholder="Optional" /></label>
        <label
          >Empty weight<input
            type="number"
            step="0.1"
            min="0"
            bind:value={selected.emptyWeight}
            oninput={onChange}
          /></label
        >
        <div class="entries-h">
          <strong>Weighted rewards</strong>
          <button type="button" class="mini" aria-label="Add reward entry" title="Add reward entry" onclick={addEntry}><Plus size={12} /></button>
        </div>
        {#each selected.rewards as entry, i (i)}
          <div class="entry-card">
            <div class="entry-top">
              <input
                value={rewardEntryId(entry)}
                oninput={(e) =>
                  setField(entry, "id", (e.currentTarget as HTMLInputElement).value)}
                placeholder="reward id"
              />
              <input
                type="number"
                step="0.1"
                min="0"
                value={rewardEntryWeight(entry)}
                oninput={(e) => {
                  const n = (e.currentTarget as HTMLInputElement).valueAsNumber;
                  setField(entry, "weight", Number.isFinite(n) ? n : 1);
                }}
                title="Weight (0 = always)"
              />
              <button
                type="button"
                class="ico"
                aria-label={`Remove reward ${rewardEntryId(entry) || i + 1}`}
                title="Remove reward"
                onclick={() => removeEntry(i)}
                ><Trash2 size={12} /></button
              >
            </div>
            <label
              >Type
              <select
                value={entryType(entry)}
                onchange={(e) =>
                  setField(entry, "type", (e.currentTarget as HTMLSelectElement).value)}
              >
                {#each REWARD_TYPE_OPTIONS as t (t.id)}
                  <option value={t.id}>{t.label}</option>
                {/each}
                {#if !REWARD_TYPE_OPTIONS.some((t) => t.id === entryType(entry))}
                  <option value={entryType(entry)}>{entryType(entry)}</option>
                {/if}
              </select>
            </label>
            {#if entryType(entry) === "item"}
              <ItemStackEditor
                value={entryItem(entry)}
                allowFilters={true}
                onChange={(v) => setEntryItem(entry, v)}
              />
            {:else if needsTable(entryType(entry))}
              <label
                >Table
                <select
                  value={String(entry.table ?? "")}
                  onchange={(e) =>
                    setField(entry, "table", (e.currentTarget as HTMLSelectElement).value)}
                >
                  <option value="">Select…</option>
                  {#each tableIds as tid (tid)}
                    <option value={tid}>{tid}</option>
                  {/each}
                </select>
              </label>
            {:else if entryType(entry) === "loot"}
              <label
                >Loot crate<input
                  value={String(entry.loot_crate ?? entry.table ?? "")}
                  oninput={(e) =>
                    setField(entry, "loot_crate", (e.currentTarget as HTMLInputElement).value)}
                /></label
              >
            {:else if entryType(entry) === "all_tables" || entryType(entry) === "all_table"}
              <p class="muted">All tables — no extra fields.</p>
            {/if}
            <details class="extra">
              <summary>Extra keys</summary>
              <textarea
                rows="3"
                value={extrasText(entry, i)}
                oninput={(e) => {
                  extrasDraft = {
                    ...extrasDraft,
                    [i]: (e.currentTarget as HTMLTextAreaElement).value,
                  };
                }}
                onchange={() => applyExtras(entry, i)}
              ></textarea>
              <button type="button" class="mini" onclick={() => applyExtras(entry, i)}
                >Apply extras</button
              >
            </details>
          </div>
        {/each}
        <button
          type="button"
          class="save"
          disabled={saving || !dirty}
          onclick={() => onSave(selected!)}
        >
          <Save size={12} /> Save table
        </button>
      </div>
    {:else}
      <p class="muted">No reward tables. Create one or add SNBT under reward_tables/.</p>
    {/if}
  </div>
</details>

<style>
  .rt {
    margin-top: 10px;
    border: 1px solid var(--ftbq-border);
    border-radius: 2px;
    background: var(--ftbq-bg-panel);
    padding: 0 10px 10px;
    color: var(--ftbq-text);
  }
  summary {
    cursor: pointer;
    padding: 10px 4px;
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--ftbq-text-muted);
  }
  .rt-body {
    display: grid;
    grid-template-columns: 160px 1fr;
    gap: 10px;
  }
  .rt-side {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .rt-side button {
    text-align: left;
    padding: 6px 8px;
    border-radius: 2px;
    border: 1px solid var(--ftbq-border);
    background: rgba(0, 0, 0, 0.25);
    color: var(--ftbq-text-muted);
    font-size: 12px;
    cursor: pointer;
  }
  .rt-side button.sel,
  .rt-side button:hover {
    background: rgba(61, 184, 168, 0.1);
    border-color: var(--ftbq-accent-teal);
    color: var(--ftbq-text);
  }
  .rt-side .add {
    color: var(--ftbq-accent-green);
  }
  .rt-side .add.primary {
    background: rgba(85, 201, 90, 0.12);
    border-color: var(--ftbq-accent-green);
    font-weight: 700;
  }
  .rt-empty {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px 4px;
  }
  .rt-empty p {
    margin: 0;
    font-size: 12px;
    color: var(--ftbq-text-muted);
  }
  .rt-edit {
    display: grid;
    gap: 8px;
  }
  .rt-edit label {
    display: grid;
    gap: 8px;
    font-size: 12px;
    text-transform: uppercase;
    color: var(--ftbq-text-muted);
  }
  .rt-edit input,
  .rt-edit select,
  .rt-edit textarea {
    font-size: 12px;
    background: var(--ftbq-bg);
    border: 1px solid var(--ftbq-border);
    color: inherit;
    border-radius: 2px;
    padding: 6px 8px;
    text-transform: none;
  }
  .entries-h {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 12px;
    color: var(--ftbq-text-muted);
  }
  .entry-card {
    display: grid;
    gap: 8px;
    padding: 8px;
    border: 1px solid var(--ftbq-border);
    border-radius: 2px;
    background: rgba(0, 0, 0, 0.2);
  }
  .entry-top {
    display: grid;
    grid-template-columns: 1fr 72px 28px;
    gap: 8px;
  }
  .mini,
  .ico,
  .save {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    border-radius: 2px;
    border: 1px solid var(--ftbq-border);
    background: transparent;
    color: var(--ftbq-text-muted);
    cursor: pointer;
  }
  .save {
    padding: 8px;
    color: var(--ftbq-accent-green);
    border-color: rgba(85, 201, 90, 0.35);
    background: rgba(85, 201, 90, 0.08);
    font-weight: 700;
    font-size: 12px;
  }
  .save:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .muted {
    font-size: 12px;
    color: var(--ftbq-text-muted);
  }
  .extra {
    font-size: 12px;
    color: var(--ftbq-text-muted);
  }
  .extra textarea {
    width: 100%;
    font-family: var(--font-mono, ui-monospace, monospace);
    margin: 6px 0;
  }
  @media (max-width: 900px) {
    .rt-body {
      grid-template-columns: 1fr;
    }
  }
</style>
