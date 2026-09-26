<script lang="ts">
  import { Sparkles, Check, X, ChevronDown } from "@lucide/svelte";
  import type { QuestPlan, QuestPlanMergeResult } from "../../lib/api";

  let {
    merge,
    needsReviewAck = false,
    onapply,
    ondiscard,
  }: {
    merge: QuestPlanMergeResult;
    needsReviewAck?: boolean;
    onapply?: (detail: { chapterKeys: string[]; questKeys: string[] }) => void;
    ondiscard?: () => void;
  } = $props();

  let chapterOn = $state<Record<string, boolean>>({});
  let questOn = $state<Record<string, boolean>>({});
  let reviewAck = $state(false);
  let expanded = $state<Record<string, boolean>>({});

  let plan = $derived(merge?.plan as QuestPlan | undefined);

  let hasBookIssues = $derived(
    (merge.validation?.bookErrors ?? []).some((e) =>
      /cycle|Duplicate|missing/i.test(e.message),
    ),
  );

  let requiresAck = $derived(
    needsReviewAck || !!plan?.needsUserReview || hasBookIssues,
  );

  $effect(() => {
    initSelection(plan);
  });

  function chKey(ch: { id?: string | null; title: string }, i: number) {
    return ch.id || ch.title || `ch-${i}`;
  }
  function qKey(q: { id?: string | null; title: string }, i: number) {
    return q.id || q.title || `q-${i}`;
  }

  function chapterHasIssue(ch: {
    quests?: { id?: string | null; title: string }[];
  }): boolean {
    const ids = new Set(
      (ch.quests ?? []).flatMap((q) => [q.id, q.title].filter(Boolean) as string[]),
    );
    return (merge.validation?.bookErrors ?? []).some((e) => ids.has(e.questId));
  }

  function initSelection(p: QuestPlan | undefined) {
    if (!p) return;
    const nextCh: Record<string, boolean> = {};
    const nextQ: Record<string, boolean> = {};
    const nextEx: Record<string, boolean> = {};
    p.chapters.forEach((ch, ci) => {
      const ck = chKey(ch, ci);
      nextCh[ck] = true;
      nextEx[ck] = ci === 0 || chapterHasIssue(ch);
      ch.quests.forEach((q, qi) => {
        nextQ[qKey(q, qi)] = true;
      });
    });
    chapterOn = nextCh;
    questOn = nextQ;
    expanded = nextEx;
  }

  function depLabel(depId: string): string {
    if (!plan) return depId;
    for (const ch of plan.chapters) {
      for (const q of ch.quests ?? []) {
        if (q.id === depId || q.title === depId) return q.title;
      }
    }
    return depId;
  }

  function chapterQuestKeys(
    ch: { quests?: { id?: string | null; title: string }[] },
  ): string[] {
    return (ch.quests ?? []).map((q, qi) => qKey(q, qi));
  }

  function toggleChapter(ck: string, questKeys: string[], on: boolean) {
    chapterOn = { ...chapterOn, [ck]: on };
    const nextQ = { ...questOn };
    for (const qk of questKeys) nextQ[qk] = on;
    questOn = nextQ;
  }

  function toggleQuest(ck: string, qk: string, questKeys: string[], on: boolean) {
    const nextQ = { ...questOn, [qk]: on };
    questOn = nextQ;
    const allOn = questKeys.length > 0 && questKeys.every((k) => nextQ[k]);
    chapterOn = { ...chapterOn, [ck]: allOn };
  }

  /** Include chapter key whenever any of its quests are selected (even if chapter checkbox off). */
  function selectedChapterKeys(): string[] {
    if (!plan) return [];
    const keys = new Set<string>();
    for (const [k, on] of Object.entries(chapterOn)) {
      if (on) keys.add(k);
    }
    plan.chapters.forEach((ch, ci) => {
      const ck = chKey(ch, ci);
      const qKeys = chapterQuestKeys(ch);
      if (qKeys.some((qk) => questOn[qk])) keys.add(ck);
    });
    return [...keys];
  }

  function selectedQuestKeys(): string[] {
    return Object.entries(questOn)
      .filter(([, on]) => on)
      .map(([k]) => k);
  }

  function modeLabel(mode?: string | null): string {
    return (mode || "upsert").toLowerCase();
  }

  let questCount = $derived(
    plan?.chapters.reduce((n, ch) => n + (ch.quests?.length ?? 0), 0) ?? 0,
  );
  let chapterCount = $derived(plan?.chapters.length ?? 0);

  let selectedCount = $derived(Object.values(questOn).filter(Boolean).length);

  let applyBlockedReason = $derived.by(() => {
    if (!merge.validation?.valid) {
      return merge.validation?.errors?.[0] || "Fix validation errors before apply";
    }
    if (requiresAck && !reviewAck) {
      return "Acknowledge the review checklist first";
    }
    if (selectedCount === 0) {
      return "Select at least one quest";
    }
    return "";
  });

  let applyDisabled = $derived(!!applyBlockedReason);

  function setAllQuests(on: boolean) {
    if (!plan) return;
    const nextCh: Record<string, boolean> = {};
    const nextQ: Record<string, boolean> = {};
    plan.chapters.forEach((ch, ci) => {
      const ck = chKey(ch, ci);
      nextCh[ck] = on;
      for (const qk of chapterQuestKeys(ch)) nextQ[qk] = on;
    });
    chapterOn = nextCh;
    questOn = nextQ;
  }

  function applyAll() {
    if (applyDisabled) return;
    onapply?.({
      chapterKeys: selectedChapterKeys(),
      questKeys: selectedQuestKeys(),
    });
  }
