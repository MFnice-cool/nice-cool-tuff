<script lang="ts">
  let {
    open = $bindable(),
    authorId = $bindable(),
    solution = $bindable(),
    symptoms = $bindable(),
    suspected = $bindable(),
    actionsJson = $bindable(),
    notes = $bindable(),
    busy,
    exportPreview,
    message,
    onSave,
    onCopyExport,
    onOpenFolder,
  }: {
    open: boolean;
    authorId: string;
    solution: string;
    symptoms: string;
    suspected: string;
    actionsJson: string;
    notes: string;
    busy: boolean;
    exportPreview: string | null;
    message: string | null;
    onSave: () => void;
    onCopyExport: () => void;
    onOpenFolder: () => void;
  } = $props();
</script>

{#if open}
  <div class="author-form">
    <h3>Save KB case</h3>
    <label>Case id<input bind:value={authorId} placeholder="authored-outofmemory" /></label>
    <label>Solution<textarea bind:value={solution} rows="3"></textarea></label>
    <label>Symptoms (one per line)<textarea bind:value={symptoms} rows="3"></textarea></label>
    <label>Suspected (comma)<input bind:value={suspected} /></label>
    <label>Actions JSON<textarea bind:value={actionsJson} rows="6" class="mono"></textarea></label>
    <label>Notes (local only)<textarea bind:value={notes} rows="2"></textarea></label>
    <div class="actions">
      <button class="primary" onclick={onSave} disabled={busy || !solution.trim()}>Save</button>
      <button class="ghost" onclick={onCopyExport} disabled={!exportPreview}>Copy export</button>
      <button class="ghost" onclick={onOpenFolder}>Open folder</button>
      <button class="ghost" onclick={() => (open = false)}>Close</button>
    </div>
    {#if message}<p class="muted-inline">{message}</p>{/if}
  </div>
{/if}
