<script lang="ts">
  import { CheckCircle, AlertTriangle, CircleHelp, Wrench, ArrowDownToLine } from "@lucide/svelte";
  import { ideStageRequest } from "../../lib/store";

  type MergedRec = {
    id: string;
    source: "rules" | "ai";
    label: string;
    detail: string;
    risk: string;
    modId: string | null;
    apply: () => void;
  };
  type WorldCoords = { x: number; y: number; z: number; label: string };

  let {
    sessionOk = false,
    topSuspect = null,
    topFinding = null,
    heroCulpritLabel = "",
    strongestEvidence = null,
    analysisBusy = false,
    primaryRec = null,
    mergedRecommendations = [],
    aiApplyBusy = false,
    applyingHintId = null,
    disablingModId = null,
    fixingIdx = null,
    aiAnalysis = null,
    logDisplayText = "",
    isHsErr = false,
    hsErrKind = null,
    memoryHint = null,
    worldCoords = null,
    cascadingFinding = null,
    mixinFinding = null,
    sideMismatchFinding = null,
    suspected = [],
    warningCount = 0,
    onFixDisableMod,
    onApplyTopSuspectUpdate,
    onApplyAiPlan,
    onJumpToFirstError,
    onShowWarnings,
  }: {
    sessionOk?: boolean;
    topSuspect?: any;
    topFinding?: any;
    heroCulpritLabel?: string;
    strongestEvidence?: any;
    analysisBusy?: boolean;
    primaryRec?: MergedRec | null;
    mergedRecommendations?: MergedRec[];
    aiApplyBusy?: boolean;
    applyingHintId?: string | null;
    disablingModId?: string | null;
    fixingIdx?: number | null;
    aiAnalysis?: any;
    logDisplayText?: string;
    isHsErr?: boolean;
    hsErrKind?: string | null;
    memoryHint?: string | null;
    worldCoords?: WorldCoords | null | undefined;
    cascadingFinding?: any;
    mixinFinding?: any;
    sideMismatchFinding?: any;
    suspected?: any[];
    warningCount?: number;
    onFixDisableMod?: (modId: string) => void;
    onApplyTopSuspectUpdate?: () => void;
    onApplyAiPlan?: () => void;
    onJumpToFirstError?: () => void;
    onShowWarnings?: () => void;
  } = $props();

  function severityChip(sev: string): string {
    if (sev === "critical") return "Fix this first";
    if (sev === "error") return "Needs a fix";
    if (sev === "warning") return "Worth checking";
    return "FYI";
  }

  function aiPlanActions(analysis: any): any[] {
    return analysis?.actions ?? analysis?.recommended_actions ?? analysis?.recommendedActions ?? [];
  }
</script>

