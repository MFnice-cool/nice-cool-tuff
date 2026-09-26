<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import {
    MessagesSquare,
    Plus,
    Send,
    Sparkles,
    Package,
    Loader2,
    CheckCircle2,
    Trash2,
    GitGraph,
    Lock,
    Unlock,
    Shuffle,
    Search,
    X,
    MoreHorizontal,
  } from "@lucide/svelte";
  import { projectPath, projectInfo, ideStageRequest, questChatFocusId, tuneChatFocusId } from "../lib/store";
  import { toasts } from "../lib/toast";
  import { api, type SearchResult, type TuneChatSession } from "../lib/api";

  let { currentView = $bindable() }: { currentView: string } = $props();

  type ChatMessage = { role: string; content: string; createdAt?: string | null };
  type PackDraftMod = {
    slug: string;
    projectId: string;
    name: string;
    reason: string;
    category: string;
    downloads: number;
    provider?: string;
  };
  type CreateModeBrief = {
    title: string;
    mcVersion: string;
    loader: string;
    targetCount: number;
    mustHave: { query: string; slugHint?: string | null; reason: string }[];
    categories: { id: string; query: string; count: number; reason: string }[];
    exclude: string[];
  };
  type PackDraft = { brief: CreateModeBrief; mods: PackDraftMod[]; unresolved: string[] };
  type CandidateAddon = {
    slug: string;
    name: string;
    summary?: string;
    score: number;
    source: string;
  };
  type CurationPersist = {
    memory?: unknown;
    pillarStatus?: { id: string; label: string; priority: number; covered: boolean; evidenceSlugs?: string[] }[];
    partial?: boolean;
    stopReason?: string;
    launcherScore?: number | null;
    tier?: string | null;
  };
  type CreateChatSession = {
    id: string;
    title: string;
    messages: ChatMessage[];
    draft?: PackDraft | null;
    curation?: CurationPersist | null;
    updatedAt: string;
  };
  type UnifiedSession = {
    kind: "create" | "quest" | "tune";
    id: string;
    title: string;
    messages: ChatMessage[];
    updatedAt: string;
    draft?: PackDraft | null;
  };

  let sessions = $state<UnifiedSession[]>([]);
  let activeId = $state<string | null>(null);
  let activeKind = $state<"create" | "quest" | "tune">("create");
  let messages = $state<ChatMessage[]>([]);
  let brief = $state<CreateModeBrief | null>(null);
  let draft = $state<PackDraft | null>(null);
  let candidates = $state<CandidateAddon[]>([]);
  let sessionGeneration = 0;
  let input = $state("");
  let targetCount = $state(80);
  let busy = $state(false);
  let busyKind = $state<"" | "plan" | "refine" | "rank" | "curate" | "quick" | "build" | "preview" | "install">("");
  let maxCurateIterations = $state(5);
  let pillarStatus = $state<{ id: string; label: string; priority: number; covered: boolean; evidenceSlugs?: string[] }[]>([]);
  let lastCuratePartial = $state(false);
  let lastCurateStop = $state("");
  let lastLauncherScore = $state<number | null>(null);
  let lastCuration = $state<CurationPersist | null>(null);
  let phase = $state("");
  let progressDone = $state(0);
  let progressTotal = $state(0);
  let progressCurrent = $state("");
  let unlisten = $state<UnlistenFn | null>(null);
  let lastPath = $state("");
  let draftConfirmOpen = $state(false);
  let draftSelected = $state<Record<string, boolean>>({});
  let installPreviewItems = $state<
    {
      slug: string;
      projectId: string;
      name: string;
      provider?: string;
      status: string;
      version?: string | null;
      fileName?: string | null;
      hashAlgo?: string | null;
      hash?: string | null;
      destPath?: string;
      error?: string | null;
    }[]
  >([]);
  let postInstallTrail = $state(false);
  let lastInstallCount = $state(0);
  let questPendingPlan = $state(false);
  let tunePendingAdvise = $state(false);
  let moreMenuOpen = $state(false);

  type ContextualNext = "" | "build" | "rank" | "curate";
  const contextualNext = $derived.by((): ContextualNext => {
    if (isSideStageChat || !brief) return "";
    if (!draft?.mods?.length) return "build";
    if (!lastCuration) return "rank";
    return "curate";
  });

  // Alternatives popover (per-mod swap suggestions).
  let altForKey = $state<string | null>(null);
  let altLoading = $state(false);
  type AltOption = { slug: string; name: string; summary: string; source: string };
  let altOptions = $state<AltOption[]>([]);

  // Inline "add a specific mod" search.
  let addModOpen = $state(false);
  let addModQuery = $state("");
  let addModResults = $state<SearchResult[]>([]);
  let addModLoading = $state(false);
  let addModTimer = $state<ReturnType<typeof setTimeout> | null>(null);

  // Editable brief helpers.
  const LOCKED_REASON = "Locked by user";
  const KNOWN_CATEGORY_IDS = [
    "technology",
    "magic",
    "decoration",
    "utility",
    "adventure",
    "worldgen",
    "storage",
    "food",
    "equipment",
    "library",
  ];
  let newMustHaveQuery = $state("");
  let newExcludeText = $state("");
  let newCategoryId = $state("");

  const active = $derived(sessions.find((s) => s.id === activeId && s.kind === activeKind) ?? null);
  const mcLabel = $derived($projectInfo?.minecraftVersion ?? "?");
  const loaderLabel = $derived($projectInfo?.loaderKind ?? "?");
  const draftConfirmCount = $derived(draft?.mods?.filter((m) => draftSelected[m.projectId || m.slug] !== false).length ?? 0);
  const isQuestChat = $derived(activeKind === "quest");
  const isTuneChat = $derived(activeKind === "tune");
  const isSideStageChat = $derived(isQuestChat || isTuneChat);

  function sortKey(updatedAt: string): number {
    const n = Number(updatedAt);
    if (!Number.isNaN(n) && n > 0) return n;
    const t = Date.parse(updatedAt);
    return Number.isNaN(t) ? 0 : t / 1000;
  }

  async function refreshSessions() {
    if (!$projectPath) {
      sessions = [];
      return;
    }
    try {
      const [createList, questChats, tuneChats] = await Promise.all([
        invoke<CreateChatSession[]>("list_create_chats", { path: $projectPath }),
        api.quests.listChats($projectPath).catch(() => ({ sessions: [], corruptSkipped: 0 })),
        api.config.listChats($projectPath).catch(() => ({ sessions: [], corruptSkipped: 0 })),
      ]);
      const questList = questChats.sessions;
      const tuneList = tuneChats.sessions;
      const unified: UnifiedSession[] = [
        ...createList.map((s) => ({
          kind: "create" as const,
          id: s.id,
          title: s.title,
          messages: s.messages ?? [],
          updatedAt: s.updatedAt,
          draft: s.draft ?? null,
        })),
        ...questList.map((s) => ({
          kind: "quest" as const,
          id: s.id,
          title: s.title || "Quest line",
          messages: (s.messages ?? []).map((m) => ({
            role: m.role,
            content: m.content,
            createdAt: m.createdAt,
          })),
          updatedAt: s.updatedAt,
        })),
        ...tuneList.map((s: TuneChatSession) => ({
          kind: "tune" as const,
          id: s.id,
          title: s.title || "Tune configs",
          messages: (s.messages ?? []).map((m) => ({
            role: m.role,
            content: m.content,
            createdAt: m.createdAt,
          })),
          updatedAt: s.updatedAt,
        })),
      ];
      unified.sort((a, b) => sortKey(b.updatedAt) - sortKey(a.updatedAt));
      sessions = unified;
      if (
        activeId &&
        !sessions.some((s) => s.id === activeId && s.kind === activeKind)
      ) {
        const first = sessions[0];
        activeId = first?.id ?? null;
        activeKind = first?.kind ?? "create";
      }
    } catch {
      sessions = [];
    }
  }

  async function selectSession(id: string, kind: "create" | "quest" | "tune" = "create") {
    if (!$projectPath) return;
    const generation = ++sessionGeneration;
    activeId = id;
    activeKind = kind;
    questPendingPlan = false;
    tunePendingAdvise = false;
    // Clear UI state that belongs to the previously selected chat so a
    // quest/tune thread never shows stale create-chat artifacts (pillars,
    // install trail, draft previews) — and vice versa.
    pillarStatus = [];
    lastCuration = null;
    postInstallTrail = false;
    lastInstallCount = 0;
    moreMenuOpen = false;
    if (kind === "quest") {
      try {
        const s = await api.quests.loadChat(id, $projectPath);
        if (generation !== sessionGeneration) return;
        messages = (s.messages ?? []).map((m) => ({
          role: m.role,
          content: m.content,
          createdAt: m.createdAt,
        }));
        draft = null;
        brief = null;
        candidates = [];
        questPendingPlan = !!s.pendingPlan;
        if (!sessions.some((x) => x.id === id && x.kind === "quest")) {
          await refreshSessions();
        }
      } catch (e) {
        toasts.error(String(e));
      }
      return;
    }
    if (kind === "tune") {
      try {
        const s = await api.config.loadChat(id, $projectPath);
        if (generation !== sessionGeneration) return;
        messages = (s.messages ?? []).map((m) => ({
          role: m.role,
          content: m.content,
          createdAt: m.createdAt,
        }));
        draft = null;
        brief = null;
        candidates = [];
        tunePendingAdvise = !!s.pendingAdvise;
        if (!sessions.some((x) => x.id === id && x.kind === "tune")) {
          await refreshSessions();
        }
      } catch (e) {
        toasts.error(String(e));
      }
      return;
    }
    try {
      const s = await invoke<CreateChatSession>("load_create_chat", {
        path: $projectPath,
        chatId: id,
      });
      if (generation !== sessionGeneration) return;
      messages = s.messages ?? [];
      draft = s.draft ?? null;
      brief = s.draft?.brief ?? brief;
      candidates = [];
      applyCurationPersist(s.curation);
      if (!sessions.some((x) => x.id === id && x.kind === "create")) {
        await refreshSessions();
      }
    } catch (e) {
      toasts.error(String(e));
    }
  }

  function openQuestChatInEditor(id: string) {
    questChatFocusId.set(id);
    currentView = "quests";
  }

  function openTuneChatInEditor(id: string) {
    tuneChatFocusId.set(id);
    ideStageRequest.set("configs");
    currentView = "ide";
  }

  async function newChat() {
    if (!$projectPath) return;
    try {
      const s = await invoke<CreateChatSession>("new_create_chat", {
        path: $projectPath,
        title: "New chat",
      });
      await refreshSessions();
      activeId = s.id;
      activeKind = "create";
      messages = [];
      draft = null;
      brief = null;
      candidates = [];
      clearCurationPersist();
      questPendingPlan = false;
      tunePendingAdvise = false;
      input = "";
    } catch (e) {
      toasts.error(String(e));
    }
  }

  async function newTuneChat() {
    if (!$projectPath) return;
    try {
      const s = await api.config.newChat("Tune configs", $projectPath);
      await refreshSessions();
      activeId = s.id;
      activeKind = "tune";
      messages = [];
      draft = null;
      brief = null;
      candidates = [];
      questPendingPlan = false;
      tunePendingAdvise = false;
      input = "";
    } catch (e) {
      toasts.error(String(e));
    }
  }

  async function deleteChat(id: string, kind: "create" | "quest" | "tune" = "create") {
    if (!$projectPath) return;
    const label =
      kind === "quest" ? "Delete this Quest AI chat?" : kind === "tune" ? "Delete this Tune AI chat?" : "Delete this chat?";
    if (!confirm(label)) return;
    try {
      if (kind === "quest") {
        await api.quests.deleteChat(id, $projectPath);
      } else if (kind === "tune") {
        await api.config.deleteChat(id, $projectPath);
      } else {
        await invoke("delete_create_chat", { path: $projectPath, chatId: id });
      }
      if (activeId === id && activeKind === kind) {
        activeId = null;
        activeKind = "create";
        messages = [];
        draft = null;
        brief = null;
        candidates = [];
        questPendingPlan = false;
        tunePendingAdvise = false;
      }
      await refreshSessions();
    } catch (e) {
      toasts.error(String(e));
    }
  }

  function applyCurationPersist(c: CurationPersist | null | undefined) {
    lastCuration = c ?? null;
    pillarStatus = c?.pillarStatus ?? [];
    lastCuratePartial = Boolean(c?.partial);
    lastCurateStop = c?.stopReason ?? "";
    lastLauncherScore =
      typeof c?.launcherScore === "number" ? c.launcherScore : null;
  }

  function clearCurationPersist() {
    lastCuration = null;
    pillarStatus = [];
    lastCuratePartial = false;
    lastCurateStop = "";
    lastLauncherScore = null;
  }

  async function persistDraft() {
    if (!$projectPath || !activeId || activeKind !== "create") return;
    const session: CreateChatSession = {
      id: activeId,
      title: brief?.title || active?.title || "Create Mode",
      messages,
      draft,
      curation: lastCuration,
      updatedAt: String(Date.now()),
    };
    try {
      await invoke("save_create_chat", { path: $projectPath, session });
      await refreshSessions();
    } catch {
      /* ignore */
    }
  }

  function pushSystemNote(content: string) {
    messages = [...messages, { role: "system", content }];
  }

  function modKey(m: { projectId?: string; slug: string }): string {
    return m.projectId || m.slug;
  }

  function ensureBrief(): CreateModeBrief {
    if (!brief) {
      brief = {
        title: "Custom pack",
        mcVersion: $projectInfo?.minecraftVersion ?? "",
        loader: $projectInfo?.loaderKind ?? "",
        targetCount,
        mustHave: [],
        categories: [],
        exclude: [],
      };
    }
    return brief;
  }

  function syncDraftBrief() {
    if (draft && brief) draft = { ...draft, brief };
  }

  // ── Per-mod row actions: Remove / Lock / Alternatives ──────────────

  function excludeKeysFor(m: PackDraftMod): string[] {
    return [m.slug, m.projectId, m.name]
      .filter((s): s is string => !!s && s.trim().length > 0)
      .map((s) => s.trim().toLowerCase());
  }

  function removeModFromDraft(m: PackDraftMod) {
    if (!draft) return;
    const key = modKey(m);
    draft = { ...draft, mods: draft.mods.filter((x) => modKey(x) !== key) };
    const b = ensureBrief();
    const exclude = new Set((b.exclude ?? []).map((s) => s.toLowerCase()));
    excludeKeysFor(m).forEach((k) => exclude.add(k));
    brief = { ...b, exclude: [...exclude] };
    syncDraftBrief();
    pushSystemNote(`Removed "${m.name}" from the draft.`);
    void persistDraft();
  }

  function isLocked(m: PackDraftMod): boolean {
    if (!brief) return false;
    const key = modKey(m).toLowerCase();
    return brief.mustHave.some(
      (mh) => mh.reason === LOCKED_REASON && (mh.slugHint ?? "").toLowerCase() === key,
    );
  }

  function toggleLock(m: PackDraftMod) {
    const b = ensureBrief();
    const key = modKey(m);
    const keyL = key.toLowerCase();
    const already = b.mustHave.some(
      (mh) => mh.reason === LOCKED_REASON && (mh.slugHint ?? "").toLowerCase() === keyL,
    );
    if (already) {
      brief = {
        ...b,
        mustHave: b.mustHave.filter(
          (mh) => !(mh.reason === LOCKED_REASON && (mh.slugHint ?? "").toLowerCase() === keyL),
        ),
      };
      pushSystemNote(`Unlocked "${m.name}".`);
    } else {
      brief = {
        ...b,
        mustHave: [...b.mustHave, { query: m.name, slugHint: key, reason: LOCKED_REASON }],
      };
      pushSystemNote(`Locked "${m.name}" — it will stay in future rebuilds.`);
    }
    syncDraftBrief();
    void persistDraft();
  }

  async function openAlternatives(m: PackDraftMod) {
    const key = modKey(m);
    if (altForKey === key) {
      altForKey = null;
      altOptions = [];
      return;
    }
    altForKey = key;
    altLoading = true;
    altOptions = [];
    try {
      const existingKeys = new Set((draft?.mods ?? []).map((x) => modKey(x).toLowerCase()));
      const seedId = m.projectId || m.slug;
      let options: AltOption[] = [];
      try {
        const partners = await invoke<
          { slug: string; name?: string; count?: number }[]
        >("suggest_partners_for_mod", { path: $projectPath, modId: seedId, limit: 8 });
        options = (partners ?? [])
          .filter((p) => p.slug && !existingKeys.has(p.slug.toLowerCase()))
          .map((p) => ({
            slug: p.slug,
            name: p.name || p.slug,
            summary: "",
            source: "often installed together",
          }));
      } catch {
        options = [];
      }
      if (options.length === 0) {
        const facet =
          m.category && m.category !== "mustHave" && m.category !== "fill" ? m.category : undefined;
        const res = await api.mods.search(m.category || m.name, {
          gameVersion: $projectInfo?.minecraftVersion ?? undefined,
          loader: $projectInfo?.loaderKind ?? undefined,
          category: facet,
          pageSize: 10,
        });
        options = (res.results ?? [])
          .filter((r) => {
            const k = (r.slug || r.id).toLowerCase();
            return k !== m.slug.toLowerCase() && !existingKeys.has(k);
          })
          .map((r) => ({
            slug: r.slug || r.id,
            name: r.name,
            summary: r.description ?? "",
            source: "catalog search",
          }));
      }
      altOptions = options.slice(0, 6);
    } catch (e) {
      toasts.error(String(e));
    } finally {
      altLoading = false;
    }
  }

  async function applyAlternative(m: PackDraftMod, choice: AltOption) {
    const currentDraft = draft;
    if (!currentDraft) return;
    altLoading = true;
    try {
      const proj = await api.mods.getProject(choice.slug);
      const key = modKey(m);
      const newMod: PackDraftMod = {
        slug: proj.slug || choice.slug,
        projectId: proj.id || choice.slug,
        name: proj.name || choice.name,
        reason: `Swapped for "${m.name}"`,
        category: m.category,
        downloads: proj.downloads ?? 0,
        provider: "modrinth",
      };
      draft = {
        ...currentDraft,
        mods: currentDraft.mods.map((x) => (modKey(x) === key ? newMod : x)),
      };
      const b = ensureBrief();
      const exclude = new Set((b.exclude ?? []).map((s) => s.toLowerCase()));
      excludeKeysFor(m).forEach((k) => exclude.add(k));
      const newKeyL = modKey(newMod).toLowerCase();
      const mustHave = b.mustHave.filter((mh) => (mh.slugHint ?? "").toLowerCase() !== key.toLowerCase());
      if (!mustHave.some((mh) => (mh.slugHint ?? "").toLowerCase() === newKeyL)) {
        mustHave.push({ query: newMod.name, slugHint: modKey(newMod), reason: LOCKED_REASON });
      }
      brief = { ...b, exclude: [...exclude], mustHave };
      syncDraftBrief();
      pushSystemNote(`Swapped "${m.name}" → "${newMod.name}".`);
      await persistDraft();
    } catch (e) {
      toasts.error(String(e));
    } finally {
      altLoading = false;
      altForKey = null;
      altOptions = [];
    }
  }

  // ── Inline "add a specific mod" search ──────────────────────────────

  function scheduleAddModSearch() {
    if (addModTimer) clearTimeout(addModTimer);
    const q = addModQuery.trim();
    if (q.length < 2) {
      addModResults = [];
      return;
    }
    addModTimer = setTimeout(() => void runAddModSearch(q), 300);
  }

  async function runAddModSearch(q: string) {
    addModLoading = true;
    try {
      const res = await api.mods.search(q, {
        gameVersion: $projectInfo?.minecraftVersion ?? undefined,
        loader: $projectInfo?.loaderKind ?? undefined,
        pageSize: 8,
      });
      addModResults = res.results ?? [];
    } catch {
      addModResults = [];
    } finally {
      addModLoading = false;
    }
  }

  function addModDirectly(r: SearchResult) {
    const key = (r.id || r.slug || "").toLowerCase();
    if (!key) return;
    if (draft?.mods.some((x) => modKey(x).toLowerCase() === key)) {
      toasts.error(`"${r.name}" is already in the draft.`);
      return;
    }
    const newMod: PackDraftMod = {
      slug: r.slug || r.id,
      projectId: r.id,
      name: r.name,
      reason: "Added manually",
      category: "mustHave",
      downloads: r.downloads ?? 0,
      provider: "modrinth",
    };
    const b = ensureBrief();
    if (!draft) draft = { brief: b, mods: [], unresolved: [] };
    draft = { ...draft, mods: [...draft.mods, newMod] };
    brief = {
      ...b,
      mustHave: [...b.mustHave, { query: newMod.name, slugHint: modKey(newMod), reason: LOCKED_REASON }],
    };
    syncDraftBrief();
    pushSystemNote(`Added "${newMod.name}" to the draft.`);
    addModQuery = "";
    addModResults = [];
    addModOpen = false;
    void persistDraft();
  }

  // ── Editable brief: must-have / categories / exclude ────────────────

  function updateBriefTitle(value: string) {
    const b = ensureBrief();
    brief = { ...b, title: value };
    syncDraftBrief();
  }

  function addMustHave() {
    const q = newMustHaveQuery.trim();
    if (!q) return;
    const b = ensureBrief();
    brief = { ...b, mustHave: [...b.mustHave, { query: q, reason: "Added by user" }] };
    newMustHaveQuery = "";
    syncDraftBrief();
  }

  function removeMustHave(idx: number) {
    if (!brief) return;
    brief = { ...brief, mustHave: brief.mustHave.filter((_, i) => i !== idx) };
    syncDraftBrief();
  }

  function addExcludeEntry() {
    const v = newExcludeText.trim().toLowerCase();
    if (!v) return;
    const b = ensureBrief();
    if (!(b.exclude ?? []).includes(v)) {
      brief = { ...b, exclude: [...(b.exclude ?? []), v] };
    }
    newExcludeText = "";
    syncDraftBrief();
  }

  function removeExcludeEntry(idx: number) {
    if (!brief) return;
    brief = { ...brief, exclude: brief.exclude.filter((_, i) => i !== idx) };
    syncDraftBrief();
  }

  function addCategoryRow() {
    const id = newCategoryId.trim().toLowerCase() || "custom";
    const b = ensureBrief();
    brief = {
      ...b,
      categories: [...b.categories, { id, query: id, count: 10, reason: "Added by user" }],
    };
    newCategoryId = "";
    syncDraftBrief();
  }

  function removeCategoryRow(idx: number) {
    if (!brief) return;
    brief = { ...brief, categories: brief.categories.filter((_, i) => i !== idx) };
    syncDraftBrief();
  }

  function bumpCategoryCount(idx: number, delta: number) {
    if (!brief) return;
    const cats = brief.categories.slice();
    const next = Math.max(0, (cats[idx].count || 0) + delta);
    cats[idx] = { ...cats[idx], count: next };
    brief = { ...brief, categories: cats };
    syncDraftBrief();
  }

  function updateCategoryField(idx: number, field: "id" | "query", value: string) {
    if (!brief) return;
    const cats = brief.categories.slice();
    cats[idx] = { ...cats[idx], [field]: value };
    brief = { ...brief, categories: cats };
    syncDraftBrief();
  }

  // ── Session list helpers ─────────────────────────────────────────────

  function timeAgo(updatedAt?: string): string {
    const secs = Number(updatedAt);
    if (!secs || Number.isNaN(secs)) return "";
    const now = Date.now() / 1000;
    const diff = Math.max(0, now - secs);
    if (diff < 60) return "just now";
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  function lastMessagePreview(s: UnifiedSession): string {
    const last = s.messages?.[s.messages.length - 1];
    if (!last) return "";
    const text = last.content.replace(/\s+/g, " ").trim();
    return text.length > 64 ? `${text.slice(0, 64)}…` : text;
  }

  function clampTargetCount() {
    const n = Math.round(Number(targetCount));
    targetCount = Math.min(120, Math.max(40, Number.isFinite(n) ? n : 80));
  }

  function phaseLabel(p: string): string {
    switch (p) {
      case "intent":
      case "plan":
      case "chat":
        return "Intent";
      case "catalog":
      case "search":
        return "Catalog";
      case "rank":
        return "Rank";
      case "preview":
      case "resolve":
        return "Install preview";
      case "install":
        return "Installing";
      default:
        return p || "Working";
    }
  }

  const progressHeadline = $derived(
    busy && phase
      ? `${phaseLabel(phase)}${progressCurrent ? ` — ${progressCurrent}` : "…"}`
      : "",
  );

  async function sendTuneMessage() {
    if (!$projectPath || !input.trim() || busy || activeKind !== "tune") return;
    const text = input.trim();
    input = "";
    busy = true;
    busyKind = "plan";
    phase = "tune";
    progressCurrent = "Config Advisor…";
    try {
      const res = await api.config.chatTurn(text, {
        chatId: activeId,
        goal: "free_text",
      }, $projectPath);
      activeId = res.session.id;
      activeKind = "tune";
      messages = (res.session.messages ?? []).map((m) => ({
        role: m.role,
        content: m.content,
        createdAt: m.createdAt,
      }));
      tunePendingAdvise = !!res.session.pendingAdvise;
      await refreshSessions();
    } catch (e) {
      toasts.error(String(e));
      messages = [
        ...messages,
        {
          role: "system",
          content: `Tune advise failed: ${String(e)}`,
          createdAt: new Date().toISOString(),
        },
      ];
    } finally {
      busy = false;
      busyKind = "";
      phase = "";
      progressCurrent = "";
    }
  }

  async function sendMessage(refine = false) {
    if (activeKind === "tune") {
      await sendTuneMessage();
      return;
    }
    if (!$projectPath || !input.trim() || busy) return;
    clampTargetCount();
    const text = input.trim();
    const historyForApi = [...messages];
    messages = [...messages, { role: "user", content: text, createdAt: new Date().toISOString() }];
    input = "";
    busy = true;
    busyKind = refine ? "refine" : "plan";
    phase = "intent";
    progressDone = 0;
    progressTotal = 0;
    progressCurrent = "Waiting for AI…";
    try {
      const res = await invoke<{
        chatId: string;
        reply: string;
        brief?: CreateModeBrief | null;
        candidates?: CandidateAddon[];
        session?: CreateChatSession;
      }>("create_mode_chat", {
        path: $projectPath,
        chatId: activeId,
        message: refine && brief
          ? `${text}\n\n(Please refine the existing pack brief.)`
          : text,
        targetCount,
        history: historyForApi,
        existingBrief: brief,
      });
      activeId = res.chatId;
      activeKind = "create";
      if (res.brief) brief = res.brief;
      candidates = res.candidates ?? [];
      if (res.session) {
        messages = res.session.messages ?? [];
      } else {
        messages = [
          ...historyForApi,
          { role: "user", content: text },
          { role: "assistant", content: res.reply },
        ];
      }
      await refreshSessions();
    } catch (e) {
      toasts.error(`${String(e)} — try Quick assemble (no AI).`);
      messages = [
        ...messages,
        {
          role: "system",
          content: `Plan failed: ${String(e)}`,
          createdAt: new Date().toISOString(),
        },
      ];
    } finally {
      busy = false;
      busyKind = "";
      phase = "";
      progressCurrent = "";
      progressDone = 0;
      progressTotal = 0;
    }
  }

  async function quickAssemble() {
    const text = input.trim();
    if (!$projectPath || !text || busy) return;
    const historyForApi = [...messages];
    messages = [...messages, { role: "user", content: text, createdAt: new Date().toISOString() }];
    input = "";
    busy = true;
    busyKind = "quick";
    phase = "intent";
    progressDone = 0;
    progressTotal = 0;
    progressCurrent = "Building brief…";
    try {
      const res = await invoke<{
        chatId: string;
        reply?: string;
        brief: CreateModeBrief;
        candidates?: CandidateAddon[];
        session?: CreateChatSession;
      }>("create_mode_quick_brief", {
        path: $projectPath,
        chatId: activeId,
        message: text,
        targetCount,
      });
      activeId = res.chatId;
      activeKind = "create";
      brief = res.brief;
      candidates = res.candidates ?? [];
      if (res.session) {
        messages = res.session.messages ?? [];
      } else {
        messages = [
          ...historyForApi,
          { role: "user", content: text },
          {
            role: "assistant",
            content: res.reply ?? `Quick brief: ${res.brief.title}`,
          },
        ];
      }
      await refreshSessions();

      phase = "catalog";
      progressDone = 0;
      progressTotal = 1;
      progressCurrent = "Searching catalogs…";
      draft = await invoke<PackDraft>("assemble_pack_draft", {
        path: $projectPath,
        brief: { ...brief, targetCount },
      });
      brief = draft.brief;
      installPreviewItems = [];
      messages = [
        ...messages,
        {
          role: "system",
          content: `Assembled draft: ${draft.mods.length} mods` +
            (draft.unresolved?.length
              ? ` (${draft.unresolved.length} must-have unresolved)`
              : ""),
        },
      ];
      await persistDraft();
      toasts.success(`Draft ready: ${draft.mods.length} mods`);
    } catch (e) {
      toasts.error(String(e));
      messages = [
        ...messages,
        {
          role: "system",
          content: `Quick assemble failed: ${String(e)}`,
          createdAt: new Date().toISOString(),
        },
      ];
    } finally {
      busy = false;
      busyKind = "";
      phase = "";
      progressCurrent = "";
      progressDone = 0;
      progressTotal = 0;
    }
  }

  async function buildDraft() {
    if (!$projectPath || !brief || busy) return;
    busy = true;
    busyKind = "build";
    phase = "catalog";
    progressDone = 0;
    progressTotal = 1;
    progressCurrent = "Searching catalogs…";
    try {
      draft = await invoke<PackDraft>("assemble_pack_draft", {
        path: $projectPath,
        brief: { ...brief, targetCount },
      });
      brief = draft.brief;
      installPreviewItems = [];
      clearCurationPersist();
      messages = [
        ...messages,
        {
          role: "system",
          content: `Catalog draft: ${draft.mods.length} mods` +
            (draft.unresolved?.length
              ? ` (${draft.unresolved.length} must-have unresolved)`
              : ""),
        },
      ];
      await persistDraft();
      toasts.success(`Draft ready: ${draft.mods.length} mods`);
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busy = false;
      busyKind = "";
      phase = "";
      progressCurrent = "";
      progressDone = 0;
      progressTotal = 0;
    }
  }

  async function rankWithAi() {
    if (!$projectPath || !brief || !draft?.mods?.length || busy) return;
    busy = true;
    busyKind = "rank";
    phase = "rank";
    progressDone = 0;
    progressTotal = 0;
    progressCurrent = "Ranking candidates…";
    try {
      const res = await invoke<{
        reply: string;
        brief: CreateModeBrief;
        draft: PackDraft;
        pillarStatus?: typeof pillarStatus;
        partial?: boolean;
        stopReason?: string;
        launcherScore?: number;
        curation?: CurationPersist;
      }>("rank_pack_draft", {
        path: $projectPath,
        brief: { ...brief, targetCount },
        draft,
        note: input.trim() || null,
      });
      brief = res.brief;
      draft = res.draft;
      applyCurationPersist(
        res.curation ?? {
          pillarStatus: res.pillarStatus,
          partial: res.partial,
          stopReason: res.stopReason,
          launcherScore: res.launcherScore,
        },
      );
      installPreviewItems = [];
      messages = [
        ...messages,
        { role: "assistant", content: res.reply },
        {
          role: "system",
          content: `Ranked draft: ${draft.mods.length} mods` +
            (lastCurateStop ? ` · stop=${lastCurateStop}` : ""),
        },
      ];
      await persistDraft();
      toasts.success(`Ranked: ${draft.mods.length} mods`);
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busy = false;
      busyKind = "";
      phase = "";
      progressCurrent = "";
      progressDone = 0;
      progressTotal = 0;
    }
  }

  async function curateWithAi() {
    if (!$projectPath || !brief || !draft?.mods?.length || busy) return;
    busy = true;
    busyKind = "curate";
    phase = "curate";
    progressDone = 0;
    progressTotal = maxCurateIterations;
    progressCurrent = "Curating with pillars + co-occurrence…";
    lastCuratePartial = false;
    lastCurateStop = "";
    lastLauncherScore = null;
    try {
      const res = await invoke<{
        reply: string;
        brief: CreateModeBrief;
        draft: PackDraft;
        pillarStatus?: typeof pillarStatus;
        partial?: boolean;
        stopReason?: string;
        launcherScore?: number;
        tier?: string;
        curation?: CurationPersist;
      }>("curate_pack_loop", {
        path: $projectPath,
        brief: { ...brief, targetCount },
        draft,
        note: input.trim() || null,
        userGoal: input.trim() || brief.title || null,
        maxIterations: maxCurateIterations,
      });
      brief = res.brief;
      draft = res.draft;
      applyCurationPersist(
        res.curation ?? {
          pillarStatus: res.pillarStatus,
          partial: res.partial,
          stopReason: res.stopReason,
          launcherScore: res.launcherScore,
          tier: res.tier,
        },
      );
      installPreviewItems = [];
      const unmet = pillarStatus.filter((p) => p.priority === 1 && !p.covered);
      const score =
        typeof lastCuration?.launcherScore === "number"
          ? lastCuration.launcherScore
          : typeof res.launcherScore === "number"
            ? res.launcherScore
            : null;
      messages = [
        ...messages,
        { role: "assistant", content: res.reply },
        {
          role: "system",
          content:
            `Curated draft: ${draft.mods.length} mods` +
            (res.tier ? ` · ${res.tier}` : "") +
            (score != null ? ` · score ${score.toFixed(2)}` : "") +
            (lastCurateStop ? ` · stop=${lastCurateStop}` : "") +
            (unmet.length
              ? `\nGameplay pillars incomplete: ${unmet.map((u) => u.label).join(", ")}`
              : ""),
        },
      ];
      await persistDraft();
      if (lastCurateStop === "ai_down") {
        toasts.warning("AI unavailable — try Quick assemble or check AI settings");
      } else if (lastCuratePartial) {
        toasts.info(
          unmet.length
            ? `Partial: pillars incomplete (${unmet.map((u) => u.label).join(", ")})`
            : "Partial curation — review draft before install",
        );
      } else {
        toasts.success(`Curated: ${draft.mods.length} mods`);
      }
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busy = false;
      busyKind = "";
      phase = "";
      progressCurrent = "";
      progressDone = 0;
      progressTotal = 0;
    }
  }

  async function cancelCurate() {
    try {
      await invoke("cancel_curate_pack_loop");
      toasts.info("Stopping curation after current step…");
    } catch (e) {
      toasts.error(String(e));
    }
  }

  async function previewDraft() {
    if (!$projectPath || !draft || busy) return;
    busy = true;
    busyKind = "preview";
    phase = "preview";
    progressCurrent = "Resolving versions & hashes…";
    progressDone = 0;
    progressTotal = 0;
    try {
      const res = await invoke<{
        checked: number;
        ok: number;
        skip?: number;
        failures: { slug: string; error: string }[];
        items?: typeof installPreviewItems;
      }>("preview_pack_draft", {
        path: $projectPath,
        draft,
        sampleLimit: Math.min(80, draft.mods.length),
      });
      installPreviewItems = res.items ?? [];
      const failN = res.failures?.length ?? 0;
      const skipN = res.skip ?? 0;
      messages = [
        ...messages,
        {
          role: "system",
          content: `Install preview: ${res.ok} ok` +
            (skipN ? `, ${skipN} already installed` : "") +
            (failN ? `, ${failN} failed` : "") +
            ` (of ${res.checked})`,
        },
      ];
      if (failN) toasts.error(`${failN} mods failed preview`);
      else toasts.success("Install preview OK");
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busy = false;
      busyKind = "";
      phase = "";
      progressCurrent = "";
    }
  }

  async function confirmInstall() {
    if (!$projectPath || !draft?.mods?.length || busy) return;
    draftSelected = Object.fromEntries(
      draft.mods.map((m) => [m.projectId || m.slug, true]),
    );
    // Refresh install preview before showing confirm (launcher-authored versions/hashes).
    if (!installPreviewItems.length) {
      try {
        busy = true;
        busyKind = "preview";
        phase = "preview";
        progressCurrent = "Resolving versions…";
        const res = await invoke<{ items?: typeof installPreviewItems }>("preview_pack_draft", {
          path: $projectPath,
          draft,
          sampleLimit: Math.min(80, draft.mods.length),
        });
        installPreviewItems = res.items ?? [];
      } catch {
        /* still allow confirm without preview rows */
      } finally {
        busy = false;
        busyKind = "";
        phase = "";
        progressCurrent = "";
      }
    }
    draftConfirmOpen = true;
  }

  async function executeDraftInstall() {
    if (!$projectPath || !draft?.mods?.length || busy) return;
    const mods = draft.mods.filter((m) => draftSelected[m.projectId || m.slug] !== false);
    if (!mods.length) {
      toasts.error("Select at least one mod to install");
      return;
    }
    draftConfirmOpen = false;
    const n = mods.length;
    const filteredDraft = { ...draft, mods };
    busy = true;
    busyKind = "install";
    phase = "install";
    progressDone = 0;
    progressTotal = n;
    progressCurrent = "Installing...";
    postInstallTrail = false;
    try {
      const res = await invoke<{ installedCount: number; requested: number }>(
        "install_pack_draft",
        {
          path: $projectPath,
          draft: filteredDraft,
          confirmed: true,
          side: "both",
        },
      );
      messages = [
        ...messages,
        {
          role: "system",
          content: `Installed ${res.installedCount} of ${res.requested} requested mods (deps may add more). Snapshot was created first.`,
        },
      ];
      await persistDraft();
      lastInstallCount = res.installedCount;
      postInstallTrail = true;
      toasts.success(`Installed ${res.installedCount} mods`);
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busy = false;
      busyKind = "";
      phase = "";
      progressCurrent = "";
      progressDone = 0;
      progressTotal = 0;
    }
  }

  function openMods() {
    ideStageRequest.set("content");
    currentView = "ide";
  }

  function openResolve() {
    ideStageRequest.set("resolve");
    currentView = "ide";
  }

  onMount(async () => {
    unlisten = await listen<{
      phase: string;
      done: number;
      total: number;
      current: string;
    }>("create-mode://progress", (ev) => {
      phase = ev.payload.phase;
      progressDone = ev.payload.done;
      progressTotal = ev.payload.total;
      progressCurrent = ev.payload.current;
    });
    try {
      const s = await invoke<{ potatoPc?: boolean }>("get_launcher_settings");
      if (s?.potatoPc) maxCurateIterations = 3;
    } catch {
      /* keep default 5 */
    }
  });

  onDestroy(() => {
    unlisten?.();
  });

  $effect(() => {
    const p = $projectPath ?? "";
    if (p !== lastPath) {
      lastPath = p;
      activeId = null;
      messages = [];
      draft = null;
      brief = null;
      candidates = [];
      clearCurationPersist();
      if (p) {
        void refreshSessions().then(() => {
          if (sessions[0]) void selectSession(sessions[0].id, sessions[0].kind);
        });
      } else {
        sessions = [];
      }
    }
  });
</script>

{#if !$projectPath}
  <div class="chats empty">
    <MessagesSquare size={40} strokeWidth={1.5} />
    <h2>Create Mode</h2>
    <p>Open an instance to plan a CreateModeBrief (Intent → Catalog → Rank → Install preview) with AI.</p>
  </div>
{:else}
  <div class="chats">
    <aside class="sessions">
      <div class="sessions-head">
        <span>Chats</span>
        <div class="sessions-head-actions">
          <button type="button" class="icon-btn" title="New Tune chat" onclick={newTuneChat}>
            <span class="head-tune">T</span>
          </button>
          <button type="button" class="icon-btn" title="New Create chat" onclick={newChat}>
            <Plus size={16} />
          </button>
        </div>
      </div>
      <div class="session-list">
        {#each sessions as s (`${s.kind}-${s.id}`)}
          <div
            class="session-row"
            class:active={s.id === activeId && s.kind === activeKind}
            class:quest={s.kind === "quest"}
            class:tune={s.kind === "tune"}
          >
            <button type="button" class="session-main" onclick={() => selectSession(s.id, s.kind)}>
              <span class="session-title-row">
                {#if s.kind === "quest"}
                  <span class="kind-badge quest">Quests</span>
                {:else if s.kind === "tune"}
                  <span class="kind-badge tune">Tune</span>
                {:else}
                  <span class="kind-badge create">Create</span>
                {/if}
                <span class="session-title">{s.title || "Untitled"}</span>
              </span>
              <span class="session-meta">
                {#if timeAgo(s.updatedAt)}<span class="session-time">{timeAgo(s.updatedAt)}</span>{/if}
                {#if lastMessagePreview(s)}<span class="session-preview">{lastMessagePreview(s)}</span>{/if}
              </span>
            </button>
            <button
              type="button"
              class="icon-btn danger"
              title="Delete"
              onclick={() => deleteChat(s.id, s.kind)}
            >
              <Trash2 size={14} />
            </button>
          </div>
        {:else}
          <p class="muted pad">No chats yet. Start one below.</p>
        {/each}
      </div>
    </aside>

    <section class="thread" class:quest-thread={isQuestChat} class:tune-thread={isTuneChat}>
      <div class="thread-meta" class:quest={isQuestChat} class:tune={isTuneChat}>
        <Sparkles size={16} />
        {#if isQuestChat}
          <span>Quest AI · FTB Quests</span>
          {#if activeId}
              <button type="button" class="btn ghost mini quest-open" onclick={() => activeId && openQuestChatInEditor(activeId)}>
              Open in Quests
            </button>
          {/if}
          {#if questPendingPlan}
            <span class="pending-dot" title="Pending plan — Apply in Quest editor">plan ready</span>
          {/if}
        {:else if isTuneChat}
          <span>Tune · Config AI</span>
          {#if activeId}
            <button type="button" class="btn ghost mini quest-open" onclick={() => activeId && openTuneChatInEditor(activeId)}>
              Open in Tune
            </button>
          {/if}
          {#if tunePendingAdvise}
            <span class="pending-dot" title="Pending config plan — Review in Tune">plan ready</span>
          {/if}
        {:else}
          <span>Create Mode · {mcLabel} / {loaderLabel}</span>
          <label class="target">
            Target mods
            <input
              type="number"
              min="40"
              max="120"
              step="1"
              bind:value={targetCount}
              disabled={busy}
              onchange={clampTargetCount}
              onblur={clampTargetCount}
            />
          </label>
        {/if}
      </div>

      <div class="messages">
        {#if messages.length === 0}
          <div class="welcome">
            {#if isQuestChat}
              <h3>Quest AI chat</h3>
              <p>
                This session was created in the Quests editor. Continue generating lore and quest lines there —
                Apply updates the editor, Save writes SNBT.
              </p>
            {:else if isTuneChat}
              <h3>Tune Config AI</h3>
              <p>
                Ask for FPS / server / compat config patches. Review → Apply lives in the Tune tab
                (snapshot first). You can also continue from here.
              </p>
            {:else}
              <h3>Describe the pack you want</h3>
              <p>
                Example: "Tech + airplanes for NeoForge 1.21.1, ~80 mods, Create required."
                Plan builds a CreateModeBrief (search JSON + must-haves from hub co-occurrence).
                Quick assemble skips Rank (Intent → Catalog → Confirm). Install only after you confirm.
              </p>
            {/if}
          </div>
        {/if}
        {#each messages as m, i (m.createdAt ?? `${m.role}-${i}-${m.content.slice(0, 48)}`)}
          <div class="bubble" class:user={m.role === "user"} class:assistant={m.role === "assistant"} class:system={m.role === "system"} class:quest={isQuestChat} class:tune={isTuneChat}>
            {m.content}
          </div>
        {/each}
        {#if busy && phase}
          <div class="bubble system progress" aria-live="polite">
            <span class="spin"><Loader2 size={14} /></span>
            {progressHeadline}
            {#if progressTotal > 0}
              ({progressDone}/{progressTotal})
            {/if}
          </div>
        {/if}
      </div>

      {#if isQuestChat}
        <div class="composer quest-composer">
          <p class="quest-hint">
            Quest AI editing lives in the Quests tab (Apply → Save). Open the session there to continue.
          </p>
          <div class="actions">
            <button
              type="button"
              class="btn accent quest-cta"
              disabled={!activeId}
              onclick={() => activeId && openQuestChatInEditor(activeId)}
            >
              <Sparkles size={14} /> Continue in Quests
            </button>
          </div>
        </div>
      {:else if isTuneChat}
        <div class="composer quest-composer tune-composer">
          <textarea
            rows="2"
            placeholder="e.g. lower render distance, optimize sodium…"
            bind:value={input}
            disabled={busy}
            onkeydown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void sendTuneMessage();
              }
            }}
          ></textarea>
          <div class="actions">
            <button
              type="button"
              class="btn primary"
              disabled={busy || !input.trim()}
              onclick={() => void sendTuneMessage()}
            >
              {#if busy}
                <span class="spin"><Loader2 size={14} /></span> Advising…
              {:else}
                <Send size={14} /> Ask
              {/if}
            </button>
            <button
              type="button"
              class="btn accent quest-cta"
              disabled={!activeId}
              onclick={() => activeId && openTuneChatInEditor(activeId)}
            >
              <Sparkles size={14} /> Open in Tune
            </button>
          </div>
        </div>
      {:else}
      <div class="composer">
        <textarea
          rows="2"
          placeholder="Pack brief..."
          bind:value={input}
          disabled={busy}
          onkeydown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void sendMessage(false);
            }
          }}
        ></textarea>
        <div class="actions">
          <button type="button" class="btn primary" disabled={busy || !input.trim()} onclick={() => sendMessage(false)} title="Enter">
            {#if busyKind === "plan"}
              <span class="spin"><Loader2 size={14} /></span> Planning…
            {:else}
              <Send size={14} /> Plan
            {/if}
          </button>

          {#if contextualNext === "build"}
            <button type="button" class="btn accent" disabled={busy || !brief} onclick={buildDraft}>
              {#if busyKind === "build"}
                <span class="spin"><Loader2 size={14} /></span> Catalog…
              {:else}
                <Package size={14} /> Build draft
              {/if}
            </button>
          {:else if contextualNext === "rank"}
            <button type="button" class="btn accent" disabled={busy || !draft?.mods?.length} onclick={() => void rankWithAi()}>
              {#if busyKind === "rank"}
                <span class="spin"><Loader2 size={14} /></span> Ranking…
              {:else}
                <Sparkles size={14} /> Rank
              {/if}
            </button>
          {:else if contextualNext === "curate"}
            <button type="button" class="btn accent" disabled={busy || !draft?.mods?.length} onclick={() => void curateWithAi()}>
              {#if busyKind === "curate"}
                <span class="spin"><Loader2 size={14} /></span> Curating…
              {:else}
                <Sparkles size={14} /> Curate
              {/if}
            </button>
          {/if}

          {#if draft?.mods?.length}
            <button type="button" class="btn accent install-confirm" disabled={busy} onclick={confirmInstall}>
              <CheckCircle2 size={14} /> Confirm install
            </button>
          {/if}

          {#if busyKind === "curate"}
            <button type="button" class="btn ghost" onclick={() => void cancelCurate()}>Stop</button>
          {/if}

          <div class="more-wrap">
            <button
              type="button"
              class="btn ghost more-toggle"
              disabled={busy && busyKind !== "curate"}
              aria-expanded={moreMenuOpen}
              onclick={() => (moreMenuOpen = !moreMenuOpen)}
            >
              <MoreHorizontal size={14} /> More
            </button>
            {#if moreMenuOpen}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="more-backdrop" onclick={() => (moreMenuOpen = false)} onkeydown={() => {}}></div>
              <div class="more-menu" role="menu">
                <button type="button" class="more-item" disabled={busy || !input.trim()} onclick={() => { moreMenuOpen = false; void quickAssemble(); }}>
                  {#if busyKind === "quick"}<span class="spin"><Loader2 size={13} /></span>{/if}
                  Quick assemble
                </button>
                <button type="button" class="more-item" disabled={busy || !brief || !input.trim()} onclick={() => { moreMenuOpen = false; sendMessage(true); }}>
                  {#if busyKind === "refine"}<span class="spin"><Loader2 size={13} /></span>{/if}
                  Refine
                </button>
                {#if contextualNext !== "rank"}
                  <button type="button" class="more-item" disabled={busy || !draft?.mods?.length} onclick={() => { moreMenuOpen = false; void rankWithAi(); }}>
                    {#if busyKind === "rank"}<span class="spin"><Loader2 size={13} /></span>{/if}
                    Rank with AI
                  </button>
                {/if}
                {#if contextualNext !== "curate"}
                  <div class="more-curate-row">
                    <button type="button" class="more-item flex" disabled={busy || !draft?.mods?.length} onclick={() => { moreMenuOpen = false; void curateWithAi(); }}>
                      {#if busyKind === "curate"}<span class="spin"><Loader2 size={13} /></span>{/if}
                      Curate
                    </button>
                    <label class="curate-iters compact" title="Curation loop iterations">
                      <span>iters</span>
                      <input type="number" min="1" max="8" bind:value={maxCurateIterations} disabled={busy} onclick={(e) => e.stopPropagation()} />
                    </label>
                  </div>
                {:else}
                  <label class="more-item curate-iters inline" title="Curation loop iterations">
                    <span>Curate iters</span>
                    <input type="number" min="1" max="8" bind:value={maxCurateIterations} disabled={busy} />
                  </label>
                {/if}
                <button type="button" class="more-item" disabled={busy || !draft?.mods?.length} onclick={() => { moreMenuOpen = false; previewDraft(); }}>
                  {#if busyKind === "preview"}<span class="spin"><Loader2 size={13} /></span>{/if}
                  Install preview
                </button>
                <button type="button" class="more-item" onclick={() => { moreMenuOpen = false; openMods(); }}>
                  Open in Content
                </button>
              </div>
            {/if}
          </div>
        </div>
        {#if pillarStatus.length}
          <div class="pillar-checklist" class:partial={lastCuratePartial}>
            <div class="pillar-head">
              <strong>Gameplay pillars</strong>
              {#if lastCurateStop}
                <span class="stop">{lastCurateStop}</span>
              {/if}
            </div>
            <ul>
              {#each pillarStatus as p (p.id)}
                <li class:covered={p.covered} class:priority={p.priority === 1}>
                  <span class="mark">{p.covered ? "✓" : "○"}</span>
                  <span class="label">{p.label}</span>
                  {#if p.priority === 1}<span class="prio">P1</span>{/if}
                </li>
              {/each}
            </ul>
            {#if lastCuratePartial}
              <p class="pillar-warn">Not pack-ready until priority-1 pillars are covered.</p>
            {/if}
          </div>
        {/if}
        {#if postInstallTrail}
          <div class="post-trail compact">
            <span>Installed {lastInstallCount} mods</span>
            <button type="button" class="btn ghost mini" onclick={openMods}><Package size={12} /> Content</button>
            <button type="button" class="btn ghost mini" onclick={openResolve}><GitGraph size={12} /> Resolve</button>
          </div>
        {/if}
      </div>
      {/if}
    </section>

    <aside class="draft" class:quest-hidden={isSideStageChat}>
      {#if isQuestChat}
        <div class="draft-head quest">
          <span>Quest AI</span>
          <strong>FTB</strong>
        </div>
        <div class="quest-side">
          <p>
            Transcript is read-only here. Open the Quests editor to Apply plans and Save SNBT.
          </p>
          <button
            type="button"
            class="btn accent quest-cta"
            disabled={!activeId}
            onclick={() => activeId && openQuestChatInEditor(activeId)}
          >
            Open Quest editor
          </button>
        </div>
      {:else}
      <div class="draft-head">
        <span>Pack draft</span>
        <strong>{draft?.mods?.length ?? 0}</strong>
      </div>

      <div class="add-mod-search">
        <div class="add-mod-input">
          <Search size={13} />
          <input
            type="text"
            placeholder="Add a specific mod…"
            bind:value={addModQuery}
            oninput={scheduleAddModSearch}
            onfocus={() => (addModOpen = true)}
            onblur={() => setTimeout(() => (addModOpen = false), 150)}
          />
          {#if addModLoading}<span class="spin"><Loader2 size={13} /></span>{/if}
        </div>
        {#if addModOpen && addModQuery.trim().length >= 2}
          <div class="add-mod-results">
            {#if !addModLoading && addModResults.length === 0}
              <div class="muted pad small">No results</div>
            {/if}
            {#each addModResults as r (r.id)}
              <button type="button" class="add-mod-row" onmousedown={(e) => { e.preventDefault(); addModDirectly(r); }}>
                <span class="mod-name">{r.name}</span>
                <code>{r.slug}</code>
              </button>
            {/each}
          </div>
        {/if}
      </div>

      {#if brief}
        <details class="brief-edit" open>
          <summary>
            <span>Brief</span>
            <span class="muted">{brief.mcVersion} · {brief.loader} · target {brief.targetCount}</span>
          </summary>
          <div class="brief-edit-body">
            <label class="field">
              Title
              <input
                type="text"
                value={brief.title}
                onchange={(e) => updateBriefTitle(e.currentTarget.value)}
              />
            </label>

            <div class="field-block">
              <div class="field-label">Must-have</div>
              <div class="chip-list">
                {#each brief.mustHave as mh, i (i)}
                  <span class="chip" class:locked={mh.reason === LOCKED_REASON}>
                    {#if mh.reason === LOCKED_REASON}<Lock size={10} />{/if}
                    {mh.query}
                    <button type="button" onclick={() => removeMustHave(i)} aria-label="Remove">
                      <X size={10} />
                    </button>
                  </span>
                {:else}
                  <span class="muted small">None yet</span>
                {/each}
              </div>
              <div class="chip-add">
                <input
                  type="text"
                  placeholder="Add must-have mod…"
                  bind:value={newMustHaveQuery}
                  onkeydown={(e) => e.key === "Enter" && addMustHave()}
                />
                <button type="button" class="btn ghost mini" onclick={addMustHave}
                  ><Plus size={12} /></button
                >
              </div>
            </div>

            <div class="field-block">
              <div class="field-label">Categories</div>
              <div class="cat-rows">
                {#each brief.categories as cat, i (i)}
                  <div class="cat-row">
                    <input
                      type="text"
                      class="cat-id"
                      list="chats-known-category-ids"
                      value={cat.id}
                      onchange={(e) => updateCategoryField(i, "id", e.currentTarget.value)}
                    />
                    <input
                      type="text"
                      class="cat-query"
                      value={cat.query}
                      onchange={(e) => updateCategoryField(i, "query", e.currentTarget.value)}
                    />
                    <div class="cat-count">
                      <button type="button" class="stepper" onclick={() => bumpCategoryCount(i, -5)}
                        >−</button
                      >
                      <span>{cat.count}</span>
                      <button type="button" class="stepper" onclick={() => bumpCategoryCount(i, 5)}
                        >+</button
                      >
                    </div>
                    <button
                      type="button"
                      class="icon-btn danger"
                      title="Remove category"
                      onclick={() => removeCategoryRow(i)}
                    >
                      <X size={12} />
                    </button>
                  </div>
                {/each}
              </div>
              <datalist id="chats-known-category-ids">
                {#each KNOWN_CATEGORY_IDS as id (id)}<option value={id}></option>{/each}
              </datalist>
              <div class="chip-add">
                <input
                  type="text"
                  placeholder="New category id…"
                  bind:value={newCategoryId}
                  onkeydown={(e) => e.key === "Enter" && addCategoryRow()}
                />
                <button type="button" class="btn ghost mini" onclick={addCategoryRow}
                  ><Plus size={12} /></button
                >
              </div>
            </div>

            <div class="field-block">
              <div class="field-label">Exclude</div>
              <div class="chip-list">
                {#each brief.exclude as ex, i (i)}
                  <span class="chip"
                    >{ex}<button type="button" onclick={() => removeExcludeEntry(i)} aria-label="Remove"
                      ><X size={10} /></button
                    ></span
                  >
                {:else}
                  <span class="muted small">None</span>
                {/each}
              </div>
              <div class="chip-add">
                <input
                  type="text"
                  placeholder="Exclude slug/name…"
                  bind:value={newExcludeText}
                  onkeydown={(e) => e.key === "Enter" && addExcludeEntry()}
                />
                <button type="button" class="btn ghost mini" onclick={addExcludeEntry}
                  ><Plus size={12} /></button
                >
              </div>
            </div>

            <button type="button" class="btn primary full" disabled={busy} onclick={buildDraft}>
              <Package size={13} /> Rebuild draft
            </button>
          </div>
        </details>
      {:else}
        <p class="muted pad">Plan a brief first, then Build draft.</p>
      {/if}
      {#if candidates.length}
        <div class="candidates">
          <div class="draft-head"><span>Catalog candidates</span><strong>{candidates.length}</strong></div>
          {#each candidates.slice(0, 12) as c (c.slug)}
            <div class="mod-row compact">
              <div class="mod-name">{c.name}</div>
              <div class="mod-meta">
                <code>{c.slug}</code>
                <span class="muted">{c.source}</span>
              </div>
            </div>
          {/each}
        </div>
      {/if}
      <div class="mod-table">
        {#if draft?.mods?.length}
          {#each draft.mods as m (m.projectId || m.slug)}
            <div class="mod-row" class:locked={isLocked(m)}>
              <div class="mod-row-head">
                <div class="mod-name">
                  {#if isLocked(m)}<span class="lock-icon"><Lock size={10} /></span>{/if}
                  {m.name}
                </div>
                <div class="mod-row-actions">
                  <button
                    type="button"
                    class="icon-btn"
                    title={isLocked(m) ? "Unlock" : "Lock (keep on rebuild)"}
                    onclick={() => toggleLock(m)}
                  >
                    {#if isLocked(m)}<Lock size={12} />{:else}<Unlock size={12} />{/if}
                  </button>
                  <button
                    type="button"
                    class="icon-btn"
                    title="Find alternative"
                    onclick={() => void openAlternatives(m)}
                  >
                    <Shuffle size={12} />
                  </button>
                  <button
                    type="button"
                    class="icon-btn danger"
                    title="Remove from draft"
                    onclick={() => removeModFromDraft(m)}
                  >
                    <X size={12} />
                  </button>
                </div>
              </div>
              <div class="mod-meta">
                <code>{m.slug}</code>
                <span class="muted">{m.category}</span>
              </div>
              <div class="mod-reason muted">{m.reason}</div>
              {#if altForKey === (m.projectId || m.slug)}
                <div class="alt-popover">
                  {#if altLoading}
                    <div class="muted pad small"><Loader2 size={12} class="spin" /> Searching…</div>
                  {:else if altOptions.length === 0}
                    <div class="muted pad small">No alternatives found.</div>
                  {:else}
                    {#each altOptions as opt (opt.slug)}
                      <button
                        type="button"
                        class="alt-row"
                        onclick={() => void applyAlternative(m, opt)}
                      >
                        <span class="mod-name">{opt.name}</span>
                        <code>{opt.slug}</code>
                        <span class="muted alt-source">{opt.source}</span>
                      </button>
                    {/each}
                  {/if}
                </div>
              {/if}
            </div>
          {/each}
        {/if}
      </div>
      {#if draft?.unresolved?.length}
        <div class="unresolved">
          Must-have unresolved: {draft.unresolved.join(", ")}
        </div>
      {/if}
      {/if}
    </aside>
  </div>
{/if}

{#if draftConfirmOpen && draft}
  <div
    class="modal-backdrop"
    role="button"
    tabindex="-1"
    onclick={() => (draftConfirmOpen = false)}
    onkeydown={() => {}}
  >
    <div class="modal draft-confirm" role="dialog" aria-modal="true">
      <div class="draft-confirm-head">
        <h2>Confirm pack install</h2>
        <p>
          Install <strong>{draftConfirmCount}</strong> of {draft.mods.length} mods (+ dependencies) into this instance.
          A snapshot will be created first. Uncheck anything you do not want.
          Versions and hashes below are resolved by the launcher (not the AI).
        </p>
      </div>
      <div class="draft-confirm-list">
        {#each draft.mods as m (m.projectId || m.slug)}
          {@const prev = installPreviewItems.find((i) => (i.projectId || i.slug) === (m.projectId || m.slug))}
          <label class="draft-confirm-row">
            <input
              type="checkbox"
              checked={draftSelected[m.projectId || m.slug] !== false}
              onchange={(e) => {
                draftSelected[m.projectId || m.slug] = e.currentTarget.checked;
                draftSelected = { ...draftSelected };
              }}
            />
            <div>
              <strong>{m.name}</strong>
              <code>{m.slug}</code>
              <span class="muted">{m.category}</span>
              {#if prev}
                <div class="preview-meta">
                  {#if prev.status === "skip"}
                    <span class="pill skip">Already installed</span>
                  {:else if prev.status === "fail"}
                    <span class="pill fail">{prev.error ?? "No version"}</span>
                  {:else}
                    <span class="pill ok">{prev.version ?? "?"}</span>
                    {#if prev.hashAlgo && prev.hash}
                      <code class="hash">{prev.hashAlgo}:{prev.hash.slice(0, 12)}…</code>
                    {/if}
                    {#if prev.destPath}
                      <span class="muted dest">{prev.destPath}</span>
                    {/if}
                  {/if}
                </div>
              {/if}
            </div>
          </label>
        {/each}
      </div>
      {#if draft.unresolved?.length}
        <p class="warn">Unresolved must-haves: {draft.unresolved.join(", ")}</p>
      {/if}
      <div class="draft-confirm-actions">
        <button type="button" class="btn ghost" onclick={() => (draftConfirmOpen = false)}>Cancel</button>
        <button
          type="button"
          class="btn accent"
          disabled={busy || draftConfirmCount === 0}
          onclick={executeDraftInstall}
        >
          Install {draftConfirmCount} mod{draftConfirmCount === 1 ? "" : "s"} (snapshot first)
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .chats {
    display: grid;
    grid-template-columns: 200px minmax(0, 1fr) 280px;
    gap: 0;
    /* Fill the fill-view pane — avoid 100vh (fights header / UI scale). */
    height: 100%;
    min-height: 0;
    /* Responsive: centered cap on very wide screens (1440p+). */
    max-width: min(1720px, 100%);
    margin: 0 auto;
    background: var(--bg-secondary);
    overflow: hidden;
  }
  .chats.empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    color: var(--text-secondary, #9aa3b5);
    padding: 48px;
  }
  .chats.empty h2 {
    margin: 0;
    color: var(--text-primary, #e8ecf4);
  }
  .sessions,
  .draft {
    background: var(--bg-primary);
    border-right: 1px solid var(--border-color, #2a2f3a);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .draft {
    border-right: none;
    border-left: 1px solid var(--border-color, #2a2f3a);
  }
  .sessions-head,
  .draft-head,
  .thread-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border-color, #2a2f3a);
    font-size: 13px;
    color: var(--text-secondary, #9aa3b5);
  }
  .sessions-head {
    justify-content: space-between;
  }
  .sessions-head-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .head-tune {
    display: inline-flex;
    width: 16px;
    height: 16px;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    font-weight: 800;
    color: var(--accent-primary);
  }
  .draft-head {
    justify-content: space-between;
  }
  .thread-meta.quest {
    color: var(--ftbq-title-gold, #f2c94c);
    border-bottom-color: var(--ftbq-accent-teal, #3db8a8);
  }
  .thread-meta.tune {
    color: var(--accent-primary);
    border-bottom-color: color-mix(in srgb, var(--accent-primary) 45%, transparent);
  }
  .pending-dot {
    margin-left: auto;
    font-size: 12px;
    font-weight: 700;
    color: var(--ftbq-accent-teal, #3db8a8);
    border: 1px solid var(--ftbq-accent-teal, #3db8a8);
    border-radius: 2px;
    padding: 2px 6px;
  }
  .bubble.quest.assistant {
    border-left: 2px solid var(--ftbq-accent-teal, #3db8a8);
  }
  .quest-composer,
  .quest-side {
    padding: 12px 14px;
    border-top: 1px solid var(--border-color, #2a2f3a);
  }
  .quest-side {
    border-top: none;
    display: flex;
    flex-direction: column;
    gap: 12px;
    color: var(--text-secondary, #9aa3b5);
    font-size: 13px;
  }
  .quest-hint {
    margin: 0 0 8px;
    font-size: 12px;
    color: var(--text-secondary, #9aa3b5);
  }
  .btn.quest-cta,
  .btn.accent.quest-cta {
    background: rgba(61, 184, 168, 0.2);
    border-color: var(--ftbq-accent-teal, #3db8a8);
    color: var(--ftbq-title-gold, #f2c94c);
  }
  .draft-head.quest {
    color: var(--ftbq-title-gold, #f2c94c);
  }
  .draft-head.quest strong {
    color: var(--ftbq-accent-teal, #3db8a8);
  }
  .draft-head strong {
    color: var(--text-primary, #e8ecf4);
  }
  .quest-open {
    margin-left: 8px;
  }
  .session-list,
  .mod-table,
  .messages {
    flex: 1;
    overflow: auto;
    min-height: 0;
    scrollbar-gutter: stable;
  }
  .session-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 2px 6px;
  }
  .session-row.active {
    background: var(--bg-tertiary);
  }
  .session-row.quest {
    border-left: 3px solid var(--ftbq-accent-teal, #3db8a8);
  }
  .session-row.quest.active {
    background: rgba(61, 184, 168, 0.12);
  }
  .session-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .kind-badge {
    flex-shrink: 0;
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    padding: 1px 5px;
    border-radius: 2px;
  }
  .kind-badge.create {
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 25%, transparent);
  }
  .kind-badge.quest {
    color: var(--ftbq-title-gold, #f2c94c);
    background: rgba(61, 184, 168, 0.15);
    border: 1px solid var(--ftbq-accent-teal, #3db8a8);
  }
  .kind-badge.tune {
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }
  .bubble.tune.assistant {
    border-left: 2px solid var(--accent-primary);
  }
  .tune-composer textarea {
    width: 100%;
    resize: none;
    font: inherit;
    font-size: 13px;
    padding: 8px 10px;
    margin-bottom: 8px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
    color: var(--text-primary);
    box-sizing: border-box;
  }
  .session-main {
    flex: 1;
    text-align: left;
    background: none;
    border: none;
    color: var(--text-primary, #e8ecf4);
    padding: 8px;
    cursor: pointer;
    font-size: 13px;
  }
  .session-title {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .session-meta {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 2px;
  }
  .session-time {
    font-size: 12px;
    color: var(--text-secondary, #9aa3b5);
  }
  .session-preview {
    display: block;
    font-size: 12px;
    color: var(--text-secondary, #9aa3b5);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .icon-btn {
    background: transparent;
    border: none;
    color: var(--text-secondary, #9aa3b5);
    cursor: pointer;
    padding: 6px;
    border-radius: 6px;
  }
  .icon-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary, #e8ecf4);
  }
  .icon-btn.danger:hover {
    color: var(--accent-danger);
  }
  .thread {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .thread-meta {
    flex-wrap: wrap;
  }
  .target {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }
  .target input {
    width: 4.5rem;
    padding: 4px 6px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color, #2a2f3a);
    background: var(--bg-primary);
    color: var(--text-primary, #e8ecf4);
    font: inherit;
    font-size: 12px;
  }
  .messages {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .welcome {
    max-width: 520px;
    color: var(--text-secondary, #9aa3b5);
  }
  .welcome h3 {
    margin: 0 0 8px;
    color: var(--text-primary, #e8ecf4);
    font-weight: 600;
  }
  .bubble {
    max-width: 85%;
    padding: 10px 12px;
    border-radius: 10px;
    font-size: 13px;
    line-height: 1.45;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    word-break: break-word;
    min-width: 0;
  }
  .bubble.user {
    align-self: flex-end;
    background: var(--bg-elevated);
    color: var(--text-primary, #e8ecf4);
  }
  .bubble.assistant {
    align-self: flex-start;
    background: var(--bg-tertiary);
    color: var(--text-primary, #e8ecf4);
  }
  .bubble.system {
    align-self: center;
    background: transparent;
    color: var(--text-secondary, #9aa3b5);
    font-size: 12px;
    border: 1px dashed var(--border-color, #2a2f3a);
  }
  .candidates {
    border-top: 1px solid var(--border-color, #2a2f3a);
  }
  .mod-row.compact {
    padding-top: 4px;
    padding-bottom: 4px;
  }
  .bubble.progress {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .composer {
    border-top: 1px solid var(--border-color, #2a2f3a);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .composer textarea {
    width: 100%;
    resize: vertical;
    min-height: 56px;
    background: var(--bg-primary);
    color: var(--text-primary, #e8ecf4);
    border: 1px solid var(--border-color, #2a2f3a);
    border-radius: var(--border-radius-sm);
    padding: 10px;
    font: inherit;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  .more-wrap {
    position: relative;
    margin-left: auto;
  }
  .more-backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
  }
  .more-menu {
    position: absolute;
    right: 0;
    bottom: calc(100% + 4px);
    z-index: 50;
    min-width: 180px;
    background: var(--bg-secondary, #151922);
    border: 1px solid var(--border-color, #2a2f3a);
    border-radius: var(--border-radius-sm);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .more-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    padding: 7px 10px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text-primary, #e8ecf4);
    font-size: 12px;
    cursor: pointer;
  }
  .more-item:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .more-item:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .more-item.flex {
    flex: 1;
  }
  .more-curate-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .curate-iters.compact {
    padding: 0 6px 0 0;
    flex-shrink: 0;
  }
  .curate-iters.inline {
    justify-content: space-between;
    cursor: default;
  }
  .curate-iters.inline input {
    width: 44px;
  }
  .post-trail.compact {
    margin-top: 4px;
    padding: 4px 0 0;
    font-size: 12px;
    gap: 8px;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    border: 1px solid var(--border-color, #2a2f3a);
    background: var(--bg-tertiary);
    color: var(--text-primary, #e8ecf4);
    border-radius: var(--border-radius-sm);
    padding: 7px 10px;
    font-size: 12px;
    cursor: pointer;
  }
  .btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .btn.primary {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
    color: var(--on-accent, #0b1a10);
    font-weight: 600;
  }
  .btn.accent {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
    color: var(--on-accent, #000);
  }
  .btn.ghost {
    background: transparent;
  }
  .add-mod-search {
    position: relative;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border-color, #2a2f3a);
  }
  .add-mod-input {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg-primary);
    border: 1px solid var(--border-color, #2a2f3a);
    border-radius: var(--border-radius-sm);
    padding: 6px 8px;
    color: var(--text-secondary, #9aa3b5);
  }
  .add-mod-input input {
    flex: 1;
    background: transparent;
    border: none;
    color: var(--text-primary, #e8ecf4);
    font: inherit;
    outline: none;
  }
  .add-mod-results {
    position: absolute;
    left: 14px;
    right: 14px;
    top: 100%;
    z-index: 20;
    background: var(--bg-secondary, #151922);
    border: 1px solid var(--border-color, #2a2f3a);
    border-radius: var(--border-radius-sm);
    max-height: 220px;
    overflow: auto;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
  }
  .add-mod-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border-color, #2a2f3a);
    padding: 8px 10px;
    cursor: pointer;
    color: var(--text-primary, #e8ecf4);
  }
  .add-mod-row:hover {
    background: var(--bg-hover);
  }
  .brief-edit {
    border-bottom: 1px solid var(--border-color, #2a2f3a);
  }
  .brief-edit summary {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    cursor: pointer;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary, #e8ecf4);
    list-style: none;
  }
  .brief-edit summary::-webkit-details-marker {
    display: none;
  }
  .brief-edit-body {
    padding: 0 14px 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    max-height: 320px;
    overflow-y: auto;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 12px;
    color: var(--text-secondary, #9aa3b5);
  }
  .field input[type="text"] {
    background: var(--bg-primary);
    color: var(--text-primary, #e8ecf4);
    border: 1px solid var(--border-color, #2a2f3a);
    border-radius: var(--border-radius-sm);
    padding: 6px 8px;
    font: inherit;
    font-size: 13px;
  }
  .field-block {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .field-label {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--text-secondary, #9aa3b5);
  }
  .chip-list {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    background: var(--bg-tertiary);
    color: var(--text-secondary, #9aa3b5);
    padding: 3px 6px;
    border-radius: 999px;
    border: 1px solid var(--border-color, #2a2f3a);
  }
  .chip.locked {
    color: var(--accent-primary);
    border-color: rgba(52, 211, 153, 0.35);
  }
  .chip button {
    display: inline-flex;
    background: none;
    border: none;
    color: inherit;
    cursor: pointer;
    padding: 0;
  }
  .chip-add {
    display: flex;
    gap: 8px;
  }
  .chip-add input {
    flex: 1;
    background: var(--bg-primary);
    color: var(--text-primary, #e8ecf4);
    border: 1px solid var(--border-color, #2a2f3a);
    border-radius: var(--border-radius-sm);
    padding: 5px 8px;
    font-size: 12px;
    font: inherit;
  }
  .cat-rows {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .cat-row {
    display: grid;
    grid-template-columns: 84px minmax(0, 1fr) auto auto;
    gap: 8px;
    align-items: center;
  }
  .cat-row input {
    background: var(--bg-primary);
    color: var(--text-primary, #e8ecf4);
    border: 1px solid var(--border-color, #2a2f3a);
    border-radius: var(--border-radius-sm);
    padding: 4px 6px;
    font-size: 12px;
    font: inherit;
    min-width: 0;
  }
  .cat-count {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-primary, #e8ecf4);
  }
  .stepper {
    width: 18px;
    height: 18px;
    line-height: 1;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color, #2a2f3a);
    border-radius: 4px;
    color: var(--text-primary, #e8ecf4);
    cursor: pointer;
    font-size: 12px;
  }
  .btn.full {
    width: 100%;
    justify-content: center;
  }
  .mod-row {
    padding: 8px 12px;
    border-bottom: 1px solid var(--border-color, #2a2f3a);
    position: relative;
  }
  .mod-row.locked {
    background: rgba(52, 211, 153, 0.05);
  }
  .mod-row-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .mod-row-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }
  .mod-row-actions .icon-btn {
    padding: 4px;
  }
  .mod-name {
    font-size: 13px;
    color: var(--text-primary, #e8ecf4);
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .lock-icon {
    display: inline-flex;
    align-items: center;
    color: var(--accent-primary);
    flex-shrink: 0;
  }
  .mod-meta {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 12px;
  }
  .mod-meta code {
    font-size: 12px;
    color: #0284c7;
  }
  .mod-reason {
    font-size: 12px;
    margin-top: 2px;
  }
  .alt-popover {
    margin-top: 6px;
    border: 1px solid var(--border-color, #2a2f3a);
    border-radius: var(--border-radius-sm);
    background: var(--bg-primary);
    overflow: hidden;
  }
  .alt-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border-color, #2a2f3a);
    padding: 6px 8px;
    cursor: pointer;
    color: var(--text-primary, #e8ecf4);
    font-size: 12px;
  }
  .alt-row:last-child {
    border-bottom: none;
  }
  .alt-row:hover {
    background: var(--bg-hover);
  }
  .alt-source {
    margin-left: auto;
    font-size: 12px;
    white-space: nowrap;
  }
  .small {
    font-size: 12px;
  }
  .unresolved {
    padding: 10px 12px;
    font-size: 12px;
    color: var(--accent-warning);
    border-top: 1px solid var(--border-color, #2a2f3a);
  }
  .muted {
    color: var(--text-secondary, #9aa3b5);
  }
  .pad {
    padding: 12px;
    font-size: 13px;
  }
  .spin {
    display: inline-flex;
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .post-trail {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-secondary, #9aa3b5);
  }
  .curate-iters {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-secondary, #9aa3b5);
  }
  .curate-iters input {
    width: 44px;
    padding: 4px 6px;
    border-radius: 6px;
    border: 1px solid var(--border-color, #2a2f3a);
    background: var(--bg-tertiary, #1a1f2a);
    color: inherit;
  }
  .pillar-checklist {
    margin-top: 10px;
    padding: 10px 12px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color, #2a2f3a);
    background: var(--bg-tertiary, #1a1f2a);
    font-size: 12px;
  }
  .pillar-checklist.partial {
    border-color: color-mix(in srgb, #c9a227 55%, var(--border-color, #2a2f3a));
  }
  .pillar-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 6px;
  }
  .pillar-head .stop {
    opacity: 0.7;
    font-size: 12px;
  }
  .pillar-checklist ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .pillar-checklist li {
    display: flex;
    align-items: center;
    gap: 8px;
    opacity: 0.75;
  }
  .pillar-checklist li.covered {
    opacity: 1;
  }
  .pillar-checklist li .mark {
    width: 1em;
    text-align: center;
  }
  .pillar-checklist li .prio {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
    padding: 1px 5px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--accent-secondary) 25%, transparent);
  }
  .pillar-warn {
    margin: 8px 0 0;
    color: #c9a227;
  }
  .btn.mini { padding: 4px 8px; font-size: 12px; }
  .modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 90;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.55);
    padding: 16px;
  }
  .draft-confirm {
    width: min(520px, 100%);
    max-height: 85vh;
    overflow: auto;
    padding: 16px 18px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color, #2a2f3a);
    background: var(--bg-secondary, #151922);
  }
  .draft-confirm-head h2 { margin: 0 0 6px; font-size: 16px; }
  .draft-confirm-head p { margin: 0 0 12px; font-size: 13px; color: var(--text-secondary, #9aa3b5); }
  .draft-confirm-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 40vh;
    overflow: auto;
    margin-bottom: 12px;
  }
  .draft-confirm-row {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 8px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color, #2a2f3a);
    cursor: pointer;
  }
  .draft-confirm-row strong { display: block; font-size: 13px; }
  .draft-confirm-row code { font-size: 12px; color: #0284c7; margin-right: 6px; }
  .preview-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    margin-top: 4px;
  }
  .preview-meta .pill {
    font-size: 12px;
    font-weight: 700;
    padding: 1px 6px;
    border-radius: 999px;
  }
  .preview-meta .pill.ok { background: color-mix(in srgb, var(--accent-primary) 22%, transparent); color: var(--accent-primary); }
  .preview-meta .pill.skip { background: rgba(154, 163, 181, 0.2); color: var(--text-muted, #9aa3b5); }
  .preview-meta .pill.fail { background: color-mix(in srgb, var(--accent-danger) 20%, transparent); color: var(--accent-danger); }
  .preview-meta .hash { font-size: 12px; color: var(--text-muted, #9aa3b5); }
  .preview-meta .dest { font-size: 12px; color: var(--text-muted, #9aa3b5); }
  .draft-confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .warn { color: var(--accent-warning); font-size: 12px; margin: 0 0 10px; }
  @media (max-width: 1100px) {
    .chats {
      grid-template-columns: 1fr;
      grid-template-rows: auto minmax(0, 1fr) auto;
      height: 100%;
      min-height: 0;
    }
    .sessions,
    .draft {
      max-height: 220px;
      border-right: none;
      border-bottom: 1px solid var(--border-color, #2a2f3a);
    }
    .draft {
      border-left: none;
      max-height: 320px;
    }
  }
</style>
