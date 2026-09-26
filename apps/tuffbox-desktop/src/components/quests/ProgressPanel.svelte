<script lang="ts">
  import { Eye, RefreshCw } from "@lucide/svelte";
  import type { QuestProgressSnapshot, QuestProgressTeamRef } from "../../lib/api";

  let {
    open = $bindable(false),
    progressMode = "save",
    progressOverlay = $bindable(false),
    progressSnap = null,
    progressTeams = [],
    progressKey = $bindable(""),
    progressLoading = false,
    simCompleted = [],
    simBusy = false,
    onentersave,
    onentersimulate,
    onloadprogress,
    onseed,
    onreset,
    onrefreshsim,
  }: {
    open?: boolean;
    progressMode?: "save" | "simulate";
    progressOverlay?: boolean;
    progressSnap?: QuestProgressSnapshot | null;
    progressTeams?: QuestProgressTeamRef[];
    progressKey?: string;
    progressLoading?: boolean;
    simCompleted?: string[];
    simBusy?: boolean;
    onentersave: () => void;
    onentersimulate: () => void;
    onloadprogress: () => void;
    onseed: () => void;
    onreset: () => void;
    onrefreshsim: () => void;
  } = $props();

  const progressTeamLabel = $derived(progressTeams.find((t) => t.relativePath === progressKey));
</script>

<details class="prog-details" bind:open>
  <summary><Eye size={14} /> Progress</summary>
  <div class="prog-bar">
    <div class="prog-modes" role="group" aria-label="Progress mode">
      <button
        type="button"
        class="ghost"
        class:sel={progressMode === "save"}
        aria-pressed={progressMode === "save"}
        onclick={() => void onentersave()}
        >Save overlay</button
      >
      <button
        type="button"
        class="ghost"
        class:sel={progressMode === "simulate"}
        aria-pressed={progressMode === "simulate"}
        onclick={() => void onentersimulate()}
        >Simulate</button
      >
    </div>
    <label class="prog-toggle">
      <input
        type="checkbox"
        bind:checked={progressOverlay}
        disabled={progressMode === "save" ? !progressSnap : false}
        title="Show progress on canvas"
      />
      Overlay
    </label>
    <select
      bind:value={progressKey}
      aria-label={progressMode === "simulate" ? "Team for Seed from team" : "Team for save overlay"}
      onchange={() => {
        if (progressMode === "save") void onloadprogress();
      }}
      disabled={progressLoading || progressTeams.length === 0}
      title={progressMode === "simulate" ? "Team used for Seed from team" : "Load save overlay"}
    >
      <option value="">
        {progressTeams.length === 0
          ? "No saves/*/ftbquests progress"
          : "Select team / player…"}
      </option>
      {#each progressTeams as t (t.relativePath)}
        <option value={t.relativePath}>{t.world} — {t.name}</option>
      {/each}
    </select>
    {#if progressMode === "save"}
      <button
        type="button"
        class="ghost"
        disabled={progressLoading || !progressKey}
        onclick={() => void onloadprogress()}
        title="Reload progress"
        aria-label="Reload progress"
      >
        <RefreshCw size={14} class={progressLoading ? "spin" : ""} />
      </button>
    {:else}
      <span class="prog-sim-hint"
        >Alt+click quests to toggle complete ({simCompleted.length})</span
      >
      <button
        type="button"
        class="ghost"
        disabled={progressLoading || !progressKey}
        onclick={() => void onseed()}
        title="Copy completed quests from selected save team"
        aria-label="Seed simulate from selected team"
        >Seed from team</button
      >
      <button
        type="button"
        class="ghost"
        disabled={simBusy}
        onclick={() => void onreset()}
        aria-label="Reset simulate completed quests"
        >Reset</button
      >
      <button
        type="button"
        class="ghost"
        disabled={simBusy}
        onclick={() => void onrefreshsim()}
        title="Reclassify"
        aria-label="Refresh simulate classification"
      >
        <RefreshCw size={14} class={simBusy ? "spin" : ""} />
      </button>
    {/if}
    {#if progressTeamLabel}
      <code class="prog-path">{progressTeamLabel.relativePath}</code>
    {/if}
  </div>
  {#if progressTeams.length === 0}
    <p class="prog-empty-hint">
      Run the client once and open a world so FTB Quests writes save progress, then refresh.
      Simulate mode still works without a save.
    </p>
  {/if}
</details>

<style>
  .prog-details {
    flex-shrink: 0;
    margin: 0 12px 8px;
    border: 1px solid var(--ftbq-frame);
    border-radius: 3px;
    background: var(--ftbq-bg-panel);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.05);
    padding: 0 8px;
  }
  .prog-details summary {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    padding: 6px 4px;
    font-size: 12px;
    color: var(--ftbq-text-muted);
    list-style: none;
  }
  .prog-details summary::-webkit-details-marker {
    display: none;
  }
  .prog-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    margin-bottom: 8px;
    padding: 6px 10px;
    border-radius: 3px;
    border: 1px solid var(--ftbq-frame);
    background: var(--ftbq-bg-panel);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.05);
    color: var(--ftbq-text-muted);
    font-size: 12px;
  }
  .prog-modes {
    display: inline-flex;
    gap: 8px;
  }
  .prog-toggle {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--ftbq-text-muted);
    cursor: pointer;
  }
  .prog-sim-hint {
    color: var(--ftbq-text-muted);
  }
  .prog-bar select {
    min-width: 200px;
    max-width: 360px;
    font-size: 12px;
    padding: 4px 6px;
    background: var(--ftbq-input-bg);
    border: 1px solid var(--ftbq-frame);
    color: inherit;
    border-radius: 3px;
  }
  .prog-path {
    font-size: 12px;
    color: var(--ftbq-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 280px;
  }
  .prog-empty-hint {
    margin: 0 4px 8px;
    font-size: 12px;
    line-height: 1.35;
    color: var(--ftbq-text-muted);
  }
  .prog-modes :global(button.ghost.sel) {
    color: var(--ftbq-accent-teal);
    border-color: rgba(61, 184, 168, 0.45);
    background: rgba(61, 184, 168, 0.1);
  }
</style>
