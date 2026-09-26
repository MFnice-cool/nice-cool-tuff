<script lang="ts">
  import { X, RefreshCw, Copy, Code2, Link2, Unlink, Plus, Save } from "@lucide/svelte";
  import CodeMirror from "svelte-codemirror-editor";
  import { javascript } from "@codemirror/lang-javascript";
  import { editorThemeFor } from "../../lib/editorTheme";
  import { completionsForExtension } from "../../lib/editorAutocomplete";
  import { EditorView } from "@codemirror/view";
  import {
    api,
    type QuestChapter,
    type QuestData,
    type QuestKubeJsAudit,
    type QuestKubeJsBinding,
    type QuestKubeJsTemplateParams,
  } from "../../lib/api";
  import { projectPath, theme } from "../../lib/store";
  import { onMount } from "svelte";

  let {
    chapters = [],
    selectedQuest = null,
    focusId = $bindable(null as string | null),
    onclose,
    onjumpquest,
    oncreatecustom,
    ondirtyquest,
  }: {
    chapters?: QuestChapter[];
    selectedQuest?: QuestData | null;
    focusId?: string | null;
    onclose: () => void;
    onjumpquest: (questId: string, chapterId: string) => void;
    /** Create a custom task/reward on the selected quest; returns new id. */
    oncreatecustom: (kind: "task" | "reward", opts?: { title?: string; maxProgress?: number }) => string | null;
    ondirtyquest: () => void;
  } = $props();

  let audit = $state<QuestKubeJsAudit | null>(null);
  let loading = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let message = $state<string | null>(null);

  let activeScript = $state("kubejs/server_scripts/tuffbox_ftb_quests.js");
  let content = $state("");
  let original = $state("");
  let dirty = $derived(content !== original);

  let templateKind = $state("customTask");
  let blockId = $state("minecraft:stone");
  let itemId = $state("minecraft:diamond");
  let maxProgress = $state(1);
  let filterStatus = $state<"all" | "missing" | "linked" | "orphan">("all");

  let cmView = $state<EditorView | null>(null);
  let pendingJumpLine = $state<number | null>(null);

  const TEMPLATE_OPTIONS = [
    { id: "customTask", label: "Custom task (tick check)" },
    { id: "breakBlock", label: "Break / mine block" },
    { id: "customReward", label: "Custom reward" },
    { id: "completed", label: "Quest completed" },
    { id: "started", label: "Quest started" },
  ] as const;

  const bookPayload = $derived({ chapters });

  const selectedBindings = $derived.by(() => {
    if (!audit || !selectedQuest) return [] as QuestKubeJsBinding[];
    return audit.bindings.filter((b) => b.questId === selectedQuest!.id);
  });

  const visibleBindings = $derived.by(() => {
    if (!audit) return [] as QuestKubeJsBinding[];
    let list = [...audit.bindings];
    if (selectedQuest) {
      const sel = list.filter((b) => b.questId === selectedQuest!.id);
      const rest = list.filter((b) => b.questId !== selectedQuest!.id);
      list = [...sel, ...rest];
    }
    if (filterStatus === "missing" || filterStatus === "linked") {
      list = list.filter((b) => b.status === filterStatus);
    }
    if (focusId) {
      const fid = focusId.toUpperCase();
      list.sort(
        (a, b) =>
          (a.id.toUpperCase() === fid ? 0 : 1) - (b.id.toUpperCase() === fid ? 0 : 1),
      );
    }
    return list;
  });

  async function refresh() {
    if (!$projectPath) return;
    loading = true;
    error = null;
    try {
      await api.quests.kubejs.ensureManaged($projectPath);
      audit = await api.quests.kubejs.audit(bookPayload as never, $projectPath);
      if (!audit.scripts.some((s) => s.relativePath === activeScript)) {
        activeScript = audit.scripts[0]?.relativePath ?? activeScript;
      }
      await loadScript(activeScript);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function loadScript(rel: string) {
    if (!$projectPath) return;
    activeScript = rel;
    const text = await api.quests.kubejs.readScript(rel, $projectPath);
    content = text;
    original = text;
  }

  async function saveScript() {
    if (!$projectPath || !dirty) return;
    saving = true;
    error = null;
    message = null;
    try {
      await api.quests.kubejs.writeScript(activeScript, content, $projectPath);
      original = content;
      message = `Saved ${activeScript}`;
      audit = await api.quests.kubejs.audit(bookPayload as never, $projectPath);
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  function onCmReady(view: EditorView) {
    cmView = view;
    if (pendingJumpLine != null) jumpToLine(pendingJumpLine);
  }

  function jumpToLine(line: number) {
    pendingJumpLine = line;
    if (!cmView) return;
    const doc = cmView.state.doc;
    const target = Math.max(1, Math.min(line, doc.lines));
    const info = doc.line(target);
    cmView.dispatch({
      selection: { anchor: info.from, head: info.to },
      effects: EditorView.scrollIntoView(info.from, { y: "center" }),
    });
    cmView.focus();
    pendingJumpLine = null;
  }

  async function copyId(id: string) {
    try {
      await navigator.clipboard.writeText(id);
      message = `Copied ${id}`;
    } catch {
      message = id;
    }
  }

  function resolveTargetId(preferBinding?: QuestKubeJsBinding | null): string | null {
    if (preferBinding) return preferBinding.id;
    if (focusId) return focusId;
    if (selectedBindings[0]) return selectedBindings[0].id;
    return null;
  }

  async function generateFor(
    kind: string,
    binding?: QuestKubeJsBinding | null,
    createIfNeeded = false,
  ) {
    if (!$projectPath) return;
    error = null;
    message = null;
    let id = resolveTargetId(binding);
    let title: string | undefined;
    let prog = maxProgress;

    if (!id && createIfNeeded) {
      if (!selectedQuest) {
        error = "Select a quest first to create a custom task/reward.";
        return;
      }
      const isReward = kind === "customReward";
      const newId = oncreatecustom(isReward ? "reward" : "task", {
        title: isReward ? "Custom reward" : kind === "breakBlock" ? "Break block" : "Custom task",
        maxProgress: kind === "breakBlock" ? Math.max(1, maxProgress) : prog,
      });
      if (!newId) {
        error = "Could not create custom object on quest.";
        return;
      }
      id = newId;
      ondirtyquest();
      title = isReward ? "Custom reward" : "Custom task";
    }

    if (!id) {
      if (kind === "completed" || kind === "started") {
        id = selectedQuest?.id ?? null;
      }
    }
    if (!id) {
      error = "No task/reward/quest id — select a binding or quest.";
      return;
    }

    if (binding) {
      title = binding.title ?? undefined;
      if (binding.kind === "customTask") {
        const fromBook = chapters
          .flatMap((c) => c.quests)
          .find((q) => q.id === binding.questId)
          ?.tasks.find((t) => t.id === binding.id);
        const mp = fromBook?.properties?.max_progress;
        if (typeof mp === "number") prog = mp;
      }
    }

    const params: QuestKubeJsTemplateParams = {
      kind,
      id,
      maxProgress: prog,
      blockId: blockId || null,
      itemId: itemId || null,
      count: 1,
      title: title ?? null,
    };

    try {
      const snippet = await api.quests.kubejs.renderTemplate(params);
      if (activeScript !== "kubejs/server_scripts/tuffbox_ftb_quests.js") {
        await loadScript("kubejs/server_scripts/tuffbox_ftb_quests.js");
      }
      const res = await api.quests.kubejs.appendHandler(snippet, $projectPath);
      await loadScript(res.relativePath);
      audit = await api.quests.kubejs.audit(bookPayload as never, $projectPath);
      focusId = id;
      message = `Appended ${kind} handler for ${id}`;
      // Jump near end of file
      const lines = content.split("\n").length;
      jumpToLine(Math.max(1, lines - 2));
    } catch (e) {
      error = String(e);
    }
  }

  function openHandler(binding: QuestKubeJsBinding) {
    const h = binding.handlers[0];
    if (!h) {
      void generateFor(binding.kind === "customReward" ? "customReward" : "customTask", binding);
      return;
    }
    void (async () => {
      await loadScript(h.relativePath);
      focusId = binding.id;
      jumpToLine(h.line);
    })();
  }

  onMount(() => {
    return projectPath.subscribe((path) => {
      if (path) void refresh();
    });
  });
</script>

<div class="drawer drawer-kjs">
  <div class="drawer-h">
    <Code2 size={16} class="hero-ico" />
    <div class="hero">
      <strong>KubeJS · FTB Quests</strong>
      <span class="sub">Wire custom tasks & rewards to FTBQuestsEvents</span>
    </div>
    <button
      type="button"
      class="ghost ico"
      title="Refresh"
      aria-label="Refresh"
      disabled={loading}
      onclick={() => void refresh()}><RefreshCw size={14} /></button
    >
    <button type="button" class="ghost ico" aria-label="Close panel" onclick={onclose}><X size={14} /></button>
  </div>

  {#if audit}
    <div class="stats" role="status">
      <button type="button" class="stat" class:on={filterStatus === "linked"} onclick={() => (filterStatus = filterStatus === "linked" ? "all" : "linked")}
        ><Link2 size={12} /> {audit.linked} linked</button
      >
      <button type="button" class="stat miss" class:on={filterStatus === "missing"} onclick={() => (filterStatus = filterStatus === "missing" ? "all" : "missing")}
        ><Unlink size={12} /> {audit.missing} missing</button
      >
      <button type="button" class="stat" class:on={filterStatus === "orphan"} onclick={() => (filterStatus = filterStatus === "orphan" ? "all" : "orphan")}
        >{audit.orphan} orphan</button
      >
    </div>
  {/if}

  {#if error}<p class="err">{error}</p>{/if}
  {#if message}<p class="ok">{message}</p>{/if}

  <div class="split">
    <div class="col list">
      <div class="col-h">
        <span>Bindings</span>
        {#if selectedQuest}
          <span class="pill">quest: {selectedQuest.title || selectedQuest.id}</span>
        {/if}
      </div>

      {#if filterStatus === "orphan" && audit}
        {#each audit.orphanHandlers as h (h.kind + h.id + h.relativePath)}
          <div class="card orphan">
            <div class="card-t">
              <code>{h.id}</code>
              <span class="badge">{h.kind}</span>
            </div>
            <p class="meta">{h.relativePath}:{h.line}</p>
            <div class="row">
              <button type="button" class="mini" onclick={() => void copyId(h.id)}><Copy size={11} /></button>
              <button
                type="button"
                class="mini"
                onclick={() => {
                  void loadScript(h.relativePath).then(() => jumpToLine(h.line));
                }}>Open</button
              >
            </div>
          </div>
        {:else}
          <p class="empty">No orphan handlers.</p>
        {/each}
      {:else}
        {#each visibleBindings as b (b.kind + b.id)}
          <div
            class="card"
            class:missing={b.status === "missing"}
            class:focus={focusId && b.id.toUpperCase() === focusId.toUpperCase()}
          >
            <div class="card-t">
              <code>{b.id}</code>
              <span class="badge" class:bad={b.status === "missing"}>{b.status}</span>
            </div>
            <p class="meta">
              {b.kind} · {b.title || "untitled"} · {b.questTitle}
            </p>
            <div class="row">
              <button type="button" class="mini" onclick={() => void copyId(b.id)} title="Copy ID"
                ><Copy size={11} /></button
              >
              <button type="button" class="mini" onclick={() => onjumpquest(b.questId, b.chapterId)}
                >Jump</button
              >
              <button type="button" class="mini" onclick={() => openHandler(b)}
                >{b.status === "missing" ? "Generate" : "Open"}</button
              >
            </div>
          </div>
        {:else}
          <p class="empty">
            {loading ? "Loading…" : "No custom tasks/rewards in the book yet."}
          </p>
        {/each}
      {/if}

      <div class="templates">
        <div class="col-h"><span>Generate</span></div>
        <label
          >Template
          <select bind:value={templateKind}>
            {#each TEMPLATE_OPTIONS as t (t.id)}
              <option value={t.id}>{t.label}</option>
            {/each}
          </select>
        </label>
        {#if templateKind === "breakBlock"}
          <label
            >Block id<input bind:value={blockId} placeholder="minecraft:iron_ore" /></label
          >
          <label
            >Count (max progress)<input type="number" min="1" bind:value={maxProgress} /></label
          >
        {:else if templateKind === "customTask"}
          <label
            >Max progress<input type="number" min="1" bind:value={maxProgress} /></label
          >
        {:else if templateKind === "customReward"}
          <label
            >Item reward<input bind:value={itemId} placeholder="minecraft:diamond" /></label
          >
        {/if}
        <div class="row gen">
          <button
            type="button"
            class="primary"
            onclick={() => void generateFor(templateKind, null, false)}
            >Append for selection</button
          >
          <button
            type="button"
            class="ghost"
            title="Create custom object on selected quest and wire script"
            onclick={() => void generateFor(templateKind, null, true)}
            ><Plus size={12} /> Add + wire</button
          >
        </div>
        <p class="hint">
          After save: <code>/reload</code> then <code>/ftbquests reload</code> in-game.
        </p>
      </div>
    </div>

    <div class="col editor">
      <div class="col-h">
        <select
          class="script-sel"
          value={activeScript}
          onchange={(e) => void loadScript((e.currentTarget as HTMLSelectElement).value)}
        >
          {#each audit?.scripts ?? [] as s (s.relativePath)}
            <option value={s.relativePath}
              >{s.managed ? "★ " : ""}{s.name}</option
            >
          {/each}
        </select>
        <button type="button" class="ghost" disabled={!dirty || saving} onclick={() => void saveScript()}
          ><Save size={12} /> Save{#if dirty}<span class="dot">●</span>{/if}</button
        >
      </div>
      <div class="cm-wrap">
        <CodeMirror
          bind:value={content}
          lang={javascript()}
          theme={editorThemeFor($theme)}
          extensions={completionsForExtension("kubejs")}
          styles={{ ".cm-editor": { height: "100%", "font-size": "12px" } }}
          on:ready={(e) => onCmReady(e.detail)}
        />
      </div>
    </div>
  </div>
</div>

<style>
  .drawer-kjs {
    position: relative;
    top: auto;
    right: auto;
    z-index: auto;
    width: 100%;
    max-height: min(80vh, 820px);
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0;
    background: transparent;
    border: none;
    border-radius: 0;
    box-shadow: none;
    overflow: hidden;
  }
  .drawer-h {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--ftbq-border, var(--ftbq-frame));
    background: linear-gradient(180deg, rgba(61, 184, 168, 0.12), transparent);
  }
  .hero {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .hero strong {
    font-size: 13px;
    letter-spacing: 0.02em;
  }
  .sub {
    font-size: 12px;
    color: var(--ftbq-text-muted);
  }
  :global(.hero-ico) {
    color: var(--ftbq-accent-teal);
    flex-shrink: 0;
  }
  .stats {
    display: flex;
    gap: 8px;
    padding: 8px 14px;
    border-bottom: 1px solid var(--ftbq-border, var(--ftbq-frame));
  }
  .stat {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    padding: 4px 8px;
    border-radius: 2px;
    border: 1px solid var(--ftbq-border, var(--ftbq-frame));
    background: rgba(0, 0, 0, 0.2);
    color: var(--ftbq-text-muted);
    cursor: pointer;
  }
  .stat.on {
    color: var(--ftbq-accent-teal);
    border-color: rgba(61, 184, 168, 0.45);
  }
  .stat.miss.on {
    color: var(--accent-danger);
    border-color: color-mix(in srgb, var(--accent-danger) 45%, transparent);
  }
  .err,
  .ok {
    margin: 0;
    padding: 6px 14px;
    font-size: 12px;
  }
  .err {
    color: var(--accent-danger);
    background: color-mix(in srgb, var(--accent-danger) 10%, transparent);
  }
  .ok {
    color: var(--ftbq-accent-teal);
  }
  .split {
    display: grid;
    grid-template-columns: minmax(260px, 320px) 1fr;
    min-height: 0;
    flex: 1;
    overflow: hidden;
  }
  .col {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: auto;
  }
  .col.list {
    border-right: 1px solid var(--ftbq-border, var(--ftbq-frame));
    padding: 8px 10px 12px;
    gap: 8px;
  }
  .col.editor {
    padding: 8px 10px 10px;
    gap: 8px;
  }
  .col-h {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--ftbq-accent-teal);
    font-weight: 700;
  }
  .pill {
    display: inline-block;
    line-height: 1.4;
    vertical-align: baseline;
    text-transform: none;
    letter-spacing: 0;
    font-weight: 500;
    font-size: 12px;
    padding: 2px 6px;
    border-radius: 2px;
    background: rgba(61, 184, 168, 0.12);
    color: var(--ftbq-text-muted);
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .card {
    display: grid;
    gap: 8px;
    padding: 8px;
    border: 1px solid var(--ftbq-border, var(--ftbq-frame));
    border-radius: 2px;
    background: rgba(0, 0, 0, 0.18);
  }
  .card.missing {
    border-color: rgba(248, 113, 113, 0.35);
  }
  .card.focus {
    box-shadow: inset 0 0 0 1px rgba(61, 184, 168, 0.55);
  }
  /* Dim the CONTENT, not the glass card itself: `.card` is a backdrop-filter
     surface (glass toggle / quiet-glass themes), and opacity on the card
     element re-groups its backdrop layer — the WebView2 square-artifact
     class (see scripts/check-glass-compositing.mjs). */
  .card.orphan :is(.card-t, .meta, .row) {
    opacity: 0.9;
  }
  .card-t {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .card-t code {
    font-size: 12px;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
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
    text-transform: uppercase;
    padding: 1px 5px;
    border-radius: 2px;
    background: rgba(61, 184, 168, 0.15);
    color: var(--ftbq-accent-teal);
  }
  .badge.bad {
    background: color-mix(in srgb, var(--accent-danger) 15%, transparent);
    color: var(--accent-danger);
  }
  .meta {
    margin: 0;
    font-size: 12px;
    color: var(--ftbq-text-muted);
  }
  .row {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .row.gen {
    margin-top: 4px;
  }
  .mini,
  .ghost,
  .primary {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    padding: 4px 8px;
    border-radius: 2px;
    border: 1px solid var(--ftbq-border, var(--ftbq-frame));
    background: transparent;
    color: inherit;
    cursor: pointer;
  }
  .primary {
    background: rgba(61, 184, 168, 0.2);
    border-color: rgba(61, 184, 168, 0.45);
    color: var(--ftbq-accent-teal);
  }
  .ico {
    width: 28px;
    height: 28px;
    justify-content: center;
    padding: 0;
  }
  .templates {
    margin-top: 8px;
    padding-top: 8px;
    border-top: 1px solid var(--ftbq-border, var(--ftbq-frame));
    display: grid;
    gap: 8px;
  }
  .templates label {
    display: grid;
    gap: 8px;
    font-size: 12px;
    text-transform: uppercase;
    color: var(--ftbq-text-muted);
  }
  .templates input,
  .templates select,
  .script-sel {
    font-size: 12px;
    text-transform: none;
    background: var(--ftbq-bg, #1a1a1e);
    border: 1px solid var(--ftbq-border, var(--ftbq-frame));
    color: inherit;
    border-radius: 2px;
    padding: 5px 7px;
  }
  .script-sel {
    flex: 1;
    min-width: 0;
  }
  .hint {
    margin: 0;
    font-size: 12px;
    color: var(--ftbq-text-muted);
    text-transform: none;
  }
  .hint code {
    font-size: 12px;
  }
  .empty {
    margin: 8px 0;
    font-size: 12px;
    color: var(--ftbq-text-muted);
  }
  .cm-wrap {
    flex: 1;
    min-height: 280px;
    border: 1px solid var(--ftbq-border, var(--ftbq-frame));
    border-radius: 2px;
    overflow: hidden;
  }
  .cm-wrap :global(.cm-editor) {
    height: 100%;
    min-height: 280px;
  }
  .dot {
    color: var(--ftbq-quest-started);
    margin-left: 2px;
  }
</style>
