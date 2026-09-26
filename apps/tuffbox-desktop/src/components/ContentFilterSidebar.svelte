<script lang="ts">
  import { Check, RotateCcw, Search, X } from "@lucide/svelte";
  import type { ModFilter } from "./contentModels";
  let { filter = $bindable(), categories = [], contextTags = [], onResetContext }: { filter: ModFilter; categories?: string[]; contextTags?: string[]; onResetContext: () => void } = $props();
  let categoryQuery = $state("");
  const visibleCategories = $derived(categories.filter((category) => category.toLowerCase().includes(categoryQuery.toLowerCase())).slice(0, 30));
  function toggleCategory(category: string) { filter = { ...filter, categories: filter.categories.includes(category) ? filter.categories.filter((item) => item !== category) : [...filter.categories, category] }; }
</script>

<aside class="flex h-full min-h-0 w-56 shrink-0 flex-col border-r border-white/[0.08] bg-neutral-950/35 p-4 backdrop-blur-xl">
  <div class="mb-5 flex items-center justify-between"><h2 class="m-0 text-sm font-semibold text-neutral-100">Filters</h2><button type="button" class="text-xs text-[var(--text-secondary)] hover:text-neutral-200" onclick={() => filter = { ...filter, categories: [] }}><RotateCcw size={13}/><span class="sr-only">Reset filters</span></button></div>
  {#if contextTags.length}
    <section class="mb-5"><div class="mb-2 text-[12px] font-bold uppercase tracking-wider text-[var(--text-secondary)]">Instance context</div><div class="flex flex-wrap gap-2">{#each contextTags as tag (tag)}<span class="inline-flex items-center gap-2 rounded-md border border-emerald-500/25 bg-emerald-500/10 px-2 py-1 font-mono text-[12px] text-emerald-200">{tag}</span>{/each}</div><button type="button" class="mt-2 text-[12px] text-[var(--text-secondary)] hover:text-neutral-200" onclick={onResetContext}>Search globally</button></section>
  {/if}
  <label class="mb-3 flex items-center gap-2 rounded-lg border border-white/10 bg-black/25 px-2 py-1.5 text-[var(--text-secondary)]"><Search size={14}/><input class="min-w-0 border-0 bg-transparent p-0 text-xs text-neutral-200 outline-none" bind:value={categoryQuery} placeholder="Find category…" /></label>
  <div class="mb-2 text-[12px] font-bold uppercase tracking-wider text-[var(--text-secondary)]">Categories</div>
  <div class="min-h-0 flex-1 space-y-1 overflow-y-auto">{#each visibleCategories as category (category)}<label class="flex cursor-pointer items-center gap-2 rounded-lg px-2 py-2 text-xs text-[var(--text-secondary)] hover:bg-white/[0.05]"><input type="checkbox" checked={filter.categories.includes(category)} onchange={() => toggleCategory(category)} /><span class="flex-1 truncate">{category}</span>{#if filter.categories.includes(category)}<Check size={13} class="text-emerald-400"/>{/if}</label>{/each}</div>
  {#if filter.categories.length}<button type="button" class="mt-4 flex items-center justify-center gap-2 rounded-lg border border-white/10 px-3 py-2 text-xs text-[var(--text-secondary)] hover:bg-white/10" onclick={() => filter = { ...filter, categories: [] }}><X size={13}/> Clear selected</button>{/if}
</aside>
