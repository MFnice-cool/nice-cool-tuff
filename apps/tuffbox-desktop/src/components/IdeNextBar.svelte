<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { Stethoscope, ArrowRight, X, AlertTriangle } from "@lucide/svelte";
  import { launchWithFeedback, launchingPath } from "../lib/launch";
  import {
    projectPath,
    ideStageRequest,
    ideSuggestedStage,
    ideIssueCount,
    ideNeedsHealth,
    workTrail,
    clearWorkTrail,
    escalateIgnoredWorkTrail,
    WORK_TRAIL_ESCALATE_MS,
    computeIdeNextAction,
    packStatusLabel,
    briefDirty,
    tuneDirty,
    questDirty,
    ideNextTrigger,
    ideIssuesRefresh,
    idePlayTrigger,
    launchSessions,
    isProjectLaunching,
    hideIdeNextBar,
  } from "../lib/store";

  let {
    onGoStage,
  }: {
    onGoStage?: (stage: string) => void;
  } = $props();

  // Plain (non-reactive) re-entrancy guard: this flag is read in the sync
  // prefix of refreshIssues() which the $effect below calls — if it were
  // $state, the finally-reset would re-trigger that effect forever and pin
  // the main thread (the IDE-mount freeze).
  let refreshing = false;
  const launchSession = $derived($launchSessions[$projectPath ?? ""] ?? null);
  const launching = $derived(isProjectLaunching($projectPath, $launchSessions));
  // launching is derived from the shared launch store so the Play button stays
  // disabled until the game is actually running (process-started) or exits.
  const launchingFromPath = $derived($launchingPath === $projectPath);

  const statusLabel = $derived(packStatusLabel($ideIssueCount, $ideNeedsHealth));
  const next = $derived(
    computeIdeNextAction({
      issueCount: $ideIssueCount,
      needsHealth: $ideNeedsHealth,
      briefDirty: $briefDirty,
      tuneDirty: $tuneDirty,
      questDirty: $questDirty,
    }),
  );

  $effect(() => {
    if (next.stage) ideSuggestedStage.set(next.stage);
  });

  async function refreshIssues() {
    if (!$projectPath || refreshing) return;
    refreshing = true;
    try {
      const counts: { errorCount?: number } = await invoke("get_diagnostic_counts", {
        path: $projectPath,
      });
      ideIssueCount.set(Number(counts?.errorCount ?? 0));
    } catch {
      /* keep last count */
    } finally {
      refreshing = false;
    }
  }

  $effect(() => {
    if ($projectPath) {
      void refreshIssues();
    } else {
      ideIssueCount.set(0);
      ideNeedsHealth.set(false);
    }
  });

  $effect(() => {
    void $ideNextTrigger;
    if ($ideNextTrigger > 0) runNext();
  });

  $effect(() => {
    void $idePlayTrigger;
    if ($idePlayTrigger > 0) void runPlay();
  });

  $effect(() => {
    void $ideIssuesRefresh;
    if ($ideIssuesRefresh > 0) void refreshIssues();
  });

  /** After 5 minutes ignored, swap contextual tip → main pack problems. */
  $effect(() => {
    const trail = $workTrail;
    if (!trail || trail.escalated) return;

    const createdAt = trail.createdAt;
    const delay = Math.max(0, WORK_TRAIL_ESCALATE_MS - (Date.now() - createdAt));
    const timer = setTimeout(() => {
      void (async () => {
        if ($workTrail?.createdAt !== createdAt || $workTrail?.escalated) return;
        await refreshIssues();
        if ($workTrail?.createdAt !== createdAt || $workTrail?.escalated) return;
        escalateIgnoredWorkTrail({
          issueCount: $ideIssueCount,
          needsHealth: $ideNeedsHealth,
        });
      })();
    }, delay);

    return () => clearTimeout(timer);
  });

  function go(stage: string) {
    onGoStage?.(stage);
    ideStageRequest.set(stage);
  }

  function runNext() {
    if (next.stage) go(next.stage);
  }

  async function runPlay() {
    if (!$projectPath || launching) return;
    try {
      await launchWithFeedback({ path: $projectPath, profile: "client" });
      // launchWithFeedback returns once the JVM is spawned; keep the spinner
      // honest by clearing on this instance's process-exited event (with a
      // grace fallback) instead of resetting synchronously.
      const path = $projectPath;
      let exited = false;
      let unlisten: () => void = () => {};
      const onExited = (event: { payload?: { id?: string } }) => {
        if (event.payload?.id === path) {
          exited = true;
          unlisten();
        }
      };
      listen<{ id: string; code?: number | null }>("process-exited", onExited).then((fn) => {
        if (exited) fn();
        else unlisten = fn;
      });
      setTimeout(() => {
        if (!exited) {
          unlisten();
        }
      }, 15000);
    } catch {
      // Error handled by launchWithFeedback's toast
    }
  }

  function onTrailAction(kind: string, stage?: string) {
    if (kind === "dismiss") {
      clearWorkTrail();
      return;
    }
    if (kind === "play") {
      clearWorkTrail();
      void runPlay();
      return;
    }
    if (kind === "stage" && stage) {
      clearWorkTrail();
      go(stage);
    }
  }

  /** Expose for parent (optional). */
  export function triggerNext() {
    runNext();
  }

  export function refresh() {
    void refreshIssues();
  }
