<script lang="ts">
  import {
    CheckCircle2,
    PanelLeft,
    PanelLeftClose,
    PanelRight,
    PanelRightClose,
    RefreshCw,
    Save,
    Search,
    Sparkles,
    AlertTriangle,
  } from "@lucide/svelte";

  let {
    title = "Quest editor",
    chapterCount = 0,
    questCount = 0,
    dirty = false,
    saving = false,
    hasErrors = false,
    mode = "Quests",
    railCollapsed = false,
    inspectorCollapsed = false,
    aiOpen = false,
    onModeChange,
    onSave,
    onAi,
    onRefresh,
    onSearch,
    onToggleRail,
    onToggleInspector,
  }: {
    title?: string;
    chapterCount?: number;
    questCount?: number;
    dirty?: boolean;
    saving?: boolean;
    hasErrors?: boolean;
    mode?: string;
    railCollapsed?: boolean;
    inspectorCollapsed?: boolean;
    aiOpen?: boolean;
    onModeChange: (mode: string) => void;
    onSave: () => void;
    onAi: () => void;
    onRefresh: () => void;
    onSearch: () => void;
    onToggleRail: () => void;
    onToggleInspector: () => void;
  } = $props();

  const modes = ["Quests", "Rewards", "Variables", "Logs"];
</script>

<header class="qt-header flex min-h-12 flex-wrap items-center justify-between gap-3 border-b border-[var(--border-color)] bg-[var(--bg-secondary)] px-4 py-2.5 flex-shrink-0">
  <div class="flex min-w-0 items-center gap-3">
    <div class="flex h-8 w-8 items-center justify-center rounded-lg bg-[var(--accent-primary)]/10 text-[var(--accent-primary)] border border-[var(--accent-primary)]/20 font-bold">
      ⚒
    </div>
    <div class="min-w-0">
      <div class="truncate text-sm font-semibold text-[var(--text-primary)]">{title}</div>
      <div class="text-[12px] font-medium text-[var(--text-muted)]">{chapterCount} chapters · {questCount} quests</div>
    </div>
  </div>

  <nav class="flex rounded-xl border border-[var(--border-color)] bg-[var(--bg-primary)] p-1" aria-label="Quest editor mode">
    {#each modes as item}
      <button
        type="button"
        class="rounded-lg px-3 py-1.5 text-xs font-semibold whitespace-nowrap transition-all {mode === item ? 'bg-[var(--accent-primary)] text-[var(--on-accent)] shadow-sm' : 'text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]'}"
        onclick={() => onModeChange(item)}
      >
        {item}
      </button>
    {/each}
  </nav>

  <div class="flex items-center gap-2">
    <button
      type="button"
      class="inline-flex items-center justify-center rounded-lg border border-[var(--border-color)] bg-[var(--bg-card)] p-2 text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] transition"
      onclick={onToggleRail}
      title={railCollapsed ? "Show chapters panel" : "Hide chapters panel"}
      aria-label={railCollapsed ? "Show chapters panel" : "Hide chapters panel"}
    >
      {#if railCollapsed}
        <PanelLeft size={16} />
      {:else}
        <PanelLeftClose size={16} />
      {/if}
    </button>

    <button
      type="button"
      class="inline-flex items-center justify-center rounded-lg border border-[var(--border-color)] bg-[var(--bg-card)] p-2 text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] transition"
      onclick={onToggleInspector}
      title={inspectorCollapsed ? "Show quest inspector" : "Hide quest inspector"}
      aria-label={inspectorCollapsed ? "Show quest inspector" : "Hide quest inspector"}
    >
      {#if inspectorCollapsed}
        <PanelRight size={16} />
      {:else}
        <PanelRightClose size={16} />
      {/if}
    </button>

    <span class="hidden items-center gap-2 rounded-full border px-2.5 py-1 text-[12px] font-semibold sm:inline-flex {hasErrors ? 'border-[var(--accent-danger)]/40 bg-[var(--accent-danger)]/10 text-[var(--accent-danger)]' : dirty ? 'border-[var(--accent-warning)]/40 bg-[var(--accent-warning)]/10 text-[var(--accent-warning)]' : 'border-[var(--accent-primary)]/40 bg-[var(--accent-primary)]/10 text-[var(--accent-primary)]'}">
      {#if hasErrors}
        <AlertTriangle size={12} /> Needs review
      {:else if dirty}
        <span class="h-1.5 w-1.5 rounded-full bg-amber-400"></span> Unsaved
      {:else}
        <CheckCircle2 size={12} /> Synced
      {/if}
    </span>

    <button
      type="button"
      class="inline-flex items-center justify-center rounded-lg border border-[var(--border-color)] bg-[var(--bg-card)] p-2 text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] transition"
      onclick={onSearch}
      title="Search fields (Ctrl+F)"
    >
      <Search size={16} />
      <span class="sr-only">Search</span>
    </button>

    <button
      type="button"
      class="inline-flex items-center gap-2 whitespace-nowrap flex-shrink-0 rounded-lg border px-3 py-1.5 text-xs font-semibold transition-all {aiOpen ? 'border-violet-400 bg-violet-600 text-white shadow-md shadow-violet-500/25' : 'border-[var(--border-color)] bg-[var(--bg-card)] text-[var(--accent-secondary)] hover:bg-[var(--bg-hover)]'}"
      onclick={onAi}
      title={aiOpen ? "Close Quest AI" : "Open Quest AI"}
    >
      <Sparkles size={14} />
      Quest AI
    </button>

    <button
      type="button"
      class="inline-flex items-center gap-2 whitespace-nowrap rounded-lg bg-[var(--accent-primary)] px-3.5 py-1.5 text-xs font-semibold text-[var(--on-accent)] shadow-md shadow-[var(--accent-primary)]/20 hover:brightness-110 active:scale-[0.98] transition disabled:opacity-50"
      onclick={onSave}
      disabled={saving}
    >
      <Save size={14} />
      {saving ? "Saving…" : "Save all"}
      <kbd class="hidden text-[12px] text-white/70 sm:inline ml-1 font-mono">Ctrl S</kbd>
    </button>

    <button
      type="button"
      class="inline-flex items-center justify-center rounded-lg border border-[var(--border-color)] bg-[var(--bg-card)] p-2 text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] transition disabled:opacity-50"
      onclick={onRefresh}
      disabled={saving}
      title="Reload quest data from disk"
    >
      <RefreshCw size={15} class={saving ? "animate-spin" : ""} />
    </button>
  </div>
</header>

