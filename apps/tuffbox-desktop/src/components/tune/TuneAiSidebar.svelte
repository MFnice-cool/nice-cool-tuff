<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import {
    Sparkles,
    PanelRightClose,
    Send,
    Loader2,
    MessageSquareText,
    Trash2,
    ExternalLink,
    Plus,
  } from "@lucide/svelte";
  import { api, type TuneChatSession } from "../../lib/api";
  import { projectPath, tuneChatFocusId } from "../../lib/store";
  import TunePlanReview from "./TunePlanReview.svelte";

  type GoalId =
    | "fps_client"
    | "server_stability"
    | "compat_safe"
    | "explain_file"
    | "fill_unknowns"
    | "free_text";

  type ChatMsg = { role: "user" | "assistant" | "system"; text: string };

  type ResearchEntry = { step: string; detail: string; ok: boolean; url?: string | null };

  type AdviseResult = {
    plan: Record<string, unknown>;
    explanation: string;
    researchLog: ResearchEntry[];
    unknownKeys: { path: string; key: string; modHint?: string | null }[];
    diffs: {
      path: string;
      patchType: string;
      beforeExcerpt: string;
      afterExcerpt: string;
      ok: boolean;
      error?: string | null;
    }[];
    validationOk: boolean;
    validationErrors: string[];
    validationWarnings: string[];
  };

  type ReviewRow = {
    key: string;
    selected: boolean;
    op: string;
    path: string | null;
    patchPreview: string | null;
    diffBefore: string | null;
    diffAfter: string | null;
    reason: string;
    risk: string;
    raw: any;
  };

  let {
    open = true,
    focusPath = null,
    onclose,
    onapplied,
  }: {
    open?: boolean;
    focusPath?: string | null;
    onclose?: () => void;
    onapplied?: (paths: string[]) => void;
  } = $props();

  const GOALS: { id: GoalId; label: string }[] = [
    { id: "fps_client", label: "FPS" },
    { id: "server_stability", label: "Server" },
    { id: "compat_safe", label: "Compat" },
    { id: "explain_file", label: "Explain file" },
    { id: "fill_unknowns", label: "Fill unknowns" },
  ];

  let sessions = $state<TuneChatSession[]>([]);
  let activeId = $state<string | null>(null);
  let messages = $state<ChatMsg[]>([]);
  let input = $state("");
  let busy = $state(false);
  let error = $state("");
  let progressLog = $state<string[]>([]);
  let pending = $state<AdviseResult | null>(null);
  let reviewOpen = $state(false);
  let reviewRows = $state<ReviewRow[]>([]);
  let acknowledged = $state(false);
  let applyBusy = $state(false);
  let showResearch = $state(false);
  let unlisten: UnlistenFn | undefined;
  let unsubPath: (() => void) | undefined;
  let unsubFocus: (() => void) | undefined;

  const canApply = $derived(
    reviewRows.some((r) => r.selected) &&
      (!(pending?.plan as any)?.needsUserReview || acknowledged) &&
      !applyBusy,
  );
  const selectedCount = $derived(reviewRows.filter((r) => r.selected).length);
  const needsAck = $derived(!!pending?.plan && (pending.plan as any).needsUserReview !== false);
  const researchCitations = $derived(
    (pending?.researchLog ?? []).filter((e) => e.ok && e.url && !e.url.includes("duckduckgo")),
  );

  function pendingFromSession(s: TuneChatSession): AdviseResult | null {
    const p = s.pendingAdvise;
    if (!p?.plan) return null;
    return {
      plan: p.plan,
      explanation: p.explanation ?? "",
      researchLog: (p.researchLog as ResearchEntry[]) ?? [],
      unknownKeys: (p.unknownKeys as AdviseResult["unknownKeys"]) ?? [],
      diffs: (p.diffs as AdviseResult["diffs"]) ?? [],
      validationOk: p.validationOk !== false,
      validationErrors: p.validationErrors ?? [],
      validationWarnings: p.validationWarnings ?? [],
    };
  }

  async function refreshList() {
    if (!$projectPath) {
      sessions = [];
      return;
    }
    try {
      const res = await api.config.listChats($projectPath);
      sessions = res.sessions ?? [];
      if (activeId && !sessions.some((s) => s.id === activeId)) {
        activeId = sessions[0]?.id ?? null;
        if (activeId) await selectSession(activeId);
        else {
          messages = [];
          pending = null;
        }
      }
    } catch {
      sessions = [];
    }
  }

  async function selectSession(id: string) {
    if (!$projectPath) return;
    activeId = id;
    error = "";
    progressLog = [];
    try {
      const s = await api.config.loadChat(id, $projectPath);
      messages = (s.messages ?? []).map((m) => ({
        role: (m.role as ChatMsg["role"]) || "assistant",
        text: m.content,
      }));
      pending = pendingFromSession(s);
      showResearch = (pending?.researchLog?.length ?? 0) > 0;
    } catch (e: any) {
      error = String(e?.message ?? e);
    }
  }

  async function newSession(title?: string) {
    if (!$projectPath) return;
    try {
      const s = await api.config.newChat(title ?? "Tune configs", $projectPath);
      await refreshList();
      await selectSession(s.id);
    } catch (e: any) {
      error = String(e?.message ?? e);
    }
  }

  async function deleteActive() {
    if (!$projectPath || !activeId || busy) return;
    if (!confirm("Delete this Tune AI chat?")) return;
    try {
      await api.config.deleteChat(activeId, $projectPath);
      activeId = null;
      messages = [];
      pending = null;
      await refreshList();
      if (sessions[0]) await selectSession(sessions[0].id);
    } catch (e: any) {
      error = String(e?.message ?? e);
    }
  }

  onMount(async () => {
    unlisten = await listen<{ sessionId: string; line: string; phase: string }>(
      "tune-ai-progress",
      (ev) => {
        if (ev.payload?.sessionId && activeId && ev.payload.sessionId !== activeId) return;
        if (ev.payload?.line) {
          progressLog = [...progressLog, ev.payload.line].slice(-40);
        }
      },
    );
    if ($projectPath) await refreshList();
    unsubPath = projectPath.subscribe((p) => {
      if (p) void refreshList();
      else {
        sessions = [];
        activeId = null;
        messages = [];
        pending = null;
      }
    });
    unsubFocus = tuneChatFocusId.subscribe((id) => {
      if (!id || !$projectPath || !open) return;
      tuneChatFocusId.set(null);
      void (async () => {
        await refreshList();
        await selectSession(id);
      })();
    });
  });

  onDestroy(() => {
    unlisten?.();
    unsubPath?.();
    unsubFocus?.();
  });

  async function runAdvise(goal: GoalId, userMessage: string) {
    if (!$projectPath || busy) return;
    if (!activeId) {
      await newSession(GOALS.find((g) => g.id === goal)?.label ?? "Tune configs");
      if (!activeId) return;
    }
    busy = true;
    error = "";
    progressLog = [];
    pending = null;
    const text = userMessage.trim();
    try {
      const res = await api.config.chatTurn(text, {
        chatId: activeId,
        goal,
        focusPath: focusPath ?? null,
      }, $projectPath);
      activeId = res.session.id;
      messages = (res.session.messages ?? []).map((m) => ({
        role: (m.role as ChatMsg["role"]) || "assistant",
        text: m.content,
      }));
      pending = {
        plan: res.advise.plan,
        explanation: res.advise.explanation,
        researchLog: res.advise.researchLog ?? [],
        unknownKeys: res.advise.unknownKeys ?? [],
        diffs: res.advise.diffs ?? [],
        validationOk: res.advise.validationOk,
        validationErrors: res.advise.validationErrors ?? [],
        validationWarnings: res.advise.validationWarnings ?? [],
      };
      showResearch = (pending.researchLog?.length ?? 0) > 0;
      if (!res.advise.validationOk) {
        error = res.advise.validationErrors.join("; ") || "Plan validation failed";
      }
      await refreshList();
    } catch (e: any) {
      error = String(e?.message ?? e);
      messages = [...messages, { role: "system", text: error }];
    } finally {
      busy = false;
    }
  }

  function openReview() {
    if (!pending?.plan) return;
    const actions = ((pending.plan as any).actions ?? []) as any[];
    const diffs = pending.diffs ?? [];
    reviewRows = actions.map((a, i) => {
      const path = a.path ?? null;
      const diff = diffs.find((d) => d.path === path);
      return {
        key: `${i}-${path ?? "x"}`,
        selected: true,
        op: a.op ?? "edit_config",
        path,
        patchPreview: a.patch ? JSON.stringify(a.patch, null, 2) : null,
        diffBefore: diff?.beforeExcerpt ?? null,
        diffAfter: diff?.afterExcerpt ?? null,
        reason: a.reason ?? "",
        risk: a.risk ?? "medium",
        raw: a,
      };
    });
    acknowledged = false;
    reviewOpen = true;
  }

  async function confirmApply() {
    if (!$projectPath || !pending?.plan || !canApply) return;
    applyBusy = true;
    error = "";
    try {
      const selected = reviewRows.filter((r) => r.selected).map((r) => r.raw);
      const plan = {
        ...(pending.plan as any),
        actions: selected,
        needsUserReview: false,
      };
      const result = await api.diagnostics.applyActionPlan(plan, $projectPath);
      const paths = selected
        .map((a) => a.path as string | undefined)
        .filter((p): p is string => !!p);
      messages = [
        ...messages,
        {
          role: "system",
          text: `Applied ${selected.length} patch(es). Snapshot: ${(result as any)?.snapshotId ?? "ok"}`,
        },
      ];
      reviewOpen = false;
      pending = null;
      if (activeId) {
        try {
          const s = await api.config.loadChat(activeId, $projectPath);
          s.pendingAdvise = null;
          s.messages = messages.map((m) => ({
            role: m.role,
            content: m.text,
            createdAt: new Date().toISOString(),
          }));
          s.updatedAt = new Date().toISOString();
          await api.config.saveChat(s, $projectPath);
          await refreshList();
        } catch {
          /* ignore */
        }
      }
      onapplied?.(paths);
    } catch (e: any) {
      error = String(e?.message ?? e);
    } finally {
      applyBusy = false;
    }
  }

  function send() {
    const text = input.trim();
    if (!text || busy) return;
    input = "";
    void runAdvise("free_text", text);
  }
