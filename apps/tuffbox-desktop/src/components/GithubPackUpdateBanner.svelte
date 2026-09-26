<script lang="ts">
  import { RefreshCw, ShieldAlert } from "@lucide/svelte";
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import { projectPath } from "../lib/store";

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

  let checking = $state(false);
  let applying = $state(false);
  let error = $state("");
  let updateAvailable = $state(false);
  let preview = $state<Preview | null>(null);
  let snapshotId = $state("");
  let reviewedDiff = $state(false);
  let confirmedCustomFiles = $state(false);
  let confirmedFullReinstall = $state(false);
  let checkGeneration = 0;
  let scopedPath: string | null = "";

  let confirmationComplete = $derived(
    preview?.signerState !== "changed" &&
      reviewedDiff &&
      (!preview?.customFiles || confirmedCustomFiles) &&
      (!preview?.requiresFullReinstall || confirmedFullReinstall),
  );

  function clearUpdateState() {
    updateAvailable = false;
    preview = null;
    snapshotId = "";
    reviewedDiff = false;
    confirmedCustomFiles = false;
    confirmedFullReinstall = false;
    error = "";
  }

  async function check(targetPath = $projectPath) {
    if (applying) return;
    const generation = ++checkGeneration;
    clearUpdateState();
    if (!targetPath) {
      checking = false;
      return;
    }
    checking = true;
    try {
      const status = await api.transport.github.checkUpdate(targetPath);
      if (generation !== checkGeneration || targetPath !== $projectPath) return;
      updateAvailable = !!status.updateAvailable;
      if (updateAvailable) {
        const nextPreview = await api.transport.github.previewUpdate(targetPath);
        if (generation !== checkGeneration || targetPath !== $projectPath) return;
        preview = nextPreview;
      }
    } catch (e) {
      if (generation !== checkGeneration || targetPath !== $projectPath) return;
      const text = String(e);
      if (text.includes("not a GitHub pack")) {
        clearUpdateState();
      } else {
        error = text;
      }
    } finally {
      if (generation === checkGeneration) checking = false;
    }
  }

  onMount(() => projectPath.subscribe((path) => {
    if (path !== scopedPath) {
      scopedPath = path;
      checkGeneration += 1;
      checking = false;
      clearUpdateState();
    }
    void check(path);
  }));

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

  async function apply() {
    if (!$projectPath || !preview || !confirmationComplete || preview.signerState === "changed") return;
    const targetPath = $projectPath;
    const reviewedCommitSha = preview.remoteCommitSha;
    const generation = ++checkGeneration;
    checking = false;
    applying = true;
    snapshotId = "";
    error = "";
    try {
      const latestPreview = await api.transport.github.previewUpdate(targetPath);
      if (generation !== checkGeneration || targetPath !== $projectPath) return;
      if (latestPreview.remoteCommitSha !== reviewedCommitSha) {
        preview = latestPreview;
        reviewedDiff = false;
        confirmedCustomFiles = false;
        confirmedFullReinstall = false;
        error = "The GitHub pack changed after review. Review the refreshed diff before applying.";
        return;
      }
      const result = await api.transport.github.applyUpdate(reviewedCommitSha, targetPath);
      if (generation !== checkGeneration || targetPath !== $projectPath) return;
      snapshotId = result.snapshotId;
      updateAvailable = false;
      preview = null;
    } catch (e) {
      if (generation !== checkGeneration || targetPath !== $projectPath) return;
      error = String(e);
    } finally {
      applying = false;
      if (targetPath !== $projectPath) void check($projectPath);
    }
  }
</script>

{#if updateAvailable && preview}
  <section class="update-card">
    <header>
      <RefreshCw size={16} />
      <div>
        <strong>GitHub pack update</strong>
        <span>{preview.installedVersion} → {preview.incomingVersion} · {preview.repo}</span>
      </div>
    </header>
    {#if preview.customFiles || preview.requiresFullReinstall || preview.signerState === "changed" || preview.signerState === "unsigned"}
      <p class="warn">
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
    <div class="review-diff">
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
        <label class="confirmation">
          <input type="checkbox" bind:checked={reviewedDiff} />
          I reviewed the complete update diff.
        </label>
      {/if}
    </div>
    {#if preview.signerState !== "changed" && (preview.customFiles || preview.requiresFullReinstall)}
      <fieldset class="risk-confirmations">
        <legend>Explicit confirmation required</legend>
        {#if preview.customFiles}
          <label class="confirmation">
            <input type="checkbox" bind:checked={confirmedCustomFiles} />
            Apply custom files from this GitHub repository.
          </label>
        {/if}
        {#if preview.requiresFullReinstall}
          <label class="confirmation">
            <input type="checkbox" bind:checked={confirmedFullReinstall} />
            Perform a full reinstall and rematerialize provider jars.
          </label>
        {/if}
      </fieldset>
    {/if}
    <div class="actions">
      {#if preview.signerState === "changed"}
        <button class="mini" disabled>Update blocked</button>
      {:else}
        <button class="mini" onclick={apply} disabled={applying || !confirmationComplete}>{applying ? "Applying…" : preview.requiresFullReinstall ? "Reinstall pack" : "Apply update"}</button>
      {/if}
      <button class="secondary mini" onclick={() => check()} disabled={checking || applying}>Refresh</button>
    </div>
    {#if error}<p class="err">{error}</p>{/if}
  </section>
{:else if snapshotId}
  <p class="ok">Update applied. Rollback snapshot: {snapshotId}</p>
{/if}

<style>
  .update-card {
    display: grid;
    gap: 8px;
    margin: 0 0 12px;
    padding: 12px 14px;
    border: 1px solid color-mix(in srgb, var(--accent-primary) 30%, var(--border-color));
    border-radius: var(--border-radius-md);
    background: color-mix(in srgb, var(--accent-primary) 8%, var(--bg-secondary));
  }
  header { display: flex; gap: 10px; align-items: center; }
  header span, li, .warn, .ok, .err, .review-diff p { color: var(--text-muted); font-size: 12px; }
  ul { margin: 0; padding-left: 18px; }
  .actions { display: flex; gap: 8px; }
  .warn { display: flex; gap: 8px; align-items: center; margin: 0; color: #fde68a; }
  .review-diff {
    display: grid;
    gap: 8px;
    padding: 8px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
  }
  .review-diff p { margin: 0; }
  .risk-confirmations {
    display: grid;
    gap: 8px;
    margin: 0;
    padding: 8px;
    border: 1px solid color-mix(in srgb, #fde68a 35%, var(--border-color));
    border-radius: var(--border-radius-sm);
  }
  .risk-confirmations legend { padding: 0 4px; color: #fde68a; font-size: 12px; font-weight: 700; }
  .confirmation { display: flex; gap: 8px; align-items: flex-start; color: var(--text-secondary); font-size: 12px; }
  .confirmation input { margin: 1px 0 0; }
  .ok { margin: 0 0 12px; color: var(--accent-primary); font-size: 12px; }
  .err { margin: 0; color: #fecaca; }
</style>
