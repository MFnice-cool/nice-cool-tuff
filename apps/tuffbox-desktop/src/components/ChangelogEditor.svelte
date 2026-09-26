<script lang="ts">
  import { Sparkles, RefreshCw, Eye, Pencil, ChevronDown } from "@lucide/svelte";
  let { value = $bindable(""), onRegenerate, onAi, disabled = false }: { value?: string; onRegenerate: () => void; onAi: () => void; disabled?: boolean } = $props();
  let mode = $state<"edit" | "preview">("edit");
  let showUnchanged = $state(false);
  const lines = $derived(value.split("\n").map((line) => line.trim()).filter(Boolean));
  const added = $derived(lines.filter((line) => /^\+|^[-*]\s*(new|added)/i.test(line)));
  const removed = $derived(lines.filter((line) => /^-|removed|deleted/i.test(line) && !/^\+/.test(line)));
  const updated = $derived(lines.filter((line) => !added.includes(line) && !removed.includes(line) && line.length > 0));
  const unchanged = $derived(Math.max(0, updated.length - 8));
</script>

<section class="flex min-h-[520px] flex-col overflow-hidden rounded-2xl border border-white/[0.08] bg-neutral-950/40 backdrop-blur-2xl shadow-2xl">
  <header class="flex flex-wrap items-center justify-between gap-3 border-b border-white/[0.08] p-4">
    <div><h2 class="m-0 text-lg font-semibold text-neutral-100">Changelog</h2><p class="mt-1 text-xs text-[var(--text-secondary)]">Keep a concise release story instead of a raw dependency dump.</p></div>
    <div class="flex flex-wrap items-center gap-2">
      <div class="flex rounded-lg border border-white/10 bg-black/20 p-1"><button type="button" class="rounded px-2.5 py-1.5 text-xs {mode === 'edit' ? 'bg-white/10 text-white' : 'text-[var(--text-secondary)]'}" onclick={() => mode = 'edit'}><Pencil size={13}/> Edit</button><button type="button" class="rounded px-2.5 py-1.5 text-xs {mode === 'preview' ? 'bg-white/10 text-white' : 'text-[var(--text-secondary)]'}" onclick={() => mode = 'preview'}><Eye size={13}/> Preview</button></div>
      <button type="button" class="inline-flex items-center gap-2 rounded-lg border border-violet-400/25 bg-violet-500/10 px-3 py-2 text-xs text-violet-200 hover:bg-violet-500/20" onclick={onAi} disabled={disabled}><Sparkles size={14}/> Generate with AI / Ollama</button>
      <button type="button" class="rounded-lg border border-white/10 p-2 text-[var(--text-secondary)] hover:bg-white/10" onclick={onRegenerate} disabled={disabled} title="Regenerate"><RefreshCw size={15}/></button>
    </div>
  </header>
  {#if mode === 'edit'}
    <textarea class="min-h-[430px] flex-1 resize-none border-0 bg-transparent p-5 font-mono text-xs leading-6 text-neutral-200 outline-none focus:ring-0" bind:value spellcheck="false" aria-label="Release changelog"></textarea>
  {:else}
    <div class="flex-1 overflow-y-auto p-5 font-mono text-xs leading-6 text-[var(--text-secondary)]">
      {#if added.length}<h2 class="mb-2 text-base font-bold text-emerald-300">## Новые моды</h2>{#each added as item}<div class="rounded px-2 py-1 text-emerald-200">+ {item.replace(/^[-+*]\s*/, '')}</div>{/each}{/if}
      {#if updated.length}<h2 class="mb-2 mt-5 text-base font-bold text-amber-200">## Обновленные моды</h2>{#each updated.slice(0, showUnchanged ? updated.length : 8) as item}<div class="rounded px-2 py-1 text-[var(--text-secondary)]">~ {item.replace(/^[-+*]\s*/, '')}</div>{/each}{/if}
      {#if removed.length}<h2 class="mb-2 mt-5 text-base font-bold text-red-300">## Удаленные моды</h2>{#each removed as item}<div class="rounded px-2 py-1 text-red-200">- {item.replace(/^[-+*]\s*/, '')}</div>{/each}{/if}
      {#if unchanged > 0}<button type="button" class="mt-4 flex items-center gap-2 text-[var(--text-secondary)] hover:text-neutral-200" onclick={() => showUnchanged = !showUnchanged}><ChevronDown size={14}/> {showUnchanged ? 'Свернуть' : `Еще ${unchanged} зависимостей без изменений...`}</button>{/if}
    </div>
  {/if}
</section>
