<script lang="ts">
  import { Trash2, Plus, Sparkles, Trophy, CheckSquare, Coins } from "@lucide/svelte";
  import type { QuestData, QuestReward, QuestTask } from "../../lib/api";
  import {
    REWARD_TYPE_OPTIONS,
    TASK_TYPE_OPTIONS,
    TASK_TYPES,
    REWARD_TYPES,
    OBSERVATION_TYPE_OPTIONS,
    ITEM_COMPLETE_OPTIONS,
    rewardTypeLabel,
    taskTypeLabel,
  } from "../../lib/questTypeLabels";
  import type { ItemValue } from "../../lib/itemStack";
  import { isItemObject, readCount, stackDisplayId } from "../../lib/itemStack";
  import ItemStackEditor from "./ItemStackEditor.svelte";

  let {
    quest,
    onDirty,
    rewardTableIds = [],
    onOpenKubeJs,
    section = "all",
  }: {
    quest: QuestData;
    onDirty: () => void;
    rewardTableIds?: string[];
    onOpenKubeJs?: (id: string) => void;
    section?: "all" | "tasks" | "rewards";
  } = $props();

  function newId(len = 12) {
    return crypto.randomUUID().replace(/-/g, "").slice(0, len);
  }

  function ensureProps(obj: { properties?: Record<string, unknown> }) {
    if (!obj.properties) obj.properties = {};
    return obj.properties;
  }

  function setProp(obj: { properties?: Record<string, unknown> }, key: string, value: unknown) {
    const p = ensureProps(obj);
    if (value === "" || value == null) delete p[key];
    else p[key] = value;
    obj.properties = { ...p };
    onDirty();
  }

  /** FTB stores item count as a sibling `count` field, not ItemStack.Count. */
  function setItemValue(
    obj: { properties?: Record<string, unknown> },
    next: ItemValue | null,
  ) {
    const p = ensureProps(obj);
    if (next == null || next === "") {
      delete p.item;
    } else if (typeof next === "string") {
      p.item = next;
    } else if (isItemObject(next)) {
      const cleaned = { ...next };
      const embedded = cleaned.Count ?? cleaned.count;
      if (typeof embedded === "number" && Number.isFinite(embedded) && embedded > 0) {
        if (p.count == null) p.count = embedded;
      }
      delete cleaned.Count;
      delete cleaned.count;
      p.item = cleaned;
    } else {
      p.item = next;
    }
    obj.properties = { ...p };
    onDirty();
  }

  function itemValueOf(props: Record<string, unknown> | undefined): ItemValue | null {
    const v = props?.item;
    if (typeof v === "string") return v;
    if (v && typeof v === "object" && !Array.isArray(v)) return v as Record<string, unknown>;
    return null;
  }

  function itemCountOf(props: Record<string, unknown> | undefined): number {
    const c = props?.count;
    if (typeof c === "number" && Number.isFinite(c) && c > 0) return c;
    if (typeof c === "string" && c !== "" && !Number.isNaN(Number(c))) {
      const n = Number(c);
      if (n > 0) return n;
    }
    const item = itemValueOf(props);
    if (item) return readCount(item, 1);
    return 1;
  }

  function setItemCount(obj: { properties?: Record<string, unknown> }, n: number) {
    const count = Number.isFinite(n) && n > 0 ? Math.floor(n) : 1;
    const p = ensureProps(obj);
    p.count = count;
    const item = p.item;
    if (isItemObject(item)) {
      const cleaned = { ...item };
      delete cleaned.Count;
      delete cleaned.count;
      p.item = cleaned;
    }
    obj.properties = { ...p };
    onDirty();
  }

  type ItemCompleteMode = "inventory" | "consume" | "craft" | "task_screen";

  function itemCompleteMode(props: Record<string, unknown> | undefined): ItemCompleteMode {
    if (props?.task_screen_only === true || props?.task_screen_only === "true") {
      return "task_screen";
    }
    const craft = props?.only_from_crafting;
    if (craft === true || craft === "TRUE" || craft === "true") return "craft";
    const consume = props?.consume_items;
    if (consume === true || consume === "TRUE" || consume === "true") return "consume";
    return "inventory";
  }

  function setItemCompleteMode(
    task: QuestTask,
    mode: ItemCompleteMode,
  ) {
    const p = ensureProps(task);
    delete p.consume_items;
    delete p.only_from_crafting;
    delete p.task_screen_only;
    if (mode === "consume") p.consume_items = true;
    else if (mode === "craft") p.only_from_crafting = true;
    else if (mode === "task_screen") p.task_screen_only = true;
    task.properties = { ...p };
    onDirty();
  }

  /** User-facing choice: obtain item vs target a block (observation). */
  type GoalKind = "item" | "block";

  function goalKind(task: QuestTask): GoalKind {
    return task.type === "observation" ? "block" : "item";
  }

  function setGoalKind(task: QuestTask, kind: GoalKind) {
    if (kind === "block" && task.type !== "observation") {
      const fromItem =
        stackDisplayId(itemValueOf(task.properties)) ??
        String(task.properties?.to_observe ?? "minecraft:stone");
      task.type = "observation";
      task.properties = {
        timer: 0,
        observation_type: "block",
        to_observe: fromItem.replace(/^#/, "") || "minecraft:stone",
      };
      onDirty();
      return;
    }
    if (kind === "item" && task.type === "observation") {
      const block = String(task.properties?.to_observe ?? "minecraft:stone");
      task.type = "item";
      task.properties = {
        item: block.startsWith("#") ? "minecraft:stone" : block,
        count: 1,
      };
      onDirty();
    }
  }

  function defaultsForTask(type: string): Partial<QuestTask> {
    switch (type) {
      case "item":
        return { properties: { count: 1 } };
      case "kill":
        return { properties: { entity: "minecraft:zombie", value: 1 } };
      case "dimension":
        return { properties: { dimension: "minecraft:overworld" } };
      case "biome":
        return { properties: { biome: "minecraft:plains" } };
      case "advancement":
        return { properties: { advancement: "minecraft:story/root" } };
      case "stat":
        return { properties: { stat: "minecraft:walk_one_cm", value: 100 } };
      case "fluid":
        return { properties: { fluid: "minecraft:water", amount: 1000 } };
      case "location":
        return {
          properties: {
            dimension: "minecraft:overworld",
            ignore_dimension: false,
            x: 0,
            y: 64,
            z: 0,
            w: 1,
            h: 1,
            d: 1,
          },
        };
      case "structure":
        return { properties: { structure: "minecraft:village" } };
      case "gamestage":
      case "stage":
        return { properties: { stage: "" } };
      case "observation":
        return {
          properties: {
            timer: 0,
            observation_type: "block",
            to_observe: "minecraft:stone",
          },
        };
      case "xp":
        return { value: 1, properties: { points: false } };
      case "forge_energy":
      case "techreborn_energy":
      case "energy":
        return { value: 1000 };
      case "custom":
        return { properties: { max_progress: 1, enable_button: false } };
      default:
        return { properties: {} };
    }
  }

  function defaultsForReward(type: string): Record<string, unknown> {
    switch (type) {
      case "item":
        return { item: "minecraft:diamond", count: 1 };
      case "xp":
        return { xp: 10 };
      case "xp_levels":
        return { xp_levels: 1 };
      case "command":
        return { command: "say hello", permission_level: 0, silent: false };
      case "random":
      case "choice":
        return { table: rewardTableIds[0] ?? "" };
      case "loot":
        return { loot_crate: "" };
      case "all_table":
      case "all_tables":
        return {};
      case "gamestage":
      case "stage":
        return { stage: "" };
      case "toast":
        return { description: "" };
      case "advancement":
        return { advancement: "minecraft:story/root", criterion: "" };
      case "currency":
        return { currency: "", amount: 1 };
      default:
        return {};
    }
  }

  function addTask(type = "item") {
    const defaults = defaultsForTask(type);
    const t: QuestTask = {
      id: newId(),
      type,
      properties: defaults.properties ?? {},
      ...(defaults.value != null ? { value: defaults.value } : {}),
    };
    quest.tasks = [...quest.tasks, t];
    onDirty();
  }

  function removeTask(i: number) {
    quest.tasks = quest.tasks.filter((_, idx) => idx !== i);
    onDirty();
  }

  function changeTaskType(task: QuestTask, type: string) {
    const defaults = defaultsForTask(type);
    task.type = type;
    task.properties = { ...(defaults.properties ?? {}) };
    if (defaults.value != null) task.value = defaults.value;
    else delete task.value;
    onDirty();
  }

  function addReward(type = "item") {
    const r: QuestReward = {
      id: newId(),
      type,
      properties: defaultsForReward(type),
    };
    quest.rewards = [...quest.rewards, r];
    onDirty();
  }

  function removeReward(i: number) {
    quest.rewards = quest.rewards.filter((_, idx) => idx !== i);
    onDirty();
  }

  function changeRewardType(reward: QuestReward, type: string) {
    reward.type = type;
    reward.properties = defaultsForReward(type);
    onDirty();
  }

  function numProp(props: Record<string, unknown> | undefined, key: string, fallback = 1): number {
    const v = props?.[key];
    if (typeof v === "number") return v;
    if (typeof v === "string" && v !== "" && !Number.isNaN(Number(v))) return Number(v);
    return fallback;
  }

  function boolProp(props: Record<string, unknown> | undefined, key: string): boolean {
    const v = props?.[key];
    return v === true || v === "true" || v === "TRUE";
  }

  function inputVal(e: Event): string {
    return (e.currentTarget as HTMLInputElement).value;
  }
  function inputNum(e: Event): number {
    return Number((e.currentTarget as HTMLInputElement).value);
  }
  function inputChecked(e: Event): boolean {
    return (e.currentTarget as HTMLInputElement).checked;
  }
  function selectVal(e: Event): string {
    return (e.currentTarget as HTMLSelectElement).value;
  }
  function textareaVal(e: Event): string {
    return (e.currentTarget as HTMLTextAreaElement).value;
  }

  function onPickTaskType(e: Event) {
    const el = e.currentTarget as HTMLSelectElement;
    if (el.value) {
      addTask(el.value);
      el.value = "";
    }
  }

  function onPickRewardType(e: Event) {
    const el = e.currentTarget as HTMLSelectElement;
    if (el.value) {
      addReward(el.value);
      el.value = "";
    }
  }

  function isItemishTask(type: string) {
    return type === "item" || type === "observation";
  }

  function isEnergyTask(type: string) {
    return type === "forge_energy" || type === "techreborn_energy" || type === "energy";
  }

  function isStageTask(type: string) {
    return type === "gamestage" || type === "stage";
  }

  function isStageReward(type: string) {
    return type === "gamestage" || type === "stage";
  }

  function isAllTablesReward(type: string) {
    return type === "all_table" || type === "all_tables";
  }
</script>

{#if section === "all" || section === "tasks"}
<section class="tr ftbq-tr" id="quest-how-to-prove">
  <div class="tr-h flex items-center justify-between gap-2">
    <div class="flex items-center gap-2">
      <CheckSquare size={14} class="text-[var(--accent-primary)]" />
      <h4>How to prove ({quest.tasks?.length ?? 0})</h4>
    </div>
    <div class="add-row flex items-center gap-2">
      <select onchange={onPickTaskType} class="add-type-select">
        <option value="">+ Add Task…</option>
        {#each TASK_TYPE_OPTIONS as t (t.id)}
          <option value={t.id}>{t.label}</option>
        {/each}
      </select>
    </div>
  </div>

  {#if (!quest.tasks || quest.tasks.length === 0)}
    <div class="tr-empty-box">
      <p class="tr-empty-msg">No tasks yet. Choose how players complete this quest:</p>
      <div class="tr-quick-actions">
        <button type="button" class="quick-add-btn" onclick={() => addTask("item")}>
          + Item task
        </button>
        <button type="button" class="quick-add-btn" onclick={() => addTask("checkmark")}>
          + Checkmark
        </button>
        <button type="button" class="quick-add-btn" onclick={() => addTask("kill")}>
          + Kill mob
        </button>
        <button type="button" class="quick-add-btn" onclick={() => addTask("xp")}>
          + XP
        </button>
      </div>
    </div>
  {/if}

  {#each quest.tasks as task, i (task.id)}
    <div class="card">
      <div class="card-h">
        <select
          value={task.type}
          onchange={(e) => changeTaskType(task, selectVal(e))}
        >
          {#each TASK_TYPE_OPTIONS as t (t.id)}
            <option value={t.id}>{t.label}</option>
          {/each}
          {#if !TASK_TYPES.includes(task.type)}
            <option value={task.type}>{taskTypeLabel(task.type)}</option>
          {/if}
        </select>
        <button type="button" class="ico danger" aria-label="Remove task" title="Remove task" onclick={() => removeTask(i)}
          ><Trash2 size={12} /></button
        >
      </div>

      {#if isItemishTask(task.type)}
        <label
          >Complete by
          <select
            value={goalKind(task)}
            onchange={(e) => setGoalKind(task, selectVal(e) as GoalKind)}
          >
            <option value="item">Obtain item in inventory</option>
            <option value="block">Observe / target a block</option>
          </select>
        </label>
      {/if}

      {#if task.type === "item"}
        <ItemStackEditor
          value={itemValueOf(task.properties)}
          allowFilters={true}
          emphasizeEmpty={true}
          emptyCta="Choose item"
          onChange={(v) => setItemValue(task, v)}
        />
        <label
          >Count<input
            type="number"
            min="1"
            value={itemCountOf(task.properties)}
            oninput={(e) => setItemCount(task, inputNum(e) || 1)}
          /></label
        >
        <label
          >Completion mode
          <select
            value={itemCompleteMode(task.properties)}
            onchange={(e) => setItemCompleteMode(task, selectVal(e) as ItemCompleteMode)}
          >
            {#each ITEM_COMPLETE_OPTIONS as opt (opt.id)}
              <option value={opt.id}>{opt.label}</option>
            {/each}
          </select>
        </label>
        <details class="raw">
          <summary>Advanced</summary>
          <p class="hint">
            Tip: “mine N blocks” in FTB Quests is usually an Item task with Count = N (drops enter
            inventory). Native break-block tracking needs an addon (e.g. QNaturals).
          </p>
          <label
            >Match components
            <select
              value={String(task.properties?.match_components ?? "none")}
              onchange={(e) => {
                const v = selectVal(e);
                setProp(task, "match_components", v === "none" ? null : v);
              }}
            >
              <option value="none">None / default</option>
              <option value="fuzzy">Fuzzy</option>
              <option value="strict">Strict</option>
            </select>
          </label>
        </details>
      {:else if task.type === "observation"}
        <label
          >Observation type
          <select
            value={String(task.properties?.observation_type ?? "block")}
            onchange={(e) => setProp(task, "observation_type", selectVal(e))}
          >
            {#each OBSERVATION_TYPE_OPTIONS as opt (opt.id)}
              <option value={opt.id}>{opt.label}</option>
            {/each}
          </select>
        </label>
        <label
          >Target (to_observe)<input
            value={String(task.properties?.to_observe ?? "")}
            oninput={(e) => setProp(task, "to_observe", inputVal(e))}
            placeholder="minecraft:stone or #minecraft:logs"
          /></label
        >
        <label
          >Timer (ticks)<input
            type="number"
            min="0"
            value={numProp(task.properties, "timer", 0)}
            oninput={(e) => setProp(task, "timer", inputNum(e) || 0)}
          /></label
        >
        <label
          >Title<input bind:value={task.title} oninput={onDirty} placeholder="Look at…" /></label
        >
      {:else if task.type === "kill"}
        <label
          >Entity<input
            value={String(task.properties?.entity ?? "")}
            oninput={(e) => setProp(task, "entity", inputVal(e))}
            placeholder="minecraft:zombie"
          /></label
        >
        <label
          >Entity type tag<input
            value={String(task.properties?.entityTypeTag ?? "")}
            oninput={(e) => setProp(task, "entityTypeTag", inputVal(e))}
            placeholder="minecraft:zombies (optional)"
          /></label
        >
        <label
          >Count<input
            type="number"
            min="1"
            value={numProp(task.properties, "value", 1)}
            oninput={(e) => setProp(task, "value", inputNum(e) || 1)}
          /></label
        >
        <label
          >Custom name<input
            value={String(task.properties?.custom_name ?? "")}
            oninput={(e) => setProp(task, "custom_name", inputVal(e))}
            placeholder="Optional name tag / player name"
          /></label
        >
        <label
          >NBT filter<input
            value={String(task.properties?.nbt_filter ?? "")}
            oninput={(e) => setProp(task, "nbt_filter", inputVal(e))}
            placeholder={'{CustomName:"…"}'}
          /></label
        >
      {:else if task.type === "dimension"}
        <label
          >Dimension<input
            value={String(task.properties?.dimension ?? "")}
            oninput={(e) => setProp(task, "dimension", inputVal(e))}
            placeholder="minecraft:the_nether"
          /></label
        >
      {:else if task.type === "biome"}
        <label
          >Biome<input
            value={String(task.properties?.biome ?? "")}
            oninput={(e) => setProp(task, "biome", inputVal(e))}
            placeholder="minecraft:plains"
          /></label
        >
      {:else if task.type === "xp"}
        <label
          >Amount<input
            type="number"
            min="1"
            value={typeof task.value === "number"
              ? task.value
              : numProp(task.properties, "value", Number(task.value) || 1)}
            oninput={(e) => {
              const n = inputNum(e) || 1;
              task.value = n;
              setProp(task, "value", n);
            }}
          /></label
        >
        <label class="checkbox">
          <input
            type="checkbox"
            checked={boolProp(task.properties, "points")}
            onchange={(e) => setProp(task, "points", inputChecked(e))}
          />
          Consume XP points (not levels)
        </label>
      {:else if task.type === "checkmark"}
        <p class="hint">Manual checkmark — player clicks to complete. No extra fields.</p>
      {:else if isStageTask(task.type)}
        <label
          >Stage<input
            value={String(task.properties?.stage ?? "")}
            oninput={(e) => setProp(task, "stage", inputVal(e))}
          /></label
        >
      {:else if task.type === "advancement"}
        <label
          >Advancement<input
            value={String(task.properties?.advancement ?? "")}
            oninput={(e) => setProp(task, "advancement", inputVal(e))}
            placeholder="minecraft:story/mine_stone"
          /></label
        >
      {:else if task.type === "stat"}
        <label
          >Stat<input
            value={String(task.properties?.stat ?? "")}
            oninput={(e) => setProp(task, "stat", inputVal(e))}
            placeholder="minecraft:walk_one_cm"
          /></label
        >
        <label
          >Value<input
            type="number"
            min="1"
            value={numProp(task.properties, "value", 1)}
            oninput={(e) => setProp(task, "value", inputNum(e) || 1)}
          /></label
        >
      {:else if task.type === "fluid"}
        <label
          >Fluid<input
            value={String(task.properties?.fluid ?? task.properties?.fluid_name ?? "")}
            oninput={(e) => setProp(task, "fluid", inputVal(e))}
            placeholder="minecraft:water"
          /></label
        >
        <label
          >Amount (mB)<input
            type="number"
            min="1"
            value={numProp(task.properties, "amount", 1000)}
            oninput={(e) => setProp(task, "amount", inputNum(e) || 1)}
          /></label
        >
      {:else if task.type === "location"}
        <label
          >Dimension<input
            value={String(task.properties?.dimension ?? "")}
            oninput={(e) => setProp(task, "dimension", inputVal(e))}
            placeholder="minecraft:overworld"
          /></label
        >
        <label class="checkbox">
          <input
            type="checkbox"
            checked={boolProp(task.properties, "ignore_dimension")}
            onchange={(e) => setProp(task, "ignore_dimension", inputChecked(e))}
          />
          Ignore dimension
        </label>
        <label
          >Position (x y z)
          <div class="item-row">
            <input
              type="number"
              title="x"
              value={numProp(task.properties, "x", 0)}
              oninput={(e) => setProp(task, "x", inputNum(e))}
            />
            <input
              type="number"
              title="y"
              value={numProp(task.properties, "y", 0)}
              oninput={(e) => setProp(task, "y", inputNum(e))}
            />
            <input
              type="number"
              title="z"
              value={numProp(task.properties, "z", 0)}
              oninput={(e) => setProp(task, "z", inputNum(e))}
            />
          </div>
        </label>
        <label
          >Size (w h d)
          <div class="item-row">
            <input
              type="number"
              min="1"
              title="w"
              value={numProp(task.properties, "w", 1)}
              oninput={(e) => setProp(task, "w", inputNum(e) || 1)}
            />
            <input
              type="number"
              min="1"
              title="h"
              value={numProp(task.properties, "h", 1)}
              oninput={(e) => setProp(task, "h", inputNum(e) || 1)}
            />
            <input
              type="number"
              min="1"
              title="d"
              value={numProp(task.properties, "d", 1)}
              oninput={(e) => setProp(task, "d", inputNum(e) || 1)}
            />
          </div>
        </label>
      {:else if task.type === "structure"}
        <label
          >Structure<input
            value={String(task.properties?.structure ?? "")}
            oninput={(e) => setProp(task, "structure", inputVal(e))}
            placeholder="minecraft:village"
          /></label
        >
      {:else if isEnergyTask(task.type)}
        <label
          >Energy amount<input
            type="number"
            min="1"
            value={typeof task.value === "number"
              ? task.value
              : numProp(task.properties, "value", Number(task.value) || 1000)}
            oninput={(e) => {
              const n = inputNum(e) || 1;
              task.value = n;
              setProp(task, "value", n);
            }}
          /></label
        >
        <p class="hint">Submitted via Task Screen (FE/RF or Tech Reborn energy).</p>
      {:else if task.type === "custom"}
        <label
          >Title<input bind:value={task.title} oninput={onDirty} placeholder="Custom task" /></label
        >
        <label
          >Max progress<input
            type="number"
            min="1"
            value={numProp(task.properties, "max_progress", 1)}
            oninput={(e) => setProp(task, "max_progress", inputNum(e) || 1)}
          /></label
        >
        <label class="checkbox">
          <input
            type="checkbox"
            checked={boolProp(task.properties, "enable_button")}
            onchange={(e) => setProp(task, "enable_button", inputChecked(e))}
          />
          Enable button
        </label>
        {#if onOpenKubeJs}
          <div class="kjs-row">
            <button type="button" class="kjs-btn" onclick={() => onOpenKubeJs?.(task.id)}
              >Open KubeJS</button
            >
          </div>
          <p class="hint">Custom task stub — implement FTBQuestsEvents.customTask in Book → KubeJS.</p>
        {:else}
          <p class="hint">Wire logic in Book → KubeJS (FTBQuestsEvents.customTask).</p>
        {/if}
      {:else}
        <label
          >Title<input
            bind:value={task.title}
            oninput={onDirty}
            placeholder="Optional title"
          /></label
        >
      {/if}

      <details class="raw">
        <summary>Advanced · raw properties</summary>
        <textarea
          rows="3"
          value={JSON.stringify(task.properties ?? {}, null, 0)}
          onchange={(e) => {
            try {
              task.properties = JSON.parse(textareaVal(e));
              onDirty();
            } catch {
              /* ignore invalid */
            }
          }}
        ></textarea>
      </details>
    </div>
  {/each}
</section>
{/if}

{#if section === "all" || section === "rewards"}
<section class="tr ftbq-tr" id="quest-what-you-get">
  <div class="tr-h flex items-center justify-between gap-2">
    <div class="flex items-center gap-2">
      <Trophy size={14} class="text-[var(--accent-warning)]" />
      <h4>What you get ({quest.rewards?.length ?? 0})</h4>
    </div>
    <div class="add-row flex items-center gap-2">
      <select onchange={onPickRewardType} class="add-type-select">
        <option value="">+ Add Reward…</option>
        {#each REWARD_TYPE_OPTIONS as t (t.id)}
          <option value={t.id}>{t.label}</option>
        {/each}
      </select>
    </div>
  </div>

  {#if (!quest.rewards || quest.rewards.length === 0)}
    <div class="tr-empty-box">
      <p class="tr-empty-msg">No rewards yet. Add XP, items, or loot tables:</p>
      <div class="tr-quick-actions">
        <button type="button" class="quick-add-btn" onclick={() => addReward("xp")}>
          + XP reward
        </button>
        <button type="button" class="quick-add-btn" onclick={() => addReward("item")}>
          + Item reward
        </button>
        <button type="button" class="quick-add-btn" onclick={() => addReward("random")}>
          + Random loot
        </button>
      </div>
    </div>
  {/if}

  {#each quest.rewards as reward, i (reward.id)}
    <div class="card">
      <div class="card-h">
        <select
          value={reward.type}
          onchange={(e) => changeRewardType(reward, selectVal(e))}
        >
          {#each REWARD_TYPE_OPTIONS as t (t.id)}
            <option value={t.id}>{t.label}</option>
          {/each}
          {#if !REWARD_TYPES.includes(reward.type)}
            <option value={reward.type}>{rewardTypeLabel(reward.type)}</option>
          {/if}
        </select>
        <button type="button" class="ico danger" aria-label="Remove reward" title="Remove reward" onclick={() => removeReward(i)}
          ><Trash2 size={12} /></button
        >
      </div>

      {#if reward.type === "item"}
        <ItemStackEditor
          value={itemValueOf(reward.properties)}
          allowFilters={true}
          onChange={(v) => setItemValue(reward, v)}
        />
        <label
          >Count<input
            type="number"
            min="1"
            value={itemCountOf(reward.properties)}
            oninput={(e) => setItemCount(reward, inputNum(e) || 1)}
          /></label
        >
        <label
          >Random bonus<input
            type="number"
            min="0"
            value={numProp(reward.properties, "random_bonus", 0)}
            oninput={(e) => setProp(reward, "random_bonus", inputNum(e) || 0)}
          /></label
        >
        <label class="checkbox">
          <input
            type="checkbox"
            checked={boolProp(reward.properties, "only_one")}
            onchange={(e) => setProp(reward, "only_one", inputChecked(e))}
          />
          Only one (skip if already owned)
        </label>
      {:else if reward.type === "xp"}
        <label
          >XP<input
            type="number"
            min="1"
            value={numProp(reward.properties, "xp", 10)}
            oninput={(e) => setProp(reward, "xp", inputNum(e) || 1)}
          /></label
        >
      {:else if reward.type === "xp_levels"}
        <label
          >Levels<input
            type="number"
            min="1"
            value={numProp(reward.properties, "xp_levels", 1)}
            oninput={(e) => setProp(reward, "xp_levels", inputNum(e) || 1)}
          /></label
        >
      {:else if reward.type === "command"}
        <label
          >Command<input
            value={String(reward.properties?.command ?? "")}
            oninput={(e) => setProp(reward, "command", inputVal(e))}
            placeholder="/say hi"
          /></label
        >
        <label
          >Permission level<input
            type="number"
            min="0"
            max="4"
            value={numProp(reward.properties, "permission_level", 0)}
            oninput={(e) => setProp(reward, "permission_level", inputNum(e) || 0)}
          /></label
        >
        <label class="checkbox">
          <input
            type="checkbox"
            checked={boolProp(reward.properties, "silent")}
            onchange={(e) => setProp(reward, "silent", inputChecked(e))}
          />
          Silent
        </label>
        <label
          >Feedback message<input
            value={String(reward.properties?.feedback_message ?? "")}
            oninput={(e) => setProp(reward, "feedback_message", inputVal(e))}
          /></label
        >
      {:else if reward.type === "random" || reward.type === "choice"}
        <label
          >Reward table
          <select
            value={String(reward.properties?.table ?? "")}
            onchange={(e) => setProp(reward, "table", selectVal(e))}
          >
            <option value="">Select table…</option>
            {#each rewardTableIds as tid (tid)}
              <option value={tid}>{tid}</option>
            {/each}
          </select>
        </label>
      {:else if reward.type === "loot"}
        <label
          >Loot crate id<input
            value={String(
              reward.properties?.loot_crate ?? reward.properties?.table ?? "",
            )}
            oninput={(e) => setProp(reward, "loot_crate", inputVal(e))}
            placeholder="crate_id"
          /></label
        >
      {:else if isAllTablesReward(reward.type)}
        <p class="hint">Grants a roll from every reward table in the book.</p>
      {:else if isStageReward(reward.type)}
        <label
          >Stage<input
            value={String(reward.properties?.stage ?? "")}
            oninput={(e) => setProp(reward, "stage", inputVal(e))}
          /></label
        >
      {:else if reward.type === "toast"}
        <label
          >Description<input
            value={String(reward.properties?.description ?? reward.title ?? "")}
            oninput={(e) => setProp(reward, "description", inputVal(e))}
          /></label
        >
      {:else if reward.type === "advancement"}
        <label
          >Advancement<input
            value={String(reward.properties?.advancement ?? "")}
            oninput={(e) => setProp(reward, "advancement", inputVal(e))}
            placeholder="minecraft:story/root"
          /></label
        >
        <label
          >Criterion<input
            value={String(reward.properties?.criterion ?? "")}
            oninput={(e) => setProp(reward, "criterion", inputVal(e))}
            placeholder="Optional criterion id"
          /></label
        >
      {:else if reward.type === "currency"}
        <label
          >Currency id<input
            value={String(reward.properties?.currency ?? "")}
            oninput={(e) => setProp(reward, "currency", inputVal(e))}
          /></label
        >
        <label
          >Amount<input
            type="number"
            min="1"
            value={numProp(reward.properties, "amount", 1)}
            oninput={(e) => setProp(reward, "amount", inputNum(e) || 1)}
          /></label
        >
      {:else if reward.type === "custom"}
        <label
          >Title<input bind:value={reward.title} oninput={onDirty} placeholder="Custom reward" /></label
        >
        <label
          >Description<input
            value={String(reward.properties?.description ?? "")}
            oninput={(e) => setProp(reward, "description", inputVal(e))}
            placeholder="Optional note for stub"
          /></label
        >
        {#if onOpenKubeJs}
          <div class="kjs-row">
            <button type="button" class="kjs-btn" onclick={() => onOpenKubeJs?.(reward.id)}
              >Open KubeJS</button
            >
          </div>
          <p class="hint">Custom reward stub — implement FTBQuestsEvents.customReward in Book → KubeJS.</p>
        {:else}
          <p class="hint">Wire logic in Book → KubeJS (FTBQuestsEvents.customReward).</p>
        {/if}
      {:else}
        <label
          >Title<input bind:value={reward.title} oninput={onDirty} placeholder="Optional" /></label
        >
      {/if}
      <details class="raw">
        <summary>Advanced · raw properties</summary>
        <textarea
          rows="3"
          value={JSON.stringify(reward.properties ?? {}, null, 0)}
          onchange={(e) => {
            try {
              reward.properties = JSON.parse(textareaVal(e));
              onDirty();
            } catch {
              /* ignore */
            }
          }}
        ></textarea>
      </details>
    </div>
  {/each}
</section>
{/if}

<style>
  .tr {
    display: grid;
    gap: 0;
    margin-top: 0;
  }
  .tr-h {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 10px 14px;
    background: var(--bg-secondary, var(--ftbq-bg-panel));
    border-bottom: 1px solid var(--border-color, var(--ftbq-frame));
  }
  .tr-h h4 {
    margin: 0;
    font-size: 13px;
    font-weight: 700;
    color: var(--text-primary, var(--ftbq-text));
    letter-spacing: -0.01em;
  }
  .add-type-select {
    font-size: 12px;
    font-weight: 600;
    padding: 5px 10px;
    background: var(--bg-card, var(--ftbq-btn-top));
    border: 1px solid var(--border-color, var(--ftbq-frame));
    color: var(--text-primary, var(--ftbq-text));
    border-radius: var(--border-radius-sm, 6px);
    cursor: pointer;
    transition: all 0.15s ease;
  }
  .add-type-select:hover {
    border-color: var(--accent-primary);
    background: var(--bg-hover);
  }
  .tr-empty-box {
    padding: 18px 14px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    text-align: center;
    background: color-mix(in srgb, var(--bg-primary) 40%, transparent);
    border-bottom: 1px dashed var(--border-color, var(--ftbq-border));
  }
  .tr-empty-msg {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted, var(--ftbq-text-muted));
  }
  .tr-quick-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    justify-content: center;
  }
  .quick-add-btn {
    font-size: 12px;
    font-weight: 600;
    padding: 5px 10px;
    background: var(--bg-card, var(--bg-elevated));
    border: 1px solid var(--border-color, var(--ftbq-frame));
    color: var(--text-secondary, var(--ftbq-text));
    border-radius: var(--border-radius-sm, 6px);
    cursor: pointer;
    transition: all 0.15s ease;
  }
  .quick-add-btn:hover {
    background: var(--bg-hover);
    border-color: var(--accent-primary);
    color: var(--accent-primary);
  }
  .tr-h h4 {
    margin: 0;
    color: var(--text-primary, var(--ftbq-text));
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.01em;
  }
  .add-row {
    display: flex;
    gap: 8px;
  }
  .add-row select {
    font-size: 12px;
    padding: 6px 10px;
    min-width: 140px;
    background: var(--ftbq-input-bg);
    border: 1px solid var(--ftbq-frame);
    color: var(--ftbq-text);
    border-radius: var(--ftbq-radius-control);
    cursor: pointer;
    transition: border-color 0.12s ease, box-shadow 0.12s ease;
  }
  .add-row select:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, var(--ftbq-frame));
  }
  .add-row select:focus {
    outline: none;
    border-color: var(--ftbq-focus-border);
    box-shadow: 0 0 0 2px var(--ftbq-focus-ring);
  }

  .card {
    display: grid;
    gap: 8px;
    padding: 12px;
    border-bottom: 1px solid var(--ftbq-border);
    background: var(--bg-secondary, var(--ftbq-bg-panel));
    margin: 0;
    transition: background 0.12s ease;
  }
  .card:hover {
    background: color-mix(in srgb, var(--ftbq-bg-panel) 95%, var(--bg-hover) 5%);
  }
  .card-h {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .card-h select {
    flex: 1;
    font-size: 12px;
    padding: 8px 10px;
    background: var(--ftbq-input-bg);
    border: 1px solid var(--ftbq-frame);
    color: var(--ftbq-text);
    border-radius: var(--ftbq-radius-control);
    cursor: pointer;
    transition: border-color 0.12s ease, box-shadow 0.12s ease;
  }
  .card-h select:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, var(--ftbq-frame));
  }
  .card-h select:focus {
    outline: none;
    border-color: var(--ftbq-focus-border);
    box-shadow: 0 0 0 2px var(--ftbq-focus-ring);
  }

  .card label {
    display: grid;
    gap: 8px;
    font-size: 12px;
    letter-spacing: 0;
    color: var(--ftbq-text-muted);
  }

  .card input[type="number"],
  .card select {
    font-size: 12px;
    text-transform: none;
    padding: 7px 10px;
    background: var(--ftbq-input-bg);
    border: 1px solid var(--ftbq-frame);
    color: var(--ftbq-text);
    border-radius: var(--ftbq-radius-control);
    transition: border-color 0.12s ease, box-shadow 0.12s ease;
  }
  .card input[type="number"]:hover,
  .card select:hover {
    border-color: color-mix(in srgb, var(--ftbq-text-muted) 50%, var(--ftbq-frame));
  }
  .card input[type="number"]:focus,
  .card select:focus {
    outline: none;
    border-color: var(--ftbq-focus-border);
    box-shadow: 0 0 0 2px var(--ftbq-focus-ring);
  }

  .item-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .item-row input {
    flex: 1;
    min-width: 0;
    padding: 7px 8px;
  }

  .ico {
    width: 34px;
    height: 34px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--ftbq-radius-control);
    border: 1px solid var(--ftbq-frame);
    background: var(--bg-secondary, var(--ftbq-bg-panel));
    color: var(--ftbq-text-muted);
    cursor: pointer;
    flex-shrink: 0;
    transition: all 0.12s ease;
  }
  .ico.danger:hover,
  .ico:hover {
    border-color: color-mix(in srgb, var(--accent-danger) 45%, var(--ftbq-frame));
    background: color-mix(in srgb, var(--accent-danger) 10%, transparent);
    color: var(--accent-danger);
  }
  .ico:active {
    transform: scale(0.95);
  }
  .ico:focus-visible {
    outline: 2px solid var(--ftbq-accent-teal);
    outline-offset: 1px;
  }

  .hint {
    margin: 0;
    padding: 8px 10px;
    font-size: 12px;
    color: var(--ftbq-text-muted);
    text-transform: none;
    background: rgba(0, 0, 0, 0.15);
    border-radius: var(--ftbq-radius-control);
    border-left: 2px solid var(--ftbq-accent-teal);
  }

  .raw {
    margin-top: 4px;
    font-size: 12px;
    color: var(--ftbq-text-muted);
    text-transform: none;
  }
  .raw summary {
    cursor: pointer;
    padding: 4px 0;
    list-style: none;
  }
  .raw summary::-webkit-details-marker {
    display: none;
  }
  .raw summary::before {
    content: "▸ ";
  }
  .raw[open] summary::before {
    content: "▾ ";
  }
  .raw textarea {
    width: 100%;
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
    text-transform: none;
    background: var(--ftbq-bg);
    border: 1px solid var(--ftbq-frame);
    color: var(--ftbq-text);
    border-radius: var(--ftbq-radius-control);
    padding: 8px;
  }
  .raw textarea:focus {
    outline: none;
    border-color: var(--ftbq-focus-border);
    box-shadow: 0 0 0 2px var(--ftbq-focus-ring);
  }

  .checkbox {
    display: flex !important;
    flex-direction: row !important;
    align-items: center;
    gap: 8px;
    text-transform: none;
    color: var(--ftbq-text);
    padding: 4px 0;
  }
  .checkbox input[type="checkbox"] {
    width: 16px;
    height: 16px;
    accent-color: var(--ftbq-accent-teal);
    cursor: pointer;
  }

  .kjs-row {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    padding-top: 4px;
  }
  .kjs-btn {
    font-size: 12px;
    font-weight: 600;
    padding: 6px 12px;
    border-radius: var(--ftbq-radius-control);
    border: 1px solid var(--ftbq-accent-teal);
    background: rgba(61, 184, 168, 0.12);
    color: var(--ftbq-accent-teal);
    cursor: pointer;
    transition: all 0.12s ease;
  }
  .kjs-btn:hover {
    background: rgba(61, 184, 168, 0.22);
    border-color: var(--ftbq-accent-teal);
  }
  .kjs-btn:active {
    transform: scale(0.98);
  }
  .kjs-btn:focus-visible {
    outline: 2px solid var(--ftbq-accent-teal);
    outline-offset: 1px;
  }
</style>
