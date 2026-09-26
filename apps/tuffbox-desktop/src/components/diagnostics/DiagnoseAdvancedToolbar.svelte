<script lang="ts">
  import {
    Bot,
    BookMarked,
    Bug,
    Copy,
    Download,
    FileText,
    FolderOpen,
    Share2,
  } from "@lucide/svelte";

  let {
    projectPath,
    aiLoading,
    sessionOk,
    sharingLog,
    currentLogText,
    supportBusy,
    planning,
    oreLoading,
    duplicateLoading,
    unifyLoading,
    wrongLoaderLoading,
    duplicateJarLoading,
    authorBusy,
    aiPrompt,
    aiShowPrompt = $bindable(),
    runAiExplain,
    shareCurrentLog,
    exportSupportPack,
    copyCurrentLog,
    openFolder,
    openSubdir,
    createFixPlan,
    scanOreGen,
    scanDuplicateItems,
    generateUnify,
    detectWrongLoaderMods,
    detectDuplicateModJars,
    openAuthorForm,
    openAiSettings,
  }: {
    projectPath: string | null;
    aiLoading: boolean;
    sessionOk: boolean;
    sharingLog: boolean;
    currentLogText: string;
    supportBusy: boolean;
    planning: boolean;
    oreLoading: boolean;
    duplicateLoading: boolean;
    unifyLoading: boolean;
    wrongLoaderLoading: boolean;
    duplicateJarLoading: boolean;
    authorBusy: boolean;
    aiPrompt: string;
    aiShowPrompt: boolean;
    runAiExplain: () => void;
    shareCurrentLog: () => void;
    exportSupportPack: () => void;
    copyCurrentLog: () => void;
    openFolder: () => void;
    openSubdir: (name: string) => void;
    createFixPlan: () => void;
    scanOreGen: () => void;
    scanDuplicateItems: () => void;
    generateUnify: () => void;
    detectWrongLoaderMods: () => void;
    detectDuplicateModJars: () => void;
    openAuthorForm: () => void;
    openAiSettings: () => void;
  } = $props();
</script>

<div class="tools-group">
  <span class="tools-label">Triage</span>
  <button class="ghost" onclick={runAiExplain} disabled={!projectPath || aiLoading || sessionOk}>
    <Bot size={15} /> AI explain
  </button>
  <button class="ghost" onclick={shareCurrentLog} disabled={!projectPath || sharingLog || !currentLogText}>
    <Share2 size={15} /> {sharingLog ? "Sharing…" : "Share mclo.gs"}
  </button>
  <button class="ghost" onclick={exportSupportPack} disabled={!projectPath || supportBusy}>
    <Download size={15} /> {supportBusy ? "…" : "Support pack"}
  </button>
  <button class="ghost" onclick={copyCurrentLog} disabled={!currentLogText}>
    <Copy size={15} /> Copy log
  </button>
</div>
<div class="tools-group">
  <span class="tools-label">Folders</span>
  <button class="ghost" onclick={openFolder} disabled={!projectPath}><FolderOpen size={15} /> Instance</button>
  <button class="ghost" onclick={() => openSubdir("logs")} disabled={!projectPath}><FileText size={15} /> logs/</button>
  <button class="ghost" onclick={() => openSubdir("crash-reports")} disabled={!projectPath}><Bug size={15} /> crashes/</button>
</div>
<div class="tools-group">
  <span class="tools-label">Scanners</span>
  <button class="ghost" onclick={createFixPlan} disabled={!projectPath || planning}>{planning ? "…" : "Fix plan"}</button>
  <button class="ghost" onclick={scanOreGen} disabled={!projectPath || oreLoading}>{oreLoading ? "…" : "Ore gen"}</button>
  <button class="ghost" onclick={scanDuplicateItems} disabled={!projectPath || duplicateLoading}>{duplicateLoading ? "…" : "Duplicates"}</button>
  <button class="ghost" onclick={generateUnify} disabled={!projectPath || unifyLoading}>{unifyLoading ? "…" : "Unify"}</button>
  <button class="ghost" onclick={detectWrongLoaderMods} disabled={!projectPath || wrongLoaderLoading}>Wrong jars</button>
  <button class="ghost" onclick={detectDuplicateModJars} disabled={!projectPath || duplicateJarLoading}>
    {duplicateJarLoading ? "Dupes…" : "Dup jars"}
  </button>
  <button class="ghost" onclick={openAuthorForm} disabled={!projectPath || authorBusy}>
    <BookMarked size={15} /> Save KB
  </button>
  <button class="ghost" onclick={openAiSettings}><Bot size={15} /> AI settings</button>
  {#if aiPrompt}
    <button class="ghost" onclick={() => (aiShowPrompt = !aiShowPrompt)}>{aiShowPrompt ? "Hide" : "Show"} AI prompt</button>
  {/if}
</div>
