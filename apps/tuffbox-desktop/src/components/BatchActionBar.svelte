<script lang="ts">
  import { Check, X } from "@lucide/svelte";
  let { count = 0, installing = false, onInstall, onClear }: { count?: number; installing?: boolean; onInstall: () => void; onClear: () => void } = $props();
</script>

{#if count > 0}<div class="fixed inset-x-4 bottom-5 z-50 mx-auto flex max-w-3xl animate-[slide-up_.2s_ease-out] flex-wrap items-center justify-between gap-3 rounded-2xl border border-emerald-500/25 bg-neutral-950/90 px-4 py-3 shadow-[0_0_30px_rgba(16,185,129,.18)] backdrop-blur-2xl"><span class="text-sm text-neutral-200"><strong>{count}</strong> selected</span><div class="flex gap-2"><button type="button" class="rounded-lg border border-white/10 px-3 py-2 text-xs text-[var(--text-secondary)] hover:bg-white/10" onclick={onClear}><X size={13}/> Clear selection</button><button type="button" class="inline-flex items-center gap-2 rounded-lg bg-emerald-600 px-4 py-2 text-xs font-semibold text-white hover:bg-emerald-500" onclick={onInstall} disabled={installing}><Check size={13}/> {installing ? 'Installing…' : 'Install selected + dependencies'}</button></div></div>{/if}
<style>@keyframes slide-up { from { opacity: 0; transform: translateY(12px); } to { opacity: 1; transform: translateY(0); } }</style>
