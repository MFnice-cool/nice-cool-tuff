<!-- Standalone "Browse content" window (mods-browser.html entry). Same
     install/remove/batch flow as the in-app modal, but hosted in its own
     OS window so the main window stays on any IDE tab. The target instance
     comes from the ?path= query param; ?type= seeds the content tab. -->
<script lang="ts">
  import { untrack } from "svelte";
  import { Search, Grid2X2, List, Loader2, X, FolderOpen } from "@lucide/svelte";
  import { api } from "../lib/api";
  import { toasts } from "../lib/toast";
  import ContentFilterSidebar from "./ContentFilterSidebar.svelte";
  import ContentCard from "./ContentCard.svelte";
  import BatchActionBar from "./BatchActionBar.svelte";
  import type { ContentType, ModFilter, ModItem, Provider } from "./contentModels";

  const query = new URLSearchParams(window.location.search);
  /** Target instance manifest path (required — this window is per-instance). */
  let projectPath = $state(query.get("path") ?? "");
  let instanceName = $state(query.get("path")?.split(/[\\/]/).filter(Boolean).pop() ?? "");
  let instanceMeta = $state("");

  const TYPES: Array<{ id: ContentType; label: string }> = [
    { id: "mod", label: "Mods" },
    { id: "resourcepack", label: "Resource packs" },
    { id: "shader", label: "Shaders" },
    { id: "datapack", label: "Datapacks" },
  ];

  const initialTypeRaw = query.get("type");
  const initialType = TYPES.some((t) => t.id === initialTypeRaw)
    ? (initialTypeRaw as ContentType)
    : "mod";

  let filter = $state<ModFilter>({
    query: "",
    provider: "both",
    contentType: initialType,
    sort: "relevance",
    categories: [],
    gameVersion: null,
    loader: null,
  });
  let items = $state<ModItem[]>([]);
  let selected = $state(new Set<string>());
  let loading = $state(false);
  let installing = $state(false);
  let page = $state(1);
  let hasMore = $state(true);
  let view = $state<"grid" | "list">("grid");
  let error = $state<string | null>(null);
  let sentinel = $state<HTMLDivElement | null>(null);
  let observer: IntersectionObserver | null = null;

  const selectedItems = $derived(items.filter((item) => selected.has(`${item.provider}:${item.id}`)));

  function key(item: ModItem) {
    return `${item.provider}:${item.id}`;
  }

  function mapResult(row: any, provider: Provider): ModItem {
    return {
      id: String(row.id),
      slug: String(row.slug ?? row.id),
      name: String(row.name ?? row.slug ?? row.id),
      author: row.author ?? null,
      description: row.description ?? "",
      iconUrl: row.iconUrl ?? null,
      provider,
      contentType: (row.projectType ?? filter.contentType) as ContentType,
      downloads: row.downloads ?? null,
      installed: !!row.installed,
      updateAvailable: !!row.updateAvailable,
      categories: row.categories ?? [],
    };
  }

  async function load(reset = false) {
    if (!projectPath) {
      error = "No instance selected — reopen the browser from an instance.";
      return;
    }
    if (loading || (!hasMore && !reset)) return;
    if (reset) {
      page = 1;
      hasMore = true;
      items = [];
    }
    loading = true;
    error = null;
    try {
      const opts = {
        gameVersion: filter.gameVersion,
        loader: filter.loader,
        contentType: filter.contentType,
        page,
        pageSize: 24,
        sort:
          filter.sort === "date" ? "newest" : filter.sort === "downloads" ? "downloads" : "relevance",
        p: projectPath,
      };
      const result = await api.mods.searchContent(filter.query, { ...opts, provider: filter.provider });
      const rows: ModItem[] = (result.results ?? []).map((row) =>
        mapResult(row, (row as any).provider === "curseforge" ? "curseforge" : "modrinth"),
      );
      items = reset ? rows : [...items, ...rows];
      page += 1;
      hasMore = rows.length >= 24;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function resetContext() {
    filter = { ...filter, gameVersion: null, loader: null };
    void load(true);
  }
  function resetFilters() {
    filter = { ...filter, query: "", categories: [], sort: "relevance" };
    void load(true);
  }
  function toggle(item: ModItem) {
    const next = new Set(selected);
    if (next.has(key(item))) next.delete(key(item));
    else next.add(key(item));
    selected = next;
  }

  async function installSelected() {
    if (!projectPath || !selectedItems.length) return;
    installing = true;
    try {
      const mr = selectedItems.filter((item) => item.provider === "modrinth").map((item) => item.id);
      const cf = selectedItems.filter((item) => item.provider === "curseforge").map((item) => item.id);
      if (mr.length) await api.mods.installContentBatch(mr, "modrinth", "both", projectPath);
      if (cf.length) await api.mods.installContentBatch(cf, "curseforge", "both", projectPath);
      const done = selectedItems.map(key);
      selected = new Set();
      items = items.map((item) => (done.includes(key(item)) ? { ...item, installed: true } : item));
      toasts.success(`Installed ${done.length} ${done.length === 1 ? "item" : "items"}`);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      installing = false;
    }
  }

  async function installOne(item: ModItem) {
    selected = new Set([key(item)]);
    await installSelected();
  }

  function removeOne(item: ModItem) {
    void api.mods
      .remove(item.id, projectPath)
      .then(() => {
        items = items.map((row) => (key(row) === key(item) ? { ...row, installed: false } : row));
      })
      .catch((e) => (error = String(e)));
  }

  function observe(node: HTMLDivElement) {
    sentinel = node;
    observer?.disconnect();
    observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) void load();
      },
      { rootMargin: "500px" },
    );
    observer.observe(node);
    return { destroy: () => observer?.disconnect() };
  }

  async function openInstanceFolder() {
    try {
      await api.files.openFolder(projectPath, null);
    } catch (e) {
      toasts.error(String(e));
    }
  }

  function closeWindow() {
    void import("@tauri-apps/api/window")
      .then(({ getCurrentWindow }) => getCurrentWindow().close())
      .catch(() => window.close());
  }

  // Another surface (main window) asked this window to switch target pack
  // and/or content type — reload everything for the new target.
  $effect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen<{ path: string; type: ContentType | null }>(
          "mods-browser:navigate",
          (event) => {
            const next = event.payload;
            if (!next?.path) return;
            const nextType =
              next.type != null && TYPES.some((t) => t.id === next.type) ? next.type : null;
            const typeChanged = nextType != null && nextType !== filter.contentType;
            if (next.path === projectPath && !typeChanged) return;
            projectPath = next.path;
            if (nextType) filter = { ...filter, contentType: nextType };
            selected = new Set();
            void loadInstanceHeader();
            void load(true);
          },
        );
      } catch {
        /* event bridge unavailable — window still works standalone */
      }
    })();
    return () => unlisten?.();
  });

  async function loadInstanceHeader() {
    try {
      const info = await api.project.validate(projectPath);
      instanceName = info.name;
      instanceMeta = `${info.minecraftVersion} · ${info.loaderKind}`;
    } catch {
      instanceName = projectPath.split(/[\\/]/).filter(Boolean).pop() ?? "";
      instanceMeta = "";
    }
  }

  // Initial load only — `load` reads `filter`, so it must run untracked or
  // every keystroke in the search box would fire a full reset search.
  // Filter changes reload through their explicit onchange/Enter handlers.
  $effect(() => {
    const target = projectPath;
    if (!target) return;
    untrack(() => {
      void load(true);
    });
  });

  void loadInstanceHeader();
