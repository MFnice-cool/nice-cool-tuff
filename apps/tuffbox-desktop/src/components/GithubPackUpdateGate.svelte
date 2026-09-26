<script lang="ts">
  import { RefreshCw, ShieldAlert, Rocket, X } from "@lucide/svelte";
  import { onDestroy, onMount } from "svelte";
  import { api } from "../lib/api";
  import { trapFocus } from "../lib/focusTrap";

  /**
   * "Update to play" gate (Modrinth-style): when the user presses Play on a
   * GitHub-transported pack that has a pending update, this modal blocks the
   * launch until they review the diff and apply the update (or cancel).
   */
  let {
    targetPath = "",
    oncontinue = () => {},
    oncancel = () => {},
  }: {
    targetPath?: string;
    /** Called after the update was applied — the caller should launch. */
    oncontinue?: () => void;
    oncancel?: () => void;
  } = $props();

  type Change = Record<string, unknown>;
  type Preview = {
    repo: string;
    installedVersion: string;
    incomingVersion: string;
    remoteCommitSha: string;
    requiresFullReinstall: boolean;
    customFiles: boolean;
    signerState?: "ok" | "changed" | "unsigned";
    changes: Change[];
  };

  let checking = $state(true);
  let applying = $state(false);
  let error = $state("");
  let noUpdate = $state(false);
  let preview = $state<Preview | null>(null);
  let reviewedDiff = $state(false);
  let confirmedCustomFiles = $state(false);
  let confirmedFullReinstall = $state(false);
  let refreshGeneration = 0;
  let destroyed = false;

  const confirmationComplete = $derived(
    preview?.signerState !== "changed" &&
      reviewedDiff &&
      (!preview?.customFiles || confirmedCustomFiles) &&
      (!preview?.requiresFullReinstall || confirmedFullReinstall),
  );

  function asObj(value: unknown): Record<string, unknown> | null {
    return value && typeof value === "object" ? (value as Record<string, unknown>) : null;
  }

  function originNote(origin: unknown): string {
    return origin === "custom" ? " · custom file" : "";
  }

  function changeLabel(change: Change): string {
    const added = asObj(change.modAdded);
    if (added) return `Added ${added.id} ${added.version}${originNote(added.origin)}`;
    const removed = asObj(change.modRemoved);
    if (removed) return `Removed ${removed.id} ${removed.version}`;
    const bumped = asObj(change.modBumped);
    if (bumped) return `Updated ${bumped.id} ${bumped.from} → ${bumped.to}${originNote(bumped.origin)}`;
    const mc = asObj(change.minecraftChanged);
    if (mc) return `Minecraft ${mc.from} → ${mc.to}`;
    const loader = asObj(change.loaderChanged);
    if (loader) return `Loader ${loader.from} → ${loader.to}`;
    if (change.overridesChanged) return "Override files changed";
    return Object.keys(change)[0] ?? "change";
  }

  async function refresh() {
    if (!targetPath) {
      checking = false;
      noUpdate = true;
      return;
    }
    const generation = ++refreshGeneration;
    checking = true;
    error = "";
    noUpdate = false;
    preview = null;
    reviewedDiff = false;
    confirmedCustomFiles = false;
    confirmedFullReinstall = false;
    try {
      const status = await api.transport.github.checkUpdate(targetPath);
      if (generation !== refreshGeneration || destroyed) return;
      if (!status.updateAvailable) {
        noUpdate = true;
        return;
      }
      preview = await api.transport.github.previewUpdate(targetPath);
    } catch (e) {
      if (generation !== refreshGeneration || destroyed) return;
      const text = String(e);
      if (text.includes("not a GitHub pack")) {
        noUpdate = true;
      } else {
        error = text;
      }
    } finally {
      if (generation === refreshGeneration && !destroyed) checking = false;
    }
  }

  async function applyAndPlay() {
    if (!targetPath || !preview || !confirmationComplete || preview.signerState === "changed") return;
    applying = true;
    error = "";
    try {
      const latest = await api.transport.github.previewUpdate(targetPath);
      if (latest.remoteCommitSha !== preview.remoteCommitSha) {
        preview = latest;
        reviewedDiff = false;
        confirmedCustomFiles = false;
        confirmedFullReinstall = false;
        error = "The GitHub pack changed after review. Review the refreshed diff before applying.";
        return;
      }
      await api.transport.github.applyUpdate(preview.remoteCommitSha, targetPath);
      oncontinue();
    } catch (e) {
      error = String(e);
    } finally {
      applying = false;
    }
  }

  onMount(() => {
    destroyed = false;
    void refresh();
  });

  onDestroy(() => {
    destroyed = true;
    refreshGeneration += 1;
  });
</script>

<div
  class="gate-backdrop"
  role="presentation"
  onclick={(e) => e.target === e.currentTarget && !applying && oncancel?.()}
