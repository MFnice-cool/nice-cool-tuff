<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { User } from "@supabase/supabase-js";
  import {
    Vote,
    ThumbsUp,
    ThumbsDown,
    RefreshCw,
    Shield,
    Package,
    Bug,
    ChevronDown,
    ChevronUp,
    AlertTriangle,
    Wrench,
    Loader2,
    Filter,
    LogIn,
    LogOut,
    UserPlus,
  } from "@lucide/svelte";
  import { projectPath } from "../lib/store";
  import { toasts } from "../lib/toast";
  import {
    getAuthSnapshot,
    signInWithEmail,
    signOut,
    signUpWithEmail,
    supabase,
  } from "../lib/supabaseAuth";
  import EmptyState from "./EmptyState.svelte";
  import KudosBalanceStrip from "./KudosBalanceStrip.svelte";

  type StatusFilter = "all" | "open" | "saved";

  type CapsuleAction = {
    op: string;
    modId?: string | null;
    reason?: string | null;
    risk?: string | null;
  };

  type CommunityCapsule = {
    id: string;
    contentHash: string;
    fingerprintKey: string;
    exception?: string | null;
    frames: string[];
    modFile?: string | null;
    mcMajor?: string | null;
    loader?: string | null;
    solution: string;
    actions: CapsuleAction[];
    involvedMods: string[];
    status: string;
    trustScore: number;
    trustPercent: number;
    confirmCount: number;
    rejectCount: number;
    successCount: number;
    failCount: number;
    createdAt?: string | null;
    updatedAt?: string | null;
    signerPublicKey?: string | null;
    authorTotalKudos?: number | null;
    authorRac?: number | null;
  };

  type VoteResponse = {
    contentHash?: string;
    status?: string;
    trustScore?: number;
    confirmCount?: number;
    rejectCount?: number;
    successCount?: number;
    failCount?: number;
    kudos?: {
      ok?: boolean;
      awarded?: boolean;
      amount?: number;
      totalKudos?: number;
      rac?: number;
    } | null;
  };

  type KudosBalance = {
    beneficiaryKey?: string;
    totalKudos?: number;
    rac?: number;
  };

  let swarmEnabled = $state(false);
  let loading = $state(false);
  let votingHash = $state<string | null>(null);
  let proposingHash = $state<string | null>(null);
  let error = $state("");
  let capsules = $state<CommunityCapsule[]>([]);
  let statusFilter = $state<StatusFilter>("all");
  let expandedId = $state<string | null>(null);
  let kudosBalance = $state<KudosBalance | null>(null);

  let authUser = $state<User | null>(null);
  let accessToken = $state("");
  let authMode = $state<"signin" | "signup">("signin");
  let authEmail = $state("");
  let authPassword = $state("");
  let authBusy = $state(false);
  let authError = $state("");
  let authNote = $state("");

  let unsubAuth: (() => void) | null = null;

  const expanded = $derived(capsules.find((c) => c.id === expandedId || c.contentHash === expandedId) ?? null);
  const canVote = $derived(swarmEnabled && !!accessToken && !!authUser);

  onMount(() => {
    void init();
    const { data } = supabase.auth.onAuthStateChange((_event, session) => {
      authUser = session?.user ?? null;
      accessToken = session?.access_token ?? "";
    });
    unsubAuth = () => data.subscription.unsubscribe();
  });

  onDestroy(() => {
    unsubAuth?.();
  });

  async function init() {
    await loadSwarm();
    await loadAuth();
    if (swarmEnabled) {
      await refresh();
      await loadKudos();
    }
  }

  async function loadKudos() {
    if (!swarmEnabled) {
      kudosBalance = null;
      return;
    }
    try {
      kudosBalance = await invoke<KudosBalance>("get_local_kudos_balance");
    } catch {
      kudosBalance = null;
    }
  }

  async function loadAuth() {
    try {
      const snap = await getAuthSnapshot();
      authUser = snap.user;
      accessToken = snap.session?.access_token ?? "";
    } catch {
      authUser = null;
      accessToken = "";
    }
  }

  async function loadSwarm() {
    try {
      const s = await invoke<{ enabled?: boolean }>("get_swarm_settings");
      swarmEnabled = !!s?.enabled;
    } catch {
      swarmEnabled = false;
    }
  }

  async function refresh() {
    if (!swarmEnabled) return;
    loading = true;
    error = "";
    try {
      capsules = await invoke<CommunityCapsule[]>("list_community_crash_capsules", {
        status: statusFilter,
        limit: 48,
      });
      if (
        expandedId &&
        !capsules.some((c) => c.id === expandedId || c.contentHash === expandedId)
      ) {
        expandedId = null;
      }
      await loadKudos();
    } catch (e) {
      error = String(e);
      capsules = [];
    } finally {
      loading = false;
    }
  }

  function setFilter(next: StatusFilter) {
    if (statusFilter === next) return;
    statusFilter = next;
    void refresh();
  }

  function toggleExpand(c: CommunityCapsule) {
    const key = c.id || c.contentHash;
    expandedId = expandedId === key ? null : key;
  }

  function truncate(text: string, max = 120): string {
    const t = text.trim();
    if (t.length <= max) return t;
    return t.slice(0, max - 1) + "…";
  }

  function statusClass(status: string): string {
    if (status === "saved" || status === "active") return "saved";
    if (status === "rejected" || status === "quarantined") return "rejected";
    return "open";
  }

  function statusLabel(status: string): string {
    if (status === "active") return "saved";
    if (status === "pending") return "open";
    if (status === "quarantined") return "rejected";
    return status || "open";
  }

  function patchFromVote(contentHash: string, res: VoteResponse | null | undefined) {
    if (!res || typeof res !== "object") return false;
    const idx = capsules.findIndex((c) => c.contentHash === contentHash);
    if (idx < 0) return false;
    const cur = capsules[idx];
    const trustScore =
      typeof res.trustScore === "number" ? res.trustScore : cur.trustScore;
    const trustPercent = Math.round(Math.max(0, Math.min(1, trustScore)) * 100);
    const next: CommunityCapsule = {
      ...cur,
      status: typeof res.status === "string" ? res.status : cur.status,
      trustScore,
      trustPercent:
        typeof res.trustScore === "number" ? trustPercent : cur.trustPercent,
      confirmCount:
        typeof res.confirmCount === "number" ? res.confirmCount : cur.confirmCount,
      rejectCount:
        typeof res.rejectCount === "number" ? res.rejectCount : cur.rejectCount,
      successCount:
        typeof res.successCount === "number" ? res.successCount : cur.successCount,
      failCount: typeof res.failCount === "number" ? res.failCount : cur.failCount,
    };
    capsules = capsules.map((c, i) => (i === idx ? next : c));
    return true;
  }

  async function submitAuth() {
    if (authBusy) return;
    authBusy = true;
    authError = "";
    authNote = "";
    try {
      if (authMode === "signup") {
        const data = await signUpWithEmail(authEmail, authPassword);
        if (data.session) {
          authUser = data.session.user;
          accessToken = data.session.access_token;
          toasts.success("Registered and signed in");
        } else {
          authNote =
            "Check your email to confirm, then sign in here. If the link fails to open, the account is still confirmed.";
          toasts.success("Check your email to confirm registration");
          authMode = "signin";
        }
      } else {
        const data = await signInWithEmail(authEmail, authPassword);
        authUser = data.session?.user ?? null;
        accessToken = data.session?.access_token ?? "";
        toasts.success("Signed in");
      }
      authPassword = "";
    } catch (e) {
      authError = e instanceof Error ? e.message : String(e);
      toasts.error(authError);
    } finally {
      authBusy = false;
    }
  }

  async function handleSignOut() {
    try {
      await signOut();
      authUser = null;
      accessToken = "";
      toasts.success("Signed out");
    } catch (e) {
      toasts.error(e instanceof Error ? e.message : String(e));
    }
  }

  async function vote(c: CommunityCapsule, voteKind: "confirm" | "reject") {
    if (!swarmEnabled || votingHash) return;
    if (!canVote) {
      toasts.warning("Register and sign in to vote");
      return;
    }
    votingHash = c.contentHash;
    error = "";
    try {
      const res = await invoke<VoteResponse>("vote_community_crash_capsule", {
        contentHash: c.contentHash,
        vote: voteKind,
        accessToken,
      });
      const patched = patchFromVote(c.contentHash, res);
      if (!patched) await refresh();
      if (voteKind === "confirm" && res.kudos?.awarded) {
        toasts.success(
          `Voted Keep — author +${res.kudos.amount ?? 10} Kudos (RAC ${Number(res.kudos.rac ?? 0).toFixed(1)})`,
        );
        void loadKudos();
        // Refresh author totals on the voted card without a full list round-trip.
        capsules = capsules.map((cap) =>
          cap.contentHash === c.contentHash
            ? {
                ...cap,
                authorTotalKudos: Number(res.kudos?.totalKudos ?? cap.authorTotalKudos ?? 0),
                authorRac: Number(res.kudos?.rac ?? cap.authorRac ?? 0),
              }
            : cap,
        );
      } else {
        toasts.success(voteKind === "confirm" ? "Voted Keep" : "Voted Discard");
      }
    } catch (e) {
      error = String(e);
      toasts.error(String(e));
    } finally {
      votingHash = null;
    }
  }

  async function tryFix(c: CommunityCapsule) {
    if (!swarmEnabled) {
      toasts.warning("Enable TuffSwarm network in Settings to apply community fixes.");
      return;
    }
    if (!$projectPath) {
      toasts.warning("Open a project first to propose this fix.");
      return;
    }
    if (proposingHash) return;
    proposingHash = c.contentHash;
    error = "";
    try {
      await invoke("propose_community_capsule_plan", {
        path: $projectPath,
        contentHash: c.contentHash,
        solution: c.solution,
        actions: c.actions ?? [],
        matchedId: c.id || null,
        trustMeta: `trust:${c.trustPercent}|keeps:${c.confirmCount}|discards:${c.rejectCount}|mc:${c.mcMajor ?? "-"}|loader:${c.loader ?? "-"}`,
      });
      toasts.success("Pending plan written — open Diagnostics to confirm apply");
      window.dispatchEvent(new CustomEvent("tuffbox:open-diagnostics"));
    } catch (e) {
      error = String(e);
      toasts.error(String(e));
    } finally {
      proposingHash = null;
    }
  }
