<script lang="ts">
  import type { QuestData, QuestValidationIssue } from "../../lib/store";
  import { DEP_REQUIREMENT_OPTIONS, SHAPE_OPTIONS } from "../../lib/questTypeLabels";
  import TaskRewardEditor from "./TaskRewardEditor.svelte";

  let {
    quest,
    chapterQuests,
    issues = [],
    onDirty,
    onRemove,
    onAddDep,
    onRemoveDep,
  }: {
    quest: QuestData;
    chapterQuests: QuestData[];
    issues?: QuestValidationIssue[];
    onDirty: () => void;
    onRemove: () => void;
    onAddDep: (depId: string) => void;
    onRemoveDep: (depId: string) => void;
  } = $props();

  let depPick = $state("");
  let descText = $state("");
  let showAdvanced = $state(false);

  let depOptions = $derived(buildDepOptions(chapterQuests, quest));
  let myIssues = $derived(issues.filter((i) => i.questId === quest.id));

  $effect(() => {
    descText = (quest.description ?? []).join("\n");
  });

  function buildDepOptions(list: QuestData[], current: QuestData) {
    const opts: { id: string; label: string }[] = [];
    for (const q of list) {
      if (q.id === current.id) continue;
      if (!current.dependencies.includes(q.id)) {
        opts.push({ id: q.id, label: q.title });
      }
    }
    return opts;
  }

  function titleOf(id: string) {
    const direct = chapterQuests.find((q) => q.id === id);
    if (direct) return direct.title;
    return id;
  }

  function applyDep() {
    if (!depPick) return;
    onAddDep(depPick);
    depPick = "";
  }

  function commitDescription() {
    quest.description = descText
      .split("\n")
      .map((s) => s.trimEnd())
      .filter((s, i, arr) => s.length > 0 || i < arr.length - 1);
    while (quest.description.length && quest.description[quest.description.length - 1] === "") {
      quest.description.pop();
    }
    onDirty();
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

  function inputVal(e: Event): string {
    return (e.currentTarget as HTMLInputElement).value;
  }
</script>

<aside class="insp">
  <div class="insp-h">
    <h3 title={quest.id}>{quest.title}</h3>
    <button type="button" class="ico danger" title="Delete quest" onclick={onRemove}>×</button>
  </div>
  <code class="qid">{quest.id}</code>

  {#if myIssues.length > 0}
    <div class="val-warn">
      {#each myIssues as issue, i (`${issue.questId}-${i}`)}
        <div>⚠ {issue.message}</div>
      {/each}
    </div>
  {/if}

  <div class="fields">
    <label>Title <input bind:value={quest.title} oninput={onDirty} /></label>
    <label>Subtitle <input bind:value={quest.subtitle} oninput={onDirty} placeholder="Optional" /></label>
    <label>Icon <input bind:value={quest.icon} oninput={onDirty} placeholder="minecraft:stone" /></label>
    <label>
      Description
      <textarea
        rows="4"
        value={descText}
        oninput={(e) => (descText = (e.target as HTMLTextAreaElement).value)}
        onchange={commitDescription}
        placeholder="One line per paragraph · & codes"
      ></textarea>
    </label>
    <label class="checkbox">
      <input type="checkbox" bind:checked={quest.optional} onchange={onDirty} />
      Optional quest
    </label>
    <label>
      Shape
      <select value={quest.shape ?? ""} onchange={(e) => { quest.shape = (e.target as HTMLSelectElement).value || null; onDirty(); }}>
        {#each SHAPE_OPTIONS as s (s.id || "__default")}
          <option value={s.id}>{s.label}</option>
        {/each}
      </select>
    </label>
    <label>Size <input type="number" step="0.25" min="0.25" bind:value={quest.size} oninput={onDirty} placeholder="1" /></label>
    <label>
      Position
      <div class="xy">
        <input type="number" step="0.5" bind:value={quest.x} oninput={onDirty} />
        <input type="number" step="0.5" bind:value={quest.y} oninput={onDirty} />
      </div>
    </label>
  </div>

  <button type="button" class="adv-tog" onclick={() => (showAdvanced = !showAdvanced)}>
    {showAdvanced ? "▾" : "▸"} FTB flags
  </button>
  {#if showAdvanced}
    <div class="fields flags">
      <label>Hide dependency lines
        <select value={triVal(quest.hideDependencyLines)} onchange={(e) => tri("hideDependencyLines", e)}>
          <option value="">unset</option>
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
      </label>
      <label>Hide dependent lines
        <select value={triVal(quest.hideDependentLines)} onchange={(e) => tri("hideDependentLines", e)}>
          <option value="">unset</option>
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
      </label>
      <label>Can repeat
        <select value={triVal(quest.canRepeat)} onchange={(e) => tri("canRepeat", e)}>
          <option value="">unset</option>
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
      </label>
      <label>Invisible
        <select value={triVal(quest.invisible)} onchange={(e) => tri("invisible", e)}>
          <option value="">unset</option>
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
      </label>
      <label>Disable toast
        <select value={triVal(quest.disableToast)} onchange={(e) => tri("disableToast", e)}>
          <option value="">unset</option>
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
      </label>
      <label
        >Min required deps<input
          type="number"
          min="0"
          value={quest.minRequiredDependencies ?? ""}
          oninput={(e) => {
            const v = inputVal(e);
            quest.minRequiredDependencies = v === "" ? null : Number(v);
            onDirty();
          }}
        /></label
      >
      <label>Dependency requirement
        <select
          value={quest.dependencyRequirement ?? ""}
          onchange={(e) => {
            quest.dependencyRequirement = (e.target as HTMLSelectElement).value || null;
            onDirty();
          }}
        >
          {#each DEP_REQUIREMENT_OPTIONS as d (d.id || "_default")}
            <option value={d.id}>{d.label}</option>
          {/each}
        </select>
      </label>
    </div>
  {/if}

  <!-- Tasks / Rewards (editable) -->
  <TaskRewardEditor {quest} {onDirty} />

  <!-- Dependencies -->
  <h4>Dependencies</h4>
  <div class="deps">
    {#each quest.dependencies as dep (dep)}
      <span class="dep-tag">
        {titleOf(dep)}
        <button type="button" class="dep-rm" onclick={() => onRemoveDep(dep)}>×</button>
      </span>
    {/each}
  </div>
  <div class="dep-add">
    <select bind:value={depPick}>
      <option value="">Add dependency…</option>
      {#each depOptions as o (o.id)}
        <option value={o.id}>{o.label}</option>
      {/each}
    </select>
    <button type="button" class="add-btn" disabled={!depPick} onclick={applyDep}>Add</button>
  </div>
</aside>

<style>
  .insp {
    width: 320px;
    background: var(--bg-secondary);
    border-left: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    overflow: auto;
    flex-shrink: 0;
  }
  .insp-h {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
    background: rgba(0,0,0,0.2);
  }
  .insp-h h3 {
    font-size: 14px;
    font-weight: 700;
    color: var(--accent);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .qid {
    display: block;
    font-size: 9px;
    color: var(--text-muted);
    padding: 4px 12px 8px;
    word-break: break-all;
    border-bottom: 1px solid var(--border);
  }
  .val-warn {
    padding: 8px 10px;
    margin: 8px 12px 0;
    background: rgba(242,201,76,0.1);
    border: 1px solid rgba(242,201,76,0.3);
    font-size: 11px;
    color: #fde68a;
    display: grid;
    gap: 4px;
  }
  .fields {
    display: grid;
    gap: 8px;
    padding: 10px 12px;
  }
  .fields label {
    display: grid;
    gap: 4px;
    font-size: 10px;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .fields input, .fields textarea, .fields select {
    font-size: 12px;
    text-transform: none;
  }
  .checkbox {
    display: flex !important;
    flex-direction: row !important;
    align-items: center;
    gap: 8px;
    text-transform: none;
    color: var(--text-primary);
  }
  .xy { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  .adv-tog {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 11px;
    text-align: left;
    padding: 6px 12px;
    cursor: pointer;
    border-top: 1px solid var(--border);
  }
  .adv-tog:hover { color: var(--text-primary); }
  .flags { padding-top: 0; }
  h4 {
    margin: 0;
    padding: 8px 12px 6px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--accent);
    background: rgba(0,0,0,0.15);
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
    font-weight: 700;
  }
  .deps {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 8px 12px;
  }
  .dep-tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    background: rgba(92,138,158,0.15);
    border: 1px solid rgba(92,138,158,0.35);
    font-size: 11px;
    color: var(--text-primary);
    border-radius: 2px;
  }
  .dep-rm {
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    padding: 0;
  }
  .dep-rm:hover { color: var(--danger); }
  .dep-add {
    display: flex;
    gap: 6px;
    padding: 0 12px 12px;
  }
  .dep-add select {
    flex: 1;
    font-size: 11px;
  }
  .add-btn {
    font-size: 11px;
    padding: 4px 10px;
    border: 1px solid var(--border);
    background: rgba(0,0,0,0.25);
    color: var(--text-primary);
    border-radius: 2px;
  }
  .add-btn:hover:not(:disabled) { border-color: var(--accent); }
  .add-btn:disabled { opacity: 0.4; }
  .ico {
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 16px;
  }
  .ico.danger:hover { color: var(--danger); }
</style>