</script>

{#if plan}
  <div class="review">
    <div class="review-h">
      <Sparkles size={14} />
      <strong>Review plan</strong>
      <span class="meta"
        >{chapterCount} ch · {questCount} quests · {(plan.confidence * 100).toFixed(0)}% · {plan.source ??
          "ai"}</span
      >
      <div class="sel-btns">
        <button type="button" class="ghost sel-btn" onclick={() => setAllQuests(true)}>All</button>
        <button type="button" class="ghost sel-btn" onclick={() => setAllQuests(false)}>None</button>
      </div>
    </div>
    <p class="expl">{plan.humanExplanation}</p>
    {#if merge.notes?.length}
      <ul class="notes">{#each merge.notes.slice(0, 12) as n, i (`n-${i}`)}<li>{n}</li>{/each}</ul>
    {/if}
    {#if merge.validation?.warnings?.length}
      <ul class="warns"
        >{#each merge.validation.warnings.slice(0, 10) as w, i (`w-${i}`)}<li>{w}</li>{/each}</ul
      >
    {/if}
    {#if merge.validation?.errors?.length}
      <ul class="errs"
        >{#each merge.validation.errors as e, i (`e-${i}`)}<li>{e}</li>{/each}</ul
      >
    {/if}
    {#if merge.validation?.bookErrors?.length}
      <ul class="warns"
        >{#each merge.validation.bookErrors as be, i (`be-${i}`)}
          <li>{be.questId}: {be.message}</li>
        {/each}</ul
      >
    {/if}

    <div class="tree">
      {#each plan.chapters as ch, ci (chKey(ch, ci))}
        {@const ck = chKey(ch, ci)}
        {@const qKeys = chapterQuestKeys(ch)}
        {@const isOpen = expanded[ck] !== false}
        <div class="ch-block">
          <div class="ch-row">
            <button
              type="button"
              class="fold"
              class:open={isOpen}
              aria-expanded={isOpen}
              onclick={() => (expanded = { ...expanded, [ck]: !isOpen })}
              aria-label={isOpen ? "Collapse chapter" : "Expand chapter"}
            >
              <ChevronDown size={14} />
            </button>
            <label class="ch">
              <input
                type="checkbox"
                checked={chapterOn[ck] || qKeys.some((k) => questOn[k])}
                onchange={(e) => toggleChapter(ck, qKeys, e.currentTarget.checked)}
              />
              <span>{ch.title}</span>
              <span class="mode-badge">{modeLabel(ch.mode)}</span>
              <small>{ch.quests?.length ?? 0}</small>
            </label>
          </div>
          {#if isOpen}
            {#each ch.quests as q, qi (qKey(q, qi))}
              {@const qk = qKey(q, qi)}
              <label class="q">
                <input
                  type="checkbox"
                  checked={questOn[qk]}
                  onchange={(e) => toggleQuest(ck, qk, qKeys, e.currentTarget.checked)}
                />
                <span>{q.title}</span>
                {#if q.dependencies?.length}
                  <span class="dep-anch" title={`depends on ${q.dependencies.length}`}>
                    {#each q.dependencies as depId, di (`${qk}-dep-${di}`)}
                      {di > 0 ? ", " : ""}↳ {depLabel(depId)}
                    {/each}
                  </span>
                {/if}
                <small>
                  {#if (q.description?.filter((d) => d.trim()).length ?? 0) > 0}<span
                      class="dot lore"
                      title="has lore"
                    ></span>{/if}
                  {#if (q.tasks?.length ?? 0) > 0}<span
                      class="dot tasks"
                      title="{q.tasks.length} tasks"
                    ></span>{/if}
                  {#if (q.dependencies?.length ?? 0) > 0}<span
                      class="dot deps"
                      title="{q.dependencies?.length} deps"
                    ></span>{/if}
                  {(q.description?.filter((d) => d.trim()).length ?? 0)}L · {q.tasks?.length ?? 0}t ·
                  {q.rewards?.length ?? 0}r
                </small>
              </label>
            {/each}
          {/if}
        </div>
      {/each}
    </div>

    {#if requiresAck}
      <label class="ack">
        <input type="checkbox" bind:checked={reviewAck} />
        {#if hasBookIssues}
          I reviewed book validation issues (cycles / duplicates / missing deps)
        {:else}
          I reviewed uncertain items / dependencies
        {/if}
      </label>
    {/if}

    <div class="actions">
      <button
        type="button"
        disabled={applyDisabled}
        title={applyBlockedReason || "Apply selected quests to the editor"}
        onclick={applyAll}
      >
        <Check size={14} /> Apply selected
      </button>
      <button type="button" class="ghost" onclick={() => ondiscard?.()}>
        <X size={14} /> Discard
      </button>
    </div>
    <p class="apply-hint save-note" role="note">
      Apply updates the editor only. Use Save (Ctrl+S) to write SNBT.
    </p>
    {#if applyBlockedReason}
      <p class="apply-hint" role="status">{applyBlockedReason}</p>
    {/if}
  </div>
{/if}

<style>
  .review {
    border: 1px solid var(--ftbq-frame);
    border-radius: 3px;
    padding: 10px;
    background: var(--ftbq-bg-panel);
    box-shadow:
      inset 0 0 0 1px rgba(255, 255, 255, 0.05),
      0 2px 8px rgba(0, 0, 0, 0.4);
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 42vh;
    overflow: auto;
    margin: 8px;
  }
  .review-h {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .review-h strong {
    color: var(--ftbq-title-gold);
    text-shadow: 1px 1px 0 rgba(0, 0, 0, 0.6);
  }
  .meta,
  .expl {
    color: var(--ftbq-text-muted);
    font-size: 12px;
    margin: 0;
  }
  .sel-btns {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }
  .sel-btn {
    font-size: 12px;
    font-weight: 600;
    padding: 2px 8px;
    color: var(--ftbq-text-muted);
  }
  .tree {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .ch-block {
    border: 1px solid rgba(255, 255, 255, 0.04);
    border-radius: 3px;
    background: rgba(0, 0, 0, 0.15);
    padding: 4px 0;
  }
  .ch-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 4px;
  }
  .fold {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border: none;
    background: transparent;
    color: var(--ftbq-text-muted);
    cursor: pointer;
    padding: 0;
    transform: rotate(-90deg);
    transition: transform 0.12s ease;
  }
  .fold.open {
    transform: rotate(0deg);
  }
  .ch,
  .q,
  .ack {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    padding: 4px 6px;
  }
  .ch {
    flex: 1;
    font-weight: 700;
    min-width: 0;
  }
  .q {
    padding-left: 36px;
    color: var(--ftbq-text-muted);
  }
  .mode-badge {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 1px 5px;
    border-radius: 2px;
    background: rgba(61, 184, 168, 0.15);
    color: var(--ftbq-accent-teal);
    flex-shrink: 0;
  }
  .ch small,
  .q small {
    margin-left: auto;
    font-size: 12px;
    opacity: 0.7;
  }
  .dep-anch {
    font-size: 12px;
    color: var(--ftbq-line, #5c8a9e);
    max-width: 40%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    margin-right: 2px;
  }
  .dot.lore {
    background: #9b7bff;
  }
  .dot.tasks {
    background: var(--ftbq-accent-teal);
  }
  .dot.deps {
    background: var(--ftbq-line, #5c8a9e);
  }
  .notes,
  .warns,
  .errs {
    margin: 0;
    padding-left: 18px;
    font-size: 12px;
  }
  .warns {
    color: var(--ftbq-quest-started);
  }
  .errs {
    color: var(--accent-danger);
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  .actions button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .actions .ghost {
    background: transparent;
    border-color: transparent;
    color: var(--ftbq-text-muted);
  }
  .apply-hint {
    margin: 0;
    font-size: 12px;
    color: var(--accent-warning);
  }
</style>
