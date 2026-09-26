<script lang="ts">
  import type { QuestChapter, QuestChapterGroup, QuestData, QuestValidationIssue, QuestBook } from "./lib/store";
  import {
    loadQuestBookFromSnbt,
    exportChapterSnbt,
    validateQuestBook,
    saveToStorage,
    loadFromStorage,
    clearStorage,
    summarizeQuestBookLoad,
    formatLoadMessage,
  } from "./lib/store";
  import { createHistoryState, pushSnapshot, undo as historyUndo, redo as historyRedo, canUndo, canRedo, clearHistory, materializeChapters } from "./lib/history";
  import { createSelectionState, selectSingle, toggleSelect, addToSelection, selectAll, clearSelection, type SelectionState } from "./lib/selection";
  import { createSearchState, searchQuests, nextResult, prevResult, type SearchState } from "./lib/search";
  import { loadTheme, saveTheme, toggleTheme, getThemeVars, type Theme } from "./lib/theme";
  import { layoutTree, layoutGrid, layoutCircle, applyLayout } from "./lib/layout";
  import ShortcutsModal from "./components/ui/ShortcutsModal.svelte";
  import ChapterRail from "./components/quests/ChapterRail.svelte";
  import QuestCanvas from "./components/quests/QuestCanvas.svelte";
  import QuestInspector from "./components/quests/QuestInspector.svelte";
  import BatchEditor from "./components/quests/BatchEditor.svelte";
  import ColorManager from "./components/quests/ColorManager.svelte";
  import RawSnbtView from "./components/quests/RawSnbtView.svelte";

  let chapters = $state<QuestChapter[]>([]);
  let chapterGroups = $state<QuestChapterGroup[]>([]);
  let bookTitle = $state<string | null>(null);
  let loadedBook = $state<QuestBook | null>(null);
  let selectedChapter = $state("");
  let selectedQuest = $state<QuestData | null>(null);
  let validationIssues = $state<QuestValidationIssue[]>([]);
  let dirtyChapters = $state(new Set<string>());
  let fitToken = $state(0);
  let error = $state<string | null>(null);
  let message = $state<string | null>(null);
  let snbtFiles = $state<Map<string, string>>(new Map());
  let showImportHelp = $state(false);

  // History state
  let history = $state(createHistoryState());
  let lastHistoryChapter = $state("");

  // Selection state
  let selection = $state<SelectionState>(createSelectionState());

  // Search state
  let search = $state<SearchState>(createSearchState());

  // Theme state
  let theme = $state<Theme>(loadTheme());
  let themeVars = $derived(getThemeVars(theme));

  // Clipboard for copy/paste
  let clipboard = $state<QuestData[]>([]);

  // Shortcuts modal
  let showShortcuts = $state(false);

  // Panel tab state (for when no quest is selected)
  let panelTab = $state<"info" | "batch" | "colors" | "raw">("info");

  // Load from localStorage on mount
  $effect(() => {
    const saved = loadFromStorage();
    if (saved && saved.chapters.length > 0) {
      loadedBook = saved;
      chapters = saved.chapters;
      chapterGroups = saved.chapterGroups ?? [];
      bookTitle = saved.title ?? null;
      if (chapters.length > 0) selectedChapter = chapters[0].id;
      validationIssues = validateQuestBook({ chapters, chapterGroups });
      // Push initial state to history
      history = pushSnapshot(history, chapters, chapterGroups, selectedChapter);
    }
  });

  // Auto-save to localStorage
  $effect(() => {
    if (chapters.length > 0 || bookTitle) {
      saveToStorage({
        chapters,
        chapterGroups,
        title: bookTitle,
        rewardTables: loadedBook?.rewardTables,
        locales: loadedBook?.locales,
        activeLocale: loadedBook?.activeLocale,
        bookSettings: loadedBook?.bookSettings,
        subtitle: loadedBook?.subtitle,
      });
    }
  });

  const chapterQuests = $derived(chapters.find((c) => c.id === selectedChapter)?.quests ?? []);
  const selectedChapterObj = $derived(chapters.find((c) => c.id === selectedChapter) ?? null);
  const totalQuests = $derived(chapters.reduce((n, c) => n + c.quests.length, 0));

  function markDirty(chapterId: string) {
    dirtyChapters = new Set([...dirtyChapters, chapterId]);
    chapters = [...chapters];
  }

  function pushHistory() {
    history = pushSnapshot(history, chapters, chapterGroups, selectedChapter);
    lastHistoryChapter = selectedChapter;
  }

  function selectChapter(id: string) {
    selectedChapter = id;
    selectedQuest = null;
    selection = clearSelection();
    fitToken += 1;
  }

  function createChapter() {
    pushHistory();
    const n: QuestChapter = {
      id: `chapter_${Date.now().toString(16)}`,
      title: `Chapter ${chapters.length + 1}`,
      titleFromSnbt: true,
      quests: [],
      extras: {},
      orderIndex: chapters.length,
      filename: `chapter_${chapters.length + 1}`,
    };
    chapters = [...chapters, n];
    selectChapter(n.id);
    markDirty(n.id);
  }

  function addQuestAt(x: number, y: number) {
    if (!selectedChapter) return;
    pushHistory();
    const ch = chapters.find((c) => c.id === selectedChapter);
    if (!ch) return;
    const newQ: QuestData = {
      id: crypto.randomUUID().replace(/-/g, "").slice(0, 16).toUpperCase(),
      title: "New Quest",
      titleFromSnbt: true,
      description: [],
      x,
      y,
      dependencies: [],
      tasks: [{ id: crypto.randomUUID().replace(/-/g, "").slice(0, 12).toUpperCase(), type: "checkmark" }],
      rewards: [],
      optional: false,
      size: 1,
      extras: {},
    };
    ch.quests = [...ch.quests, newQ];
    markDirty(selectedChapter);
    selectedQuest = newQ;
    selection = selectSingle(selection, newQ.id);
  }

  function removeQuest(q: QuestData) {
    pushHistory();
    const ch = chapters.find((c) => c.id === selectedChapter);
    if (!ch) return;
    ch.quests = ch.quests.filter((x) => x.id !== q.id);
    for (const other of ch.quests) {
      other.dependencies = other.dependencies.filter((d) => d !== q.id);
    }
    markDirty(selectedChapter);
    if (selectedQuest?.id === q.id) selectedQuest = null;
    selection = clearSelection();
  }

  function removeSelectedQuests() {
    if (selection.selectedIds.size === 0) return;
    pushHistory();
    const ch = chapters.find((c) => c.id === selectedChapter);
    if (!ch) return;
    ch.quests = ch.quests.filter((x) => !selection.selectedIds.has(x.id));
    for (const other of ch.quests) {
      other.dependencies = other.dependencies.filter((d) => !selection.selectedIds.has(d));
    }
    markDirty(selectedChapter);
    selectedQuest = null;
    selection = clearSelection();
  }

  function moveQuest(q: QuestData, x: number, y: number) {
    q.x = x;
    q.y = y;
    markDirty(selectedChapter);
    if (selectedQuest?.id === q.id) selectedQuest = q;
  }

  function moveSelectedQuests(dx: number, dy: number) {
    if (selection.selectedIds.size === 0) return;
    pushHistory();
    const ch = chapters.find((c) => c.id === selectedChapter);
    if (!ch) return;
    for (const q of ch.quests) {
      if (selection.selectedIds.has(q.id)) {
        q.x += dx;
        q.y += dy;
      }
    }
    markDirty(selectedChapter);
    // Update selectedQuest if it's in the selection
    if (selectedQuest && selection.selectedIds.has(selectedQuest.id)) {
      selectedQuest = { ...selectedQuest };
    }
  }

  function wouldCycle(questId: string, depId: string, list: QuestData[]): boolean {
    const visited = new Set<string>();
    function dfs(id: string): boolean {
      if (id === questId) return true;
      if (visited.has(id)) return false;
      visited.add(id);
      const q = list.find((x) => x.id === id);
      if (q) {
        for (const d of q.dependencies) {
          if (dfs(d)) return true;
        }
      }
      return false;
    }
    return dfs(depId);
  }

  function addDep(q: QuestData, depId: string) {
    if (!depId || q.dependencies.includes(depId) || depId === q.id) return;
    const list = chapters.find((c) => c.id === selectedChapter)?.quests ?? [];
    if (wouldCycle(q.id, depId, list)) {
      error = "Dependency would create a cycle";
      return;
    }
    pushHistory();
    error = null;
    q.dependencies = [...q.dependencies, depId];
    markDirty(selectedChapter);
    selectedQuest = q;
  }

  function removeDep(q: QuestData, depId: string) {
    pushHistory();
    q.dependencies = q.dependencies.filter((d) => d !== depId);
    markDirty(selectedChapter);
    selectedQuest = q;
  }

  function linkQuests(fromId: string, toDepId: string) {
    const ch = chapters.find((c) => c.id === selectedChapter);
    const q = ch?.quests.find((x) => x.id === fromId);
    if (q) addDep(q, toDepId);
  }

  function deleteChapter(id: string) {
    pushHistory();
    chapters = chapters.filter((c) => c.id !== id);
    dirtyChapters = new Set([...dirtyChapters].filter((x) => x !== id));
    if (selectedChapter === id) {
      selectedChapter = chapters[0]?.id ?? "";
      selectedQuest = null;
    }
  }

  function moveChapter(id: string, dir: -1 | 1) {
    const idx = chapters.findIndex((c) => c.id === id);
    if (idx < 0) return;
    const j = idx + dir;
    if (j < 0 || j >= chapters.length) return;
    pushHistory();
    const next = [...chapters];
    [next[idx], next[j]] = [next[j]!, next[idx]!];
    next.forEach((c, i) => {
      c.orderIndex = i;
      markDirty(c.id);
    });
    chapters = next;
  }

  function renameChapter(id: string, title: string) {
    pushHistory();
    const ch = chapters.find((c) => c.id === id);
    if (ch) {
      ch.title = title;
      ch.titleFromSnbt = true;
      markDirty(id);
    }
  }

  function exportAll() {
    const parts: string[] = [];
    for (const ch of chapters) {
      parts.push(exportChapterSnbt(ch));
    }
    const blob = new Blob([parts.join("\n\n")], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "quest_chapters.snbt";
    a.click();
    URL.revokeObjectURL(url);
    message = `Exported ${chapters.length} chapter(s)`;
  }

  function exportChapter(chapterId: string) {
    const ch = chapters.find((c) => c.id === chapterId);
    if (!ch) return;
    const snbt = exportChapterSnbt(ch);
    const blob = new Blob([snbt], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${ch.filename ?? ch.id}.snbt`;
    a.click();
    URL.revokeObjectURL(url);
    message = `Exported "${ch.title}"`;
  }

  function applyLoadedBook(book: QuestBook, fileMap: Map<string, string>) {
    if (book.chapters.length === 0) {
      error = "Could not parse any chapters from files";
      return false;
    }
    snbtFiles = fileMap;
    loadedBook = book;
    chapters = book.chapters;
    chapterGroups = book.chapterGroups;
    bookTitle = book.title ?? null;
    selectedChapter = chapters[0]?.id ?? "";
    selectedQuest = null;
    dirtyChapters = new Set();
    selection = clearSelection();
    validationIssues = validateQuestBook(book);
    message = formatLoadMessage(summarizeQuestBookLoad(book, fileMap.size));
    error = null;
    return true;
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    showImportHelp = false;
    const items = e.dataTransfer?.items;
    if (!items) return;

    const reads: Promise<Array<{ path: string; content: string }>>[] = [];
    for (const item of Array.from(items)) {
      if (item.kind === "file") {
        const entry = item.webkitGetAsEntry?.();
        if (entry) {
          reads.push(readEntryRecursive(entry, ""));
        }
      }
    }

    Promise.all(reads)
      .then((nested) => {
        const fileMap = new Map<string, string>();
        for (const r of nested.flat()) {
          if (r.path.endsWith(".snbt")) {
            fileMap.set(r.path.replace(/\\/g, "/"), r.content);
          }
        }
        if (fileMap.size === 0) {
          error = "No .snbt files found in drop";
          return;
        }
        pushHistory();
        applyLoadedBook(loadQuestBookFromSnbt(fileMap), fileMap);
      })
      .catch((err) => {
        error = `Import failed: ${err instanceof Error ? err.message : String(err)}`;
      });
  }

  function readEntryRecursive(
    entry: FileSystemEntry,
    basePath: string
  ): Promise<Array<{ path: string; content: string }>> {
    return new Promise((resolve, reject) => {
      if (entry.isFile) {
        (entry as FileSystemFileEntry).file((file) => {
          const reader = new FileReader();
          reader.onload = () =>
            resolve([{ path: basePath + file.name, content: reader.result as string }]);
          reader.onerror = () => reject(reader.error ?? new Error("Failed to read file"));
          reader.readAsText(file);
        }, reject);
      } else if (entry.isDirectory) {
        const dir = entry as FileSystemDirectoryEntry;
        const reader = dir.createReader();
        const entries: FileSystemEntry[] = [];
        function readBatch() {
          reader.readEntries((batch) => {
            if (batch.length === 0) {
              Promise.all(
                entries.map((e) => readEntryRecursive(e, basePath + dir.name + "/"))
              )
                .then((results) => resolve(results.flat()))
                .catch(reject);
            } else {
              entries.push(...batch);
              readBatch();
            }
          }, reject);
        }
        readBatch();
      } else {
        resolve([]);
      }
    });
  }

  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    showImportHelp = true;
  }

  function handleDragLeave() {
    showImportHelp = false;
  }

  function handleUndo() {
    const result = historyUndo(history, chapters, chapterGroups, selectedChapter);
    if (result.snapshot) {
      history = result.state;
      chapters = materializeChapters(result.snapshot) as typeof chapters;
      chapterGroups = JSON.parse(result.snapshot.chapterGroups);
      selectedChapter = result.snapshot.selectedChapter;
      selectedQuest = null;
      selection = clearSelection();
      validationIssues = validateQuestBook({ chapters, chapterGroups });
    }
  }

  function handleRedo() {
    const result = historyRedo(history, chapters, chapterGroups, selectedChapter);
    if (result.snapshot) {
      history = result.state;
      chapters = materializeChapters(result.snapshot) as typeof chapters;
      chapterGroups = JSON.parse(result.snapshot.chapterGroups);
      selectedChapter = result.snapshot.selectedChapter;
      selectedQuest = null;
      selection = clearSelection();
      validationIssues = validateQuestBook({ chapters, chapterGroups });
    }
  }

  function handleCopy() {
    if (selection.selectedIds.size === 0) return;
    const ch = chapters.find((c) => c.id === selectedChapter);
    if (!ch) return;
    clipboard = ch.quests.filter((q) => selection.selectedIds.has(q.id));
    message = `Copied ${clipboard.length} quest(s)`;
  }

  function handlePaste() {
    if (clipboard.length === 0) return;
    pushHistory();
    const ch = chapters.find((c) => c.id === selectedChapter);
    if (!ch) return;

    const newIds = new Map<string, string>();
    const newQuests: QuestData[] = [];

    for (const q of clipboard) {
      const newId = crypto.randomUUID().replace(/-/g, "").slice(0, 16).toUpperCase();
      newIds.set(q.id, newId);
      newQuests.push({
        ...q,
        id: newId,
        x: q.x + 1,
        y: q.y + 1,
        dependencies: q.dependencies.map((d) => newIds.get(d) ?? d),
      });
    }

    ch.quests = [...ch.quests, ...newQuests];
    markDirty(selectedChapter);
    selection = selectAll(newQuests.map((q) => q.id));
    message = `Pasted ${newQuests.length} quest(s)`;
  }

  function handleSelectAll() {
    const ids = chapterQuests.map((q) => q.id);
    selection = selectAll(ids);
  }

  function handleDelete() {
    removeSelectedQuests();
  }

  function handleKeyDown(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    const isInput = target?.closest?.("input,textarea,select");

    // Ctrl/Cmd shortcuts
    if (e.ctrlKey || e.metaKey) {
      switch (e.key.toLowerCase()) {
        case "z":
          e.preventDefault();
          if (e.shiftKey) {
            handleRedo();
          } else {
            handleUndo();
          }
          return;
        case "y":
          e.preventDefault();
          handleRedo();
          return;
        case "c":
          if (!isInput) {
            e.preventDefault();
            handleCopy();
          }
          return;
        case "v":
          if (!isInput) {
            e.preventDefault();
            handlePaste();
          }
          return;
        case "a":
          if (!isInput) {
            e.preventDefault();
            handleSelectAll();
          }
          return;
        case "s":
          e.preventDefault();
          exportAll();
          return;
        case "f":
          e.preventDefault();
          search = { ...search, isOpen: !search.isOpen };
          return;
        case "/":
          e.preventDefault();
          showShortcuts = !showShortcuts;
          return;
        case "0":
          e.preventDefault();
          fitToken += 1;
          return;
      }
    }

    // Escape
    if (e.key === "Escape") {
      if (search.isOpen) {
        search = { ...search, isOpen: false };
      } else {
        selection = clearSelection();
        selectedQuest = null;
      }
      return;
    }

    // Delete/Backspace
    if (e.key === "Delete" || e.key === "Backspace") {
      if (!isInput) {
        e.preventDefault();
        handleDelete();
      }
      return;
    }

    // Arrow keys for nudging
    if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(e.key)) {
      if (!isInput && selection.selectedIds.size > 0) {
        e.preventDefault();
        const step = e.shiftKey ? 5 : 1;
        switch (e.key) {
          case "ArrowUp": moveSelectedQuests(0, -step); break;
          case "ArrowDown": moveSelectedQuests(0, step); break;
          case "ArrowLeft": moveSelectedQuests(-step, 0); break;
          case "ArrowRight": moveSelectedQuests(step, 0); break;
        }
      }
    }
  }

  function handleSearchKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      if (e.shiftKey) {
        search = prevResult(search);
      } else {
        search = nextResult(search);
      }
      // Navigate to result
      const result = search.results[search.selectedIndex];
      if (result) {
        if (result.chapterId !== selectedChapter) {
          selectChapter(result.chapterId);
        }
        selectedQuest = result.quest;
        selection = selectSingle(selection, result.quest.id);
        fitToken += 1;
      }
    }
    if (e.key === "Escape") {
      search = { ...search, isOpen: false };
    }
  }

  function updateSearch(query: string) {
    search = { ...search, query, results: searchQuests(query, chapters), selectedIndex: 0 };
  }

  function applyAutoLayout(mode: "tree" | "grid" | "circle") {
    if (!selectedChapter) return;
    pushHistory();
    const ch = chapters.find((c) => c.id === selectedChapter);
    if (!ch || ch.quests.length === 0) return;

    let positions: Map<string, { x: number; y: number }>;
    switch (mode) {
      case "tree":
        positions = layoutTree(ch.quests);
        break;
      case "grid":
        positions = layoutGrid(ch.quests);
        break;
      case "circle":
        positions = layoutCircle(ch.quests);
        break;
    }

    ch.quests = applyLayout(ch.quests, positions);
    markDirty(selectedChapter);
    fitToken += 1;
    message = `Applied ${mode} layout to ${ch.quests.length} quests`;
  }

  function clearAll() {
    if (!confirm("Clear all chapters? This cannot be undone.")) return;
    pushHistory();
    chapters = [];
    chapterGroups = [];
    bookTitle = null;
    loadedBook = null;
    selectedChapter = "";
    selectedQuest = null;
    dirtyChapters = new Set();
    selection = clearSelection();
    clearStorage();
    history = clearHistory();
    message = "Cleared";
  }

  function handleQuestUpdate(chapterId: string, updatedQuest: QuestData) {
    pushHistory();
    const ch = chapters.find((c) => c.id === chapterId);
    if (!ch) return;
    ch.quests = ch.quests.map((q) => q.id === updatedQuest.id ? updatedQuest : q);
    markDirty(chapterId);
    if (selectedQuest?.id === updatedQuest.id) selectedQuest = updatedQuest;
  }

  function handleFileImport(e: Event) {
    const input = e.target as HTMLInputElement;
    const files = input.files;
    if (!files || files.length === 0) return;

    const fileMap = new Map<string, string>();
    let pending = files.length;

    for (const file of Array.from(files)) {
      const reader = new FileReader();
      reader.onload = () => {
        const rel =
          (file as File & { webkitRelativePath?: string }).webkitRelativePath ||
          file.name;
        fileMap.set(rel.replace(/\\/g, "/"), reader.result as string);
        pending--;
        if (pending === 0) {
          pushHistory();
          applyLoadedBook(loadQuestBookFromSnbt(fileMap), fileMap);
        }
      };
      reader.readAsText(file);
    }
    input.value = "";
  }
</script>

<svelte:window onkeydown={handleKeyDown} />

<div
  class="app"
  style={Object.entries(themeVars).map(([k, v]) => `${k}:${v}`).join(';')}
  ondrop={handleDrop}
  ondragover={handleDragOver}
  ondragleave={handleDragLeave}
  role="application"
>
  <header class="toolbar">
    <div class="toolbar-left">
      <h1 class="logo">Quest Editor</h1>
      {#if bookTitle}
        <span class="book-title">{bookTitle}</span>
      {/if}
    </div>
    <div class="toolbar-center">
      <span class="stat">{chapters.length} chapters · {totalQuests} quests</span>
      {#if validationIssues.length > 0}
        <span class="stat warn">{validationIssues.length} issues</span>
      {/if}
      {#if dirtyChapters.size > 0}
        <span class="dirty">{dirtyChapters.size} unsaved</span>
      {/if}
    </div>
    <div class="toolbar-right">
      <button type="button" class="btn ghost" onclick={handleUndo} disabled={!canUndo(history)} title="Undo (Ctrl+Z)">↩</button>
      <button type="button" class="btn ghost" onclick={handleRedo} disabled={!canRedo(history)} title="Redo (Ctrl+Shift+Z)">↪</button>
      <button type="button" class="btn ghost" onclick={createChapter}>+ Chapter</button>
      <div class="layout-group">
        <button type="button" class="btn ghost" onclick={() => applyAutoLayout("tree")} title="Tree layout">Tree</button>
        <button type="button" class="btn ghost" onclick={() => applyAutoLayout("grid")} title="Grid layout">Grid</button>
        <button type="button" class="btn ghost" onclick={() => applyAutoLayout("circle")} title="Circle layout">Circle</button>
      </div>
      <button type="button" class="btn ghost" onclick={exportAll} disabled={chapters.length === 0}>Export All</button>
      <button type="button" class="btn ghost" onclick={() => theme = toggleTheme(theme)} title="Toggle theme">{theme === 'dark' ? '☀' : '☾'}</button>
      <button type="button" class="btn danger" onclick={clearAll} disabled={chapters.length === 0}>Clear</button>
    </div>
  </header>

  {#if error}
    <button type="button" class="notice error" onclick={() => (error = null)}>{error}</button>
  {/if}
  {#if message}
    <button type="button" class="notice success" onclick={() => (message = null)}>{message}</button>
  {/if}

  {#if showImportHelp}
    <div class="drop-overlay">
      <div class="drop-hint">Drop .snbt files or folders here</div>
    </div>
  {/if}

  {#if search.isOpen}
    <div class="search-panel">
      <input
        type="text"
        class="search-input"
        placeholder="Search quests..."
        value={search.query}
        oninput={(e) => updateSearch((e.target as HTMLInputElement).value)}
        onkeydown={handleSearchKeyDown}
      />
      <span class="search-count">
        {#if search.results.length > 0}
          {search.selectedIndex + 1} / {search.results.length}
        {:else}
          {search.query ? 'No results' : 'Type to search'}
        {/if}
      </span>
      <button type="button" class="btn ghost" onclick={() => search = { ...search, isOpen: false }}>✕</button>
    </div>
  {/if}

  {#if chapters.length === 0}
    <div class="empty">
      <div class="empty-icon">Quests</div>
      <h2>Start a quest line</h2>
      <p>Drop an FTB Quests <code>quests</code> folder (or <code>.snbt</code> files) here, or create a new chapter.</p>
      <div class="empty-actions">
        <button type="button" class="btn primary" onclick={createChapter}>+ Create first chapter</button>
        <label class="btn ghost file-label">
          Import .snbt files
          <input type="file" accept=".snbt" multiple onchange={handleFileImport} hidden />
        </label>
        <label class="btn ghost file-label">
          Import quests folder
          <input type="file" multiple webkitdirectory onchange={handleFileImport} hidden />
        </label>
      </div>
    </div>
  {:else}
    <div class="workspace">
      <ChapterRail
        {chapters}
        {chapterGroups}
        {selectedChapter}
        dirtyIds={dirtyChapters}
        onSelect={selectChapter}
        onCreate={createChapter}
        onDirty={markDirty}
        onDelete={deleteChapter}
        onMove={moveChapter}
        onExport={exportChapter}
        onRename={renameChapter}
      />
      <div class="canvas-area">
        <QuestCanvas
          quests={chapterQuests}
          selectedId={selectedQuest?.id ?? null}
          selectedIds={selection.selectedIds}
          issues={validationIssues}
          {fitToken}
          emptyHint="Double-click to add a quest"
          onSelect={(q, e) => {
            if (!q) {
              selection = clearSelection();
              selectedQuest = null;
              return;
            }
            if (e?.shiftKey) {
              selection = toggleSelect(selection, q.id);
            } else if (e?.ctrlKey || e?.metaKey) {
              selection = addToSelection(selection, q.id);
            } else {
              selection = selectSingle(selection, q.id);
            }
            selectedQuest = q;
          }}
          onMove={moveQuest}
          onAddAt={addQuestAt}
          onLink={linkQuests}
          onSelectMultiple={(ids) => { selection = selectAll(ids); }}
        />
      </div>
      {#if selectedQuest}
        <QuestInspector
          quest={selectedQuest}
          {chapterQuests}
          issues={validationIssues}
          onDirty={() => markDirty(selectedChapter)}
          onRemove={() => { if (selectedQuest) removeQuest(selectedQuest); }}
          onAddDep={(id) => { if (selectedQuest) addDep(selectedQuest, id); }}
          onRemoveDep={(id) => { if (selectedQuest) removeDep(selectedQuest, id); }}
        />
      {:else if selectedChapterObj}
        <div class="side-panel">
          <div class="panel-tabs">
            <button type="button" class="tab" class:active={panelTab === "info"}
              onclick={() => panelTab = "info"}>Info</button>
            <button type="button" class="tab" class:active={panelTab === "batch"}
              onclick={() => panelTab = "batch"}>Batch</button>
            <button type="button" class="tab" class:active={panelTab === "colors"}
              onclick={() => panelTab = "colors"}>Colors</button>
            <button type="button" class="tab" class:active={panelTab === "raw"}
              onclick={() => panelTab = "raw"}>Raw</button>
          </div>
          <div class="panel-content">
            {#if panelTab === "info"}
              <div class="chapter-info">
                <h3>{selectedChapterObj.title}</h3>
                <p>{selectedChapterObj.quests.length} quests</p>
                {#if selection.selectedIds.size > 0}
                  <p class="hint">{selection.selectedIds.size} selected</p>
                {/if}
                <p class="hint">Double-click canvas to add quest</p>
                <div class="shortcuts">
                  <p><kbd>Ctrl+Z</kbd> Undo</p>
                  <p><kbd>Ctrl+Shift+Z</kbd> Redo</p>
                  <p><kbd>Ctrl+C</kbd> Copy</p>
                  <p><kbd>Ctrl+V</kbd> Paste</p>
                  <p><kbd>Ctrl+A</kbd> Select all</p>
                  <p><kbd>Ctrl+F</kbd> Search</p>
                  <p><kbd>Ctrl+/</kbd> All shortcuts</p>
                  <p><kbd>Del</kbd> Delete selected</p>
                  <p><kbd>Arrows</kbd> Nudge</p>
                </div>
              </div>
            {:else if panelTab === "batch"}
              <BatchEditor
                {chapters}
                onQuestUpdate={handleQuestUpdate}
                onExportChapter={exportChapter}
              />
            {:else if panelTab === "colors"}
              <ColorManager
                {chapters}
                onQuestUpdate={handleQuestUpdate}
              />
            {:else if panelTab === "raw"}
              <RawSnbtView chapter={selectedChapterObj} />
            {/if}
          </div>
        </div>
      {/if}
    </div>
  {/if}

  <ShortcutsModal isOpen={showShortcuts} onClose={() => showShortcuts = false} />
</div>

<style>
  :global(*) { box-sizing: border-box; margin: 0; padding: 0; }
  :global(body) {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: var(--bg-primary);
    color: var(--text-primary);
    overflow: hidden;
    height: 100vh;
  }
  :global(button) { cursor: pointer; border-radius: 2px; font-weight: 600; }
  :global(input), :global(select), :global(textarea) {
    border-radius: 2px;
    border: 1px solid var(--border);
    background: var(--bg-primary);
    color: var(--text-primary);
    padding: 6px 8px;
    font-size: 12px;
  }
  :global(input:focus), :global(select:focus), :global(textarea:focus) {
    outline: none;
    border-color: var(--accent);
  }

  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--bg-primary);
    position: relative;
  }
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 16px;
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    gap: 12px;
    z-index: 10;
  }
  .toolbar-left { display: flex; align-items: center; gap: 12px; }
  .toolbar-center { display: flex; align-items: center; gap: 16px; }
  .toolbar-right { display: flex; align-items: center; gap: 8px; }
  .logo {
    font-size: 14px;
    font-weight: 800;
    color: var(--accent);
    letter-spacing: 0.02em;
  }
  .book-title {
    font-size: 12px;
    color: var(--text-muted);
  }
  .stat { font-size: 11px; color: var(--text-muted); }
  .stat.warn { color: var(--warning); }
  .dirty {
    font-size: 10px;
    color: var(--warning);
    padding: 2px 8px;
    background: rgba(251, 191, 36, 0.12);
    border: 1px solid rgba(251, 191, 36, 0.25);
    border-radius: 2px;
  }
  .btn {
    padding: 6px 12px;
    border: 1px solid var(--border);
    background: rgba(0,0,0,0.25);
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 600;
    border-radius: 2px;
  }
  .btn:hover:not(:disabled) { border-color: var(--accent); background: rgba(61,184,168,0.12); }
  .btn:disabled { opacity: 0.4; cursor: default; }
  .btn.primary { border-color: var(--success); background: rgba(85,201,90,0.18); color: var(--success); }
  .btn.primary:hover { background: rgba(85,201,90,0.28); }
  .btn.danger { color: var(--danger); }
  .btn.danger:hover { border-color: var(--danger); background: rgba(248,113,113,0.1); }
  .file-label { display: inline-flex; align-items: center; cursor: pointer; }
  .layout-group {
    display: flex;
    gap: 2px;
    padding: 0 4px;
    border-left: 1px solid var(--border);
    border-right: 1px solid var(--border);
  }

  .notice {
    display: block;
    width: 100%;
    padding: 8px 16px;
    font-size: 12px;
    cursor: pointer;
    flex-shrink: 0;
    border: none;
    text-align: left;
    font-family: inherit;
  }
  .notice.error { background: rgba(239,68,68,0.1); color: #fecaca; border-bottom: 1px solid rgba(239,68,68,0.3); }
  .notice.success { background: rgba(85,201,90,0.1); color: var(--success); border-bottom: 1px solid rgba(85,201,90,0.3); }

  .drop-overlay {
    position: absolute;
    inset: 0;
    background: rgba(61,184,168,0.15);
    border: 3px dashed var(--accent);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
    pointer-events: none;
  }
  .drop-hint {
    font-size: 18px;
    font-weight: 700;
    color: var(--accent);
    padding: 24px 48px;
    background: rgba(26,26,30,0.9);
    border-radius: 8px;
  }

  .search-panel {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .search-input {
    flex: 1;
    max-width: 300px;
    padding: 6px 12px;
  }
  .search-count {
    font-size: 11px;
    color: var(--text-muted);
    min-width: 60px;
  }

  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--text-muted);
  }
  .empty-icon { font-size: 48px; color: var(--accent); }
  .empty h2 { font-size: 20px; color: var(--accent); }
  .empty p { font-size: 13px; max-width: 400px; text-align: center; line-height: 1.5; }
  .empty code { background: rgba(255,255,255,0.08); padding: 2px 6px; border-radius: 3px; }
  .empty-actions { display: flex; gap: 12px; margin-top: 8px; }

  .workspace {
    flex: 1;
    display: flex;
    min-height: 0;
    overflow: hidden;
  }
  .canvas-area {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .side-panel {
    width: 320px;
    background: var(--bg-secondary);
    border-left: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .panel-tabs {
    display: flex;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .tab {
    flex: 1;
    padding: 8px 4px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    cursor: pointer;
    transition: color 0.15s, border-color 0.15s;
  }
  .tab:hover { color: var(--text-primary); }
  .tab.active {
    color: var(--accent);
    border-bottom-color: var(--accent);
  }
  .panel-content {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .chapter-info {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .chapter-info h3 { font-size: 14px; color: var(--accent); }
  .chapter-info p { font-size: 12px; color: var(--text-muted); }
  .hint { font-size: 11px; color: var(--text-muted); }

  .shortcuts {
    margin-top: 16px;
    padding-top: 16px;
    border-top: 1px solid var(--border);
  }
  .shortcuts p {
    font-size: 11px;
    color: var(--text-muted);
    margin-bottom: 4px;
  }
  .shortcuts kbd {
    display: inline-block;
    padding: 1px 5px;
    font-size: 10px;
    font-family: monospace;
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: 3px;
    margin-right: 4px;
  }
</style>