</script>

<!-- The component stays mounted when the "IDE top panel" setting hides it:
     its effects also serve the global ide:next / play (Ctrl+Shift+P)
     triggers — unmounting would orphan those shortcuts. -->
<div class="ide-next-bar" class:panel-hidden={$hideIdeNextBar}>
  <div class="ide-next-status">
    {#if $ideIssueCount > 0}
      <span class="pill warn">
        <AlertTriangle size={13} />
        {statusLabel}
      </span>
    {:else if $ideNeedsHealth}
      <span class="pill warn"><Stethoscope size={13} /> {statusLabel}</span>
    {:else}
      <span class="pill ok"><Stethoscope size={13} /> {statusLabel}</span>
    {/if}
    {#if next.detail}
      <span class="detail" title={next.detail}>{next.detail}</span>
    {/if}
    {#if launching}
      <span class="detail launch-detail" title={launchSession?.message}>
        {launchSession?.message || "Launching…"}
      </span>
    {/if}
  </div>

  <div class="ide-next-main">
    <button
      type="button"
      class="next-cta"
      onclick={runNext}
      disabled={launching}
      title={`Suggested next step: ${next.label}`}
      aria-label={`Suggested next step: ${next.label}`}
    >
      {next.label}
      <ArrowRight size={14} />
    </button>
  </div>

  <div class="ide-next-actions">
    <button
      type="button"
      class="ghost"
      onclick={() => go("diagnose")}
      title="Open the Health check — read crash logs and find what breaks the pack"
    >
      <Stethoscope size={14} />
      Health check
    </button>
  </div>
</div>

{#if $workTrail}
  <div
    class="ide-work-trail"
    class:panel-hidden={$hideIdeNextBar}
    class:escalated={!!$workTrail.escalated}
    role="status"
  >
    <span class="trail-msg">{$workTrail.message}</span>
    <div class="trail-actions">
      {#each $workTrail.actions as act (act.id)}
        <button
          type="button"
          class={act.kind === "dismiss" ? "ghost mini" : "secondary mini"}
          onclick={() => onTrailAction(act.kind, act.stage)}
        >
          {act.label}
        </button>
      {/each}
      <button type="button" class="icon-x" onclick={() => clearWorkTrail()} title="Dismiss" aria-label="Dismiss">
        <X size={14} />
      </button>
    </div>
  </div>
{/if}

<style>
  .ide-next-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px 14px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border-color);
    background: color-mix(in srgb, var(--bg-secondary) 88%, transparent);
    flex-shrink: 0;
  }
  /* "IDE top panel" setting (Settings → Appearance): the bar (and the work
     trail in the same strip) leave the layout entirely, while the component
     stays mounted to keep serving global triggers. */
  .panel-hidden {
    display: none;
  }
  .ide-next-status {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 3px 9px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 700;
    white-space: nowrap;
  }
  .pill.ok {
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    color: var(--accent-primary);
  }
  .pill.warn {
    background: rgba(245, 158, 11, 0.14);
    color: var(--accent-warning);
  }
  .detail {
    font-size: 12.5px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ide-next-main {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 140px;
  }
  .next-cta {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    border-radius: var(--border-radius-sm);
    border: none;
    background: var(--accent-primary);
    color: var(--on-accent, #000);
    font-size: 12.5px;
    font-weight: 700;
    cursor: pointer;
  }
  .next-cta:disabled { opacity: 0.6; cursor: not-allowed; }
  .ide-next-actions {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }
  .ide-next-actions .ghost {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-primary);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .ide-next-actions .ghost:hover { color: var(--text-primary); }
  .ide-work-trail {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid color-mix(in srgb, var(--accent-primary) 25%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
    font-size: 12px;
    flex-shrink: 0;
  }
  .ide-work-trail.escalated {
    border-bottom-color: color-mix(in srgb, var(--accent-warning, #f59e0b) 40%, transparent);
    background: color-mix(in srgb, var(--accent-warning, #f59e0b) 12%, transparent);
  }
  .trail-msg { color: var(--text-primary); font-weight: 600; }
  .trail-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .secondary.mini, .ghost.mini {
    padding: 4px 9px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
  }
  .secondary.mini {
    background: var(--bg-primary);
    color: var(--text-primary);
  }
  .ghost.mini { background: transparent; color: var(--text-secondary); }
  .icon-x {
    border: none;
    background: transparent;
    color: var(--text-muted);
    padding: 4px;
    cursor: pointer;
    display: inline-flex;
  }
</style>
