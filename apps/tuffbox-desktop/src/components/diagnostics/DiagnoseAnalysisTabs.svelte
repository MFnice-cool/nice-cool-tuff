<script lang="ts">
  import { fly } from "svelte/transition";
  import { quintOut } from "svelte/easing";
  import { Zap, MessageCircle } from "@lucide/svelte";

  type FixAction = {
    kind: string;
    label: string;
    modId: string | null;
  };

  let {
    crashFindings = [],
    crashLoading = false,
    aiAnalysis = null,
    aiLoading = false,
    aiSoftError = null,
    aiApplyBusy = false,
    aiFeedbackBusy = false,
    aiFeedbackMsg = null,
    applyingHintId = null,
    cascadeLabel = null,
    onApplyFindingFix,
    onRetryAi,
    onApplyAiPlan,
    onFeedback,
  }: {
    crashFindings?: any[];
    crashLoading?: boolean;
    aiAnalysis?: any;
    aiLoading?: boolean;
    aiSoftError?: string | null;
    aiApplyBusy?: boolean;
    aiFeedbackBusy?: boolean;
    aiFeedbackMsg?: string | null;
    applyingHintId?: string | null;
    cascadeLabel?: string | null;
    onApplyFindingFix?: (payload: { finding: any; action: FixAction }) => void;
    onRetryAi?: () => void;
    onApplyAiPlan?: () => void;
    onFeedback?: (helpful: boolean) => void;
  } = $props();

  let detailTab: "rules" | "ai" = $state("rules");
  const sourceBadge = $derived(cascadeLabel || aiAnalysis?.source || null);

  function severityChip(sev: string): string {
    if (sev === "critical") return "Fix this first";
    if (sev === "error") return "Needs a fix";
    if (sev === "warning") return "Worth checking";
    return "FYI";
  }

  function aiPlanActions(analysis: any): any[] {
    return analysis?.actions ?? analysis?.recommended_actions ?? analysis?.recommendedActions ?? [];
  }

  function aiActionLabel(action: any): string {
    const op = String(action?.op ?? action?.action_type ?? action?.actionType ?? "").toLowerCase();
    switch (op) {
      case "install_mod":
      case "install":
        return "Install";
      case "remove_mod":
      case "remove":
        return "Remove";
      case "disable_mod":
      case "disable":
        return "Disable";
      case "update_mod":
      case "update":
        return "Update";
      case "change_mod_version":
        return "Change version";
      case "reinstall_mod":
      case "reinstall":
        return "Reinstall";
      case "edit_config":
      case "config_change":
        return "Edit config";
      default:
        return op || "Action";
    }
  }

  function aiActionVersion(action: any): string | null {
    const v = String(action?.version ?? "").trim();
    if (!v) return null;
    const fake = new Set(["1.2.3", "0.0.0", "x.y.z", "latest", "version", "unknown", "null", "string"]);
    if (fake.has(v.toLowerCase()) || v === "X.Y.Z" || v === "<version>" || v === "{{version}}") return null;
    return v;
  }
</script>

