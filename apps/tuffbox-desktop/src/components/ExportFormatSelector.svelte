<script lang="ts">
  import { PackageOpen, FileArchive, Box, Server, FolderTree, CheckCircle2, AlertTriangle } from "@lucide/svelte";

  export type ExportFormatOption = {
    id: string;
    title: string;
    badge: string;
    blurb: string;
    detail: string;
    pathKind: "file" | "dir";
    validation: string | null;
  };

  let {
    formats,
    selected,
    warningCounts = {},
    errorCounts = {},
    onSelect,
  }: {
    formats: ExportFormatOption[];
    selected: string;
    warningCounts?: Record<string, number>;
    errorCounts?: Record<string, number>;
    onSelect: (id: string) => void;
  } = $props();

  function Icon({ id }: { id: string }) {
    if (id === "mrpack") return PackageOpen;
    if (id === "curseforge") return FileArchive;
    if (id === "prism") return Box;
    if (id === "server") return Server;
    return FolderTree;
  }
</script>

<div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-5" role="listbox" aria-label="Export format">
  {#each formats as format (format.id)}
    {@const FormatIcon = Icon({ id: format.id })}
    {@const warnings = warningCounts[format.id] ?? 0}
    {@const errors = errorCounts[format.id] ?? 0}
    <button
      type="button"
      class="group min-h-[112px] rounded-xl border border-white/[0.08] bg-white/[0.025] p-4 text-left transition-all duration-200 hover:border-white/20 hover:bg-white/[0.04] {selected === format.id ? 'border-emerald-500/60 bg-emerald-500/10 shadow-[0_0_15px_rgba(16,185,129,0.15)]' : ''}"
      class:has-error={errors > 0}
      role="option"
      aria-selected={selected === format.id}
      onclick={() => onSelect(format.id)}
    >
      <div class="flex items-start justify-between gap-3">
        <span class="flex h-10 w-10 items-center justify-center rounded-xl border border-white/10 bg-white/[0.06] text-emerald-300 transition-colors group-hover:text-emerald-200">
          <FormatIcon size={22} />
        </span>
        {#if errors > 0}
          <span class="inline-flex items-center gap-2 rounded-full border border-red-400/30 bg-red-500/10 px-2 py-1 text-[12px] font-bold text-red-200"><AlertTriangle size={12} /> {errors} error{errors === 1 ? '' : 's'}</span>
        {:else if warnings > 0}
          <span class="inline-flex items-center gap-2 rounded-full border border-amber-400/30 bg-amber-500/10 px-2 py-1 text-[12px] font-bold text-amber-200">{warnings} warning{warnings === 1 ? '' : 's'}</span>
        {:else}
          <CheckCircle2 size={16} class="text-emerald-400" aria-label="Ready to export" />
        {/if}
      </div>
      <div class="mt-3 flex items-baseline gap-2">
        <strong class="truncate text-sm text-neutral-100">{format.title}</strong>
        <span class="shrink-0 font-mono text-xs text-[var(--text-secondary)]">{format.badge}</span>
      </div>
      <p class="mt-1 truncate text-xs text-[var(--text-secondary)]">{format.blurb}</p>
    </button>
  {/each}
</div>
