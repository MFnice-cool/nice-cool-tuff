<script lang="ts">
  import { onMount, tick } from "svelte";
  import { Search, ArrowRight, CornerDownLeft } from "@lucide/svelte";
  import { trapFocus } from "../lib/focusTrap";
  import { ideRecentCommands, recentProjects, projectPath } from "../lib/store";

  let {
    onclose,
    onnavigate,
  }: {
    onclose?: () => void;
    onnavigate?: (id: string) => void;
  } = $props();

  type Item = { id: string; label: string; category: string; shortcut?: string };

  let query = $state("");
  let inputEl = $state<HTMLInputElement | undefined>(undefined);
  let selectedIndex = $state(0);

  const viewItems: Item[] = [
    { id: "dashboard", label: "Home", category: "Views", shortcut: "Ctrl+1" },
    { id: "ide", label: "Open IDE", category: "Views", shortcut: "Ctrl+2" },
    { id: "mods", label: "IDE · Content", category: "Views", shortcut: "Ctrl+3" },
    { id: "graph", label: "IDE · Resolve", category: "Views", shortcut: "Ctrl+4" },
    { id: "configs", label: "IDE · Configs", category: "Views", shortcut: "Ctrl+5" },
    { id: "diagnostics", label: "IDE · Diagnose", category: "Views", shortcut: "Ctrl+6" },
    { id: "crash-votes", label: "Crash Votes", category: "Views" },
    { id: "snapshots", label: "IDE · Snapshots", category: "Views", shortcut: "Ctrl+7" },
    { id: "world", label: "IDE · World map", category: "Views", shortcut: "Ctrl+8" },
    { id: "library", label: "Library", category: "Views" },
    { id: "chats", label: "Chats", category: "Views" },
    { id: "me", label: "Me", category: "Views" },
    { id: "ore-gen", label: "Ore Heights", category: "Views" },
    { id: "recipes", label: "Recipe Browser", category: "Views" },
    { id: "quests", label: "Quest Editor", category: "Views" },
  ];

  const ideStageItems: Item[] = [
    { id: "ide:brief", label: "Brief", category: "IDE stages", shortcut: "Ctrl+0" },
    { id: "ide:setup", label: "Setup", category: "IDE stages" },
    { id: "ide:content", label: "Content (Mods)", category: "IDE stages", shortcut: "Ctrl+1" },
    { id: "ide:resolve", label: "Resolve (Graph)", category: "IDE stages", shortcut: "Ctrl+2" },
    { id: "ide:history", label: "History", category: "IDE stages", shortcut: "Ctrl+3" },
    { id: "ide:quests", label: "Quests", category: "IDE stages", shortcut: "Ctrl+8" },
    { id: "ide:recipes", label: "Recipes", category: "IDE stages" },
    { id: "ide:world-map", label: "World", category: "IDE stages" },
    { id: "ide:ore-gen", label: "Ores", category: "IDE stages" },
    { id: "ide:configs", label: "Tune", category: "IDE stages", shortcut: "Ctrl+7" },
    { id: "ide:test", label: "Test", category: "IDE stages", shortcut: "Ctrl+4" },
    { id: "ide:diagnose", label: "Diagnose / Health", category: "IDE stages", shortcut: "Ctrl+5" },
    { id: "ide:snapshots", label: "Snapshots", category: "IDE stages", shortcut: "Ctrl+6" },
    { id: "ide:export", label: "Export", category: "IDE stages", shortcut: "Ctrl+9" },
    { id: "ide:release", label: "Release", category: "IDE stages" },
  ];

  const actionItems: Item[] = [
    { id: "action:test-launch", label: "Test launch (Play)", category: "Actions", shortcut: "Ctrl+Shift+P" },
    { id: "action:next", label: "Next Action", category: "Actions", shortcut: "Ctrl+Enter" },
    { id: "action:refresh-graph", label: "Refresh pack graph", category: "Actions" },
    { id: "action:open-folder", label: "Open instance folder", category: "Actions" },
    { id: "action:optimize-pack", label: "Optimize pack (Content)", category: "Actions" },
    { id: "action:export-mrpack", label: "Export mrpack", category: "Actions" },
    { id: "settings", label: "Settings", category: "Actions" },
    { id: "settings-java", label: "Java settings (Launcher)", category: "Actions" },
    { id: "project-settings", label: "Project Settings", category: "Actions" },
    { id: "new-instance", label: "Create New Instance", category: "Actions" },
    { id: "shortcuts", label: "Keyboard Shortcuts", category: "Actions" },
  ];

  const instanceItems: Item[] = $derived(
    ($recentProjects ?? [])
      .filter((p) => p.path !== $projectPath)
      .slice(0, 8)
      .map((p) => ({
        id: `instance:${p.path}`,
        label: p.info?.name ?? p.path,
        category: "Instances",
      })),
  );

  const allItems = $derived.by(() => {
    const recent: Item[] = ($ideRecentCommands ?? []).map((r) => ({
      id: r.id,
      label: r.label,
      category: "Recent",
    }));
    return [...recent, ...instanceItems, ...viewItems, ...ideStageItems, ...actionItems];
  });

  function groupItems(items: Item[]) {
    const groups: Record<string, Item[]> = {};
    for (const item of items) {
      (groups[item.category] ??= []).push(item);
    }
    return groups;
  }

  const filtered = $derived(
    query.trim()
      ? allItems.filter(
          (item) =>
            item.label.toLowerCase().includes(query.toLowerCase()) ||
            item.category.toLowerCase().includes(query.toLowerCase()) ||
            item.id.toLowerCase().includes(query.toLowerCase()),
        )
      : allItems,
  );

  const grouped = $derived(groupItems(filtered));
  const flatList = $derived(filtered);

  $effect(() => {
    if (selectedIndex >= flatList.length) {
      selectedIndex = Math.max(0, flatList.length - 1);
    }
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      selectedIndex = Math.min(selectedIndex + 1, flatList.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selectedIndex = Math.max(selectedIndex - 1, 0);
    } else if (e.key === "Enter" && flatList[selectedIndex]) {
      e.preventDefault();
      selectItem(flatList[selectedIndex]);
    }
  }

  function selectItem(item: Item) {
    onnavigate?.(item.id);
    onclose?.();
  }

  function scrollToSelected() {
    const el = document.querySelector(`.cmd-item[data-index="${selectedIndex}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }

  $effect(() => {
    if (selectedIndex >= 0) {
      tick().then(scrollToSelected);
    }
  });

  onMount(() => {
    inputEl?.focus();
  });
</script>

<div class="cmd-backdrop tb-modal-backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose?.()}>
  <div class="cmd-dialog tb-anim-search-enter" role="dialog" aria-modal="true" aria-label="Command palette" use:trapFocus={{ onEscape: () => onclose?.() }}>
    <div class="cmd-input-wrap">
      <Search size={18} class="cmd-search-icon" />
      <input
        bind:this={inputEl}
        bind:value={query}
        class="cmd-input"
        type="text"
        placeholder="Search stages, views, actions..."
        spellcheck="false"
        onkeydown={handleKeydown}
      />
      <kbd class="cmd-esc">ESC</kbd>
    </div>

    <div class="cmd-results">
      {#each Object.entries(grouped) as [category, items] (category)}
        <div class="cmd-group">
          <div class="cmd-group-label">{category}</div>
          {#each items as item (item.id)}
            {@const globalIdx = flatList.indexOf(item)}
            <button
              class="cmd-item"
              class:selected={globalIdx === selectedIndex}
              data-index={globalIdx}
              onclick={() => selectItem(item)}
              onmouseenter={() => (selectedIndex = globalIdx)}
            >
              <span class="cmd-item-label">{item.label}</span>
              <span class="cmd-item-right">
                {#if item.shortcut}
                  <kbd>{item.shortcut}</kbd>
                {/if}
                <ArrowRight size={14} class="cmd-item-arrow" />
              </span>
            </button>
          {/each}
        </div>
      {:else}
        <div class="cmd-empty">No results for "{query}"</div>
      {/each}
    </div>

    <div class="cmd-footer">
      <span><kbd>↑↓</kbd> navigate</span>
      <span><CornerDownLeft size={12} /> select</span>
      <span><kbd>esc</kbd> close</span>
    </div>
  </div>
</div>

<style>
  .cmd-backdrop {
    position: fixed;
    inset: 0;
    z-index: 10000;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: min(18vh, 140px);
    background: color-mix(in srgb, var(--bg-primary) 55%, transparent);
    backdrop-filter: blur(4px);
  }
  .cmd-dialog {
    width: min(560px, calc(100vw - 32px));
    max-height: min(70vh, 520px);
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg, 12px);
    background: var(--bg-secondary);
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.35);
    overflow: hidden;
  }
  .cmd-input-wrap {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border-color);
  }
  .cmd-input-wrap :global(.cmd-search-icon) {
    flex-shrink: 0;
    color: var(--text-muted);
  }
  .cmd-input {
    flex: 1;
    border: none;
    background: transparent;
    color: var(--text-primary);
    font-size: 15px;
    outline: none;
  }
  .cmd-esc {
    font-size: 12px;
    padding: 2px 6px;
    border-radius: 4px;
    border: 1px solid var(--border-color);
    color: var(--text-muted);
  }
  .cmd-results {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
  }
  .cmd-group {
    margin-bottom: 8px;
  }
  .cmd-group-label {
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
    padding: 4px 8px;
  }
  .cmd-item {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 10px;
    border: none;
    border-radius: var(--border-radius-sm, 6px);
    background: transparent;
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
  }
  .cmd-item:hover,
  .cmd-item.selected {
    background: color-mix(in srgb, var(--accent-primary) 14%, transparent);
  }
  .cmd-item-label {
    font-size: 13px;
  }
  .cmd-item-right {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-muted);
  }
  .cmd-item-right kbd {
    font-size: 12px;
    padding: 1px 5px;
    border-radius: 4px;
    border: 1px solid var(--border-color);
  }
  .cmd-empty {
    padding: 24px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }
  .cmd-footer {
    display: flex;
    gap: 16px;
    padding: 8px 14px;
    border-top: 1px solid var(--border-color);
    font-size: 12px;
    color: var(--text-muted);
  }
  .cmd-footer kbd {
    font-size: 12px;
    padding: 1px 4px;
    border-radius: 3px;
    border: 1px solid var(--border-color);
  }
</style>
