<script lang="ts">
  import { AlertTriangle, ChevronDown, ChevronRight, Link2, Layers } from "@lucide/svelte";

  export type PreflightGroup = {
    code: string;
    severity: "error" | "warning";
    message: string;
    count: number;
    targets: string[];
  };

  let {
    errors = [],
    warnings = [],
    expanded = new Set<string>(),
    onToggle,
    onExpandAll,
    onCollapseAll,
    onAction,
  }: {
    errors?: PreflightGroup[];
    warnings?: PreflightGroup[];
    expanded?: Set<string>;
    onToggle: (code: string) => void;
    onExpandAll: () => void;
    onCollapseAll: () => void;
    onAction?: (action: string, target?: string) => void;
  } = $props();

  let handled = $state(new Set<string>());
  const humanCode = (code: string, message: string) => {
    if (code === "MOD_WITHOUT_DOWNLOAD_URL") return "Мод будет упакован локально (нет ссылки на Modrinth)";
    if (code === "UNKNOWN_MOD_SIDE") return "Не удалось определить сторону загрузки мода";
    return message || code.replaceAll("_", " ").toLowerCase();
  };
  const actionFor = (code: string) => code === "MOD_WITHOUT_DOWNLOAD_URL" ? "Указать ссылку вручную" : code === "UNKNOWN_MOD_SIDE" ? "Оставить в overrides" : "Исключить мод";
  function act(group: PreflightGroup) {
    const next = new Set(handled);
    next.add(group.code);
    handled = next;
    onAction?.(actionFor(group.code), group.targets[0]);
  }
</script>

{#if errors.length || warnings.length}
  <section class="grid gap-3" aria-labelledby="preflight-heading">
    <div class="flex flex-wrap items-center gap-2">
      <div class="flex items-center gap-2"><AlertTriangle size={17} class="text-amber-300" /><h2 id="preflight-heading" class="m-0 text-sm font-semibold text-neutral-100">Pre-export checks</h2></div>
      <span class="rounded-full border border-amber-500/30 bg-amber-500/10 px-2 py-1 text-xs text-amber-200">{warnings.reduce((n, g) => n + g.count, 0)} warnings</span>
      {#if errors.length}<span class="rounded-full border border-red-500/30 bg-red-500/10 px-2 py-1 text-xs text-red-200">{errors.reduce((n, g) => n + g.count, 0)} errors</span>{/if}
      <div class="ml-auto flex gap-2 text-xs text-[var(--text-secondary)]"><button type="button" class="hover:text-neutral-100" onclick={onExpandAll}>Expand all</button><span>·</span><button type="button" class="hover:text-neutral-100" onclick={onCollapseAll}>Collapse all</button></div>
    </div>

    {#each errors as group (group.code)}
      <article class="rounded-xl border border-red-500/30 bg-red-500/10 p-3 text-red-100">
        <div class="flex gap-3"><AlertTriangle size={18} class="mt-0.5 shrink-0" /><div><strong class="text-sm">{humanCode(group.code, group.message)}</strong><p class="mt-1 text-xs text-red-200/80">Export must be fixed before continuing.</p></div></div>
      </article>
    {/each}

    {#each warnings as group (group.code)}
      <article class="rounded-xl border border-amber-500/30 bg-amber-500/10 p-3 text-amber-100 transition-opacity {handled.has(group.code) ? 'opacity-60' : ''}">
        <button type="button" class="flex w-full items-start gap-3 text-left" onclick={() => onToggle(group.code)} aria-expanded={expanded.has(group.code)}>
          {#if expanded.has(group.code)}<ChevronDown size={17} class="mt-0.5 shrink-0" />{:else}<ChevronRight size={17} class="mt-0.5 shrink-0" />{/if}
          <span class="min-w-0 flex-1"><strong class="block text-sm">{humanCode(group.code, group.message)}</strong><span class="mt-1 block text-xs text-amber-200/75">{group.count > 1 ? `${group.count} affected entries` : group.targets[0] ?? 'Review this item before export.'}</span></span>
          <span class="shrink-0 rounded-full border border-amber-400/30 px-2 py-0.5 text-[12px] uppercase tracking-wide">{group.count}</span>
        </button>
        {#if expanded.has(group.code)}
          <div class="mt-3 border-t border-amber-400/20 pt-3">
            <div class="flex flex-wrap gap-2">{#each group.targets as target (target)}<code class="rounded bg-black/20 px-2 py-1 font-mono text-[12px] text-amber-100/90">{target}</code>{/each}</div>
            <div class="mt-3 flex flex-wrap gap-2">
              <button type="button" class="inline-flex items-center gap-2 rounded-lg border border-amber-300/30 bg-amber-300/10 px-2.5 py-1.5 text-xs font-semibold hover:bg-amber-300/20" onclick={() => act(group)}><Link2 size={13} /> {actionFor(group.code)}</button>
              <button type="button" class="inline-flex items-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-2.5 py-1.5 text-xs text-neutral-200 hover:bg-white/10" onclick={() => onAction?.('Оставить в overrides', group.targets[0])}><Layers size={13} /> Оставить в overrides</button>
            </div>
          </div>
        {/if}
      </article>
    {/each}
  </section>
{/if}
