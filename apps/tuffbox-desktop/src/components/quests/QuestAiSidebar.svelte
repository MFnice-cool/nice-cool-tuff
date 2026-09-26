<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import {
    Sparkles,
    Plus,
    Trash2,
    Send,
    Loader2,
    PanelRightClose,
    GitBranch,
    MessageSquareText,
    AlertCircle,
    Info,
    Code,
  } from "@lucide/svelte";
  import {
    api,
    type QuestChatSession,
    type QuestData,
    type QuestPlanMergeResult,
    type AiTokenUsage,
  } from "../../lib/api";
  import { projectPath, questChatFocusId } from "../../lib/store";
  import QuestPlanReview from "./QuestPlanReview.svelte";
  import ConfirmDialog from "../ConfirmDialog.svelte";
  import { invoke } from "@tauri-apps/api/core";

  type QuestAiProgressPayload = {
    chatId: string;
    line: string;
    phase: string;
    i?: number;
    n?: number;
  };

  type QuestAiTokenPayload = {
    chatId: string;
    text: string;
    phase: string;
  };

  let {
    open = true,
    onclose,
    onapply,
    onsavechapter,
    anchorQuest = null,
    anchorChapterTitle = null,
    targetChapterId = null,
  }: {
    open?: boolean;
    onclose?: () => void;
    onapply?: (result: QuestPlanMergeResult) => void;
    /** Persist the anchor quest's chapter to disk before a branch turn. */
    onsavechapter?: (
      chapterId: string,
    ) => Promise<"saved" | "notdirty" | "cancelled" | "error">;
    anchorQuest?: QuestData | null;
    anchorChapterTitle?: string | null;
    /** Current editor chapter — generate/extend upsert target. */
    targetChapterId?: string | null;
  } = $props();

  let sessions = $state<QuestChatSession[]>([]);
  let activeId = $state<string | null>(null);
  let session = $state<QuestChatSession | null>(null);
  let input = $state("");
  let showJson = $state(false);
  let rawJson = $state("");
  let forceAi = $state(false);
  let busy = $state(false);
  const AI_ADV_PREFS_KEY = "tuffbox.quests.aiAdvanced";

  function loadAiAdvPrefs() {
    try {
      const raw = localStorage.getItem(AI_ADV_PREFS_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as { forceAi?: boolean; pendingIntent?: string };
      if (typeof parsed.forceAi === "boolean") forceAi = parsed.forceAi;
      if (
        parsed.pendingIntent === "generate" ||
        parsed.pendingIntent === "extend" ||
        parsed.pendingIntent === "lore" ||
        parsed.pendingIntent === "branch"
      ) {
        pendingIntent = parsed.pendingIntent;
      }
    } catch {
      /* ignore */
    }
  }

  function saveAiAdvPrefs() {
    try {
      localStorage.setItem(
        AI_ADV_PREFS_KEY,
        JSON.stringify({ forceAi, pendingIntent }),
      );
    } catch {
      /* ignore */
    }
  }
  let error = $state("");
  let aiReadyHint = $state("");
  let merge = $state<QuestPlanMergeResult | null>(null);
  let progressLog = $state<string[]>([]);
  let streamDraft = $state("");
  let loreWarning = $state("");
  /** Active intent for the next send: "generate" | "extend" | "lore" | "branch". */
  let pendingIntent = $state<"generate" | "extend" | "lore" | "branch">("generate");
  let discardConfirmOpen = $state(false);
  let deleteConfirmOpen = $state(false);
  let deleteTarget = $state<{ id: string; title: string } | null>(null);
  let composerHint = $state("");
  let transcriptEl = $state<HTMLDivElement | null>(null);
  let composerEl = $state<HTMLTextAreaElement | null>(null);
  let wasOpen = $state(false);
  let unlistenProgress: UnlistenFn | undefined;
  let unlistenTokens: UnlistenFn | undefined;
  let aiPrefsReady = $state(false);
  type ExampleChip = { label: string; text: string };

  function uiLang(): "ru" | "en" {
    const t = (typeof navigator !== "undefined" ? navigator.language : "en").toLowerCase();
    return t.startsWith("ru") ? "ru" : "en";
  }

  const EXAMPLE_CHIPS_BY_LANG: Record<"en" | "ru", ExampleChip[]> = {
    en: [
      {
        label: "24-quest progression",
        text: "24-quest progression line: early game → nether, with descriptions and item rewards",
      },
      {
        label: "3 chapters",
        text: "3 chapters: early / mid / late progression with about 18 quests total",
      },
      {
        label: "Create mod starter",
        text: "16-quest chapter for Create mod early kinetic progression with lore and rewards",
      },
      {
        label: "Magic & Alchemy",
        text: "12-quest branch focusing on brewing, potions, and enchanted gear",
      },
    ],
    ru: [
      {
        label: "Линейка 24 квеста",
        text: "линейка на 24 квеста: early game → nether, с описаниями и наградами",
      },
      {
        label: "3 главы",
        text: "3 главы: early / mid / late прогрессия, около 18 квестов всего",
      },
      {
        label: "Create starter",
        text: "глава Create early game на 16 квестов с кинетикой, лором и наградами",
      },
      {
        label: "Магия и зелья",
        text: "линейка на 12 квестов: зельеварение, зачарования и алхимия",
      },
    ],
  };

  let exampleChips = $derived(EXAMPLE_CHIPS_BY_LANG[uiLang()]);
  let lastUsage = $state<AiTokenUsage | null>(null);

  function formatTokens(n: number | null | undefined): string {
    if (n == null || !Number.isFinite(n)) return "—";
    if (n >= 1000) return `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k`;
    return String(n);
  }

  function formatUsage(u: AiTokenUsage | null | undefined): string | null {
    if (!u) return null;
    const pin = u.promptTokens;
    const cout = u.completionTokens;
    const tot = u.totalTokens ?? (pin != null && cout != null ? pin + cout : null);
    if (pin == null && cout == null && tot == null) return null;
    const parts: string[] = [];
    if (pin != null) parts.push(`${formatTokens(pin)} in`);
    if (cout != null) parts.push(`${formatTokens(cout)} out`);
    if (tot != null && (pin == null || cout == null)) parts.push(`${formatTokens(tot)} tot`);
    let costHint = "";
    if (pin != null || cout != null) {
      const usd = ((pin ?? 0) * 0.15 + (cout ?? 0) * 0.6) / 1_000_000;
      if (usd >= 0.0001) costHint = ` · ~$${usd < 0.01 ? usd.toFixed(4) : usd.toFixed(3)}`;
    }
    return parts.join(" · ") + costHint;
  }

  let sessionUsageLabel = $derived.by(() => {
    const msgs = session?.messages ?? [];
    const acc: AiTokenUsage = { promptTokens: 0, completionTokens: 0, totalTokens: 0 };
    let any = false;
    for (const m of msgs) {
      const u = m.usage;
      if (!u) continue;
      any = true;
      if (u.promptTokens != null) acc.promptTokens = (acc.promptTokens ?? 0) + u.promptTokens;
      if (u.completionTokens != null)
        acc.completionTokens = (acc.completionTokens ?? 0) + u.completionTokens;
      if (u.totalTokens != null) acc.totalTokens = (acc.totalTokens ?? 0) + u.totalTokens;
    }
    return any ? formatUsage(acc) : null;
  });

  function useChip(text: string) {
    input = text;
    showJson = false;
  }

  function setIntent(i: "generate" | "extend" | "lore" | "branch") {
    pendingIntent = i;
  }

  const INTENT_ORDER = ["generate", "branch", "extend", "lore"] as const;

  function intentEnabled(i: (typeof INTENT_ORDER)[number]): boolean {
    if (i === "branch") return !!anchorQuest;
    if (i === "extend" || i === "lore") return !!session?.pendingPlan;
    return true;
  }

  function onIntentKeydown(e: KeyboardEvent) {
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(e.key)) {
      return;
    }
    e.preventDefault();
    const enabled = INTENT_ORDER.filter(intentEnabled);
    if (enabled.length === 0) return;
    const idx = Math.max(0, enabled.indexOf(pendingIntent as (typeof INTENT_ORDER)[number]));
    let next = idx;
    if (e.key === "Home") next = 0;
    else if (e.key === "End") next = enabled.length - 1;
    else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
      next = (idx - 1 + enabled.length) % enabled.length;
    } else {
      next = (idx + 1) % enabled.length;
    }
    setIntent(enabled[next]!);
  }

  function appendProgressLine(line: string) {
    const last = progressLog[progressLog.length - 1];
    if (last === line) return;
    progressLog = [...progressLog, line];
  }

  /** Format raw Rust / Serde / schema errors into friendly user-facing messages */
  function friendlyErrorMessage(rawMsg: string): string {
    const msg = rawMsg.toLowerCase();
    if (msg.includes("missing field `title`") || msg.includes("missing field `quests`") || msg.includes("invalid type")) {
      return "AI generated output with missing or unexpected fields. Try simplifying your prompt or asking for fewer quests.";
    }
    if (msg.includes("connection refused") || msg.includes("failed to connect") || msg.includes("11434")) {
      return "Unable to connect to local Ollama. Please check if Ollama is running or configure Settings → AI.";
    }
    if (msg.includes("rate limit") || msg.includes("429") || msg.includes("quota")) {
      return "AI provider rate limit reached. Please wait a moment before sending another prompt.";
    }
    if (msg.includes("context length") || msg.includes("maximum context") || msg.includes("tokens")) {
      return "The prompt and quest plan exceeded the AI context window. Try splitting into smaller chapters.";
    }
    if (msg.includes("json error") || msg.includes("syntax error") || msg.includes("trailing comma")) {
      return "AI output contained invalid JSON syntax. Please retry or adjust prompt specificity.";
    }
    return rawMsg.replace(/^Error:\s*/i, "");
  }

  /** When an anchor quest is selected, default the next send to "branch". */
  $effect(() => {
    if (anchorQuest && pendingIntent === "generate") {
      pendingIntent = "branch";
    }
    if (!anchorQuest && pendingIntent === "branch") {
      pendingIntent = "generate";
    }
  });

  $effect(() => {
    const _msgs = session?.messages?.length ?? 0;
    const _stream = streamDraft.length;
    const _prog = progressLog.length;
    void _msgs;
    void _stream;
    void _prog;
    if (!transcriptEl) return;
    queueMicrotask(() => {
      transcriptEl?.scrollTo({ top: transcriptEl.scrollHeight, behavior: "smooth" });
    });
  });

  async function refreshList() {
    if (!$projectPath) return;
    try {
      sessions = (await api.quests.listChats($projectPath)).sessions;
    } catch (e) {
      sessions = [];
      error = friendlyErrorMessage(String(e));
    }
  }

  async function selectSession(id: string) {
    if (!$projectPath) return;
    activeId = id;
    try {
      session = await api.quests.loadChat(id, $projectPath);
      merge = null;
      progressLog = [];
      streamDraft = "";
      error = "";
      composerHint = "";
      if (session.pendingPlan) {
        merge = await api.quests.filterAndMergePlan(
          session.pendingPlan,
          [],
          [],
          $projectPath,
        );
      }
    } catch (e) {
      error = friendlyErrorMessage(String(e));
    }
  }

  async function newSession() {
    if (!$projectPath) return;
    try {
      session = await api.quests.newChat("Quest line", $projectPath);
      activeId = session.id;
      merge = null;
      progressLog = [];
      streamDraft = "";
      await refreshList();
    } catch (e) {
      error = friendlyErrorMessage(String(e));
    }
  }

  function requestDeleteSession(id: string, title: string) {
    deleteTarget = { id, title };
    deleteConfirmOpen = true;
  }

  async function confirmDeleteSession() {
    deleteConfirmOpen = false;
    const target = deleteTarget;
    deleteTarget = null;
    if (!target || !$projectPath) return;
    try {
      await api.quests.deleteChat(target.id, $projectPath);
      if (activeId === target.id) {
        activeId = null;
        session = null;
        merge = null;
      }
      await refreshList();
    } catch (e) {
      error = friendlyErrorMessage(String(e));
    }
  }

  async function reopenPendingReview() {
    if (!session?.pendingPlan || !$projectPath || busy) return;
    busy = true;
    error = "";
    try {
      merge = await api.quests.filterAndMergePlan(
        session.pendingPlan,
        [],
        [],
        $projectPath,
      );
    } catch (e) {
      error = friendlyErrorMessage(String(e));
    } finally {
      busy = false;
    }
  }

  async function preflightAi(): Promise<boolean> {
    aiReadyHint = "";
    try {
      const result = await invoke<string>("test_integration", { provider: "ai" });
      aiReadyHint = result;
      return true;
    } catch (e) {
      error = `AI provider not connected: ${friendlyErrorMessage(String(e))}. Check Settings → AI to verify your configuration.`;
      return false;
    }
  }

  let canSend = $derived(
    !!$projectPath &&
      (pendingIntent === "lore" ||
        (showJson ? rawJson.trim().length > 0 : input.trim().length > 0)),
  );

  async function send(intent: "generate" | "extend" | "lore" | "branch" | null = "generate") {
    if (!$projectPath || busy) return;
    const useIntent = intent ?? pendingIntent;
    const text = showJson && rawJson.trim() ? rawJson.trim() : input.trim();
    if (!text && useIntent !== "lore") {
      composerHint = showJson
        ? "Paste QuestPlan JSON first"
        : "Describe the quest line first";
      return;
    }
    composerHint = "";
    busy = true;
    error = "";
    loreWarning = "";
    progressLog = ["Starting generation…"];
    streamDraft = "";
    try {
      if (showJson && rawJson.trim()) {
        merge = await api.quests.parseAndMergePlan(rawJson, $projectPath);
        progressLog = ["Parsed QuestPlan JSON"];
        input = "";
      } else {
        const useForceAi = forceAi;
        if (useForceAi && useIntent !== "lore") {
          progressLog = ["Checking AI provider…"];
          const ok = await preflightAi();
          if (!ok) return;
        }
        if (useIntent === "branch" && anchorQuest && targetChapterId) {
          if (!onsavechapter) {
            progressLog = [...progressLog, "Error: anchor chapter save callback missing"];
            error = "Unable to save the anchor chapter — close and reopen the AI panel, then retry.";
            return;
          }
          progressLog = [...progressLog, "Saving anchor chapter…"];
          const saveOutcome = await onsavechapter(targetChapterId);
          if (saveOutcome === "cancelled") {
            progressLog = [...progressLog, "Branch aborted — anchor chapter not saved"];
            composerHint = "Branch aborted: anchor chapter was not saved.";
            return;
          }
          if (saveOutcome === "error") {
            progressLog = [...progressLog, "Error: anchor chapter save failed"];
            error = "Failed to save the anchor chapter. Please check editor errors and retry.";
            return;
          }
          progressLog = [
            ...progressLog,
            saveOutcome === "saved"
              ? "Anchor chapter saved"
              : "Anchor chapter up to date",
          ];
        }
        const msg =
          useIntent === "lore"
            ? input.trim() || "Regenerate lore for pending plan"
            : useIntent === "extend"
              ? input.trim() || "Extend the quest line by 8 quests"
              : useIntent === "branch"
                ? input.trim() ||
                  `Create a branch of 6 quests from "${anchorQuest?.title ?? "selected"}"`
                : text;
        const result = await api.quests.chatTurn(
          msg,
          {
            chatId: activeId,
            forceAi: useForceAi,
            intent: useIntent,
            anchorQuestId: useIntent === "branch" ? anchorQuest?.id ?? null : null,
            targetChapterId:
              useIntent === "generate" || useIntent === "extend"
                ? targetChapterId
                : null,
          },
          $projectPath,
        );
        session = result.session;
        activeId = result.session.id;
        merge = result.merge;
        progressLog = result.progressLog ?? progressLog;
        lastUsage = result.usage ?? result.session.messages.at(-1)?.usage ?? null;
        const logJoined = progressLog.join("\n");
        if (/offline heuristic/i.test(logJoined)) {
          loreWarning = "Used offline generation template. Enable 'Force AI' in Advanced settings for full LLM.";
        }
        if (/Lore AI unavailable|Lore fail|template fill/i.test(logJoined)) {
          loreWarning =
            (loreWarning ? loreWarning + " " : "") +
            "Quest lore used default fallback templates.";
        }
        input = "";
        await refreshList();
      }
    } catch (e) {
      const rawMsg = String(e);
      if (/cancelled/i.test(rawMsg)) {
        error = "";
        progressLog = [...progressLog, "Cancelled"];
      } else {
        const rawMatch = rawMsg.match(
          /<<<QUEST_RAW_JSON>>>\r?\n?([\s\S]*?)\r?\n?<<<END_QUEST_RAW_JSON>>>/,
        );
        const recovered = (rawMatch?.[1] ?? streamDraft).trim();
        if (recovered) {
          rawJson = recovered;
          showJson = true;
          composerHint =
            "AI output had syntax formatting issues. Recovered raw JSON has been loaded into the editor.";
          error = "The AI generated response had formatting issues. Raw JSON has been restored for manual review.";
        } else {
          error = friendlyErrorMessage(rawMsg);
        }
        progressLog = [...progressLog, `Error: ${error}`];
        if (activeId) {
          try {
            session = await api.quests.loadChat(activeId, $projectPath!);
          } catch {
            /* ignore */
          }
        }
        await refreshList();
      }
    } finally {
      busy = false;
      streamDraft = "";
    }
  }

  async function stopGeneration() {
    if (!busy) return;
    try {
      await api.quests.cancelChatTurn();
      appendProgressLine("Stopping…");
    } catch (e) {
      error = friendlyErrorMessage(String(e));
    }
  }

  function onApplyReview(detail: { chapterKeys: string[]; questKeys: string[] }) {
    if (busy) return;
    if (!merge?.plan || !$projectPath) return;
    void (async () => {
      busy = true;
      try {
        const filtered = await api.quests.filterAndMergePlan(
          merge!.plan,
          detail.chapterKeys,
          detail.questKeys,
          $projectPath!,
        );
        onapply?.(filtered);
        merge = null;
        if (session) {
          const next: QuestChatSession = { ...session, pendingPlan: null };
          session = next;
          await api.quests.saveChat(next, $projectPath!);
          await refreshList();
        }
      } catch (err) {
        error = friendlyErrorMessage(String(err));
      } finally {
        busy = false;
      }
    })();
  }

  async function confirmDiscardPendingPlan() {
    discardConfirmOpen = false;
    if (!session?.pendingPlan || !$projectPath) return;
    const next: QuestChatSession = { ...session, pendingPlan: null };
    session = next;
    merge = null;
    if (pendingIntent === "extend" || pendingIntent === "lore") {
      pendingIntent = "generate";
    }
    try {
      await api.quests.saveChat(next, $projectPath);
      await refreshList();
    } catch (e) {
      error = friendlyErrorMessage(String(e));
    }
  }

  onMount(() => {
    loadAiAdvPrefs();
    aiPrefsReady = true;
    void refreshList();
    void listen<QuestAiProgressPayload>("quest-ai-progress", (event) => {
      const payload = event.payload;
      if (!busy) return;
      if (activeId != null && payload.chatId !== activeId) return;
      appendProgressLine(payload.line);
      if (payload.phase && payload.phase !== "outline") {
        streamDraft = "";
      }
    }).then((unlisten) => {
      unlistenProgress = unlisten;
    });
    void listen<QuestAiTokenPayload>("quest-ai-token", (event) => {
      const payload = event.payload;
      if (!busy) return;
      if (activeId != null && payload.chatId !== activeId) return;
      if (streamDraft.length >= 262144) return;
      streamDraft += payload.text;
      if (streamDraft.length > 262144) {
        streamDraft = `${streamDraft.slice(0, 262144)}\n…`;
      }
    }).then((unlisten) => {
      unlistenTokens = unlisten;
    });
  });

  onDestroy(() => {
    unlistenProgress?.();
    unlistenTokens?.();
  });

  $effect(() => {
    if (!aiPrefsReady) return;
    void forceAi;
    void pendingIntent;
    saveAiAdvPrefs();
  });

  $effect(() => {
    if ($projectPath) void refreshList();
  });

  $effect(() => {
    if ($questChatFocusId && $projectPath && open) {
      const id = $questChatFocusId;
      questChatFocusId.set(null);
      void selectSession(id);
    }
  });

  $effect(() => {
    const justOpened = open && !wasOpen;
    wasOpen = open;
    if (!justOpened || !open) return;
    if (discardConfirmOpen || deleteConfirmOpen) return;
    queueMicrotask(() => {
      if (discardConfirmOpen || deleteConfirmOpen) return;
      composerEl?.focus({ preventScroll: true });
    });
  });
