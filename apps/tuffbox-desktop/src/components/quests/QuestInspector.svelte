<script lang="ts">
  import {
    Trash2,
    Link2,
    AlertTriangle,
    Copy,
    Check,
    ChevronDown,
    ChevronRight,
    Sparkles,
    FileText,
    CheckSquare,
    Trophy,
    Settings,
    HelpCircle,
  } from "@lucide/svelte";
  import QuestItemIcon from "./QuestItemIcon.svelte";
  import type { QuestChapter, QuestData, QuestValidationIssue } from "../../lib/api";
  import { DEP_REQUIREMENT_OPTIONS, SHAPE_OPTIONS } from "../../lib/questTypeLabels";
  import {
    localeValueAsString,
    type LocaleMap,
  } from "../../lib/questLocale";
  import TaskRewardEditor from "./TaskRewardEditor.svelte";
  import ItemStackEditor from "./ItemStackEditor.svelte";

  let {
    quest,
    chapterQuests,
    chapters = [],
    issues = [],
    rewardTableIds = [],
    activeLocale = null,
    compareLocale = null,
    compareMap = null,
    availableLocales = [],
    onDirty,
    onCompareDirty,
    onCompareLocaleChange,
    onCompareMapChange,
    onRemove,
    onAddDep,
    onRemoveDep,
    onOpenKubeJs,
    focusFieldToken = 0,
    focusField = null as string | null,
  }: {
    quest: QuestData;
    chapterQuests: QuestData[];
    chapters?: QuestChapter[];
    issues?: QuestValidationIssue[];
    rewardTableIds?: string[];
    activeLocale?: string | null;
    compareLocale?: string | null;
    compareMap?: LocaleMap | null;
    availableLocales?: string[];
    onDirty: () => void;
    onCompareDirty?: (code: string) => void;
    onCompareLocaleChange?: (code: string | null) => void;
    onCompareMapChange?: (code: string, map: LocaleMap) => void;
    onRemove: () => void;
    onAddDep: (depId: string) => void;
    onRemoveDep: (depId: string) => void;
    onOpenKubeJs?: (id: string) => void;
    /** Increment to focus `focusField` (title | tasks | icon). */
    focusFieldToken?: number;
    focusField?: string | null;
  } = $props();

  let inspTab = $state<"general" | "tasks" | "rewards" | "deps" | "more">("general");
  let depPick = $state("");
  let depFilter = $state("");
  let descText = $state("");
  let descEl = $state<HTMLTextAreaElement | null>(null);
  let extraKey = $state("");
  let idCopied = $state(false);
  let idCopyTimer: ReturnType<typeof setTimeout> | null = null;
  let titleInputEl = $state<HTMLInputElement | null>(null);
  let lastFocusToken = $state(0);

  async function copyQuestId() {
    try {
      await navigator.clipboard.writeText(quest.id);
      idCopied = true;
      if (idCopyTimer) clearTimeout(idCopyTimer);
      idCopyTimer = setTimeout(() => {
        idCopied = false;
        idCopyTimer = null;
      }, 1500);
    } catch {
      /* clipboard may be unavailable */
    }
  }
  let extraVal = $state("");
  let cmpTitle = $state("");
  let cmpSubtitle = $state("");
  let cmpDesc = $state("");

  $effect(() => {
    if (!focusFieldToken || focusFieldToken === lastFocusToken) return;
    lastFocusToken = focusFieldToken;
    const field = focusField;
    if (field === "title" || field === "icon") {
      inspTab = "general";
      queueMicrotask(() => {
        titleInputEl?.focus();
        titleInputEl?.select();
      });
    } else if (field === "tasks" || field === "item") {
      inspTab = "tasks";
    } else if (field === "rewards") {
      inspTab = "rewards";
    } else if (field === "deps") {
      inspTab = "deps";
    } else if (field === "more") {
      inspTab = "more";
    }
  });

  let depOptions = $derived(buildDepOptions(chapters, chapterQuests, quest));
  let filteredDepOptions = $derived.by(() => {
    const q = depFilter.trim().toLowerCase();
    if (!q) return depOptions;
    return depOptions.filter(
      (o) =>
        o.label.toLowerCase().includes(q) ||
        o.id.toLowerCase().includes(q),
    );
  });
  let myIssues = $derived(issues.filter((i) => i.questId === quest.id));
  let showCompare = $derived(
    !!compareLocale &&
      !!compareMap &&
      compareLocale !== activeLocale &&
      availableLocales.length > 1,
  );

  function normalizeDescLines(text: string): string[] {
    const lines = text
      .split("\n")
      .map((s) => s.trimEnd())
      .filter((s, i, arr) => s.length > 0 || i < arr.length - 1);
    while (lines.length && lines[lines.length - 1] === "") lines.pop();
    return lines;
  }

  function commitDescriptionTo(target: QuestData, text: string): boolean {
    const lines = normalizeDescLines(text);
    const prev = (target.description ?? []).join("\n");
    const next = lines.join("\n");
    if (prev === next) return false;
    target.description = lines;
    target.descriptionFromSnbt = true;
    onDirty();
    return true;
  }

  function commitDescription() {
    commitDescriptionTo(quest, descText);
  }

  /** Flush pending description when leaving a quest (deselect / switch) before resync. */
  $effect(() => {
    const target = quest;
    void target.id;
    descText = (target.description ?? []).join("\n");
    return () => {
      commitDescriptionTo(target, descText);
    };
  });

  $effect(() => {
    const id = quest.id;
    const map = compareMap;
    const code = compareLocale;
    if (!map || !code) {
      cmpTitle = "";
      cmpSubtitle = "";
      cmpDesc = "";
      return;
    }
    cmpTitle = localeValueAsString(map, `quest.${id}.title`);
    cmpSubtitle = localeValueAsString(map, `quest.${id}.quest_subtitle`);
    cmpDesc = localeValueAsString(map, `quest.${id}.quest_desc`);
    return () => {
      if (!code || !map) return;
      const lines = normalizeDescLines(cmpDesc);
      const key = `quest.${id}.quest_desc`;
      const prev = localeValueAsString(map, key);
      if (prev === lines.join("\n")) return;
      const next: LocaleMap = { ...map };
      for (const [k, v] of Object.entries(next)) {
        if (Array.isArray(v)) next[k] = [...v];
      }
      next[key] = lines;
      onCompareMapChange?.(code, next);
      onCompareDirty?.(code);
    };
  });

  function patchCompare(mutator: (map: LocaleMap) => void) {
    if (!compareLocale || !compareMap) return;
    const next: LocaleMap = { ...compareMap };
    for (const [k, v] of Object.entries(next)) {
      if (Array.isArray(v)) next[k] = [...v];
    }
    mutator(next);
    onCompareMapChange?.(compareLocale, next);
    onCompareDirty?.(compareLocale);
  }

  function commitCompareDesc() {
    const lines = normalizeDescLines(cmpDesc);
    patchCompare((map) => {
      map[`quest.${quest.id}.quest_desc`] = lines;
    });
  }

  function buildDepOptions(
    allChapters: QuestChapter[],
    sameChapter: QuestData[],
    current: QuestData,
  ) {
    const opts: { id: string; label: string }[] = [];
    const walk: { title: string; same: boolean; quests: QuestData[] }[] =
      allChapters.length > 0
        ? (() => {
            const currentChId = allChapters.find((ch) =>
              ch.quests.some((q) => q.id === current.id),
            )?.id;
            return allChapters.map((ch) => ({
              title: ch.title || ch.filename || ch.id.slice(0, 8),
              same: ch.id === currentChId,
              quests: ch.quests,
            }));
          })()
        : [{ title: "", same: true, quests: sameChapter }];

    for (const ch of walk) {
      for (const q of ch.quests) {
        if (q.id === current.id) continue;
        const base = ch.same ? q.title : `${ch.title} · ${q.title}`;
        if (!current.dependencies.includes(q.id)) {
          opts.push({ id: q.id, label: base });
        }
        for (const t of q.tasks ?? []) {
          if (!t.id || current.dependencies.includes(t.id)) continue;
          opts.push({
            id: t.id,
            label: `${base} · ${t.title || t.type || "task"}`,
          });
        }
      }
    }
    return opts;
  }

  function titleOf(id: string) {
    const direct = chapterQuests.find((q) => q.id === id);
    if (direct) return direct.title;
    const viaTask = chapterQuests.find((q) => q.tasks?.some((t) => t.id === id));
    if (viaTask) {
      const task = viaTask.tasks.find((t) => t.id === id);
      return `${viaTask.title}${task?.title ? ` · ${task.title}` : " (task)"}`;
    }
    for (const ch of chapters) {
      const q = ch.quests.find((x) => x.id === id);
      if (q) return `${ch.title || ch.filename || ch.id.slice(0, 8)} · ${q.title}`;
      const owner = ch.quests.find((x) => x.tasks?.some((t) => t.id === id));
      if (owner) {
        const task = owner.tasks.find((t) => t.id === id);
        return `${ch.title || ch.filename || ch.id.slice(0, 8)} · ${owner.title}${task?.title ? ` · ${task.title}` : " (task)"}`;
      }
    }
    return id;
  }

  function applyDep() {
    if (!depPick) return;
    onAddDep(depPick);
    depPick = "";
    depFilter = "";
  }

  function applyDepFromFilter() {
    if (depPick) {
      applyDep();
      return;
    }
    const first = filteredDepOptions[0];
    if (!first) return;
    onAddDep(first.id);
    depPick = "";
    depFilter = "";
  }

  function wrapFmt(code: string) {
    const el = descEl;
    const text = descText;
    const selStart = el ? el.selectionStart : text.length;
    const selEnd = el ? el.selectionEnd : text.length;
    const start = selStart ?? text.length;
    const end = selEnd ?? text.length;
    const selected = text.slice(start, end);
    const head = text.slice(0, start);
    const tail = text.slice(end);
    let applied: string;
    let cursorAt: number;
    if (selected && end > start) {
      applied = `${head}${code}${selected}&r${tail}`;
      cursorAt = start + code.length + selected.length;
    } else {
      applied = `${head}${code}${tail}`;
      cursorAt = start + code.length;
    }
    descText = applied;
    commitDescription();
    queueMicrotask(() => {
      if (!descEl) return;
      descEl.focus();
      descEl.setSelectionRange(cursorAt, cursorAt);
    });
  }

  function insertTemplate(kind: string) {
    const lines =
      kind === "objective"
        ? ["&7Objective:", "&fComplete the listed tasks.", "&8Rewards unlock the next step."]
        : kind === "story"
          ? ["&l&6Chapter beat", "&7A story beat for this pack.", "&aFinish to continue the line."]
          : ["&eHint: &7check JEI for recipes."];
    descText = [...(descText ? descText.split("\n") : []), ...lines].join("\n");
    commitDescription();
  }

  function tri(field: keyof QuestData, e: Event) {
    const s = (e.target as HTMLSelectElement).value;
    (quest as unknown as Record<string, unknown>)[field as string] =
      s === "true" ? true : s === "false" ? false : null;
    onDirty();
  }

  function triVal(v: boolean | null | undefined): string {
    return v === true ? "true" : v === false ? "false" : "";
  }

  function ensureExtras() {
    if (!quest.extras) quest.extras = {};
    return quest.extras;
  }

  function addExtra() {
    const k = extraKey.trim();
    if (!k) return;
    let parsed: unknown = extraVal;
    try {
      parsed = JSON.parse(extraVal);
    } catch {
      /* string */
    }
    ensureExtras()[k] = parsed as never;
    quest.extras = { ...quest.extras };
    extraKey = "";
    extraVal = "";
    onDirty();
  }

  function removeExtra(k: string) {
    if (!quest.extras) return;
    delete quest.extras[k];
    quest.extras = { ...quest.extras };
    onDirty();
  }

  function selectVal(e: Event): string {
    return (e.currentTarget as HTMLSelectElement).value;
  }
  function inputVal(e: Event): string {
    return (e.currentTarget as HTMLInputElement).value;
  }

  function autoGrowDescription(e: Event) {
    const el = e.currentTarget as HTMLTextAreaElement;
    descText = el.value;
    el.style.height = "auto";
    el.style.height = `${Math.max(90, el.scrollHeight)}px`;
  }
