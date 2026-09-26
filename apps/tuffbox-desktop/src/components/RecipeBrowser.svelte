<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import {
    api,
    type ScannedRecipe,
    type IngredientDisplay,
    type RecipeScanResult,
    type RecipeRuntimeStatus,
    type RuntimeRecipeCategory,
    type CraftDraft,
    type TagDraft,
  } from "../lib/api";
  import {
    Search,
    RefreshCw,
    ChevronLeft,
    ChevronRight,
    Copy,
    Flame,
    ArrowRight,
    Star,
    History,
    Hammer,
    Anvil,
    Scissors,
    Grid3x3,
    Package,
    Bookmark,
    Trash2,
    FileCode,
    X,
    Keyboard,
    Radio,
    Play,
    Plus,
    Pencil,
    Save,
    Eraser,
    Sparkles,
    SlidersHorizontal,
    Code,
    Check,
    Layers,
    Info,
    BookOpen,
    Tag,
  } from "@lucide/svelte";
  import { isProjectLaunching, launchSessions, projectPath } from "../lib/store";
  import { launchWithFeedback } from "../lib/launch";
  import VanillaClientJarPrompt from "./VanillaClientJarPrompt.svelte";

  type FocusMode = "recipes" | "uses";
  type ItemEntry = {
    id: string;
    name: string;
    modNs: string;
    recipeCount: number;
    useCount: number;
  };
  type ItemFocusCounts = { recipes: number; uses: number };
  type PaletteMode = "items" | "tags";
  type EditorKind =
    | "crafting"
    | "smelting"
    | "blasting"
    | "smoking"
    | "campfire"
    | "smithing"
    | "stonecutting"
    | "tags";

  const ITEMS_PER_PAGE = 72;
  const ICON_BATCH_SIZE = 48;
  const BOOKMARK_KEY = "tuffbox.jei.bookmarks";
  const DRAG_MIME = "application/x-tuffbox-item";

  const EDITOR_KINDS: { id: EditorKind; label: string; icon: any }[] = [
    { id: "crafting", label: "Crafting", icon: Grid3x3 },
    { id: "smelting", label: "Furnace", icon: Flame },
    { id: "blasting", label: "Blast Furnace", icon: Flame },
    { id: "smoking", label: "Smoker", icon: Flame },
    { id: "campfire", label: "Campfire", icon: Flame },
    { id: "smithing", label: "Smithing", icon: Anvil },
    { id: "stonecutting", label: "Stonecutter", icon: Scissors },
    { id: "tags", label: "Item Tags", icon: Tag },
  ];

  let recipes = $state<ScannedRecipe[]>([]);
  let items = $state<ItemEntry[]>([]);
  let catalogReady = $state(false);
  let scanMeta = $state<Omit<RecipeScanResult, "recipes"> | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let message = $state<string | null>(null);
  let filter = $state("");
  let categoryFilter = $state("all");
  let modFilter = $state("all");
  let focusMode = $state<FocusMode>("recipes");
  let selectedItem = $state("");
  let recipeIndex = $state(0);
  let itemPage = $state(0);
  let lastLoadedPath = $state<string | null>(null);
  let bookmarks = $state<string[]>([]);
  let historyStack = $state<string[]>([]);
  let showBookmarks = $state(true);
  let showHelp = $state(false);
  let cycleTick = $state(0);
  let cycleTimer: ReturnType<typeof setInterval> | null = null;
  let pendingRemoves = $state(new Set<string>());
  let recipeSource = $state<"offline" | "runtime">("offline");
  let runtimeStatus = $state<RecipeRuntimeStatus | null>(null);
  let runtimeCategories = $state<RuntimeRecipeCategory[]>([]);
  let runtimePoller: ReturnType<typeof setInterval> | null = null;
  const projectLaunching = $derived(isProjectLaunching($projectPath, $launchSessions));

  // Editor State
  let editorOpen = $state(false);
  let editorKind = $state<EditorKind>("crafting");
  let editGrid = $state<(string | null)[]>(Array(9).fill(null));
  let editOutput = $state<string | null>(null);
  let editCount = $state(1);
  let editShaped = $state(true);
  let editInput = $state<string | null>(null);
  let editXp = $state(0);
  let editCookTime = $state(200);
  let editTemplate = $state<string | null>(null);
  let editBase = $state<string | null>(null);
  let editAddition = $state<string | null>(null);
  let replaceRecipeId = $state<string | null>(null);
  let editorSaving = $state(false);
  let paletteMode = $state<PaletteMode>("items");
  let knownTags = $state<string[]>([]);
  let tagsLoading = $state(false);
  let editTagId = $state("");
  let tagMembers = $state<string[]>([]);
  let tagAdd = $state<string[]>([]);
  let tagRemove = $state<string[]>([]);
  let tagRemoveAll = $state(false);
  let tagLoadingMembers = $state(false);
  let codeSnippet = $state<string | null>(null);
  let snippetCopied = $state(false);

  // Vanilla Jar State
  const dismissedVanillaPrompt = new Set<string>();
  let vanillaPromptOpen = $state(false);
  let vanillaPromptVersion = $state("");
  let vanillaPromptSize = $state<number | null>(null);
  let vanillaDownloading = $state(false);
  let vanillaDownloadError = $state<string | null>(null);

  // Icon Cache
  type IconState = "loading" | "missing" | string;
  let iconCache = $state<Record<string, IconState>>({});
  const iconInFlight = new Set<string>();
  let iconPreloadQueue: string[] = [];
  let iconPreloadTimer: ReturnType<typeof setTimeout> | null = null;
  let iconBatchRunning = $state(false);

  async function preloadIconsBatch(ids: string[]) {
    if (!ids.length || !$projectPath) return;
    const pending = [...new Set(ids)].filter(
      (id) => id && !id.startsWith("#") && iconCache[id] === undefined && !iconInFlight.has(id)
    );
    if (!pending.length) return;

    for (const id of pending) {
      iconInFlight.add(id);
      iconCache[id] = "loading";
    }
    iconCache = { ...iconCache };

    try {
      const result = await api.recipes.itemIconsBatch(pending, $projectPath);
      for (const id of pending) {
        const file = result[id];
        iconCache[id] = file ?? "missing";
        iconInFlight.delete(id);
      }
    } catch {
      for (const id of pending) {
        iconCache[id] = "missing";
        iconInFlight.delete(id);
      }
    }
    iconCache = { ...iconCache };
  }

  async function flushIconPreload() {
    if (iconBatchRunning || iconPreloadQueue.length === 0 || !$projectPath) return;
    iconBatchRunning = true;
    try {
      while (iconPreloadQueue.length > 0) {
        const chunk = iconPreloadQueue.splice(0, ICON_BATCH_SIZE);
        await preloadIconsBatch(chunk);
        await tick();
      }
    } finally {
      iconBatchRunning = false;
      if (iconPreloadQueue.length > 0) scheduleIconPreload();
    }
  }

  function scheduleIconPreload(ids: Array<string | null | undefined> = []) {
    for (const id of ids) {
      if (!id || id.startsWith("#")) continue;
      if (iconCache[id] !== undefined || iconInFlight.has(id)) continue;
      if (!iconPreloadQueue.includes(id)) iconPreloadQueue.push(id);
    }
    if (iconPreloadTimer) clearTimeout(iconPreloadTimer);
    iconPreloadTimer = setTimeout(() => {
      iconPreloadTimer = null;
      void flushIconPreload();
    }, 32);
  }

  async function ensureItemIcon(itemId: string | null | undefined) {
    if (!itemId || !$projectPath || itemId.startsWith("#")) return;
    if (iconCache[itemId] !== undefined || iconInFlight.has(itemId)) return;
    await preloadIconsBatch([itemId]);
  }

  function normalizeIconUrl(url: string | null | undefined): string | null {
    if (!url) return null;
    if (url.startsWith("data:") || url.startsWith("http://") || url.startsWith("https://") || url.startsWith("blob:")) {
      return url;
    }
    return null;
  }

  function iconSrc(itemId: string | null | undefined, explicit?: string | null): string | null {
    const normalized = normalizeIconUrl(explicit);
    if (normalized) return normalized;
    if (!itemId) return null;
    const state = iconCache[itemId];
    return typeof state === "string" && state !== "loading" && state !== "missing" ? state : null;
  }

  function onIconError(itemId: string | null | undefined) {
    if (!itemId) return;
    iconCache[itemId] = "missing";
    iconCache = { ...iconCache };
  }

  function preloadIcons(ids: Array<string | null | undefined>) {
    scheduleIconPreload(ids);
  }

  const CATEGORY_META: Record<string, { label: string; icon: any }> = {
    all: { label: "All Recipes", icon: Layers },
    crafting: { label: "Crafting", icon: Grid3x3 },
    cooking: { label: "Furnace", icon: Flame },
    smithing: { label: "Smithing", icon: Anvil },
    stonecutting: { label: "Stonecutter", icon: Scissors },
    other: { label: "Custom", icon: Hammer },
  };

  onMount(() => {
    try {
      bookmarks = JSON.parse(localStorage.getItem(BOOKMARK_KEY) || "[]");
    } catch {
      bookmarks = [];
    }
    cycleTimer = setInterval(() => {
      cycleTick = (cycleTick + 1) % 1000;
    }, 1000);
    window.addEventListener("keydown", onKey);
    runtimePoller = setInterval(checkRuntimeTransition, 5000);
  });

  onDestroy(() => {
    if (cycleTimer) clearInterval(cycleTimer);
    if (runtimePoller) clearInterval(runtimePoller);
    if (iconPreloadTimer) clearTimeout(iconPreloadTimer);
    window.removeEventListener("keydown", onKey);
  });

  function onKey(e: KeyboardEvent) {
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
    if (e.key === "r" || e.key === "R") {
      if (selectedItem) {
        focusMode = "recipes";
        recipeIndex = 0;
      }
    } else if (e.key === "u" || e.key === "U") {
      if (selectedItem) {
        focusMode = "uses";
        recipeIndex = 0;
      }
    } else if (e.key === "ArrowLeft") {
      prevRecipe();
    } else if (e.key === "ArrowRight") {
      nextRecipe();
    } else if (e.key === "b" || e.key === "B") {
      if (selectedItem) toggleBookmark(selectedItem);
    } else if (e.key === "Backspace" && historyStack.length > 1) {
      e.preventDefault();
      goBack();
    }
  }

  async function maybeOfferVanillaJar(vanillaJarFound?: boolean) {
    if (!$projectPath || vanillaJarFound !== false) return;
    if (dismissedVanillaPrompt.has($projectPath)) return;
    try {
      const status = await api.minecraft.clientJarStatus($projectPath);
      if (status.found) return;
      vanillaPromptVersion = status.resolvedVersion || status.version;
      vanillaPromptSize = status.downloadSize ?? null;
      vanillaDownloadError = null;
      vanillaPromptOpen = true;
    } catch {
      /* ignore */
    }
  }

  async function downloadVanillaJar() {
    if (!$projectPath) return;
    vanillaDownloading = true;
    vanillaDownloadError = null;
    try {
      await api.minecraft.downloadClientJar($projectPath);
      vanillaPromptOpen = false;
      dismissedVanillaPrompt.delete($projectPath);
      message = `Downloaded Minecraft ${vanillaPromptVersion} client jar. Rescanning…`;
      await loadRecipes(false, true);
    } catch (e) {
      vanillaDownloadError = String(e);
    } finally {
      vanillaDownloading = false;
    }
  }

  function dismissVanillaPrompt() {
    if ($projectPath) dismissedVanillaPrompt.add($projectPath);
    vanillaPromptOpen = false;
  }

  async function loadRecipes(preferLive = true, preserveSelection = false) {
    if (!$projectPath) return;
    loading = true;
    catalogReady = false;
    error = null;
    message = null;
    const previousSelection = preserveSelection ? selectedItem : "";
    try {
      runtimeStatus = await api.recipes.runtimeStatus($projectPath);
      if (preferLive && runtimeStatus.connected) {
        const live = await api.recipes.runtimeSnapshot($projectPath);
        recipes = (live.recipes ?? []).filter((recipe) => !!recipe.outputId);
        runtimeCategories = live.categories ?? [];
        recipeSource = "runtime";
        scanMeta = {
          jarCount: 0,
          datapackFiles: 0,
          truncated: live.truncated,
          totalScanned: live.totalScanned,
        };
        message = `Connected to live JEI: ${recipes.length} runtime recipes in ${runtimeCategories.length} categories.`;
      } else {
        const result = await api.recipes.scan($projectPath);
        recipes = result.recipes ?? [];
        runtimeCategories = [];
        recipeSource = "offline";
        scanMeta = {
          jarCount: result.jarCount,
          datapackFiles: result.datapackFiles,
          truncated: result.truncated,
          totalScanned: result.totalScanned,
          vanillaJarFound: result.vanillaJarFound,
        };
        if (result.truncated) {
          message = `Indexed ${recipes.length} recipes (limit reached; ${result.totalScanned} files scanned).`;
        } else {
          message = `Indexed ${recipes.length} recipes from ${result.jarCount} jars` +
            (result.datapackFiles ? ` + ${result.datapackFiles} datapacks` : "") + ".";
        }
        await maybeOfferVanillaJar(result.vanillaJarFound);
      }
      await tick();
      await loadFullItemCatalog(recipes);
      lastLoadedPath = $projectPath;
      if (!preserveSelection) categoryFilter = "all";
      selectedItem = previousSelection && recipes.some(
        (recipe) => recipe.outputId === previousSelection || recipe.inputIds.includes(previousSelection)
      ) ? previousSelection : (recipes[0]?.outputId ?? "");
      recipeIndex = 0;
      itemPage = 0;
      if (!preserveSelection) historyStack = selectedItem ? [selectedItem] : [];
    } catch (e) {
      if (preferLive && runtimeStatus?.connected) {
        try {
          const fallback = await api.recipes.scan($projectPath);
          recipes = fallback.recipes ?? [];
          runtimeCategories = [];
          recipeSource = "offline";
          scanMeta = {
            jarCount: fallback.jarCount,
            datapackFiles: fallback.datapackFiles,
            truncated: fallback.truncated,
            totalScanned: fallback.totalScanned,
            vanillaJarFound: fallback.vanillaJarFound,
          };
          await tick();
          await loadFullItemCatalog(recipes);
          lastLoadedPath = $projectPath;
          message = `Live JEI disconnected; showing offline recipes. ${String(e)}`;
          await maybeOfferVanillaJar(fallback.vanillaJarFound);
        } catch (fallbackError) {
          error = String(fallbackError);
        }
      } else {
        error = String(e);
      }
    } finally {
      loading = false;
    }
  }

  async function checkRuntimeTransition() {
    if (!$projectPath || loading) return;
    try {
      const status = await api.recipes.runtimeStatus($projectPath);
      const changed = status.connected !== runtimeStatus?.connected;
      runtimeStatus = status;
      if (changed && status.connected && recipeSource !== "runtime") {
        await loadRecipes(true, true);
      } else if (changed && !status.connected && recipeSource === "runtime") {
        await loadRecipes(false, true);
      }
    } catch {
      /* ignore */
    }
  }

  async function launchJeiLive() {
    if (!$projectPath) return;
    error = null;
    try {
      const profiles = await api.project.listProfiles($projectPath);
      const profile = profiles.find((entry) => entry.side.toLowerCase() !== "server") ?? profiles[0];
      if (!profile) throw new Error("Create a client profile before launching JEI Live.");
      const result = await launchWithFeedback({ path: $projectPath, profile: profile.id });
      if (!result) return;
      message = `Launching ${profile.name}. Live recipes connect once the game loads.`;
      runtimeStatus = { connected: false, supported: true, message: "Waiting for game runtime…", minecraftVersion: null, pid: null };
    } catch (e) {
      error = String(e);
    }
  }

  function prettifyItem(id: string): string {
    if (!id || id === "?") return "Unknown";
    const bare = id.replace(/^#/, "");
    const name = bare.split(":").pop() ?? bare;
    return name.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
  }

  function itemNamespace(id: string): string {
    return id.replace(/^#/, "").split(":")[0] ?? "";
  }

  function itemHue(id: string): number {
    let h = 0;
    for (const c of id) h = (h * 31 + c.charCodeAt(0)) % 360;
    return h;
  }

  function matchesJeiSearch(itemId: string, query: string, displayName?: string): boolean {
    const q = query.trim().toLowerCase();
    if (!q) return true;
    const tokens = q.split(/\s+/).filter(Boolean);
    for (const token of tokens) {
      if (token.startsWith("-")) {
        const neg = token.slice(1);
        if (neg && matchesToken(itemId, neg, displayName)) return false;
        continue;
      }
      if (!matchesToken(itemId, token, displayName)) return false;
    }
    return true;
  }

  function matchesToken(itemId: string, token: string, displayName?: string): boolean {
    const name = (displayName || prettifyItem(itemId)).toLowerCase();
    const id = itemId.toLowerCase();
    if (token.startsWith("@")) return itemNamespace(itemId).toLowerCase().includes(token.slice(1));
    if (token.startsWith("#")) return id.includes(token.slice(1)) || id.startsWith("#") || id.includes(token);
    if (token.startsWith("&")) return id.includes(token.slice(1));
    if (token.startsWith("$")) return name.includes(token.slice(1)) || id.includes(token.slice(1));
    return name.includes(token) || id.includes(token);
  }

  function buildItemCatalog(list: ScannedRecipe[]): ItemEntry[] {
    const map = new Map<string, ItemEntry>();
    const names = new Map<string, string>();
    for (const recipe of list) {
      if (recipe.layout.output?.name) names.set(recipe.layout.output.id, recipe.layout.output.name);
      for (const slot of recipe.layout.slots ?? []) {
        for (const ingredient of slot.ingredients) {
          if (ingredient.name) names.set(ingredient.id, ingredient.name);
        }
      }
    }
    const touch = (id: string, field: "recipeCount" | "useCount") => {
      if (!id || id === "unknown:unknown") return;
      const existing = map.get(id);
      if (existing) existing[field]++;
      else
        map.set(id, {
          id,
          name: names.get(id) ?? prettifyItem(id),
          modNs: itemNamespace(id),
          recipeCount: field === "recipeCount" ? 1 : 0,
          useCount: field === "useCount" ? 1 : 0,
        });
    };
    for (const r of list) {
      touch(r.outputId, "recipeCount");
      for (const inp of r.inputIds) touch(inp, "useCount");
    }
    return [...map.values()].sort((a, b) => a.name.localeCompare(b.name));
  }

  function recipesForItem(itemId: string, mode: FocusMode): ScannedRecipe[] {
    if (!itemId) return [];
    return recipes.filter((r) => {
      if (categoryFilter !== "all" && r.category !== categoryFilter) return false;
      if (modFilter !== "all" && itemNamespace(r.outputId) !== modFilter && !r.modSource.includes(modFilter))
        return false;
      if (mode === "recipes") return r.outputId === itemId;
      return r.inputIds.includes(itemId);
    });
  }

  function selectItem(id: string, mode?: FocusMode, pushHistory = true) {
    if (!id || id.startsWith("#")) return;
    if (pushHistory && id !== selectedItem) {
      historyStack = [...historyStack.filter((h) => h !== id), id].slice(-40);
    }
    selectedItem = id;
    if (mode) focusMode = mode;
    recipeIndex = 0;
  }

  function goBack() {
    if (historyStack.length < 2) return;
    const next = [...historyStack];
    next.pop();
    const prev = next[next.length - 1];
    historyStack = next;
    selectedItem = prev;
    recipeIndex = 0;
  }

  function toggleBookmark(id: string) {
    if (bookmarks.includes(id)) bookmarks = bookmarks.filter((b) => b !== id);
    else bookmarks = [...bookmarks, id];
    localStorage.setItem(BOOKMARK_KEY, JSON.stringify(bookmarks));
  }

  function resolveSlot(ing: IngredientDisplay | null): IngredientDisplay | null {
    if (!ing) return null;
    if (ing.alts && ing.alts.length > 0) {
      return ing.alts[cycleTick % ing.alts.length] ?? ing;
    }
    return ing;
  }

  function slotLabel(ing: IngredientDisplay | null): string {
    const r = resolveSlot(ing);
    if (!r) return "";
    if (ing?.alts && ing.alts.length > 0) {
      if (r.kind === "tag" || r.id.startsWith("#")) {
        const bare = r.id.replace(/^#/, "");
        const path = bare.split(":").pop() ?? bare;
        return path.slice(0, 2);
      }
      return prettifyItem(r.id).slice(0, 3);
    }
    if (r.kind === "tag" || r.id.startsWith("#")) return "#";
    return prettifyItem(r.id).slice(0, 3);
  }

  function slotTitle(ing: IngredientDisplay | null): string {
    if (!ing) return "";
    if (ing.alts && ing.alts.length > 0) {
      const head = ing.kind === "tag" || ing.id.startsWith("#") ? `${ing.id}\n` : "";
      return head + ing.alts.map((a) => a.id).join("\n");
    }
    return ing.id;
  }

  async function copyKubeJS(r: ScannedRecipe) {
    const script = await api.recipes.generateScript("remove", [r.id]);
    await navigator.clipboard.writeText(script.content);
    message = "KubeJS recipe remove snippet copied to clipboard.";
    snippetCopied = true;
    setTimeout(() => (snippetCopied = false), 2000);
  }

  async function queueRemove(r: ScannedRecipe) {
    pendingRemoves = new Set([...pendingRemoves, r.id]);
    message = `Queued ${pendingRemoves.size} recipe(s) for KubeJS remove.`;
  }

  async function flushRemoves() {
    if (!$projectPath || pendingRemoves.size === 0) return;
    try {
      const path = await api.recipes.writeRemoves([...pendingRemoves], $projectPath);
      message = `Wrote ${pendingRemoves.size} recipe removes to ${path}`;
      pendingRemoves = new Set();
    } catch (e) {
      error = String(e);
    }
  }

  function ingredientIdForEdit(ing: IngredientDisplay | null): string | null {
    if (!ing) return null;
    if (ing.kind === "tag" || ing.id.startsWith("#")) {
      return ing.id.startsWith("#") ? ing.id : `#${ing.id}`;
    }
    return ing.id || null;
  }

  function resetEditorDraft() {
    editGrid = Array(9).fill(null);
    editOutput = null;
    editCount = 1;
    editShaped = true;
    editInput = null;
    editXp = 0;
    editCookTime = 200;
    editTemplate = null;
    editBase = null;
    editAddition = null;
    replaceRecipeId = null;
    editTagId = "";
    tagMembers = [];
    tagAdd = [];
    tagRemove = [];
    tagRemoveAll = false;
  }

  async function ensureTagsLoaded() {
    if (!$projectPath || knownTags.length > 0 || tagsLoading) return;
    tagsLoading = true;
    try {
      knownTags = await api.recipes.listItemTags($projectPath);
    } catch (e) {
      knownTags = [];
    } finally {
      tagsLoading = false;
    }
  }

  async function loadTagMembers(tagId: string) {
    if (!$projectPath || !tagId.trim()) {
      tagMembers = [];
      return;
    }
    tagLoadingMembers = true;
    try {
      tagMembers = await api.recipes.getTagEntries(tagId.trim(), $projectPath);
    } catch (e) {
      tagMembers = [];
    } finally {
      tagLoadingMembers = false;
    }
  }

  async function selectEditTag(tagId: string) {
    const bare = tagId.replace(/^#/, "");
    editTagId = bare;
    tagAdd = [];
    tagRemove = [];
    tagRemoveAll = false;
    await loadTagMembers(bare.startsWith("#") ? bare : `#${bare}`);
  }

  async function openNewRecipeEditor(kind: EditorKind = "crafting") {
    resetEditorDraft();
    editorKind = kind;
    editorOpen = true;
    paletteMode = kind === "tags" ? "tags" : "items";
    await ensureTagsLoaded();
    message =
      kind === "tags"
        ? "Pick a tag, add items by click or drag, then Save."
        : `Compose ingredients for ${kind} recipe, then click Add.`;
  }

  function cookingKindFromType(recipeType: string): EditorKind {
    const t = recipeType.replace(/^minecraft:/, "");
    if (t === "blasting") return "blasting";
    if (t === "smoking") return "smoking";
    if (t === "campfire_cooking") return "campfire";
    return "smelting";
  }

  function editableCategory(r: ScannedRecipe): "crafting" | "cooking" | "smithing" | "stonecutting" | "other" {
    const cat = (r.layout?.category || r.category || "").toLowerCase();
    if (cat === "crafting" || cat === "cooking" || cat === "smithing" || cat === "stonecutting") {
      return cat;
    }
    const hay = `${cat} ${r.recipeType || ""}`.toLowerCase();
    if ((hay.includes("craft") || hay.includes("workbench")) && !hay.includes("smith")) return "crafting";
    if (hay.includes("smelt") || hay.includes("blast") || hay.includes("smok") || hay.includes("campfire") || hay.includes("furnace") || hay.includes("cook")) {
      return "cooking";
    }
    if (hay.includes("smith")) return "smithing";
    if (hay.includes("stonecut") || (hay.includes("cutting") && !hay.includes("wood"))) return "stonecutting";
    return "other";
  }

  function firstIngredient(slot: { ingredients?: IngredientDisplay[] } | null | undefined): IngredientDisplay | null {
    if (!slot?.ingredients?.length) return null;
    return slot.ingredients[0] ?? null;
  }

  function gridFromRecipe(r: ScannedRecipe): (IngredientDisplay | null)[] {
    const grid = Array(9).fill(null) as (IngredientDisplay | null)[];
    const hasGrid = (r.layout.grid ?? []).some((s) => !!s);
    if (hasGrid) {
      for (let i = 0; i < 9; i++) grid[i] = r.layout.grid[i] ?? null;
      return grid;
    }
    const slots = (r.layout.slots ?? []).filter(
      (s) => (s.role || "").toLowerCase() !== "output" && (s.ingredients?.length ?? 0) > 0,
    );
    if (slots.length === 0) return grid;
    const xs = slots.map((s) => s.x);
    const ys = slots.map((s) => s.y);
    const minX = Math.min(...xs);
    const maxX = Math.max(...xs);
    const minY = Math.min(...ys);
    const maxY = Math.max(...ys);
    const spanX = Math.max(1, maxX - minX);
    const spanY = Math.max(1, maxY - minY);
    for (const slot of slots) {
      const col = Math.min(2, Math.round(((slot.x - minX) / spanX) * 2));
      const row = Math.min(2, Math.round(((slot.y - minY) / spanY) * 2));
      const idx = row * 3 + col;
      if (!grid[idx]) grid[idx] = firstIngredient(slot);
    }
    if (grid.filter(Boolean).length <= 1 && slots.length > 1) {
      for (let i = 0; i < Math.min(9, slots.length); i++) {
        grid[i] = firstIngredient(slots[i]);
      }
    }
    return grid;
  }

  function cookingInputFromRecipe(r: ScannedRecipe): IngredientDisplay | null {
    if (r.layout.grid?.[4]) return r.layout.grid[4];
    if (r.layout.grid?.find(Boolean)) return r.layout.grid.find(Boolean) ?? null;
    const slots = (r.layout.slots ?? []).filter((s) => (s.role || "").toLowerCase() !== "output");
    return firstIngredient(slots[0] ?? null);
  }

  async function openEditRecipe(r: ScannedRecipe) {
    const cat = editableCategory(r);
    const gridSlots = gridFromRecipe(r);
    if (cat === "crafting") {
      const grid = Array(9).fill(null) as (string | null)[];
      for (let i = 0; i < 9; i++) {
        grid[i] = ingredientIdForEdit(gridSlots[i] ?? null);
      }
      resetEditorDraft();
      editGrid = grid;
      editOutput = ingredientIdForEdit(r.layout.output) || r.outputId || null;
      editCount = Math.min(64, Math.max(1, r.layout.outputCount || 1));
      editShaped = !r.layout.shapeless;
      editorKind = "crafting";
      replaceRecipeId = r.id;
    } else if (cat === "cooking") {
      resetEditorDraft();
      editorKind = cookingKindFromType(r.recipeType);
      editInput = ingredientIdForEdit(cookingInputFromRecipe(r));
      editOutput = ingredientIdForEdit(r.layout.output) || r.outputId || null;
      editCount = Math.min(64, Math.max(1, r.layout.outputCount || 1));
      editCookTime = r.layout.cookTime ?? 200;
      editXp = r.layout.experience ?? 0;
      replaceRecipeId = r.id;
    } else if (cat === "smithing") {
      resetEditorDraft();
      editorKind = "smithing";
      editTemplate = ingredientIdForEdit(gridSlots[3] ?? null);
      editBase = ingredientIdForEdit(gridSlots[4] ?? null) || ingredientIdForEdit(gridSlots[0] ?? null);
      editAddition = ingredientIdForEdit(gridSlots[5] ?? null) || ingredientIdForEdit(gridSlots[1] ?? null);
      editOutput = ingredientIdForEdit(r.layout.output) || r.outputId || null;
      editCount = Math.min(64, Math.max(1, r.layout.outputCount || 1));
      replaceRecipeId = r.id;
    } else if (cat === "stonecutting") {
      resetEditorDraft();
      editorKind = "stonecutting";
      const inputSlot = cookingInputFromRecipe(r);
      editInput = ingredientIdForEdit(inputSlot);
      editOutput = ingredientIdForEdit(r.layout.output) || r.outputId || null;
      editCount = Math.min(64, Math.max(1, r.layout.outputCount || 1));
      replaceRecipeId = r.id;
    } else {
      message = "This custom recipe type cannot be edited in-place — use Replace with crafting.";
      return;
    }
    editorOpen = true;
    paletteMode = "items";
    await ensureTagsLoaded();
    scheduleIconPreload([
      editOutput,
      editInput,
      editTemplate,
      editBase,
      editAddition,
      ...editGrid,
    ]);
  }

  async function openReplaceWithCrafting(r: ScannedRecipe) {
    resetEditorDraft();
    editorKind = "crafting";
    editShaped = true;
    editOutput = ingredientIdForEdit(r.layout.output) || r.outputId || null;
    editCount = Math.min(64, Math.max(1, r.layout.outputCount || 1));
    const gridSlots = gridFromRecipe(r);
    const grid = Array(9).fill(null) as (string | null)[];
    for (let i = 0; i < 9; i++) {
      grid[i] = ingredientIdForEdit(gridSlots[i] ?? null);
    }
    if (!grid.some(Boolean) && r.inputIds?.length) {
      for (let i = 0; i < Math.min(9, r.inputIds.length); i++) {
        const id = r.inputIds[i];
        if (id && !id.startsWith("#")) grid[i] = id;
        else if (id) grid[i] = id.startsWith("#") ? id : `#${id}`;
      }
    }
    editGrid = grid;
    replaceRecipeId = r.id;
    editorOpen = true;
    paletteMode = "items";
    await ensureTagsLoaded();
    scheduleIconPreload([editOutput, ...editGrid]);
  }

  function closeEditor() {
    editorOpen = false;
    editorKind = "crafting";
    resetEditorDraft();
  }

  function clearEditorGrid() {
    if (editorKind === "tags") {
      tagAdd = [];
      tagRemove = [];
      tagRemoveAll = false;
      return;
    }
    editGrid = Array(9).fill(null);
    editOutput = null;
    editCount = 1;
    editInput = null;
    editTemplate = null;
    editBase = null;
    editAddition = null;
    editXp = 0;
    editCookTime = 200;
  }

  function setEditSlot(index: number, id: string | null) {
    const next = [...editGrid];
    next[index] = id;
    editGrid = next;
  }

  let suppressSlotClickUntil = 0;

  function onDragStartItem(e: DragEvent, id: string) {
    if (!e.dataTransfer) return;
    e.dataTransfer.setData("text/plain", id);
    e.dataTransfer.setData(DRAG_MIME, id);
    e.dataTransfer.effectAllowed = "copy";
  }

  function allowEditorDrop(e: DragEvent) {
    if (!editorOpen) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
  }

  function readDragId(e: DragEvent): string | null {
    const raw = e.dataTransfer?.getData(DRAG_MIME) || e.dataTransfer?.getData("text/plain") || "";
    const id = raw.trim().split(/\r?\n/)[0]?.trim() ?? "";
    return id || null;
  }

  function markDropJustHappened() {
    suppressSlotClickUntil = Date.now() + 250;
  }

  function onDropGrid(e: DragEvent, index: number) {
    if (!editorOpen || editorKind !== "crafting") return;
    e.preventDefault();
    e.stopPropagation();
    markDropJustHappened();
    const id = readDragId(e);
    if (id) setEditSlot(index, id);
  }

  function onDropOutput(e: DragEvent) {
    if (!editorOpen || editorKind === "tags") return;
    e.preventDefault();
    e.stopPropagation();
    markDropJustHappened();
    const id = readDragId(e);
    if (!id || id.startsWith("#")) {
      message = "Recipe output must be a valid item ID (not a tag).";
      return;
    }
    editOutput = id;
  }

  function onDropSingleInput(e: DragEvent) {
    if (!editorOpen) return;
    e.preventDefault();
    e.stopPropagation();
    markDropJustHappened();
    const id = readDragId(e);
    if (id) editInput = id;
  }

  function onDropSmithing(e: DragEvent, slot: "template" | "base" | "addition") {
    if (!editorOpen || editorKind !== "smithing") return;
    e.preventDefault();
    e.stopPropagation();
    markDropJustHappened();
    const id = readDragId(e);
    if (!id) return;
    if (slot === "template") editTemplate = id;
    else if (slot === "base") editBase = id;
    else editAddition = id;
  }

  function onDropTagAdd(e: DragEvent) {
    if (!editorOpen || editorKind !== "tags") return;
    e.preventDefault();
    e.stopPropagation();
    markDropJustHappened();
    const id = readDragId(e);
    if (!id) return;
    if (!tagAdd.includes(id)) tagAdd = [...tagAdd, id];
    tagRemove = tagRemove.filter((x) => x !== id);
  }

  function placePaletteItem(id: string) {
    if (!editorOpen || !id) return;
    if (editorKind === "tags") {
      if (!tagAdd.includes(id)) tagAdd = [...tagAdd, id];
      tagRemove = tagRemove.filter((x) => x !== id);
      return;
    }
    if (editorKind === "crafting") {
      const idx = editGrid.findIndex((s) => !s);
      if (idx >= 0) {
        setEditSlot(idx, id);
        return;
      }
      if (!editOutput) {
        editOutput = id;
        return;
      }
      message = "Crafting grid and output slots are full. Right-click a slot to clear it.";
      return;
    }
    if (isCookingKind(editorKind) || editorKind === "stonecutting") {
      if (!editInput) {
        editInput = id;
        return;
      }
      if (!editOutput) {
        editOutput = id;
        return;
      }
      message = "Input and output slots are full. Right-click to clear.";
      return;
    }
    if (editorKind === "smithing") {
      if (!editTemplate) {
        editTemplate = id;
        return;
      }
      if (!editBase) {
        editBase = id;
        return;
      }
      if (!editAddition) {
        editAddition = id;
        return;
      }
      if (!editOutput) {
        editOutput = id;
        return;
      }
      message = "Smithing slots are full. Right-click to clear.";
      return;
    }
  }

  let quickAddId = $state("");

  function submitQuickAdd() {
    const id = quickAddId.trim();
    if (!id) return;
    placePaletteItem(id);
    quickAddId = "";
  }

  function clearEditSlotIfClick(index: number) {
    if (Date.now() < suppressSlotClickUntil) return;
    setEditSlot(index, null);
  }

  function clearInputIfClick() {
    if (Date.now() < suppressSlotClickUntil) return;
    editInput = null;
  }

  function clearSmithingIfClick(slot: "template" | "base" | "addition") {
    if (Date.now() < suppressSlotClickUntil) return;
    if (slot === "template") editTemplate = null;
    else if (slot === "base") editBase = null;
    else editAddition = null;
  }

  function toggleTagRemove(id: string) {
    if (tagRemove.includes(id)) tagRemove = tagRemove.filter((x) => x !== id);
    else tagRemove = [...tagRemove, id];
    tagAdd = tagAdd.filter((x) => x !== id);
  }

  function removePendingAdd(id: string) {
    tagAdd = tagAdd.filter((x) => x !== id);
  }

  function onOutputClick(e: MouseEvent) {
    if (Date.now() < suppressSlotClickUntil) return;
    if (!editorOpen || !editOutput || editorKind === "tags") return;
    if (e.shiftKey) {
      e.preventDefault();
      editCount = editCount >= 64 ? 1 : editCount + 1;
    }
  }

  function isCookingKind(k: EditorKind): boolean {
    return k === "smelting" || k === "blasting" || k === "smoking" || k === "campfire";
  }

  function editorCanSave(): boolean {
    if (editorKind === "tags") {
      const tag = editTagId.trim();
      if (!tag.includes(":")) return false;
      return tagRemoveAll || tagAdd.length > 0 || tagRemove.length > 0;
    }
    if (!editOutput) return false;
    if (editorKind === "crafting") return editGrid.some((s) => !!s);
    if (isCookingKind(editorKind) || editorKind === "stonecutting") return !!editInput;
    if (editorKind === "smithing") return !!editBase && !!editAddition;
    return false;
  }

  async function saveCraftRecipe() {
    if (!$projectPath || !editorCanSave()) return;
    editorSaving = true;
    error = null;
    try {
      if (editorKind === "tags") {
        const draft: TagDraft = {
          tagId: editTagId.trim().replace(/^#/, ""),
          add: [...tagAdd],
          remove: [...tagRemove],
          removeAll: tagRemoveAll,
        };
        const path = await api.recipes.writeTags(draft, $projectPath);
        message = `Saved tag modifications to ${path}`;
        knownTags = [];
        await ensureTagsLoaded();
        closeEditor();
        return;
      }
      if (!editOutput) return;
      const kind =
        editorKind === "crafting" ? (editShaped ? "shaped" : "shapeless") : editorKind;
      const draft: CraftDraft = {
        kind,
        shaped: editShaped,
        grid: [...editGrid],
        output: editOutput,
        outputCount: Math.min(64, Math.max(1, editCount || 1)),
        replaceId: replaceRecipeId,
        input: editInput,
        xp: isCookingKind(editorKind) ? editXp : null,
        cookTime: isCookingKind(editorKind) ? editCookTime : null,
        template: editTemplate,
        base: editBase,
        addition: editAddition,
      };
      const path = await api.recipes.writeCraft(draft, $projectPath);
      message = replaceRecipeId ? `Updated recipe in ${path}` : `Added ${kind} recipe in ${path}`;
      closeEditor();
      await loadRecipes(false, true);
    } catch (e) {
      error = String(e);
    } finally {
      editorSaving = false;
    }
  }

  const filteredTags = $derived((() => {
    const q = filter.trim().toLowerCase();
    if (!q) return knownTags;
    return knownTags.filter((t) => t.toLowerCase().includes(q));
  })());

  function navigateSlot(ing: IngredientDisplay | null, mode: FocusMode) {
    const r = resolveSlot(ing);
    if (!r) return;
    if (r.id.startsWith("#") || r.kind === "tag") return;
    selectItem(r.id, mode);
  }

  function runtimeSlotIngredient(slot: { ingredients: IngredientDisplay[] }): IngredientDisplay | null {
    if (!slot.ingredients?.length) return null;
    return slot.ingredients[cycleTick % slot.ingredients.length] ?? slot.ingredients[0];
  }

  function runtimeCategory(id: string) {
    return runtimeCategories.find((category) => category.id === id);
  }

  function buildCategories(): string[] {
    const ids = new Set(recipes.map((recipe) => recipe.category));
    if (recipeSource === "runtime" && runtimeCategories.length > 0) {
      const ordered = runtimeCategories.map((category) => category.id).filter((id) => ids.has(id));
      const extras = [...ids].filter((id) => !ordered.includes(id)).sort((a, b) => a.localeCompare(b));
      return ["all", ...ordered, ...extras];
    }
    const buckets = ["crafting", "cooking", "smithing", "stonecutting", "other"];
    return ["all", ...buckets.filter((bucket) => ids.has(bucket))];
  }

  function categoryLabel(cat: string): string {
    if (cat === "all") return "All";
    return CATEGORY_META[cat]?.label ?? runtimeCategory(cat)?.title ?? prettifyItem(cat);
  }

  function categoryIconComponent(cat: string) {
    if (CATEGORY_META[cat]?.icon) return CATEGORY_META[cat].icon;
    const hay = `${cat} ${runtimeCategory(cat)?.title ?? ""}`.toLowerCase();
    if ((hay.includes("craft") || hay.includes("workbench")) && !hay.includes("smith")) return Grid3x3;
    if (hay.includes("smelt") || hay.includes("furnace") || hay.includes("blast") || hay.includes("cook")) return Flame;
    if (hay.includes("smith")) return Anvil;
    if (hay.includes("stonecut") || hay.includes("cutting") || hay.includes("saw")) return Scissors;
    return Hammer;
  }

  function buildFilteredCounts(
    list: ScannedRecipe[],
    category: string,
    modNs: string
  ): Map<string, ItemFocusCounts> {
    const map = new Map<string, ItemFocusCounts>();
    const touch = (id: string, field: "recipes" | "uses") => {
      if (!id || id === "unknown:unknown") return;
      const existing = map.get(id) ?? { recipes: 0, uses: 0 };
      existing[field]++;
      map.set(id, existing);
    };
    for (const recipe of list) {
      if (category !== "all" && recipe.category !== category) continue;
      if (modNs !== "all") {
        const outputNs = itemNamespace(recipe.outputId);
        if (outputNs !== modNs && !recipe.modSource.includes(modNs)) continue;
      }
      touch(recipe.outputId, "recipes");
      for (const inp of recipe.inputIds) touch(inp, "uses");
    }
    return map;
  }

  async function loadFullItemCatalog(list: ScannedRecipe[]) {
    const fromRecipes = buildItemCatalog(list);
    const map = new Map(fromRecipes.map((i) => [i.id, i]));
    try {
      if ($projectPath) {
        const catalog = await api.recipes.listItemCatalog($projectPath);
        for (const entry of catalog ?? []) {
          const existing = map.get(entry.id);
          if (existing) {
            if (entry.name && entry.name !== entry.id) existing.name = entry.name;
          } else {
            map.set(entry.id, {
              id: entry.id,
              name: entry.name || prettifyItem(entry.id),
              modNs: entry.modNs || itemNamespace(entry.id),
              recipeCount: 0,
              useCount: 0,
            });
          }
        }
      }
    } catch (e) {
      console.warn("listItemCatalog error", e);
    }
    items = [...map.values()].sort((a, b) => a.name.localeCompare(b.name));
    catalogReady = true;
  }

  function focusCountForItem(item: ItemEntry): number {
    if (categoryFilter === "all" && modFilter === "all") {
      return focusMode === "recipes" ? item.recipeCount : item.useCount;
    }
    const counts = filteredCounts.get(item.id);
    if (!counts) return 0;
    return focusMode === "recipes" ? counts.recipes : counts.uses;
  }

  function prevRecipe() {
    if (recipeIndex > 0) recipeIndex--;
  }
  function nextRecipe() {
    if (recipeIndex < activeRecipes.length - 1) recipeIndex++;
  }

  const filteredCounts = $derived(buildFilteredCounts(recipes, categoryFilter, modFilter));
  const modNamespaces = $derived(["all", ...new Set(items.map((i) => i.modNs).filter(Boolean))].sort());
  const filteredItems = $derived(catalogReady
    ? items.filter((i) => {
        if (modFilter !== "all" && i.modNs !== modFilter) return false;
        return matchesJeiSearch(i.id, filter, i.name);
      })
    : []);
  const totalItemPages = $derived(Math.max(1, Math.ceil(filteredItems.length / ITEMS_PER_PAGE)));
  const totalTagPages = $derived(Math.max(1, Math.ceil(filteredTags.length / ITEMS_PER_PAGE)));
  const overlayPageCount = $derived(editorOpen && paletteMode === "tags" ? totalTagPages : totalItemPages);
  $effect(() => {
    if (itemPage >= overlayPageCount) itemPage = Math.max(0, overlayPageCount - 1);
  });
  const pageItems = $derived(filteredItems.slice(itemPage * ITEMS_PER_PAGE, (itemPage + 1) * ITEMS_PER_PAGE));
  const pageTags = $derived(filteredTags.slice(itemPage * ITEMS_PER_PAGE, (itemPage + 1) * ITEMS_PER_PAGE));
  const activeRecipes = $derived(recipesForItem(selectedItem, focusMode));
  const categories = $derived(buildCategories());
  $effect(() => {
    if (!categories.includes(categoryFilter)) categoryFilter = "all";
  });
  $effect(() => {
    if (activeRecipes.length === 0) recipeIndex = 0;
    else if (recipeIndex >= activeRecipes.length) recipeIndex = activeRecipes.length - 1;
  });
  const currentRecipe = $derived(activeRecipes[recipeIndex] ?? null);
  const bookmarkItems = $derived(bookmarks
    .map((id) => items.find((i) => i.id === id) ?? { id, name: prettifyItem(id), modNs: itemNamespace(id), recipeCount: 0, useCount: 0 })
    .filter(Boolean));

  $effect(() => {
    preloadIcons(pageItems.map((item) => item.id));
  });
  $effect(() => {
    preloadIcons(bookmarkItems.map((item) => item.id));
  });
  $effect(() => {
    if (selectedItem) ensureItemIcon(selectedItem);
  });
  $effect(() => {
    if (currentRecipe) {
      preloadIcons([
        currentRecipe.outputId,
        currentRecipe.layout.output?.id,
        ...(currentRecipe.layout.output?.alts?.map((a) => a.id) ?? []),
        ...currentRecipe.layout.grid.flatMap((slot) => [
          resolveSlot(slot)?.id,
          ...(slot?.alts?.map((a) => a.id) ?? []),
        ]),
        ...(currentRecipe.layout.slots ?? []).flatMap((slot) =>
          slot.ingredients.flatMap((ingredient) => [
            ingredient.id,
            ...(ingredient.alts?.map((a) => a.id) ?? []),
          ])
        ),
        ...(runtimeCategory(currentRecipe.category)?.stations ?? []).map((station) => station.id),
      ]);
    }
  });
  $effect(() => {
    if ($projectPath && $projectPath !== lastLoadedPath) {
      knownTags = [];
      closeEditor();
      loadRecipes();
    }
  });
  $effect(() => {
    if (filter) itemPage = 0;
  });
</script>

<div class="recipe-workspace flex flex-col w-full h-full min-h-0 bg-[var(--bg-primary)] text-[var(--text-primary)] overflow-hidden" class:busy={loading}>
  <!-- Top Workspace Toolbar -->
  <header class="workspace-header flex flex-wrap items-center justify-between gap-3 px-4 py-2.5 bg-[var(--bg-secondary)] border-b border-[var(--border-color)] flex-shrink-0">
    <div class="flex items-center gap-3 min-w-0">
      <div class="flex h-8 w-8 items-center justify-center rounded-lg bg-[var(--accent-primary)]/10 text-[var(--accent-primary)] border border-[var(--accent-primary)]/20 font-bold flex-shrink-0">
        🍲
      </div>
      <div class="min-w-0">
        <div class="flex items-center gap-2">
          <h1 class="text-sm font-bold text-[var(--text-primary)] m-0 leading-tight">
            {editorOpen ? "Recipe & Tag Editor" : "Recipe Browser"}
          </h1>
          <span class="inline-flex items-center gap-2 px-2 py-0.5 rounded-full text-[12px] font-semibold {recipeSource === 'runtime' ? 'bg-emerald-500/10 text-[var(--accent-primary)] border border-emerald-500/20' : 'bg-[var(--bg-card)] text-[var(--text-muted)] border border-[var(--border-color)]'}">
            <Radio size={10} class={recipeSource === 'runtime' ? 'text-[var(--accent-primary)]' : 'text-[var(--text-muted)]'} />
            {recipeSource === "runtime" ? "Live JEI" : "Offline Files"}
          </span>
        </div>
        <div class="text-[12px] font-medium text-[var(--text-muted)] truncate mt-0.5">
          {recipes.length} recipes · {items.length} items
          {#if scanMeta?.jarCount} · {scanMeta.jarCount} JARs{/if}
          {#if scanMeta?.datapackFiles} · {scanMeta.datapackFiles} datapacks{/if}
          {#if scanMeta?.vanillaJarFound === false}
            · <button
              type="button"
              class="text-[var(--accent-warning)] hover:underline font-semibold"
              onclick={() => {
                if ($projectPath) dismissedVanillaPrompt.delete($projectPath);
                void maybeOfferVanillaJar(false);
              }}
            >Vanilla JAR missing</button>
          {/if}
        </div>
      </div>
    </div>

    <!-- Center Search & Filter Group -->
    <div class="flex items-center gap-2 flex-1 max-w-xl mx-2 min-w-[280px]">
      <div class="relative flex-1 flex items-center">
        <Search size={14} class="absolute left-3 text-[var(--text-muted)] pointer-events-none" />
        <input
          class="w-full pl-8 pr-7 py-1.5 text-xs bg-[var(--bg-card)] border border-[var(--border-color)] rounded-lg text-[var(--text-primary)] placeholder-[var(--text-muted)] focus:outline-none focus:border-[var(--accent-primary)] transition"
          bind:value={filter}
          placeholder="Search name, @mod, #tag, &id, -exclude…"
          spellcheck="false"
        />
        {#if filter}
          <button
            type="button"
            class="absolute right-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] p-0.5"
            onclick={() => (filter = "")}
            aria-label="Clear search"
          >
            <X size={13} />
          </button>
        {/if}
      </div>

      <select
        bind:value={modFilter}
        class="text-xs py-1.5 px-2.5 bg-[var(--bg-card)] border border-[var(--border-color)] rounded-lg text-[var(--text-secondary)] focus:outline-none focus:border-[var(--accent-primary)] cursor-pointer max-w-[140px] truncate"
        title="Filter by mod namespace"
      >
        {#each modNamespaces as ns (ns)}
          <option value={ns}>{ns === "all" ? "All mods" : `@${ns}`}</option>
        {/each}
      </select>
    </div>

    <!-- Right Header Actions -->
    <div class="flex items-center gap-2 flex-shrink-0">
      <button
        type="button"
        class="hud-icon-btn p-2 rounded-lg border border-[var(--border-color)] bg-[var(--bg-card)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] transition"
        title="Keyboard shortcuts & help"
        onclick={() => (showHelp = !showHelp)}
      >
        <Keyboard size={15} />
      </button>

      <button
        type="button"
        class="hud-icon-btn p-2 rounded-lg border transition {showBookmarks ? 'border-amber-400/40 bg-amber-400/10 text-[var(--accent-warning)]' : 'border-[var(--border-color)] bg-[var(--bg-card)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]'}"
        title="Toggle bookmarks sidebar"
        onclick={() => (showBookmarks = !showBookmarks)}
      >
        <Bookmark size={15} />
      </button>

      {#if runtimeStatus?.supported && !runtimeStatus.connected}
        <button
          type="button"
          class="flex items-center gap-2 px-3 py-1.5 text-xs font-semibold rounded-lg bg-emerald-500/10 text-[var(--accent-primary)] border border-emerald-500/25 hover:bg-emerald-500/20 transition disabled:opacity-50"
          onclick={launchJeiLive}
          disabled={!$projectPath || loading || projectLaunching}
          title={projectLaunching ? "Launch in progress" : runtimeStatus.message}
        >
          <Play size={13} /> {projectLaunching ? "Launching…" : "Launch Live JEI"}
        </button>
      {/if}

      {#if pendingRemoves.size > 0}
        <button
          type="button"
          class="flex items-center gap-2 px-3 py-1.5 text-xs font-semibold rounded-lg bg-amber-500/15 text-[var(--accent-warning)] border border-amber-500/30 hover:bg-amber-500/25 transition"
          onclick={flushRemoves}
        >
          <FileCode size={13} /> Write {pendingRemoves.size} removes
        </button>
      {/if}

      <button
        type="button"
        class="flex items-center gap-2 px-3.5 py-1.5 text-xs font-bold rounded-lg bg-[var(--accent-primary)] text-[var(--on-accent)] hover:brightness-110 shadow-sm transition disabled:opacity-50"
        onclick={() => openNewRecipeEditor("crafting")}
        disabled={!$projectPath || loading}
      >
        <Plus size={14} /> New recipe
      </button>

      <button
        type="button"
        class="hud-icon-btn p-2 rounded-lg border border-[var(--border-color)] bg-[var(--bg-card)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] transition disabled:opacity-50"
        onclick={() => loadRecipes(true, true)}
        disabled={!$projectPath || loading}
        title="Rescan recipe files & datapacks"
      >
        <RefreshCw size={15} class={loading ? "spin" : ""} />
      </button>
    </div>
  </header>

  <!-- Help & Shortcuts Banner -->
  {#if showHelp}
    <div class="flex flex-wrap items-center gap-3 px-4 py-2 bg-[var(--bg-secondary)] border-b border-[var(--border-color)] text-xs text-[var(--text-secondary)]">
      <span><kbd>R</kbd> Recipes for item</span>
      <span><kbd>U</kbd> Uses for item</span>
      <span><kbd>B</kbd> Bookmark toggle</span>
      <span><kbd>←</kbd><kbd>→</kbd> Recipe pages</span>
      <span><kbd>Backspace</kbd> History back</span>
      <span class="text-[var(--text-muted)]">| LMB = Recipes · RMB = Uses</span>
      <button type="button" class="ml-auto text-[var(--text-muted)] hover:text-[var(--text-primary)]" onclick={() => (showHelp = false)}>
        <X size={14} />
      </button>
    </div>
  {/if}

  <!-- Error / Success Notices -->
  {#if error}
    <div class="px-4 py-2 bg-red-500/10 border-b border-red-500/20 text-[var(--accent-danger)] text-xs flex items-center justify-between">
      <span>{error}</span>
      <button type="button" class="text-[var(--accent-danger)] hover:text-[var(--accent-danger)] font-bold" onclick={() => (error = null)}>×</button>
    </div>
  {/if}
  {#if message}
    <div class="px-4 py-2 bg-emerald-500/10 border-b border-emerald-500/20 text-[var(--accent-primary)] text-xs flex items-center justify-between">
      <span>{message}</span>
      <button type="button" class="text-[var(--accent-primary)] hover:text-[var(--accent-primary)] font-bold" onclick={() => (message = null)}>×</button>
    </div>
  {/if}

  <!-- Main Stage -->
  {#if !$projectPath}
    <div class="flex-1 flex flex-col items-center justify-center p-8 text-center text-[var(--text-muted)]">
      <Package size={48} class="mb-3 opacity-40 text-[var(--accent-primary)]" />
      <h3 class="text-base font-bold text-[var(--text-primary)] mb-1">Open a project to explore recipes</h3>
      <p class="text-xs max-w-sm leading-relaxed">
        Scan mod JARs, datapacks, and KubeJS recipes to inspect or craft custom items.
      </p>
    </div>
  {:else if (loading || !catalogReady) && !editorOpen}
    <div class="flex-1 flex flex-col items-center justify-center p-8 text-center text-[var(--text-muted)]">
      <RefreshCw size={44} class="spin mb-3 text-[var(--accent-primary)]" />
      <h3 class="text-base font-bold text-[var(--text-primary)] mb-1">Indexing recipe catalog…</h3>
      <p class="text-xs max-w-sm leading-relaxed">
        Reading mod data packs and item tags. This will take just a moment.
      </p>
    </div>
  {:else if recipes.length === 0 && !editorOpen}
    <div class="flex-1 flex flex-col items-center justify-center p-8 text-center text-[var(--text-muted)]">
      <Grid3x3 size={48} class="mb-3 opacity-40 text-[var(--accent-primary)]" />
      <h3 class="text-base font-bold text-[var(--text-primary)] mb-1">No recipes found</h3>
      {#if scanMeta?.vanillaJarFound === false}
        <p class="text-xs max-w-sm leading-relaxed mb-4">
          Vanilla Minecraft client jar is not found. Download it to index vanilla recipes.
        </p>
      {:else}
        <p class="text-xs max-w-sm leading-relaxed mb-4">
          Install mods in your profile or create custom KubeJS recipes.
        </p>
      {/if}
      <div class="flex gap-2">
        <button
          type="button"
          class="px-4 py-2 bg-[var(--accent-primary)] text-[var(--on-accent)] font-bold text-xs rounded-lg hover:brightness-110 shadow-sm"
          onclick={() => loadRecipes()}
        >
          Scan now
        </button>
        <button
          type="button"
          class="px-4 py-2 bg-[var(--bg-card)] border border-[var(--border-color)] text-[var(--text-primary)] font-semibold text-xs rounded-lg hover:bg-[var(--bg-hover)]"
          onclick={() => openNewRecipeEditor("crafting")}
        >
          + Create first recipe
        </button>
      </div>
    </div>
  {:else}
    <!-- Split Layout: Left/Center Recipe Canvas Stage + Right Item Catalog Grid -->
    <div class="flex-1 flex min-h-0 items-stretch overflow-hidden">
      <!-- Central Recipe View Area -->
      <main class="flex-1 flex flex-col min-w-0 min-h-0 bg-[var(--bg-primary)] border-r border-[var(--border-color)] overflow-hidden">
        {#if editorOpen}
          <!-- Recipe Editor Mode -->
          <div class="flex-1 flex flex-col min-h-0 p-4 overflow-y-auto">
            <div class="flex items-center justify-between pb-3 mb-4 border-b border-[var(--border-color)]">
              <div class="flex items-center gap-3">
                <span class="mc-slot mini" style="--hue: {itemHue(editOutput ?? editTagId)}">
                  {#if editorKind === "tags"}
                    <span class="letter">#</span>
                  {:else if editOutput && iconSrc(editOutput)}
                    <img src={iconSrc(editOutput)} alt="" class="slot-icon" />
                  {:else}
                    <Plus size={14} class="text-[var(--accent-primary)]" />
                  {/if}
                </span>
                <div>
                  <h2 class="text-sm font-bold text-[var(--text-primary)] m-0">
                    {#if editorKind === "tags"}
                      {editTagId ? `Tag #${editTagId}` : "Item Tags Editor"}
                    {:else}
                      {replaceRecipeId ? `Edit Recipe (${replaceRecipeId})` : `New ${editorKind} Recipe`}
                    {/if}
                  </h2>
                  <span class="text-xs text-[var(--text-muted)]">
                    {replaceRecipeId ? "Replaces existing ID in KubeJS" : "Saves to server_scripts/tuffbox_recipes.js"}
                  </span>
                </div>
              </div>

              <!-- Recipe Kind Switcher -->
              {#if !replaceRecipeId}
                <div class="flex bg-[var(--bg-card)] p-1 rounded-lg border border-[var(--border-color)] gap-2">
                  {#each EDITOR_KINDS as k (k.id)}
                    {@const KindIcon = k.icon}
                    <button
                      type="button"
                      class="px-2.5 py-1 text-xs font-semibold rounded-md transition flex items-center gap-2 {editorKind === k.id ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)]'}"
                      onclick={() => {
                        editorKind = k.id;
                        paletteMode = k.id === "tags" ? "tags" : "items";
                      }}
                    >
                      <KindIcon size={13} />
                      <span>{k.label}</span>
                    </button>
                  {/each}
                </div>
              {/if}
            </div>

            <!-- Editor Work Area -->
            <div class="flex-1 flex flex-col items-center justify-center p-2 min-h-0">
              {#if editorKind === "crafting"}
                <div class="flex gap-2 mb-3">
                  <button
                    type="button"
                    class="px-3 py-1 text-xs font-bold rounded-md border transition {editShaped ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] border-transparent' : 'border-[var(--border-color)] text-[var(--text-muted)] hover:bg-[var(--bg-hover)]'}"
                    onclick={() => (editShaped = true)}
                  >
                    Shaped (Exact 3×3)
                  </button>
                  <button
                    type="button"
                    class="px-3 py-1 text-xs font-bold rounded-md border transition {!editShaped ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] border-transparent' : 'border-[var(--border-color)] text-[var(--text-muted)] hover:bg-[var(--bg-hover)]'}"
                    onclick={() => (editShaped = false)}
                  >
                    Shapeless
                  </button>
                </div>

                <div class="mc-panel p-4 bg-[var(--bg-secondary)] border-2 border-[var(--border-color)] rounded-xl shadow-xl flex items-center gap-6">
                  <div
                    class="craft-grid grid grid-cols-3 gap-2 p-2 bg-[var(--bg-primary)] border border-[var(--border-color)] rounded-lg"
                    role="presentation"
                    ondragenter={allowEditorDrop}
                    ondragover={allowEditorDrop}
                  >
                    {#each editGrid as slotId, i (i)}
                      <button
                        type="button"
                        class="mc-slot w-14 h-14 rounded bg-[var(--bg-elevated)] border border-[var(--border-color)] hover:border-[var(--accent-primary)] flex items-center justify-center relative cursor-pointer"
                        class:empty={!slotId}
                        style="--hue: {itemHue(slotId ?? '')}"
                        title={slotId ? `${slotId}\nRight-click to remove` : "Drop item or click to place"}
                        ondragenter={allowEditorDrop}
                        ondragover={allowEditorDrop}
                        ondrop={(e) => onDropGrid(e, i)}
                        oncontextmenu={(e) => { e.preventDefault(); setEditSlot(i, null); }}
                        onclick={() => clearEditSlotIfClick(i)}
                      >
                        {#if slotId && iconSrc(slotId)}
                          <img src={iconSrc(slotId)} alt="" class="slot-icon w-10 h-10 object-contain pixelated" onerror={() => onIconError(slotId)} />
                        {:else if slotId}
                          <span class="letter font-bold text-xs text-white">{slotId.startsWith("#") ? "#" : prettifyItem(slotId).slice(0, 3)}</span>
                        {:else}
                          <span class="text-[12px] text-[var(--text-muted)] font-mono">{i + 1}</span>
                        {/if}
                      </button>
                    {/each}
                  </div>

                  <ArrowRight size={28} class="text-[var(--text-secondary)]" />

                  <div class="flex flex-col items-center gap-2">
                    <span class="text-[12px] font-bold text-[var(--text-secondary)] uppercase tracking-wider">Output</span>
                    <button
                      type="button"
                      class="mc-slot out w-16 h-16 rounded-lg bg-[var(--bg-elevated)] border-2 border-[var(--accent-primary)] hover:brightness-110 flex items-center justify-center relative cursor-pointer shadow-md"
                      class:empty={!editOutput}
                      style="--hue: {itemHue(editOutput ?? '')}"
                      title={editOutput ? `${editOutput}\nShift+click to change count (${editCount})` : "Drop output item here"}
                      ondragenter={allowEditorDrop}
                      ondragover={allowEditorDrop}
                      ondrop={onDropOutput}
                      oncontextmenu={(e) => { e.preventDefault(); editOutput = null; editCount = 1; }}
                      onclick={onOutputClick}
                    >
                      {#if editOutput && iconSrc(editOutput)}
                        <img src={iconSrc(editOutput)} alt="" class="slot-icon w-12 h-12 object-contain pixelated" onerror={() => onIconError(editOutput)} />
                      {:else if editOutput}
                        <span class="letter font-bold text-xs text-white">{prettifyItem(editOutput).slice(0, 3)}</span>
                      {:else}
                        <Plus size={20} class="text-[var(--text-muted)]" />
                      {/if}
                      {#if editOutput && editCount > 1}
                        <em class="absolute bottom-1 right-1.5 text-xs font-black text-[var(--accent-warning)] font-mono not-italic">{editCount}</em>
                      {/if}
                    </button>
                    <span class="text-[12px] text-[var(--text-muted)]">Shift+Click for count</span>
                  </div>
                </div>

              {:else if isCookingKind(editorKind) || editorKind === "stonecutting"}
                <!-- Furnace / Cooker / Stonecutter -->
                <div class="mc-panel p-5 bg-[var(--bg-secondary)] border-2 border-[var(--border-color)] rounded-xl shadow-xl flex items-center gap-6">
                  <div class="flex flex-col items-center gap-2">
                    <span class="text-[12px] font-bold text-[var(--text-secondary)] uppercase tracking-wider">Input</span>
                    <button
                      type="button"
                      class="mc-slot w-16 h-16 rounded-lg bg-[var(--bg-elevated)] border border-[var(--border-color)] hover:border-[var(--accent-primary)] flex items-center justify-center relative cursor-pointer"
                      class:empty={!editInput}
                      style="--hue: {itemHue(editInput ?? '')}"
                      title={editInput ?? "Drop input item"}
                      ondragenter={allowEditorDrop}
                      ondragover={allowEditorDrop}
                      ondrop={onDropSingleInput}
                      oncontextmenu={(e) => { e.preventDefault(); editInput = null; }}
                      onclick={clearInputIfClick}
                    >
                      {#if editInput && iconSrc(editInput)}
                        <img src={iconSrc(editInput)} alt="" class="slot-icon w-12 h-12 object-contain pixelated" onerror={() => onIconError(editInput)} />
                      {:else if editInput}
                        <span class="letter font-bold text-xs text-white">{editInput.startsWith("#") ? "#" : prettifyItem(editInput).slice(0, 3)}</span>
                      {:else}
                        <Plus size={20} class="text-[var(--text-muted)]" />
                      {/if}
                    </button>
                  </div>

                  <div class="flex flex-col items-center gap-2 px-2">
                    {#if isCookingKind(editorKind)}
                      <Flame size={28} class="text-[var(--accent-warning)] animate-pulse" />
                      <div class="flex items-center gap-2 text-xs">
                        <label class="text-[12px] text-[var(--text-secondary)]">XP <input type="number" step="0.1" min="0" bind:value={editXp} class="w-14 px-1.5 py-0.5 bg-[var(--bg-primary)] border border-[var(--border-color)] rounded text-[var(--text-primary)] text-center" /></label>
                        <label class="text-[12px] text-[var(--text-secondary)]">Ticks <input type="number" min="1" bind:value={editCookTime} class="w-14 px-1.5 py-0.5 bg-[var(--bg-primary)] border border-[var(--border-color)] rounded text-[var(--text-primary)] text-center" /></label>
                      </div>
                    {:else}
                      <Scissors size={28} class="text-[var(--text-secondary)]" />
                    {/if}
                  </div>

                  <ArrowRight size={28} class="text-[var(--text-secondary)]" />

                  <div class="flex flex-col items-center gap-2">
                    <span class="text-[12px] font-bold text-[var(--text-secondary)] uppercase tracking-wider">Output</span>
                    <button
                      type="button"
                      class="mc-slot out w-16 h-16 rounded-lg bg-[var(--bg-elevated)] border-2 border-[var(--accent-primary)] hover:brightness-110 flex items-center justify-center relative cursor-pointer shadow-md"
                      class:empty={!editOutput}
                      style="--hue: {itemHue(editOutput ?? '')}"
                      title={editOutput ? `${editOutput}\nShift+click for count` : "Drop output item"}
                      ondragenter={allowEditorDrop}
                      ondragover={allowEditorDrop}
                      ondrop={onDropOutput}
                      oncontextmenu={(e) => { e.preventDefault(); editOutput = null; editCount = 1; }}
                      onclick={onOutputClick}
                    >
                      {#if editOutput && iconSrc(editOutput)}
                        <img src={iconSrc(editOutput)} alt="" class="slot-icon w-12 h-12 object-contain pixelated" onerror={() => onIconError(editOutput)} />
                      {:else if editOutput}
                        <span class="letter font-bold text-xs text-white">{prettifyItem(editOutput).slice(0, 3)}</span>
                      {:else}
                        <Plus size={20} class="text-[var(--text-muted)]" />
                      {/if}
                      {#if editOutput && editCount > 1}
                        <em class="absolute bottom-1 right-1.5 text-xs font-black text-[var(--accent-warning)] font-mono not-italic">{editCount}</em>
                      {/if}
                    </button>
                  </div>
                </div>

              {:else if editorKind === "smithing"}
                <!-- Smithing Table -->
                <div class="mc-panel p-5 bg-[var(--bg-secondary)] border-2 border-[var(--border-color)] rounded-xl shadow-xl flex items-center gap-4">
                  {#each [
                    { key: "template", label: "Template", val: editTemplate },
                    { key: "base", label: "Base Item", val: editBase },
                    { key: "addition", label: "Addition", val: editAddition },
                  ] as slot, i (slot.key)}
                    <div class="flex flex-col items-center gap-2">
                      <span class="text-[12px] font-bold text-[var(--text-secondary)] uppercase">{slot.label}</span>
                      <button
                        type="button"
                        class="mc-slot w-14 h-14 rounded bg-[var(--bg-elevated)] border border-[var(--border-color)] hover:border-[var(--accent-primary)] flex items-center justify-center relative cursor-pointer"
                        class:empty={!slot.val}
                        style="--hue: {itemHue(slot.val ?? '')}"
                        ondragenter={allowEditorDrop}
                        ondragover={allowEditorDrop}
                        ondrop={(e) => onDropSmithing(e, slot.key as "template" | "base" | "addition")}
                        oncontextmenu={(e) => {
                          e.preventDefault();
                          if (slot.key === "template") editTemplate = null;
                          else if (slot.key === "base") editBase = null;
                          else editAddition = null;
                        }}
                        onclick={() => clearSmithingIfClick(slot.key as "template" | "base" | "addition")}
                      >
                        {#if slot.val && iconSrc(slot.val)}
                          <img src={iconSrc(slot.val)} alt="" class="slot-icon w-10 h-10 object-contain pixelated" onerror={() => onIconError(slot.val)} />
                        {:else if slot.val}
                          <span class="letter font-bold text-xs text-white">{prettifyItem(slot.val).slice(0, 3)}</span>
                        {:else}
                          <Plus size={16} class="text-[var(--text-muted)]" />
                        {/if}
                      </button>
                    </div>
                    {#if i < 2}<span class="text-lg font-bold text-[var(--text-muted)] mt-4">+</span>{/if}
                  {/each}

                  <ArrowRight size={28} class="text-[var(--text-secondary)] mt-4 mx-2" />

                  <div class="flex flex-col items-center gap-2">
                    <span class="text-[12px] font-bold text-[var(--text-secondary)] uppercase">Result</span>
                    <button
                      type="button"
                      class="mc-slot out w-14 h-14 rounded-lg bg-[var(--bg-elevated)] border-2 border-[var(--accent-primary)] flex items-center justify-center relative cursor-pointer shadow-md"
                      class:empty={!editOutput}
                      style="--hue: {itemHue(editOutput ?? '')}"
                      ondragenter={allowEditorDrop}
                      ondragover={allowEditorDrop}
                      ondrop={onDropOutput}
                      oncontextmenu={(e) => { e.preventDefault(); editOutput = null; editCount = 1; }}
                      onclick={onOutputClick}
                    >
                      {#if editOutput && iconSrc(editOutput)}
                        <img src={iconSrc(editOutput)} alt="" class="slot-icon w-10 h-10 object-contain pixelated" onerror={() => onIconError(editOutput)} />
                      {:else if editOutput}
                        <span class="letter font-bold text-xs text-white">{prettifyItem(editOutput).slice(0, 3)}</span>
                      {:else}
                        <Plus size={16} class="text-[var(--text-muted)]" />
                      {/if}
                    </button>
                  </div>
                </div>

              {:else if editorKind === "tags"}
                <!-- Tags Editor -->
                <div class="w-full max-w-xl p-4 bg-[var(--bg-secondary)] border border-[var(--border-color)] rounded-xl flex flex-col gap-3">
                  <label class="flex flex-col gap-2">
                    <span class="text-xs font-semibold text-[var(--text-secondary)]">Tag Identifier (e.g. <code>c:iron_ingots</code> or <code>forge:ores/copper</code>)</span>
                    <input
                      type="text"
                      class="px-3 py-1.5 text-xs font-mono bg-[var(--bg-card)] border border-[var(--border-color)] rounded-lg text-[var(--text-primary)]"
                      placeholder="forge:ingots/copper"
                      bind:value={editTagId}
                      onchange={() => editTagId && loadTagMembers(editTagId)}
                      onblur={() => editTagId && loadTagMembers(editTagId)}
                    />
                  </label>

                  <label class="flex items-center gap-2 cursor-pointer text-xs text-[var(--text-secondary)]">
                    <input type="checkbox" bind:checked={tagRemoveAll} />
                    <span>Clear existing tag items before applying additions (<code>removeAll</code>)</span>
                  </label>

                  <div
                    class="p-3 border-2 border-dashed border-[var(--border-color)] rounded-lg bg-[var(--bg-card)] min-h-16 flex flex-col justify-center"
                    role="region"
                    aria-label="Drop items to add"
                    ondragenter={allowEditorDrop}
                    ondragover={allowEditorDrop}
                    ondrop={onDropTagAdd}
                  >
                    <span class="text-xs font-medium text-[var(--text-muted)]">
                      Drop items or tags here to <strong class="text-[var(--accent-primary)]">add</strong>
                    </span>
                    {#if tagAdd.length > 0}
                      <div class="flex flex-wrap gap-2 mt-2">
                        {#each tagAdd as id (id)}
                          <button
                            type="button"
                            class="text-xs px-2 py-0.5 rounded bg-emerald-500/20 text-[var(--accent-primary)] border border-emerald-500/30 flex items-center gap-2"
                            onclick={() => removePendingAdd(id)}
                          >
                            + {id} <X size={11} />
                          </button>
                        {/each}
                      </div>
                    {/if}
                  </div>

                  <div class="border-t border-[var(--border-color)] pt-2">
                    <span class="text-xs font-semibold text-[var(--text-muted)] block mb-1">
                      Current tag members {tagLoadingMembers ? "…" : `(${tagMembers.length})`}
                    </span>
                    <div class="flex flex-wrap gap-2 max-h-36 overflow-y-auto p-1 bg-[var(--bg-card)] rounded-lg border border-[var(--border-color)]">
                      {#each tagMembers as id (id)}
                        <button
                          type="button"
                          class="text-xs px-2 py-0.5 rounded border transition {tagRemove.includes(id) ? 'bg-red-500/20 text-[var(--accent-danger)] border-red-500/30 line-through' : 'bg-[var(--bg-secondary)] text-[var(--text-secondary)] border-[var(--border-color)] hover:text-[var(--accent-danger)]'}"
                          title={tagRemove.includes(id) ? "Marked for removal" : "Click to mark for removal"}
                          onclick={() => toggleTagRemove(id)}
                        >
                          {tagRemove.includes(id) ? "− " : ""}{id}
                        </button>
                      {/each}
                      {#if !tagLoadingMembers && tagMembers.length === 0}
                        <span class="text-xs text-[var(--text-muted)] p-1">Tag has no known members (new tag)</span>
                      {/if}
                    </div>
                  </div>
                </div>
              {/if}
            </div>

            <!-- Editor Footer Toolbar -->
            <div class="mt-4 pt-3 border-t border-[var(--border-color)] flex flex-wrap items-center justify-between gap-3">
              <div class="flex items-center gap-2 flex-1 max-w-md">
                <input
                  type="text"
                  class="flex-1 px-3 py-1.5 text-xs bg-[var(--bg-card)] border border-[var(--border-color)] rounded-lg text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent-primary)] font-mono"
                  placeholder="Type item ID (e.g. minecraft:iron_ingot)…"
                  bind:value={quickAddId}
                  onkeydown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      submitQuickAdd();
                    }
                  }}
                />
                <button
                  type="button"
                  class="px-3 py-1.5 text-xs font-semibold bg-[var(--bg-card)] border border-[var(--border-color)] rounded-lg text-[var(--text-primary)] hover:bg-[var(--bg-hover)] flex items-center gap-2"
                  onclick={submitQuickAdd}
                >
                  <Plus size={13} /> Add
                </button>
              </div>

              <div class="flex items-center gap-2">
                <button
                  type="button"
                  class="px-3 py-1.5 text-xs font-semibold bg-[var(--bg-card)] border border-[var(--border-color)] rounded-lg text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] flex items-center gap-2"
                  onclick={clearEditorGrid}
                >
                  <Eraser size={13} /> Clear
                </button>
                <button
                  type="button"
                  class="px-3 py-1.5 text-xs font-semibold bg-[var(--bg-card)] border border-[var(--border-color)] rounded-lg text-[var(--text-secondary)] hover:bg-[var(--bg-hover)]"
                  onclick={closeEditor}
                >
                  Cancel
                </button>
                <button
                  type="button"
                  class="px-4 py-1.5 text-xs font-bold rounded-lg bg-[var(--accent-primary)] text-[var(--on-accent)] hover:brightness-110 shadow-sm flex items-center gap-2 disabled:opacity-50"
                  disabled={!editorCanSave() || editorSaving}
                  onclick={saveCraftRecipe}
                >
                  <Save size={13} />
                  {editorSaving ? "Saving…" : (replaceRecipeId || editorKind === "tags") ? "Save" : "Add Recipe"}
                </button>
              </div>
            </div>
          </div>

        {:else if !selectedItem}
          <!-- Empty Selection State -->
          <div class="flex-1 flex flex-col items-center justify-center p-8 text-center text-[var(--text-muted)]">
            <div class="p-6 rounded-2xl bg-[var(--bg-secondary)] border border-[var(--border-color)] shadow-sm max-w-md flex flex-col items-center">
              <div class="w-12 h-12 rounded-xl bg-[var(--accent-primary)]/10 text-[var(--accent-primary)] border border-[var(--accent-primary)]/20 flex items-center justify-center mb-3">
                <Grid3x3 size={24} />
              </div>
              <h3 class="text-base font-bold text-[var(--text-primary)] mb-1">Select an item from the catalog</h3>
              <p class="text-xs text-[var(--text-secondary)] leading-relaxed mb-4">
                Click any item in the grid on the right to view recipes, or right-click to view item uses.
              </p>

              <div class="grid grid-cols-2 gap-2 w-full pt-2 border-t border-[var(--border-color)]">
                <button
                  type="button"
                  class="px-3 py-2 text-xs font-bold rounded-lg bg-[var(--accent-primary)] text-[var(--on-accent)] hover:brightness-110 shadow-sm flex items-center justify-center gap-2"
                  onclick={() => openNewRecipeEditor("crafting")}
                >
                  <Plus size={13} /> + Crafting Recipe
                </button>
                <button
                  type="button"
                  class="px-3 py-2 text-xs font-semibold rounded-lg bg-[var(--bg-card)] border border-[var(--border-color)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] flex items-center justify-center gap-2"
                  onclick={() => openNewRecipeEditor("tags")}
                >
                  <Tag size={13} /> Edit Item Tags
                </button>
              </div>
            </div>
          </div>

        {:else}
          <!-- Active Item Recipe Viewer Stage -->
          <div class="flex-1 flex flex-col min-h-0 overflow-hidden">
            <!-- Top Item Context & Recipe/Uses Switcher -->
            <div class="flex items-center justify-between gap-3 px-4 py-3 border-b border-[var(--border-color)] bg-[var(--bg-secondary)] flex-shrink-0">
              <div class="flex items-center gap-3 min-w-0">
                <button
                  type="button"
                  class="p-1.5 rounded-lg border border-[var(--border-color)] bg-[var(--bg-card)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] disabled:opacity-40"
                  disabled={historyStack.length < 2}
                  onclick={goBack}
                  title="Back (Backspace)"
                >
                  <History size={14} />
                </button>

                <div class="flex items-center gap-2.5 min-w-0">
                  <span class="mc-slot mini w-9 h-9 rounded-lg bg-[var(--bg-elevated)] border border-[var(--border-color)] flex items-center justify-center flex-shrink-0" style="--hue: {itemHue(selectedItem)}">
                    {#if iconSrc(selectedItem)}
                      <img src={iconSrc(selectedItem)} alt="" class="slot-icon w-7 h-7 object-contain pixelated" onerror={() => onIconError(selectedItem)} />
                    {:else}
                      <span class="letter font-bold text-xs text-white">{prettifyItem(selectedItem).slice(0, 2)}</span>
                    {/if}
                  </span>
                  <div class="min-w-0">
                    <strong class="text-sm font-bold text-[var(--text-primary)] truncate block">{prettifyItem(selectedItem)}</strong>
                    <code class="text-[12px] text-[var(--text-muted)] font-mono truncate block">{selectedItem}</code>
                  </div>
                </div>

                <button
                  type="button"
                  class="p-1.5 rounded-lg border transition {bookmarks.includes(selectedItem) ? 'border-amber-400 bg-amber-400/10 text-[var(--accent-warning)]' : 'border-[var(--border-color)] bg-[var(--bg-card)] text-[var(--text-muted)] hover:text-[var(--accent-warning)]'}"
                  title="Bookmark item (B)"
                  onclick={() => toggleBookmark(selectedItem)}
                >
                  <Star size={14} />
                </button>
              </div>

              <!-- Recipes vs Uses Switcher -->
              <div class="flex bg-[var(--bg-card)] p-0.5 rounded-lg border border-[var(--border-color)]">
                <button
                  type="button"
                  class="px-3 py-1 text-xs font-bold rounded-md transition flex items-center gap-2 {focusMode === 'recipes' ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)]'}"
                  onclick={() => { focusMode = "recipes"; recipeIndex = 0; }}
                >
                  <span>Recipes</span>
                  <span class="text-[12px] px-1.5 py-0.2 rounded-full {focusMode === 'recipes' ? 'bg-white/20 text-white' : 'bg-[var(--bg-secondary)] text-[var(--text-muted)]'}">
                    {recipesForItem(selectedItem, "recipes").length}
                  </span>
                </button>

                <button
                  type="button"
                  class="px-3 py-1 text-xs font-bold rounded-md transition flex items-center gap-2 {focusMode === 'uses' ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)]'}"
                  onclick={() => { focusMode = "uses"; recipeIndex = 0; }}
                >
                  <span>Uses</span>
                  <span class="text-[12px] px-1.5 py-0.2 rounded-full {focusMode === 'uses' ? 'bg-white/20 text-white' : 'bg-[var(--bg-secondary)] text-[var(--text-muted)]'}">
                    {recipesForItem(selectedItem, "uses").length}
                  </span>
                </button>
              </div>
            </div>

            <!-- Horizontal Category Tabs -->
            <div class="flex items-center gap-2 px-4 py-2 border-b border-[var(--border-color)] bg-[var(--bg-card)] overflow-x-auto flex-shrink-0">
              {#each categories as cat (cat)}
                {@const IconComp = categoryIconComponent(cat)}
                <button
                  type="button"
                  class="px-3 py-1 text-xs font-semibold rounded-md transition flex items-center gap-2 whitespace-nowrap {categoryFilter === cat ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]'}"
                  onclick={() => {
                    categoryFilter = cat;
                    recipeIndex = 0;
                  }}
                >
                  <IconComp size={13} />
                  <span>{categoryLabel(cat)}</span>
                </button>
              {/each}
            </div>

            <!-- Active Recipe Content Stage -->
            <div class="flex-1 flex flex-col items-center justify-center p-4 overflow-y-auto min-h-0">
              {#if activeRecipes.length === 0}
                <div class="text-center text-[var(--text-muted)] py-12">
                  <p class="text-xs">No {focusMode} found for this item in {categoryLabel(categoryFilter)}.</p>
                </div>
              {:else if currentRecipe}
                <div class="flex flex-col items-center gap-4 max-w-xl w-full">
                  <!-- Pagination Bar -->
                  <div class="flex items-center justify-between w-full max-w-md">
                    <button
                      type="button"
                      class="p-1.5 rounded-lg border border-[var(--border-color)] bg-[var(--bg-card)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] disabled:opacity-40"
                      onclick={prevRecipe}
                      disabled={recipeIndex === 0}
                      title="Previous recipe (Left Arrow)"
                    >
                      <ChevronLeft size={16} />
                    </button>

                    <div class="flex items-center gap-2">
                      <span class="text-xs font-mono font-bold text-[var(--text-primary)]">
                        {recipeIndex + 1} of {activeRecipes.length}
                      </span>
                      <span class="text-xs text-[var(--text-muted)] uppercase font-semibold">
                        · {CATEGORY_META[currentRecipe.layout.category]?.label ?? runtimeCategory(currentRecipe.category)?.title ?? currentRecipe.layout.category}
                      </span>
                      {#if currentRecipe.layout.shapeless}
                        <span class="text-[12px] font-bold text-[var(--accent-secondary)] bg-cyan-400/10 px-1.5 py-0.5 rounded">Shapeless</span>
                      {/if}
                    </div>

                    <button
                      type="button"
                      class="p-1.5 rounded-lg border border-[var(--border-color)] bg-[var(--bg-card)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] disabled:opacity-40"
                      onclick={nextRecipe}
                      disabled={recipeIndex >= activeRecipes.length - 1}
                      title="Next recipe (Right Arrow)"
                    >
                      <ChevronRight size={16} />
                    </button>
                  </div>

                  <!-- Recipe Frame Layout -->
                  <div class="mc-panel p-5 bg-[var(--bg-secondary)] border-2 border-[var(--border-color)] rounded-xl shadow-xl flex items-center justify-center gap-6 w-full max-w-md">
                    {#if currentRecipe.layout.category === "crafting"}
                      <div class="craft-grid grid grid-cols-3 gap-2 p-2 bg-[var(--bg-primary)] border border-[var(--border-color)] rounded-lg">
                        {#each currentRecipe.layout.grid as slot, i (i)}
                          {@const resolved = resolveSlot(slot)}
                          <button
                            type="button"
                            class="mc-slot w-14 h-14 rounded bg-[var(--bg-elevated)] border border-[var(--border-color)] hover:border-[var(--accent-primary)] flex items-center justify-center relative {slot ? 'cursor-pointer' : 'cursor-default'}"
                            class:empty={!slot}
                            style={slot ? `--hue: ${itemHue(resolved?.id ?? '')}` : ''}
                            title={slotTitle(slot)}
                            disabled={!slot}
                            onclick={() => navigateSlot(slot, "uses")}
                            oncontextmenu={(e) => { e.preventDefault(); navigateSlot(slot, "recipes"); }}
                          >
                            {#if slot}
                              {#if iconSrc(resolved?.id, resolved?.iconUrl)}
                                <img src={iconSrc(resolved?.id, resolved?.iconUrl)} alt="" class="slot-icon w-10 h-10 object-contain pixelated" onerror={() => onIconError(resolved?.id)} />
                              {:else}
                                <span class="letter font-bold text-xs text-white">{slotLabel(slot)}</span>
                              {/if}
                              {#if slot.alts && slot.alts.length > 1}
                                <span class="absolute top-1 right-1 w-1.5 h-1.5 rounded-full bg-cyan-400"></span>
                              {/if}
                            {/if}
                          </button>
                        {/each}
                      </div>

                      <ArrowRight size={28} class="text-[var(--text-secondary)] flex-shrink-0" />

                      <button
                        type="button"
                        class="mc-slot out w-16 h-16 rounded-lg bg-[var(--bg-elevated)] border-2 border-[var(--accent-primary)] flex items-center justify-center relative cursor-pointer shadow-md flex-shrink-0 hover:brightness-110"
                        style="--hue: {itemHue(currentRecipe.layout.output.id)}"
                        title={currentRecipe.layout.output.id}
                        onclick={() => navigateSlot(currentRecipe.layout.output, "recipes")}
                        oncontextmenu={(e) => { e.preventDefault(); navigateSlot(currentRecipe.layout.output, "uses"); }}
                      >
                        {#if iconSrc(currentRecipe.layout.output.id, currentRecipe.layout.output.iconUrl)}
                          <img src={iconSrc(currentRecipe.layout.output.id, currentRecipe.layout.output.iconUrl)} alt="" class="slot-icon w-12 h-12 object-contain pixelated" onerror={() => onIconError(currentRecipe.layout.output.id)} />
                        {:else}
                          <span class="letter font-bold text-xs text-white">{prettifyItem(currentRecipe.layout.output.id).slice(0, 3)}</span>
                        {/if}
                        {#if currentRecipe.layout.outputCount > 1}
                          <em class="absolute bottom-1 right-1.5 text-xs font-black text-[var(--accent-warning)] font-mono not-italic">{currentRecipe.layout.outputCount}</em>
                        {/if}
                      </button>

                    {:else if currentRecipe.layout.category === "cooking"}
                      {@const input = resolveSlot(currentRecipe.layout.grid[4])}
                      <button
                        type="button"
                        class="mc-slot w-16 h-16 rounded-lg bg-[var(--bg-elevated)] border border-[var(--border-color)] hover:border-[var(--accent-primary)] flex items-center justify-center relative cursor-pointer"
                        style="--hue: {itemHue(input?.id ?? '')}"
                        title={slotTitle(currentRecipe.layout.grid[4])}
                        onclick={() => navigateSlot(currentRecipe.layout.grid[4], "uses")}
                      >
                        {#if iconSrc(input?.id, input?.iconUrl)}
                          <img src={iconSrc(input?.id, input?.iconUrl)} alt="" class="slot-icon w-12 h-12 object-contain pixelated" onerror={() => onIconError(input?.id)} />
                        {:else}
                          <span class="letter font-bold text-xs text-white">{slotLabel(currentRecipe.layout.grid[4])}</span>
                        {/if}
                      </button>

                      <div class="flex flex-col items-center gap-2">
                        <Flame size={26} class="text-[var(--accent-warning)] animate-pulse" />
                        {#if currentRecipe.layout.cookTime}
                          <span class="text-[12px] font-mono text-[var(--text-secondary)]">{(currentRecipe.layout.cookTime / 20).toFixed(1)}s</span>
                        {/if}
                        {#if currentRecipe.layout.experience}
                          <span class="text-[12px] font-bold text-[var(--accent-primary)] font-mono">+{currentRecipe.layout.experience} XP</span>
                        {/if}
                      </div>

                      <ArrowRight size={28} class="text-[var(--text-secondary)]" />

                      <button
                        type="button"
                        class="mc-slot out w-16 h-16 rounded-lg bg-[var(--bg-elevated)] border-2 border-[var(--accent-primary)] flex items-center justify-center relative cursor-pointer shadow-md"
                        style="--hue: {itemHue(currentRecipe.layout.output.id)}"
                        onclick={() => navigateSlot(currentRecipe.layout.output, "recipes")}
                      >
                        {#if iconSrc(currentRecipe.layout.output.id, currentRecipe.layout.output.iconUrl)}
                          <img src={iconSrc(currentRecipe.layout.output.id, currentRecipe.layout.output.iconUrl)} alt="" class="slot-icon w-12 h-12 object-contain pixelated" onerror={() => onIconError(currentRecipe.layout.output.id)} />
                        {:else}
                          <span class="letter font-bold text-xs text-white">{prettifyItem(currentRecipe.layout.output.id).slice(0, 3)}</span>
                        {/if}
                        {#if currentRecipe.layout.outputCount > 1}
                          <em class="absolute bottom-1 right-1.5 text-xs font-black text-[var(--accent-warning)] font-mono not-italic">{currentRecipe.layout.outputCount}</em>
                        {/if}
                      </button>

                    {:else if currentRecipe.layout.category === "smithing"}
                      {#each [0, 1, 2] as i (i)}
                        {@const slot = currentRecipe.layout.grid[3 + i]}
                        {@const resolved = resolveSlot(slot)}
                        <button
                          type="button"
                          class="mc-slot w-14 h-14 rounded bg-[var(--bg-elevated)] border border-[var(--border-color)] flex items-center justify-center relative {slot ? 'cursor-pointer' : 'cursor-default'}"
                          class:empty={!slot}
                          style={slot ? `--hue: ${itemHue(resolved?.id ?? '')}` : ''}
                          title={slotTitle(slot)}
                          disabled={!slot}
                          onclick={() => navigateSlot(slot, "uses")}
                        >
                          {#if slot}
                            {#if iconSrc(resolved?.id, resolved?.iconUrl)}
                              <img src={iconSrc(resolved?.id, resolved?.iconUrl)} alt="" class="slot-icon w-10 h-10 object-contain pixelated" onerror={() => onIconError(resolved?.id)} />
                            {:else}
                              <span class="letter font-bold text-xs text-white">{slotLabel(slot)}</span>
                            {/if}
                          {/if}
                        </button>
                        {#if i < 2}<span class="text-base font-bold text-[var(--text-muted)]">+</span>{/if}
                      {/each}

                      <ArrowRight size={28} class="text-[var(--text-secondary)]" />

                      <button
                        type="button"
                        class="mc-slot out w-14 h-14 rounded-lg bg-[var(--bg-elevated)] border-2 border-[var(--accent-primary)] flex items-center justify-center relative cursor-pointer"
                        style="--hue: {itemHue(currentRecipe.layout.output.id)}"
                        onclick={() => navigateSlot(currentRecipe.layout.output, "recipes")}
                      >
                        {#if iconSrc(currentRecipe.layout.output.id, currentRecipe.layout.output.iconUrl)}
                          <img src={iconSrc(currentRecipe.layout.output.id, currentRecipe.layout.output.iconUrl)} alt="" class="slot-icon w-10 h-10 object-contain pixelated" onerror={() => onIconError(currentRecipe.layout.output.id)} />
                        {:else}
                          <span class="letter font-bold text-xs text-white">{prettifyItem(currentRecipe.layout.output.id).slice(0, 3)}</span>
                        {/if}
                      </button>

                    {:else}
                      <!-- Generic / Custom layout -->
                      <div class="flex flex-wrap gap-2 items-center justify-center">
                        {#each currentRecipe.layout.grid.filter(Boolean) as slot, idx (resolveSlot(slot)?.id ?? idx)}
                          {@const resolved = resolveSlot(slot)}
                          <button
                            type="button"
                            class="mc-slot w-12 h-12 rounded bg-[var(--bg-elevated)] border border-[var(--border-color)] flex items-center justify-center relative cursor-pointer"
                            style="--hue: {itemHue(resolved?.id ?? '')}"
                            title={slotTitle(slot)}
                            onclick={() => navigateSlot(slot, "uses")}
                          >
                            {#if iconSrc(resolved?.id, resolved?.iconUrl)}
                              <img src={iconSrc(resolved?.id, resolved?.iconUrl)} alt="" class="slot-icon w-9 h-9 object-contain pixelated" onerror={() => onIconError(resolved?.id)} />
                            {:else}
                              <span class="letter font-bold text-xs text-white">{slotLabel(slot)}</span>
                            {/if}
                          </button>
                        {/each}
                      </div>

                      <ArrowRight size={28} class="text-[var(--text-secondary)]" />

                      <button
                        type="button"
                        class="mc-slot out w-14 h-14 rounded-lg bg-[var(--bg-elevated)] border-2 border-[var(--accent-primary)] flex items-center justify-center relative cursor-pointer"
                        style="--hue: {itemHue(currentRecipe.layout.output.id)}"
                        onclick={() => navigateSlot(currentRecipe.layout.output, "recipes")}
                      >
                        {#if iconSrc(currentRecipe.layout.output.id, currentRecipe.layout.output.iconUrl)}
                          <img src={iconSrc(currentRecipe.layout.output.id, currentRecipe.layout.output.iconUrl)} alt="" class="slot-icon w-10 h-10 object-contain pixelated" onerror={() => onIconError(currentRecipe.layout.output.id)} />
                        {:else}
                          <span class="letter font-bold text-xs text-white">{prettifyItem(currentRecipe.layout.output.id).slice(0, 3)}</span>
                        {/if}
                      </button>
                    {/if}
                  </div>

                  <!-- Metadata Chips -->
                  <div class="flex items-center gap-2 flex-wrap justify-center">
                    <span class="text-[12px] font-mono px-2 py-0.5 rounded bg-[var(--bg-card)] border border-[var(--border-color)] text-[var(--text-muted)]">
                      {currentRecipe.recipeType.replace(/^minecraft:/, '')}
                    </span>
                    <span class="text-[12px] font-semibold px-2 py-0.5 rounded bg-[var(--accent-primary)]/10 text-[var(--accent-primary)] border border-[var(--accent-primary)]/20 truncate max-w-xs" title={currentRecipe.sourceFile}>
                      {currentRecipe.modSource}
                    </span>
                    <code class="text-[12px] font-mono text-[var(--text-muted)]">{currentRecipe.id}</code>
                  </div>

                  <!-- Recipe Action Buttons -->
                  <div class="flex items-center gap-2 flex-wrap justify-center pt-2">
                    {#if ["crafting", "cooking", "smithing", "stonecutting"].includes(editableCategory(currentRecipe))}
                      <button
                        type="button"
                        class="px-3 py-1.5 text-xs font-bold rounded-lg bg-[var(--bg-card)] border border-[var(--border-color)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] flex items-center gap-2"
                        onclick={() => openEditRecipe(currentRecipe)}
                      >
                        <Pencil size={13} /> Edit Recipe
                      </button>
                    {:else}
                      <button
                        type="button"
                        class="px-3 py-1.5 text-xs font-bold rounded-lg bg-[var(--bg-card)] border border-[var(--border-color)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] flex items-center gap-2"
                        onclick={() => openReplaceWithCrafting(currentRecipe)}
                      >
                        <Pencil size={13} /> Replace with Crafting
                      </button>
                    {/if}

                    <button
                      type="button"
                      class="px-3 py-1.5 text-xs font-semibold rounded-lg bg-[var(--bg-card)] border border-[var(--border-color)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] flex items-center gap-2"
                      onclick={() => copyKubeJS(currentRecipe)}
                      title="Copy KubeJS remove script snippet"
                    >
                      {#if snippetCopied}
                        <Check size={13} class="text-[var(--accent-primary)]" />
                        <span class="text-[var(--accent-primary)] font-bold">Copied</span>
                      {:else}
                        <Copy size={13} /> Copy KubeJS
                      {/if}
                    </button>

                    <button
                      type="button"
                      class="px-3 py-1.5 text-xs font-semibold rounded-lg bg-red-500/10 border border-red-500/20 text-[var(--accent-danger)] hover:bg-red-500/20 flex items-center gap-2"
                      onclick={() => queueRemove(currentRecipe)}
                      title="Queue this recipe for removal in KubeJS"
                    >
                      <Trash2 size={13} /> Queue Remove
                    </button>
                  </div>
                </div>
              {/if}
            </div>
          </div>
        {/if}
      </main>

      <!-- Right Item Catalog Sidebar (JEI Item Matrix) -->
      <aside class="w-96 min-w-[340px] max-w-md flex flex-col min-h-0 bg-[var(--bg-secondary)] overflow-hidden flex-shrink-0">
        <!-- Bookmark Strip (if any) -->
        {#if showBookmarks && bookmarkItems.length > 0}
          <div class="px-3 py-2 border-b border-[var(--border-color)] bg-[var(--bg-card)] flex flex-col gap-2 flex-shrink-0">
            <div class="flex items-center justify-between">
              <span class="text-[12px] font-bold uppercase tracking-wider text-[var(--accent-warning)] flex items-center gap-2">
                <Star size={11} /> Bookmarks ({bookmarkItems.length})
              </span>
            </div>
            <div class="grid grid-cols-8 gap-2 max-h-20 overflow-y-auto">
              {#each bookmarkItems as item (item.id)}
                <button
                  type="button"
                  class="item-slot w-8 h-8 rounded bg-[var(--bg-secondary)] border border-[var(--border-color)] hover:border-amber-400 flex items-center justify-center relative cursor-pointer {selectedItem === item.id ? 'border-amber-400 shadow-sm' : ''}"
                  style="--hue: {itemHue(item.id)}"
                  title="{item.id} — {item.name}"
                  draggable={editorOpen ? "true" : "false"}
                  ondragstart={(e) => editorOpen && onDragStartItem(e, item.id)}
                  onclick={() => editorOpen ? placePaletteItem(item.id) : selectItem(item.id, "recipes")}
                  oncontextmenu={(e) => { e.preventDefault(); !editorOpen && selectItem(item.id, "uses"); }}
                >
                  {#if iconSrc(item.id)}
                    <img src={iconSrc(item.id)} alt="" class="w-6 h-6 object-contain pixelated" onerror={() => onIconError(item.id)} />
                  {:else}
                    <span class="text-[12px] font-bold text-[var(--text-primary)] uppercase">{item.name.slice(0, 2)}</span>
                  {/if}
                </button>
              {/each}
            </div>
          </div>
        {/if}

        <!-- Catalog Header -->
        <div class="flex items-center justify-between px-3 py-2 border-b border-[var(--border-color)] bg-[var(--bg-secondary)] flex-shrink-0">
          {#if editorOpen}
            <div class="flex bg-[var(--bg-card)] p-0.5 rounded-lg border border-[var(--border-color)]">
              <button
                type="button"
                class="px-2.5 py-0.5 text-xs font-bold rounded-md transition {paletteMode === 'items' ? 'bg-[var(--accent-primary)] text-[var(--on-accent)]' : 'text-[var(--text-muted)]'}"
                onclick={() => { paletteMode = "items"; itemPage = 0; }}
              >
                Items ({filteredItems.length})
              </button>
              <button
                type="button"
                class="px-2.5 py-0.5 text-xs font-bold rounded-md transition {paletteMode === 'tags' ? 'bg-[var(--accent-primary)] text-[var(--on-accent)]' : 'text-[var(--text-muted)]'}"
                onclick={() => { paletteMode = "tags"; itemPage = 0; ensureTagsLoaded(); }}
              >
                Tags ({filteredTags.length})
              </button>
            </div>
          {:else}
            <span class="text-xs font-bold text-[var(--text-primary)]">Item Catalog</span>
            <span class="text-[12px] font-mono text-[var(--text-muted)]">{filteredItems.length} items</span>
          {/if}

          <!-- Page Navigation -->
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="p-1 rounded border border-[var(--border-color)] bg-[var(--bg-card)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] disabled:opacity-40"
              disabled={itemPage === 0}
              onclick={() => (itemPage = Math.max(0, itemPage - 1))}
            >
              <ChevronLeft size={13} />
            </button>
            <span class="text-[12px] font-mono font-semibold text-[var(--text-muted)]">
              {itemPage + 1} / {overlayPageCount}
            </span>
            <button
              type="button"
              class="p-1 rounded border border-[var(--border-color)] bg-[var(--bg-card)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] disabled:opacity-40"
              disabled={itemPage >= overlayPageCount - 1}
              onclick={() => (itemPage = Math.min(overlayPageCount - 1, itemPage + 1))}
            >
              <ChevronRight size={13} />
            </button>
          </div>
        </div>

        <!-- Catalog Grid (High Density) -->
        <div class="flex-1 p-2 overflow-y-auto min-h-0 bg-[var(--bg-card)]">
          {#if editorOpen && paletteMode === "tags"}
            {#if tagsLoading}
              <div class="text-center text-xs text-[var(--text-muted)] py-8">Loading tags…</div>
            {:else if pageTags.length === 0}
              <div class="text-center text-xs text-[var(--text-muted)] py-8">No tags match query</div>
            {:else}
              <div class="flex flex-col gap-2">
                {#each pageTags as tagId (tagId)}
                  <button
                    type="button"
                    class="text-left px-2.5 py-1.5 rounded-lg border text-xs font-mono truncate transition flex items-center gap-2 {editorKind === 'tags' && editTagId === tagId.replace(/^#/, '') ? 'bg-[var(--accent-primary)]/15 border-[var(--accent-primary)] text-[var(--on-accent)] font-bold' : 'bg-[var(--bg-secondary)] border-[var(--border-color)] text-[var(--text-secondary)] hover:bg-[var(--bg-hover)]'}"
                    draggable="true"
                    ondragstart={(e) => onDragStartItem(e, tagId)}
                    onclick={() => {
                      if (editorKind === "tags") selectEditTag(tagId);
                    }}
                  >
                    <span class="text-[var(--accent-secondary)] font-bold">#</span>
                    <span class="truncate">{tagId}</span>
                  </button>
                {/each}
              </div>
            {/if}
          {:else}
            <div class="grid grid-cols-8 gap-2">
              {#each pageItems as item (item.id)}
                <button
                  type="button"
                  class="item-slot w-10 h-10 rounded-lg bg-[var(--bg-secondary)] border border-[var(--border-color)] hover:border-[var(--accent-primary)] hover:scale-105 transition flex items-center justify-center relative cursor-pointer {selectedItem === item.id ? 'border-[var(--accent-primary)] shadow-[0_0_8px_color-mix(in_srgb,var(--accent-primary)_30%,transparent)] bg-[var(--bg-hover)]' : ''}"
                  style="--hue: {itemHue(item.id)}"
                  title="{item.name}\n{item.id}\nLMB: Recipes ({item.recipeCount}) · RMB: Uses ({item.useCount})"
                  draggable={editorOpen ? "true" : "false"}
                  ondragstart={(e) => editorOpen && onDragStartItem(e, item.id)}
                  onclick={() => editorOpen ? placePaletteItem(item.id) : selectItem(item.id, "recipes")}
                  oncontextmenu={(e) => { e.preventDefault(); !editorOpen && selectItem(item.id, "uses"); }}
                  onauxclick={(e) => {
                    if (e.button === 1) {
                      e.preventDefault();
                      toggleBookmark(item.id);
                    }
                  }}
                >
                  {#if iconSrc(item.id)}
                    <img src={iconSrc(item.id)} alt="" class="w-7 h-7 object-contain pixelated pointer-events-none" onerror={() => onIconError(item.id)} />
                  {:else}
                    <span class="text-[12px] font-bold text-[var(--text-primary)] uppercase pointer-events-none">{item.name.slice(0, 2)}</span>
                  {/if}
                  {#if bookmarks.includes(item.id)}
                    <span class="absolute top-1 left-1 w-1.5 h-1.5 rounded-full bg-amber-400 shadow-sm"></span>
                  {/if}
                  {#if !editorOpen && focusCountForItem(item) > 0}
                    <span class="absolute bottom-0.5 right-1 text-[12px] font-mono font-bold text-[var(--accent-warning)] pointer-events-none">
                      {focusCountForItem(item)}
                    </span>
                  {/if}
                </button>
              {/each}
            </div>
          {/if}
        </div>

        <!-- Recent History Footer Strip -->
        {#if historyStack.length > 0}
          <div class="px-3 py-2 border-t border-[var(--border-color)] bg-[var(--bg-secondary)] flex flex-col gap-2 flex-shrink-0">
            <span class="text-[12px] font-bold uppercase tracking-wider text-[var(--text-muted)] flex items-center gap-2">
              <History size={10} /> Recently Viewed
            </span>
            <div class="grid grid-cols-8 gap-2">
              {#each [...historyStack].reverse().slice(0, 8) as id (id)}
                <button
                  type="button"
                  class="w-8 h-8 rounded bg-[var(--bg-secondary)] border border-[var(--border-color)] hover:border-[var(--accent-primary)] flex items-center justify-center cursor-pointer {selectedItem === id ? 'border-[var(--accent-primary)]' : ''}"
                  style="--hue: {itemHue(id)}"
                  title={id}
                  onclick={() => selectItem(id, focusMode, false)}
                >
                  {#if iconSrc(id)}
                    <img src={iconSrc(id)} alt="" class="w-6 h-6 object-contain pixelated" onerror={() => onIconError(id)} />
                  {:else}
                    <span class="text-[12px] font-bold text-[var(--text-primary)] uppercase">{prettifyItem(id).slice(0, 2)}</span>
                  {/if}
                </button>
              {/each}
            </div>
          </div>
        {/if}
      </aside>
    </div>
  {/if}
</div>

<VanillaClientJarPrompt
  open={vanillaPromptOpen}
  version={vanillaPromptVersion || "?"}
  downloadSize={vanillaPromptSize}
  downloading={vanillaDownloading}
  error={vanillaDownloadError}
  ondownload={downloadVanillaJar}
  ondismiss={dismissVanillaPrompt}
/>

<style>
  .recipe-workspace {
    font-family: var(--font-sans, inherit);
    /* Centered cap on 1440p+ — the recipe grid keeps card-sized columns
       instead of stretching edge-to-edge (matches .graph 1840). */
    max-width: min(1840px, 100%);
    margin: 0 auto;
  }
  .pixelated {
    image-rendering: pixelated;
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