</script>

<aside class="qai flex flex-col h-full min-h-0 bg-[var(--bg-secondary)] border-l border-[var(--border-color)] text-[var(--text-primary)]" class:open aria-busy={busy}>
  <!-- Header -->
  <div class="qai-h flex items-center justify-between gap-2 px-3.5 py-2.5 border-b border-[var(--border-color)] bg-[var(--bg-card)] flex-shrink-0">
    <div class="flex items-center gap-2">
      <Sparkles size={16} class="text-[var(--accent-primary)]" />
      <strong class="text-xs font-bold text-[var(--text-primary)]">Quest AI</strong>
      {#if sessionUsageLabel}
        <span class="usage-pill text-[12px] text-[var(--text-muted)] bg-[var(--bg-secondary)] px-2 py-0.5 rounded border border-[var(--border-color)]" title="Session token usage">
          {sessionUsageLabel}
        </span>
      {/if}
    </div>
    <button type="button" class="close-btn text-[var(--text-muted)] hover:text-[var(--text-primary)] p-1 rounded hover:bg-[var(--bg-hover)]" title="Close AI Sidebar" onclick={() => onclose?.()}>
      <PanelRightClose size={15} />
    </button>
  </div>

  {#if anchorQuest}
    <div class="anchor-banner flex items-center gap-2 px-3 py-2 bg-[var(--accent-primary)]/10 border-b border-[var(--accent-primary)]/20 text-xs flex-shrink-0" title="Branch will root at this quest">
      <GitBranch size={13} class="text-[var(--accent-primary)] flex-shrink-0" />
      <div class="flex-1 min-w-0">
        <span class="text-[12px] uppercase font-semibold text-[var(--accent-primary)] block">Branch root</span>
        <span class="font-bold truncate block text-[var(--text-primary)]">{anchorQuest.title || "Untitled quest"}</span>
      </div>
      <code class="text-[12px] font-mono bg-[var(--bg-primary)] px-1.5 py-0.5 rounded text-[var(--text-muted)] border border-[var(--border-color)]">
        {anchorQuest.id.slice(0, 8)}
      </code>
    </div>
  {/if}

  <!-- Sessions List -->
  <div class="sessions px-3 py-2 border-b border-[var(--border-color)] bg-[var(--bg-secondary)] flex-shrink-0">
    <div class="flex items-center justify-between mb-1.5">
      <span class="text-[12px] font-semibold text-[var(--text-muted)] uppercase tracking-wider">Sessions</span>
      <button type="button" class="text-xs font-semibold text-[var(--accent-primary)] hover:brightness-110 flex items-center gap-2" onclick={newSession} disabled={!$projectPath}>
        <Plus size={13} /> New
      </button>
    </div>
    <div class="sess-list flex flex-col gap-2 max-h-24 overflow-y-auto">
      {#each sessions as s (s.id)}
        <div class="sess flex items-center justify-between gap-2 px-2 py-1 rounded text-xs transition {activeId === s.id ? 'bg-[var(--accent-primary)]/15 font-semibold text-[var(--text-primary)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)]'}">
          <button type="button" class="sess-open flex-1 text-left truncate" onclick={() => selectSession(s.id)}>
            {s.title}
          </button>
          <button
            type="button"
            class="text-[var(--text-muted)] hover:text-red-400 p-0.5"
            aria-label={`Delete chat ${s.title}`}
            title={`Delete chat ${s.title}`}
            onclick={() => requestDeleteSession(s.id, s.title)}
          >
            <Trash2 size={12} />
          </button>
        </div>
      {/each}
    </div>
  </div>

  <!-- Chat Transcript -->
  <div class="transcript flex-1 overflow-y-auto p-3 flex flex-col gap-2.5 min-h-0 bg-[var(--bg-primary)]" bind:this={transcriptEl}>
    {#if !session?.messages?.length}
      <div class="empty-chat flex flex-col items-center justify-center py-6 text-center text-[var(--text-muted)]">
        <MessageSquareText size={28} class="mb-2 opacity-50 text-[var(--accent-primary)]" />
        <p class="text-xs max-w-xs leading-relaxed text-[var(--text-secondary)]">
          Describe the quest line or chapter you want to create. Click <strong>Review</strong> to preview nodes before merging.
        </p>
        <div class="chips flex flex-wrap gap-2 mt-3.5 justify-center">
          {#each exampleChips as chip (chip.label)}
            <button
              type="button"
              class="chip text-xs px-2.5 py-1 bg-[var(--bg-card)] border border-[var(--border-color)] hover:border-[var(--accent-primary)] hover:text-[var(--text-primary)] rounded-full transition text-[var(--text-secondary)]"
              onclick={() => useChip(chip.text)}
            >
              {chip.label}
            </button>
          {/each}
        </div>
      </div>
    {:else}
      {#each session.messages as m, i (`${m.role}-${i}`)}
        <div
          class="msg p-2.5 rounded-lg border text-xs leading-relaxed {m.role === 'user' ? 'bg-[var(--accent-primary)]/10 border-[var(--accent-primary)]/30 text-[var(--text-primary)] ml-3' : 'bg-[var(--bg-card)] border-[var(--border-color)] text-[var(--text-primary)] mr-3'}"
          aria-live={m.role === "assistant" ? "polite" : undefined}
        >
          <strong class="block mb-1 text-[12px] font-bold text-[var(--accent-primary)]">
            {m.role === "user" ? "You" : "Quest AI"}
          </strong>
          <p class="whitespace-pre-wrap">{m.content}</p>
          {#if m.progressLog?.length}
            <details class="prog mt-2 text-[12px] text-[var(--text-muted)] border-t border-[var(--border-color)] pt-1.5">
              <summary class="cursor-pointer font-medium">{m.progressLog.length} generation log entries</summary>
              <ul class="list-disc pl-4 mt-1 space-y-0.5">
                {#each m.progressLog as p, pi (`p-${pi}`)}
                  <li>{p}</li>
                {/each}
              </ul>
            </details>
          {/if}
          {#if formatUsage(m.usage)}
            <div class="text-[12px] text-[var(--text-muted)] mt-1.5 text-right font-mono">{formatUsage(m.usage)}</div>
          {/if}
        </div>
      {/each}
    {/if}
  </div>

  <!-- Live Generation Progress Banner -->
  {#if busy && (progressLog.length || streamDraft)}
    <div class="live-prog flex items-center gap-2 px-3 py-2 bg-[var(--accent-primary)]/10 border-t border-[var(--accent-primary)]/20 text-xs text-[var(--text-primary)]">
      <Loader2 size={13} class="spin text-[var(--accent-primary)] flex-shrink-0" />
      <span class="truncate">{progressLog.length ? progressLog[progressLog.length - 1] : "Generating quest plan…"}</span>
    </div>
    {#if streamDraft}
      <details class="stream-wrap px-3 py-1 bg-[var(--bg-secondary)] border-t border-[var(--border-color)]" open={busy && !merge}>
        <summary class="text-[12px] text-[var(--text-muted)] cursor-pointer font-medium">Live draft ({streamDraft.length} chars)</summary>
        <pre class="text-[12px] max-h-24 overflow-y-auto bg-[var(--bg-primary)] p-2 rounded mt-1 font-mono text-[var(--text-secondary)] whitespace-pre-wrap border border-[var(--border-color)]">{streamDraft}</pre>
      </details>
    {/if}
  {/if}

  <!-- Error and Warning Notifications -->
  {#if error}
    <div class="err flex items-start gap-2 px-3 py-2 bg-red-500/10 border-t border-red-500/20 text-red-400 text-xs" role="alert">
      <AlertCircle size={14} class="flex-shrink-0 mt-0.5" />
      <span class="flex-1 leading-normal">{error}</span>
      <button type="button" class="text-red-400 hover:text-[var(--accent-danger)] font-bold px-1" title="Dismiss" aria-label="Dismiss error" onclick={() => (error = "")}>
        ×
      </button>
    </div>
  {/if}
  {#if loreWarning}
    <div class="warn flex items-start gap-2 px-3 py-2 bg-amber-500/10 border-t border-amber-500/20 text-[var(--accent-warning)] text-xs" role="status">
      <Info size={14} class="flex-shrink-0 mt-0.5" />
      <span class="flex-1 leading-normal">{loreWarning}</span>
      <button type="button" class="text-[var(--accent-warning)] hover:text-[var(--accent-warning)] font-bold px-1" title="Dismiss" aria-label="Dismiss warning" onclick={() => (loreWarning = "")}>
        ×
      </button>
    </div>
  {/if}

  <!-- Merge Review Area -->
  {#if merge}
    <div class="p-2 border-t border-[var(--border-color)] bg-[var(--bg-card)]">
      <QuestPlanReview
        {merge}
        needsReviewAck={!!merge.plan?.needsUserReview}
        onapply={onApplyReview}
        ondiscard={() => (discardConfirmOpen = true)}
      />
    </div>
  {/if}

  {#if session?.pendingPlan && !merge}
    <div class="pending-plan-bar flex items-center justify-between gap-2 px-3 py-2 bg-amber-500/10 border-t border-amber-500/20 text-xs">
      <span class="font-semibold text-[var(--accent-warning)]">Pending plan ready for review</span>
      <div class="flex items-center gap-2">
        <button
          type="button"
          class="px-2.5 py-1 bg-[var(--accent-primary)] text-[var(--on-accent)] font-semibold rounded hover:brightness-110 disabled:opacity-50"
          disabled={busy}
          onclick={() => void reopenPendingReview()}
        >
          Review
        </button>
        <button
          type="button"
          class="px-2 py-1 bg-red-500/20 text-[var(--accent-danger)] border border-red-500/30 font-medium rounded hover:bg-red-500/30 disabled:opacity-50"
          disabled={busy}
          onclick={() => (discardConfirmOpen = true)}
        >
          Discard
        </button>
      </div>
    </div>
  {/if}

  <!-- Composer & Action Section -->
  <div class="composer p-3 border-t border-[var(--border-color)] bg-[var(--bg-secondary)] flex flex-col gap-2.5 flex-shrink-0">
    <!-- Intent Tabs / Pills -->
    <div
      class="intent-row flex bg-[var(--bg-card)] p-0.5 rounded-lg border border-[var(--border-color)] gap-0.5"
      role="radiogroup"
      aria-label="Quest AI intent"
      tabindex="-1"
      onkeydown={onIntentKeydown}
    >
      <button
        type="button"
        class="intent flex-1 py-1 text-center text-xs font-semibold rounded-md transition {pendingIntent === 'generate' ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)]'}"
        role="radio"
        aria-checked={pendingIntent === "generate"}
        tabindex={pendingIntent === "generate" ? 0 : -1}
        onclick={() => setIntent("generate")}
      >
        Generate
      </button>
      <button
        type="button"
        class="intent flex-1 py-1 text-center text-xs font-semibold rounded-md transition flex items-center justify-center gap-2 {pendingIntent === 'branch' ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)] disabled:opacity-40'}"
        role="radio"
        aria-checked={pendingIntent === "branch"}
        tabindex={pendingIntent === "branch" ? 0 : -1}
        disabled={!anchorQuest}
        title={anchorQuest ? "Branch from selected quest" : "Select a quest on canvas first"}
        onclick={() => setIntent("branch")}
      >
        <GitBranch size={11} /> Branch
      </button>
      <button
        type="button"
        class="intent flex-1 py-1 text-center text-xs font-semibold rounded-md transition {pendingIntent === 'extend' ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)] disabled:opacity-40'}"
        role="radio"
        aria-checked={pendingIntent === "extend"}
        tabindex={pendingIntent === "extend" ? 0 : -1}
        disabled={!session?.pendingPlan}
        title={session?.pendingPlan ? "Append to pending plan" : "Generate a plan first"}
        onclick={() => setIntent("extend")}
      >
        Extend
      </button>
      <button
        type="button"
        class="intent flex-1 py-1 text-center text-xs font-semibold rounded-md transition {pendingIntent === 'lore' ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)] disabled:opacity-40'}"
        role="radio"
        aria-checked={pendingIntent === "lore"}
        tabindex={pendingIntent === "lore" ? 0 : -1}
        disabled={!session?.pendingPlan}
        title={session?.pendingPlan ? "Regenerate lore only" : "Generate a plan first"}
        onclick={() => setIntent("lore")}
      >
        Lore
      </button>
    </div>

    {#if showJson}
      <textarea
        rows="4"
        class="w-full text-xs font-mono p-2 bg-[var(--bg-primary)] border border-[var(--border-color)] rounded-lg text-[var(--text-primary)] resize-y focus:outline-none focus:border-[var(--accent-primary)]"
        placeholder={'{ "why": "…", "chapters": [{ "title": "…", "quests": […] }] }'}
        bind:this={composerEl}
        bind:value={rawJson}
        oninput={() => (composerHint = "")}
      ></textarea>
    {:else}
      <textarea
        rows="3"
        class="w-full text-xs p-2.5 bg-[var(--bg-primary)] border border-[var(--border-color)] rounded-lg text-[var(--text-primary)] resize-y focus:outline-none focus:border-[var(--accent-primary)] placeholder-[var(--text-muted)]"
        placeholder={pendingIntent === "branch"
          ? `Describe how the branch progresses from "${anchorQuest?.title ?? "…"}"…`
          : pendingIntent === "lore"
            ? "Provide custom lore guidelines or tone instructions…"
            : "Describe the quest line or chapter topic…"}
        bind:this={composerEl}
        bind:value={input}
        oninput={() => (composerHint = "")}
        onkeydown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) void send(pendingIntent);
        }}
      ></textarea>
    {/if}

    {#if composerHint}
      <div class="composer-hint text-xs text-amber-400 bg-amber-400/10 px-2 py-1 rounded" role="status">
        {composerHint}
      </div>
    {/if}

    <div class="flex flex-wrap items-center justify-between gap-x-2 gap-y-1.5 min-w-0">
      <div class="flex min-w-0 items-center gap-2">
        <button
          type="button"
          class="text-[12px] font-semibold text-[var(--text-muted)] hover:text-[var(--text-primary)] flex items-center gap-2 whitespace-nowrap flex-shrink-0"
          onclick={() => (showJson = !showJson)}
          disabled={busy}
        >
          <Code size={12} />
          {showJson ? "Switch to Text" : "Paste JSON"}
        </button>
        <span class="text-[12px] text-[var(--text-muted)] whitespace-nowrap">Ctrl+Enter to send</span>
      </div>

      <div class="ml-auto flex items-center gap-2 flex-shrink-0">
        {#if busy}
          <button
            type="button"
            class="px-3.5 py-1.5 bg-red-600 text-white text-xs font-bold rounded-lg hover:bg-red-500 shadow-sm"
            onclick={() => void stopGeneration()}
          >
            Stop
          </button>
        {:else}
          <button
            type="button"
            class="px-4 py-1.5 bg-[var(--accent-primary)] text-[var(--on-accent)] text-xs font-bold rounded-lg hover:brightness-110 shadow-sm flex items-center gap-2 disabled:opacity-40"
            disabled={!canSend}
            onclick={() => send(pendingIntent)}
          >
            <Send size={13} />
            {pendingIntent === "branch" ? "Branch" : pendingIntent === "extend" ? "Extend" : pendingIntent === "lore" ? "Lore" : "Generate"}
          </button>
        {/if}
      </div>
    </div>

    <details class="text-[12px] text-[var(--text-muted)] pt-0.5">
      <summary class="cursor-pointer font-medium hover:text-[var(--text-primary)]">Advanced settings</summary>
      <label class="flex items-center gap-2 mt-1.5 cursor-pointer">
        <input type="checkbox" bind:checked={forceAi} />
        <span>Force AI call (bypass offline template fallback)</span>
      </label>
    </details>
  </div>
</aside>

{#if discardConfirmOpen}
  <ConfirmDialog
    title="Discard pending plan?"
    message="This clears the unmerged QuestPlan from the current chat session. You can generate a new one anytime."
    danger={true}
    confirmLabel="Discard"
    onconfirm={() => void confirmDiscardPendingPlan()}
    oncancel={() => (discardConfirmOpen = false)}
  />
{/if}

{#if deleteConfirmOpen && deleteTarget}
  <ConfirmDialog
    title="Delete chat session?"
    message={`Delete chat “${deleteTarget.title}”? History cannot be recovered.`}
    danger={true}
    confirmLabel="Delete"
    onconfirm={() => void confirmDeleteSession()}
    oncancel={() => {
      deleteConfirmOpen = false;
      deleteTarget = null;
    }}
  />
{/if}

<style>
  .qai {
    width: 340px;
    min-width: 280px;
    max-width: 420px;
  }
  :global(.spin) {
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
