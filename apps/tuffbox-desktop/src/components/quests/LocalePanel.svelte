<script lang="ts">
  import type { QuestChapter, QuestChapterGroup } from "../../lib/api";
  import {
    expectedLocaleKeys,
    isValidLocaleCode,
    localeGap,
    type LocaleGapEntry,
    type LocaleMap,
  } from "../../lib/questLocale";

  let {
    locales,
    activeLocale = null,
    compareLocale = null,
    chapterGroups,
    chapters,
    onCreateLocale,
    onJumpGap,
    onCompareLocaleChange,
    onFillGapsFromBase = undefined,
  }: {
    locales: Record<string, LocaleMap>;
    activeLocale?: string | null;
    compareLocale?: string | null;
    chapterGroups: QuestChapterGroup[];
    chapters: QuestChapter[];
    onCreateLocale: (code: string, fromCode: string | null) => Promise<void> | void;
    onJumpGap: (entry: LocaleGapEntry, targetCode: string) => void;
    onCompareLocaleChange: (code: string | null) => void;
    onFillGapsFromBase?: (targetCode: string, baseCode: string, keys: string[]) => void;
  } = $props();

  let codes = $derived(Object.keys(locales).sort((a, b) => a.localeCompare(b)));

  let baseCode = $state("");
  let targetCode = $state("");
  let newCode = $state("");
  let copyFrom = $state("");
  let createError = $state<string | null>(null);
  let creating = $state(false);

  $effect(() => {
    const list = codes;
    if (!baseCode || !list.includes(baseCode)) {
      baseCode = list.includes("en_us") ? "en_us" : (list[0] ?? "");
    }
    if (!targetCode || !list.includes(targetCode)) {
      targetCode =
        (activeLocale && list.includes(activeLocale) ? activeLocale : null) ??
        list.find((c) => c !== baseCode) ??
        list[0] ??
        "";
    }
    if (copyFrom && !list.includes(copyFrom)) copyFrom = "";
    if (!copyFrom && list.length) copyFrom = list.includes("en_us") ? "en_us" : list[0]!;
  });

  let gaps = $derived.by<LocaleGapEntry[]>(() => {
    if (!baseCode || !targetCode || baseCode === targetCode) return [];
    const base = locales[baseCode] ?? {};
    const target = locales[targetCode] ?? {};
    const keys = expectedLocaleKeys({ chapterGroups, chapters });
    return localeGap(base, target, keys);
  });

  async function submitCreate() {
    createError = null;
    const code = newCode.trim().toLowerCase();
    if (!isValidLocaleCode(code)) {
      createError = "Use a code like en_us or ru_ru";
      return;
    }
    if (locales[code]) {
      createError = `Locale “${code}” already exists`;
      return;
    }
    creating = true;
    try {
      await onCreateLocale(code, copyFrom || null);
      newCode = "";
      targetCode = code;
    } catch (e) {
      createError = String(e);
    } finally {
      creating = false;
    }
  }

  function preview(text: string): string {
    const t = text.replace(/\s+/g, " ").trim();
    return t.length > 72 ? `${t.slice(0, 72)}…` : t;
  }
</script>

