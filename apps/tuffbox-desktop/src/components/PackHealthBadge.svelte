<script lang="ts">
  import { HeartPulse, AlertTriangle, CheckCircle2, XCircle } from "@lucide/svelte";
  import { onDestroy } from "svelte";
  import { api } from "../lib/api";
  import { projectPath } from "../lib/store";

  /**
   * Pack health badge: polls the aggregate get_pack_health report for the
   * active project and shows a compact verdict. Click jumps into Diagnose.
   * Best-effort by design — failures render as "unknown", never as errors.
   */
  let { onOpenDiagnostics = () => {} }: { onOpenDiagnostics?: () => void } = $props();

  type Health = {
    diagnostics: { errors: number; warnings: number };
    exportIssues: Array<{ severity: string; code: string; message: string }>;
    wrongLoaderCount: number;
    duplicateGroups: Array<{ modId: string; keepCandidate: string; count: number }>;
    questIssues: number;
    lastCrash: { at: string; exitCode: number | null } | null;
    overall: "healthy" | "warnings" | "errors";
  };

  let health = $state<Health | null>(null);
  let loading = $state(false);
  let lastPath: string | null = null;
  // `lastPath` is only updated after a successful response, so it cannot
  // prevent duplicate first-load requests. Track the in-flight project
  // separately to avoid racing the badge and Diagnose refresh paths.
  let activePath: string | null = null;
  let pollTimer: ReturnType<typeof setInterval> | undefined;

  // Re-check when the project changes; refresh every 60 s while visible.
  $effect(() => {
    const path = $projectPath;
    if (!path) {
      health = null;
      return;
    }
    void load(path);
    if (!pollTimer) {
      pollTimer = setInterval(() => {
        if (document.hidden) return; // "while visible" — a hidden window skips the scan
        const p = $projectPath;
        if (p) void load(p);
      }, 60000);
    }
  });

  async function load(path: string) {
    if (activePath === path) return;
    activePath = path;
    loading = true;
    try {
      const result = await api.diagnostics.getPackHealth(path);
      if ($projectPath === path) {
        health = result;
        lastPath = path;
      }
    } catch {
      // best-effort: keep the previous snapshot, do not alarm the user
    } finally {
      if (activePath === path) {
        activePath = null;
        loading = false;
      }
    }
  }

  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
  });

  function verdictLabel(h: Health): string {
    if (h.overall === "healthy") return "Healthy";
    if (h.overall === "warnings") return "Warnings";
    return "Errors";
  }
</script>

{#if $projectPath}
  {#if health}
    <button
      type="button"
      class="health-badge {health.overall}"
      title={`Pack health: ${verdictLabel(health)} · ${health.diagnostics.errors} error(s), ${health.diagnostics.warnings} warning(s)${
        health.wrongLoaderCount ? `, ${health.wrongLoaderCount} wrong-loader` : ""
      }${health.duplicateGroups.length ? `, ${health.duplicateGroups.length} duplicate group(s)` : ""}`}
      onclick={() => onOpenDiagnostics()}
    >
      {#if health.overall === "healthy"}
        <CheckCircle2 size={14} />
      {:else if health.overall === "errors"}
        <XCircle size={14} />
      {:else}
        <AlertTriangle size={14} />
      {/if}
      <span>{verdictLabel(health)}</span>
      <HeartPulse size={12} class="pulse" />
    </button>
  {:else}
    <div class="health-badge unknown" aria-busy="true"><HeartPulse size={14} /><span>…</span></div>
  {/if}
{/if}

<style>
  .health-badge {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 4px 12px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
    transition: border-color var(--motion-fast) ease, color var(--motion-fast) ease;
  }
  .health-badge:hover { border-color: var(--accent-primary); color: var(--text-primary); }
  .health-badge.healthy { color: var(--accent-primary); }
  .health-badge.warnings { color: #fbbf24; border-color: rgba(251, 191, 36, 0.45); }
  .health-badge.errors { color: #f87171; border-color: rgba(248, 113, 113, 0.5); }
  .health-badge.unknown { cursor: default; }
  :global(.pulse) { opacity: 0.6; }
</style>