</script>

{#if open}
  <aside class="tune-ai">
    <header class="ai-head">
      <div class="ai-title">
        <Sparkles size={16} />
        <strong>Config AI</strong>
      </div>
      <div class="ai-head-actions">
        <button type="button" class="ghost icon" title="New chat" disabled={busy} onclick={() => newSession()} aria-label="New chat">
          <Plus size={14} />
        </button>
        <button
          type="button"
          class="ghost icon"
          title="Delete chat"
          disabled={busy || !activeId}
          onclick={deleteActive}
          aria-label="Delete chat"
        >
          <Trash2 size={14} />
        </button>
        <button type="button" class="ghost icon" onclick={() => onclose?.()} aria-label="Close AI">
          <PanelRightClose size={16} />
        </button>
      </div>
    </header>

    {#if sessions.length > 0}
      <div class="session-pills">
        {#each sessions.slice(0, 8) as s (s.id)}
          <button
            type="button"
            class="pill"
            class:active={s.id === activeId}
            disabled={busy}
            onclick={() => selectSession(s.id)}
            title={s.title}
          >
            {(s.title || "Tune").slice(0, 18)}
          </button>
        {/each}
      </div>
    {/if}

    <div class="chips">
      {#each GOALS as g (g.id)}
        <button
          type="button"
          class="chip"
          disabled={busy || !$projectPath}
          onclick={() => runAdvise(g.id, "")}
        >
          {g.label}
        </button>
      {/each}
    </div>

    {#if focusPath}
      <p class="focus-hint">Focus: <code>{focusPath}</code></p>
    {/if}

    <div class="thread">
      {#if messages.length === 0}
        <p class="muted">
          Ask Tune to set safe config values. Chats sync to the sidebar Chats list.
          Review before apply.
        </p>
      {/if}
      {#each messages as m, i (i + m.role + m.text.slice(0, 24))}
        <div class="bubble" class:user={m.role === "user"} class:assistant={m.role === "assistant"} class:system={m.role === "system"}>
          {m.text}
        </div>
      {/each}
      {#if progressLog.length > 0}
        <div class="progress">
          {#each progressLog as line, i (i + line)}
            <div>{line}</div>
          {/each}
        </div>
      {/if}
    </div>

    {#if error}
      <p class="err">{error}</p>
    {/if}

    {#if pending && researchCitations.length > 0}
      <details class="research" bind:open={showResearch}>
        <summary>Sources ({researchCitations.length})</summary>
        <ul>
          {#each researchCitations as e, i (i + (e.url ?? e.detail))}
            <li>
              <span class="step">{e.step}</span>
              {#if e.url}
                <a href={e.url} target="_blank" rel="noreferrer">
                  {e.url.replace(/^https?:\/\//, "").slice(0, 48)}{#if e.url.length > 56}…{/if}
                  <ExternalLink size={10} />
                </a>
              {:else}
                <span>{e.detail}</span>
              {/if}
            </li>
          {/each}
        </ul>
      </details>
    {/if}

    {#if pending}
      <div class="pending">
        <MessageSquareText size={14} />
        <span>{((pending.plan as any)?.actions ?? []).length} patch(es)</span>
        <button type="button" class="primary mini" disabled={busy} onclick={openReview}>
          Review
        </button>
      </div>
    {/if}

    <div class="composer">
      <textarea
        bind:value={input}
        rows="2"
        placeholder="e.g. lower render distance, disable sodium memory tracing…"
        disabled={busy || !$projectPath}
        onkeydown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            send();
          }
        }}
      ></textarea>
      <button type="button" class="primary send" disabled={busy || !input.trim()} onclick={send}>
        {#if busy}
          <Loader2 size={16} class="spin" />
        {:else}
          <Send size={16} />
        {/if}
      </button>
    </div>
  </aside>
{/if}

<TunePlanReview
  bind:open={reviewOpen}
  explanation={pending?.explanation ?? ""}
  bind:rows={reviewRows}
  needsAck={needsAck}
  bind:acknowledged
  canApply={canApply}
  selectedCount={selectedCount}
  busy={applyBusy}
  onCancel={() => (reviewOpen = false)}
  onConfirm={confirmApply}
/>

<style>
  .tune-ai {
    width: 100%;
    min-width: 0;
    max-width: none;
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--border-color);
    background: var(--bg-secondary);
    padding: 8px;
    gap: 8px;
    box-sizing: border-box;
  }
  .ai-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex-shrink: 0;
  }
  .ai-head-actions { display: flex; gap: 8px; align-items: center; }
  .ai-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .session-pills {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    flex-shrink: 0;
  }
  .pill {
    font-size: 12px;
    padding: 2px 7px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-muted);
    cursor: pointer;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pill.active {
    color: var(--accent-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 45%, transparent);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    flex-shrink: 0;
  }
  .chip {
    font-size: 12px;
    font-weight: 700;
    padding: 3px 8px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-muted);
    cursor: pointer;
  }
  .chip:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--accent-primary);
  }
  .chip:disabled { opacity: 0.5; cursor: default; }
  .focus-hint {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
    word-break: break-all;
  }
  .focus-hint code { color: var(--accent-primary); }
  .thread {
    flex: 1;
    min-height: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .muted { margin: 0; font-size: 12px; color: var(--text-muted); }
  .bubble {
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
    border: 1px solid var(--border-color);
  }
  .bubble.user {
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    align-self: flex-end;
  }
  .bubble.assistant { background: var(--bg-tertiary); }
  .bubble.system { background: transparent; color: var(--text-muted); border-style: dashed; }
  .progress {
    font-size: 12px;
    color: var(--text-muted);
    display: grid;
    gap: 8px;
  }
  .err { margin: 0; font-size: 12px; color: var(--accent-danger); }
  .research {
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-muted);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
    padding: 6px 8px;
    background: var(--bg-tertiary);
  }
  .research summary { cursor: pointer; font-weight: 600; color: var(--text-secondary); }
  .research ul {
    margin: 6px 0 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 8px;
  }
  .research li { display: flex; flex-direction: column; gap: 8px; }
  .research .step { font-size: 12px; text-transform: uppercase; letter-spacing: 0.03em; opacity: 0.8; }
  .research a {
    color: var(--accent-primary);
    text-decoration: none;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    word-break: break-all;
  }
  .research a:hover { text-decoration: underline; }
  .pending {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    font-size: 12px;
    flex-shrink: 0;
  }
  .pending .mini { margin-left: auto; padding: 4px 10px; font-size: 12px; }
  .composer {
    display: flex;
    gap: 8px;
    flex-shrink: 0;
    align-items: flex-end;
  }
  .composer textarea {
    flex: 1;
    resize: none;
    font: inherit;
    font-size: 12px;
    padding: 8px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
    color: var(--text-primary);
  }
  .composer .send {
    padding: 8px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  :global(.spin) { animation: spin 0.9s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .ghost.icon {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
  }
  .ghost.icon:disabled { opacity: 0.4; cursor: default; }
</style>
