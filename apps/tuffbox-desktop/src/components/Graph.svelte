<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { GitGraph, RefreshCw, AlertTriangle, Box, Workflow, Download, X, Loader2, Maximize2, Minimize2, RotateCw, Info, Ban, ShieldAlert, ChevronDown, List, ExternalLink } from "@lucide/svelte";
  import { projectPath, projectInfo, pushWorkTrail, requestIdeIssuesRefresh } from "../lib/store";
  import EmptyState from "./EmptyState.svelte";
  import CatalogProjectView from "./CatalogProjectView.svelte";
  import { trapFocus } from "../lib/focusTrap";
  import * as d3 from "d3-force";
  import { onDestroy, onMount, untrack } from "svelte";


  type GraphNode = {
    id: string;
    kind: string;
    label: string;
    version?: string | null;
    side?: string;
    metadata?: Record<string, string>;
  };

  type GraphEdge = {
    from: string;
    to: string;
    kind: string;
    constraint?: string | null;
    reason?: string | null;
  };

  type GraphModel = {
    nodes: GraphNode[];
    edges: GraphEdge[];
  };

  type PositionedNode = GraphNode & { x: number; y: number; fx?: number | null; fy?: number | null; tone: string; ghost?: boolean; groupKey?: string };
  type DownloadProgress = {
    id: string;
    name: string;
    percent: number;
    status: string;
  };
  type DownloadBatch = {
    phase: string;
    failed?: { modId: string; error: string }[];
    downloaded?: string[];
    alreadyPresent?: string[];
  };

  let graph = $state<GraphModel | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  /** Apply / install failures — keep the graph visible (do not reuse `error`). */
  let planError = $state<string | null>(null);
  let selectedId = $state<string | null>(null);
  let lastLoadedPath = $state<string | null>(null);
  let resolving = $state(false);
  let message = $state<string | null>(null);
  let changePlan = $state<any | null>(null);
  /** Monotonic token so stale loadChangePlan / refresh results never overwrite a newer load. */
  let changePlanGen = 0;
  let changePlanLoading = $state(false);
  let graphSource = $state("local");
  let graphGeneratedAt = $state<string | null>(null);
  let graphRefreshing = $state(false);
  let refreshError = $state<string | null>(null);
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

  // Dependency preview dialog
  let depPreviewOpen = $state(false);
  let depPreviewLoading = $state(false);
  let depPreviewSlug = $state("");
  let depPreviewName = $state("");
  let depPreviewRequired = $state<{ target: string; reason?: string | null; alreadyInstalled?: boolean }[]>([]);
  let depPreviewOptional = $state<{ target: string; reason?: string | null; alreadyInstalled?: boolean }[]>([]);
  let depPreviewInstallWithOptional = $state(true);
  let depInstallStatus = $state<"idle" | "downloading" | "done" | "failed">("idle");
  let depInstallMessage = $state("");
  let depInstallError = $state<string | null>(null);
  let depInstallFailedIds = $state<string[]>([]);
  let depInstallBatchSeen = $state(false);
  let unlistenDownloadProgress: UnlistenFn | null = null;
  let unlistenDownloadBatch: UnlistenFn | null = null;
  let graphCanvasEl = $state<HTMLElement | undefined>(undefined);
  let fullscreenElement = $state<Element | null>(null);
  const graphFullscreen = $derived(fullscreenElement === graphCanvasEl);

  let showNodeList = $state(true);
  let changePlanExpanded = $state(true);
  let changePlanSeenKey = "";
  let canvasResizeObserver: ResizeObserver | null = null;

  function isLowPerfMode(): boolean {
    return document.documentElement.classList.contains("potato-pc");
  }

  function pauseSimulation() {
    simulation?.stop();
  }

  /// Cancel a long in-flight graph load / layout: invalidates the generation
  /// counter (so stale async results are dropped), stops the d3 simulation,
  /// and clears the loading overlay. The last successful graph stays painted.
  function cancelGraphLoad() {
    changePlanGen++;
    loading = false;
    changePlanLoading = false;
    graphRefreshing = false;
    pauseSimulation();
  }

  $effect(() => {
    if (!changePlan) return;
    const key = `${changePlan.summary}|${changePlan.actions?.length ?? 0}`;
    if (key === changePlanSeenKey) return;
    changePlanSeenKey = key;
    changePlanExpanded = !!(changePlan.actions?.length);
  });

  /** Right-click = same install path as left-click (hint: "click or right-click to install"). */
  async function onNodeContextInstall(event: MouseEvent, node: GraphNode & { ghost?: boolean }) {
    event.preventDefault();
    event.stopPropagation();
    selectedId = node.id;
    if (node.kind === "Missing" || !!node.ghost) {
      await installGhostNode(node.id);
      return;
    }
    // Installed dep whose jar may be missing on disk — same as icon click.
    if (node.kind === "Mod" && depNodeIds.has(node.id)) {
      await downloadMissingFiles();
      return;
    }
    const missing = missingDepsByMod.get(node.id) ?? [];
    for (const edge of missing) {
      await installSingleMissingDep(edge);
    }
  }

  function normalizeNode(node: any): GraphNode {
    return {
      ...node,
      id: typeof node.id === "string" ? node.id : node.id?.[0] ?? String(node.id),
    };
  }

  function normalizeEdge(edge: any): GraphEdge {
    const from = typeof edge.from === "string" ? edge.from : edge.from?.[0] ?? String(edge.from);
    const to = typeof edge.to === "string" ? edge.to : edge.to?.[0] ?? String(edge.to);
    return { ...edge, from, to };
  }

  function applyGraph(
    raw: any,
    opts: { resetSelection?: boolean; preserveLayout?: boolean } | boolean = {},
  ) {
    // Backward-compat: older call sites passed a boolean resetSelection flag.
    const options =
      typeof opts === "boolean" ? { resetSelection: opts } : opts ?? {};
    const resetSelection = !!options.resetSelection;
    const preserveLayout = !!options.preserveLayout;
    graph = {
      nodes: (raw.nodes ?? []).map(normalizeNode),
      edges: (raw.edges ?? []).map(normalizeEdge),
    };
    graphSource = raw.source ?? "local";
    graphGeneratedAt = raw.generatedAt ?? null;
    if (!preserveLayout) {
      simulationLayoutKey = "";
    }
    if (resetSelection) selectedId = null;
    hydrateMissingIcons().catch(() => {});
    queueMicrotask(() => hydrateGhostNodes().catch(() => {}));
  }

  let pendingRestorePositions: Map<
    string,
    {
      x?: number | null;
      y?: number | null;
      fx?: number | null;
      fy?: number | null;
      hubX?: number;
      hubY?: number;
      isHub?: boolean;
    }
  > | null = null;

  /** Soft reload after delete/install: keep camera, positions, and icon cache. */
  async function reloadGraphInPlace() {
    if (!$projectPath) return;
    const prevPos = new Map(
      simNodes.map((n) => [
        n.id,
        { x: n.x, y: n.y, fx: n.fx, fy: n.fy, hubX: n.hubX, hubY: n.hubY, isHub: n.isHub },
      ]),
    );
    const raw: any = await invoke("get_graph", { path: $projectPath });
    resetViewOnNextLayout = false;
    pendingRestorePositions = prevPos;
    applyGraph(raw, { preserveLayout: true });
    const gen = ++changePlanGen;
    await loadChangePlan(gen);
  }

  async function refreshGraph(manual = true, gen?: number) {
    if (!$projectPath || graphRefreshing) return;
    graphRefreshing = true;
    refreshError = null;
    const myGen = gen ?? ++changePlanGen;
    try {
      const raw: any = await invoke("refresh_graph", { path: $projectPath });
      if (myGen !== changePlanGen) return;
      // Soft-apply background enrich: keep camera/physics so it doesn't feel like a 2nd load.
      if (!manual && graph) {
        pendingRestorePositions = new Map(
          simNodes.map((n) => [
            n.id,
            { x: n.x, y: n.y, fx: n.fx, fy: n.fy, hubX: n.hubX, hubY: n.hubY, isHub: n.isHub },
          ]),
        );
        resetViewOnNextLayout = false;
        applyGraph(raw, { preserveLayout: true });
      } else {
        applyGraph(raw);
      }
      await loadChangePlan(myGen);
      if (manual) message = "Dependency metadata refreshed.";
    } catch (e) {
      if (myGen !== changePlanGen) return;
      refreshError = String(e);
      if (!graph) error = refreshError;
      // Still try a local plan so the panel isn't stuck empty after a failed refresh.
      await loadChangePlan(myGen);
    } finally {
      if (myGen === changePlanGen) {
        graphRefreshing = false;
        changePlanLoading = false;
      }
    }
  }

  async function load(force = false) {
    if (!$projectPath) return;
    if (!force && lastLoadedPath === $projectPath && graph) return;
    loading = true;
    error = null;
    brokenIcons = new Set();
    ghostMeta = {};
    simulationLayoutKey = "";
    resetViewOnNextLayout = true;
    pendingRestorePositions = null;
    // Drop previous project's plan immediately — avoids flashing the wrong summary.
    const gen = ++changePlanGen;
    changePlan = null;
    changePlanLoading = true;
    try {
      const raw: any = await invoke("get_graph", { path: $projectPath });
      if (gen !== changePlanGen) return;
      applyGraph(raw, { resetSelection: true });
      // Don't pre-select a node — otherwise every unrelated edge is dimmed
      // to near-invisible and the graph looks disconnected.
      lastLoadedPath = $projectPath;
      // Paint local/cache graph immediately.
      loading = false;

      // Cache/network are already Modrinth-enriched (or fingerprint-current).
      // Only cold "local" graphs need a background refresh — otherwise every open
      // looked like two loads (get_graph → refresh_graph + sim restart).
      if (raw.source === "local") {
        void refreshGraph(false, gen);
      } else {
        await loadChangePlan(gen);
        changePlanLoading = false;
      }
    } catch (e) {
      if (gen !== changePlanGen) return;
      error = String(e);
      changePlanLoading = false;
    } finally {
      if (gen === changePlanGen) loading = false;
    }
  }

  async function loadChangePlan(gen?: number) {
    if (!$projectPath) return;
    const myGen = gen ?? changePlanGen;
    try {
      const plan = await invoke("get_resolve_change_plan", { path: $projectPath });
      if (myGen !== changePlanGen) return;
      changePlan = plan;
    } catch {
      if (myGen !== changePlanGen) return;
      changePlan = null;
    } finally {
      if (myGen === changePlanGen) changePlanLoading = false;
    }
  }

  function nodeById(id: string) {
    return graph?.nodes.find((n) => n.id === id) ?? null;
  }

  function resolveNodeLabel(id: string): string {
    return nodeById(id)?.label ?? ghostNodes.find((n) => n.id === id)?.label ?? id.replace(/^mod:/, "");
  }

  // Track which nodes have a broken icon so we can fall back to a letter avatar
  let brokenIcons = $state(new Set<string>())
  /// Modrinth/CurseForge metadata for missing (ghost) dependency nodes.
  let ghostMeta = $state<Record<string, { name: string; iconUrl?: string | null; projectId?: string; description?: string; source?: string }>>({});

  function markIconBroken(id: string) {
    if (!brokenIcons.has(id)) {
      brokenIcons = new Set([...brokenIcons, id]);
    }
  }

  function modIconLookupKey(node: GraphNode): string | null {
    if (node.metadata?.project_id) return node.metadata.project_id;
    if (node.metadata?.source === "modrinth" && node.id.startsWith("mod:")) {
      return node.id.slice(4);
    }
    return null;
  }

  async function resolveIconForNode(node: GraphNode) {
    const key = modIconLookupKey(node);
    if (!key || !graph) return;
    try {
      const url: string | null = await invoke("get_modrinth_project_icon", { projectId: key });
      if (url) {
        node.metadata = { ...(node.metadata ?? {}), icon_url: url };
        graph = { ...graph };
        if (brokenIcons.has(node.id)) {
          const next = new Set(brokenIcons);
          next.delete(node.id);
          brokenIcons = next;
        }
      }
    } catch {
      // keep letter-avatar fallback
    }
  }

  async function handleIconError(node: GraphNode) {
    markIconBroken(node.id);
    await resolveIconForNode(node);
  }

  // Hydrate icons for Mod nodes that don't have icon_url in metadata.
  async function hydrateMissingIcons() {
    if (!graph) return;
    const missing = graph.nodes.filter((n) => {
      if (!isModOrPackKind(n.kind)) return false;
      if (brokenIcons.has(n.id)) return !!modIconLookupKey(n);
      if (n.metadata?.icon_url) return false;
      return !!modIconLookupKey(n);
    });
    if (missing.length === 0) return;
    await Promise.all(missing.map((n) => resolveIconForNode(n)));
  }

  function point(id: string) {
    return simNodes.find((n) => n.id === id) ?? positionById.get(id);
  }

  function edgePath(edge: GraphEdge): string {
    // Hard dependencies point from the parent (library/dependency) toward the
    // child (the mod that requires it) so the hub reads top-down. Conflict and
    // loader/optional edges keep their natural direction.
    const reversed = edge.kind === "Requires";
    const srcId = reversed ? edge.to : edge.from;
    const tgtId = reversed ? edge.from : edge.to;
    const source = point(srcId);
    const target = point(tgtId);
    if (!source || !target) return "";
    const dx = target.x - source.x;
    const dy = target.y - source.y;
    const distance = Math.hypot(dx, dy);
    if (distance < 1) return "";
    const ux = dx / distance;
    const uy = dy / distance;
    const r1 = nodeSize(source) / 2;
    const r2 = nodeSize(target) / 2;
    const x1 = source.x + ux * (r1 + 2);
    const y1 = source.y + uy * (r1 + 2);
    const x2 = target.x - ux * (r2 + 8);
    const y2 = target.y - uy * (r2 + 8);
    // Match modrinth-extras: straight edges by default; only parallel
    // multi-edges get a light fan so they stay readable.
    const parallel = displayEdges.filter(
      (e) =>
        (e.from === srcId && e.to === tgtId) ||
        (e.from === tgtId && e.to === srcId),
    );
    if (parallel.length <= 1) {
      return `M ${x1} ${y1} L ${x2} ${y2}`;
    }
    const idx = parallel.findIndex(
      (e) => e.from === edge.from && e.to === edge.to && e.kind === edge.kind,
    );
    const bend = (idx - (parallel.length - 1) / 2) * 22;
    const mx = (x1 + x2) / 2 + -uy * bend;
    const my = (y1 + y2) / 2 + ux * bend;
    return `M ${x1} ${y1} Q ${mx} ${my} ${x2} ${y2}`;
  }

  function isGhost(id: string) {
    return id.startsWith("__ghost__");
  }

  function isContentPackKind(kind: string): boolean {
    return kind === "ResourcePack" || kind === "ShaderPack";
  }

  function isModOrPackKind(kind: string): boolean {
    return kind === "Mod" || isContentPackKind(kind);
  }

  function edgeDanger(edge: GraphEdge) {
    if (edge.kind === "Conflicts" || edge.kind === "BreaksWith") {
      const from = nodeById(edge.from);
      const to = nodeById(edge.to);
      return !!from && !!to && isModOrPackKind(from.kind) && isModOrPackKind(to.kind);
    }
    return edge.kind === "Requires" && !nodeById(edge.to);
  }

  // Deterministic position from string hash (for ghost nodes)
  function hashPos(s: string, baseX: number, baseY: number): {x: number, y: number} {
    let h = 0;
    for (let i = 0; i < s.length; i++) h = ((h << 5) - h) + s.charCodeAt(i);
    const angle = ((h % 360) / 180) * Math.PI;
    const dist = 100 + (Math.abs(h) % 80);
    return { x: baseX + Math.cos(angle) * dist, y: baseY + Math.sin(angle) * dist };
  }

  function modIdFromNode(nodeId: string) {
    return nodeId.startsWith("mod:") ? nodeId.slice(4) : nodeId;
  }

  function nodeIconUrl(node: GraphNode | PositionedNode): string | null {
    if (brokenIcons.has(node.id)) return null;
    if (node.metadata?.icon_url) return node.metadata.icon_url;
    const ghost = ghostMeta[node.id];
    if (ghost?.iconUrl) return ghost.iconUrl;
    return null;
  }

  function displayLabel(node: GraphNode | PositionedNode): string {
    return ghostMeta[node.id]?.name ?? node.label;
  }

  function nodeSize(node: PositionedNode): number {
    if (node.kind === "Missing") return 36;
    return depNodeIds.has(node.id) ? 36 : 48;
  }

  async function removeConflictNode(nodeId: string) {
    if (!$projectPath) return;
    const modId = modIdFromNode(nodeId);
    resolving = true;
    error = null;
    message = null;
    try {
      await invoke("remove_project_mod", { path: $projectPath, modId });
      message = `Removed ${modId}.`;
      await reloadGraphInPlace();
    } catch (e) {
      error = String(e);
    } finally {
      resolving = false;
    }
  }

  async function removeModNode(event: MouseEvent | Event, nodeId: string) {
    event.stopPropagation();
    await removeConflictNode(nodeId);
  }

  async function installGhostNode(nodeId: string) {
    if (!$projectPath) return;
    await previewModrinthDep(modIdFromNode(nodeId));
  }

  async function downloadMissingFiles() {
    if (!$projectPath) return;
    resolving = true;
    error = null;
    message = null;
    try {
      const downloaded: string[] = await invoke("download_missing_files", { path: $projectPath });
      message = downloaded.length
        ? `Downloaded ${downloaded.length} file(s): ${downloaded.join(", ")}`
        : "All mod files are already on disk.";
      await load(true);
    } catch (e) {
      error = String(e);
    } finally {
      resolving = false;
    }
  }

  function handleNodeClick(node: PositionedNode) {
    selectedId = node.id;
    if (node.kind === "Missing" || node.ghost) {
      installGhostNode(node.id);
    }
  }

  /// Click on a dependency icon (SVG node or card icon). For missing/ghost
  /// nodes this installs the dep. For installed dep nodes that were pulled
  /// in implicitly (not added by the user), clicking the icon re-installs /
  /// re-downloads the file to make sure it's on disk.
  function handleDepIconClick(node: PositionedNode, event?: MouseEvent) {
    event?.stopPropagation();
    selectedId = node.id;
    if (node.kind === "Missing" || node.ghost) {
      installGhostNode(node.id);
    } else if (node.kind === "Mod" && depNodeIds.has(node.id)) {
      // Already installed as a dep — re-download the file in case it's missing
      downloadMissingFiles();
    }
  }

  function summarizePlanResults(applied: string[]): { message: string | null; planError: string | null } {
    const skipped = applied.filter((s) => s.startsWith("skipped "));
    const ok = applied.filter((s) => !s.startsWith("skipped "));
    if (skipped.length && ok.length) {
      return {
        message: `Applied: ${ok.join(", ")}`,
        planError: skipped.join("; "),
      };
    }
    if (skipped.length) {
      return { message: null, planError: skipped.join("; ") };
    }
    if (ok.length) {
      return { message: `Applied plan: ${ok.join(", ")}`, planError: null };
    }
    return { message: "No deterministic actions were applied.", planError: null };
  }

  async function applyAction(index: number) {
    if (!$projectPath || !changePlan) return;
    resolving = true;
    planError = null;
    message = null;
    try {
      const applied: string[] = await invoke("apply_resolve_action", { path: $projectPath, actionIndex: index });
      const summary = summarizePlanResults(applied);
      message = summary.message
        ? summary.message.replace(/^Applied plan:/, "Applied action:")
        : applied.length
          ? null
          : "No deterministic action was applied.";
      planError = summary.planError;
      await load(true);
      if (applied.some((s) => !s.startsWith("skipped "))) {
        pushWorkTrail(`Applied resolve action · ${applied.join(", ")}`, [
          { id: "test", label: "Test launch", kind: "play" },
          { id: "dismiss", label: "Dismiss", kind: "dismiss" },
        ]);
        requestIdeIssuesRefresh();
      }
    } catch (e) {
      planError = String(e);
    } finally {
      resolving = false;
    }
  }

  async function applyChangePlan() {
    if (!$projectPath || !changePlan) return;
    resolving = true;
    planError = null;
    message = null;
    try {
      const applied: string[] = await invoke("apply_resolve_change_plan", { path: $projectPath });
      const summary = summarizePlanResults(applied);
      message = summary.message;
      planError = summary.planError;
      await load(true);
      if (applied.some((s) => !s.startsWith("skipped "))) {
        pushWorkTrail(`Applied resolve plan (${applied.filter((s) => !s.startsWith("skipped ")).length})`, [
          { id: "test", label: "Test launch", kind: "play" },
          { id: "dismiss", label: "Dismiss", kind: "dismiss" },
        ]);
        requestIdeIssuesRefresh();
      }
    } catch (e) {
      planError = String(e);
    } finally {
      resolving = false;
    }
  }

  function formatChangeAction(action: Record<string, any>): string {
    if (action.installMod) {
      const version = action.installMod.version;
      return `Install ${action.installMod.project_id}${version ? ` at ${version}` : " (latest compatible version)"}`;
    }
    if (action.removeMod) return `Remove ${action.removeMod.node_id}`;
    if (action.disableMod) return `Disable ${action.disableMod.node_id}`;
    if (action.updateMod) return `Update ${action.updateMod.node_id} to ${action.updateMod.target_version}`;
    if (action.editConfig) return `Edit configuration: ${action.editConfig.path}`;
    return "Apply recommended change";
  }

  async function installMissingDependencies() {
    if (!$projectPath || !hasMissingSpotlight) return;
    resolving = true;
    planError = null;
    message = null;
    try {
      const installed: string[] = await invoke("resolve_missing_dependencies", { path: $projectPath });
      message = installed.length ? `Installed dependencies: ${installed.join(", ")}` : "No installable missing dependencies were found.";
      await load(true);
      if (installed.length) {
        pushWorkTrail(`Installed ${installed.length} dependencies`, [
          { id: "test", label: "Test launch", kind: "play" },
          { id: "dismiss", label: "Dismiss", kind: "dismiss" },
        ]);
        requestIdeIssuesRefresh();
      }
    } catch (e) {
      planError = String(e);
    } finally {
      resolving = false;
    }
  }

  /// Installs a single missing dependency by its slug/id extracted from the
  /// graph edge. The edge.to for a missing dep is the raw Modrinth slug.
  async function installSingleMissingDep(edge: GraphEdge) {
    if (!$projectPath) return;
    const depId = edge.to.startsWith("mod:") ? edge.to.slice(4) : edge.to;
    await previewModrinthDep(depId);
  }

  /// Fetches Modrinth dependency info and shows the preview dialog.
  async function previewModrinthDep(depId: string) {
    if (!$projectPath) return;
    depPreviewSlug = depId;
    depPreviewName = depId;
    depPreviewRequired = [];
    depPreviewOptional = [];
    depPreviewOpen = true;
    depPreviewLoading = true;
    depPreviewInstallWithOptional = true;
    depInstallStatus = "idle";
    depInstallMessage = "";
    depInstallError = null;
    depInstallFailedIds = [];
    try {
      const preview: any = await invoke("preview_modrinth_install", { path: $projectPath, modId: depId });
      if (preview) {
        depPreviewName = preview.name ?? depId;
        const depsList = preview.dependencies ?? [];
        const installed = new Set<string>(preview.installedDependencies ?? []);
        depsList.forEach((dep: any) => {
          const kind = (dep.type ?? "").toLowerCase();
          const entry = {
            target: dep.target,
            reason: dep.reason ?? null,
            alreadyInstalled: installed.has(dep.target),
          };
          if (kind.includes("required") || kind.includes("requires")) {
            depPreviewRequired.push(entry);
          } else {
            depPreviewOptional.push(entry);
          }
        });
      }
    } catch {
      // preview failed — install directly
    } finally {
      depPreviewLoading = false;
    }
  }

  /// Actually install from the dep preview dialog.
  async function confirmDepInstall() {
    if (!$projectPath) return;
    resolving = true;
    error = null;
    message = null;
    depInstallStatus = "downloading";
    depInstallMessage = `Downloading ${depPreviewName}…`;
    depInstallError = null;
    depInstallFailedIds = [];
    depInstallBatchSeen = false;
    try {
      if (depPreviewInstallWithOptional) {
        await invoke("add_modrinth_mod_with_dependencies", {
          path: $projectPath,
          modId: depPreviewSlug,
          side: "auto",
          dependencyTargets: null,
        });
      } else {
        await invoke("add_modrinth_mod", {
          path: $projectPath,
          modId: depPreviewSlug,
          side: "auto",
        });
      }
      if (!depInstallBatchSeen) {
        depInstallStatus = "done";
        depInstallMessage = `${depPreviewName} installed.`;
        message = `Installed ${depPreviewName}.`;
        await load(true);
      }
    } catch (e) {
      depInstallStatus = "failed";
      depInstallError = String(e);
      depInstallMessage = `Could not install ${depPreviewName}.`;
    } finally {
      resolving = false;
    }
  }

  async function retryDepInstall() {
    if (!$projectPath) return;
    if (depInstallFailedIds.length === 0) {
      await confirmDepInstall();
      return;
    }
    resolving = true;
    depInstallStatus = "downloading";
    depInstallMessage = `Retrying ${depPreviewName}…`;
    depInstallError = null;
    depInstallBatchSeen = false;
    try {
      await invoke("retry_failed_mod_downloads", {
        path: $projectPath,
        modIds: depInstallFailedIds,
      });
    } catch (e) {
      depInstallStatus = "failed";
      depInstallError = String(e);
      depInstallMessage = `Retry failed for ${depPreviewName}.`;
    } finally {
      resolving = false;
    }
  }

  /// Installed mods that other mods require/optionally depend on.
  /// These are "downloaded dependencies" and get the third (amber) tone.
  /// Mods with no incoming dep edges are "main" and keep their side color.

  // Transitive reduction over the Requires graph: if A requires B and B
  // requires C, drop the direct A->C link so the centre isn't overloaded with
  // redundant edges (modrinth-extras / hub-and-spoke readability). Non-Requires
  // edges (conflicts, optional, loader links) are left untouched.
  function reduceTransitiveRequires(allEdges: GraphEdge[]): GraphEdge[] {
    const req = allEdges.filter((e) => e.kind === "Requires");
    const adj = new Map<string, string[]>();
    for (const e of req) {
      if (!adj.has(e.from)) adj.set(e.from, []);
      adj.get(e.from)!.push(e.to);
    }
    const reachable = (start: string, excludeDirect: string): boolean => {
      const seen = new Set<string>([start]);
      const stack = [...(adj.get(start) ?? [])];
      while (stack.length) {
        const cur = stack.pop()!;
        if (cur === excludeDirect) return true;
        if (seen.has(cur)) continue;
        seen.add(cur);
        for (const next of adj.get(cur) ?? []) {
          if (next !== excludeDirect) stack.push(next);
        }
      }
      return false;
    };
    const keep = req.filter((e) => !reachable(e.from, e.to));
    const others = allEdges.filter((e) => e.kind !== "Requires");
    return [...keep, ...others];
  }

  const nodes = $derived(graph?.nodes ?? []);
  const edges = $derived(graph?.edges ?? []);
  const displayEdges = $derived(reduceTransitiveRequires(
    edges.filter((e) =>
      ["Requires", "Optional", "Conflicts", "BreaksWith", "Replaces", "RequiresLoader", "RequiresMinecraft", "RequiresJava"].includes(e.kind)
    )
  ));

  const depNodeIds = $derived(new Set(
    nodes
      .filter((node) => node.kind === "Mod")
      .filter((node) => {
        const incoming = displayEdges.filter(
          (e) => e.to === node.id && (e.kind === "Requires" || e.kind === "Optional")
        );
        return incoming.length > 0;
      })
      .map((node) => node.id)
  ));

  const selected = $derived(selectedId ? nodeById(selectedId) : null);
  const selectedEdges = $derived(selectedId
    ? displayEdges.filter((edge) => edge.from === selectedId || edge.to === selectedId)
    : []);
  const missingEdges = $derived(displayEdges.filter(
    (edge) => edge.kind === "Requires" && (!nodeById(edge.to) || nodeById(edge.to)?.kind === "Missing")
  ));
  const missingDepsByMod = $derived((() => {
    const map = new Map<string, GraphEdge[]>();
    for (const edge of missingEdges) {
      const list = map.get(edge.from) ?? [];
      list.push(edge);
      map.set(edge.from, list);
    }
    return map;
  })());
  const missingRequirerIds = $derived(new Set(missingEdges.map((e) => e.from)));
  /** Who asks for what — used by Missing spotlight side panel. */
  const missingDemandList = $derived(
    [...missingDepsByMod.entries()]
      .map(([fromId, deps]) => ({
        fromId,
        fromLabel: resolveNodeLabel(fromId),
        deps: deps.map((edge) => ({
          id: edge.to,
          label: resolveNodeLabel(edge.to),
          edge,
        })),
      }))
      .sort((a, b) => a.fromLabel.localeCompare(b.fromLabel)),
  );
  const selectedMissingDeps = $derived(selectedId ? (missingDepsByMod.get(selectedId) ?? []) : []);

  function catalogResultFromNode(node: GraphNode) {
    const meta = node.metadata ?? {};
    const ghost = ghostMeta[node.id];
    const slug = (meta.slug || node.id.replace(/^mod:/, "").replace(/^__ghost__/, "")).trim();
    const projectId = String(meta.project_id || ghost?.projectId || "").trim();
    const id = (projectId || slug).trim();
    if (!id && !slug) return null;
    const source = (meta.source || ghost?.source || "modrinth").toLowerCase();
    // Local jars still open via Modrinth/CF when we know a project id or slug;
    // never send provider "local" to the catalog API.
    const provider =
      source === "curseforge"
        ? "curseforge"
        : source === "modrinth" || source === "local" || source === "direct"
          ? "modrinth"
          : "modrinth";
    return {
      id: id || slug,
      slug: slug || id,
      name: node.label,
      description: meta.description || ghost?.description || "",
      projectType: "mod",
      iconUrl: meta.icon_url || ghost?.iconUrl || null,
      author: meta.author || null,
      downloads: null,
      follows: null,
      categories: nodeCategories(node),
      provider,
    };
  }

  function openSelectedInLauncher() {
    if (!selected) return;
    const result = catalogResultFromNode(selected);
    if (!result) {
      message = "No catalog page for this node.";
      return;
    }
    catalogViewResult = result;
  }

  async function openSelectedExternal() {
    if (!catalogViewResult) return;
    const slugOrId = (catalogViewResult.slug || catalogViewResult.id || "").trim();
    if (!slugOrId) return;
    const url =
      catalogViewResult.provider === "curseforge"
        ? /^\d+$/.test(slugOrId)
          ? `https://www.curseforge.com/projects/${slugOrId}`
          : `https://www.curseforge.com/minecraft/mc-mods/${slugOrId}`
        : `https://modrinth.com/mod/${slugOrId}`;
    try {
      const { open } = await import("@tauri-apps/plugin-shell");
      await open(url);
    } catch (e) {
      message = `Could not open link: ${e}`;
    }
  }

  async function installSelectedFromCatalog() {
    if (!catalogViewResult || !$projectPath) return;
    const id = (catalogViewResult.slug || catalogViewResult.id || "").trim();
    if (!id) return;
    catalogViewResult = null;
    await previewModrinthDep(id);
  }

  async function installSelectedMissingDeps() {
    for (const edge of selectedMissingDeps) {
      await installSingleMissingDep(edge);
    }
  }

  const conflictEdges = $derived(displayEdges.filter((edge) => {
    if (!["Conflicts", "BreaksWith"].includes(edge.kind)) return false;
    const from = nodeById(edge.from);
    const to = nodeById(edge.to);
    // Both ends must be installed mods. Declaring "incompatible with X" when X
    // isn't in the pack is metadata, not a real conflict (undefined !== "Missing"
    // used to let those through and recommend removing the installed mod).
    return (
      !!from &&
      !!to &&
      isModOrPackKind(from.kind) &&
      isModOrPackKind(to.kind)
    );
  }));

  /** Deduped conflict pairs with a recommended removal target. */
  const conflictInsights = $derived((() => {
    type Insight = {
      key: string;
      edge: GraphEdge;
      leftId: string;
      rightId: string;
      leftLabel: string;
      rightLabel: string;
      severity: "critical" | "warning";
      reason: string;
      known: boolean;
      recommendRemoveId: string;
      recommendRemoveLabel: string;
      keepId: string;
      keepLabel: string;
      leftDependents: number;
      rightDependents: number;
    };
    const seen = new Set<string>();
    const out: Insight[] = [];
    const dependentsOf = (id: string) =>
      displayEdges.filter((e) => e.to === id && (e.kind === "Requires" || e.kind === "Optional")).length;

    for (const edge of conflictEdges) {
      const a = edge.from < edge.to ? edge.from : edge.to;
      const b = edge.from < edge.to ? edge.to : edge.from;
      const key = `${a}::${b}`;
      if (seen.has(key)) continue;
      seen.add(key);

      const leftId = edge.from;
      const rightId = edge.to;
      const leftLabel = nodeById(leftId)?.label ?? leftId.replace(/^mod:/, "");
      const rightLabel = nodeById(rightId)?.label ?? rightId.replace(/^mod:/, "");
      const leftDependents = dependentsOf(leftId);
      const rightDependents = dependentsOf(rightId);
      // Prefer removing the mod fewer others depend on.
      const recommendRemoveId = leftDependents <= rightDependents ? leftId : rightId;
      const keepId = recommendRemoveId === leftId ? rightId : leftId;
      const reason =
        (edge.reason && edge.reason.trim()) ||
        (edge.kind === "BreaksWith"
          ? `${leftLabel} breaks when used with ${rightLabel}`
          : `${leftLabel} is incompatible with ${rightLabel}`);
      const known = /known/i.test(reason);

      out.push({
        key,
        edge,
        leftId,
        rightId,
        leftLabel,
        rightLabel,
        severity: edge.kind === "BreaksWith" ? "critical" : "warning",
        reason,
        known,
        recommendRemoveId,
        recommendRemoveLabel: nodeById(recommendRemoveId)?.label ?? recommendRemoveId.replace(/^mod:/, ""),
        keepId,
        keepLabel: nodeById(keepId)?.label ?? keepId.replace(/^mod:/, ""),
        leftDependents,
        rightDependents,
      });
    }

    return out.sort((x, y) => {
      if (x.severity !== y.severity) return x.severity === "critical" ? -1 : 1;
      if (x.known !== y.known) return x.known ? -1 : 1;
      return x.leftLabel.localeCompare(y.leftLabel);
    });
  })());

  const byKind = $derived(nodes.reduce<Record<string, number>>((acc, node) => {
    acc[node.kind] = (acc[node.kind] ?? 0) + 1;
    return acc;
  }, {}));

  /// Human-friendly mod categories so the graph reads as labelled clusters
  type ModGroup = {
    key: string;
    label: string;
    color: string;
    matches: (id: string, label: string) => boolean;
  };

  /// Clusters mirror Modrinth's official mod category taxonomy (1:1).
  /// Keyword `matches` is only a fallback when provider categories are missing.
  const MOD_GROUPS: ModGroup[] = [
    {
      key: "local",
      label: "Local",
      color: "rgba(148,163,184,0.55)",
      matches: () => false,
    },
    {
      key: "resourcepacks",
      label: "Resource packs",
      color: "rgba(56,189,248,0.55)",
      matches: () => false,
    },
    {
      key: "shaders",
      label: "Shaders",
      color: "rgba(192,132,252,0.55)",
      matches: () => false,
    },
    {
      key: "adventure",
      label: "Adventure",
      color: "rgba(239,68,68,0.5)",
      matches: (id, label) =>
        /(adventure|dungeon|exploration|quest|rpg.?quest|twilight|aether|cataclysm|yungs)/i.test(
          id + " " + label
        ),
    },
    {
      key: "cursed",
      label: "Cursed",
      color: "rgba(127,29,29,0.55)",
      matches: (id, label) =>
        /(cursed|horror|scary|creepy|nightmare|villain|evil|gore|haunt)/i.test(
          id + " " + label
        ),
    },
    {
      key: "decor",
      label: "Decoration",
      color: "rgba(45,212,191,0.5)",
      matches: (id, label) =>
        /(bookshelf|supplementaries|decor|furniture|macaw|macaws|chisel|construction|lantern|lighting|painting|statue|plant|flower|abundance|cobble|quark)/i.test(
          id + " " + label
        ),
    },
    {
      key: "economy",
      label: "Economy",
      color: "rgba(234,179,8,0.5)",
      matches: (id, label) =>
        /(economy|shop|trade|market|currency|coin|bank|claim|land.?claim)/i.test(
          id + " " + label
        ),
    },
    {
      key: "equipment",
      label: "Equipment",
      color: "rgba(248,113,113,0.5)",
      matches: (id, label) =>
        /(equipment|weapon|armor|tool|sword|bow|shield|combat|tactical)/i.test(
          id + " " + label
        ),
    },
    {
      key: "farming",
      label: "Food",
      color: "rgba(132,204,22,0.5)",
      matches: (id, label) =>
        /(farmers|croptopia|delight|food|farm|brewin|beer|cuisine|pam|crop|nutrition|spice|bee|honey)/i.test(
          id + " " + label
        ),
    },
    {
      key: "game_mechanics",
      label: "Game Mechanics",
      color: "rgba(251,146,60,0.5)",
      matches: (id, label) =>
        /(game.?mechanic|origin|origins|skill|level.?up|progression|difficulty|hardcore|serene|seasons?)/i.test(
          id + " " + label
        ),
    },
    {
      key: "library",
      label: "Library",
      color: "rgba(148,163,184,0.55)",
      matches: (id, label) =>
        /(api|lib|library|core|architectury|cloth|yacl|trinkets|cardinal|collective|midnightlib|resourceful|balm|configured|framework|modmenu|curios|packetfixer|fabric-language|mixin)/i.test(
          id + " " + label
        ),
    },
    {
      key: "magic",
      label: "Magic",
      color: "rgba(217,70,239,0.5)",
      matches: (id, label) =>
        /(magic|mana|arcanus|hexerei|ironspell|spell|bloodmagic|astral|occultism|forbidden|ars|naturesaura|reliquary|artifact|runes|mage|wizard|ritual)/i.test(
          id + " " + label
        ),
    },
    {
      key: "management",
      label: "Management",
      color: "rgba(96,165,250,0.5)",
      matches: (id, label) =>
        /(jei|rei|emi|justenough|roughlyenough|jade|wthit|theoneprobe|waila|appleskin|tooltip)/i.test(
          id + " " + label
        ),
    },
    {
      key: "minigame",
      label: "Minigame",
      color: "rgba(244,114,182,0.5)",
      matches: (id, label) =>
        /(minigame|arcade|bingo|parkour|party.?game)/i.test(id + " " + label),
    },
    {
      key: "mobs",
      label: "Mobs",
      color: "rgba(220,38,38,0.55)",
      matches: (id, label) =>
        /(mob|creature|enemy|boss|undead|mutant|goblin|dragon|bestiary|alex.?mobs|guard|iceandfire|entity)/i.test(
          id + " " + label
        ),
    },
    {
      key: "rendering",
      label: "Optimization",
      color: "color-mix(in srgb, var(--accent-secondary) 50%, transparent)",
      matches: (id, label) =>
        /(sodium|iris|lithium|ferrite|phosphor|embeddium|oculus|voxy|entityculling|rubidium|canary|immediatelyfast|starlight|noisium|dynamic-fps|lazydfu|etf|entity.?texture|dashloader|smoothboot|memoryleakfix|modernfix|krypton|shader|exordium|optifine|performance|fps|distanthorizons)/i.test(
          id + " " + label
        ),
    },
    {
      key: "social",
      label: "Social",
      color: "rgba(56,189,248,0.5)",
      matches: (id, label) =>
        /(social|chat|emoji|discord|voice|proximity|skin|cosmetic|presence)/i.test(
          id + " " + label
        ),
    },
    {
      key: "storage",
      label: "Storage",
      color: "rgba(14,165,233,0.5)",
      matches: (id, label) =>
        /(inventory|sort|chest|shulker|sophisticated|ironchest|backpack|storage|expandedstorage|itemzo)/i.test(
          id + " " + label
        ),
    },
    {
      key: "create",
      label: "Technology",
      color: "rgba(245,158,11,0.55)",
      matches: (id, label) =>
        /(create|flywheel|ponder|steam|mechanical|automated|factory|pipez|integrateddynamics|applied|refinedstorage|thermal|mekanism|techreborn|industrial|immersive|powah|energy|ae2)/i.test(
          id + " " + label
        ),
    },
    {
      key: "transport",
      label: "Transportation",
      color: "rgba(94,234,212,0.5)",
      matches: (id, label) =>
        /(transport|train|rail|boat|vehicle|flight|elevator|teleport|waystone|waystones)/i.test(
          id + " " + label
        ),
    },
    {
      key: "qol",
      label: "Utility",
      color: "rgba(250,204,21,0.5)",
      matches: (id, label) =>
        /(qol|utility|xaero|journeymap|minimap|map|sound|music|rightclick|mouse|keybind|zoom|recipe|patchouli|comfort|physic|easy|fast)/i.test(
          id + " " + label
        ),
    },
    {
      key: "worldgen",
      label: "World Generation",
      color: "rgba(34,197,94,0.5)",
      matches: (id, label) =>
        /(biome|terrablender|terralith|tectonic|biomesoplenty|byg|ohthebiomes|reterraforged|cave|structure|explorify|wwoo|noise|worldgen|geophilic|incendium|amplified|promenade|regions|nullscape|wilder)/i.test(
          id + " " + label
        ),
    },
  ];

  const modNodes = $derived(nodes.filter((node) => node.kind === "Mod"));
  const resourcePackNodes = $derived(nodes.filter((node) => node.kind === "ResourcePack"));
  const shaderPackNodes = $derived(nodes.filter((node) => node.kind === "ShaderPack"));
  /// Side-panel grouping: mods bucketed by the same human categories used for
  /// the graph clusters, so the list reads top-to-bottom like the canvas.
  const groupedMods = $derived((() => {
    const buckets = new Map<string, { label: string; color: string; nodes: GraphNode[] }>();
    for (const node of modNodes) {
      const g = categorizeMod(node);
      const bucket = buckets.get(g.key) ?? { label: g.label, color: g.color, nodes: [] };
      bucket.nodes.push(node);
      buckets.set(g.key, bucket);
    }
    const order = MOD_GROUPS.map((g) => g.key);
    return [...buckets.entries()]
      .sort((a, b) => {
        const ia = order.indexOf(a[0]);
        const ib = order.indexOf(b[0]);
        return (ia === -1 ? 99 : ia) - (ib === -1 ? 99 : ib) || a[1].label.localeCompare(b[1].label);
      })
      .map(([, bucket]) => ({
        ...bucket,
        nodes: bucket.nodes.slice().sort((x, y) => x.label.localeCompare(y.label)),
      }));
  })());
  const platformNodes = $derived(nodes.filter((node) =>
    node.kind !== "Mod" &&
    node.kind !== "Profile" &&
    node.kind !== "Missing" &&
    !isContentPackKind(node.kind)
  ));
  const profileNodes = $derived(nodes.filter((node) => node.kind === "Profile"));

  // Synthesize ghost nodes for any edge endpoint that has no real node. This
  // is the single most important fix: the Rust builder intentionally emits
  // edges to *missing* dependencies, but d3's forceLink throws
  // "missing: <id>" when a link references a node that isn't in the array,
  // which aborted the whole simulation and left the canvas blank. Ghost
  // nodes keep every link resolvable while still being visually distinct
  // (and the missing-dependency panel operates on the real graph).
  const nodeIdSet = (id: string) => nodes.some((n) => n.id === id);
  const ghostNodes = $derived((() => {
    const out: GraphNode[] = nodes
      .filter((node) => node.kind === "Missing")
      .map((node) => ({
        ...node,
        label: ghostMeta[node.id]?.name ?? node.label,
        metadata: {
          ...(node.metadata ?? {}),
          ...(ghostMeta[node.id]?.iconUrl ? { icon_url: ghostMeta[node.id].iconUrl! } : {}),
          ...(ghostMeta[node.id]?.projectId ? { project_id: ghostMeta[node.id].projectId! } : {}),
          source: "modrinth",
        },
      }));
    const seen = new Set(out.map((node) => node.id));
    // Only required missing targets become installable ghosts — optional
    // integrations must not appear in "Missing dependencies".
    for (const e of edges) {
      if (e.kind !== "Requires") continue;
      const end = e.to;
      if (!nodeIdSet(end) && !seen.has(end)) {
        seen.add(end);
        const slug = end.replace(/^mod:/, "").replace(/^__ghost__/, "");
        const cached = ghostMeta[end];
        out.push({
          id: end,
          kind: "Missing",
          label: cached?.name ?? slug,
          version: null,
          side: "unknown",
          metadata: {
            ...(cached?.iconUrl ? { icon_url: cached.iconUrl } : {}),
            ...(cached?.projectId ? { project_id: cached.projectId } : {}),
            ...(cached?.description ? { description: cached.description } : {}),
            source: cached?.source ?? "modrinth",
          },
        });
      }
    }
    return out;
  })());
  const missingSpotlightIds = $derived((() => {
    const ids = new Set<string>();
    for (const edge of missingEdges) {
      ids.add(edge.from);
      ids.add(edge.to);
    }
    for (const n of ghostNodes) ids.add(n.id);
    return ids;
  })());
  const hasMissingSpotlight = $derived(missingSpotlightIds.size > 0);
  const displayNodes = $derived([
    ...modNodes,
    ...resourcePackNodes,
    ...shaderPackNodes,
    ...platformNodes,
    ...ghostNodes,
  ]);

  // --- Canvas edge curation ---
  // The full edge list drives the side panel, missing-dep detection and the
  // layout sim, but drawing every runtime link turns the canvas into spaghetti
  // (on a real 160-mod pack: ~320 loader/minecraft/java links converging on the
  // centre). The canvas therefore hides ONLY those pure-runtime links by
  // default. Every real mod-to-mod relation — hard deps (grey), optional
  // integrations (green) and conflicts (red) — stays visible, so the graph
  // reads as a connected web instead of just the red conflict edges. Hub
  // fan-ins (e.g. everything → fabric-api) are still drawn but faded so they
  // don't dominate. The "All edges" toggle also reveals the runtime links.
  let showAllEdges = $state(false);
  const RUNTIME_EDGE_KINDS = ["RequiresLoader", "RequiresMinecraft", "RequiresJava"];
  const HUB_FAN_IN_THRESHOLD = 8;
  const hubTargetIds = $derived(new Set(
    [
      ...displayEdges
        .filter((e) => e.kind === "Requires")
        .reduce((acc, e) => acc.set(e.to, (acc.get(e.to) ?? 0) + 1), new Map<string, number>())
        .entries(),
    ]
      .filter(([, count]) => count >= HUB_FAN_IN_THRESHOLD)
      .map(([id]) => id),
  ));
  const renderedEdges = $derived.by(() => {
    const base = showAllEdges
      ? displayEdges
      : displayEdges.filter((e) => {
          // Pure runtime links (loader/minecraft/java) — only for the selected node.
          if (RUNTIME_EDGE_KINDS.includes(e.kind)) {
            return !!selectedId && (e.from === selectedId || e.to === selectedId);
          }
          // Hub fan-in (everything → fabric-api etc.) — hide unless selected.
          if (e.kind === "Requires" && hubTargetIds.has(e.to)) {
            return !!selectedId && (e.from === selectedId || e.to === selectedId);
          }
          // Optional integrations across the whole pack drown the canvas; keep
          // them only when a node is selected or when the user opts into "All edges".
          if (e.kind === "Optional") {
            return !!selectedId && (e.from === selectedId || e.to === selectedId);
          }
          return true;
        });
    // Missing spotlight must always draw requirer → missing edges, even when the
    // missing target is a hub (normally filtered) so "who needs what" is visible.
    if (highlightMode !== "missing" || missingEdges.length === 0) return base;
    const keys = new Set(base.map((e) => `${e.from}:${e.to}:${e.kind}`));
    const extra = missingEdges.filter((e) => !keys.has(`${e.from}:${e.to}:${e.kind}`));
    return extra.length > 0 ? [...base, ...extra] : base;
  });

  async function hydrateGhostNodes() {
    if (!$projectPath || !graph) return;
    const missing = ghostNodes.filter((n) => !ghostMeta[n.id]);
    if (missing.length === 0) return;
    const updates: Record<string, { name: string; iconUrl?: string | null; projectId?: string; description?: string; source?: string }> = {};
    await Promise.all(
      missing.map(async (node) => {
        const key = modIdFromNode(node.id);
        // Try Modrinth first (slug or project id).
        try {
          const project: any = await invoke("get_modrinth_project", { projectId: key });
          if (project) {
            updates[node.id] = {
              name: project.name ?? key,
              iconUrl: project.iconUrl ?? null,
              projectId: project.id ?? key,
              description: project.description ?? undefined,
              source: "modrinth",
            };
            return;
          }
        } catch {
          // fall through to CurseForge
        }
        // CurseForge fallback: search by slug, take the top hit.
        try {
          const page: any = await invoke("search_curseforge_mods", {
            path: $projectPath,
            query: key,
            gameVersion: null,
            loader: null,
            contentType: "mod",
            page: 1,
            pageSize: 5,
          });
          const hit = page?.results?.[0];
          if (hit) {
            updates[node.id] = {
              name: hit.name ?? key,
              iconUrl: hit.iconUrl ?? null,
              projectId: hit.id ?? key,
              description: hit.description ?? undefined,
              source: "curseforge",
            };
          }
        } catch {
          // keep slug label
        }
      })
    );
    if (Object.keys(updates).length > 0) {
      ghostMeta = { ...ghostMeta, ...updates };
      if (positioned.length > 0) {
        positioned = positioned.map((node) => ({
          ...node,
          label: displayLabel(node),
        }));
      }
    }
  }

  /// Group mods by which profile includes them (via IncludedInProfile edges)
  const modsByProfile = $derived((() => {
    const map = new Map<string, GraphNode[]>();
    const orphaned: GraphNode[] = [];
    for (const mod of modNodes) {
      const profiles = edges
        .filter((e) => e.kind === "IncludedInProfile" && e.to === mod.id)
        .map((e) => nodeById(e.from)?.label ?? e.from);
      if (profiles.length === 0) {
        orphaned.push(mod);
      } else {
        for (const prof of profiles) {
          const list = map.get(prof) ?? [];
          list.push(mod);
          map.set(prof, list);
        }
      }
    }
    return { map, orphaned };
  })());
  type LayoutNode = PositionedNode & {
    depth: number;
    isHub: boolean;
    component: number;
    hubId: string;
    hubX: number;
    hubY: number;
    vx?: number;
    vy?: number;
    fx?: number | null;
    fy?: number | null;
    index?: number;
    groupKey?: string;
  };

  type GroupMeta = { key: string; label: string; color: string; x: number; y: number; r: number };
  let groupMeta = $state<GroupMeta[]>([]);

  /// Cluster currently hovered in the legend / on a halo; used to spotlight that
  /// group's nodes and edges and dim the rest.
  let hoveredGroup = $state<string | null>(null);

  /** Spotlight conflicts or missing deps (toolbar toggle). */
  let highlightMode = $state<null | "conflicts" | "missing">(null);

  const LEGEND_STORAGE_KEY = "tuffbox.graph.legend-expanded";
  let legendExpanded = $state(
    typeof localStorage === "undefined"
      ? true
      : localStorage.getItem(LEGEND_STORAGE_KEY) !== "false",
  );

  function toggleLegend() {
    legendExpanded = !legendExpanded;
    try {
      localStorage.setItem(LEGEND_STORAGE_KEY, String(legendExpanded));
    } catch {
      /* ignore */
    }
  }

  function toggleHighlight(mode: "conflicts" | "missing") {
    const next = highlightMode === mode ? null : mode;
    highlightMode = next;
    // Clear selection so selection-dimming does not hide other requirer→dep links.
    if (next) selectedId = null;
  }

  function nodeGroupKey(id: string): string | null {
    return positionById.get(id)?.groupKey ?? null;
  }
  /// True when an edge belongs to the hovered cluster (both endpoints in it, or
  /// one endpoint is a core/runtime hub that the cluster radiates from).
  function edgeInHoveredGroup(from: string, to: string): boolean {
    if (!hoveredGroup) return true;
    const gf = nodeGroupKey(from);
    const gt = nodeGroupKey(to);
    const core = (g: string | null) => g === "core" || g === "runtime" || g === null;
    if (core(gf) || core(gt)) return true;
    return gf === hoveredGroup || gt === hoveredGroup;
  }

  function nodeInHighlight(nodeId: string, kind?: string): boolean {
    if (!highlightMode) return true;
    if (highlightMode === "conflicts") {
      return conflictEdges.some((e) => e.from === nodeId || e.to === nodeId);
    }
    // Requirers (edge.from) + missing targets (edge.to / ghosts).
    return missingSpotlightIds.has(nodeId);
  }

  function edgeInHighlight(from: string, to: string, _kind: string, _danger: boolean): boolean {
    if (!highlightMode) return true;
    if (highlightMode === "conflicts") {
      return conflictEdges.some(
        (e) => (e.from === from && e.to === to) || (e.from === to && e.to === from),
      );
    }
    // Strict: only the actual "installed mod → missing dep" edges.
    return missingEdges.some((e) => e.from === from && e.to === to);
  }

  /** When a spotlight is on, only that set dims; ignore selection/hover dimming. */
  function nodeIsDimmed(node: { id: string; kind: string; groupKey?: string }): boolean {
    if (highlightMode) return !nodeInHighlight(node.id, node.kind);
    if (selectedId && selectedId !== node.id && !selectedEdges.some((e) => e.from === node.id || e.to === node.id)) {
      return true;
    }
    if (
      hoveredGroup !== null &&
      node.groupKey !== hoveredGroup &&
      node.groupKey !== "core" &&
      node.groupKey !== "runtime"
    ) {
      return true;
    }
    return false;
  }

  function edgeIsDimmed(from: string, to: string, kind: string, danger: boolean): boolean {
    if (highlightMode) return !edgeInHighlight(from, to, kind, danger);
    if (selectedId && from !== selectedId && to !== selectedId) return true;
    if (!edgeInHoveredGroup(from, to)) return true;
    return false;
  }

  let canvasWidth = $state(1600);
  let canvasHeight = $state(900);
  // Must be $state so {#each positioned} mounts nodes after simulation starts.
  // Do NOT $effect(read positioned → write positioned) — that loops forever.
  let positioned = $state<PositionedNode[]>([]);
  let simulation: any = null;
  let simulationLayoutKey = "";
  let simNodes: LayoutNode[] = [];
  const nodeEls = new Map<string, SVGGElement>();
  const edgeEls = new Map<string, SVGPathElement>();
  // Camera fit flag — plain let. Must NOT be $state: startSimulation reads+writes
  // it, and that used to live inside $effect → effect_update_depth_exceeded (tab hang).
  let resetViewOnNextLayout = true;

  /// Modrinth's official category taxonomy → our cluster key (1:1).
  /// Keyword matching is only a fallback for mods with no cached categories.
  const MODRINTH_CATEGORY_TO_GROUP: Record<string, string> = {
    adventure: "adventure",
    cursed: "cursed",
    decoration: "decor",
    economy: "economy",
    equipment: "equipment",
    food: "farming",
    "game-mechanics": "game_mechanics",
    library: "library",
    magic: "magic",
    management: "management",
    minigame: "minigame",
    mobs: "mobs",
    optimization: "rendering",
    social: "social",
    storage: "storage",
    technology: "create",
    transportation: "transport",
    utility: "qol",
    worldgen: "worldgen",
  };

  /// CurseForge display names → Modrinth slugs (after space/punct normalize).
  const CATEGORY_ALIASES: Record<string, string> = {
    "adventure-and-rpg": "adventure",
    "api-and-library": "library",
    "armor-tools-and-weapons": "equipment",
    cosmetic: "social",
    "map-and-information": "management",
    performance: "optimization",
    "utility-qol": "utility",
    "world-gen": "worldgen",
    "world-generation": "worldgen",
    farming: "food",
    redstone: "technology",
    automation: "technology",
    energy: "technology",
    "energy-fluid-and-item-transport": "transportation",
    "player-transport": "transportation",
    biomes: "worldgen",
    dimensions: "worldgen",
    structures: "worldgen",
    "ores-and-resources": "worldgen",
    "bug-fixes": "utility",
    "server-utility": "utility",
    miscellaneous: "utility",
    education: "utility",
    genetics: "game-mechanics",
    processing: "technology",
    mcreator: "library",
    "twitch-integration": "social",
  };

  /** CF returns "Utility & QoL" / "World Gen"; Modrinth uses "utility" / "worldgen". */
  function normalizeCategorySlug(raw: string): string {
    const slug = raw
      .trim()
      .toLowerCase()
      .replace(/[&+,/]/g, " ")
      .replace(/\s+/g, "-")
      .replace(/-+/g, "-")
      .replace(/^-|-$/g, "");
    return CATEGORY_ALIASES[slug] ?? slug;
  }

  function nodeCategories(node: { metadata?: Record<string, string> }): string[] {
    const raw = node.metadata?.categories;
    if (!raw) return [];
    // Prefer `|` (current graph serializer); also accept legacy comma-joined lists.
    const parts = raw.includes("|") ? raw.split("|") : raw.split(",");
    return parts
      .map((c) => normalizeCategorySlug(c))
      .filter((c) => c && !LOADER_CATEGORY_NOISE.has(c));
  }

  const LOADER_CATEGORY_NOISE = new Set([
    "fabric", "forge", "neoforge", "quilt", "bukkit", "spigot", "paper", "purpur",
    "sponge", "bungeecord", "waterfall", "velocity",
  ]);

  /// Content-specific tags beat catch-alls (utility / library).
  const GROUP_PRIORITY = [
    "local",
    "cursed",
    "rendering",
    "worldgen",
    "mobs",
    "adventure",
    "equipment",
    "magic",
    "game_mechanics",
    "create",
    "transport",
    "farming",
    "decor",
    "storage",
    "management",
    "economy",
    "social",
    "minigame",
    "library",
    "qol",
  ];

  /// Assign every Mod/Missing node to one cluster. Strong name signals win over
  /// ambiguous Modrinth tags (e.g. ETF tagged decoration, Macaw's tagged storage).
  function categorizeMod(node: GraphNode | PositionedNode): ModGroup {
    const id = node.id;
    const label = node.label;
    const slug = id.replace(/^mod:/, "").replace(/^__ghost__/, "");
    const text = `${slug} ${label}`;
    const cats = nodeCategories(node);
    const contentType = (node.metadata?.content_type ?? "").toLowerCase();

    if (node.kind === "ResourcePack" || contentType === "resourcepack") {
      return MOD_GROUPS.find((g) => g.key === "resourcepacks")!;
    }
    if (node.kind === "ShaderPack" || contentType === "shader") {
      return MOD_GROUPS.find((g) => g.key === "shaders")!;
    }

    if ((node.metadata?.source ?? "").toLowerCase() === "local") {
      return MOD_GROUPS.find((g) => g.key === "local")!;
    }

    const decorGroup = MOD_GROUPS.find((g) => g.key === "decor")!;
    const renderingGroup = MOD_GROUPS.find((g) => g.key === "rendering")!;

    // Strong overrides before Modrinth tags (fixes known mis-tags).
    if (/(macaw|macaws|furniture|chisel|supplementaries)/i.test(text)) {
      return decorGroup;
    }
    if (/(entity.?texture|^etf\b|\betf\b|shader|iris|sodium|better.?end.?sky|end.?sky)/i.test(text)) {
      return renderingGroup;
    }

    if (cats.length) {
      const mappedKeys = cats
        .map((c) => MODRINTH_CATEGORY_TO_GROUP[c])
        .filter((k): k is string => !!k);
      if (mappedKeys.length) {
        let best: ModGroup | null = null;
        let bestRank = Number.POSITIVE_INFINITY;
        for (const key of mappedKeys) {
          const rank = GROUP_PRIORITY.indexOf(key);
          const r = rank >= 0 ? rank : 999;
          if (r < bestRank) {
            bestRank = r;
            best = MOD_GROUPS.find((g) => g.key === key) ?? null;
          }
        }
        if (best) {
          return best;
        }
      }
    }
    for (const key of GROUP_PRIORITY) {
      const group = MOD_GROUPS.find((g) => g.key === key);
      if (group?.matches(slug, label)) {
        return group;
      }
    }
    return MOD_GROUPS.find((g) => g.key === "qol")!;
  }

  // Bump when layout algorithm parameters change so cached graphs re-seed.
  const LAYOUT_VERSION = "v7-local-cluster";

  const layoutKey = $derived([
    LAYOUT_VERSION,
    ...displayNodes.map((n) => n.id).sort(),
    ...displayEdges.map((e) => `${e.from}:${e.to}:${e.kind}`).sort(),
  ].join("|"));

  // Edge kinds that link every mod to the core runtime. They carry no layout
  // information (every mod has both), but as d3 links they dominate the physics
  // and drag the whole graph into a knot — so they are drawn but never fed to
  // the link force.
  const HUB_EDGE_KINDS = ["RequiresLoader", "RequiresMinecraft", "RequiresJava"];

  /// Deterministic clustered layout.
  ///
  /// Replaces the old free-form force layout, which on real packs (150+ mods,
  /// 550+ edges) collapsed into an unreadable knot: hub edges to the pinned
  /// loader/minecraft nodes pulled every mod toward the centre, the category
  /// catch-all grew huge, and the undamped anchor spring overshot chaotically.
  ///
  /// Instead we compute positions up front and let the simulation only polish:
  ///  - every mod is routed into one category cluster (categorizeMod);
  ///  - each cluster is packed as a phyllotaxis (sunflower) disc, highest
  ///    degree nodes at the centre, so it is compact and overlap-free by
  ///    construction — the disc radius scales with sqrt(member count);
  ///  - cluster anchors are placed sequentially on a ring sized from the disc
  ///    diameters, so clusters can never overlap or collapse into one corner;
  ///  - the sim then runs only collide + strong anchor springs + intra-cluster
  ///    links (hub/runtime edges excluded), starting from an already-tidy
  ///    layout — nothing can explode, and the result is deterministic.
  function startSimulation() {
    if (!displayNodes.length) return;

    const isCoreNode = (node: GraphNode) =>
      node.kind === "MinecraftVersion" ||
      node.kind === "Loader" ||
      node.kind === "JavaRuntime" ||
      node.kind === "Profile";

    // Links that actually shape the layout: real mod-to-mod relations only.
    // Optional edges are visual-only — including them in the link force pulls
    // unrelated clusters into each other on large packs.
    const simLinks = displayEdges
      .filter((e) => !HUB_EDGE_KINDS.includes(e.kind) && e.kind !== "Optional")
      .map((e) => ({ source: e.from, target: e.to, ...e }));
    const linkId = (value: any) => (typeof value === "object" && value ? value.id : value);
    const simDegree = new Map<string, number>();
    for (const id of displayNodes.map((n) => n.id)) simDegree.set(id, 0);
    for (const l of simLinks) {
      simDegree.set(linkId(l.source), (simDegree.get(linkId(l.source)) ?? 0) + 1);
      simDegree.set(linkId(l.target), (simDegree.get(linkId(l.target)) ?? 0) + 1);
    }
    const degOf = (id: string) => simDegree.get(id) ?? 0;

    // Group members by category, sorted by degree (cluster hubs first).
    const byGroup = new Map<string, GraphNode[]>();
    for (const node of displayNodes) {
      if (isCoreNode(node)) continue;
      const key = categorizeMod(node).key;
      if (!byGroup.has(key)) byGroup.set(key, []);
      byGroup.get(key)!.push(node);
    }
    const groupOrder = MOD_GROUPS.map((g) => g.key);
    const clusters = [...byGroup.entries()]
      .map(([key, members]) => {
        members.sort((a, b) => degOf(b.id) - degOf(a.id) || a.id.localeCompare(b.id));
        return { key, members };
      })
      .sort((a, b) => groupOrder.indexOf(a.key) - groupOrder.indexOf(b.key));

    // Center-to-center pitch for sunflower packing (icon 48px + label clearance).
    // The previous SPACING/2 ≈ 29px packed icons on top of each other.
    const NODE_PITCH = 96;
    // Halo radius cap: catch-all groups (Utility etc.) can hold 100+ mods and
    // the raw sqrt formula balloons into a full-canvas circle that pushes the
    // group label off-screen. Cap keeps every halo readable; nodes may extend
    // past it on very large packs.
    const MAX_GROUP_RADIUS = 420;
    const discRadius = (n: number) =>
      Math.min(NODE_PITCH * Math.sqrt(Math.max(1, n)) + 56, MAX_GROUP_RADIUS);

    // Ring placement: each cluster gets an arc from its diameter + gap so
    // neighbouring discs never touch.
    const CLUSTER_GAP = 160;
    const arcWidths = clusters.map((c) => 2 * discRadius(c.members.length) + CLUSTER_GAP);
    const ringRadius = Math.max(420, arcWidths.reduce((a, b) => a + b, 0) / (2 * Math.PI));
    const maxDisc = Math.max(280, ...clusters.map((c) => discRadius(c.members.length)));
    const canvasSpan = (ringRadius + maxDisc + 180) * 2;
    canvasWidth = Math.max(1800, canvasSpan);
    canvasHeight = Math.max(1200, canvasSpan);
    const cx = canvasWidth / 2;
    const cy = canvasHeight / 2;

    let angleCursor = -Math.PI / 2;
    const anchors = new Map<string, { x: number; y: number }>();
    clusters.forEach((cluster, i) => {
      const arc = arcWidths[i] / ringRadius;
      const angle = angleCursor + arc / 2;
      angleCursor += arc;
      anchors.set(cluster.key, {
        x: cx + Math.cos(angle) * ringRadius,
        y: cy + Math.sin(angle) * ringRadius,
      });
    });

    // Expose cluster halos/labels for rendering. Halo radius follows the disc
    // size so big categories (a 100+ mod Utility pack) stay inside.
    groupMeta = clusters.map((cluster) => {
      const def = MOD_GROUPS.find((g) => g.key === cluster.key) ?? {
        key: cluster.key,
        label: "Other Mods",
        color: "rgba(113,113,122,0.5)",
        matches: () => false,
      };
      const anchor = anchors.get(cluster.key)!;
      return {
        key: cluster.key,
        label: def.label,
        color: def.color,
        x: anchor.x,
        y: anchor.y,
        r: Math.min(discRadius(cluster.members.length) + 24, MAX_GROUP_RADIUS),
      };
    });

    // Seed positions: core runtime nodes on a small triangle at the centre
    // (they were previously stacked on one point), mods on a phyllotaxis
    // spiral inside their cluster disc.
    const coreNodes = displayNodes.filter(isCoreNode);
    const corePos = new Map<string, { x: number; y: number }>();
    coreNodes.forEach((node, i) => {
      const angle = (i / Math.max(1, coreNodes.length)) * Math.PI * 2 - Math.PI / 2;
      corePos.set(node.id, {
        x: cx + Math.cos(angle) * (coreNodes.length > 1 ? 90 : 0),
        y: cy + Math.sin(angle) * (coreNodes.length > 1 ? 90 : 0),
      });
    });
    const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));
    const clusterOf = new Map<string, string>();
    for (const cluster of clusters) for (const m of cluster.members) clusterOf.set(m.id, cluster.key);

    simNodes = displayNodes.map((node) => {
      const core = isCoreNode(node);
      const anchor = core
        ? corePos.get(node.id) ?? { x: cx, y: cy }
        : anchors.get(clusterOf.get(node.id) ?? "") ?? { x: cx, y: cy };
      let x = anchor.x;
      let y = anchor.y;
      if (!core) {
        const cluster = clusters.find((c) => c.key === clusterOf.get(node.id));
        const idx = Math.max(0, cluster?.members.findIndex((m) => m.id === node.id) ?? 0);
        const r = Math.min(
          NODE_PITCH * Math.sqrt(idx + 0.5),
          MAX_GROUP_RADIUS - 30,
        );
        const theta = idx * GOLDEN_ANGLE;
        x = anchor.x + Math.cos(theta) * r;
        y = anchor.y + Math.sin(theta) * r;
      }
      const isGhost = node.kind === "Missing";
      let tone: string;
      if (isGhost) tone = "ghost";
      else if (node.kind === "ResourcePack") tone = "resourcepack";
      else if (node.kind === "ShaderPack") tone = "shader";
      else if (node.kind === "Mod") tone = depNodeIds.has(node.id) ? "dep" : String(node.side ?? "both").toLowerCase();
      else if (node.kind === "Profile") tone = "profile";
      else tone = "runtime";

      return {
        ...node,
        label: displayLabel(node),
        x,
        y,
        fx: core ? anchor.x : null,
        fy: core ? anchor.y : null,
        tone,
        ghost: isGhost,
        depth: 0,
        isHub: core,
        component: 0,
        hubId: node.id,
        hubX: anchor.x,
        hubY: anchor.y,
        groupKey: core ? "core" : clusterOf.get(node.id) ?? "qol",
      } as LayoutNode;
    });

    // Soft-reload: reuse prior coordinates so delete doesn't remount the graph.
    if (pendingRestorePositions) {
      let restored = 0;
      for (const n of simNodes) {
        const prev = pendingRestorePositions.get(n.id);
        if (!prev || prev.x == null || prev.y == null) continue;
        n.x = prev.x;
        n.y = prev.y;
        n.hubX = prev.hubX ?? n.hubX;
        n.hubY = prev.hubY ?? n.hubY;
        if (n.isHub) {
          n.fx = prev.fx ?? prev.x;
          n.fy = prev.fy ?? prev.y;
        }
        restored += 1;
      }
      pendingRestorePositions = null;
      resetViewOnNextLayout = false;
    }

    if (simulation) simulation.stop();

    const collideRadius = (d: LayoutNode) => nodeSize(d) / 2 + 30;
    const margin = 100;

    function finalizeLayoutAfterSim() {
      for (const d of simNodes) {
        d.x = Math.max(margin, Math.min(canvasWidth - margin, d.x ?? cx));
        d.y = Math.max(margin, Math.min(canvasHeight - margin, d.y ?? cy));
      }
      groupMeta = groupMeta.map((group) => {
        const members = simNodes.filter((n) => n.groupKey === group.key && !n.isHub);
        if (members.length === 0) return group;
        const gx = members.reduce((s, n) => s + (n.x ?? 0), 0) / members.length;
        const gy = members.reduce((s, n) => s + (n.y ?? 0), 0) / members.length;
        let maxR = 80;
        for (const n of members) {
          const dx = (n.x ?? 0) - gx;
          const dy = (n.y ?? 0) - gy;
          maxR = Math.max(maxR, Math.hypot(dx, dy) + collideRadius(n) + 12);
        }
        return { ...group, x: gx, y: gy, r: Math.min(maxR, MAX_GROUP_RADIUS) };
      });
      updateGraphDom();
      if (resetViewOnNextLayout) {
        fitToContent();
        resetViewOnNextLayout = false;
      }
    }

    // Polish physics on a tidy seed: collide prevents icon/label overlap,
    // soft springs keep mods near their cluster, weak links refine local groups.
    const lowPerf = isLowPerfMode();
    simulation = d3
      .forceSimulation<LayoutNode>(simNodes)
      .force("charge", d3.forceManyBody<LayoutNode>().strength(-260).distanceMax(320))
      .force(
        "link",
        d3
          .forceLink(simLinks.filter((l) => clusterOf.get(linkId(l.source)) === clusterOf.get(linkId(l.target))))
          .id((d: any) => d.id)
          .distance(NODE_PITCH * 1.35)
          .strength(0.06),
      )
      .force(
        "collide",
        d3
          .forceCollide<LayoutNode>()
          .radius(collideRadius)
          .strength(1)
          .iterations(3),
      )
      .force("x", d3.forceX<LayoutNode>((d) => d.hubX).strength((d) => (d.isHub ? 0 : 0.05)))
      .force("y", d3.forceY<LayoutNode>((d) => d.hubY).strength((d) => (d.isHub ? 0 : 0.05)))
      .alpha(0.85)
      .alphaDecay(0.06)
      .velocityDecay(0.45)
      .on("tick", () => {
        for (const d of simNodes) {
          d.x = Math.max(margin, Math.min(canvasWidth - margin, d.x ?? cx));
          d.y = Math.max(margin, Math.min(canvasHeight - margin, d.y ?? cy));
        }
        updateGraphDom();
      })
      .on("end", () => {
        finalizeLayoutAfterSim();
      });
    positioned = simNodes.map((n) => ({ ...n, x: Math.round(n.x), y: Math.round(n.y) }));
    updateGraphDom();
    if (lowPerf) {
      simulation.stop();
      finalizeLayoutAfterSim();
    }
  }

  $effect(() => {
    const key = layoutKey;
    const count = displayNodes.length;
    if (!count || key === simulationLayoutKey) return;
    simulationLayoutKey = key;
    // Defer + untrack: startSimulation writes positioned / camera $state.
    // Tracking those inside this effect re-enters forever under Svelte 5.
    queueMicrotask(() => {
      if (simulationLayoutKey !== key) return;
      untrack(() => startSimulation());
    });
  });

  // Refresh labels when Modrinth metadata arrives — update in place at the
  // fetch site. Do NOT $effect(read positioned → write positioned): that
  // loops forever under Svelte 5 runes (tab hang).
  const positionById = $derived(new Map(positioned.map((node) => [node.id, node])));

  // Reactive edge paths: recomputed whenever `positioned` (and thus
  // `positionById`) changes, so arrows track nodes live during the sim.
  const edgePaths = $derived(renderedEdges.map((edge) => ({
    key: `${edge.from}:${edge.to}:${edge.kind}`,
    edge,
    kind: edge.kind,
    from: edge.from,
    to: edge.to,
    d: edgePath(edge),
    danger: edgeDanger(edge),
    // Fan-in into a heavily-shared library (fabric-api, cloth-config, …): still
    // drawn so the hub reads as a star, but faded so it doesn't drown the graph.
    hub: edge.kind === "Requires" && hubTargetIds.has(edge.to),
  })));

  function edgeKey(edge: GraphEdge): string {
    return `${edge.from}:${edge.to}:${edge.kind}`;
  }

  function registerNode(el: Element, node: PositionedNode) {
    const g = el as SVGGElement;
    nodeEls.set(node.id, g);
    g.style.transform = `translate(${Math.round(node.x)}px, ${Math.round(node.y)}px)`;
  }

  function registerEdge(el: Element, edge: GraphEdge) {
    edgeEls.set(edgeKey(edge), el as SVGPathElement);
  }

  // Imperative DOM update (modrinth-extras pattern): mutate transforms and
  // edge `d` attributes directly each tick instead of rebuilding a reactive
  // array, so arrows stay glued to nodes and dragging is smooth.
  function updateGraphDom() {
    for (const node of simNodes) {
      const el = nodeEls.get(node.id);
      if (el) el.style.transform = `translate(${Math.round(node.x)}px, ${Math.round(node.y)}px)`;
    }
    for (const edge of renderedEdges) {
      const el = edgeEls.get(edgeKey(edge));
      if (el) el.setAttribute("d", edgePath(edge));
    }
  }

  // --- Obsidian-style pan & zoom viewport state ---
  // The canvas itself stays a fixed logical size; instead of resizing the
  // SVG we move/scale a "camera" viewBox over it, exactly like Obsidian's
  // graph view: wheel zooms (toward the cursor), drag empty space to pan,
  // double-click / button to reset.
  let viewportEl = $state<SVGSVGElement>();
  let viewX = $state(0);
  let viewY = $state(0);
  let viewScale = $state(1);
  let isPanning = $state(false);
  let isNodeDragging = $state(false);
  let panStart = { x: 0, y: 0, viewX: 0, viewY: 0 };
  let panPointerId: number | null = null;
  let nodeDragPointerId: number | null = null;
  let endNodeDrag: (() => void) | null = null;
  const viewBoxHeight = $derived(canvasHeight / viewScale);
  const viewBoxWidth = $derived(canvasWidth / viewScale);
  const viewBoxString = $derived(`${viewX} ${viewY} ${viewBoxWidth} ${viewBoxHeight}`);
  const denseGraph = $derived(renderedEdges.length > 70);

  function clientToSvgPoint(clientX: number, clientY: number) {
    // Prefer screen CTM so letterboxed SVG (preserveAspectRatio meet) maps correctly.
    // Manual viewBox math ignores letterboxing and snaps drags toward the canvas center.
    if (viewportEl) {
      const ctm = viewportEl.getScreenCTM();
      if (ctm) {
        const pt = viewportEl.createSVGPoint();
        pt.x = clientX;
        pt.y = clientY;
        const svg = pt.matrixTransform(ctm.inverse());
        return { x: svg.x, y: svg.y };
      }
    }
    const rect = viewportEl?.getBoundingClientRect();
    if (!rect?.width || !rect?.height) return { x: viewX, y: viewY };
    const relX = (clientX - rect.left) / rect.width;
    const relY = (clientY - rect.top) / rect.height;
    return {
      x: viewX + relX * viewBoxWidth,
      y: viewY + relY * viewBoxHeight,
    };
  }

  function screenDeltaToSvg(dx: number, dy: number) {
    const ctm = viewportEl?.getScreenCTM();
    if (ctm && ctm.a && ctm.d) {
      return { x: dx / ctm.a, y: dy / ctm.d };
    }
    const rect = viewportEl?.getBoundingClientRect();
    if (!rect?.width || !rect?.height) return { x: 0, y: 0 };
    return {
      x: (dx / rect.width) * viewBoxWidth,
      y: (dy / rect.height) * viewBoxHeight,
    };
  }

  function handleWheel(event: WheelEvent) {
    // Plain wheel scrolls the tab (details / node list below the canvas).
    // Ctrl/Meta+wheel (or fullscreen) zooms the graph — Obsidian-style.
    if (!(event.ctrlKey || event.metaKey) && !graphFullscreen) {
      return;
    }
    event.preventDefault();
    const zoomFactor = event.deltaY > 0 ? 1.12 : 1 / 1.12;
    const nextScale = Math.min(8, Math.max(0.15, viewScale / zoomFactor));
    const cursor = clientToSvgPoint(event.clientX, event.clientY);
    const nextWidth = canvasWidth / nextScale;
    const nextHeight = canvasHeight / nextScale;
    // Keep the point under the cursor stationary while zooming.
    const ratioX = (cursor.x - viewX) / viewBoxWidth;
    const ratioY = (cursor.y - viewY) / viewBoxHeight;
    viewX = cursor.x - ratioX * nextWidth;
    viewY = cursor.y - ratioY * nextHeight;
    viewScale = nextScale;
  }

  let panMoved = $state(false);

  function endBackgroundPan(event?: PointerEvent) {
    if (!isPanning) return;
    isPanning = false;
    if (
      event &&
      panPointerId != null &&
      viewportEl?.hasPointerCapture?.(panPointerId)
    ) {
      try {
        viewportEl.releasePointerCapture(panPointerId);
      } catch {
        /* already released */
      }
    }
    panPointerId = null;
    window.removeEventListener("pointermove", handleBackgroundPointerMove);
    window.removeEventListener("pointerup", handleBackgroundPointerUp);
    window.removeEventListener("pointercancel", handleBackgroundPointerUp);
  }

  function handleBackgroundPointerDown(event: PointerEvent) {
    if (event.button !== 0) return;
    if (isPanning || isNodeDragging) return;
    // Only pan on empty canvas — nodes stopPropagation on their own pointerdown.
    event.preventDefault();
    isPanning = true;
    panMoved = false;
    panPointerId = event.pointerId;
    panStart = { x: event.clientX, y: event.clientY, viewX, viewY };
    try {
      viewportEl?.setPointerCapture?.(event.pointerId);
    } catch {
      /* ignore */
    }
    window.addEventListener("pointermove", handleBackgroundPointerMove);
    window.addEventListener("pointerup", handleBackgroundPointerUp);
    window.addEventListener("pointercancel", handleBackgroundPointerUp);
  }

  function handleBackgroundPointerMove(event: PointerEvent) {
    if (!isPanning || !viewportEl) return;
    if (panPointerId != null && event.pointerId !== panPointerId) return;
    const delta = screenDeltaToSvg(event.clientX - panStart.x, event.clientY - panStart.y);
    if (Math.abs(event.clientX - panStart.x) > 3 || Math.abs(event.clientY - panStart.y) > 3) {
      panMoved = true;
    }
    viewX = panStart.viewX - delta.x;
    viewY = panStart.viewY - delta.y;
  }

  function handleBackgroundPointerUp(event: PointerEvent) {
    if (panPointerId != null && event.pointerId !== panPointerId) return;
    endBackgroundPan(event);
  }

  function abortGraphPointers() {
    endBackgroundPan();
    endNodeDrag?.();
  }

  function fitToContent(padding = 80) {
    const fitNodes = simNodes.length ? simNodes : positioned;
    if (!fitNodes.length) {
      viewX = 0;
      viewY = 0;
      viewScale = 1;
      return;
    }
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const node of fitNodes) {
      const half = nodeSize(node) / 2 + 28;
      minX = Math.min(minX, node.x - half);
      minY = Math.min(minY, node.y - half);
      maxX = Math.max(maxX, node.x + half);
      maxY = Math.max(maxY, node.y + half + 18);
    }
    // Include group halos and their labels so a fit never crops a halo or
    // leaves a category label outside the viewport.
    for (const group of groupMeta) {
      minX = Math.min(minX, group.x - group.r);
      maxX = Math.max(maxX, group.x + group.r);
      minY = Math.min(minY, group.y - group.r - 18 - 14);
      maxY = Math.max(maxY, group.y + group.r);
    }
    const contentW = Math.max(160, maxX - minX);
    const contentH = Math.max(160, maxY - minY);
    const targetW = contentW + padding * 2;
    const targetH = contentH + padding * 2;
    // Cap like modrinth-extras (max ~1.2–1.5) so we never blow past readable size.
    const scale = Math.min(canvasWidth / targetW, canvasHeight / targetH, 1.35);
    viewScale = Math.min(4, Math.max(0.2, scale));
    const vbW = canvasWidth / viewScale;
    const vbH = canvasHeight / viewScale;
    viewX = Math.round((minX + maxX) / 2 - vbW / 2);
    viewY = Math.round((minY + maxY) / 2 - vbH / 2);
  }

  function resetView() {
    fitToContent();
  }

  function zoomBy(factor: number) {
    const centerX = viewX + viewBoxWidth / 2;
    const centerY = viewY + viewBoxHeight / 2;
    const nextScale = Math.min(8, Math.max(0.2, viewScale * factor));
    const nextWidth = canvasWidth / nextScale;
    const nextHeight = canvasHeight / nextScale;
    viewX = centerX - nextWidth / 2;
    viewY = centerY - nextHeight / 2;
    viewScale = nextScale;
  }

  async function toggleFullscreen() {
    try {
      if (graphFullscreen) {
        await document.exitFullscreen();
      } else {
        const el = graphCanvasEl;
        if (el) await el.requestFullscreen();
      }
      resetView();
    } catch (e) {
      error = `Fullscreen mode is unavailable: ${String(e)}`;
    }
  }

  onMount(() => {
    const onBlur = () => abortGraphPointers();
    const onVis = () => {
      if (document.hidden) {
        abortGraphPointers();
        pauseSimulation();
      }
    };
    window.addEventListener("blur", onBlur);
    document.addEventListener("visibilitychange", onVis);

    void listen<DownloadProgress>("mod-download-progress", (event) => {
      if (depInstallStatus !== "downloading") return;
      depInstallBatchSeen = true;
      const item = event.payload;
      depInstallMessage = item.status === "downloading"
        ? `Downloading ${item.name}… ${Math.round(item.percent)}%`
        : `${item.name}: ${item.status}`;
    }).then((unlisten) => {
      unlistenDownloadProgress = unlisten;
    });
    void listen<DownloadBatch>("mod-download-batch", (event) => {
      if (depInstallStatus !== "downloading") return;
      depInstallBatchSeen = true;
      if (event.payload.phase !== "done") return;
      const failures = event.payload.failed ?? [];
      if (failures.length > 0) {
        depInstallStatus = "failed";
        depInstallFailedIds = failures.map((failure) => failure.modId);
        depInstallError = failures.map((failure) => `${failure.modId}: ${failure.error}`).join("\n");
        depInstallMessage = `Download failed for ${failures.length} file${failures.length > 1 ? "s" : ""}.`;
      } else {
        depInstallStatus = "done";
        depInstallMessage = `${depPreviewName} and its dependencies are installed.`;
        message = `Installed ${depPreviewName}.`;
        void load(true);
      }
    }).then((unlisten) => {
      unlistenDownloadBatch = unlisten;
    });

    return () => {
      window.removeEventListener("blur", onBlur);
      document.removeEventListener("visibilitychange", onVis);
      abortGraphPointers();
    };
  });

  onDestroy(() => {
    abortGraphPointers();
    unlistenDownloadProgress?.();
    unlistenDownloadBatch?.();
    canvasResizeObserver?.disconnect();
    canvasResizeObserver = null;
    simulation?.stop();
  });

  $effect(() => {
    if (graphCanvasEl && typeof ResizeObserver !== "undefined" && !canvasResizeObserver) {
        canvasResizeObserver = new ResizeObserver(() => {
          // SVG fills the flex canvas via width/height 100%; observer keeps layout in sync on stage resize.
        });
        canvasResizeObserver.observe(graphCanvasEl);
      }
  });

  $effect(() => {
    if ($projectPath && lastLoadedPath !== $projectPath) void load();
  });
  function handleNodeMouseDown(event: PointerEvent, node: PositionedNode) {
    event.stopPropagation();
    if (event.button !== 0) return;
    event.preventDefault();
    selectedId = node.id;
    if (!simulation || isNodeDragging) return;
    const sim = simNodes.find((n) => n.id === node.id);
    if (!sim) return;
    // Don't start a canvas pan while dragging a node.
    endBackgroundPan();
    const layoutNode = sim;
    const keepPinned = !!layoutNode.isHub;
    const pointerId = event.pointerId;
    const target = event.currentTarget as Element | null;
    isNodeDragging = true;
    nodeDragPointerId = pointerId;
    const grab = clientToSvgPoint(event.clientX, event.clientY);
    const grabOffsetX = grab.x - (layoutNode.x ?? 0);
    const grabOffsetY = grab.y - (layoutNode.y ?? 0);
    layoutNode.fx = layoutNode.x;
    layoutNode.fy = layoutNode.y;
    try {
      target?.setPointerCapture?.(pointerId);
    } catch {
      /* ignore */
    }

    const onPointerMove = (ev: PointerEvent) => {
      if (ev.pointerId !== pointerId) return;
      const p = clientToSvgPoint(ev.clientX, ev.clientY);
      const nextX = p.x - grabOffsetX;
      const nextY = p.y - grabOffsetY;
      layoutNode.fx = nextX;
      layoutNode.fy = nextY;
      layoutNode.x = nextX;
      layoutNode.y = nextY;
      if (keepPinned) {
        layoutNode.hubX = nextX;
        layoutNode.hubY = nextY;
        // Only core/cluster-root nodes share this hub position; category groups
        // keep their own anchors, so we must NOT drag every mod along with a hub.
        for (const other of simNodes) {
          if (other.groupKey === layoutNode.groupKey && other.id !== layoutNode.id) {
            other.hubX = nextX;
            other.hubY = nextY;
          }
        }
      }
      simulation?.alpha(0.1).restart();
    };

    const finish = (ev?: PointerEvent) => {
      if (ev && ev.pointerId !== pointerId) return;
      // Hubs stay pinned (modrinth-extras root behavior); others rejoin the sim.
      if (!keepPinned) {
        layoutNode.fx = null;
        layoutNode.fy = null;
      }
      simulation?.alphaTarget(0);
      if (isLowPerfMode()) {
        simulation?.stop();
      }
      if (target?.hasPointerCapture?.(pointerId)) {
        try {
          target.releasePointerCapture(pointerId);
        } catch {
          /* already released */
        }
      }
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
      isNodeDragging = false;
      nodeDragPointerId = null;
      endNodeDrag = null;
    };

    endNodeDrag = () => finish();
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
  }
