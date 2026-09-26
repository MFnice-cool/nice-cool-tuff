<script lang="ts">
  import { onMount } from "svelte";
  import { Share2, Pencil, Check, X } from "@lucide/svelte";
  import { trapFocus } from "../lib/focusTrap";
  import { invoke } from "@tauri-apps/api/core";

  type DistillAction = {
    op: string;
    modId?: string | null;
    projectId?: string | null;
    version?: string | null;
    path?: string | null;
    reason?: string | null;
    risk?: string;
  };

  type DistillValidation = {
    ok?: boolean;
    errors?: string[];
    warnings?: string[];
  };

  type DistillPlan = {
    humanExplanation?: string;
    confidence?: number;
    actions?: DistillAction[];
    fingerprintKey?: string;
    distillSource?: string;
    resolutionId?: string;
    beta?: boolean;
    playerTrail?: boolean;
    source?: string;
    validation?: DistillValidation;
    groundingNotes?: string[];
  };

  const KNOWN_OPS = new Set([
    "install_mod",
    "remove_mod",
    "disable_mod",
    "update_mod",
    "change_mod_version",
    "reinstall_mod",
    "edit_config",
  ]);

  let {
    path = "",
    resolutionId = null,
    seedExplanation = "",
    shareBusy = false,
    shareError = null,
    onconfirm,
    ondismiss,
  }: {
    path?: string;
    resolutionId?: string | null;
    seedExplanation?: string;
    shareBusy?: boolean;
    shareError?: string | null;
    onconfirm?: (detail: {
      humanExplanation: string;
      actions: DistillAction[];
      fingerprintKey: string | null;
    }) => void | Promise<void>;
    ondismiss?: () => void;
  } = $props();

  let loading = $state(true);
  let error = $state<string | null>(null);
  let plan = $state<DistillPlan | null>(null);
  let editing = $state(false);
  let editExplanation = $state("");
  let editActionsJson = $state("");
  let editError = $state<string | null>(null);
  let confirmBusy = $state(false);

  const validationOk = $derived(plan?.validation?.ok !== false);
  const validationErrors = $derived(plan?.validation?.errors ?? []);
  const validationWarnings = $derived(plan?.validation?.warnings ?? []);
  const canConfirm = $derived(
    !!plan && !confirmBusy && !shareBusy && !loading && validationOk && !error,
  );
  const isPlayerTrail = $derived(
    plan?.playerTrail === true || plan?.distillSource === "player_trail",
  );
  const isGroupTest = $derived(
    plan?.distillSource === "group_test" || plan?.source === "group_test",
  );

  onMount(() => {
    void runDistill();
  });

  async function runDistill() {
    if (!path) {
      loading = false;
      error = "Missing project path";
      return;
    }
    loading = true;
    error = null;
    try {
      plan = await invoke<DistillPlan>("distill_resolved_crash_plan", {
        path,
        resolutionId,
      });
      editExplanation = plan?.humanExplanation ?? seedExplanation ?? "";
      editActionsJson = JSON.stringify(plan?.actions ?? [], null, 2);
      if (plan?.validation && plan.validation.ok === false) {
        error = (plan.validation.errors ?? []).join("; ") || "Plan failed validation";
      }
    } catch (e) {
      error = String(e);
      plan = null;
      editExplanation = seedExplanation || "";
      editActionsJson = "[]";
    } finally {
      loading = false;
    }
  }

  function actionLabel(a: DistillAction): string {
    const target = a.modId || a.projectId || a.path || "-";
    return `${a.op} ${target}${a.version ? ` → ${a.version}` : ""}`;
  }

  function startEdit() {
    editing = true;
    editError = null;
    editExplanation = plan?.humanExplanation ?? editExplanation;
    editActionsJson = JSON.stringify(plan?.actions ?? [], null, 2);
  }

  function validateActions(actions: DistillAction[]): string | null {
    for (const a of actions) {
      if (!a || typeof a !== "object") return "Each action must be an object";
      if (!a.op || typeof a.op !== "string") return "Each action needs an op string";
      if (!KNOWN_OPS.has(a.op)) return `Unknown op: ${a.op}`;
      if (a.op !== "edit_config" && !a.modId && !a.projectId) {
        return `${a.op} requires modId or projectId`;
      }
    }
    return null;
  }

  function applyEdit() {
    editError = null;
    let actions: DistillAction[];
    try {
      const parsed = JSON.parse(editActionsJson);
      if (!Array.isArray(parsed)) {
        editError = "Actions must be a JSON array";
        return;
      }
      actions = parsed;
    } catch {
      editError = "Invalid JSON for actions";
      return;
    }
    const bad = validateActions(actions);
    if (bad) {
      editError = bad;
      return;
    }
    plan = {
      ...(plan ?? {}),
      humanExplanation: editExplanation.trim() || plan?.humanExplanation || "",
      actions,
      distillSource: "user_edited",
      validation: { ok: true, errors: [], warnings: plan?.validation?.warnings ?? [] },
    };
    error = null;
    editing = false;
  }

  async function onConfirm() {
    if (!plan || !canConfirm) return;
    confirmBusy = true;
    try {
      await onconfirm?.({
        humanExplanation: plan.humanExplanation ?? "",
        actions: plan.actions ?? [],
        fingerprintKey: plan.fingerprintKey ?? null,
      });
    } finally {
      // Parent owns shareBusy while the dialog stays open on error.
      confirmBusy = false;
    }
  }
