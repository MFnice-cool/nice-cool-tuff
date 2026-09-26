<script lang="ts">
  let {
    session,
    suspects,
    active,
    status,
    phaseKey,
    busy,
    auto = $bindable(),
    projectPath,
    launching,
    onStart,
    onTest,
    onReport,
    onCancel,
  }: {
    session: any | null;
    suspects: string[];
    active: boolean;
    status: string;
    phaseKey: string;
    busy: boolean;
    auto: boolean;
    projectPath: string | null;
    launching: boolean;
    onStart: () => void;
    onTest: () => void;
    onReport: (outcome: "healthy" | "crash") => void;
    onCancel: () => void;
  } = $props();
</script>

{#if session}
  <div class="notice warning group-test-panel">
    <strong>Group test</strong>
    <span>step {session.step} · {status}</span>
    <small>
      Covering {session.covering.length}
      · clean {session.knownClean.length}
      {#if session.testGroup.length}
        · testing [{session.testGroup.join(", ")}]
      {/if}
    </small>
    {#if session.defectives.length}
      <small>Isolated: {session.defectives.join(", ")}</small>
    {/if}
    {#if active}
      <label class="group-test-auto">
        <input type="checkbox" bind:checked={auto} />
        Auto-launch next step
      </label>
      <div class="group-test-actions">
        <button type="button" class="secondary small" disabled={launching || busy} onclick={onTest}>Test launch</button>
        <button type="button" class="secondary small" disabled={busy} onclick={() => onReport("crash")}>Still crashed</button>
        <button type="button" class="secondary small" disabled={busy} onclick={() => onReport("healthy")}>Launched</button>
        <button type="button" class="ghost small" disabled={busy} onclick={onCancel}>Cancel</button>
      </div>
    {:else if phaseKey === "done"}
      <small>Verified covering. Share prompt can use these disables.</small>
    {/if}
  </div>
{:else}
  <div class="notice warning">
    Group test suspects: {suspects.length ? suspects.join(", ") : "recent + crash suspects"}
    <button type="button" class="secondary small" disabled={busy || !projectPath} onclick={onStart}>Start group test</button>
  </div>
{/if}
