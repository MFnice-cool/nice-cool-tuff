<script lang="ts">
  import { Wrench, ExternalLink, Eye } from "@lucide/svelte";
  import {
    type Problem,
    type FixAction,
    severityChip,
    crashProblems,
    packProblems,
  } from "./problemModel";

  let {
    problems = [],
    sessionOk = false,
    applyingId = null,
    appliedIds = new Set<string>(),
    onApply,
    onWhy,
  }: {
    problems?: Problem[];
    sessionOk?: boolean;
    applyingId?: string | null;
    appliedIds?: Set<string>;
    onApply?: (problem: Problem, action: FixAction) => void;
    onWhy?: (problem: Problem) => void;
  } = $props();

  const crash = $derived(crashProblems(problems));
  const pack = $derived(packProblems(problems));
  const showCrash = $derived(!sessionOk || crash.length > 0);
  const showPackDivider = $derived(showCrash && crash.length > 0 && pack.length > 0);

  function riskLabel(risk: Problem["risk"]): string {
    if (risk === "destructive") return "Removes files · snapshot first";
    if (risk === "caution") return "Snapshot before apply";
    return "Safe change";
  }
</script>

<section class="dx-problems" id="dx-problems-list">
  {#if problems.length === 0}
    <div class="dx-problems-empty">
      {#if sessionOk}
        <p>No pack issues detected. You're clear to play or export.</p>
      {:else}
        <p>No problems listed yet. Hit <strong>Re-analyze</strong> if the game still crashes, or open <strong>Evidence</strong> for the raw log.</p>
      {/if}
    </div>
  {:else}
    {#if showCrash && crash.length}
      <div class="dx-problems-section">
        {#if !sessionOk}
          <h3 class="dx-sec-label">Crash</h3>
        {/if}
        <ul class="dx-problem-list">
          {#each crash as p (p.id)}
            {@render card(p)}
          {/each}
        </ul>
      </div>
    {/if}

    {#if pack.length}
      <div class="dx-problems-section" id="dx-pack-warnings">
        {#if showPackDivider}
          <h3 class="dx-sec-label">Also in this pack</h3>
        {:else if sessionOk}
          <h3 class="dx-sec-label">Worth checking</h3>
        {:else}
          <h3 class="dx-sec-label">Pack</h3>
        {/if}
        <ul class="dx-problem-list">
          {#each pack as p (p.id)}
            {@render card(p)}
          {/each}
        </ul>
      </div>
    {/if}
  {/if}
</section>

{#snippet card(p: Problem)}
  {@const installActs = p.actions.filter(
    (a) =>
      a.kind === "installDependency" ||
      a.kind === "installAllMissing" ||
      a.kind === "installMissingForMod",
  )}
  {@const otherActs = p.actions.filter(
    (a) =>
      a.kind !== "installDependency" &&
      a.kind !== "installAllMissing" &&
      a.kind !== "installMissingForMod",
  )}
  {@const primary =
    installActs.find((a) => a.kind === "installAllMissing" || a.kind === "installMissingForMod") ??
    installActs[0] ??
    otherActs[0]}
  {@const secondaryInstalls = installActs.filter((a) => a !== primary)}
  {@const secondaryOther = otherActs.filter((a) => a !== primary).slice(0, 4)}
  <li class="dx-card sev-{p.severity}" class:busy={applyingId === p.id} class:applied={appliedIds.has(p.id)}>
    <div class="dx-card-top">
      <span class="sev-chip">{severityChip(p.severity)}</span>
      <span class="cat-chip">{p.category}</span>
      <span class="src-chip">{p.source === "ai" ? "AI" : p.source}</span>
      {#if appliedIds.has(p.id)}
        <span class="applied-chip">Applied — verify</span>
      {/if}
    </div>
    <h4>{p.title}</h4>
    {#if p.summary}
      <p class="summary">{p.summary}</p>
    {/if}
    {#if p.modIds.length}
      <p class="mods">
        {#each p.modIds as mid (mid)}
          <code>{mid}</code>
        {/each}
      </p>
    {/if}
    {#if p.steps?.length && !primary}
      <ol class="steps">
        {#each p.steps as step, i (i)}
          <li>{step}</li>
        {/each}
      </ol>
    {/if}
    <div class="dx-card-actions">
      {#if primary}
        <button
          type="button"
          class="primary"
          class:danger={p.risk === "destructive"}
          disabled={applyingId === p.id}
          onclick={() => onApply?.(p, primary)}
        >
          <Wrench size={14} />
          {applyingId === p.id ? "Applying…" : primary.label}
        </button>
      {/if}
      {#each secondaryOther as act (act.kind + act.label + (act.modId ?? ""))}
        <button
          type="button"
          class="ghost"
          disabled={applyingId === p.id}
          onclick={() => onApply?.(p, act)}
        >
          {#if act.kind === "openResolve" || act.kind === "openSetup"}
            <ExternalLink size={13} />
          {/if}
          {act.label}
        </button>
      {/each}
      {#if p.evidence?.line || p.source === "hint" || p.source === "rules" || p.layer === "crash"}
        <button type="button" class="ghost" onclick={() => onWhy?.(p)}>
          <Eye size={13} /> Why?
        </button>
      {/if}
    </div>
    {#if secondaryInstalls.length > 0}
      <div class="dx-install-grid">
        {#each secondaryInstalls as act (act.kind + act.label + (act.modId ?? ""))}
          <button
            type="button"
            class="install-chip"
            disabled={applyingId === p.id}
            onclick={() => onApply?.(p, act)}
          >
            {act.label}
          </button>
        {/each}
      </div>
    {/if}
    {#if primary}
      <p class="risk" class:destructive={p.risk === "destructive"}>{riskLabel(p.risk)}</p>
    {/if}
  </li>
{/snippet}

<style>
  .dx-problems { min-width: 0; }
  .dx-problems-empty {
    padding: 28px 16px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
    border: 1px dashed var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
  }
  .dx-problems-empty strong { color: var(--text-secondary); }
  .dx-sec-label {
    margin: 0 0 8px;
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .dx-problems-section + .dx-problems-section { margin-top: 18px; }
  .dx-problem-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .dx-card {
    padding: 14px 16px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
  }
  .dx-card.sev-critical {
    border-color: rgba(239, 68, 68, 0.45);
    background: linear-gradient(135deg, rgba(239, 68, 68, 0.08), var(--bg-secondary) 70%);
  }
  .dx-card.sev-error {
    border-color: rgba(245, 158, 11, 0.4);
    background: linear-gradient(135deg, rgba(245, 158, 11, 0.07), var(--bg-secondary) 70%);
  }
  .dx-card.busy { opacity: 0.75; }
  .dx-card.applied {
    border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }
  .applied-chip {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0.02em;
    background: color-mix(in srgb, var(--accent-primary) 14%, transparent);
    color: var(--accent-primary);
  }
  .dx-card-top { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 8px; }
  .sev-chip, .cat-chip, .src-chip {
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0.02em;
  }
  .sev-chip { background: color-mix(in srgb, var(--accent-warning) 15%, transparent); color: var(--accent-warning); }
  .sev-critical .sev-chip { background: color-mix(in srgb, var(--accent-danger) 15%, transparent); color: var(--accent-danger); }
  .cat-chip, .src-chip { background: var(--bg-tertiary); color: var(--text-muted); text-transform: capitalize; }
  .dx-card h4 { margin: 0; font-size: 15px; color: var(--text-primary); }
  .summary { margin: 6px 0 0; font-size: 13px; line-height: 1.45; color: var(--text-secondary); }
  .mods { margin: 8px 0 0; display: flex; flex-wrap: wrap; gap: 8px; }
  .mods code {
    font-size: 12px;
    padding: 2px 6px;
    border-radius: 6px;
    background: var(--bg-tertiary);
    color: var(--text-muted);
  }
  .steps {
    margin: 10px 0 0;
    padding-left: 18px;
    font-size: 12px;
    color: var(--text-secondary);
    line-height: 1.45;
  }
  .dx-card-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 12px;
    align-items: center;
  }
  .dx-install-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 10px;
  }
  .install-chip {
    padding: 5px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 35%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
  }
  .install-chip:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-primary) 16%, transparent);
  }
  .install-chip:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .dx-card-actions .primary {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 7px 12px;
    border-radius: var(--border-radius-sm);
    border: none;
    background: var(--accent-primary);
    color: #000;
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
  }
  .dx-card-actions .primary.danger {
    background: var(--accent-danger);
    color: var(--on-accent, #fff);
  }
  .dx-card-actions .ghost {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: transparent;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .dx-card-actions .ghost:hover { color: var(--text-primary); background: var(--bg-hover); }
  .dx-card-actions button:disabled { opacity: 0.55; cursor: not-allowed; }
  .risk {
    margin: 8px 0 0;
    font-size: 12px;
    color: var(--text-muted);
  }
  .risk.destructive { color: var(--accent-danger); font-weight: 600; }
</style>
