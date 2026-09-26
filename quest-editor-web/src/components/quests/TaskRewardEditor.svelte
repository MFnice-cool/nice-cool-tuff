<script lang="ts">
  import type { QuestData, QuestReward, QuestTask } from "../../lib/store";
  import {
    REWARD_TYPE_OPTIONS,
    TASK_TYPE_OPTIONS,
    TASK_TYPES,
    REWARD_TYPES,
    rewardTypeLabel,
    taskTypeLabel,
  } from "../../lib/questTypeLabels";

  let {
    quest,
    onDirty,
  }: {
    quest: QuestData;
    onDirty: () => void;
  } = $props();

  function newId(len = 12) {
    return crypto.randomUUID().replace(/-/g, "").slice(0, len);
  }

  function ensureProps(obj: { properties?: Record<string, unknown> }) {
    if (!obj.properties) obj.properties = {};
    return obj.properties;
  }

  function getItemId(props: Record<string, unknown> | undefined): string {
    const v = props?.item;
    if (typeof v === "string") return v;
    if (!v || typeof v !== "object") return "";
    const obj = v as Record<string, unknown>;
    if (typeof obj.id === "string") return String(obj.id);
    return "";
  }

  function setProp(obj: { properties?: Record<string, unknown> }, key: string, value: unknown) {
    const p = ensureProps(obj);
    if (value === "" || value == null) delete p[key];
    else p[key] = value;
    obj.properties = { ...p };
    onDirty();
  }

  function addTask(type = "item") {
    const t: QuestTask = {
      id: newId(),
      type,
      properties: {},
    };
    if (type === "item") t.properties = { item: "minecraft:stone", count: 1 };
    else if (type === "kill") t.properties = { entity: "minecraft:zombie", value: 1 };
    else if (type === "dimension") t.properties = { dimension: "minecraft:overworld" };
    else if (type === "biome") t.properties = { biome: "minecraft:plains" };
    else if (type === "xp") t.value = 1;
    else if (type === "location") t.properties = { dimension: "minecraft:overworld", x: 0, y: 64, z: 0 };
    else if (type === "stat") t.properties = { stat: "minecraft:walk_one_cm", value: 1 };
    else if (type === "fluid") t.properties = { fluid: "minecraft:water", amount: 1000 };
    else if (type === "advancement") t.properties = { advancement: "minecraft:story/mine_stone" };
    else if (type === "structure") t.properties = { structure: "minecraft:village" };
    else if (type === "stage") t.properties = { stage: "" };
    quest.tasks = [...quest.tasks, t];
    onDirty();
  }

  function removeTask(i: number) {
    quest.tasks = quest.tasks.filter((_, idx) => idx !== i);
    onDirty();
  }

  function addReward(type = "item") {
    const r: QuestReward = {
      id: newId(),
      type,
      properties:
        type === "item"
          ? { item: "minecraft:diamond", count: 1 }
          : type === "xp"
            ? { xp: 10 }
            : type === "xp_levels"
              ? { xp_levels: 1 }
              : type === "command"
                ? { command: "say hello" }
                : type === "stage"
                  ? { stage: "" }
                  : type === "toast"
                    ? { description: "" }
                    : {},
    };
    quest.rewards = [...quest.rewards, r];
    onDirty();
  }

  function removeReward(i: number) {
    quest.rewards = quest.rewards.filter((_, idx) => idx !== i);
    onDirty();
  }

  function numProp(props: Record<string, unknown> | undefined, key: string, fallback = 1): number {
    const v = props?.[key];
    if (typeof v === "number") return v;
    if (typeof v === "string" && v !== "" && !Number.isNaN(Number(v))) return Number(v);
    return fallback;
  }

  function strProp(props: Record<string, unknown> | undefined, key: string): string {
    const v = props?.[key];
    return v == null ? "" : String(v);
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

  function changeTaskType(task: QuestTask, type: string) {
    task.type = type;
    // Seed sensible defaults when switching into a typed form
    if (type === "item" && !task.properties?.item) {
      task.properties = { item: "minecraft:stone", count: 1 };
    } else if (type === "kill" && !task.properties?.entity) {
      task.properties = { entity: "minecraft:zombie", value: 1 };
    } else if (type === "location" && task.properties?.x == null) {
      task.properties = { ...(task.properties ?? {}), dimension: "minecraft:overworld", x: 0, y: 64, z: 0 };
    }
    onDirty();
  }
</script>

<section class="tr">
  <div class="tr-h">
    <h4>Tasks</h4>
    <select class="add-sel" onchange={onPickTaskType} title="Add task">
      <option value="">+ Task…</option>
      {#each TASK_TYPE_OPTIONS as t (t.id)}
        <option value={t.id}>{t.label}</option>
      {/each}
    </select>
  </div>

  {#if !quest.tasks?.length}
    <div class="tr-empty">No tasks — pick a type to add one.</div>
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
        <button type="button" class="ico danger" title="Remove task" onclick={() => removeTask(i)}>×</button>
      </div>

      <label>Title <input bind:value={task.title} oninput={onDirty} placeholder="Optional" /></label>

      {#if task.type === "item"}
        <label
          >Item
          <input
            value={getItemId(task.properties)}
            oninput={(e) => setProp(task, "item", inputVal(e))}
            placeholder="modid:item"
          />
        </label>
        <label
          >Count
          <input
            type="number"
            min="1"
            value={numProp(task.properties, "count", 1)}
            oninput={(e) => setProp(task, "count", inputNum(e) || 1)}
          />
        </label>
        <label class="checkbox">
          <input
            type="checkbox"
            checked={!!task.properties?.consume_items}
            onchange={(e) => setProp(task, "consume_items", inputChecked(e))}
          />
          Consume items
        </label>
      {:else if task.type === "kill"}
        <label
          >Entity
          <input
            value={strProp(task.properties, "entity")}
            oninput={(e) => setProp(task, "entity", inputVal(e))}
            placeholder="minecraft:zombie"
          />
        </label>
        <label
          >Count
          <input
            type="number"
            min="1"
            value={numProp(task.properties, "value", 1)}
            oninput={(e) => setProp(task, "value", inputNum(e) || 1)}
          />
        </label>
      {:else if task.type === "dimension"}
        <label
          >Dimension
          <input
            value={strProp(task.properties, "dimension")}
            oninput={(e) => setProp(task, "dimension", inputVal(e))}
            placeholder="minecraft:the_nether"
          />
        </label>
      {:else if task.type === "biome"}
        <label
          >Biome
          <input
            value={strProp(task.properties, "biome")}
            oninput={(e) => setProp(task, "biome", inputVal(e))}
            placeholder="minecraft:plains"
          />
        </label>
      {:else if task.type === "xp"}
        <label
          >XP
          <input
            type="number"
            min="1"
            value={typeof task.value === "number" ? task.value : Number(task.value) || 1}
            oninput={(e) => {
              task.value = inputNum(e) || 1;
              onDirty();
            }}
          />
        </label>
      {:else if task.type === "checkmark"}
        <p class="hint">Manual checkmark — no extra fields.</p>
      {:else if task.type === "stage"}
        <label
          >Stage
          <input
            value={strProp(task.properties, "stage")}
            oninput={(e) => setProp(task, "stage", inputVal(e))}
          />
        </label>
      {:else if task.type === "advancement"}
        <label
          >Advancement
          <input
            value={strProp(task.properties, "advancement")}
            oninput={(e) => setProp(task, "advancement", inputVal(e))}
            placeholder="minecraft:story/mine_stone"
          />
        </label>
      {:else if task.type === "stat"}
        <label
          >Stat
          <input
            value={strProp(task.properties, "stat")}
            oninput={(e) => setProp(task, "stat", inputVal(e))}
            placeholder="minecraft:walk_one_cm"
          />
        </label>
        <label
          >Value
          <input
            type="number"
            value={numProp(task.properties, "value", 1)}
            oninput={(e) => setProp(task, "value", inputNum(e) || 1)}
          />
        </label>
      {:else if task.type === "fluid"}
        <label
          >Fluid
          <input
            value={strProp(task.properties, "fluid") || strProp(task.properties, "fluid_name")}
            oninput={(e) => setProp(task, "fluid", inputVal(e))}
            placeholder="minecraft:water"
          />
        </label>
        <label
          >Amount (mB)
          <input
            type="number"
            value={numProp(task.properties, "amount", 1000)}
            oninput={(e) => setProp(task, "amount", inputNum(e) || 1)}
          />
        </label>
      {:else if task.type === "location"}
        <label
          >Dimension
          <input
            value={strProp(task.properties, "dimension")}
            oninput={(e) => setProp(task, "dimension", inputVal(e))}
            placeholder="minecraft:overworld"
          />
        </label>
        <label
          >Position (x / y / z)
          <div class="xyz">
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
          >Radius
          <input
            type="number"
            min="0"
            value={numProp(task.properties, "radius", 0)}
            oninput={(e) => setProp(task, "radius", inputNum(e))}
          />
        </label>
      {:else if task.type === "observation"}
        <label
          >Timer (ticks)
          <input
            type="number"
            value={numProp(task.properties, "timer", 0)}
            oninput={(e) => setProp(task, "timer", inputNum(e) || 0)}
          />
        </label>
      {:else if task.type === "structure"}
        <label
          >Structure
          <input
            value={strProp(task.properties, "structure")}
            oninput={(e) => setProp(task, "structure", inputVal(e))}
            placeholder="minecraft:village"
          />
        </label>
      {:else}
        <p class="hint">Use raw properties for this task type.</p>
      {/if}

      <details class="raw">
        <summary>Raw properties</summary>
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

  <div class="tr-h">
    <h4>Rewards</h4>
    <select class="add-sel" onchange={onPickRewardType} title="Add reward">
      <option value="">+ Reward…</option>
      {#each REWARD_TYPE_OPTIONS as t (t.id)}
        <option value={t.id}>{t.label}</option>
      {/each}
    </select>
  </div>

  {#if !quest.rewards?.length}
    <div class="tr-empty">No rewards — pick a type to add one.</div>
  {/if}

  {#each quest.rewards as reward, i (reward.id)}
    <div class="card">
      <div class="card-h">
        <select
          value={reward.type}
          onchange={(e) => {
            reward.type = selectVal(e);
            onDirty();
          }}
        >
          {#each REWARD_TYPE_OPTIONS as t (t.id)}
            <option value={t.id}>{t.label}</option>
          {/each}
          {#if !REWARD_TYPES.includes(reward.type)}
            <option value={reward.type}>{rewardTypeLabel(reward.type)}</option>
          {/if}
        </select>
        <button type="button" class="ico danger" title="Remove reward" onclick={() => removeReward(i)}>×</button>
      </div>

      <label>Title <input bind:value={reward.title} oninput={onDirty} placeholder="Optional" /></label>

      {#if reward.type === "item"}
        <label
          >Item
          <input
            value={getItemId(reward.properties)}
            oninput={(e) => setProp(reward, "item", inputVal(e))}
            placeholder="modid:item"
          />
        </label>
        <label
          >Count
          <input
            type="number"
            min="1"
            value={numProp(reward.properties, "count", 1)}
            oninput={(e) => setProp(reward, "count", inputNum(e) || 1)}
          />
        </label>
      {:else if reward.type === "xp"}
        <label
          >XP
          <input
            type="number"
            min="1"
            value={numProp(reward.properties, "xp", 10)}
            oninput={(e) => setProp(reward, "xp", inputNum(e) || 1)}
          />
        </label>
      {:else if reward.type === "xp_levels"}
        <label
          >Levels
          <input
            type="number"
            min="1"
            value={numProp(reward.properties, "xp_levels", 1)}
            oninput={(e) => setProp(reward, "xp_levels", inputNum(e) || 1)}
          />
        </label>
      {:else if reward.type === "command"}
        <label
          >Command
          <input
            value={strProp(reward.properties, "command")}
            oninput={(e) => setProp(reward, "command", inputVal(e))}
            placeholder="/say hi"
          />
        </label>
      {:else if reward.type === "random" || reward.type === "choice"}
        <label
          >Table id
          <input
            value={strProp(reward.properties, "table")}
            oninput={(e) => setProp(reward, "table", inputVal(e))}
            placeholder="reward_table_id"
          />
        </label>
      {:else if reward.type === "stage"}
        <label
          >Stage
          <input
            value={strProp(reward.properties, "stage")}
            oninput={(e) => setProp(reward, "stage", inputVal(e))}
          />
        </label>
      {:else if reward.type === "toast"}
        <label
          >Description
          <input
            value={strProp(reward.properties, "description") || (reward.title ?? "")}
            oninput={(e) => setProp(reward, "description", inputVal(e))}
          />
        </label>
      {:else}
        <p class="hint">Use raw properties for this reward type.</p>
      {/if}

      <details class="raw">
        <summary>Raw properties</summary>
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

<style>
  .tr {
    display: flex;
    flex-direction: column;
    gap: 0;
    border-top: 1px solid var(--border);
  }
  .tr-h {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 12px 6px;
    background: rgba(0, 0, 0, 0.15);
    border-bottom: 1px solid var(--border);
  }
  .tr-h h4 {
    margin: 0;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--accent);
    font-weight: 700;
  }
  .add-sel {
    font-size: 11px;
    max-width: 140px;
  }
  .tr-empty {
    font-size: 11px;
    color: var(--text-muted);
    padding: 8px 12px;
    font-style: italic;
  }
  .card {
    display: grid;
    gap: 6px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    background: rgba(0, 0, 0, 0.12);
  }
  .card-h {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .card-h select {
    flex: 1;
    font-size: 11px;
  }
  .card label {
    display: grid;
    gap: 3px;
    font-size: 10px;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .card input,
  .card textarea,
  .card select {
    font-size: 12px;
    text-transform: none;
  }
  .checkbox {
    display: flex !important;
    flex-direction: row !important;
    align-items: center;
    gap: 8px;
    text-transform: none !important;
    color: var(--text-primary) !important;
  }
  .xyz {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 4px;
  }
  .hint {
    margin: 0;
    font-size: 11px;
    color: var(--text-muted);
    font-style: italic;
    text-transform: none;
  }
  .raw {
    font-size: 11px;
    color: var(--text-muted);
  }
  .raw summary {
    cursor: pointer;
    padding: 2px 0;
  }
  .raw textarea {
    width: 100%;
    font-family: ui-monospace, monospace;
    font-size: 11px;
    margin-top: 4px;
  }
  .ico {
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 16px;
    line-height: 1;
    padding: 0 4px;
  }
  .ico.danger:hover {
    color: var(--danger);
  }
</style>