<div class="locale-panel">
  <section class="block">
    <h4>Create locale</h4>
    <div class="row">
      <label class="field">
        Code
        <input
          type="text"
          placeholder="ru_ru"
          bind:value={newCode}
          onkeydown={(e) => e.key === "Enter" && void submitCreate()}
        />
      </label>
      <label class="field">
        Copy from
        <select bind:value={copyFrom}>
          <option value="">(harvest current display)</option>
          {#each codes as c (c)}
            <option value={c}>{c}</option>
          {/each}
        </select>
      </label>
      <button type="button" class="btn primary" disabled={creating} onclick={() => void submitCreate()}>
        {creating ? "Creating…" : "Create"}
      </button>
    </div>
    {#if createError}
      <p class="err">{createError}</p>
    {/if}
    <p class="hint">Writes <code>lang/&lt;code&gt;.snbt</code> immediately. Switcher updates after create.</p>
  </section>

  <section class="block">
    <h4>Compare / gap report</h4>
    <div class="row">
      <label class="field">
        Base
        <select bind:value={baseCode} disabled={codes.length === 0}>
          {#each codes as c (c)}
            <option value={c}>{c}</option>
          {/each}
        </select>
      </label>
      <label class="field">
        Target
        <select bind:value={targetCode} disabled={codes.length === 0}>
          {#each codes as c (c)}
            <option value={c}>{c}</option>
          {/each}
        </select>
      </label>
      <label class="field">
        Inspector compare
        <select
          value={compareLocale ?? ""}
          onchange={(e) => {
            const v = (e.currentTarget as HTMLSelectElement).value;
            onCompareLocaleChange(v || null);
          }}
        >
          <option value="">(off)</option>
          {#each codes as c (c)}
            {#if c !== activeLocale}
              <option value={c}>{c}</option>
            {/if}
          {/each}
        </select>
      </label>
    </div>
    {#if codes.length < 2}
      <p class="hint">Need at least two locales for a gap report.</p>
    {:else if baseCode === targetCode}
      <p class="hint">Pick different base and target.</p>
    {:else}
      <p class="gap-count">
        {gaps.length} gap{gaps.length === 1 ? "" : "s"}
        ({gaps.filter((g) => g.kind === "missing").length} missing ·
        {gaps.filter((g) => g.kind === "empty").length} empty)
      </p>
      {#if gaps.length > 0 && onFillGapsFromBase}
        <button
          type="button"
          class="btn primary"
          onclick={() =>
            onFillGapsFromBase(
              targetCode,
              baseCode,
              gaps.map((g) => g.key),
            )}
        >
          Fill missing keys from base
        </button>
      {/if}
      <div class="gap-list">
        {#each gaps.slice(0, 200) as g (g.key)}
          <button type="button" class="gap-row" onclick={() => onJumpGap(g, targetCode)}>
            <span class="kind {g.kind}">{g.kind}</span>
            <code class="key">{g.key}</code>
            <span class="prev">{preview(g.basePreview)}</span>
          </button>
        {/each}
        {#if gaps.length > 200}
          <p class="hint">Showing first 200 of {gaps.length}.</p>
        {/if}
        {#if gaps.length === 0}
          <p class="hint">No gaps — target covers all non-empty base keys in the book.</p>
        {/if}
      </div>
    {/if}
  </section>
</div>

<style>
  .locale-panel {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 4px 2px 12px;
    color: var(--ftbq-text);
    font-size: 12px;
  }
  .block h4 {
    margin: 0 0 8px;
    font-size: 12px;
    font-weight: 700;
    color: var(--ftbq-title-gold);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: flex-end;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 12px;
    color: var(--ftbq-text-muted);
  }
  .field input,
  .field select {
    min-width: 100px;
    padding: 5px 7px;
    font-size: 12px;
    background: var(--ftbq-input-bg);
    border: 1px solid var(--ftbq-frame);
    color: var(--ftbq-text);
    border-radius: 3px;
  }
  .btn {
    padding: 6px 12px;
    border: 1px solid var(--ftbq-border);
    background: rgba(0, 0, 0, 0.25);
    color: var(--ftbq-text);
    font-size: 12px;
    font-weight: 600;
    border-radius: 3px;
    cursor: pointer;
  }
  .btn.primary {
    border-color: var(--ftbq-accent-green);
    background: rgba(85, 201, 90, 0.18);
    color: var(--ftbq-accent-green);
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .hint {
    margin: 6px 0 0;
    font-size: 12px;
    color: var(--ftbq-text-muted);
  }
  .err {
    margin: 6px 0 0;
    font-size: 12px;
    color: var(--accent-danger);
  }
  .gap-count {
    margin: 8px 0 6px;
    font-size: 12px;
    color: var(--ftbq-text-muted);
  }
  .gap-list {
    max-height: 280px;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    border: 1px solid var(--ftbq-border);
    border-radius: var(--border-radius-sm);
    padding: 6px;
    background: rgba(0, 0, 0, 0.2);
  }
  .gap-row {
    display: grid;
    grid-template-columns: 58px minmax(0, 1.2fr) minmax(0, 1fr);
    gap: 8px;
    align-items: center;
    text-align: left;
    padding: 5px 6px;
    border: 1px solid transparent;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
    border-radius: 3px;
  }
  .gap-row:hover {
    border-color: rgba(61, 184, 168, 0.35);
    background: rgba(61, 184, 168, 0.08);
  }
  .kind {
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .kind.missing {
    color: var(--accent-danger);
  }
  .kind.empty {
    color: var(--accent-warning);
  }
  .key {
    font-size: 12px;
    color: var(--ftbq-accent-teal);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .prev {
    font-size: 12px;
    color: var(--ftbq-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