<!-- 3. Analysis as tabs (not side-by-side) -->
<section class="dx-tabs panel">
  <div class="dx-tabbar" role="tablist">
    <button
      type="button"
      role="tab"
      class="dx-tab"
      class:active={detailTab === "rules"}
      aria-selected={detailTab === "rules"}
      onclick={() => (detailTab = "rules")}
    >
      <Zap size={14} /> Rules
      {#if crashFindings.length}<span class="count">{crashFindings.length}</span>{/if}
      {#if crashLoading}<span class="analyzing-pill">…</span>{/if}
    </button>
    <button
      type="button"
      role="tab"
      class="dx-tab"
      class:active={detailTab === "ai"}
      aria-selected={detailTab === "ai"}
      onclick={() => (detailTab = "ai")}
    >
      <MessageCircle size={14} /> AI
      {#if sourceBadge}<span class="ai-source-badge">{sourceBadge}</span>{/if}
      {#if aiLoading}<span class="analyzing-pill">…</span>{/if}
    </button>
  </div>

  {#key detailTab}
    {#if detailTab === "rules"}
      <div class="dx-tabpanel" role="tabpanel" in:fly={{ x: -12, duration: 280, opacity: 0, easing: quintOut }}>
        {#if crashFindings.length === 0 && !crashLoading}
          <div class="muted-box">No rule-based findings for this source.</div>
        {:else}
          <div class="findings-stack">
            {#each crashFindings.slice(0, 10) as f, fIdx (f.code + f.title + fIdx)}
              <article class="finding-card {f.severity}" class:ai-agree={f.aiAgree}>
                <header>
                  <span class="sev-chip {f.severity}">{severityChip(f.severity)}</span>
                  <strong>{f.title}</strong>
                  {#if f.aiAgree}<span class="ai-agree-badge" title={f.aiHint ?? ""}>AI agrees</span>{/if}
                </header>
                <p>{f.description}</p>
                {#if f.aiHint}<p class="ai-hint">AI: {f.aiHint}</p>{/if}
                {#if f.autoFix}<p class="auto-fix"><strong>Try this:</strong> {f.autoFix}</p>{/if}
                {#if f.fixes?.length}
                  <div class="finding-actions">
                    {#each f.fixes.slice(0, 3) as action (action.kind + (action.modId ?? "") + action.label)}
                      <button class="secondary small" onclick={() => onApplyFindingFix?.({ finding: f, action })} disabled={applyingHintId !== null}>
                        {action.label}
                      </button>
                    {/each}
                  </div>
                {/if}
              </article>
            {/each}
          </div>
        {/if}
      </div>
    {:else}
      <div class="dx-tabpanel" role="tabpanel" in:fly={{ x: 12, duration: 280, opacity: 0, easing: quintOut }}>
        {#if aiLoading && !aiAnalysis}
          <div class="muted-box">AI is reading this crash…</div>
        {:else if !aiAnalysis}
          <div class="muted-box">
            {#if aiSoftError}
              AI failed — {aiSoftError}
            {:else}
              No AI result yet.
            {/if}
            <button class="ghost mini" type="button" onclick={() => onRetryAi?.()}>Retry AI</button>
          </div>
        {:else}
          <p class="ai-human">{aiAnalysis.humanExplanation ?? aiAnalysis.human_explanation}</p>
          <div class="ai-stats compact">
            <div class="ai-stat"><strong>{Math.round((aiAnalysis.confidence ?? 0) * 100)}%</strong> conf</div>
            <div class="ai-stat"><strong>{aiPlanActions(aiAnalysis).length}</strong> actions</div>
            {#if aiAnalysis.model}<div class="ai-stat"><strong>{aiAnalysis.model}</strong></div>{/if}
          </div>
          {#if aiAnalysis.normalizeNotes?.length}
            <div class="notice warning tight">Adjusted: {aiAnalysis.normalizeNotes.join("; ")}</div>
          {/if}
          {#if aiAnalysis.additionalContext ?? aiAnalysis.additional_context}
            <div class="notice warning tight">{aiAnalysis.additionalContext ?? aiAnalysis.additional_context}</div>
          {/if}
          {#if (aiAnalysis.suspectedMods ?? aiAnalysis.suspected_mods)?.length}
            <div class="ai-list">
              <strong>Suspected</strong>
              <div class="crash-tags">
                {#each (aiAnalysis.suspectedMods ?? aiAnalysis.suspected_mods) as modId (modId)}
                  <code>{modId}</code>
                {/each}
              </div>
            </div>
          {/if}
          {#if aiPlanActions(aiAnalysis).length}
            <div class="ai-list">
              <strong>AI ActionPlan</strong>
              <ul>
                {#each aiPlanActions(aiAnalysis) as action, aIdx (aIdx)}
                  <li>
                    <strong>{aiActionLabel(action)}</strong>
                    {#if action.modId ?? action.mod_id}<code>{action.modId ?? action.mod_id}</code>{/if}
                    {#if aiActionVersion(action)}<span class="ai-ver">v{aiActionVersion(action)}</span>{/if}
                    <span class="risk-pill">{action.risk ?? "medium"}</span>
                    <span>{action.reason ?? action.description ?? ""}</span>
                  </li>
                {/each}
              </ul>
            </div>
          {/if}
          <div class="ai-feedback">
            <button class="secondary small" disabled={aiApplyBusy || (aiAnalysis.validation && aiAnalysis.validation.ok === false)} onclick={() => onApplyAiPlan?.()}>
              {aiApplyBusy ? "Applying…" : "Review & apply AI plan"}
            </button>
            <button class="ghost mini" disabled={aiFeedbackBusy} onclick={() => onFeedback?.(true)}>Helped</button>
            <button class="ghost mini" disabled={aiFeedbackBusy} onclick={() => onFeedback?.(false)}>Wrong</button>
            {#if aiFeedbackMsg}<small>{aiFeedbackMsg}</small>{/if}
          </div>
        {/if}
      </div>
    {/if}
  {/key}
</section>

<style>
  .panel { padding: 16px; min-width: 0; background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: var(--border-radius-lg); }
  .muted-box { padding: 12px; border-radius: 10px; border: 1px dashed var(--border-color); color: var(--text-muted); font-size: 12px; }
  .notice { padding: 12px 14px; border-radius: var(--border-radius-lg); margin-bottom: 14px; border: 1px solid var(--border-color); }
  .notice.warning { color: var(--accent-warning); background: color-mix(in srgb, var(--accent-warning) 8%, transparent); border-color: color-mix(in srgb, var(--accent-warning) 28%, transparent); }
  .notice.tight { padding: 8px 10px; margin-bottom: 10px; font-size: 12px; }
  .risk-pill {
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    padding: 1px 6px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    color: var(--text-muted);
  }
  .analyzing-pill {
    display: inline-flex;
    align-items: center;
    padding: 2px 8px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 700;
  }
  .sev-chip {
    display: inline-flex;
    align-items: center;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    padding: 2px 7px;
    border-radius: 999px;
    color: var(--text-muted);
    background: rgba(148, 163, 184, 0.15);
  }
  .sev-chip.critical { color: var(--accent-danger); background: color-mix(in srgb, var(--accent-danger) 18%, transparent); }
  .sev-chip.error { color: var(--accent-danger); background: color-mix(in srgb, var(--accent-danger) 16%, transparent); }
  .sev-chip.warning { color: var(--accent-warning); background: color-mix(in srgb, var(--accent-warning) 14%, transparent); }
  .sev-chip.info { color: var(--accent-secondary); background: color-mix(in srgb, var(--accent-secondary) 12%, transparent); }
  .dx-tabs { padding: 0; overflow: hidden; margin-bottom: 14px; }
  .dx-tabbar {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--border-color);
    background: var(--bg-tertiary);
  }
  .dx-tab {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 11px 16px;
    border: none;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--text-muted);
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    transition:
      color var(--motion-fast) var(--ease-out),
      background var(--motion-fast) var(--ease-out),
      border-color var(--motion-med) var(--ease-spring);
  }
  .dx-tab.active {
    color: var(--text-primary);
    border-bottom-color: var(--accent-primary);
    background: var(--bg-secondary);
  }
  .dx-tabpanel { padding: 14px 16px 16px; }
  .count {
    display: inline-flex;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    place-items: center;
    border-radius: 999px;
    background: var(--bg-tertiary);
    font-size: 12px;
  }
  .findings-stack { display: flex; flex-direction: column; gap: 10px; }
  .finding-card {
    padding: 12px;
    border-radius: 10px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
  }
  .finding-card header { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-bottom: 6px; }
  .finding-card header strong { color: var(--text-primary); }
  .finding-card p { margin: 0 0 6px; color: var(--text-secondary); font-size: 13px; line-height: 1.45; }
  .finding-actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 8px; }
  .ai-hint, .auto-fix { font-size: 12px; color: var(--text-muted); }
  .ai-agree-badge, .ai-source-badge {
    display: inline-flex;
    padding: 2px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 800;
  }
  .ai-human { margin: 0 0 12px; color: var(--text-primary); font-size: 14px; line-height: 1.5; }
  .ai-stats { display: flex; flex-wrap: wrap; gap: 10px; margin-bottom: 12px; }
  .ai-stat { padding: 6px 10px; border-radius: var(--border-radius-sm); background: var(--bg-tertiary); font-size: 12px; color: var(--text-muted); }
  .ai-stat strong { color: var(--text-primary); margin-right: 4px; }
  .ai-list { margin-top: 12px; }
  .ai-list strong { display: block; margin-bottom: 6px; font-size: 12px; color: var(--text-muted); }
  .ai-list ul { margin: 0; padding-left: 18px; color: var(--text-secondary); font-size: 13px; }
  .ai-list li { margin-bottom: 6px; }
  .ai-feedback { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-top: 14px; }
  .crash-tags { display: flex; flex-wrap: wrap; gap: 8px; }
  .crash-tags code { padding: 2px 6px; border-radius: 4px; background: var(--bg-secondary); font-size: 12px; }
</style>
