<script lang="ts">
  import DedupAskDialog from "./DedupAskDialog.svelte";
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open as openExternal } from "@tauri-apps/plugin-shell";
  import { Sparkles, RefreshCw, Download, AlertTriangle } from "@lucide/svelte";
  import { projectPath } from "../lib/store";
  import { toasts } from "../lib/toast";
  import { getAuthSnapshot } from "../lib/supabaseAuth";
  import CatalogProjectView from "./CatalogProjectView.svelte";
  import { trapFocus } from "../lib/focusTrap";
  import KudosBalanceStrip from "./KudosBalanceStrip.svelte";

  // Import-time dedup question (docs/17 §4) for pack installs from trends.
  let dedupAsk = $state<{ resolve: (v: boolean | null) => void; name: string } | null>(null);

  function askDedupChoice(name: string): Promise<boolean | null> {
    return new Promise((resolve) => {
      dedupAsk = {
        resolve: (v) => {
          dedupAsk = null;
          resolve(v);
        },
        name,
      };
    });
  }

  let { swarmEnabled = false, p2pEnabled = false }: { swarmEnabled?: boolean; p2pEnabled?: boolean } =
    $props();

  const creationReady = $derived(swarmEnabled && p2pEnabled);

  type KudosBalance = {
    beneficiaryKey?: string;
    totalKudos?: number;
    rac?: number;
  };

  type Pair = { modA: string; modB: string; count: number };
  type Group = { mods: string[]; score: number };
  type Preview = {
    projectId: string;
    slug: string;
    name: string;
    version: string;
    fileName?: string | null;
    side: string;
    dependencies: unknown[];
  };
  type MpiHit = {
    id: string;
    slug: string;
    name: string;
    description?: string;
    iconUrl?: string | null;
    downloads?: number | null;
    pageUrl?: string;
    url?: string;
    links?: Record<string, string>;
    provider: "modpackindex";
    projectType: "modpack";
  };
  type MrHit = {
    id: string;
    slug: string;
    name: string;
    description?: string;
    iconUrl?: string | null;
    downloads?: number | null;
    follows?: number | null;
    projectType?: string;
  };
  type MpiCategory = {
    id: number;
    slug: string;
    name: string;
    kind: "modpack" | "mod" | string;
  };

  let pairs = $state<Pair[]>([]);
  let groups = $state<Group[]>([]);
  let suggestions = $state<string[]>([]);
  let popularPacks = $state<MpiHit[]>([]);
  let popularMods = $state<MrHit[]>([]);
  let packCategories = $state<MpiCategory[]>([]);
  let selectedPackCategoryId = $state<number | null>(null);
  let packQuery = $state("");
  let packSearchTimer: ReturnType<typeof setTimeout> | null = null;
  let loading = $state(false);
  let error = $state("");
  let previewBusy = $state<string | null>(null);
  let installBusy = $state<string | null>(null);
  let previews = $state<Record<string, Preview | null>>({});
  let lastKey = $state("");
  let catalogViewResult = $state<{
    id: string;
    slug: string;
    name: string;
    description: string;
    projectType: string;
    iconUrl?: string | null;
    author?: string | null;
    downloads?: number | null;
    follows?: number | null;
    categories?: string[];
    provider?: string;
  } | null>(null);
  let catalogInstalling = $state(false);

  const CREATION_KINDS_FALLBACK = [
    "kubejs_ore_gen",
    "quest_scripts",
    "recipe_balance",
    "mod_configs",
    "full_pack_scaffold",
  ] as const;

  let creationKinds = $state<string[]>([...CREATION_KINDS_FALLBACK]);
  let creationKind = $state<string>("kubejs_ore_gen");
  let creationBrief = $state("");
  let creationMc = $state("");
  let creationLoader = $state("");
  let creationModIds = $state<string[]>([]);
  let creationBusy = $state(false);
  let creationError = $state("");
  let creationOutcome = $state<{
    result: {
      ok: boolean;
      jobId?: string;
      artifacts: { path: string; content: string }[];
      error?: string | null;
      claimedConfidence?: number;
      workerSignerPublicKey?: string | null;
    };
    verification: {
      passed: boolean;
      checks: { name: string; ok: boolean; detail: string }[];
      rewardGranted?: boolean;
    };
  } | null>(null);
  let creationAccepted = $state(false);
  let creationApplied = $state(false);
  let creationAppliedCount = $state(0);
  let creationJobId = $state("");
  let accessToken = $state("");
  let authUserEmail = $state("");
  let kudosBalance = $state<KudosBalance | null>(null);
  let kudosLoading = $state(false);

  const packThemeCategories = $derived(packCategories.filter((c) => c.kind === "modpack"));

  const canAccept = $derived(
    !!creationOutcome?.verification.passed &&
      !!creationOutcome?.result.workerSignerPublicKey?.trim() &&
      !!accessToken &&
      !creationAccepted &&
      !creationBusy,
  );

  const acceptDisabledReason = $derived.by(() => {
    if (creationAccepted || creationOutcome?.verification.rewardGranted) {
      return "Already accepted — reward granted";
    }
    if (creationBusy) return "Busy…";
    if (!creationOutcome?.verification.passed) return "Verification must pass first";
    if (!creationOutcome?.result.workerSignerPublicKey?.trim()) {
      return "Worker did not report a device signer key";
    }
    if (!accessToken) return "Sign in on Crash Votes to Accept and award Kudos";
    return "";
  });

  const claimedConfidenceLabel = $derived.by(() => {
    const c = creationOutcome?.result.claimedConfidence;
    if (c == null || Number.isNaN(c)) return null;
    const frac = c <= 1 ? c : c / 100;
    const pct = Math.round(frac * 100);
    const scaffold = frac < 0.45 ? " (likely scaffold)" : "";
    return `${pct}% (${frac.toFixed(2)})${scaffold}`;
  });

  async function refreshAuth() {
    try {
      const snap = await getAuthSnapshot();
      accessToken = snap.session?.access_token ?? "";
      authUserEmail = snap.user?.email?.trim() ?? "";
    } catch {
      accessToken = "";
      authUserEmail = "";
    }
  }

  async function loadKudos() {
    if (!swarmEnabled) {
      kudosBalance = null;
      return;
    }
    kudosLoading = true;
    try {
      kudosBalance = await invoke<KudosBalance>("get_local_kudos_balance");
    } catch {
      kudosBalance = null;
    } finally {
      kudosLoading = false;
    }
  }

  function humanizeCreationError(raw: string): string {
    const s = String(raw ?? "").trim();
    const lower = s.toLowerCase();
    if (lower.includes("p2p not enabled")) {
      return "Enable Prefer local P2P in Settings.";
    }
    if (lower.includes("no creation worker available") || lower.includes("no creation worker")) {
      return "No Creation worker peer online. Enable Creation worker on another node or wait.";
    }
    if (lower.includes("timed out") || lower.includes("timeout")) {
      return "Worker timed out. Try again or shorten the brief.";
    }
    if (lower.includes("brief too short")) {
      return "Brief must be at least 8 characters.";
    }
    if (
      lower.includes("ollama") ||
      lower.includes("connection refused") ||
      lower.includes("ai unavailable") ||
      lower.includes("model missing") ||
      lower.includes("used scaffold")
    ) {
      return s.replace(/^error:\s*/i, "").trim() || s;
    }
    return s.replace(/^error:\s*/i, "").trim() || s;
  }

  const softAiFallback = $derived.by(() => {
    const err = creationOutcome?.result.error?.trim() ?? "";
    if (!err) return false;
    const lower = err.toLowerCase();
    return (
      lower.includes("used scaffold") ||
      lower.includes("ai unavailable") ||
      lower.includes("ai output failed")
    );
  });

  async function loadCreationDefaults() {
    const path = $projectPath;
    if (!path) {
      creationMc = "";
      creationLoader = "";
      creationModIds = [];
      return;
    }
    try {
      const d = await invoke<{
        mcVersion?: string;
        loader?: string;
        modIds?: string[];
        kinds?: string[];
      }>("creation_job_defaults", { path });
      creationMc = d.mcVersion ?? "";
      creationLoader = d.loader ?? "";
      creationModIds = d.modIds ?? [];
      if (d.kinds?.length) {
        creationKinds = d.kinds;
        if (!creationKinds.includes(creationKind)) creationKind = creationKinds[0]!;
      }
    } catch {
      /* optional when no project */
    }
  }

  async function submitCreation() {
    creationError = "";
    creationOutcome = null;
    creationAccepted = false;
    creationApplied = false;
    creationAppliedCount = 0;
    creationJobId = "";
    const brief = creationBrief.trim();
    if (!brief) {
      creationError = "Brief is required.";
      return;
    }
    if (brief.length < 8) {
      creationError = "Brief must be at least 8 characters.";
      return;
    }
    if (!swarmEnabled) {
      creationError = "Enable TuffSwarm in Settings.";
      return;
    }
    if (!p2pEnabled) {
      creationError = "Enable Prefer local P2P in Settings.";
      return;
    }
    creationBusy = true;
    try {
      const jobId =
        typeof crypto !== "undefined" && "randomUUID" in crypto
          ? crypto.randomUUID()
          : `creation-${Date.now()}`;
      creationJobId = jobId;
      const outcome = await invoke<{
        result: {
          ok: boolean;
          jobId?: string;
          artifacts: { path: string; content: string }[];
          error?: string | null;
          claimedConfidence?: number;
          workerSignerPublicKey?: string | null;
        };
        verification: {
          passed: boolean;
          checks: { name: string; ok: boolean; detail: string }[];
          rewardGranted?: boolean;
        };
      }>("submit_creation_job", {
        job: {
          schemaVersion: 1,
          jobId,
          kind: creationKind,
          constraints: {
            mcVersion: creationMc.trim(),
            loader: creationLoader.trim(),
            modIds: creationModIds,
          },
          brief,
          reward: { kind: "kudos", amount: 50 },
          verify: { syntax: true, testLaunch: false },
          deadlineMs: 120_000,
        },
      });
      creationOutcome = outcome;
      await refreshAuth();
      if (outcome.verification.passed) {
        toasts.success("CreationJob verified — review artifacts before apply.");
      } else {
        toasts.error("CreationJob returned but verification failed.");
      }
    } catch (e) {
      creationError = humanizeCreationError(String(e));
      toasts.error(creationError);
    } finally {
      creationBusy = false;
    }
  }

  async function applyCreation() {
    if (!creationOutcome?.result.artifacts.length) return;
    if (!$projectPath) {
      toasts.error("Open a project first.");
      return;
    }
    if (!creationOutcome.verification.passed) {
      toasts.error("Cannot apply: verification failed.");
      return;
    }
    const n = creationOutcome.result.artifacts.length;
    if (!confirm(`Write ${n} artifact(s) into the open project?`)) return;
    creationBusy = true;
    try {
      const res = await invoke<{ written: string[] }>("apply_creation_artifacts", {
        path: $projectPath,
        artifacts: creationOutcome.result.artifacts,
      });
      creationApplied = true;
      creationAppliedCount = res.written.length;
      toasts.success(`Applied ${res.written.length} file(s).`);
    } catch (e) {
      toasts.error(String(e));
    } finally {
      creationBusy = false;
    }
  }

  async function acceptCreation() {
    if (!creationOutcome?.verification.passed) {
      toasts.error("Cannot accept: verification failed.");
      return;
    }
    const workerPk = creationOutcome.result.workerSignerPublicKey?.trim() ?? "";
    if (!workerPk) {
      toasts.error("Worker did not report a device signer key.");
      return;
    }
    const jobId =
      creationOutcome.result.jobId?.trim() || creationJobId.trim();
    if (!jobId) {
      toasts.error("Missing jobId.");
      return;
    }
    await refreshAuth();
    if (!accessToken) {
      toasts.error("Sign in (Crash Votes) to accept and award Kudos.");
      return;
    }
    if (
      !confirm(
        "Accept this result and award Kudos to the worker? This cannot be undone for this job.",
      )
    ) {
      return;
    }
    creationBusy = true;
    try {
      const body = await invoke<{
        ok?: boolean;
        kudos?: { awarded?: boolean; amount?: number; totalKudos?: number; rac?: number };
      }>("accept_creation_result", {
        jobId,
        workerSignerPublicKey: workerPk,
        accessToken,
        amount: 50,
      });
      creationAccepted = true;
      if (creationOutcome) {
        creationOutcome = {
          ...creationOutcome,
          verification: { ...creationOutcome.verification, rewardGranted: true },
        };
      }
      const k = body.kudos;
      if (k?.awarded) {
        toasts.success(
          `Accepted — awarded ${k.amount ?? 50} Kudos (total ${k.totalKudos ?? "—"} · RAC ${k.rac != null ? Number(k.rac).toFixed(1) : "—"}).`,
        );
        if (k.totalKudos != null || k.rac != null) {
          kudosBalance = {
            totalKudos: k.totalKudos ?? kudosBalance?.totalKudos,
            rac: k.rac ?? kudosBalance?.rac,
          };
        }
      } else {
        toasts.success("Accepted (Kudos already awarded for this job).");
      }
      await loadKudos();
    } catch (e) {
      toasts.error(String(e));
    } finally {
      creationBusy = false;
    }
  }
  function truncateArt(s: string, max = 280): string {
    const t = s.replace(/\r\n/g, "\n");
    return t.length <= max ? t : `${t.slice(0, max)}…`;
  }

  function formatCount(n?: number | null): string {
    if (n == null) return "—";
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
    return String(n);
  }

  async function loadCategories() {
    if (packCategories.length > 0) return;
    packCategories = await invoke<MpiCategory[]>("list_modpack_index_categories").catch(
      () => [],
    );
  }

  async function loadPacks() {
    const page = await invoke<{ results: MpiHit[]; total: number }>("search_modpack_index", {
      query: packQuery.trim(),
      page: 1,
      limit: 8,
      categoryId: selectedPackCategoryId,
    }).catch(() => ({ results: [], total: 0 }));
    popularPacks = page.results ?? [];
  }

  function togglePackCategory(id: number) {
    selectedPackCategoryId = selectedPackCategoryId === id ? null : id;
    if (packSearchTimer) {
      clearTimeout(packSearchTimer);
      packSearchTimer = null;
    }
    void loadPacks();
  }

  async function loadMods() {
    const path = $projectPath ?? "";
    const modsPage = await invoke<{ results: MrHit[]; total: number }>("search_modrinth_mods", {
      path,
      query: "",
      sort: "follows",
      contentType: "mod",
      page: 1,
      pageSize: 10,
    }).catch(() => ({ results: [], total: 0 }));
    popularMods = modsPage.results ?? [];
  }

  async function loadSwarm() {
    if (!$projectPath || !swarmEnabled) {
      pairs = [];
      groups = [];
      suggestions = [];
      return;
    }
    await invoke("report_mod_cooccurrence", {
      path: $projectPath,
      source: "library_trends_refresh",
    }).catch(() => {});
    const trends: any = await invoke("get_creation_trends", {
      path: $projectPath,
      limit: 20,
    });
    pairs = trends?.mergedPairs ?? trends?.localPairs ?? [];
    groups = trends?.groups ?? [];
    suggestions =
      trends?.suggestions ??
      (await invoke("suggest_mods_from_trends", {
        path: $projectPath,
        limit: 8,
      }).catch(() => []));
  }

  async function refresh() {
    loading = true;
    error = "";
    try {
      await Promise.all([loadPacks(), loadMods(), loadSwarm()]);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function schedulePackSearch() {
    if (packSearchTimer) clearTimeout(packSearchTimer);
    packSearchTimer = setTimeout(() => {
      packSearchTimer = null;
      void loadPacks();
    }, 300);
  }

  function onPackQueryKeydown(e: KeyboardEvent) {
    if (e.key !== "Enter") return;
    if (packSearchTimer) {
      clearTimeout(packSearchTimer);
      packSearchTimer = null;
    }
    void loadPacks();
  }

  $effect(() => {
    const key = `${swarmEnabled}:${$projectPath ?? ""}`;
    if (key === lastKey) return;
    lastKey = key;
    void refresh();
  });

  onMount(() => {
    void loadCategories();
    void refresh();
    void refreshAuth();
    if (swarmEnabled) {
      void loadCreationDefaults();
      void loadKudos();
    }
    return projectPath.subscribe(() => {
      if (swarmEnabled) void loadCreationDefaults();
    });
  });

  onDestroy(() => {
    if (packSearchTimer) clearTimeout(packSearchTimer);
  });

  function slugFromUrl(url: string): string {
    const clean = url.replace(/\/$/, "");
    const parts = clean.split("/");
    return parts[parts.length - 1] || clean;
  }

  function openPack(hit: MpiHit) {
    const links = hit.links ?? {};
    const mr =
      links.modrinth ||
      links.Modrinth ||
      (hit.pageUrl?.includes("modrinth.com") ? hit.pageUrl : null) ||
      (hit.url?.includes("modrinth.com") ? hit.url : null);
    const cf =
      links.curseforge ||
      links.CurseForge ||
      (hit.pageUrl?.includes("curseforge.com") ? hit.pageUrl : null) ||
      (hit.url?.includes("curseforge.com") ? hit.url : null);

    let provider: "modrinth" | "curseforge" = "modrinth";
    let id = hit.slug || hit.id;
    if (cf && !mr) {
      provider = "curseforge";
      id = slugFromUrl(cf);
    } else if (mr) {
      provider = "modrinth";
      id = slugFromUrl(mr);
    }

    catalogViewResult = {
      id,
      slug: hit.slug || id,
      name: hit.name,
      description: hit.description || "",
      projectType: "modpack",
      iconUrl: hit.iconUrl ?? null,
      author: null,
      downloads: hit.downloads ?? null,
      follows: null,
      categories: [],
      provider,
    };
  }

  function openMr(hit: MrHit) {
    catalogViewResult = {
      id: hit.id,
      slug: hit.slug || hit.id,
      name: hit.name,
      description: hit.description || "",
      projectType: hit.projectType || "mod",
      iconUrl: hit.iconUrl ?? null,
      author: null,
      downloads: hit.downloads ?? null,
      follows: hit.follows ?? null,
      categories: [],
      provider: "modrinth",
    };
  }

  async function openCatalogExternal() {
    if (!catalogViewResult) return;
    const slugOrId = (catalogViewResult.slug || catalogViewResult.id || "").trim();
    if (!slugOrId) return;
    const isPack = (catalogViewResult.projectType || "").includes("pack");
    const url =
      catalogViewResult.provider === "curseforge"
        ? /^\d+$/.test(slugOrId)
          ? `https://www.curseforge.com/projects/${slugOrId}`
          : `https://www.curseforge.com/minecraft/${isPack ? "modpacks" : "mc-mods"}/${slugOrId}`
        : `https://modrinth.com/${isPack ? "modpack" : "mod"}/${slugOrId}`;
    try {
      await openExternal(url);
    } catch (e) {
      toasts.error(String(e));
    }
  }

  async function installFromCatalog() {
    if (!catalogViewResult) return;
    const id = catalogViewResult.id || catalogViewResult.slug;
    if (!id) return;
    const isPack = (catalogViewResult.projectType || "").toLowerCase().includes("pack");
    catalogInstalling = true;
    try {
      if (isPack) {
        let targetDir = "";
        try {
          const info = await invoke<{ current: string; default: string }>("get_instances_path_info");
          targetDir = (info.current || info.default || "").replace(/[\\/]+$/, "");
        } catch {
          const home = ((await invoke("get_home_dir").catch(() => "")) as string) || "";
          if (home) targetDir = `${home.replace(/[\\/]+$/, "")}/TuffBox/instances`;
        }
        if (!targetDir) {
          toasts.error("Could not resolve instances folder.");
          return;
        }
        let source: string;
        if (catalogViewResult.provider === "curseforge") {
          const files = await invoke<Array<{ id: number; fileName?: string }>>(
            "get_curseforge_modpack_files",
            { modId: Number(catalogViewResult.id) || catalogViewResult.id, gameVersion: null },
          );
          const fileId = files?.[0]?.id;
          if (fileId == null) throw new Error("No CurseForge files for this modpack.");
          source = `cf:${catalogViewResult.id}:${fileId}`;
        } else {
          source = await invoke<string>("get_modrinth_pack_download", { projectId: id });
        }
        const dedup = await askDedupChoice(catalogViewResult.name);
        if (dedup === null) return; // closed the question — abort
        const res: any = await invoke("install_modpack", {
          source,
          targetDir,
          instanceName: catalogViewResult.name,
          dedup,
        });
        const dedupInfo = res?.dedup;
        if (dedupInfo?.mode === "shared" && (dedupInfo.linked ?? 0) > 0) {
          const mb = Math.round((dedupInfo.bytesReclaimed ?? 0) / 1e6);
          toasts.info(
            `File deduplication on — ${dedupInfo.linked} file(s) shared${mb > 0 ? `, ~${mb} MB saved` : ""}.`,
            4000,
          );
        }
        toasts.success(`Installed pack ${catalogViewResult.name}`);
        catalogViewResult = null;
        return;
      }
      if (!$projectPath) {
        toasts.error("Open a project first to install mods.");
        return;
      }
      if (catalogViewResult.provider === "curseforge") {
        toasts.info("Use Library → Discover or Content to install CurseForge mods.");
        return;
      }
      await invoke("add_modrinth_mod_with_dependencies", {
        path: $projectPath,
        modId: id,
        side: "both",
        dependencyTargets: null,
      });
      toasts.success(`Installed ${catalogViewResult.name}`);
      catalogViewResult = null;
      await refresh();
    } catch (e) {
      toasts.error(String(e));
    } finally {
      catalogInstalling = false;
    }
  }

  async function previewSlug(slug: string) {
    if (!$projectPath) return;
    previewBusy = slug;
    try {
      const preview = await invoke<Preview>("preview_modrinth_install", {
        path: $projectPath,
        modId: slug,
      });
      previews = { ...previews, [slug]: preview };
    } catch (e) {
      previews = { ...previews, [slug]: null };
      toasts.error(`${slug}: ${String(e)}`);
    } finally {
      previewBusy = null;
    }
  }

  async function installSlug(slug: string) {
    if (!$projectPath) return;
    if (!previews[slug]) await previewSlug(slug);
    const p = previews[slug];
    if (!p) return;
    if (!confirm(`Install ${p.name} (${p.version}) from Modrinth?`)) return;
    installBusy = slug;
    try {
      await invoke("add_modrinth_mod_with_dependencies", {
        path: $projectPath,
        modId: p.projectId || slug,
        side: p.side || "both",
        dependencyTargets: null,
      });
      toasts.success(`Installed ${p.name}`);
      await refresh();
    } catch (e) {
      toasts.error(String(e));
    } finally {
      installBusy = null;
    }
  }
</script>

<DedupAskDialog
  open={!!dedupAsk}
  packName={dedupAsk?.name ?? ""}
  onanswer={(v) => dedupAsk?.resolve(v)}
  oncancel={() => dedupAsk?.resolve(null)}
/>

<div class="creation">
  <div class="creation-head">
    <Sparkles size={18} />
    <div>
      <h2>Creation trends</h2>
      <p>
        Suggest more mods from hub co-occurrence. Packs/categories prefer hub
        (<code>/v1/mods/modpacks</code>, <code>/modpack-categories</code>, 15m cache).
        {#if swarmEnabled} TuffSwarm stats enabled.{/if}
      </p>
    </div>
    <button class="ghost" disabled={loading} onclick={refresh}>
      <span class:spin={loading} style="display:inline-flex"><RefreshCw size={14} /></span> Refresh
    </button>
  </div>

  {#if error}<div class="err">{error}</div>{/if}

  {#if swarmEnabled}
    <section class="peer-gen">
      <h3>Request peer generation</h3>
      {#if kudosLoading || kudosBalance}
        <div class="kudos-wrap">
          <KudosBalanceStrip
            title="Your Kudos"
            total={Number(kudosBalance?.totalKudos ?? 0)}
            rac={Number(kudosBalance?.rac ?? 0)}
            loading={kudosLoading && !kudosBalance}
            hint="Same device wallet — worker Accept awards here too"
          />
        </div>
      {/if}
      {#if !p2pEnabled}
        <p class="muted p2p-cta">
          Enable Prefer local P2P in Settings to request peer generation.
        </p>
      {:else if creationReady}
        <p class="muted">
          Submit a CreationJob to a TuffSwarm creation worker. Review artifacts, then apply to the open
          project (never auto-applied).
        </p>
        <div class="peer-form">
          <label>
            Kind
            <select bind:value={creationKind} disabled={creationBusy}>
              {#each creationKinds as k (k)}
                <option value={k}>{k}</option>
              {/each}
            </select>
          </label>
          <label>
            Minecraft
            <input bind:value={creationMc} placeholder="1.20.1" disabled={creationBusy} />
          </label>
          <label>
            Loader
            <input bind:value={creationLoader} placeholder="fabric" disabled={creationBusy} />
          </label>
          <label class="span-2">
            Brief
            <textarea
              bind:value={creationBrief}
              rows="3"
              placeholder="Describe what to generate…"
              disabled={creationBusy}
            ></textarea>
          </label>
          {#if creationModIds.length > 0}
            <p class="muted span-2">Constraints: {creationModIds.length} mod id(s) from inventory</p>
          {/if}
          <div class="row span-2">
            <button
              type="button"
              class="primary"
              disabled={creationBusy || !creationBrief.trim() || creationBrief.trim().length < 8}
              onclick={submitCreation}
            >
              {creationBusy ? "Waiting for worker…" : "Submit CreationJob"}
            </button>
          </div>
        </div>
        {#if creationError}<div class="err">{creationError}</div>{/if}
        {#if creationOutcome}
          <div class="peer-result" class:pass={creationOutcome.verification.passed}>
            <strong>
              Verification {creationOutcome.verification.passed ? "passed" : "failed"}
            </strong>
            <p class="muted order-hint">
              Suggested: Apply → try in pack → Accept result (awards Kudos).
            </p>
            {#if claimedConfidenceLabel}
              <p class="muted">Claimed confidence: {claimedConfidenceLabel}</p>
            {/if}
            {#if creationOutcome.result.error}
              <p class={softAiFallback ? "muted warn-line" : "err"}>
                {#if softAiFallback}<strong>Scaffold fallback:</strong> {/if}
                {creationOutcome.result.error}
              </p>
            {/if}
            {#if creationOutcome.verification.checks?.length}
              <ul class="check-list">
                {#each creationOutcome.verification.checks as check, i (check.name + String(i))}
                  <li class:ok={check.ok} class:fail={!check.ok}>
                    <span class="check-status">{check.ok ? "ok" : "fail"}</span>
                    <span>{check.name}{check.detail ? ` — ${check.detail}` : ""}</span>
                  </li>
                {/each}
              </ul>
            {/if}
            <ul class="art-list">
              {#each creationOutcome.result.artifacts as art, i (art.path + String(i))}
                <li>
                  <code>{art.path}</code>
                  <pre>{truncateArt(art.content)}</pre>
                </li>
              {/each}
            </ul>
            {#if creationOutcome.result.artifacts.length === 0}
              <p class="muted">No artifacts returned.</p>
            {/if}
            {#if creationApplied}
              <p class="muted">Applied {creationAppliedCount} file(s).</p>
            {/if}
            <p class="muted auth-line">
              {#if accessToken}
                Signed in{authUserEmail ? ` as ${authUserEmail}` : ""}.
              {:else}
                Sign in on Crash Votes to Accept and award Kudos.
              {/if}
            </p>
            {#if creationAccepted || creationOutcome.verification.rewardGranted}
              <p class="muted reward-granted">Reward granted</p>
            {/if}
            <div class="row">
              <button
                type="button"
                class="primary"
                disabled={
                  creationBusy ||
                  !creationOutcome.verification.passed ||
                  creationOutcome.result.artifacts.length === 0
                }
                onclick={applyCreation}
              >
                Apply to project
              </button>
              <button
                type="button"
                disabled={!canAccept}
                onclick={acceptCreation}
                title={acceptDisabledReason || "Award Kudos to the worker"}
              >
                {creationAccepted || creationOutcome.verification.rewardGranted
                  ? "Accepted"
                  : "Accept result"}
              </button>
            </div>
          </div>
        {/if}
      {/if}
    </section>
  {/if}

  <section>
    <h3>{packQuery.trim() ? "Search results" : "Popular modpacks · Modpack Index"}</h3>
    {#if packThemeCategories.length > 0}
      <div class="tag-row" role="group" aria-label="Pack themes">
        {#each packThemeCategories as cat (cat.id)}
          <button
            type="button"
            class="tag-chip"
            class:active={selectedPackCategoryId === cat.id}
            onclick={() => togglePackCategory(cat.id)}
          >
            {cat.name}
          </button>
        {/each}
      </div>
    {/if}
    <div class="pack-search">
      <input
        type="search"
        bind:value={packQuery}
        placeholder="Search modpacks…"
        aria-label="Search modpacks"
        oninput={schedulePackSearch}
        onkeydown={onPackQueryKeydown}
      />
    </div>
    {#if loading && popularPacks.length === 0}
      <p class="muted">Loading…</p>
    {:else if popularPacks.length === 0}
      <p class="muted">
        {packQuery.trim()
          ? "No packs found."
          : "Couldn’t load Modpack Index modpacks right now."}
      </p>
    {:else}
      <div class="hit-grid">
        {#each popularPacks as hit (hit.id)}
          <button type="button" class="hit-card" onclick={() => openPack(hit)}>
            {#if hit.iconUrl}
              <img src={hit.iconUrl} alt="" />
            {:else}
              <span class="hit-fallback">{(hit.name?.[0] || "?").toUpperCase()}</span>
            {/if}
            <div>
              <strong>{hit.name}</strong>
              <small><Download size={11} /> {formatCount(hit.downloads)}</small>
            </div>
          </button>
        {/each}
      </div>
    {/if}
    <p class="attr">
      Pack data from
      <a href="https://www.modpackindex.com" target="_blank" rel="noopener noreferrer"
        >Modpack Index</a
      >
    </p>
  </section>

  <section>
    <h3>Trending mods · Modrinth</h3>
    {#if loading && popularMods.length === 0}
      <p class="muted">Loading…</p>
    {:else if popularMods.length === 0}
      <p class="muted">Couldn’t load Modrinth mods right now.</p>
    {:else}
      <div class="hit-grid mods">
        {#each popularMods as hit (hit.id)}
          <button type="button" class="hit-card compact" onclick={() => openMr(hit)}>
            {#if hit.iconUrl}
              <img src={hit.iconUrl} alt="" />
            {:else}
              <span class="hit-fallback">{(hit.name?.[0] || "?").toUpperCase()}</span>
            {/if}
            <div>
              <strong>{hit.name}</strong>
              <small>{formatCount(hit.follows)} follows · {formatCount(hit.downloads)} dl</small>
            </div>
          </button>
        {/each}
      </div>
    {/if}
  </section>

  {#if !swarmEnabled}
    <div class="gate">
      <AlertTriangle size={16} />
      Enable <strong>Use TuffSwarm network</strong> in Settings for pack co-occurrence groups.
    </div>
  {:else if !$projectPath}
    <div class="gate">Open a project to build TuffSwarm co-occurrence from your installed mods.</div>
  {:else}
    <section>
      <h3>Frequent groups (TuffSwarm)</h3>
      {#if groups.length === 0}
        <p class="muted">No groups yet — need overlapping pairs from real packs.</p>
      {:else}
        <ul>
          {#each groups.slice(0, 8) as g (g.mods.join("|"))}
            <li>
              {#each g.mods as m, i (m)}
                {#if i > 0}<span class="plus">+</span>{/if}
                <code>{m}</code>
              {/each}
              <span>×{g.score}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <section>
      <h3>Top pairs</h3>
      {#if pairs.length === 0}
        <p class="muted">No pairs yet — install mods or export a pack with TuffSwarm on.</p>
      {:else}
        <ul>
          {#each pairs.slice(0, 10) as p (p.modA + p.modB)}
            <li><code>{p.modA}</code> + <code>{p.modB}</code> <span>×{p.count}</span></li>
          {/each}
        </ul>
      {/if}
    </section>

    <section>
      <h3>Suggested for your pack</h3>
      {#if suggestions.length === 0}
        <p class="muted">No partners yet — install a few mods first.</p>
      {:else}
        <div class="suggest-grid">
          {#each suggestions as slug (slug)}
            <div class="suggest-card">
              <strong>{slug}</strong>
              {#if previews[slug]}
                <small>{previews[slug]?.name} · {previews[slug]?.version}</small>
              {/if}
              <div class="row">
                <button class="ghost mini" disabled={previewBusy === slug} onclick={() => previewSlug(slug)}>
                  {previewBusy === slug ? "…" : "Preview"}
                </button>
                <button class="mini" disabled={installBusy === slug} onclick={() => installSlug(slug)}>
                  <Download size={12} />
                  {installBusy === slug ? "…" : "Install"}
                </button>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </section>
  {/if}
</div>

{#if catalogViewResult}
  <div
    class="catalog-backdrop"
    role="button"
    tabindex="-1"
    onclick={(e) => {
      if (e.target === e.currentTarget) catalogViewResult = null;
    }}
    onkeydown={() => {}}
  >
    <div
      class="catalog-modal"
      role="dialog"
      aria-modal="true"
      use:trapFocus={{ onEscape: () => (catalogViewResult = null) }}
    >
      <CatalogProjectView
        result={catalogViewResult}
        installing={catalogInstalling}
        onback={() => (catalogViewResult = null)}
        oninstall={() => void installFromCatalog()}
        onopenexternal={() => void openCatalogExternal()}
      />
    </div>
  </div>
{/if}

<style>
  .creation {
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
    padding: 18px;
  }
  .creation-head {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    margin-bottom: 16px;
  }
  .creation-head h2 {
    margin: 0;
    font-size: 16px;
  }
  .creation-head p {
    margin: 2px 0 0;
    color: var(--text-muted);
    font-size: 12px;
  }
  .creation-head button {
    margin-left: auto;
  }
  .peer-gen {
    margin-bottom: 18px;
    padding-bottom: 14px;
    border-bottom: 1px solid var(--border-color);
  }
  .kudos-wrap {
    margin: 8px 0 10px;
  }
  .peer-gen h3 {
    margin: 0 0 4px;
    font-size: 14px;
  }
  .peer-form {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
    margin-top: 10px;
  }
  .peer-form label {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .peer-form .span-2 {
    grid-column: 1 / -1;
  }
  .peer-form input,
  .peer-form select,
  .peer-form textarea {
    font: inherit;
    font-size: 13px;
    color: var(--text-primary);
    background: var(--bg-elevated);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
    padding: 6px 8px;
  }
  .peer-result {
    margin-top: 12px;
    padding: 10px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
  }
  .peer-result.pass {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, transparent);
  }
  .art-list {
    margin-top: 8px;
  }
  .art-list li {
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
  }
  .art-list pre {
    margin: 0;
    padding: 8px;
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
    background: var(--bg-tertiary);
    border-radius: var(--border-radius-sm);
    max-height: 140px;
    overflow: auto;
  }
  .tag-row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 0 0 10px;
  }
  .tag-chip {
    font-size: 12px;
    padding: 4px 10px;
    border-radius: 6px;
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
    color: var(--text-muted);
    cursor: pointer;
  }
  .tag-chip:hover {
    color: var(--text-secondary);
    border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }
  .tag-chip.active {
    color: var(--text-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 50%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
  }
  .pack-search {
    margin: 0 0 10px;
  }
  .pack-search input {
    width: 100%;
    max-width: 320px;
    padding: 8px 12px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
    color: var(--text-primary);
    font-size: 13px;
  }
  .pack-search input::placeholder {
    color: var(--text-muted);
  }
  .attr {
    margin: 10px 0 0;
    font-size: 12px;
    color: var(--text-muted);
  }
  .attr a {
    color: var(--text-muted);
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .attr a:hover {
    color: var(--text-secondary);
  }
  .gate {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-muted);
    font-size: 13px;
    padding: 12px;
    margin-top: 14px;
    background: var(--bg-elevated);
    border-radius: var(--border-radius-sm);
  }
  .err {
    color: var(--accent-danger);
    margin-bottom: 10px;
    font-size: 13px;
  }
  .warn-line {
    margin-bottom: 10px;
    font-size: 13px;
    color: var(--accent-warning, #f59e0b);
  }
  section {
    margin-top: 16px;
  }
  section:first-of-type {
    margin-top: 0;
  }
  h3 {
    font-size: 12px;
    margin: 0 0 10px;
    color: var(--text-muted);
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .hit-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 8px;
  }
  .hit-grid.mods {
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  }
  .hit-card {
    display: grid;
    grid-template-columns: 40px 1fr auto;
    gap: 10px;
    align-items: center;
    text-align: left;
    padding: 10px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
    color: var(--text-secondary);
    cursor: pointer;
  }
  .hit-card.compact {
    grid-template-columns: 36px 1fr;
  }
  .hit-card:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 40%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 6%, transparent);
  }
  .hit-card img,
  .hit-fallback {
    width: 40px;
    height: 40px;
    border-radius: 10px;
    object-fit: cover;
    background: var(--bg-tertiary);
  }
  .hit-fallback {
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 800;
    color: var(--accent-primary);
  }
  .hit-card.compact img,
  .hit-card.compact .hit-fallback {
    width: 36px;
    height: 36px;
  }
  .hit-card strong {
    display: block;
    color: var(--text-primary);
    font-size: 13px;
    line-height: 1.25;
  }
  .hit-card small {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    margin-top: 2px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .hit-card :global(svg:last-child) {
    color: var(--text-muted);
    opacity: 0.7;
    flex-shrink: 0;
  }
  ul {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 8px;
  }
  li {
    font-size: 12px;
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }
  li span {
    color: var(--text-muted);
  }
  .plus {
    color: var(--text-muted);
  }
  .muted {
    color: var(--text-muted);
    font-size: 12px;
  }
  .p2p-cta {
    margin: 8px 0 0;
    padding: 10px;
    background: var(--bg-elevated);
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
  }
  .order-hint {
    margin: 6px 0 0;
  }
  .check-list {
    margin: 8px 0;
  }
  .check-list li {
    font-size: 12px;
  }
  .check-list li.ok .check-status {
    color: var(--accent-primary);
  }
  .check-list li.fail .check-status {
    color: var(--accent-danger);
  }
  .check-status {
    font-weight: 700;
    text-transform: uppercase;
    font-size: 12px;
    letter-spacing: 0.04em;
  }
  .auth-line,
  .reward-granted {
    margin: 8px 0;
  }
  .reward-granted {
    color: var(--accent-primary);
  }
  .suggest-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 10px;
  }
  .suggest-card {
    background: var(--bg-elevated);
    border-radius: 10px;
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .suggest-card small {
    color: var(--text-muted);
  }
  .row {
    display: flex;
    gap: 8px;
  }
  button.primary {
    font: inherit;
    font-size: 13px;
    padding: 6px 12px;
    border-radius: var(--border-radius-sm);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 50%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 18%, transparent);
    color: var(--text-primary);
    cursor: pointer;
  }
  button.primary:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-primary) 28%, transparent);
  }
  button.primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .mini {
    font-size: 12px;
    padding: 4px 8px;
  }
  :global(.spin) {
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .catalog-backdrop {
    position: fixed;
    inset: 0;
    z-index: 90;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
  }
  .catalog-modal {
    width: min(920px, 96vw);
    max-height: min(90vh, 900px);
    overflow: auto;
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: 18px;
    padding: 0;
  }
</style>