</script>

<svelte:document bind:fullscreenElement />

<div class="graph">
  <div class="toolbar">
    <div class="title">
      <GitGraph size={18} />
      <span>Dependency graph</span>
    </div>
    <div class="toolbar-actions">
      <button class="secondary" onclick={installMissingDependencies} disabled={!$projectPath || resolving || !hasMissingSpotlight}>
        <Workflow size={16} />
        {resolving ? "Resolving..." : "Auto-install dependencies"}
      </button>
      <button class="ghost" onclick={() => refreshGraph(true)} title="Refresh dependency metadata" disabled={!$projectPath || loading || graphRefreshing}>
        <RefreshCw size={16} class={graphRefreshing ? "spin" : ""} />
      </button>
    </div>
  </div>

  {#if message}<div class="notice success">{message}</div>{/if}
  {#if planError}<div class="notice plan-error" role="alert">{planError}</div>{/if}
  {#if graph}
    <div class="graph-status" class:stale={graphSource === "local"} class:error={!!refreshError}>
      {#if graphRefreshing}
        <Loader2 size={13} class="spin" /> Updating dependencies in the background…
      {:else if refreshError}
        Offline graph shown. Refresh failed: {refreshError}
      {:else if graphSource === "local"}
        Local graph shown; network metadata has not been cached yet.
      {:else}
        {graphSource === "cache" ? "Cached dependency graph" : "Current dependency graph"}
        {graphGeneratedAt ? ` · ${new Date(graphGeneratedAt).toLocaleString()}` : ""}
      {/if}
    </div>
  {/if}

  {#if loading && !graph}
    <div class="graph-body">
      <section class="graph-canvas" aria-busy="true" aria-label="Dependency graph canvas">
        <div class="graph-loading-overlay" role="status">
          <Loader2 size={20} class="spin" />
          <span>Loading graph…</span>
          <button type="button" class="ghost mini" onclick={cancelGraphLoad}>Cancel</button>
        </div>
      </section>
    </div>
  {:else if error}
    <EmptyState icon={AlertTriangle} title="Failed to load graph" description={error} />
  {:else if graph}
    <div class="stats stats-compact">
      <div class="stat-card accent">
        <span class="stat-value">{modNodes.length}</span>
        <span class="stat-label">Mods</span>
      </div>
      <div class="stat-card">
        <span class="stat-value">{displayEdges.length}</span>
        <span class="stat-label">Dependencies</span>
      </div>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -- the card is a real focusable button (role="button" + tabindex="0" + Enter/Space keydown) only when hasMissingSpotlight is set; svelte cannot see the conditional -->
      <div
        class="stat-card"
        class:danger={hasMissingSpotlight}
        class:clickable={hasMissingSpotlight}
        role={hasMissingSpotlight ? "button" : undefined}
        tabindex={hasMissingSpotlight ? 0 : undefined}
        onclick={() => hasMissingSpotlight && toggleHighlight("missing")}
        onkeydown={(e) => {
          if (!hasMissingSpotlight) return;
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            toggleHighlight("missing");
          }
        }}
        title={hasMissingSpotlight ? "Spotlight missing dependencies" : undefined}
      >
        <span class="stat-value">{Math.max(missingEdges.length, ghostNodes.length)}</span>
        <span class="stat-label">Missing</span>
      </div>
      <div class="stat-card" class:danger={conflictInsights.length > 0}>
        <span class="stat-value">{conflictInsights.length}</span>
        <span class="stat-label">Conflicts</span>
      </div>
    </div>

    <div class="graph-body">
    <section
      bind:this={graphCanvasEl}
      class="graph-canvas"
      class:fullscreen={graphFullscreen}
      class:hl-conflicts={highlightMode === "conflicts"}
      class:hl-missing={highlightMode === "missing"}
      aria-label="Dependency graph canvas"
    >
      <div class="canvas-controls">
        <button class="ghost mini" onclick={() => zoomBy(1.25)} title="Zoom in (Ctrl+scroll)">+</button>
        <button class="ghost mini" onclick={() => zoomBy(1 / 1.25)} title="Zoom out (Ctrl+scroll)">−</button>
        <button class="ghost mini" onclick={resetView} title="Fit graph to view">⤢</button>
        <button
          class="ghost mini edge-toggle"
          class:active={showAllEdges}
          onclick={() => (showAllEdges = !showAllEdges)}
          title={showAllEdges
            ? "Hide optional / hub / runtime edges"
            : "Show optional, hub fan-in and runtime edges"}
        >
          {showAllEdges ? "Fewer edges" : "More edges"}
        </button>
        <button
          class="ghost mini edge-toggle hl-btn conflicts"
          class:active={highlightMode === "conflicts"}
          disabled={conflictEdges.length === 0}
          onclick={() => toggleHighlight("conflicts")}
          title="Spotlight conflict edges (high contrast)"
        >
          <ShieldAlert size={14} /> Conflicts
        </button>
        <button
          class="ghost mini edge-toggle hl-btn missing"
          class:active={highlightMode === "missing"}
          disabled={!hasMissingSpotlight}
          onclick={() => toggleHighlight("missing")}
          title="Spotlight missing deps and show which mod requires each"
        >
          <AlertTriangle size={14} /> Missing
        </button>
        <button class="ghost mini" onclick={toggleFullscreen} title={graphFullscreen ? "Exit fullscreen" : "Open fullscreen"}>
          {#if graphFullscreen}<Minimize2 size={14} />{:else}<Maximize2 size={14} />{/if}
        </button>
        <span class="zoom-readout">{Math.round(viewScale * 100)}%</span>
      </div>
      <div class="graph-legend" class:collapsed={!legendExpanded}>
        <button type="button" class="legend-toggle" onclick={toggleLegend} aria-expanded={legendExpanded}>
          <span>Legend</span>
          <ChevronDown size={14} class={legendExpanded ? "rot" : ""} />
        </button>
        {#if legendExpanded}
          <div class="legend-body">
            <div class="legend-block">
              <span class="legend-title">Edges</span>
              <span class="legend-row"><i class="lg-line hard"></i> Hard dependency</span>
              <span class="legend-row"><i class="lg-line conflict"></i> Conflict</span>
              <span class="legend-row"><i class="lg-line optional"></i> Optional</span>
              <span class="legend-row"><i class="lg-line runtime"></i> Loader / runtime</span>
            </div>
            <div class="legend-block">
              <span class="legend-title">Nodes</span>
              <span class="legend-row"><i class="lg-dot client"></i> Client</span>
              <span class="legend-row"><i class="lg-dot server"></i> Server</span>
              <span class="legend-row"><i class="lg-dot both"></i> Both</span>
              <span class="legend-row"><i class="lg-dot dep"></i> Dependency</span>
              <span class="legend-row"><i class="lg-dot resourcepack"></i> Resource pack</span>
              <span class="legend-row"><i class="lg-dot shader"></i> Shader</span>
              <span class="legend-row"><i class="lg-dot missing"></i> Missing</span>
              <span class="legend-row"><i class="lg-dot requirer"></i> Needs missing</span>
            </div>
            <div class="legend-block legend-groups">
              <span class="legend-title">Clusters</span>
              {#each groupMeta as group (group.key)}
                <button
                  class="legend-chip"
                  class:on={hoveredGroup === group.key}
                  style={`--chip:${group.color}`}
                  onmouseenter={() => (hoveredGroup = group.key)}
                  onmouseleave={() => (hoveredGroup = null)}
                  onclick={(e) => { e.stopPropagation(); (hoveredGroup = hoveredGroup === group.key ? null : group.key); } }
                >{group.label}</button>
              {/each}
            </div>
          </div>
        {/if}
      </div>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -- svg canvas handles wheel/pointer/click/dblclick/keydown; role="img" is non-interactive -->
      <svg
        bind:this={viewportEl}
        viewBox={viewBoxString}
        role="img"
        aria-label="Dependency graph"
        class:panning={isPanning}
        class:dragging={isNodeDragging}
        onwheel={handleWheel}
        onpointerdown={handleBackgroundPointerDown}
        onclick={() => {
          // Click on empty canvas (not a node — those stopPropagation) clears
          // the current selection, but only if the user wasn't panning.
          if (!panMoved && !isNodeDragging) selectedId = null;
        }}
        ondblclick={resetView}
        onkeydown={() => {}}
      >
        <defs>
          <marker id="arrow" markerWidth="8" markerHeight="8" refX="7" refY="3" orient="auto" markerUnits="userSpaceOnUse">
            <path d="M0,0 L0,6 L7,3 z" fill="rgba(161,161,170,.75)" />
          </marker>
          <marker id="arrow-danger" markerWidth="8" markerHeight="8" refX="7" refY="3" orient="auto" markerUnits="userSpaceOnUse">
            <path d="M0,0 L0,6 L7,3 z" fill="rgba(239,68,68,.95)" />
          </marker>
          <marker id="arrow-danger-hot" markerWidth="12" markerHeight="12" refX="10" refY="4.5" orient="auto" markerUnits="userSpaceOnUse">
            <path d="M0,0 L0,9 L10,4.5 z" fill="#ff3b3b" />
          </marker>
          <marker id="arrow-missing-hot" markerWidth="12" markerHeight="12" refX="10" refY="4.5" orient="auto" markerUnits="userSpaceOnUse">
            <path d="M0,0 L0,9 L10,4.5 z" fill="#fbbf24" />
          </marker>
          <marker id="arrow-optional" markerWidth="8" markerHeight="8" refX="7" refY="3" orient="auto" markerUnits="userSpaceOnUse">
            <path d="M0,0 L0,6 L7,3 z" fill="rgba(132,204,22,.85)" />
          </marker>
          <marker id="arrow-runtime" markerWidth="8" markerHeight="8" refX="7" refY="3" orient="auto" markerUnits="userSpaceOnUse">
            <path d="M0,0 L0,6 L7,3 z" fill="rgba(161,161,170,.45)" />
          </marker>
          <filter id="glow-conflict" x="-80%" y="-80%" width="260%" height="260%">
            <feGaussianBlur stdDeviation="3.5" result="b" />
            <feMerge>
              <feMergeNode in="b" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
          <filter id="glow-missing" x="-80%" y="-80%" width="260%" height="260%">
            <feGaussianBlur stdDeviation="3.5" result="b" />
            <feMerge>
              <feMergeNode in="b" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>
        {#each groupMeta as group (group.key)}
          <g
            class="graph-group"
            class:active={hoveredGroup === group.key}
            class:dim={hoveredGroup !== null && hoveredGroup !== group.key}
            style={`color:${group.color}`}
            role="group"
            aria-label={group.label}
            onmouseenter={() => (hoveredGroup = group.key)}
            onmouseleave={() => (hoveredGroup = null)}
          >
            <circle class="group-halo" cx={group.x} cy={group.y} r={group.r} />
            <text class="group-label" x={group.x} y={group.y - group.r - 18} text-anchor="middle">{group.label}</text>
          </g>
        {/each}
        {#each edgePaths as ep (ep.key)}
          {@const inHl = !!highlightMode && edgeInHighlight(ep.from, ep.to, ep.kind, ep.danger)}
          <path
            use:registerEdge={ep.edge}
            class="graph-edge"
            class:dense={denseGraph}
            class:danger-edge={ep.danger}
            class:optional-edge={ep.kind === "Optional"}
            class:hub-edge={ep.hub && !ep.danger}
            class:runtime-edge={["RequiresLoader", "RequiresMinecraft", "RequiresJava"].includes(ep.kind)}
            class:dimmed={edgeIsDimmed(ep.from, ep.to, ep.kind, ep.danger)}
            class:spotlight={inHl}
            class:spotlight-conflicts={inHl && highlightMode === "conflicts"}
            class:spotlight-missing={inHl && highlightMode === "missing"}
            d={ep.d}
            marker-end={
              inHl && highlightMode === "conflicts"
                ? "url(#arrow-danger-hot)"
                : inHl && highlightMode === "missing"
                  ? "url(#arrow-missing-hot)"
                  : ep.danger
                    ? "url(#arrow-danger)"
                    : ep.kind === "Optional"
                      ? "url(#arrow-optional)"
                      : ["RequiresLoader", "RequiresMinecraft", "RequiresJava"].includes(ep.kind)
                        ? "url(#arrow-runtime)"
                        : "url(#arrow)"
            }
          />
        {/each}
        {#each positioned as node (node.id)}
          {@const size = nodeSize(node)}
          {@const half = size / 2}
          {@const icon = nodeIconUrl(node)}
          {@const isGhost = node.kind === "Missing" || node.ghost}
          {@const isInstalledDep = node.kind === "Mod" && depNodeIds.has(node.id)}
          {@const isClickableDep = isGhost || isInstalledDep}
          {@const inHl = !!highlightMode && nodeInHighlight(node.id, node.kind)}
          {@const isMissingRequirer = highlightMode === "missing" && missingRequirerIds.has(node.id)}
          <g
            use:registerNode={node}
            class="svg-node tone-{node.tone}"
            class:selected={selectedId === node.id}
            class:clickable-dep={isInstalledDep}
            class:dimmed={nodeIsDimmed(node)}
            class:spotlight={inHl}
            class:spotlight-conflicts={inHl && highlightMode === "conflicts"}
            class:spotlight-missing={inHl && highlightMode === "missing"}
            class:spotlight-requirer={isMissingRequirer}
            role="button"
            tabindex="0"
            onpointerdown={(e) => handleNodeMouseDown(e, node)}
            onclick={(e) => { e.stopPropagation(); handleNodeClick(node); } }
            oncontextmenu={(e) => { e.stopPropagation(); void onNodeContextInstall(e, node); } }
            onkeydown={(e) => e.key === "Enter" && handleNodeClick(node)}
            aria-label={node.label}
          >
            {#if inHl}
              <circle
                class="spotlight-ring"
                class:conflicts={highlightMode === "conflicts"}
                class:missing={highlightMode === "missing"}
                r={half + 14}
              />
            {/if}
            {#if isClickableDep}
              <g
                class="dep-icon-hit"
                role="button"
                tabindex="0"
                onclick={(e) => { e.stopPropagation(); handleDepIconClick(node); } }
                onkeydown={(e) => e.key === "Enter" && handleDepIconClick(node)}
                aria-label={isGhost ? `Install ${node.label}` : `Re-download ${node.label}`}
              >
                <rect x={-half} y={-half} width={size} height={size} rx="8" ry="8" />
                {#if icon}
                  <clipPath id="clip-{node.id.replace(/[^a-zA-Z0-9]/g, '_')}">
                    <rect x={-half + 2} y={-half + 2} width={size - 4} height={size - 4} rx="6" ry="6" />
                  </clipPath>
                  <image
                    href={icon}
                    x={-half + 2}
                    y={-half + 2}
                    width={size - 4}
                    height={size - 4}
                    clip-path={`url(#clip-${node.id.replace(/[^a-zA-Z0-9]/g, '_')})`}
                    preserveAspectRatio="xMidYMid slice"
                    onerror={() => handleIconError(node)}
                  />
                {:else}
                  <text class="fallback-letter" y="5" text-anchor="middle">{node.label?.[0]?.toUpperCase() ?? "?"}</text>
                {/if}
              </g>
            {:else}
              <rect x={-half} y={-half} width={size} height={size} rx="8" ry="8" />
              {#if icon}
                <clipPath id="clip-{node.id.replace(/[^a-zA-Z0-9]/g, '_')}">
                  <rect x={-half + 2} y={-half + 2} width={size - 4} height={size - 4} rx="6" ry="6" />
                </clipPath>
                <image
                  href={icon}
                  x={-half + 2}
                  y={-half + 2}
                  width={size - 4}
                  height={size - 4}
                  clip-path={`url(#clip-${node.id.replace(/[^a-zA-Z0-9]/g, '_')})`}
                  preserveAspectRatio="xMidYMid slice"
                  onerror={() => handleIconError(node)}
                />
              {:else}
                <text class="fallback-letter" y="5" text-anchor="middle">{node.label?.[0]?.toUpperCase() ?? "?"}</text>
              {/if}
            {/if}
            {#if isGhost}
              <text class="ghost-download" y={half + 14} text-anchor="middle">⬇ {selectedId === node.id || node.label.length <= 14 ? node.label : node.label.slice(0, 13) + "…"}</text>
            {:else}
              <text class="node-label-text" y={half + 14} text-anchor="middle">{selectedId === node.id || node.label.length <= 18 ? node.label : node.label.slice(0, 17) + "…"}</text>
              <g class="remove-btn" role="button" tabindex="-1" aria-label="Remove mod" onpointerdown={(e) => e.stopPropagation()} onclick={(e) => { e.stopPropagation(); removeConflictNode(node.id); } } onkeydown={(e) => { e.stopPropagation(); if (e.key === "Enter") removeConflictNode(node.id); } }>
                <circle cx={half - 2} cy={-half + 2} r="8" />
                <text x={half - 2} y={-half + 6} text-anchor="middle" class="remove-x">×</text>
              </g>
            {/if}
          </g>
        {/each}
      </svg>
      {#if loading}
        <div class="graph-loading-overlay" role="status">
          <Loader2 size={20} class="spin" />
          <span>Loading graph…</span>
          <button type="button" class="ghost mini" onclick={cancelGraphLoad}>Cancel</button>
        </div>
      {/if}
    </section>

    <aside class="details" aria-label="Selected node">
        <div class="details-header">
          <div>
            <span class="eyebrow">{highlightMode === "missing" ? "Missing map" : "Selected node"}</span>
            {#if highlightMode === "missing"}
              <h2>{missingDemandList.length} mod{missingDemandList.length === 1 ? "" : "s"} need deps</h2>
            {:else if selected}
              <h2>{selected.label}</h2>
            {:else}
              <h2 class="details-placeholder">None selected</h2>
            {/if}
          </div>
          {#if highlightMode === "missing"}
            <span class="tag missing-tag">Missing</span>
          {:else if selected}
            <span class="tag">{selected.kind}</span>
          {/if}
        </div>

        {#if highlightMode === "missing"}
          {#if missingDemandList.length === 0}
            <div class="muted-box subtle-hint">
              <Info size={15} />
              <span>No installed mod → missing dependency links found.</span>
            </div>
          {:else}
            <p class="missing-map-hint">Solid amber = mod that asks · dashed = missing dep · arrow = requires</p>
            <div class="missing-map">
              {#each missingDemandList as row (row.fromId)}
                <div class="missing-map-row" class:active={selectedId === row.fromId || row.deps.some((d) => d.id === selectedId)}>
                  <button
                    type="button"
                    class="missing-map-mod"
                    class:active={selectedId === row.fromId}
                    title="Focus {row.fromLabel}"
                    onclick={() => (selectedId = row.fromId)}
                  >
                    <strong>{row.fromLabel}</strong>
                    <span>needs {row.deps.length}</span>
                  </button>
                  <ul class="missing-map-deps">
                    {#each row.deps as dep (dep.id)}
                      <li>
                        <button
                          type="button"
                          class="missing-map-dep"
                          class:active={selectedId === dep.id}
                          title="Focus {dep.label}"
                          onclick={() => (selectedId = dep.id)}
                        >
                          {dep.label}
                        </button>
                        <button
                          type="button"
                          class="secondary mini"
                          title="Install {dep.label}"
                          onclick={() => installGhostNode(dep.id)}
                          disabled={resolving}
                        >
                          <Download size={12} />
                        </button>
                      </li>
                    {/each}
                  </ul>
                </div>
              {/each}
            </div>
          {/if}
        {:else if selected}
          <div class="details-actions">
            {#if catalogResultFromNode(selected)}
              <button type="button" class="secondary mini details-action-btn" onclick={openSelectedInLauncher}>
                <ExternalLink size={14} />
                Open page
              </button>
            {/if}
            {#if selected.kind === "Missing"}
              <button class="install-btn" onclick={() => installGhostNode(selected.id)} disabled={resolving}>
                <Download size={16} />
                {resolving ? "Installing..." : "Install from Modrinth"}
              </button>
            {:else if isModOrPackKind(selected.kind)}
              {#if selected.kind === "Mod" && depNodeIds.has(selected.id)}
                <button class="secondary mini details-action-btn" onclick={downloadMissingFiles} disabled={resolving}>
                  <Download size={14} />
                  Re-download files
                </button>
              {/if}
              {#if selectedMissingDeps.length > 0}
                <button class="install-btn" onclick={installSelectedMissingDeps} disabled={resolving}>
                  <Download size={16} />
                  {resolving ? "Installing..." : `Install all ${selectedMissingDeps.length} missing`}
                </button>
              {/if}
              <button class="remove-btn-panel" onclick={() => removeConflictNode(selected.id)} disabled={resolving}>
                <X size={16} />
                {selected.kind === "ResourcePack"
                  ? "Remove resource pack"
                  : selected.kind === "ShaderPack"
                    ? "Remove shader"
                    : "Remove mod"}
              </button>
            {:else}
              <button class="remove-btn-panel" onclick={() => removeConflictNode(selected.id)} disabled={resolving}>
                <X size={16} />
                Remove mod
              </button>
            {/if}
          </div>

          <div class="details-grid">
            <div><span>ID</span><code>{selected.id}</code></div>
            <div><span>Version</span><code>{selected.version ?? "—"}</code></div>
            <div><span>Side</span><code>{selected.side ?? "—"}</code></div>
            <div><span>Relations</span><code>{selectedEdges.length}</code></div>
          </div>

          {#if selected.metadata && Object.keys(selected.metadata).length > 0}
            <h3>Metadata</h3>
            <div class="kv">
              {#each Object.entries(selected.metadata) as [key, value] (key)}
                <span>{key}</span><code>{value}</code>
              {/each}
            </div>
          {/if}

          <h3>Relations</h3>
          {#if selectedEdges.length === 0}
            <div class="muted-box">No direct relations.</div>
          {:else}
            <div class="relations">
              {#each selectedEdges as edge (`${edge.from}:${edge.to}:${edge.kind}`)}
                {@const otherId = edge.from === selectedId ? edge.to : edge.from}
                {@const otherNode = nodeById(otherId) ?? ghostNodes.find((n) => n.id === otherId) ?? null}
                {@const isMissingDep =
                  edge.kind === "Requires" &&
                  edge.from === selectedId &&
                  (!otherNode || otherNode.kind === "Missing")}
                <div class="relation" class:incoming={edge.to === selectedId}>
                  <span class="relation-kind">{edge.kind}</span>
                  <span class="relation-text">
                    {edge.from === selectedId ? "requires" : "required by"}
                    <strong>{resolveNodeLabel(otherId)}</strong>
                  </span>
                  {#if edge.reason}<small>{edge.reason}</small>{/if}
                  {#if isMissingDep}
                    <button class="secondary mini" onclick={() => installGhostNode(otherId)} disabled={resolving}>
                      <Download size={12} /> Install
                    </button>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        {:else}
          <div class="muted-box subtle-hint">
            <Info size={15} />
            <span>Pick a node on the graph to see what it requires and what requires it.</span>
          </div>
        {/if}
      </aside>

    {#if changePlanLoading && !changePlan}
      <section class="change-plan-panel loading">
        <div class="change-plan-head">
          <span class="eyebrow">Change plan</span>
          <strong class="change-plan-summary muted">Resolving dependencies…</strong>
        </div>
      </section>
    {:else if changePlan}
      <section class="change-plan-panel" class:expanded={changePlanExpanded}>
        <button
          type="button"
          class="change-plan-toggle"
          onclick={() => (changePlanExpanded = !changePlanExpanded)}
          aria-expanded={changePlanExpanded}
        >
          <div class="change-plan-head">
            <span class="eyebrow">Change plan</span>
            <strong class="change-plan-summary">{changePlan.summary}</strong>
            <span class="change-plan-risk" class:req={changePlan.requiresSnapshot}>
              {changePlan.requiresSnapshot ? "snapshot required" : "no snapshot"} · risk {changePlan.risk}
            </span>
          </div>
          <ChevronDown size={16} class={changePlanExpanded ? "rot" : ""} />
        </button>
        {#if changePlanExpanded}
          <ul class="change-plan-list">
            {#if changePlan.actions?.length}
              {#each changePlan.actions as action, index (index)}
                <li>
                  <button
                    type="button"
                    class="change-plan-item"
                    onclick={() => applyAction(index)}
                    disabled={resolving}
                    title={formatChangeAction(action)}
                  >
                    <span class="change-plan-item-idx">{index + 1}</span>
                    <span class="change-plan-item-text">{formatChangeAction(action)}</span>
                  </button>
                </li>
              {/each}
            {:else}
              <li class="change-plan-empty">No pending actions.</li>
            {/if}
          </ul>
          <div class="change-plan-actions">
            <button class="primary mini" onclick={applyChangePlan} disabled={resolving}>
              {changePlan.actions?.length ? "Apply full plan" : "Mark reviewed"}
            </button>
          </div>
        {/if}
      </section>
    {/if}

    <div class="list-toggle-row">
      <button type="button" class="ghost mini list-toggle-btn" onclick={() => (showNodeList = !showNodeList)}>
        <List size={14} />
        {showNodeList ? "Hide list" : "Show list"}
        {#if !showNodeList}
          <span class="list-toggle-meta">
            ({modNodes.length} mod{modNodes.length === 1 ? "" : "s"}{resourcePackNodes.length > 0 ? `, ${resourcePackNodes.length} resource pack${resourcePackNodes.length === 1 ? "" : "s"}` : ""}{shaderPackNodes.length > 0 ? `, ${shaderPackNodes.length} shader${shaderPackNodes.length === 1 ? "" : "s"}` : ""}{ghostNodes.length > 0 ? `, ${ghostNodes.length} missing` : ""})
          </span>
        {/if}
      </button>
    </div>

    {#if showNodeList}
    <div class="graph-list">
      <section class="node-column mods-column">
        <h3><Box size={16} /> Mods ({modNodes.length})</h3>

        {#if ghostNodes.length > 0}
          <div class="missing-column">
            <h3><Download size={16} /> Missing dependencies ({ghostNodes.length})</h3>
            <p class="column-hint">Click or right-click a row to install from Modrinth.</p>
            <div class="mod-grid">
              {#each ghostNodes as node (node.id)}
                {@const icon = !brokenIcons.has(node.id) ? nodeIconUrl(node) : null}
                <div
                  class="node-card compact missing-card"
                  class:selected={selectedId === node.id}
                  role="button"
                  tabindex="0"
                  title="Install {node.label}"
                  onclick={() => installGhostNode(node.id)}
                  oncontextmenu={(e) => onNodeContextInstall(e, node)}
                  onkeydown={(e) => (e.key === "Enter" || e.key === " ") && installGhostNode(node.id)}
                >
                  {#if icon}
                    <img class="card-icon" src={icon} alt="" loading="lazy" onerror={() => handleIconError(node)} />
                  {:else}
                    <span class="card-icon-fallback missing-fallback">{node.label?.[0]?.toUpperCase() ?? "?"}</span>
                  {/if}
                  <div class="card-text">
                    <span class="node-label">{node.label}</span>
                    <span class="node-meta">{ghostMeta[node.id]?.description ?? "Required · not installed"}</span>
                  </div>
                  <button
                    class="card-install-btn"
                    type="button"
                    title="Install {node.label}"
                    onclick={(e) => { e.stopPropagation(); installGhostNode(node.id); } }
                    disabled={resolving}
                  >
                    <Download size={14} strokeWidth={2.25} />
                    <span>Install</span>
                  </button>
                </div>
              {/each}
            </div>
          </div>
        {/if}

        {#if shaderPackNodes.length > 0}
          <div class="pack-column">
            <h3>Shaders ({shaderPackNodes.length})</h3>
            <div class="mod-grid">
              {#each shaderPackNodes.slice().sort((a, b) => a.label.localeCompare(b.label)) as node (node.id)}
                {@const icon = !brokenIcons.has(node.id) ? nodeIconUrl(node) : null}
                {@const missingDeps = missingDepsByMod.get(node.id) ?? []}
                <div
                  class="node-card compact shader-card"
                  class:selected={selectedId === node.id}
                  role="button"
                  tabindex="0"
                  onclick={() => (selectedId = node.id)}
                  oncontextmenu={(e) => onNodeContextInstall(e, node)}
                  onkeydown={(event) => (event.key === "Enter" || event.key === " ") && (selectedId = node.id)}
                >
                  {#if icon}
                    <img class="card-icon" src={icon} alt="" loading="lazy" onerror={() => handleIconError(node)} />
                  {:else}
                    <span class="card-icon-fallback">{node.label?.[0]?.toUpperCase() ?? "?"}</span>
                  {/if}
                  <div class="card-text">
                    <span class="node-label">{node.label}</span>
                    <span class="node-meta">{node.version ?? "unknown"}{missingDeps.length > 0 ? ` · ${missingDeps.length} missing` : ""}</span>
                  </div>
                  {#if missingDeps.length > 0}
                    <div class="card-missing-list">
                      {#each missingDeps as edge (edge.to)}
                        <button
                          class="card-install-btn"
                          type="button"
                          title="Install {resolveNodeLabel(edge.to)}"
                          onclick={(e) => {
                            e.stopPropagation();
                            void installSingleMissingDep(edge);
                          }}
                          disabled={resolving}
                        >
                          <Download size={14} strokeWidth={2.25} />
                          <span>Install {resolveNodeLabel(edge.to)}</span>
                        </button>
                      {/each}
                    </div>
                  {/if}
                  <span class="card-remove" role="button" tabindex="0" title="Remove shader" onclick={(e) => { e.stopPropagation(); removeConflictNode(node.id); } } onkeydown={(e) => { e.stopPropagation(); if (e.key === "Enter") void removeConflictNode(node.id); } }>
                    <X size={14} />
                  </span>
                </div>
              {/each}
            </div>
          </div>
        {/if}

        {#if resourcePackNodes.length > 0}
          <div class="pack-column">
            <h3>Resource packs ({resourcePackNodes.length})</h3>
            <div class="mod-grid">
              {#each resourcePackNodes.slice().sort((a, b) => a.label.localeCompare(b.label)) as node (node.id)}
                {@const icon = !brokenIcons.has(node.id) ? nodeIconUrl(node) : null}
                {@const missingDeps = missingDepsByMod.get(node.id) ?? []}
                <div
                  class="node-card compact resourcepack-card"
                  class:selected={selectedId === node.id}
                  role="button"
                  tabindex="0"
                  onclick={() => (selectedId = node.id)}
                  oncontextmenu={(e) => onNodeContextInstall(e, node)}
                  onkeydown={(event) => (event.key === "Enter" || event.key === " ") && (selectedId = node.id)}
                >
                  {#if icon}
                    <img class="card-icon" src={icon} alt="" loading="lazy" onerror={() => handleIconError(node)} />
                  {:else}
                    <span class="card-icon-fallback">{node.label?.[0]?.toUpperCase() ?? "?"}</span>
                  {/if}
                  <div class="card-text">
                    <span class="node-label">{node.label}</span>
                    <span class="node-meta">{node.version ?? "unknown"}{missingDeps.length > 0 ? ` · ${missingDeps.length} missing` : ""}</span>
                  </div>
                  {#if missingDeps.length > 0}
                    <div class="card-missing-list">
                      {#each missingDeps as edge (edge.to)}
                        <button
                          class="card-install-btn"
                          type="button"
                          title="Install {resolveNodeLabel(edge.to)}"
                          onclick={(e) => {
                            e.stopPropagation();
                            void installSingleMissingDep(edge);
                          }}
                          disabled={resolving}
                        >
                          <Download size={14} strokeWidth={2.25} />
                          <span>Install {resolveNodeLabel(edge.to)}</span>
                        </button>
                      {/each}
                    </div>
                  {/if}
                  <span class="card-remove" role="button" tabindex="0" title="Remove resource pack" onclick={(e) => { e.stopPropagation(); removeConflictNode(node.id); } } onkeydown={(e) => { e.stopPropagation(); if (e.key === "Enter") void removeConflictNode(node.id); } }>
                    <X size={14} />
                  </span>
                </div>
              {/each}
            </div>
          </div>
        {/if}

        {#if modNodes.length === 0}
          <div class="muted-box">No mod nodes yet.</div>
        {:else}
          {#each groupedMods as group (group.label)}
            <div class="profile-group-title" style={`--group-color:${group.color}`}>{group.label} ({group.nodes.length})</div>
            <div class="mod-grid">
              {#each group.nodes as node (node.id)}
                {@const icon = !brokenIcons.has(node.id) ? nodeIconUrl(node) : null}
                {@const isClickableDep = depNodeIds.has(node.id)}
                {@const missingDeps = missingDepsByMod.get(node.id) ?? []}
                <div
                  class="node-card compact side-{node.side}"
                  class:selected={selectedId === node.id}
                  class:is-dep={isClickableDep}
                  role="button"
                  tabindex="0"
                  onclick={() => (selectedId = node.id)}
                  oncontextmenu={(e) => onNodeContextInstall(e, node)}
                  onkeydown={(event) => (event.key === "Enter" || event.key === " ") && (selectedId = node.id)}
                >
                  {#if icon}
                    {#if isClickableDep}
                      <button
                        class="card-icon-btn"
                        title="Click to re-download this dependency"
                        onclick={(e) => { e.stopPropagation(); downloadMissingFiles(); } }
                      >
                        <img class="card-icon" src={icon} alt="" loading="lazy" onerror={() => handleIconError(node)} />
                      </button>
                    {:else}
                      <img class="card-icon" src={icon} alt="" loading="lazy" onerror={() => handleIconError(node)} />
                    {/if}
                  {:else if isClickableDep}
                    <button
                      class="card-icon-btn"
                      title="Click to re-download this dependency"
                      onclick={(e) => { e.stopPropagation(); downloadMissingFiles(); } }
                    >
                      <span class="card-icon-fallback">{node.label?.[0]?.toUpperCase() ?? "?"}</span>
                    </button>
                  {:else}
                    <span class="card-icon-fallback">{node.label?.[0]?.toUpperCase() ?? "?"}</span>
                  {/if}
                  <div class="card-text">
                    <span class="node-label">{node.label}</span>
                    <span class="node-meta">{node.version ?? "unknown"}{depNodeIds.has(node.id) ? " · dep" : ""}{missingDeps.length > 0 ? ` · ${missingDeps.length} missing` : ""}</span>
                  </div>
                  {#if missingDeps.length > 0}
                    <div class="card-missing-list">
                      {#each missingDeps as edge (edge.to)}
                        <button
                          class="card-install-btn"
                          type="button"
                          title="Install {resolveNodeLabel(edge.to)}"
                          onclick={(e) => {
                            e.stopPropagation();
                            void installSingleMissingDep(edge);
                          }}
                          disabled={resolving}
                        >
                          <Download size={14} strokeWidth={2.25} />
                          <span>Install {resolveNodeLabel(edge.to)}</span>
                        </button>
                      {/each}
                      {#if missingDeps.length > 1}
                        <button
                          class="card-install-btn all"
                          type="button"
                          title="Install all {missingDeps.length} missing dependencies"
                          onclick={(e) => {
                            e.stopPropagation();
                            void (async () => {
                              for (const edge of missingDeps) {
                                await installSingleMissingDep(edge);
                              }
                            })();
                          }}
                          disabled={resolving}
                        >
                          <Download size={14} strokeWidth={2.25} />
                          <span>Install all ({missingDeps.length})</span>
                        </button>
                      {/if}
                    </div>
                  {/if}
                  <span class="card-remove" role="button" tabindex="0" title="Remove mod" onclick={(e) => { e.stopPropagation(); removeConflictNode(node.id); } } onkeydown={(e) => { e.stopPropagation(); if (e.key === "Enter") void removeConflictNode(node.id); } }>
                    <X size={14} />
                  </span>
                </div>
              {/each}
            </div>
          {/each}
        {/if}
      </section>
    </div>
    {/if}

    {#if conflictInsights.length > 0}
      <div class="conflict-panel">
        <div class="conflict-panel-head">
          <h3><ShieldAlert size={16} /> Conflicts ({conflictInsights.length})</h3>
          <p>Known incompatibilities first. Suggestion keeps the mod more others depend on.</p>
        </div>
        <div class="conflict-list">
          {#each conflictInsights as c (c.key)}
            <article class="conflict-card" class:critical={c.severity === "critical"} class:known={c.known}>
              <div class="conflict-card-top">
                <span class="conflict-badge">{c.severity === "critical" ? "Breaks" : c.known ? "Known" : "Conflict"}</span>
                <div class="conflict-pair">
                  <strong>{c.leftLabel}</strong>
                  <span class="vs">vs</span>
                  <strong>{c.rightLabel}</strong>
                </div>
              </div>
              <p class="conflict-reason">{c.reason}</p>
              <div class="conflict-meta">
                <span>{c.leftLabel}: {c.leftDependents} dependent{c.leftDependents === 1 ? "" : "s"}</span>
                <span>{c.rightLabel}: {c.rightDependents} dependent{c.rightDependents === 1 ? "" : "s"}</span>
              </div>
              <div class="conflict-actions">
                <button
                  class="primary small"
                  type="button"
                  disabled={resolving}
                  title="Recommended: fewer dependents"
                  onclick={() => removeConflictNode(c.recommendRemoveId)}
                >
                  <Ban size={13} />
                  Remove {c.recommendRemoveLabel}
                </button>
                <button
                  class="ghost small"
                  type="button"
                  disabled={resolving}
                  onclick={() => removeConflictNode(c.keepId)}
                >
                  Remove {c.keepLabel} instead
                </button>
                <button class="ghost small" type="button" onclick={() => (selectedId = c.leftId)}>Focus</button>
              </div>
            </article>
          {/each}
        </div>
      </div>
    {/if}
    </div>

  {:else}
    <EmptyState icon={GitGraph} title="No project selected" description="Open a project to view its dependency graph." />
  {/if}
</div>

{#if catalogViewResult}
  <div
    class="modal-backdrop catalog-backdrop"
    role="button"
    tabindex="-1"
    onclick={(e) => {
      if (e.target === e.currentTarget) catalogViewResult = null;
    }}
    onkeydown={() => {}}
  >
    <div
      class="modal catalog-modal"
      role="dialog"
      aria-modal="true"
      use:trapFocus={{ onEscape: () => (catalogViewResult = null) }}
    >
      <CatalogProjectView
        result={catalogViewResult}
        minecraftVersion={$projectInfo?.minecraftVersion ?? null}
        loaderKind={$projectInfo?.loaderKind ?? null}
        installing={resolving}
        onback={() => (catalogViewResult = null)}
        oninstall={() => void installSelectedFromCatalog()}
        onopenexternal={() => void openSelectedExternal()}
      />
    </div>
  </div>
{/if}

{#if depPreviewOpen}
  <div class="modal-backdrop" role="button" tabindex="-1" onclick={(e) => e.target === e.currentTarget && (depPreviewOpen = false)} onkeydown={() => {}}>
    <div class="modal" role="dialog" aria-modal="true" use:trapFocus={{ onEscape: () => (depPreviewOpen = false) }}>
      <div class="modal-header">
        <div>
          <h2>Install dependency: {depPreviewName}</h2>
          <p>This mod has dependencies on Modrinth. Choose what to install.</p>
        </div>
        <button class="icon-btn" onclick={() => (depPreviewOpen = false)}><X size={18} /></button>
      </div>
      <div class="modal-body">
        {#if depPreviewLoading}
          <div class="loading"><Loader2 size={16} class="spin" /> Loading dependency info from Modrinth...</div>
        {:else}
          <div class="dep-list">
            <h4>
              Required ({depPreviewRequired.length})
              {#if depPreviewRequired.some((d) => d.alreadyInstalled)}
                <span class="dep-installed-count">{depPreviewRequired.filter((d) => d.alreadyInstalled).length} already installed</span>
              {/if}
            </h4>
            {#if depPreviewRequired.length === 0}
              <p class="muted">No hard dependencies — installing the mod alone should work.</p>
            {:else}
              {#each depPreviewRequired as dep (dep.target)}
                <div class="dep-entry required" class:already-installed={dep.alreadyInstalled}>
                  <span class="dep-target">{dep.target}</span>
                  {#if dep.alreadyInstalled}<span class="dep-installed-pill">Installed</span>{/if}
                  {#if dep.reason}<small>{dep.reason}</small>{/if}
                </div>
              {/each}
            {/if}
          </div>
          <div class="dep-list">
            <h4>
              Optional ({depPreviewOptional.length})
              {#if depPreviewOptional.some((d) => d.alreadyInstalled)}
                <span class="dep-installed-count">{depPreviewOptional.filter((d) => d.alreadyInstalled).length} already installed</span>
              {/if}
            </h4>
            {#if depPreviewOptional.length === 0}
              <p class="muted">No optional dependencies listed.</p>
            {:else}
              {#each depPreviewOptional as dep (dep.target)}
                <div class="dep-entry optional" class:already-installed={dep.alreadyInstalled}>
                  <span class="dep-target">{dep.target}</span>
                  {#if dep.alreadyInstalled}<span class="dep-installed-pill">Installed</span>{/if}
                  {#if dep.reason}<small>{dep.reason}</small>{/if}
                </div>
              {/each}
            {/if}
          </div>
          <label class="checkbox-row">
            <input type="checkbox" bind:checked={depPreviewInstallWithOptional} />
            <span>Install optional dependencies too</span>
          </label>
        {/if}
      </div>
      <div class="modal-footer">
        {#if depInstallStatus === "downloading"}
          <div class="install-transfer" aria-live="polite">
            <Loader2 size={16} class="spin" />
            <span>{depInstallMessage}</span>
          </div>
        {:else if depInstallStatus === "failed"}
          <div class="install-transfer failed" aria-live="assertive">
            <AlertTriangle size={16} />
            <div><strong>{depInstallMessage}</strong><pre>{depInstallError}</pre></div>
          </div>
        {:else if depInstallStatus === "done"}
          <div class="install-transfer done" aria-live="polite">
            <Download size={16} />
            <span>{depInstallMessage}</span>
          </div>
        {/if}
        <div class="modal-footer-actions">
          <button class="secondary" onclick={() => (depPreviewOpen = false)} disabled={depInstallStatus === "downloading"}>
            {depInstallStatus === "done" ? "Close" : "Cancel"}
          </button>
          {#if depInstallStatus === "failed"}
            <button onclick={retryDepInstall} disabled={resolving}>
              <RotateCw size={16} /> Retry
            </button>
          {:else if depInstallStatus !== "done"}
            <button onclick={confirmDepInstall} disabled={depPreviewLoading || depInstallStatus === "downloading"}>
              <Download size={16} /> {depInstallStatus === "downloading" ? "Downloading…" : "Install"}
            </button>
          {/if}
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .graph {
    display: flex;
    flex-direction: column;
    flex: 1;
    height: 100%;
    min-height: 0;
    overflow-x: hidden;
    overflow-y: auto;
    /* Responsive: centered cap on 1440p+ so node canvas keeps aspect. */
    max-width: min(1840px, 100%);
    margin: 0 auto;
    width: 100%;
    scrollbar-gutter: stable;
    overscroll-behavior: contain;
    container-type: size;
  }

  .graph-body {
    flex: 0 0 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: visible;
    gap: 10px;
  }

  .toolbar,
  .toolbar-actions,
  .notice {
    display: flex;
    align-items: center;
    flex-shrink: 0;
  }

  .toolbar {
    justify-content: space-between;
    margin-bottom: 10px;
  }

  .toolbar-actions { gap: 10px; }

  .notice {
    gap: 10px;
    padding: 12px 14px;
    border-radius: var(--border-radius-lg);
    margin-bottom: 14px;
    border: 1px solid var(--border-color);
  }

  .notice.success {
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
    border-color: color-mix(in srgb, var(--accent-primary) 25%, transparent);
  }

  .notice.plan-error {
    color: var(--danger, #ef4444);
    background: color-mix(in srgb, var(--danger, #ef4444) 8%, transparent);
    border-color: color-mix(in srgb, var(--danger, #ef4444) 30%, transparent);
  }

  .graph-status {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: -4px 0 8px;
    color: var(--text-muted);
    font-size: 12px;
    flex-shrink: 0;
  }

  .graph-status.stale { color: var(--accent-warning); }
  .graph-status.error { color: var(--accent-danger); }

  .title,
  h3 {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text-secondary);
    font-weight: 600;
  }

  h3 {
    margin: 0 0 12px;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .stats {
    display: flex;
    gap: 16px;
    margin-bottom: 20px;
    flex-wrap: wrap;
    flex-shrink: 0;
  }

  .stats.stats-compact {
    flex-wrap: nowrap;
    gap: 8px;
    margin-bottom: 8px;
  }

  .change-plan-panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 8px;
    padding: 8px 12px;
    border: 1px solid color-mix(in srgb, var(--accent-primary) 28%, transparent);
    border-radius: var(--border-radius-lg);
    background: radial-gradient(circle at top left, color-mix(in srgb, var(--accent-primary) 9%, transparent), transparent 42%), var(--bg-secondary);
    flex-shrink: 0;
  }
  .change-plan-panel.loading {
    opacity: 0.72;
    border-color: var(--border-color);
  }
  .change-plan-toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    width: 100%;
    padding: 0;
    margin: 0;
    border: none;
    background: transparent;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }
  .change-plan-toggle :global(.rot) {
    transform: rotate(180deg);
  }
  .change-plan-panel:not(.expanded) .change-plan-actions,
  .change-plan-panel:not(.expanded) .change-plan-list {
    display: none;
  }
  .change-plan-summary.muted {
    color: var(--text-muted);
    font-weight: 600;
  }
  .change-plan-head { display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; }
  .change-plan-summary { font-size: 14px; font-weight: 700; color: var(--text-primary); }
  .change-plan-risk {
    font-size: 12px; color: var(--text-muted);
    padding: 2px 8px; border-radius: 999px;
    background: var(--bg-tertiary); border: 1px solid var(--border-color);
  }
  .change-plan-risk.req { color: var(--accent-warning); border-color: color-mix(in srgb, var(--accent-warning) 35%, transparent); }
  .change-plan-actions { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .change-plan-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .change-plan-item {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    width: 100%;
    text-align: left;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 1.35;
    cursor: pointer;
  }
  .change-plan-item:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--accent-primary);
  }
  .change-plan-item:disabled { opacity: 0.5; cursor: default; }
  .change-plan-item-idx {
    flex: 0 0 auto;
    min-width: 1.5rem;
    font-weight: 700;
    color: var(--text-muted);
  }
  .change-plan-item-text { flex: 1; min-width: 0; white-space: normal; }
  .change-plan-empty {
    padding: 6px 2px;
    color: var(--text-muted);
    font-size: 12px;
  }
  .chip {
    max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    padding: 5px 10px; font-size: 12px; border-radius: 999px;
    background: var(--bg-tertiary); border: 1px solid var(--border-color); color: var(--text-secondary);
    transform: none;
  }
  .chip:hover:not(:disabled) { color: var(--text-primary); border-color: var(--accent-primary); }
  .chip:disabled { opacity: .5; cursor: default; }

  .subtle-hint { display: flex; align-items: center; gap: 8px; color: var(--text-muted); font-size: 13px; }
  .subtle-hint span { line-height: 1.4; }
  .mini { padding: 5px 8px; font-size: 12px; }

  .graph-canvas {
    position: relative;
    flex: 0 0 auto;
    height: calc(100cqh - 64px);
    min-height: 420px;
    width: 100%;
    min-width: 0;
    display: flex;
    flex-direction: column;
    margin-bottom: 0;
    background:
      radial-gradient(circle at 78% 18%, color-mix(in srgb, var(--accent-primary) 8%, transparent), transparent 28%),
      #09090b;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
    overflow: hidden;
    user-select: none;
    -webkit-user-select: none;
    -webkit-touch-callout: none;
  }

  .graph-loading-overlay {
    position: absolute;
    inset: 0;
    z-index: 5;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    background: rgba(9, 9, 11, 0.55);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    color: var(--text-muted);
    font-size: 14px;
    font-weight: 600;
  }

  .graph-canvas svg {
    width: 100%;
    height: 100%;
    flex: 1;
    min-height: 0;
    display: block;
    cursor: grab;
    touch-action: none;
    user-select: none;
    -webkit-user-select: none;
  }

  .graph-canvas:fullscreen {
    margin: 0;
    border: 0;
    border-radius: 0;
    background: #09090b;
    height: 100%;
    min-height: 100%;
  }

  .graph-canvas:fullscreen svg {
    height: 100%;
  }

  .graph-canvas svg.panning,
  .graph-canvas svg.dragging {
    cursor: grabbing;
  }

  .graph-canvas svg text {
    user-select: none;
    -webkit-user-select: none;
    pointer-events: none;
  }

  .canvas-controls {
    position: absolute;
    top: 12px;
    right: 12px;
    z-index: 2;
    display: flex;
    align-items: center;
    gap: 8px;
    background: rgba(9, 9, 11, 0.72);
    border: 1px solid var(--border-color);
    border-radius: 10px;
    padding: 4px;
    backdrop-filter: blur(6px);
  }

  .canvas-controls .mini {
    width: 26px;
    height: 26px;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 15px;
    line-height: 1;
  }

  .canvas-controls .edge-toggle {
    width: auto;
    padding: 0 10px;
    font-size: 12px;
    white-space: nowrap;
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .canvas-controls .mini.active {
    background: color-mix(in srgb, var(--accent-secondary) 18%, transparent);
    border-color: color-mix(in srgb, var(--accent-secondary) 45%, transparent);
    color: var(--accent-secondary);
  }

  .canvas-controls .hl-btn.conflicts.active {
    background: color-mix(in srgb, var(--accent-danger) 28%, transparent);
    border-color: color-mix(in srgb, var(--accent-danger) 95%, transparent);
    color: var(--accent-danger);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent-danger) 35%, transparent), 0 0 18px color-mix(in srgb, var(--accent-danger) 35%, transparent);
  }

  .canvas-controls .hl-btn.missing.active {
    background: color-mix(in srgb, var(--accent-warning) 28%, transparent);
    border-color: color-mix(in srgb, var(--accent-warning) 95%, transparent);
    color: var(--accent-warning);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent-warning) 35%, transparent), 0 0 18px color-mix(in srgb, var(--accent-warning) 35%, transparent);
  }

  .zoom-readout {
    font-size: 12px;
    color: var(--text-muted);
    padding: 0 6px;
    min-width: 38px;
    text-align: center;
  }

  /* ── High-contrast Conflicts / Missing spotlight ───────────────── */
  .graph-canvas.hl-conflicts .graph-edge.dimmed,
  .graph-canvas.hl-missing .graph-edge.dimmed {
    opacity: 0.04;
    stroke-width: 1;
  }

  .graph-canvas.hl-conflicts .svg-node.dimmed,
  .graph-canvas.hl-missing .svg-node.dimmed {
    opacity: 0.12;
    filter: grayscale(0.85);
  }

  .graph-canvas.hl-conflicts .graph-group,
  .graph-canvas.hl-missing .graph-group {
    opacity: 0.18;
  }

  .graph-canvas .graph-edge.spotlight:not(.dimmed) {
    opacity: 1;
  }

  .graph-canvas .graph-edge.spotlight-conflicts:not(.dimmed) {
    stroke: #ff2d2d;
    stroke-width: 5.5;
    stroke-dasharray: 10 6;
    filter: url(#glow-conflict);
    animation: edge-pulse-conflict 1.1s ease-in-out infinite;
  }

  .graph-canvas .graph-edge.spotlight-missing:not(.dimmed) {
    stroke: #fbbf24;
    stroke-width: 5;
    stroke-dasharray: 8 5;
    filter: url(#glow-missing);
    animation: edge-pulse-missing 1.2s ease-in-out infinite;
  }

  .svg-node.spotlight:not(.dimmed) {
    opacity: 1;
  }

  .svg-node.spotlight-conflicts:not(.dimmed) {
    filter: drop-shadow(0 0 14px rgba(255, 45, 45, 0.95))
      drop-shadow(0 0 28px rgba(239, 68, 68, 0.7));
  }

  .svg-node.spotlight-missing:not(.dimmed) {
    filter: drop-shadow(0 0 14px rgba(251, 191, 36, 0.95))
      drop-shadow(0 0 28px rgba(245, 158, 11, 0.7));
  }

  .svg-node.spotlight-conflicts:not(.dimmed) rect {
    stroke: #ff4d4d;
    stroke-width: 3.2;
    fill: rgba(239, 68, 68, 0.22);
  }

  .svg-node.spotlight-missing:not(.dimmed) rect {
    stroke: #fbbf24;
    stroke-width: 3.2;
    stroke-dasharray: 5 3;
    fill: rgba(245, 158, 11, 0.2);
  }

  /* Installed mod that requires a missing dep — solid amber (vs dashed missing). */
  .svg-node.spotlight-requirer:not(.dimmed) rect {
    stroke: #f59e0b;
    stroke-width: 3.5;
    stroke-dasharray: none;
    fill: rgba(245, 158, 11, 0.28);
  }

  .svg-node.spotlight-requirer:not(.dimmed) .node-label-text {
    fill: #fde68a;
  }

  .svg-node .spotlight-ring {
    fill: none;
    pointer-events: none;
    stroke-width: 3.5;
  }

  .svg-node .spotlight-ring.conflicts {
    stroke: #ff3b3b;
    animation: ring-pulse-conflict 1.15s ease-in-out infinite;
  }

  .svg-node .spotlight-ring.missing {
    stroke: #fbbf24;
    animation: ring-pulse-missing 1.25s ease-in-out infinite;
  }

  @keyframes edge-pulse-conflict {
    0%, 100% { stroke-opacity: 1; stroke-width: 5.5; }
    50% { stroke-opacity: 0.72; stroke-width: 7; }
  }

  @keyframes edge-pulse-missing {
    0%, 100% { stroke-opacity: 1; stroke-width: 5; }
    50% { stroke-opacity: 0.75; stroke-width: 6.5; }
  }

  @keyframes ring-pulse-conflict {
    0%, 100% { stroke-opacity: 0.95; stroke-width: 3.5; }
    50% { stroke-opacity: 0.35; stroke-width: 6; }
  }

  @keyframes ring-pulse-missing {
    0%, 100% { stroke-opacity: 0.95; stroke-width: 3.5; }
    50% { stroke-opacity: 0.4; stroke-width: 6; }
  }

  .graph-canvas .graph-edge {
    fill: none;
    stroke: rgba(161, 161, 170, 0.55);
    stroke-width: 1.6;
    transition: opacity 120ms ease;
  }

  /* Category cluster halos: faint, colour-coded discs that make each group of
     mods read as a labelled region of the canvas for human perception. */
  .graph-group .group-halo {
    fill: currentColor;
    fill-opacity: 0.06;
    stroke: currentColor;
    stroke-opacity: 0.28;
    stroke-width: 1.5;
    stroke-dasharray: 2 8;
    pointer-events: none;
  }
  .graph-group .group-label {
    fill: currentColor;
    fill-opacity: 0.85;
    stroke: none;
    font-size: 13px;
    font-weight: 800;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    pointer-events: none;
    transition: fill-opacity 120ms ease;
  }
  /* Cluster spotlight: hovered halo brightens; the rest fade back. */
  .graph-group { cursor: pointer; transition: opacity 120ms ease; user-select: none; }
  .graph-group.active .group-halo {
    fill-opacity: 0.14;
    stroke-opacity: 0.6;
  }
  .graph-group.active .group-label { fill-opacity: 1; }
  .graph-group.dim { opacity: 0.35; }

  /* Graph legend overlay (edge semantics + node tones + clickable clusters). */
  .graph-legend {
    position: absolute;
    left: 12px;
    bottom: 12px;
    z-index: 2;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: calc(100% - 24px);
    padding: 8px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: rgba(9, 9, 11, 0.74);
    backdrop-filter: blur(6px);
    color: var(--text-secondary);
    font-size: 12px;
  }
  .graph-legend.collapsed {
    padding: 6px 8px;
  }
  .legend-toggle {
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin: 0;
    padding: 2px 4px;
    border: none;
    background: transparent;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    cursor: pointer;
  }
  .legend-toggle :global(svg) {
    transition: transform 0.15s ease;
    color: var(--text-muted);
  }
  .legend-toggle :global(svg.rot) {
    transform: rotate(180deg);
  }
  .legend-body {
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
  }
  .legend-block { display: flex; flex-direction: column; gap: 8px; }
  .legend-title {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-muted);
    margin-bottom: 2px;
  }
  .legend-row { display: flex; align-items: center; gap: 8px; line-height: 1.4; }
  .lg-line {
    width: 22px; height: 0;
    border-top-width: 2px; border-top-style: solid;
    display: inline-block;
  }
  .lg-line.hard { border-color: rgba(161,161,170,.85); }
  .lg-line.conflict { border-color: rgba(239,68,68,.95); border-top-style: dashed; }
  .lg-line.optional { border-color: rgba(132,204,22,.85); border-top-style: dashed; }
  .lg-line.runtime { border-color: rgba(161,161,170,.4); }
  .lg-dot {
    width: 11px; height: 11px; border-radius: 3px;
    border: 1.6px solid; display: inline-block;
    background: var(--bg-tertiary);
  }
  .lg-dot.client { border-color: color-mix(in srgb, var(--accent-secondary) 80%, transparent); }
  .lg-dot.server { border-color: rgba(59,130,246,.8); }
  .lg-dot.both { border-color: color-mix(in srgb, var(--accent-primary) 80%, transparent); }
  .lg-dot.dep { border-color: rgba(245,158,11,.8); border-style: dashed; }
  .lg-dot.resourcepack { border-color: #38bdf8; }
  .lg-dot.shader { border-color: #c084fc; }
  .lg-dot.missing { border-color: rgba(113,113,122,.9); border-style: dashed; }
  .lg-dot.requirer { border-color: #f59e0b; border-style: solid; background: rgba(245,158,11,.25); }
  .legend-groups { max-width: 240px; }
  .legend-chip {
    display: inline-flex; align-items: center;
    padding: 2px 8px; margin: 0 4px 4px 0;
    font-size: 12px; font-weight: 700;
    border-radius: 999px;
    border: 1px solid var(--chip, var(--border-color));
    color: var(--text-secondary);
    background: transparent;
    cursor: pointer;
    transition: background 120ms ease, color 120ms ease;
  }
  .legend-chip:hover, .legend-chip.on {
    background: var(--chip, var(--bg-tertiary));
    color: #09090b;
  }

  .graph-canvas .graph-edge.dense {
    stroke: rgba(161, 161, 170, 0.28);
    stroke-width: 1.2;
  }

  .graph-canvas .graph-edge.dimmed {
    opacity: 0.12;
  }

  .svg-node.dimmed {
    opacity: 0.25;
  }

  /* Loader/runtime edges (fabric-api -> mods) are numerous; keep them faint
     and thin so the hub reads as a clean star instead of a tangle. */
  .graph-canvas .graph-edge.runtime-edge {
    stroke: rgba(161, 161, 170, 0.22);
    stroke-width: 1;
  }

  /* Hard-dep fan-in into a shared library hub: kept visible (so the graph is a
     connected web, not just red conflicts) but faded so the hub doesn't drown
     the layout. Selecting the hub still highlights these normally. */
  .graph-canvas .graph-edge.hub-edge {
    stroke: rgba(161, 161, 170, 0.3);
    stroke-width: 1.1;
  }

  .graph-canvas .graph-edge.danger-edge {
    stroke: rgba(239, 68, 68, 0.95);
    stroke-width: 2;
    stroke-dasharray: 6 5;
  }

  /* Optional dependencies / integrations — semantic yellow-green dashed, kept
     visually distinct from the solid grey hard-dependency links. */
  .graph-canvas .graph-edge.optional-edge {
    stroke: rgba(132, 204, 22, 0.8);
    stroke-width: 1.6;
    stroke-dasharray: 3 5;
  }

  .svg-node {
    cursor: grab;
    transition: opacity 120ms ease;
    user-select: none;
  }
  .graph-canvas svg.dragging .svg-node,
  .svg-node:active {
    cursor: grabbing;
  }

  /* Installed dependency — third color (amber) */
  .svg-node.clickable-dep rect {
    stroke: rgba(245,166,35,.7);
    stroke-dasharray: 4 3;
  }
  .svg-node.clickable-dep:hover rect {
    stroke: rgba(245,166,35,1);
    fill: rgba(245,166,35,.08);
  }

  .svg-node rect {
    fill: var(--bg-tertiary);
    stroke: var(--border-color);
    stroke-width: 1.6;
    transition: stroke 120ms ease, fill 120ms ease;
  }

  .svg-node.selected rect,
  .svg-node:hover rect {
    stroke: color-mix(in srgb, var(--accent-primary) 85%, transparent);
    fill: color-mix(in srgb, var(--accent-primary) 18%, transparent);
  }

  .svg-node.tone-client rect { stroke: color-mix(in srgb, var(--accent-secondary) 70%, transparent); }
  .svg-node.tone-server rect { stroke: rgba(59,130,246,.7); }
  .svg-node.tone-both rect { stroke: color-mix(in srgb, var(--accent-primary) 65%, transparent); }
  .svg-node.tone-runtime rect { stroke: rgba(245,158,11,.7); }
  .svg-node.tone-profile rect { stroke: rgba(96,165,250,.7); }
  .svg-node.tone-resourcepack rect {
    stroke: #38bdf8;
    fill: rgba(14, 116, 144, 0.22);
  }
  .svg-node.tone-shader rect {
    stroke: #c084fc;
    fill: rgba(126, 34, 168, 0.22);
  }
  .svg-node.tone-ghost rect {
    stroke: rgba(113, 113, 122, 0.9);
    stroke-dasharray: 4 3;
    fill: rgba(24, 24, 27, 0.95);
  }
  .svg-node.tone-ghost .node-label-text { fill: #a1a1aa; }
  .svg-node.tone-ghost .fallback-letter { fill: #71717a; }

  .svg-node .fallback-letter {
    fill: #e5e7eb;
    font-size: 16px;
    font-weight: 900;
    pointer-events: none;
  }

  .svg-node .node-label-text {
    fill: #d1d5db;
    font-size: 12px;
    font-weight: 700;
    pointer-events: none;
  }

  .svg-node .ghost-download {
    fill: #a1a1aa;
    font-size: 12px;
    font-weight: 700;
    pointer-events: none;
    cursor: pointer;
  }

  .svg-node.tone-ghost {
    cursor: pointer;
  }
  .svg-node.tone-ghost:hover rect {
    stroke: color-mix(in srgb, var(--accent-primary) 85%, transparent);
    fill: color-mix(in srgb, var(--accent-primary) 12%, transparent);
  }

  .remove-btn {
    cursor: pointer;
    opacity: 0;
    transition: opacity 120ms ease;
  }
  .svg-node:hover .remove-btn,
  .svg-node.selected .remove-btn {
    opacity: 1;
  }
  .remove-btn circle {
    fill: rgba(239,68,68,.85);
    stroke: none;
  }
  .remove-btn:hover circle {
    fill: rgba(239,68,68,1);
  }
  .remove-btn .remove-x {
    fill: #fff;
    font-size: 13px;
    font-weight: 900;
    pointer-events: none;
  }

  .svg-node text {
    fill: #e5e7eb;
    font-size: 12px;
    font-weight: 800;
    pointer-events: none;
  }

  .stat-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
    padding: 18px 24px;
    min-width: 130px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .stats-compact .stat-card {
    flex: 1;
    min-width: 0;
    flex-direction: row;
    align-items: baseline;
    justify-content: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: 10px;
  }

  .stats-compact .stat-value {
    font-size: 18px;
    font-weight: 800;
  }

  .stats-compact .stat-label {
    font-size: 12px;
    letter-spacing: 0.04em;
  }

  .stat-card.accent {
    border-color: color-mix(in srgb, var(--accent-primary) 32%, transparent);
    background: linear-gradient(135deg, color-mix(in srgb, var(--accent-primary) 12%, transparent), var(--bg-secondary));
  }

  .stat-card.danger {
    border-color: rgba(239, 68, 68, 0.35);
    background: linear-gradient(135deg, rgba(239, 68, 68, 0.12), var(--bg-secondary));
  }

  .stat-card.clickable {
    cursor: pointer;
  }
  .stat-card.clickable:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 55%, transparent);
  }

  .stat-value {
    font-size: 28px;
    font-weight: 800;
  }

  .stat-label {
    font-size: 12px;
    text-transform: uppercase;
    color: var(--text-muted);
    letter-spacing: 0.05em;
  }

  .graph-layout,
  .graph-list {
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .list-toggle-row {
    display: flex;
    flex-shrink: 0;
  }

  .list-toggle-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .list-toggle-meta {
    color: var(--text-muted);
    font-size: 12px;
  }

  .node-column,
  .details {
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
    padding: 16px;
  }

  .details {
    flex: 0 0 auto;
    width: 100%;
    min-height: 160px;
    max-height: none;
    overflow: visible;
    position: static;
  }

  .details-placeholder {
    color: var(--text-muted);
    font-weight: 600;
  }

  .details-action-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .catalog-backdrop .catalog-modal {
    /* Match Mods "Add" browser: near-fullscreen so the catalog page is readable. */
    width: calc(100vw - 12px);
    height: calc(100vh - 12px);
    max-width: none;
    max-height: calc(100vh - 12px);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    padding: 16px 18px 14px;
    border-radius: 14px;
  }

  .catalog-backdrop .catalog-modal :global(.catalog-page) {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .mods-column {
    min-height: auto;
  }

  .missing-column {
    border-bottom: 1px solid var(--border-color);
    padding-bottom: 14px;
    margin-bottom: 14px;
  }
  .column-hint {
    color: var(--text-muted);
    font-size: 12px;
    margin: 0 0 8px;
    font-style: italic;
  }
  .missing-card {
    border-color: rgba(82, 82, 91, 0.65);
    background: rgba(24, 24, 27, 0.85);
  }
  .missing-card:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 55%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
  }
  .pack-column {
    border-bottom: 1px solid var(--border-color);
    padding-bottom: 14px;
    margin-bottom: 14px;
  }
  .shader-card {
    border-color: rgba(192, 132, 252, 0.55);
    background: rgba(126, 34, 168, 0.12);
  }
  .shader-card:hover {
    border-color: #c084fc;
    background: rgba(126, 34, 168, 0.2);
  }
  .resourcepack-card {
    border-color: rgba(56, 189, 248, 0.55);
    background: rgba(14, 116, 144, 0.12);
  }
  .resourcepack-card:hover {
    border-color: #38bdf8;
    background: rgba(14, 116, 144, 0.2);
  }
  .missing-fallback {
    background: linear-gradient(135deg, var(--bg-hover), var(--bg-tertiary));
    color: #a1a1aa;
    font-size: 18px;
    font-weight: 800;
  }

  .profile-group-title {
    margin: 14px 0 8px;
    font-size: 12px;
    font-weight: 700;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .profile-group-title::before {
    content: "";
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--group-color, var(--accent-primary));
  }

  .profile-group-title.orphaned::before {
    background: var(--text-muted);
  }

  .mod-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 10px;
  }

  .node-card {
    width: 100%;
    text-align: left;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    margin-bottom: 10px;
    border-radius: 14px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }

  .node-card.compact {
    margin-bottom: 0;
    flex-direction: row;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }

  .node-card.is-dep {
    opacity: 0.85;
    border-style: dashed;
    border-color: rgba(245, 158, 11, 0.45);
    background: rgba(245, 158, 11, 0.06);
  }

  .card-icon {
    width: 36px;
    height: 36px;
    border-radius: var(--border-radius-sm);
    object-fit: cover;
    flex-shrink: 0;
    background: var(--bg-elevated);
  }
  .card-icon-btn {
    padding: 0;
    border: none;
    background: transparent;
    cursor: pointer;
    border-radius: var(--border-radius-sm);
    line-height: 0;
    transition: transform 100ms ease, box-shadow 100ms ease;
  }
  .card-icon-btn:hover {
    transform: scale(1.08);
    box-shadow: 0 0 0 2px rgba(245,166,35,.6);
  }

  .card-icon-fallback {
    width: 36px;
    height: 36px;
    border-radius: var(--border-radius-sm);
    background: linear-gradient(135deg, var(--accent-secondary), var(--accent-primary));
    display: flex;
    align-items: center;
    justify-content: center;
    color: #fff;
    font-weight: 900;
    font-size: 16px;
    flex-shrink: 0;
  }

  .card-text {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
    flex: 1;
  }

  .card-remove {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 6px;
    color: var(--text-muted);
    opacity: 0;
    transition: opacity 120ms ease, color 120ms ease, background 120ms ease;
    flex-shrink: 0;
    cursor: pointer;
  }
  .node-card:hover .card-remove,
  .node-card.selected .card-remove {
    opacity: 1;
  }
  .card-remove:hover {
    color: #ef4444;
    background: rgba(239,68,68,.12);
  }

  .card-install-deps {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--border-radius-sm);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 35%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 10%, transparent);
    color: var(--accent-primary);
    flex-shrink: 0;
  }

  .card-install-deps:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-primary) 18%, transparent);
  }

  .card-install-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    height: 30px;
    padding: 0 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 45%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 14%, transparent);
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
    white-space: nowrap;
  }
  .card-missing-list {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    flex: 1 1 100%;
    width: 100%;
    margin-top: 4px;
  }
  .card-install-btn.all {
    border-color: color-mix(in srgb, var(--accent-primary) 70%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 22%, transparent);
  }
  .card-install-btn :global(svg) {
    fill: none !important;
    stroke: currentColor !important;
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }
  .card-install-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-primary) 24%, transparent);
    border-color: var(--accent-primary);
  }
  .card-install-btn:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .conflict-panel {
    flex-shrink: 0;
    max-height: min(36vh, 320px);
    overflow: auto;
    margin-top: 0;
    padding: 16px;
    border-radius: var(--border-radius-lg);
    border: 1px solid rgba(239, 68, 68, 0.28);
    background: linear-gradient(180deg, rgba(239, 68, 68, 0.08), var(--bg-secondary));
  }
  .conflict-panel-head h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 4px;
    color: var(--accent-danger);
    font-size: 14px;
  }
  .conflict-panel-head p {
    margin: 0 0 12px;
    color: var(--text-muted);
    font-size: 12px;
  }
  .conflict-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 10px;
  }
  .conflict-card {
    padding: 12px;
    border-radius: var(--border-radius-md);
    border: 1px solid rgba(245, 158, 11, 0.35);
    background: rgba(24, 24, 27, 0.88);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .conflict-card.critical {
    border-color: rgba(239, 68, 68, 0.5);
    background: rgba(239, 68, 68, 0.08);
  }
  .conflict-card.known {
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent-primary) 12%, transparent);
  }
  .conflict-card-top {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }
  .conflict-badge {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    flex-shrink: 0;
    padding: 2px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent-warning) 16%, transparent);
    color: var(--accent-warning);
    font-size: 12px;
    font-weight: 800;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .conflict-card.critical .conflict-badge {
    background: color-mix(in srgb, var(--accent-danger) 18%, transparent);
    color: var(--accent-danger);
  }
  .conflict-pair {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
    color: var(--text-primary);
    font-size: 13px;
  }
  .conflict-pair .vs {
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
  }
  .conflict-reason {
    margin: 0;
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 1.45;
  }
  .conflict-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    color: var(--text-muted);
    font-size: 12px;
  }
  .conflict-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .conflict-actions .primary.small,
  .conflict-actions .ghost.small {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 28px;
    padding: 0 10px;
    font-size: 12px;
  }

  .node-card:hover,
  .node-card.selected {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
    color: var(--text-primary);
  }

  .node-label {
    color: var(--text-primary);
    font-weight: 700;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .node-card.selected .node-label {
    white-space: normal;
    overflow: visible;
    text-overflow: unset;
  }

  .node-meta {
    color: var(--text-muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .side-client {
    box-shadow: inset 3px 0 color-mix(in srgb, var(--accent-secondary) 75%, transparent);
  }

  .side-server {
    box-shadow: inset 3px 0 rgba(59, 130, 246, 0.75);
  }

  .side-both {
    box-shadow: inset 3px 0 color-mix(in srgb, var(--accent-primary) 75%, transparent);
  }

  .details-header {
    display: flex;
    justify-content: space-between;
    gap: 14px;
    margin-bottom: 16px;
  }

  .details-header h2 {
    margin: 4px 0 0;
    font-size: 22px;
  }

  .details-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 14px;
  }

  .install-btn,
  .remove-btn-panel {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    border-radius: 10px;
    font-weight: 700;
    font-size: 13px;
    border: none;
    cursor: pointer;
    transition: filter 120ms ease;
  }
  .install-btn {
    background: var(--accent-primary);
    color: #fff;
    flex: 1;
  }
  .install-btn:hover:not(:disabled) { filter: brightness(1.1); }
  .install-btn:disabled { opacity: .5; cursor: wait; }

  .remove-btn-panel {
    background: rgba(239,68,68,.12);
    color: #ef4444;
    border: 1px solid rgba(239,68,68,.3);
  }
  .remove-btn-panel:hover:not(:disabled) {
    background: rgba(239,68,68,.2);
  }
  .remove-btn-panel:disabled { opacity: .5; cursor: wait; }

  .eyebrow {
    color: var(--text-muted);
    text-transform: uppercase;
    font-size: 12px;
    letter-spacing: 0.08em;
  }

  .details-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
    margin-bottom: 18px;
  }

  .details-grid div,
  .kv,
  .relation,
  .muted-box,
  .missing-row {
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    border-radius: 14px;
    padding: 12px;
  }

  .details-grid span,
  .kv span {
    display: block;
    color: var(--text-muted);
    font-size: 12px;
    text-transform: uppercase;
    margin-bottom: 6px;
  }

  code {
    font-family: var(--font-mono, ui-monospace, monospace);
    color: var(--text-secondary);
    word-break: break-all;
  }

  .tag,
  .relation-kind {
    display: inline-flex;
    align-items: center;
    height: 24px;
    padding: 0 9px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 800;
    text-transform: uppercase;
  }

  .tag.missing-tag {
    background: color-mix(in srgb, var(--accent-warning) 18%, transparent);
    color: var(--accent-warning);
  }

  .missing-map-hint {
    margin: 0 0 12px;
    font-size: 12px;
    line-height: 1.4;
    color: var(--text-muted);
  }

  .missing-map {
    display: grid;
    gap: 10px;
  }

  .missing-map-row {
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    padding: 10px;
    display: grid;
    gap: 8px;
  }

  .missing-map-row.active {
    border-color: color-mix(in srgb, #f59e0b 55%, transparent);
    background: rgba(245, 158, 11, 0.08);
  }

  .missing-map-mod {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    padding: 0;
    border: none;
    background: transparent;
    color: var(--text-primary);
    text-align: left;
    cursor: pointer;
  }

  .missing-map-mod strong {
    font-size: 14px;
    font-weight: 700;
  }

  .missing-map-mod span {
    flex-shrink: 0;
    font-size: 12px;
    color: var(--accent-warning);
    font-weight: 700;
  }

  .missing-map-mod.active strong,
  .missing-map-dep.active {
    color: var(--accent-warning);
  }

  .missing-map-deps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 8px;
  }

  .missing-map-deps li {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .missing-map-dep {
    flex: 1;
    min-width: 0;
    padding: 6px 8px;
    border-radius: var(--border-radius-sm);
    border: 1px dashed rgba(251, 191, 36, 0.45);
    background: rgba(251, 191, 36, 0.06);
    color: var(--text-secondary);
    font-size: 12px;
    text-align: left;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .missing-map-dep:hover,
  .missing-map-mod:hover strong {
    color: var(--text-primary);
  }

  .kv {
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: 8px;
    margin-bottom: 18px;
  }

  .relations {
    display: grid;
    gap: 8px;
  }

  .relation {
    display: grid;
    gap: 8px;
  }

  .relation.incoming .relation-kind {
    background: color-mix(in srgb, var(--accent-secondary) 12%, transparent);
    color: var(--accent-secondary);
  }

  .relation-text {
    color: var(--text-secondary);
  }

  .relation-text strong {
    color: var(--text-primary);
    margin-left: 6px;
  }

  small,
  .muted-box {
    color: var(--text-muted);
    line-height: 1.45;
  }


  .missing-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 8px;
    width: 100%;
    cursor: pointer;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    padding: 10px 12px;
    color: var(--text-secondary);
    text-align: left;
    transform: none;
    transition: border-color .15s;
  }

  .missing-row:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 5%, transparent);
  }

  .missing-row:disabled {
    opacity: .5;
    cursor: wait;
  }

  .dep-slug {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dep-icon {
    flex-shrink: 0;
    color: var(--accent-primary);
    opacity: .7;
  }

  /* Dep-tone node: amber for downloaded dependency mods */
  .svg-node.tone-dep rect {
    stroke: rgba(245, 158, 11, 0.7);
    fill: rgba(245, 158, 11, 0.08);
  }

  .ghost-node {
    cursor: pointer;
    transition: all .15s ease;
    animation: ghost-pulse 2s ease-in-out infinite;
  }
  .ghost-node:hover {
    fill: color-mix(in srgb, var(--accent-primary) 18%, transparent);
    stroke: color-mix(in srgb, var(--accent-primary) 85%, transparent);
    r: 16;
  }
  .ghost-label {
    cursor: pointer;
    transition: fill .15s;
  }
  .ghost-label:hover {
    fill: #d4d4d8;
    font-size: 12px;
  }
  @keyframes ghost-pulse {
    0%, 100% { opacity: 0.7; }
    50% { opacity: 1; }
  }

  .empty,
  .loading {
    color: var(--text-muted);
    padding: 80px;
    text-align: center;
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
  }

  .error {
    color: var(--accent-danger);
    border-color: color-mix(in srgb, var(--accent-danger) 35%, transparent);
  }

  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0,0,0,0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }
  .modal {
    background: var(--bg-primary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
    max-width: 520px;
    width: 90%;
    max-height: 80vh;
    overflow-y: auto;
    padding: 0;
  }
  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    padding: 20px 24px 12px;
    border-bottom: 1px solid var(--border-color);
  }
  .modal-header h2 { margin: 0; font-size: 18px; }
  .modal-header p { margin: 4px 0 0; font-size: 13px; color: var(--text-muted); }
  .modal-body { padding: 16px 24px; }
  .modal-footer {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 24px 20px;
  }
  .modal-footer-actions {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
  }
  .install-transfer {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--accent-primary) 28%, transparent);
    border-radius: 10px;
    color: var(--text-secondary);
    background: color-mix(in srgb, var(--accent-primary) 6%, transparent);
    font-size: 13px;
  }
  .install-transfer.failed {
    border-color: color-mix(in srgb, var(--accent-danger) 35%, transparent);
    background: color-mix(in srgb, var(--accent-danger) 8%, transparent);
    color: var(--accent-danger);
  }
  .install-transfer.done { color: var(--accent-primary); }
  .install-transfer pre {
    margin: 4px 0 0;
    white-space: pre-wrap;
    color: inherit;
    font: inherit;
    font-size: 12px;
  }
  .icon-btn {
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    padding: 4px;
    border-radius: 6px;
  }
  .icon-btn:hover { color: var(--text-primary); background: var(--bg-tertiary); }
  .dep-list { margin-bottom: 14px; }
  .dep-list h4 { font-size: 13px; margin: 0 0 8px; color: var(--text-secondary); }
  .dep-entry {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 0;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    margin-bottom: 4px;
    font-size: 13px;
    border-left: 3px solid;
  }
  .dep-entry.required { background: color-mix(in srgb, var(--accent-primary) 8%, transparent); border-left-color: color-mix(in srgb, var(--accent-primary) 60%, transparent); }
  .dep-entry.optional { background: rgba(245,158,11,0.08); border-left-color: rgba(245,158,11,0.6); }
  .dep-entry.already-installed { opacity: 0.72; }
  .dep-target { font-weight: 600; }
  .dep-entry small { flex-basis: 100%; display: block; color: var(--text-muted); font-size: 12px; margin-top: 2px; }
  .dep-installed-pill {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-left: 8px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    color: var(--success, #22c55e);
    background: color-mix(in srgb, var(--success, #22c55e) 16%, transparent);
    padding: 2px 6px;
    border-radius: 4px;
  }
  .dep-installed-count {
    margin-left: 6px;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-muted);
    text-transform: none;
    letter-spacing: 0;
  }
  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 0;
    font-size: 14px;
    cursor: pointer;
  }
  .checkbox-row input { width: auto; }
  .muted { color: var(--text-muted); font-size: 13px; }

  @media (max-width: 1180px) {
    .graph-layout,
    .graph-list {
      flex-direction: column;
    }
  }
</style>
