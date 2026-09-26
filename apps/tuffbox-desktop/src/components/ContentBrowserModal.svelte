<script lang="ts">
  import { X, Search, Grid2X2, List, Loader2, SlidersHorizontal } from "@lucide/svelte";
  import { api } from "../lib/api";
  import { projectPath, projectInfo } from "../lib/store";
  import { get } from "svelte/store";
  import ContentFilterSidebar from "./ContentFilterSidebar.svelte";
  import ContentCard from "./ContentCard.svelte";
  import BatchActionBar from "./BatchActionBar.svelte";
  import type { ContentType, ModFilter, ModItem, Provider } from "./contentModels";

  let { open = $bindable(false), onInstalled, contextTags = [], categories = ["adventure", "optimization", "technology", "magic", "library", "decoration", "fabric", "forge", "client", "server"] }: { open?: boolean; onInstalled?: () => void; contextTags?: string[]; categories?: string[] } = $props();
  let filter = $state<ModFilter>({ query: "", provider: "both", contentType: "mod", sort: "relevance", categories: [], gameVersion: null, loader: null });
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
  const providerLabel = $derived(filter.provider === "both" ? "Both providers" : filter.provider === "modrinth" ? "Modrinth" : "CurseForge");
  const instance = $derived($projectInfo);

  function close() { open = false; }
  function key(item: ModItem) { return `${item.provider}:${item.id}`; }
  function mapResult(row: any, provider: Provider): ModItem { return { id: String(row.id), slug: String(row.slug ?? row.id), name: String(row.name ?? row.slug ?? row.id), author: row.author ?? null, description: row.description ?? "", iconUrl: row.iconUrl ?? null, provider, contentType: (row.projectType ?? filter.contentType) as ContentType, downloads: row.downloads ?? null, installed: !!row.installed, updateAvailable: !!row.updateAvailable, categories: row.categories ?? [] }; }

  async function load(reset = false) {
    if (loading || (!hasMore && !reset)) return;
    if (reset) { page = 1; hasMore = true; items = []; }
    loading = true; error = null;
    try {
      const opts = { gameVersion: filter.gameVersion, loader: filter.loader, contentType: filter.contentType, page, pageSize: 24, sort: filter.sort === "date" ? "newest" : filter.sort === "downloads" ? "downloads" : "relevance", p: get(projectPath) ?? undefined };
      const result = await api.mods.searchContent(filter.query, { ...opts, provider: filter.provider });
      const rows: ModItem[] = (result.results ?? []).map((row) => mapResult(row, (row as any).provider === "curseforge" ? "curseforge" : "modrinth"));
      items = reset ? rows : [...items, ...rows]; page += 1; hasMore = rows.length >= 24;
    } catch (e) { error = e instanceof Error ? e.message : String(e); } finally { loading = false; }
  }
  function resetContext() { filter = { ...filter, gameVersion: null, loader: null }; void load(true); }
  function resetFilters() { filter = { ...filter, query: "", categories: [], sort: "relevance" }; void load(true); }
  function toggle(item: ModItem) { const next = new Set(selected); next.has(key(item)) ? next.delete(key(item)) : next.add(key(item)); selected = next; }
  async function installSelected() { const path = get(projectPath); if (!path || !selectedItems.length) return; installing = true; try { const mr = selectedItems.filter((item) => item.provider === "modrinth").map((item) => item.id); const cf = selectedItems.filter((item) => item.provider === "curseforge").map((item) => item.id); if (mr.length) await api.mods.installContentBatch(mr, "modrinth", "both", path); if (cf.length) await api.mods.installContentBatch(cf, "curseforge", "both", path); selected = new Set(); items = items.map((item) => selectedItems.some((chosen) => key(chosen) === key(item)) ? { ...item, installed: true } : item); onInstalled?.(); } catch (e) { error = e instanceof Error ? e.message : String(e); } finally { installing = false; } }
  async function installOne(item: ModItem) { selected = new Set([key(item)]); await installSelected(); }
  function removeOne(item: ModItem) { void api.mods.remove(item.id, get(projectPath) ?? undefined).then(() => { items = items.map((row) => key(row) === key(item) ? { ...row, installed: false } : row); }).catch((e) => error = String(e)); }
  function observe(node: HTMLDivElement) { sentinel = node; observer?.disconnect(); observer = new IntersectionObserver((entries) => { if (entries[0]?.isIntersecting) void load(); }, { rootMargin: "500px" }); observer.observe(node); return { destroy: () => observer?.disconnect() }; }
  $effect(() => { if (open) void load(true); });
</script>