<!-- Verdict first (answer before the scary log) -->
<section class="dx-verdict" class:ok={sessionOk} class:warn={!sessionOk && !!(topSuspect || topFinding)} class:neutral={!sessionOk && !topSuspect && !topFinding}>
  <div class="dx-verdict-icon">
    {#if sessionOk}
      <CheckCircle size={22} />
    {:else if topSuspect || topFinding}
      <AlertTriangle size={22} />
    {:else}
      <CircleHelp size={22} />
    {/if}
  </div>
  <div class="dx-verdict-body">
    {#if sessionOk}
      <span class="eyebrow">You're good</span>
      <h1>Last launch looked fine</h1>
      <p class="dx-verdict-copy">
        No crash to chase right now.
        {#if warningCount > 0}
          <button type="button" class="linkish" onclick={() => onShowWarnings?.()}>
            {warningCount} pack warning{warningCount === 1 ? "" : "s"}
          </button>
          still worth a look.
        {/if}
      </p>
    {:else if topFinding && (!topSuspect || (topFinding.severity === "critical" || topFinding.severity === "error"))}
      <span class="eyebrow">{severityChip(topFinding.severity)}</span>
      <h1>{topFinding.title}</h1>
      <p class="dx-verdict-copy">{topFinding.description}</p>
      {#if topFinding.autoFix}
        <p class="dx-next-step"><strong>Try this:</strong> {topFinding.autoFix}</p>
      {/if}
      {#if topSuspect}
        <p class="dx-verdict-copy muted-inline">
          Suspect: {topSuspect.name || topSuspect.id}
          · {topSuspect.confidence}%
        </p>
      {/if}
    {:else if topSuspect}
      <span class="eyebrow">Looks like this broke it</span>
      <h1>{heroCulpritLabel || topSuspect.name}</h1>
      <p class="dx-verdict-copy">
        {topSuspect.confidence}% confidence
        {#if topSuspect.blameRole}· {topSuspect.blameRole}{/if}
      </p>
      {#if strongestEvidence}
        <p class="dx-evidence"><code>{strongestEvidence}</code></p>
      {/if}
    {:else}
      <span class="eyebrow">Still figuring it out</span>
      <h1>No clear culprit yet</h1>
      <p class="dx-verdict-copy">
        {analysisBusy
          ? "Scanning the log…"
          : "Hit Re-analyze, or open Evidence for the raw log."}
      </p>
    {/if}

    <div class="dx-cta">
      {#if !sessionOk && primaryRec}
        <button
          class="primary"
          type="button"
          onclick={() => primaryRec?.apply()}
          disabled={aiApplyBusy || applyingHintId !== null}
        >
          <Wrench size={15} />
          {primaryRec.label}
        </button>
      {:else if !sessionOk && topSuspect?.knownInManifest}
        <button class="primary" type="button" onclick={() => onFixDisableMod?.(topSuspect.id)} disabled={disablingModId === topSuspect.id}>
          {disablingModId === topSuspect.id ? "Disabling…" : `Disable ${topSuspect.name}`}
        </button>
        <button class="ghost" type="button" onclick={() => onApplyTopSuspectUpdate?.()} disabled={fixingIdx === -1}>Update</button>
      {/if}
      {#if !sessionOk && aiAnalysis && aiPlanActions(aiAnalysis).length > 0}
        <button
          class="secondary"
          type="button"
          onclick={() => onApplyAiPlan?.()}
          disabled={aiApplyBusy || (aiAnalysis.validation && aiAnalysis.validation.ok === false)}
        >
          {aiApplyBusy ? "Applying…" : "Review AI plan"}
        </button>
      {/if}
      {#if !sessionOk}
        <button class="ghost" type="button" onclick={() => onJumpToFirstError?.()} disabled={!logDisplayText}>
          <ArrowDownToLine size={15} /> Evidence
        </button>
      {/if}
    </div>
  </div>
</section>

{#if !sessionOk && (isHsErr || memoryHint || cascadingFinding || mixinFinding || sideMismatchFinding || worldCoords)}
  <div class="dx-class-cards">
    {#if isHsErr}
      <div class="dx-class-card">
        <strong>Java native crash</strong>
        <p>
          {hsErrKind === "oom"
            ? "JVM ran out of memory. Raise RAM carefully and check for leaks."
            : "JVM fatal error — check Problematic frame and GPU/Java version."}
        </p>
        <button type="button" class="ghost mini" onclick={() => ideStageRequest.set("setup")}>Open Setup</button>
      </div>
    {/if}
    {#if memoryHint && !isHsErr}
      <div class="dx-class-card">
        <strong>Out of memory</strong>
        <p>{memoryHint}</p>
        <button type="button" class="ghost mini" onclick={() => ideStageRequest.set("setup")}>JVM / Setup</button>
      </div>
    {/if}
    {#if cascadingFinding}
      <div class="dx-class-card warn">
        <strong>Cascading error</strong>
        <p>{cascadingFinding.description}</p>
        <button type="button" class="ghost mini" onclick={() => onJumpToFirstError?.()}>Jump to early error</button>
      </div>
    {/if}
    {#if mixinFinding}
      <div class="dx-class-card">
        <strong>Mixin conflict</strong>
        <p>{mixinFinding.description}</p>
        <button type="button" class="ghost mini" onclick={() => onJumpToFirstError?.()}>Open Evidence</button>
      </div>
    {/if}
    {#if sideMismatchFinding}
      <div class="dx-class-card">
        <strong>Client-only / wrong side</strong>
        <p>{sideMismatchFinding.description}</p>
      </div>
    {/if}
    {#if worldCoords}
      <div class="dx-class-card">
        <strong>{worldCoords.label} @ {worldCoords.x}, {worldCoords.y}, {worldCoords.z}</strong>
        <p>Hint: restore nearby chunk or teleport away if a ticking entity is stuck.</p>
      </div>
    {/if}
  </div>
{/if}

<style>
  .muted-inline { margin: 0; color: var(--text-muted); font-size: 12px; }
  .eyebrow { display: block; margin-bottom: 4px; color: var(--text-muted); font-size: 12px; font-weight: 800; letter-spacing: .08em; text-transform: uppercase; }
  .linkish {
    border: none; background: none; padding: 0;
    color: var(--accent-primary); font: inherit; font-weight: 700;
    cursor: pointer; text-decoration: underline;
  }
  .dx-verdict {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 14px;
    padding: 18px;
    margin-bottom: 14px;
    border-radius: var(--border-radius-lg);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
  }
  .dx-verdict.warn {
    border-color: rgba(245, 158, 11, 0.42);
    background: linear-gradient(135deg, rgba(245, 158, 11, 0.11), var(--bg-secondary) 65%);
  }
  .dx-verdict.ok {
    border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent);
    background: linear-gradient(135deg, color-mix(in srgb, var(--accent-primary) 8%, transparent), var(--bg-secondary) 65%);
  }
  .dx-verdict-icon {
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    border-radius: var(--border-radius-md);
    color: var(--text-muted);
    background: var(--bg-tertiary);
  }
  .dx-verdict.warn .dx-verdict-icon { color: var(--accent-warning); background: rgba(245, 158, 11, 0.13); }
  .dx-verdict.ok .dx-verdict-icon { color: var(--accent-primary); background: color-mix(in srgb, var(--accent-primary) 13%, transparent); }
  .dx-verdict-body { min-width: 0; }
  .dx-verdict-body h1 { margin: 0; color: var(--text-primary); font-size: 20px; line-height: 1.3; }
  .dx-verdict-copy { margin: 6px 0 0; color: var(--text-secondary); font-size: 13px; line-height: 1.45; }
  .dx-next-step {
    margin: 10px 0 0;
    padding: 10px 12px;
    border-radius: var(--border-radius-sm);
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 22%, transparent);
    color: var(--text-primary);
    font-size: 13px;
    line-height: 1.4;
  }
  .dx-evidence {
    margin: 10px 0 0;
    padding: 10px 12px;
    border-left: 3px solid var(--accent-warning);
    border-radius: 0 10px 10px 0;
    background: var(--bg-tertiary);
    font-size: 12px;
    color: var(--text-secondary);
    word-break: break-word;
  }
  .dx-cta { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-top: 14px; }
  .dx-cta button:disabled { opacity: 0.55; cursor: not-allowed; }
  .dx-class-cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 8px;
    margin-bottom: 12px;
  }
  .dx-class-card {
    padding: 10px 12px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    font-size: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .dx-class-card.warn {
    border-color: rgba(251, 191, 36, 0.45);
    background: rgba(251, 191, 36, 0.08);
  }
  .dx-class-card p { margin: 0; color: var(--text-secondary); }
  @media (max-width: 720px) {
    .dx-verdict { grid-template-columns: 1fr; }
  }
</style>