</script>

<div
  class="sc-backdrop"
  role="button"
  tabindex="-1"
  onclick={(e) => e.target === e.currentTarget && ondismiss?.()}
  onkeydown={() => {}}
>
  <div
    class="sc-dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="share-capsule-title"
    use:trapFocus={{ onEscape: () => ondismiss?.() }}
  >
    <div class="sc-icon"><Share2 size={28} /></div>
    <h3 id="share-capsule-title">Share efficient fix with TuffSwarm?</h3>
    {#if !loading && plan}
      <p class="sc-lead">
        {#if isGroupTest}
          Group testing isolated the mods that must stay disabled. Confirm the covering, or edit
          before share.
        {:else if isPlayerTrail}
          We logged launcher actions between the crash and the successful launch, then dropped
          cancelled toggles. Remaining disables are a covering — not proof of a single root cause.
        {:else}
          Beta: AI distilled your fix path into a minimal plan. Confirm if it looks right, or edit so
          peers do not repeat mistakes.
        {/if}
      </p>
    {:else if !loading}
      <p class="sc-lead">
        Review the recorded fix before sharing with TuffSwarm.
      </p>
    {/if}

    {#if loading}
      <p class="sc-status">Preparing share plan…</p>
    {:else if error && !plan}
      <p class="sc-error">{error}</p>
      <div class="sc-actions">
        <button class="ghost" type="button" onclick={() => ondismiss?.()}>Not now</button>
        <button type="button" onclick={() => void runDistill()}>Retry</button>
      </div>
    {:else}
      {#if error}
        <p class="sc-error">{error}</p>
      {/if}
      {#if plan?.distillSource}
        <p class="sc-meta">
          Source: {plan.distillSource}
          {#if plan.confidence != null}
            · confidence {Math.round(plan.confidence * 100)}%
          {/if}
        </p>
      {/if}

      {#if editing}
        <label class="sc-label" for="distill-explanation">Explanation</label>
        <textarea id="distill-explanation" class="sc-textarea" rows="3" bind:value={editExplanation}></textarea>
        <label class="sc-label" for="distill-actions">Actions (JSON)</label>
        <textarea id="distill-actions" class="sc-textarea sc-code" rows="8" bind:value={editActionsJson}></textarea>
        {#if editError}
          <p class="sc-error">{editError}</p>
        {/if}
        <div class="sc-actions">
          <button class="ghost" type="button" onclick={() => (editing = false)}>Cancel edit</button>
          <button type="button" onclick={applyEdit}><Check size={14} /> Apply edits</button>
        </div>
      {:else}
        <div class="sc-excerpt">{plan?.humanExplanation || seedExplanation}</div>
        {#if (plan?.groundingNotes ?? []).length}
          <ul class="sc-notes">
            {#each plan?.groundingNotes ?? [] as note, ni (ni)}
              <li>{note}</li>
            {/each}
          </ul>
        {/if}
        {#if validationErrors.length}
          <ul class="sc-validation err">
            {#each validationErrors as err (err)}
              <li>{err}</li>
            {/each}
          </ul>
        {/if}
        {#if validationWarnings.length}
          <ul class="sc-validation warn">
            {#each validationWarnings as w (w)}
              <li>{w}</li>
            {/each}
          </ul>
        {/if}
        {#if (plan?.actions ?? []).length}
          <ul class="sc-actions-list">
            {#each plan?.actions ?? [] as a, i (i)}
              <li>
                <div class="sc-action-top">
                  <code>{actionLabel(a)}</code>
                  {#if a.risk}<span class="sc-risk">{a.risk}</span>{/if}
                </div>
                {#if a.reason}
                  <span class="sc-reason">{a.reason}</span>
                {/if}
              </li>
            {/each}
          </ul>
        {:else}
          <p class="sc-muted">No structured actions — explanation only will be shared.</p>
        {/if}
        {#if shareError}
          <p class="sc-error">{shareError}</p>
        {/if}
        <div class="sc-actions">
          <button class="ghost" type="button" onclick={() => ondismiss?.()}>
            <X size={14} /> Not now
          </button>
          <button class="ghost" type="button" onclick={startEdit}>
            <Pencil size={14} /> Edit
          </button>
          <button type="button" disabled={!canConfirm} onclick={onConfirm}>
            <Check size={14} /> {confirmBusy || shareBusy ? "Sharing…" : "Confirm & share"}
          </button>
        </div>
      {/if}
    {/if}

    {#if loading}
      <div class="sc-actions">
        <button class="ghost" type="button" onclick={() => ondismiss?.()}>Not now</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .sc-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 220;
    backdrop-filter: blur(8px);
  }
  .sc-dialog {
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-xl);
    padding: 28px;
    width: min(520px, 94vw);
    text-align: center;
    box-shadow: var(--shadow-lg);
    max-height: min(88vh, 720px);
    overflow: auto;
  }
  .sc-icon {
    margin-bottom: 12px;
    color: var(--accent-primary);
  }
  .sc-dialog h3 {
    font-size: 18px;
    margin-bottom: 8px;
    color: var(--text-primary);
  }
  .sc-lead,
  .sc-status,
  .sc-meta,
  .sc-muted {
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.5;
    margin-bottom: 12px;
  }
  .sc-error {
    color: #fca5a5;
    font-size: 13px;
    margin-bottom: 12px;
  }
  .sc-excerpt {
    text-align: left;
    background: var(--bg-elevated);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    padding: 12px 14px;
    font-size: 13px;
    color: var(--text-primary);
    margin-bottom: 12px;
    line-height: 1.45;
  }
  .sc-actions-list {
    list-style: none;
    padding: 0;
    margin: 0 0 14px;
    text-align: left;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .sc-actions-list li {
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    padding: 8px 10px;
  }
  .sc-actions-list code {
    font-size: 12px;
    color: #fdba74;
  }
  .sc-action-top {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .sc-risk {
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    padding: 1px 6px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    color: var(--text-muted);
  }
  .sc-notes {
    list-style: disc;
    padding-left: 18px;
    margin: 0 0 12px;
    text-align: left;
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.45;
  }
  .sc-notes li { margin-bottom: 4px; }
  .sc-reason {
    display: block;
    margin-top: 4px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .sc-validation {
    list-style: none;
    padding: 0;
    margin: 0 0 12px;
    text-align: left;
    font-size: 12px;
  }
  .sc-validation.err li {
    color: #fca5a5;
  }
  .sc-validation.warn li {
    color: #fcd34d;
  }
  .sc-label {
    display: block;
    text-align: left;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    margin: 8px 0 4px;
  }
  .sc-textarea {
    width: 100%;
    box-sizing: border-box;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
    color: var(--text-primary);
    padding: 8px 10px;
    font-size: 13px;
    resize: vertical;
  }
  .sc-code {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
  }
  .sc-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    justify-content: center;
    margin-top: 16px;
  }
  .sc-actions button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
</style>