{#if open}
  <div class="fixed inset-0 z-[80] flex items-center justify-center bg-black/60 p-4 backdrop-blur-md" role="presentation" onclick={(event) => event.target === event.currentTarget && close()}>
    <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
    <section class="flex h-[min(88vh,820px)] w-[min(1400px,100%)] flex-col overflow-hidden rounded-2xl border border-white/[0.08] bg-neutral-950/85 shadow-2xl backdrop-blur-2xl" role="dialog" aria-modal="true" aria-label="Add content">
      <header class="flex flex-wrap items-center gap-3 border-b border-white/[0.08] p-4"><div class="mr-2 flex items-center gap-2"><h1 class="m-0 text-base font-semibold text-neutral-100">Add content</h1><span class="text-xs text-[var(--text-secondary)]">{instance?.name ?? "Global catalog"}</span></div><div class="flex rounded-xl border border-white/10 bg-black/20 p-1">{#each [{id: 'mod', label: 'Mods'}, {id: 'resourcepack', label: 'Resource packs'}, {id: 'shader', label: 'Shaders'}, {id: 'datapack', label: 'Datapacks'}] as type}<button type="button" class="rounded-lg px-3 py-1.5 text-xs {filter.contentType === type.id ? 'bg-emerald-500/15 text-emerald-200' : 'text-[var(--text-secondary)] hover:text-neutral-200'}" onclick={() => { filter = { ...filter, contentType: type.id as ContentType }; void load(true); }}>{type.label}</button>{/each}</div><select class="ml-auto w-auto bg-black/30 text-xs" value={filter.provider} onchange={(event) => { filter = { ...filter, provider: event.currentTarget.value as ModFilter['provider'] }; void load(true); }}><option value="both">Both providers</option><option value="modrinth">Modrinth</option><option value="curseforge">CurseForge</option></select><button type="button" class="rounded-lg p-2 text-[var(--text-secondary)] hover:bg-white/10" onclick={close} aria-label="Close"><X size={17}/></button></header>
      <div class="flex min-h-0 flex-1"><ContentFilterSidebar bind:filter {categories} contextTags={filter.gameVersion || filter.loader ? [filter.gameVersion, filter.loader].filter(Boolean) as string[] : contextTags} onResetContext={resetContext} /><main class="relative min-w-0 flex-1 overflow-y-auto p-5"><div class="mb-5 flex flex-wrap items-center gap-2"><div class="flex min-w-[240px] flex-1 items-center gap-2 rounded-xl border border-white/10 bg-black/30 px-3 py-2 focus-within:border-emerald-500/50"><Search size={16} class="text-[var(--text-secondary)]"/><input class="min-w-0 flex-1 border-0 bg-transparent p-0 text-sm text-neutral-200 outline-none" bind:value={filter.query} onkeydown={(event) => event.key === 'Enter' && load(true)} placeholder="Search mods, shaders, resource packs…" /><kbd class="hidden text-[12px] text-[var(--text-secondary)] sm:inline">Enter</kbd></div><select class="w-auto bg-black/30 text-xs" value={filter.sort} onchange={(event) => { filter = { ...filter, sort: event.currentTarget.value as ModFilter['sort'] }; void load(true); }}><option value="relevance">By relevance</option><option value="downloads">By downloads</option><option value="date">By date</option></select><div class="flex rounded-xl border border-white/10 bg-black/20 p-1"><button type="button" class="rounded-lg p-2 {view === 'grid' ? 'bg-white/10 text-emerald-300' : 'text-[var(--text-secondary)]'}" onclick={() => view = 'grid'} title="Grid"><Grid2X2 size={15}/></button><button type="button" class="rounded-lg p-2 {view === 'list' ? 'bg-white/10 text-emerald-300' : 'text-[var(--text-secondary)]'}" onclick={() => view = 'list'} title="List"><List size={15}/></button></div></div>{#if error}<div class="mb-4 rounded-xl border border-red-500/30 bg-red-500/10 p-3 text-xs text-red-200">{error}</div>{/if}{#if view === 'grid'}<div class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4">{#each items as item (key(item))}<ContentCard {item} selected={selected.has(key(item))} onToggle={() => toggle(item)} onInstall={() => void installOne(item)} onRemove={item.installed ? () => removeOne(item) : undefined}/>{/each}</div>{:else}<div class="space-y-2">{#each items as item (key(item))}<div class="flex items-center gap-3 rounded-xl border border-white/[0.08] bg-white/[0.03] p-3"><ContentCard {item} selected={selected.has(key(item))} onToggle={() => toggle(item)} onInstall={() => void installOne(item)} /></div>{/each}</div>{/if}{#if loading}<div class="flex justify-center p-8 text-[var(--text-secondary)]"><Loader2 size={20} class="animate-spin"/></div>{/if}<div use:observe class="h-2" aria-hidden="true"></div></main></div><BatchActionBar count={selected.size} {installing} onInstall={installSelected} onClear={() => selected = new Set()} /></section>
  </div>
{/if}
