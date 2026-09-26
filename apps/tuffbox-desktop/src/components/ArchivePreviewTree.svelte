<script lang="ts">
  import { ChevronDown, ChevronRight, FileArchive, FileCode2, Folder, Image, ShieldCheck } from "@lucide/svelte";

  let { modFiles = [], configCount = 0, outputName = "build.mrpack", entryCount = 0 }: {
    modFiles?: string[];
    configCount?: number;
    outputName?: string;
    entryCount?: number;
  } = $props();

  let open = $state(new Set(["mods", "root"]));
  function toggle(key: string) { const next = new Set(open); next.has(key) ? next.delete(key) : next.add(key); open = next; }
</script>

<section class="grid gap-3 rounded-xl border border-white/[0.08] bg-white/[0.025] p-4" aria-labelledby="archive-preview-heading">
  <div class="flex items-start justify-between gap-3">
    <div><div class="flex items-center gap-2"><FileArchive size={17} class="text-emerald-300" /><h2 id="archive-preview-heading" class="m-0 text-sm font-semibold text-neutral-100">Archive manifest preview</h2></div><p class="mt-1 text-xs text-[var(--text-secondary)]">Everything that will be included in the export.</p></div>
    <span class="rounded-full border border-emerald-500/25 bg-emerald-500/10 px-2 py-1 text-[12px] text-emerald-200">{entryCount || modFiles.length + configCount + 2} entries</span>
  </div>
  <div class="rounded-lg border border-white/[0.07] bg-black/20 p-2 font-mono text-xs">
    <button type="button" class="flex w-full items-center gap-2 rounded px-2 py-2 text-left text-neutral-200 hover:bg-white/[0.05]" onclick={() => toggle('root')}>{#if open.has('root')}<ChevronDown size={14}/>{:else}<ChevronRight size={14}/>{/if}<FileArchive size={14} class="text-emerald-300" /> {outputName}</button>
    {#if open.has('root')}
      <div class="ml-5 border-l border-white/10 pl-3">
        <button type="button" class="flex w-full items-center gap-2 rounded px-2 py-2 text-left text-[var(--text-secondary)] hover:bg-white/[0.05]" onclick={() => toggle('mods')}>{#if open.has('mods')}<ChevronDown size={14}/>{:else}<ChevronRight size={14}/>{/if}<Folder size={14} class="text-amber-300" /> mods/ <span class="ml-auto text-[12px] text-[var(--text-secondary)]">{modFiles.length} jars</span></button>
        {#if open.has('mods')}
          <div class="ml-5 border-l border-white/10 pl-3">{#each modFiles.slice(0, 8) as mod (mod)}<div class="flex items-center gap-2 px-2 py-1.5 text-[var(--text-secondary)]"><FileCode2 size={13} class="text-blue-300" /><span class="truncate">{mod}</span></div>{/each}{#if modFiles.length > 8}<div class="px-2 py-1 text-[12px] text-[var(--text-secondary)]">+ {modFiles.length - 8} more jars</div>{/if}</div>
        {/if}
        <div class="flex items-center gap-2 px-2 py-2 text-[var(--text-secondary)]"><Folder size={14} class="text-amber-300" /> config/ <span class="ml-auto text-[12px] text-[var(--text-secondary)]">{configCount || 'included'}</span></div>
        <div class="flex items-center gap-2 px-2 py-2 text-[var(--text-secondary)]"><FileCode2 size={14} class="text-violet-300" /> modrinth.index.json</div>
        <div class="flex items-center gap-2 px-2 py-2 text-[var(--text-secondary)]"><Image size={14} class="text-pink-300" /> pack.png <ShieldCheck size={13} class="ml-auto text-emerald-400" aria-label="No private files included" /></div>
      </div>
    {/if}
  </div>
  <p class="m-0 flex items-center gap-2 text-[12px] text-[var(--text-secondary)]"><ShieldCheck size={13} class="text-emerald-400" /> Logs, backups, and private tokens are excluded from the manifest.</p>
</section>
