<script lang="ts">
  import { Tag, GitBranch, Boxes } from "@lucide/svelte";
  let { version = $bindable("1.0.0"), changedMods = 0, components = 0, onSave, onIncrement }: {
    version?: string;
    changedMods?: number;
    components?: number;
    onSave: () => void;
    onIncrement: (part: "patch" | "minor" | "major") => void;
  } = $props();
</script>

<section class="grid gap-4">
  <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
    <div class="rounded-xl border border-white/[0.08] bg-white/[0.04] p-3"><div class="flex items-center gap-2 text-emerald-300"><Tag size={16}/><span class="text-[12px] uppercase tracking-wide text-[var(--text-secondary)]">Version</span></div><strong class="mt-2 block text-lg text-neutral-100">{version || "—"}</strong><span class="text-xs text-[var(--text-secondary)]">SemVer tag</span></div>
    <div class="rounded-xl border border-white/[0.08] bg-white/[0.04] p-3"><div class="flex items-center gap-2 text-blue-300"><GitBranch size={16}/><span class="text-[12px] uppercase tracking-wide text-[var(--text-secondary)]">Changed mods</span></div><strong class="mt-2 block text-lg text-neutral-100">{changedMods}</strong><span class="text-xs text-[var(--text-secondary)]">since last release</span></div>
    <div class="rounded-xl border border-white/[0.08] bg-white/[0.04] p-3"><div class="flex items-center gap-2 text-violet-300"><Boxes size={16}/><span class="text-[12px] uppercase tracking-wide text-[var(--text-secondary)]">Components</span></div><strong class="mt-2 block text-lg text-neutral-100">{components}</strong><span class="text-xs text-[var(--text-secondary)]">in changelog</span></div>
  </div>
  <div class="flex flex-wrap gap-2 rounded-xl border border-white/[0.08] bg-black/20 p-2">
    <input class="min-w-[150px] flex-1 bg-black/30 font-mono text-sm text-neutral-200" bind:value={version} placeholder="Major.Minor.Patch" aria-label="Release version" />
    <button type="button" class="rounded-lg border border-white/10 px-2.5 py-2 text-xs text-[var(--text-secondary)] hover:bg-white/10" onclick={() => onIncrement("patch")}>+Patch</button>
    <button type="button" class="rounded-lg border border-white/10 px-2.5 py-2 text-xs text-[var(--text-secondary)] hover:bg-white/10" onclick={() => onIncrement("minor")}>+Minor</button>
    <button type="button" class="rounded-lg border border-white/10 px-2.5 py-2 text-xs text-[var(--text-secondary)] hover:bg-white/10" onclick={() => onIncrement("major")}>+Major</button>
    <button type="button" class="rounded-lg bg-emerald-600 px-3 py-2 text-xs font-semibold text-white hover:bg-emerald-500" onclick={onSave}>Save version</button>
  </div>
</section>