</script>

<div class="crash-votes fade-slide-in">
  <header class="page-head slim">
    <div class="title-row">
      <Vote size={20} />
      <h1>Crash Votes</h1>
      <button
        type="button"
        class="privacy-tip"
        title="Raw crash logs are not shared — only signatures and fix plans. Voting requires a TuffSwarm account."
        aria-label="Privacy: signatures and fix plans only, not raw logs"
      >
        <Shield size={15} />
      </button>
    </div>

    <div class="toolbar">
      <div class="filters" role="group" aria-label="Status filter">
        <Filter size={14} class="filter-icon" />
        <button
          class="chip"
          class:active={statusFilter === "all"}
          onclick={() => setFilter("all")}
          disabled={!swarmEnabled || loading}
        >
          All
        </button>
        <button
          class="chip"
          class:active={statusFilter === "open"}
          onclick={() => setFilter("open")}
          disabled={!swarmEnabled || loading}
        >
          Open
        </button>
        <button
          class="chip"
          class:active={statusFilter === "saved"}
          onclick={() => setFilter("saved")}
          disabled={!swarmEnabled || loading}
        >
          Saved
        </button>
      </div>
      <button
        class="ghost refresh-btn"
        disabled={!swarmEnabled || loading}
        onclick={() => refresh()}
        title="Refresh capsules"
      >
        <span class:spin={loading} style="display:inline-flex">
          <RefreshCw size={14} />
        </span>
        Refresh
      </button>
    </div>
  </header>

  {#if swarmEnabled && kudosBalance}
    <KudosBalanceStrip
      title="Your author Kudos"
      total={Number(kudosBalance.totalKudos ?? 0)}
      rac={Number(kudosBalance.rac ?? 0)}
      hint="Manual Keep awards author Kudos · Soft-verify playtime also votes Keep when signed in"
    />
  {/if}

  {#if swarmEnabled}
    <section class="auth-panel compact tb-card" aria-label="Account">
      {#if authUser}
        <span class="auth-email-inline">{authUser.email}</span>
        <button class="secondary mini" disabled={authBusy} onclick={() => handleSignOut()}>
          <LogOut size={13} /> Sign out
        </button>
      {:else}
        <div class="auth-tabs" role="tablist">
          <button
            class="chip"
            class:active={authMode === "signin"}
            onclick={() => (authMode = "signin")}
            disabled={authBusy}
          >
            <LogIn size={13} /> Sign in
          </button>
          <button
            class="chip"
            class:active={authMode === "signup"}
            onclick={() => (authMode = "signup")}
            disabled={authBusy}
          >
            <UserPlus size={13} /> Register
          </button>
        </div>
        <form class="auth-form compact" onsubmit={(e) => { e.preventDefault(); void submitAuth(); }}>
          <input type="email" bind:value={authEmail} placeholder="Email" autocomplete="username" required disabled={authBusy} />
          <input
            type="password"
            bind:value={authPassword}
            placeholder="Password"
            autocomplete={authMode === "signup" ? "new-password" : "current-password"}
            minlength="6"
            required
            disabled={authBusy}
          />
          <button type="submit" class="mini" disabled={authBusy || !authEmail.trim() || authPassword.length < 6}>
            {#if authBusy}
              <Loader2 size={14} class="spin" />
            {:else if authMode === "signup"}
              <UserPlus size={14} />
            {:else}
              <LogIn size={14} />
            {/if}
            {authMode === "signup" ? "Register" : "Sign in"}
          </button>
        </form>
        {#if authError}
          <p class="auth-msg err">{authError}</p>
        {/if}
        {#if authNote}
          <p class="auth-msg">{authNote}</p>
        {/if}
      {/if}
    </section>
  {/if}

  {#if !swarmEnabled}
    <div class="gate tb-card">
      <AlertTriangle size={18} />
      <div>
        <strong>TuffSwarm is off</strong>
        <p>
          Enable <strong>Use TuffSwarm network</strong> in Settings to browse and vote on
          community crash capsules.
        </p>
      </div>
    </div>
  {:else if loading && capsules.length === 0}
    <div class="state loading-state">
      <Loader2 size={28} class="spin" />
      <p>Loading community capsules…</p>
    </div>
  {:else if error && capsules.length === 0}
    <div class="state error-state tb-card">
      <AlertTriangle size={20} />
      <p>{error}</p>
      <button class="secondary" onclick={() => refresh()}>Retry</button>
    </div>
  {:else if capsules.length === 0}
    <EmptyState
      icon={Bug}
      title="No capsules yet"
      description="When the community shares crash fix plans, they appear here for voting."
      actionLabel="Refresh"
      onaction={() => refresh()}
    />
  {:else}
    {#if error}
      <div class="inline-err">{error}</div>
    {/if}

    <div class="capsule-grid">
      {#each capsules as c (c.contentHash)}
        {@const isOpen = expandedId === c.id || expandedId === c.contentHash}
        <article class="capsule-tile tb-card" class:selected={isOpen}>
          <button
            type="button"
            class="tile-hit"
            onclick={() => toggleExpand(c)}
            aria-expanded={isOpen}
          >
            <div class="tile-top">
              <span class="status-chip {statusClass(c.status)}">
                {statusLabel(c.status)}
              </span>
              <span class="meta-bits">
                {#if c.mcMajor}<span>{c.mcMajor}</span>{/if}
                {#if c.loader}<span>{c.loader}</span>{/if}
              </span>
              <span class="chevron">
                {#if isOpen}
                  <ChevronUp size={16} />
                {:else}
                  <ChevronDown size={16} />
                {/if}
              </span>
            </div>

            <div class="exception-line">
              <Bug size={14} />
              <span>{c.exception || truncate(c.fingerprintKey, 80) || "Unknown signature"}</span>
            </div>

            <span class="solution-preview">{truncate(c.solution, 140)}</span>

            {#if c.signerPublicKey}
              <div class="author-kudos" title="Author Kudos / RAC (soft-verify Keep)">
                Author Kudos <strong>{Number(c.authorTotalKudos ?? 0).toFixed(0)}</strong>
                · RAC <strong>{Number(c.authorRac ?? 0).toFixed(1)}</strong>
              </div>
            {/if}

            <div class="trust-block">
              <div class="trust-head">
                <Shield size={13} />
                <strong>{c.trustPercent}%</strong>
                <span class="trust-label">trust</span>
                <span class="vote-counts">
                  <ThumbsUp size={12} /> {c.confirmCount}
                  <ThumbsDown size={12} /> {c.rejectCount}
                  · checked {c.confirmCount + c.rejectCount}
                </span>
              </div>
              <div class="trust-meter" role="meter" aria-valuenow={c.trustPercent} aria-valuemin={0} aria-valuemax={100}>
                <div class="trust-fill" style="width: {Math.max(0, Math.min(100, c.trustPercent))}%"></div>
              </div>
            </div>

            {#if c.involvedMods?.length}
              <div class="mod-row">
                <Package size={12} />
                <span>{c.involvedMods.slice(0, 3).join(", ")}{c.involvedMods.length > 3 ? ` +${c.involvedMods.length - 3}` : ""}</span>
              </div>
            {/if}
          </button>

          <div class="tile-actions">
            <button
              class="vote keep"
              disabled={!!votingHash || !canVote}
              onclick={() => vote(c, "confirm")}
              title={canVote ? "Keep — this fix helped" : "Sign in to vote"}
            >
              <ThumbsUp size={14} />
              {votingHash === c.contentHash ? "…" : "Keep"}
            </button>
            <button
              class="vote discard secondary"
              disabled={!!votingHash || !canVote}
              onclick={() => vote(c, "reject")}
              title={canVote ? "Discard — this fix is wrong or harmful" : "Sign in to vote"}
            >
              <ThumbsDown size={14} />
              Discard
            </button>
          </div>
        </article>
      {/each}
    </div>

    {#if expanded}
      <section class="detail-drawer tb-card" aria-label="Capsule details">
        <div class="drawer-head">
          <div>
            <span class="status-chip {statusClass(expanded.status)}">
              {statusLabel(expanded.status)}
            </span>
            <h2>{expanded.exception || "Crash signature"}</h2>
            <p class="fp-key" title={expanded.fingerprintKey}>{expanded.fingerprintKey}</p>
          </div>
          <button class="ghost" onclick={() => (expandedId = null)}>Close</button>
        </div>

        <div class="drawer-grid">
          <div class="panel">
            <h3><Bug size={14} /> Crash signature</h3>
            <p class="panel-note">Exception + stack frames (not raw logs).</p>
            {#if expanded.exception}
              <pre class="sig-block">{expanded.exception}</pre>
            {/if}
            {#if expanded.frames?.length}
              <ol class="frames">
                {#each expanded.frames as frame, i (i + ":" + frame)}
                  <li><code>{frame}</code></li>
                {/each}
              </ol>
            {:else}
              <p class="muted">No frame summary in this capsule.</p>
            {/if}
          </div>

          <div class="panel">
            <h3><Package size={14} /> Involved mods</h3>
            {#if expanded.involvedMods?.length}
              <ul class="mod-list">
                {#each expanded.involvedMods as mod (mod)}
                  <li>{mod}</li>
                {/each}
              </ul>
            {:else}
              <p class="muted">No mods listed.</p>
            {/if}
          </div>

          <div class="panel span-2">
            <h3><Wrench size={14} /> Solution</h3>
            <p class="solution-text">{expanded.solution}</p>
          </div>

          <div class="panel span-2">
            <h3>Actions</h3>
            {#if expanded.actions?.length}
              <ul class="action-list">
                {#each expanded.actions as a, i (a.op + "-" + (a.modId ?? "") + "-" + i)}
                  {@const kind =
                    a.op?.includes("disable") || a.op?.includes("remove")
                      ? "remove"
                      : a.op?.includes("edit") || a.op?.includes("update") || a.op?.includes("change")
                        ? "change"
                        : "add"}
                  <li>
                    <span class="diff-chip {kind}">
                      {kind === "add" ? "+" : kind === "remove" ? "−" : "~"}
                    </span>
                    <code class="op">{a.op}</code>
                    {#if a.modId}<span class="mod-id">{a.modId}</span>{/if}
                    {#if a.reason}<span class="reason">{a.reason}</span>{/if}
                    {#if a.risk}<span class="risk" class:high={a.risk === "high"}>{a.risk}</span>{/if}
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="muted">No structured actions.</p>
            {/if}
          </div>
        </div>

        <div class="drawer-trust">
          <div class="trust-block large">
            <div class="trust-head">
              <Shield size={16} />
              <strong>{expanded.trustPercent}%</strong>
              <span class="trust-label">trust</span>
              <span class="vote-counts">
                <ThumbsUp size={13} /> {expanded.confirmCount} keep ·
                <ThumbsDown size={13} /> {expanded.rejectCount} discard
                · success {expanded.successCount} / fail {expanded.failCount}
              </span>
            </div>
            <div class="trust-meter tall" role="meter" aria-valuenow={expanded.trustPercent} aria-valuemin={0} aria-valuemax={100}>
              <div class="trust-fill" style="width: {Math.max(0, Math.min(100, expanded.trustPercent))}%"></div>
            </div>
          </div>
        </div>

        <div class="drawer-footer">
          {#if !swarmEnabled}
            <div class="hint"><AlertTriangle size={14} /> Enable TuffSwarm to apply fixes.</div>
          {:else if !$projectPath}
            <div class="hint"><AlertTriangle size={14} /> Open a project to try this fix on an instance.</div>
          {/if}
          <div class="footer-actions">
            {#if !canVote}
              <div class="hint"><AlertTriangle size={14} /> Sign in above to Keep or Discard.</div>
            {/if}
            <button
              class="vote keep"
              disabled={!!votingHash || !canVote}
              onclick={() => expanded && vote(expanded, "confirm")}
            >
              <ThumbsUp size={14} /> Keep
            </button>
            <button
              class="vote discard secondary"
              disabled={!!votingHash || !canVote}
              onclick={() => expanded && vote(expanded, "reject")}
            >
              <ThumbsDown size={14} /> Discard
            </button>
            <button
              disabled={!!proposingHash || !swarmEnabled || !$projectPath}
              title={!$projectPath
                ? "Open a project first"
                : !swarmEnabled
                  ? "Enable TuffSwarm in Settings"
                  : "Write pending plan for Diagnostics"}
              onclick={() => expanded && tryFix(expanded)}
            >
              <Wrench size={14} />
              {proposingHash === expanded.contentHash ? "Proposing…" : "Try fix on this project"}
            </button>
          </div>
        </div>
      </section>
    {/if}
  {/if}
</div>

<style>
  .crash-votes {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 0 2px 32px;
    max-width: 1400px;
  }

  .page-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .page-head.slim {
    padding-bottom: 4px;
  }

  .author-kudos {
    margin-top: 6px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .author-kudos strong {
    color: var(--text-secondary);
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--accent-primary);
  }

  .title-row h1 {
    margin: 0;
    font-size: 20px;
    font-weight: 800;
    color: var(--text-primary);
    letter-spacing: -0.02em;
  }

  .privacy-tip {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 4px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    border-radius: 6px;
    cursor: help;
  }

  .privacy-tip:hover {
    color: var(--accent-primary);
    background: var(--bg-hover);
  }

  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }

  .filters {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    padding: 4px 6px 4px 10px;
  }

  .filters :global(.filter-icon) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .chip {
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 700;
    background: transparent;
    color: var(--text-secondary);
    border: none;
    border-radius: var(--border-radius-sm);
  }

  .chip:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
    transform: none;
  }

  .chip.active {
    background: var(--bg-elevated);
    color: var(--text-primary);
    box-shadow: 0 0 0 1px var(--border-color);
  }

  .refresh-btn {
    font-size: 13px;
  }

  .gate {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 16px 18px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
    color: var(--accent-warning);
  }

  .gate strong {
    display: block;
    color: var(--text-primary);
    margin-bottom: 4px;
  }

  .gate p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 13px;
  }

  .state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 48px 24px;
    color: var(--text-muted);
  }

  .error-state {
    background: var(--bg-secondary);
    border: 1px solid color-mix(in srgb, var(--accent-danger) 35%, transparent);
    border-radius: var(--border-radius-lg);
    color: var(--accent-danger);
    text-align: center;
  }

  .inline-err {
    color: var(--accent-danger);
    font-size: 13px;
    padding: 8px 12px;
    background: color-mix(in srgb, var(--accent-danger) 8%, transparent);
    border-radius: var(--border-radius-sm);
  }

  .capsule-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 14px;
  }

  .capsule-tile {
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
    overflow: hidden;
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
  }

  .capsule-tile:hover {
    border-color: var(--bg-active);
  }

  .capsule-tile.selected {
    border-color: var(--accent-primary);
    box-shadow: 0 0 0 1px var(--accent-primary);
  }

  .tile-hit {
    display: flex;
    flex-direction: column;
    gap: 10px;
    align-items: stretch;
    text-align: left;
    padding: 14px 14px 10px;
    background: transparent;
    color: inherit;
    border: none;
    border-radius: 0;
    font-weight: 400;
    cursor: pointer;
    width: 100%;
  }

  .tile-hit:hover {
    background: var(--bg-tertiary);
    transform: none;
  }

  .tile-top {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .status-chip {
    display: inline-flex;
    align-items: center;
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 800;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    background: var(--bg-elevated);
    color: var(--text-muted);
  }

  .status-chip.saved {
    background: color-mix(in srgb, var(--accent-primary) 14%, transparent);
    color: var(--accent-primary);
  }

  .status-chip.open {
    background: color-mix(in srgb, var(--accent-warning) 14%, transparent);
    color: var(--accent-warning);
  }

  .status-chip.rejected {
    background: color-mix(in srgb, var(--accent-danger) 14%, transparent);
    color: var(--accent-danger, #ef4444);
  }

  .auth-panel {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px 16px;
  }

  .auth-panel.compact {
    flex-direction: row;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
  }

  .auth-email-inline {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .auth-tabs {
    display: flex;
    gap: 8px;
  }

  .auth-form {
    display: grid;
    grid-template-columns: 1fr 1fr auto;
    gap: 10px;
    align-items: end;
  }

  .auth-form.compact {
    flex: 1;
    min-width: min(100%, 420px);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .auth-form.compact input {
    flex: 1;
    min-width: 120px;
  }

  .auth-msg {
    margin: 0;
    font-size: 13px;
    color: var(--text-muted);
  }

  .auth-msg.err {
    color: var(--accent-danger, #ef4444);
  }

  @media (max-width: 820px) {
    .auth-form,
    .auth-form.compact {
      grid-template-columns: 1fr;
      flex-direction: column;
      align-items: stretch;
      width: 100%;
    }
  }

  .meta-bits {
    display: flex;
    gap: 8px;
    flex: 1;
    min-width: 0;
    color: var(--text-muted);
    font-size: 12px;
  }

  .meta-bits span {
    background: var(--bg-elevated);
    padding: 2px 6px;
    border-radius: 6px;
  }

  .chevron {
    color: var(--text-muted);
    display: inline-flex;
  }

  .exception-line {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    font-size: 13px;
    font-weight: 700;
    color: var(--text-primary);
    line-height: 1.35;
  }

  .exception-line :global(svg) {
    flex-shrink: 0;
    margin-top: 2px;
    color: var(--accent-danger);
  }

  .solution-preview {
    margin: 0;
    font-size: 12px;
    color: var(--text-secondary);
    line-height: 1.45;
    min-height: 2.9em;
    display: block;
  }

  .trust-block {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .trust-head {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .trust-head strong {
    color: var(--accent-primary);
    font-size: 15px;
    font-weight: 800;
  }

  .trust-label {
    color: var(--text-muted);
    text-transform: uppercase;
    font-size: 12px;
    letter-spacing: 0.06em;
    font-weight: 700;
  }

  .vote-counts {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--text-muted);
    font-size: 12px;
  }

  .trust-meter {
    height: 6px;
    border-radius: 999px;
    background: var(--bg-elevated);
    overflow: hidden;
  }

  .trust-meter.tall {
    height: 10px;
  }

  .trust-fill {
    height: 100%;
    border-radius: inherit;
    background: linear-gradient(90deg, var(--accent-primary), var(--accent-hover));
    transition: width 0.25s ease;
  }

  .mod-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .tile-actions {
    display: flex;
    gap: 8px;
    padding: 0 14px 14px;
  }

  .vote {
    flex: 1;
    font-size: 12px;
    padding: 8px 10px;
  }

  .vote.keep {
    background: color-mix(in srgb, var(--accent-primary) 16%, transparent);
    color: var(--accent-primary);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }

  .vote.keep:hover:not(:disabled) {
    background: var(--accent-primary);
    color: var(--on-accent, #000);
  }

  .vote.discard {
    color: var(--text-secondary);
  }

  button:disabled {
    opacity: 0.55;
    cursor: not-allowed;
    transform: none;
  }

  .detail-drawer {
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .drawer-head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 12px;
  }

  .drawer-head h2 {
    margin: 8px 0 4px;
    font-size: 18px;
    font-weight: 800;
  }

  .fp-key {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
    font-family: var(--font-mono, ui-monospace, monospace);
    word-break: break-all;
  }

  .drawer-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }

  .panel {
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    padding: 12px 14px;
  }

  .panel.span-2 {
    grid-column: 1 / -1;
  }

  .panel h3 {
    margin: 0 0 8px;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .panel-note {
    margin: -4px 0 8px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .sig-block {
    margin: 0 0 10px;
    padding: 10px;
    background: var(--bg-elevated);
    border-radius: var(--border-radius-sm);
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--text-primary);
  }

  .frames {
    margin: 0;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .frames code {
    font-size: 12px;
    color: var(--text-secondary);
    word-break: break-all;
  }

  .mod-list,
  .action-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .mod-list li {
    font-size: 13px;
    padding: 6px 8px;
    background: var(--bg-elevated);
    border-radius: var(--border-radius-sm);
  }

  .action-list li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    padding: 6px 8px;
    background: var(--bg-elevated);
    border-radius: var(--border-radius-sm);
  }

  .diff-chip {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    padding: 0 5px;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 800;
    border: 1px solid var(--border-color);
  }
  .diff-chip.add { color: var(--accent-primary); border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent); background: color-mix(in srgb, var(--accent-primary) 10%, transparent); }
  .diff-chip.remove { color: var(--accent-danger); border-color: color-mix(in srgb, var(--accent-danger) 35%, transparent); background: color-mix(in srgb, var(--accent-danger) 10%, transparent); }
  .diff-chip.change { color: var(--accent-warning); border-color: color-mix(in srgb, var(--accent-warning) 35%, transparent); background: color-mix(in srgb, var(--accent-warning) 10%, transparent); }

  .op {
    color: var(--accent-primary);
    font-weight: 700;
  }

  .mod-id {
    color: var(--text-primary);
  }

  .reason {
    color: var(--text-muted);
    flex: 1;
    min-width: 120px;
  }

  .risk {
    font-size: 12px;
    font-weight: 800;
    text-transform: uppercase;
    color: var(--text-muted);
    padding: 2px 6px;
    border-radius: 6px;
    background: var(--bg-secondary);
  }

  .risk.high {
    color: var(--accent-danger);
    background: color-mix(in srgb, var(--accent-danger) 12%, transparent);
  }

  .solution-text {
    margin: 0;
    font-size: 14px;
    line-height: 1.55;
    color: var(--text-primary);
    white-space: pre-wrap;
  }

  .muted {
    color: var(--text-muted);
    font-size: 12px;
    margin: 0;
  }

  .drawer-footer {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding-top: 4px;
    border-top: 1px solid var(--border-color);
  }

  .hint {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--accent-warning);
  }

  .footer-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-left: auto;
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (max-width: 720px) {
    .drawer-grid {
      grid-template-columns: 1fr;
    }

    .panel.span-2 {
      grid-column: auto;
    }
  }
</style>