>
  <div
    class="gate-dialog"
    role="alertdialog"
    aria-modal="true"
    aria-label="Update required before play"
    use:trapFocus={{ onEscape: () => !applying && oncancel?.() }}
  >
    <header class="gate-header">
      <div class="gate-title">
        <Rocket size={18} />
        <div>
          <strong>Update before play</strong>
          {#if preview}
            <span>{preview.installedVersion} → {preview.incomingVersion} · {preview.repo}</span>
          {/if}
        </div>
      </div>
      <button class="gate-close" aria-label="Close" disabled={applying} onclick={() => oncancel?.()}>
        <X size={16} />
      </button>
    </header>

    {#if checking}
      <div class="gate-status">
        <span class="gate-spin"><RefreshCw size={18} /></span>
        Checking for GitHub pack updates…
      </div>
    {:else if noUpdate}
      <div class="gate-status">
        <Rocket size={18} />
        This pack is up to date. You can play.
      </div>
      <div class="gate-actions">
        <button class="secondary" onclick={() => oncancel?.()}>Cancel</button>
        <button class="primary" onclick={() => oncontinue()}>Play now</button>
      </div>
    {:else if preview}
      {#if preview.customFiles || preview.requiresFullReinstall || preview.signerState === "changed" || preview.signerState === "unsigned"}
        <p class="gate-warn">
          <ShieldAlert size={14} />
          {#if preview.signerState === "changed"}
            Update blocked: the signing key changed. The pack author or repository owner must restore the original signer and republish before this update can be applied.
          {:else if preview.requiresFullReinstall}
            Minecraft or loader changed — full rematerialize of provider jars.
          {:else if preview.signerState === "unsigned"}
            Incoming pack is unsigned. Review the diff before applying.
          {:else}
            Includes custom files from GitHub. Review before applying.
          {/if}
        </p>
      {/if}

      <div class="gate-diff">
        <strong>Review diff (required)</strong>
        {#if (preview.changes?.length ?? 0) > 0}
          <ul>
            {#each preview.changes ?? [] as change, i (i)}
              <li>{changeLabel(change)}</li>
            {/each}
          </ul>
        {:else}
          <p>No file-level changes were reported.</p>
        {/if}
        {#if preview.signerState !== "changed"}
          <label class="gate-check">
            <input type="checkbox" bind:checked={reviewedDiff} />
            I reviewed the complete update diff.
          </label>
        {/if}
      </div>

      {#if preview.signerState !== "changed" && (preview.customFiles || preview.requiresFullReinstall)}
        <fieldset class="gate-risk">
          <legend>Explicit confirmation required</legend>
          {#if preview.customFiles}
            <label class="gate-check">
              <input type="checkbox" bind:checked={confirmedCustomFiles} />
              Apply custom files from this GitHub repository.
            </label>
          {/if}
          {#if preview.requiresFullReinstall}
            <label class="gate-check">
              <input type="checkbox" bind:checked={confirmedFullReinstall} />
              Perform a full reinstall and rematerialize provider jars.
            </label>
          {/if}
        </fieldset>
      {/if}

      <div class="gate-actions">
        <button class="secondary" disabled={applying} onclick={() => oncancel?.()}>Not now</button>
        {#if preview.signerState === "changed"}
          <button class="primary" disabled>Update blocked</button>
        {:else}
          <button class="primary" disabled={applying || !confirmationComplete} onclick={() => void applyAndPlay()}>
            {applying ? "Applying…" : preview.requiresFullReinstall ? "Reinstall & play" : "Update & play"}
          </button>
        {/if}
        <button class="ghost" disabled={applying} onclick={() => void refresh()} title="Re-check for updates">
          <RefreshCw size={14} /> Refresh
        </button>
      </div>
      {#if error}<p class="gate-err">{error}</p>{/if}
    {:else if error}
      <div class="gate-status">
        <ShieldAlert size={18} />
        {error}
      </div>
      <div class="gate-actions">
        <button class="secondary" onclick={() => oncancel?.()}>Cancel</button>
        <button class="primary" onclick={() => void refresh()}>Retry</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .gate-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 200;
    backdrop-filter: blur(8px);
  }

  .gate-dialog {
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-xl);
    padding: 20px 22px;
    width: min(520px, 92vw);
    max-height: 84vh;
    overflow-y: auto;
    box-shadow: var(--shadow-lg);
    display: grid;
    gap: 12px;
  }

  .gate-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  .gate-title {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    color: var(--accent-primary);
  }

  .gate-title div {
    display: grid;
    gap: 8px;
  }

  .gate-title strong {
    font-size: 15px;
    color: var(--text-primary);
  }

  .gate-title span {
    font-size: 12px;
    color: var(--text-muted);
  }

  .gate-close {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    border-radius: var(--border-radius-sm);
  }

  .gate-close:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .gate-close:disabled {
    opacity: 0.4;
  }

  .gate-status {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-muted);
    font-size: 13px;
    padding: 8px 0;
  }

  .gate-spin {
    display: inline-flex;
    animation: gate-spin 0.8s linear infinite;
  }

  @keyframes gate-spin {
    to { transform: rotate(360deg); }
  }

  .gate-warn {
    display: flex;
    gap: 8px;
    align-items: center;
    margin: 0;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, #fde68a 35%, var(--border-color));
    border-radius: var(--border-radius-sm);
    background: color-mix(in srgb, #fde68a 8%, transparent);
    color: #fde68a;
    font-size: 12px;
  }

  .gate-diff {
    display: grid;
    gap: 8px;
    padding: 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
  }

  .gate-diff strong {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .gate-diff ul {
    margin: 0;
    padding-left: 18px;
  }

  .gate-diff li,
  .gate-diff p {
    color: var(--text-muted);
    font-size: 12px;
    margin: 0;
  }

  .gate-risk {
    display: grid;
    gap: 8px;
    margin: 0;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, #fde68a 35%, var(--border-color));
    border-radius: var(--border-radius-sm);
  }

  .gate-risk legend {
    padding: 0 4px;
    color: #fde68a;
    font-size: 12px;
    font-weight: 700;
  }

  .gate-check {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    color: var(--text-secondary);
    font-size: 12px;
  }

  .gate-check input {
    margin: 1px 0 0;
  }

  .gate-actions {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  .gate-actions .primary {
    background: var(--accent-primary);
    color: var(--on-accent, #000);
    box-shadow: var(--play-glow);
  }

  .gate-actions .primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .gate-actions .primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .gate-err {
    margin: 0;
    color: #fecaca;
    font-size: 12px;
  }
</style>