</script>

<aside class="insp ftbq-view flex flex-col min-w-0 min-h-0 h-full overflow-hidden bg-[var(--bg-secondary)] border-l border-[var(--border-color)]">
  <!-- Inspector Top Header -->
  <div class="insp-header flex items-center justify-between gap-2.5 px-4 py-2.5 border-b border-[var(--border-color)] bg-[var(--bg-secondary)] flex-shrink-0">
    <div class="flex items-center gap-2.5 min-w-0 flex-1">
      <div class="flex-shrink-0">
        <QuestItemIcon itemId={typeof quest.icon === "string" ? quest.icon : null} fallback={quest.title?.charAt(0) ?? "?"} size={28} />
      </div>
      <div class="min-w-0 flex-1">
        <h3 class="truncate text-sm font-bold text-[var(--text-primary)]" title={quest.id}>
          {quest.title || "Untitled quest"}
        </h3>
        <div class="flex items-center gap-2">
          <button
            type="button"
            class="qid-mini"
            title={idCopied ? "Copied full ID!" : `Copy quest ID: ${quest.id}`}
            onclick={() => void copyQuestId()}
          >
            {#if idCopied}
              <Check size={11} class="text-emerald-400" />
              <span class="text-emerald-400 font-bold">Copied</span>
            {:else}
              <Copy size={11} />
              <code>{quest.id.slice(0, 8)}</code>
            {/if}
          </button>
          {#if quest.optional}
            <span class="text-[12px] font-semibold text-amber-400 bg-amber-400/10 px-1.5 py-0.5 rounded">Optional</span>
          {/if}
        </div>
      </div>
    </div>

    <button
      type="button"
      class="ico danger"
      title="Delete quest from chapter"
      aria-label="Delete quest"
      onclick={onRemove}
    >
      <Trash2 size={15} />
    </button>
  </div>

  <!-- Segmented Section Tabs -->
  <div class="insp-tabs flex border-b border-[var(--border-color)] bg-[var(--bg-primary)] p-1 gap-2 flex-shrink-0" role="tablist">
    <button
      type="button"
      role="tab"
      class="insp-tab flex-1 py-1.5 px-1.5 text-xs font-semibold rounded-md transition text-center {inspTab === 'general' ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]'}"
      aria-selected={inspTab === "general"}
      onclick={() => (inspTab = "general")}
    >
      General
    </button>
    <button
      type="button"
      role="tab"
      class="insp-tab flex-1 py-1.5 px-1.5 text-xs font-semibold rounded-md transition text-center flex items-center justify-center gap-2 {inspTab === 'tasks' ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]'}"
      aria-selected={inspTab === "tasks"}
      onclick={() => (inspTab = "tasks")}
    >
      Tasks
      <span class="tab-badge {inspTab === 'tasks' ? 'bg-white/20 text-white' : 'bg-[var(--bg-card)] text-[var(--text-muted)]'}">
        {quest.tasks?.length ?? 0}
      </span>
    </button>
    <button
      type="button"
      role="tab"
      class="insp-tab flex-1 py-1.5 px-1.5 text-xs font-semibold rounded-md transition text-center flex items-center justify-center gap-2 {inspTab === 'rewards' ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]'}"
      aria-selected={inspTab === "rewards"}
      onclick={() => (inspTab = "rewards")}
    >
      Rewards
      <span class="tab-badge {inspTab === 'rewards' ? 'bg-white/20 text-white' : 'bg-[var(--bg-card)] text-[var(--text-muted)]'}">
        {quest.rewards?.length ?? 0}
      </span>
    </button>
    <button
      type="button"
      role="tab"
      class="insp-tab flex-1 py-1.5 px-1.5 text-xs font-semibold rounded-md transition text-center flex items-center justify-center gap-2 {inspTab === 'deps' ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]'}"
      aria-selected={inspTab === "deps"}
      onclick={() => (inspTab = "deps")}
    >
      Unlocks
      {#if quest.dependencies?.length}
        <span class="tab-badge {inspTab === 'deps' ? 'bg-white/20 text-white' : 'bg-[var(--bg-card)] text-[var(--text-muted)]'}">
          {quest.dependencies.length}
        </span>
      {/if}
    </button>
    <button
      type="button"
      role="tab"
      class="insp-tab flex-1 py-1.5 px-1.5 text-xs font-semibold rounded-md transition text-center {inspTab === 'more' ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]'}"
      aria-selected={inspTab === "more"}
      onclick={() => (inspTab = "more")}
    >
      More
    </button>
  </div>

  {#if myIssues.length > 0}
    <div class="val-warn px-4 py-2 bg-amber-500/10 border-b border-amber-500/20 text-[var(--accent-warning)] text-xs flex flex-col gap-2">
      {#each myIssues as issue (issue.message)}
        <div class="flex items-center gap-2"><AlertTriangle size={13} class="flex-shrink-0" /> {issue.message}</div>
      {/each}
    </div>
  {/if}

  <div class="insp-scroll-body flex-1 min-h-0 overflow-y-auto overflow-x-hidden p-3.5 space-y-4">
    {#if inspTab === "general"}
      <!-- General Tab -->
      <div class="fields space-y-3">
        <label>
          <span class="field-label">Quest Title</span>
          <input
            bind:this={titleInputEl}
            bind:value={quest.title}
            placeholder="Enter quest title…"
            oninput={() => {
              quest.titleFromSnbt = true;
              onDirty();
            }}
          />
        </label>

        <label>
          <span class="field-label">Subtitle (optional)</span>
          <input
            bind:value={quest.subtitle}
            oninput={() => {
              quest.subtitleFromSnbt = true;
              onDirty();
            }}
            placeholder="One-line subtitle or teaser…"
          />
        </label>

        <!-- Icon management block -->
        <div class="icon-block p-3 rounded-lg border border-[var(--border-color)] bg-[var(--bg-card)]">
          <span class="field-label block mb-2 font-semibold">Quest Icon</span>
          <div class="flex items-center gap-3">
            <ItemStackEditor
              label="Icon"
              value={quest.icon ?? null}
              allowFilters={false}
              onChange={(v) => {
                quest.icon = v;
                onDirty();
              }}
            />
            <div class="flex-1 text-xs text-[var(--text-muted)] leading-relaxed">
              {#if quest.icon}
                <span class="text-emerald-400 font-medium">Custom icon set.</span>
                <button
                  type="button"
                  class="block mt-1 text-xs text-[var(--accent-primary)] hover:underline"
                  onclick={() => {
                    quest.icon = null;
                    onDirty();
                  }}
                >
                  Reset to auto icon
                </button>
              {:else}
                <span>Auto: inherits from first task item or title initial.</span>
              {/if}
            </div>
          </div>
        </div>

        <label>
          <div class="flex items-center justify-between">
            <span class="field-label">Description / Lore</span>
            <details class="fmt-details">
              <summary class="text-[12px] font-semibold text-[var(--accent-primary)] cursor-pointer">Formatting</summary>
              <div class="fmt-bar">
                <button type="button" onclick={() => wrapFmt("&l")}>Bold</button>
                <button type="button" onclick={() => wrapFmt("&a")}>Green</button>
                <button type="button" onclick={() => wrapFmt("&7")}>Gray</button>
                <button type="button" onclick={() => wrapFmt("&e")}>Gold</button>
                <button type="button" onclick={() => insertTemplate("objective")}>Objective</button>
                <button type="button" onclick={() => insertTemplate("story")}>Story</button>
              </div>
            </details>
          </div>
          <textarea
            rows="4"
            bind:this={descEl}
            value={descText}
            oninput={autoGrowDescription}
            onchange={commitDescription}
            onblur={commitDescription}
            placeholder="Explain what the player should do and why…"
          ></textarea>
        </label>

        <label class="checkbox flex items-center gap-2 cursor-pointer pt-1">
          <input type="checkbox" bind:checked={quest.optional} onchange={onDirty} />
          <span class="text-xs font-medium text-[var(--text-primary)]">Optional quest (not required for chapter completion)</span>
        </label>
      </div>

    {:else if inspTab === "tasks"}
      <!-- Tasks Tab -->
      <TaskRewardEditor {quest} {onDirty} {rewardTableIds} {onOpenKubeJs} section="tasks" />

    {:else if inspTab === "rewards"}
      <!-- Rewards Tab -->
      <TaskRewardEditor {quest} {onDirty} {rewardTableIds} {onOpenKubeJs} section="rewards" />

    {:else if inspTab === "deps"}
      <!-- Unlocks / Dependencies Tab -->
      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <span class="field-label font-bold flex items-center gap-2 text-sm">
            <Link2 size={14} class="text-[var(--accent-primary)]" />
            Unlocking Requirements
          </span>
          <span class="text-xs text-[var(--text-muted)]">{quest.dependencies.length} linked</span>
        </div>

        <div class="deps p-2.5 rounded-lg border border-[var(--border-color)] bg-[var(--bg-card)] min-h-[50px] flex flex-wrap gap-2" id="quest-deps">
          {#each quest.dependencies as dep (dep)}
            <span class="dep-tag inline-flex items-center gap-2 px-2.5 py-1 rounded bg-[var(--bg-secondary)] border border-[var(--border-color)] text-xs font-semibold text-[var(--text-primary)]" title={dep}>
              {titleOf(dep)}
              <button type="button" class="dep-rm text-[var(--text-muted)] hover:text-red-400 font-bold ml-1" onclick={() => onRemoveDep(dep)}>×</button>
            </span>
          {/each}
          {#if quest.dependencies.length === 0}
            <span class="deps-empty text-xs text-[var(--text-muted)] p-2">No dependencies — this quest is available immediately at start.</span>
          {/if}
        </div>

        <div class="dep-add space-y-2 p-3 rounded-lg border border-[var(--border-color)] bg-[var(--bg-primary)]">
          <span class="text-xs font-semibold text-[var(--text-secondary)] block">Add dependency:</span>
          <input
            type="search"
            class="dep-filter w-full text-xs"
            placeholder="Search quest by name or id…"
            bind:value={depFilter}
            onkeydown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                applyDepFromFilter();
              }
            }}
          />
          <div class="flex gap-2">
            <select bind:value={depPick} class="flex-1 text-xs">
              <option value="">Select quest to link…</option>
              {#each filteredDepOptions as o (o.id)}
                <option value={o.id}>{o.label}</option>
              {/each}
            </select>
            <button
              type="button"
              class="add-btn px-3 py-1.5 bg-[var(--accent-primary)] text-[var(--on-accent)] text-xs font-semibold rounded-md hover:brightness-110 disabled:opacity-50"
              disabled={!depPick && filteredDepOptions.length === 0}
              onclick={applyDepFromFilter}
            >
              Add
            </button>
          </div>
        </div>

        <div class="grid grid-cols-2 gap-3 pt-2">
          <label>
            <span class="field-label">Min required deps</span>
            <input
              type="number"
              min="0"
              placeholder="All"
              value={quest.minRequiredDependencies ?? ""}
              oninput={(e) => {
                const v = inputVal(e);
                quest.minRequiredDependencies = v === "" ? null : Number(v);
                onDirty();
              }}
            />
          </label>
          <label>
            <span class="field-label">Requirement mode</span>
            <select
              value={quest.dependencyRequirement ?? ""}
              onchange={(e) => {
                quest.dependencyRequirement = selectVal(e) || null;
                onDirty();
              }}
            >
              {#each DEP_REQUIREMENT_OPTIONS as d (d.id || "_default")}
                <option value={d.id}>{d.label}</option>
              {/each}
            </select>
          </label>
        </div>
      </div>

    {:else if inspTab === "more"}
      <!-- More / Advanced Tab -->
      <div class="fields space-y-3" id="quest-more">
        <div class="grid grid-cols-2 gap-3">
          <label>
            <span class="field-label">Node Shape</span>
            <select
              value={quest.shape ?? ""}
              onchange={(e) => {
                quest.shape = selectVal(e) || null;
                onDirty();
              }}
            >
              {#each SHAPE_OPTIONS as s (s.id || "_default")}
                <option value={s.id}>{s.label}</option>
              {/each}
            </select>
          </label>
          <label>
            <span class="field-label">Size multiplier</span>
            <input
              type="number"
              step="0.25"
              min="0.25"
              bind:value={quest.size}
              oninput={onDirty}
              placeholder="1.0"
            />
          </label>
        </div>

        <label>
          <span class="field-label">Canvas Coordinates (X, Y)</span>
          <div class="xy grid grid-cols-2 gap-2">
            <input type="number" step="0.5" bind:value={quest.x} oninput={onDirty} placeholder="X" />
            <input type="number" step="0.5" bind:value={quest.y} oninput={onDirty} placeholder="Y" />
          </div>
        </label>

        <h4 class="sub-h font-bold text-xs uppercase text-[var(--text-muted)] tracking-wider pt-2">FTB Behavior Flags</h4>
        <div class="grid grid-cols-2 gap-2 text-xs">
          <label>
            <span class="field-label">Hide dep lines</span>
            <select value={triVal(quest.hideDependencyLines)} onchange={(e) => tri("hideDependencyLines", e)}>
              <option value="">Default</option>
              <option value="true">Hide</option>
              <option value="false">Show</option>
            </select>
          </label>
          <label>
            <span class="field-label">Hide dependent</span>
            <select value={triVal(quest.hideDependentLines)} onchange={(e) => tri("hideDependentLines", e)}>
              <option value="">Default</option>
              <option value="true">Hide</option>
              <option value="false">Show</option>
            </select>
          </label>
          <label>
            <span class="field-label">Can repeat</span>
            <select value={triVal(quest.canRepeat)} onchange={(e) => tri("canRepeat", e)}>
              <option value="">Default (No)</option>
              <option value="true">Yes</option>
              <option value="false">No</option>
            </select>
          </label>
          <label>
            <span class="field-label">Visibility</span>
            <select value={triVal(quest.invisible)} onchange={(e) => tri("invisible", e)}>
              <option value="">Default</option>
              <option value="true">Hidden until unlocked</option>
              <option value="false">Always visible</option>
            </select>
          </label>
        </div>

        {#if availableLocales.length > 1}
          <div class="p-3 rounded-lg border border-[var(--border-color)] bg-[var(--bg-card)] space-y-2">
            <label class="compare-pick">
              <span class="field-label font-semibold">Compare / Translate Locale</span>
              <select
                value={compareLocale ?? ""}
                onchange={(e) => {
                  const v = (e.currentTarget as HTMLSelectElement).value;
                  onCompareLocaleChange?.(v || null);
                }}
              >
                <option value="">(off)</option>
                {#each availableLocales as c (c)}
                  {#if c !== activeLocale}
                    <option value={c}>{c}</option>
                  {/if}
                {/each}
              </select>
            </label>

            {#if showCompare}
              <div class="locale-cols grid grid-cols-2 gap-2 pt-2">
                <div class="locale-col space-y-1.5">
                  <span class="col-h text-[12px] font-bold text-[var(--accent-primary)]">{activeLocale ?? "active"}</span>
                  <input
                    class="text-xs"
                    bind:value={quest.title}
                    oninput={() => {
                      quest.titleFromSnbt = true;
                      onDirty();
                    }}
                  />
                  <textarea
                    rows="2"
                    class="text-xs"
                    value={descText}
                    oninput={autoGrowDescription}
                    onchange={commitDescription}
                    onblur={commitDescription}
                  ></textarea>
                </div>
                <div class="locale-col space-y-1.5">
                  <span class="col-h text-[12px] font-bold text-[var(--accent-secondary)]">{compareLocale}</span>
                  <input
                    class="text-xs"
                    bind:value={cmpTitle}
                    oninput={() =>
                      patchCompare((map) => {
                        map[`quest.${quest.id}.title`] = cmpTitle;
                      })}
                  />
                  <textarea
                    rows="2"
                    class="text-xs"
                    bind:value={cmpDesc}
                    onchange={commitCompareDesc}
                    onblur={commitCompareDesc}
                  ></textarea>
                </div>
              </div>
            {/if}
          </div>
        {/if}

        <h4 class="sub-h font-bold text-xs uppercase text-[var(--text-muted)] tracking-wider pt-2">Extra SNBT Properties</h4>
        {#each Object.entries(quest.extras ?? {}) as [k, v] (k)}
          <div class="extra-row flex items-center justify-between gap-2 p-1.5 bg-[var(--bg-card)] rounded border border-[var(--border-color)] text-xs">
            <code class="text-[var(--accent-primary)]">{k}</code>
            <span class="truncate flex-1 text-right text-[var(--text-secondary)]">{typeof v === "string" ? v : JSON.stringify(v)}</span>
            <button type="button" class="text-red-400 font-bold px-1" onclick={() => removeExtra(k)}>×</button>
          </div>
        {/each}
        <div class="extra-add flex gap-2">
          <input placeholder="key" class="text-xs flex-1" bind:value={extraKey} />
          <input placeholder="value / JSON" class="text-xs flex-1" bind:value={extraVal} />
          <button type="button" class="px-3 py-1 bg-[var(--bg-card)] border border-[var(--border-color)] text-xs font-semibold rounded hover:bg-[var(--bg-hover)]" onclick={addExtra}>Add</button>
        </div>
      </div>
    {/if}
  </div>
</aside>

<style>
  .insp {
    font-family: var(--font-sans, inherit);
    font-size: 13px;
    line-height: 1.5;
  }
  .insp-header h3 {
    margin: 0;
    font-size: 14px;
    font-weight: 700;
    color: var(--text-primary);
  }
  .field-label {
    display: block;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    margin-bottom: 4px;
  }
  .qid-mini {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    font-family: var(--font-mono, ui-monospace, monospace);
    color: var(--text-muted);
    padding: 2px 6px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm, 4px);
    background: var(--bg-card);
    cursor: pointer;
    transition: all 0.15s ease;
  }
  .qid-mini:hover {
    border-color: var(--accent-primary);
    color: var(--text-primary);
  }
  .tab-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    font-weight: 700;
    padding: 1px 5px;
    border-radius: 999px;
  }
  .fmt-details {
    display: inline-block;
  }
  .fmt-bar {
    display: flex;
    gap: 8px;
    padding: 4px;
    margin-top: 4px;
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
  }
  .fmt-bar button {
    font-size: 12px;
    font-weight: 600;
    padding: 3px 6px;
    border: none;
    background: transparent;
    color: var(--text-secondary);
    border-radius: 3px;
    cursor: pointer;
  }
  .fmt-bar button:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .ico.danger {
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--border-radius-sm, 4px);
    border: 1px solid color-mix(in srgb, var(--accent-danger) 35%, var(--border-color));
    background: color-mix(in srgb, var(--accent-danger) 8%, var(--bg-card));
    color: var(--accent-danger);
    cursor: pointer;
    transition: all 0.15s ease;
  }
  .ico.danger:hover {
    border-color: var(--accent-danger);
    background: color-mix(in srgb, var(--accent-danger) 20%, var(--bg-card));
  }
</style>
