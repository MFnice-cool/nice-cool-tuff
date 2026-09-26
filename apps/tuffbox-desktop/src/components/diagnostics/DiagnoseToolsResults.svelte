<script lang="ts">
  import { Wrench } from "@lucide/svelte";
  import DiagnoseAuthorPanel from "./DiagnoseAuthorPanel.svelte";
  import DiagnoseHeuristicPlan from "./DiagnoseHeuristicPlan.svelte";

  let {
    plan,
    aiPrompt,
    aiShowPrompt = $bindable(),
    selectedOption = $bindable(),
    applying,
    onApplyPlan,
    authorOpen = $bindable(),
    authorId = $bindable(),
    authorSolution = $bindable(),
    authorSymptoms = $bindable(),
    authorSuspected = $bindable(),
    authorActionsJson = $bindable(),
    authorNotes = $bindable(),
    authorBusy,
    authorExportPreview,
    authorMessage,
    onSaveAuthor,
    onCopyAuthorExport,
    onOpenAuthorFolder,
  }: {
    plan: any | null;
    aiPrompt: string;
    aiShowPrompt: boolean;
    selectedOption: number | null;
    applying: boolean;
    onApplyPlan: () => void;
    authorOpen: boolean;
    authorId: string;
    authorSolution: string;
    authorSymptoms: string;
    authorSuspected: string;
    authorActionsJson: string;
    authorNotes: string;
    authorBusy: boolean;
    authorExportPreview: string | null;
    authorMessage: string | null;
    onSaveAuthor: () => void;
    onCopyAuthorExport: () => void;
    onOpenAuthorFolder: () => void;
  } = $props();

  const hasResults = $derived(
    !!plan || !!aiPrompt || authorOpen,
  );
</script>

{#if hasResults}
  <section class="tools-results">
    <h2><Wrench size={16} /> Tool results</h2>
    {#if aiShowPrompt && aiPrompt}
      <pre class="log-pre">{aiPrompt.slice(0, 20000)}</pre>
    {/if}
    <DiagnoseHeuristicPlan
      plan={plan}
      bind:selectedOption
      applying={applying}
      onApply={onApplyPlan}
    />
    <DiagnoseAuthorPanel
      bind:open={authorOpen}
      bind:authorId
      bind:solution={authorSolution}
      bind:symptoms={authorSymptoms}
      bind:suspected={authorSuspected}
      bind:actionsJson={authorActionsJson}
      bind:notes={authorNotes}
      busy={authorBusy}
      exportPreview={authorExportPreview}
      message={authorMessage}
      onSave={onSaveAuthor}
      onCopyExport={onCopyAuthorExport}
      onOpenFolder={onOpenAuthorFolder}
    />
  </section>
{/if}

<style>
  .tools-results { margin-bottom: 14px; padding: 14px; }
  .tools-results h2{ margin: 0 0 10px; display: flex; align-items: center; gap: 8px; }
  .log-pre { margin: 0; max-height: 320px; padding: 12px; border-radius: var(--border-radius-md); background: #09090b; color: #d4d4d8; font-family: var(--font-mono, ui-monospace, monospace); font-size: 12px; line-height: 1.55; white-space: pre-wrap; overflow: auto; }
</style>