</script>

<div class="mbw-root">
  <header class="mbw-head">
    <div class="mbw-title">
      <h1>Add content</h1>
      <span class="mbw-sub" title={projectPath}>{instanceName || "Instance"}{instanceMeta ? ` · ${instanceMeta}` : ""}</span>
    </div>
    <div class="mbw-types" role="tablist" aria-label="Content type">
      {#each TYPES as type}
        <button
          type="button"
          role="tab"
          aria-selected={filter.contentType === type.id}
          class:active={filter.contentType === type.id}
          onclick={() => {
            filter = { ...filter, contentType: type.id };
            void load(true);
          }}
        >
          {type.label}
        </button>
      {/each}
    </div>
    <select
      class="mbw-provider"
      aria-label="Provider"
      value={filter.provider}
      onchange={(e) => {
        filter = { ...filter, provider: (e.currentTarget as HTMLSelectElement).value as ModFilter["provider"] };
        void load(true);
      }}
    >
      <option value="both">Both providers</option>
      <option value="modrinth">Modrinth</option>
      <option value="curseforge">CurseForge</option>
    </select>
    <button type="button" class="mbw-icon-btn" title="Open instance folder" aria-label="Open instance folder" onclick={() => void openInstanceFolder()}>
      <FolderOpen size={16} />
    </button>
    <button type="button" class="mbw-icon-btn" title="Close window" aria-label="Close window" onclick={closeWindow}>
      <X size={17} />
    </button>
  </header>

  <div class="mbw-body">
    <ContentFilterSidebar
      bind:filter
      categories={["adventure", "optimization", "technology", "magic", "library", "decoration", "fabric", "forge", "client", "server"]}
      contextTags={filter.gameVersion || filter.loader ? ([filter.gameVersion, filter.loader].filter(Boolean) as string[]) : []}
      onResetContext={resetContext}
    />
    <main class="mbw-main">
      <div class="mbw-toolbar">
        <div class="mbw-search">
          <Search size={16} />
          <input
            type="text"
            placeholder="Search mods, shaders, resource packs…"
            aria-label="Search content"
            bind:value={filter.query}
            onkeydown={(e) => e.key === "Enter" && load(true)}
          />
        </div>
        <select
          class="mbw-sort"
          aria-label="Sort"
          value={filter.sort}
          onchange={(e) => {
            filter = { ...filter, sort: (e.currentTarget as HTMLSelectElement).value as ModFilter["sort"] };
            void load(true);
          }}
        >
          <option value="relevance">By relevance</option>
          <option value="downloads">By downloads</option>
          <option value="date">By date</option>
        </select>
        <div class="mbw-views">
          <button type="button" class:active={view === "grid"} title="Grid" aria-label="Grid view" onclick={() => (view = "grid")}>
            <Grid2X2 size={15} />
          </button>
          <button type="button" class:active={view === "list"} title="List" aria-label="List view" onclick={() => (view = "list")}>
            <List size={15} />
          </button>
        </div>
      </div>

      {#if error}
        <div class="mbw-error" role="alert">{error}</div>
      {/if}

      {#if view === "grid"}
        <div class="mbw-grid">
          {#each items as item (key(item))}
            <ContentCard
              {item}
              selected={selected.has(key(item))}
              onToggle={() => toggle(item)}
              onInstall={() => void installOne(item)}
              onRemove={item.installed ? () => removeOne(item) : undefined}
            />
          {/each}
        </div>
      {:else}
        <div class="mbw-list">
          {#each items as item (key(item))}
            <div class="mbw-list-row">
              <ContentCard
                {item}
                selected={selected.has(key(item))}
                onToggle={() => toggle(item)}
                onInstall={() => void installOne(item)}
              />
            </div>
          {/each}
        </div>
      {/if}

      {#if items.length === 0 && loading}
        <div class="mbw-loading"><Loader2 size={20} class="mbw-spin" /> Loading…</div>
      {/if}
      {#if items.length === 0 && !loading && !error}
        <div class="mbw-empty">Nothing found{filter.query ? ` for "${filter.query}"` : ""}. Try different filters or another provider.</div>
      {/if}
      <div use:observe class="mbw-sentinel" aria-hidden="true"></div>
      {#if loading && items.length > 0}
        <div class="mbw-loading"><Loader2 size={18} class="mbw-spin" /></div>
      {/if}
    </main>
  </div>

  <BatchActionBar count={selected.size} {installing} onInstall={installSelected} onClear={() => (selected = new Set())} />
</div>

<style>
  .mbw-root {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--bg-primary);
    color: var(--text-primary);
    overflow: hidden;
  }
  .mbw-head {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border-color);
  }
  .mbw-title {
    display: flex;
    flex-direction: column;
    min-width: 0;
    margin-right: 4px;
  }
  .mbw-title h1 {
    margin: 0;
    font-size: 15px;
    font-weight: 800;
    color: var(--text-primary);
  }
  .mbw-sub {
    font-size: 12px;
    color: var(--text-muted);
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mbw-types {
    display: inline-flex;
    gap: 8px;
    padding: 2px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
  }
  .mbw-types [role="tab"] {
    padding: 5px 10px;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
  }
  .mbw-types [role="tab"]:hover {
    color: var(--text-primary);
  }
  .mbw-types [role="tab"].active {
    background: color-mix(in srgb, var(--accent-primary) 16%, transparent);
    color: var(--accent-primary);
  }
  .mbw-provider,
  .mbw-sort {
    height: 30px;
    padding: 0 8px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    font-size: 12px;
  }
  .mbw-icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    cursor: pointer;
  }
  .mbw-icon-btn:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }
  .mbw-head .mbw-icon-btn:last-child {
    margin-left: auto;
  }
  .mbw-body {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .mbw-main {
    position: relative;
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: 14px;
  }
  .mbw-toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 14px;
  }
  .mbw-search {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 220px;
    height: 34px;
    padding: 0 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
    color: var(--text-muted);
  }
  .mbw-search input {
    flex: 1;
    min-width: 0;
    border: none;
    outline: none;
    background: transparent;
    color: var(--text-primary);
    font-size: 13px;
  }
  .mbw-views {
    display: inline-flex;
    gap: 8px;
    padding: 2px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
  }
  .mbw-views button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }
  .mbw-views button.active {
    background: color-mix(in srgb, var(--accent-primary) 16%, transparent);
    color: var(--accent-primary);
  }
  .mbw-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
    gap: 14px;
  }
  .mbw-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .mbw-list-row {
    min-width: 0;
  }
  .mbw-list-row :global(article) {
    min-height: 0;
  }
  .mbw-error {
    margin-bottom: 12px;
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--accent-danger) 40%, transparent);
    border-radius: var(--border-radius-md);
    background: color-mix(in srgb, var(--accent-danger) 10%, transparent);
    color: var(--text-primary);
    font-size: 12px;
  }
  .mbw-empty {
    padding: 40px 16px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }
  .mbw-loading {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 18px;
    color: var(--text-muted);
    font-size: 13px;
  }
  .mbw-sentinel {
    height: 8px;
  }
  /* Applied via the class prop on lucide components — needs :global. */
  .mbw-root :global(.mbw-spin) {
    animation: mbw-spin 0.9s linear infinite;
  }
  @keyframes mbw-spin {
    to {
      transform: rotate(360deg);
    }
  }
  :global(.potato-pc .mbw-spin) {
    animation: none;
  }
</style>
