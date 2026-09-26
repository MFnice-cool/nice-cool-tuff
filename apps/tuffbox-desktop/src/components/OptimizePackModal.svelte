<script lang="ts">
  import { X, Zap, Loader2, Check, Package } from "@lucide/svelte";
  import { api } from "../lib/api";
  import { isProjectLaunching, launchSessions, projectPath, projectInfo } from "../lib/store";
  import { launchWithFeedback } from "../lib/launch";
  import { toasts } from "../lib/toast";
  import { trapFocus } from "../lib/focusTrap";
  import { portal } from "../lib/portal";
  import { get } from "svelte/store";
  import ModPresetsModal from "./ModPresetsModal.svelte";

  let {
    open = $bindable(false),
    onApplied,
  }: {
    open?: boolean;
    onApplied?: () => void;
  } = $props();

  type Mode = "curated" | "fo" | "custom";

  type CuratedMod = {
    slug: string;
    name: string;
    projectId: string;
    alreadyInstalled: boolean;
    role: string;
  };

  type CustomMod = {
    slug: string;
    name: string;
    /** Exact jar name — present on FO rows, absent on custom offers. */
    fileName?: string;
    provider: string;
    projectId: string;
    versionId?: string | null;
    reason: string;
    risk: string;
    alreadyInstalled: boolean;
    selected: boolean;
  };

  type ConfigRow = {
    key: string;
    selected: boolean;
    op: string;
    path: string | null;
    reason: string;
    risk: string;
    raw: Record<string, unknown>;
  };

  let mode = $state<Mode>("curated");
  const projectLaunching = $derived(isProjectLaunching($projectPath, $launchSessions));
  let loading = $state(false);
  let applying = $state(false);
  let error = $state<string | null>(null);
  let warnings = $state<string[]>([]);
  let applyConfigs = $state(true);
  let useAiConfigs = $state(false);
  /** True only after a successful curated preview (not merely listed in JSON). */
  let curatedAvailable = $state(false);
  let curatedUnavailableMessage = $state<string | null>(null);
  let projectLoader = $state("");
  let projectMcVersion = $state("");
  let curatedMappedVersions = $state<string[]>([]);
  let curatedMeta = $state<{
    name: string;
    slug: string;
    versionNumber?: string;
    minecraftVersion: string;
    loader: string;
  } | null>(null);
  let curatedMods = $state<CuratedMod[]>([]);
  let customMods = $state<CustomMod[]>([]);
  let foMods = $state<CustomMod[]>([]);
  let foMeta = $state<{
    name: string;
    slug: string;
    versionNumber?: string;
    minecraftVersion: string;
    loader: string;
    modCount: number;
  } | null>(null);
  let catalogSource = $state<"bundled" | "supabase" | string>("bundled");
  let configRows = $state<ConfigRow[]>([]);
  let findings = $state<Record<string, unknown>[]>([]);
  let doneMessage = $state<string | null>(null);
  let sessionOpen = $state(false);
  let presetsOpen = $state(false);
  /** Non-reactive generation counter — must not be $state (close path would retrigger $effect). */
  let loadGen = 0;

  const selectedCustomCount = $derived(
    customMods.filter((m) => m.selected && !m.alreadyInstalled).length,
  );
  const selectedFoCount = $derived(
    foMods.filter((m) => m.selected && !m.alreadyInstalled).length,
  );
  const selectedConfigCount = $derived(
    applyConfigs ? configRows.filter((r) => r.selected).length : 0,
  );
  const curatedInstallCount = $derived(
    curatedMods.filter((m) => !m.alreadyInstalled).length,
  );
  const contextLabel = $derived.by(() => {
    const loader = curatedMeta?.loader || projectLoader || "—";
    const mc = curatedMeta?.minecraftVersion || projectMcVersion || "—";
    return `${loader} · MC ${mc}`;
  });
  const applyDisabled = $derived.by(() => {
    if (applying || loading) return true;
    if (mode === "curated") return !curatedAvailable;
    if (mode === "fo") return selectedFoCount === 0 && selectedConfigCount === 0;
    return selectedCustomCount === 0 && selectedConfigCount === 0;
  });
  const applyLabel = $derived.by(() => {
    if (mode === "curated") {
      if (!curatedAvailable) return "Apply";
      const parts: string[] = [];
      if (curatedInstallCount > 0) {
        parts.push(`${curatedInstallCount} mod${curatedInstallCount === 1 ? "" : "s"}`);
      }
      if (selectedConfigCount > 0) {
        parts.push(`${selectedConfigCount} config${selectedConfigCount === 1 ? "" : "s"}`);
      }
      return parts.length ? `Apply (${parts.join(", ")})` : "Apply (already installed)";
    }
    const modCount = mode === "fo" ? selectedFoCount : selectedCustomCount;
    const parts: string[] = [];
    if (modCount > 0) {
      parts.push(`${modCount} mod${modCount === 1 ? "" : "s"}`);
    }
    if (selectedConfigCount > 0) {
      parts.push(`${selectedConfigCount} config${selectedConfigCount === 1 ? "" : "s"}`);
    }
    return parts.length ? `Apply (${parts.join(", ")})` : "Apply";
  });

  function loaderIsFabricFamily(loader: string | undefined | null): boolean {
    const l = (loader ?? "").toLowerCase();
    return l === "fabric" || l === "quilt";
  }

  /** Backend redirects curated previews to the FO tab when the mapped project is a modpack. */
  function isFoRedirect(msg: string | null | undefined): boolean {
    return !!msg && msg.includes("Fabulously Optimized tab");
  }

  function close() {
    open = false;
  }

  function actionToRow(a: Record<string, unknown>, idx: number): ConfigRow {
    return {
      key: `${a.op ?? "edit_config"}-${a.path ?? idx}-${idx}`,
      selected: true,
      op: String(a.op ?? "edit_config"),
      path: (a.path as string | null) ?? null,
      reason: String(a.reason ?? ""),
      risk: String(a.risk ?? "low"),
      raw: a,
    };
  }

  function beginLoad(): number {
    const gen = loadGen + 1;
    loadGen = gen;
    loading = true;
    return gen;
  }

  function endLoad(gen: number) {
    if (gen === loadGen) loading = false;
  }

  function curatedEmptyCopy(): string {
    if (curatedUnavailableMessage) return curatedUnavailableMessage;
    const loader = projectLoader || "this loader";
    const mc = projectMcVersion || "this Minecraft version";
    if (curatedMappedVersions.length) {
      return `No curated pack for ${loader} · MC ${mc}. Mapped versions: ${curatedMappedVersions.join(", ")}. Switch to Custom for performance mods.`;
    }
    return `No curated pack for ${loader} · MC ${mc}. Switch to Custom, or publish a Modrinth project and add a row in optimize-packs.json.`;
  }

  async function bootstrap() {
    const gen = beginLoad();
    error = null;
    doneMessage = null;
    warnings = [];
    curatedMods = [];
    customMods = [];
    foMods = [];
    foMeta = null;
    configRows = [];
    findings = [];
    curatedMeta = null;
    useAiConfigs = false;
    curatedAvailable = false;
    curatedUnavailableMessage = null;
    curatedMappedVersions = [];

    const path = get(projectPath);
    if (!path) {
      error = "No project open.";
      endLoad(gen);
      return;
    }

    try {
      const listed = await api.mods.listCuratedOptimizePacks(path);
      if (gen !== loadGen) return;

      projectLoader = listed.loader || get(projectInfo)?.loaderKind || "";
      projectMcVersion = listed.minecraftVersion || "";
      curatedMappedVersions = (listed.entries ?? [])
        .map((e) => e.minecraftVersion)
        .filter(Boolean);
      curatedUnavailableMessage = listed.unavailableMessage ?? null;
      const listedAvailable = !!listed.available;
      curatedAvailable = false;

      const preferCurated =
        listedAvailable && loaderIsFabricFamily(listed.loader || projectLoader);
      mode = preferCurated ? "curated" : "custom";

      if (preferCurated) {
        try {
          await loadCurated(path);
          if (gen !== loadGen) return;
        } catch (curatedErr) {
          if (gen !== loadGen) return;
          const curatedMsg =
            curatedErr instanceof Error ? curatedErr.message : String(curatedErr);
          curatedUnavailableMessage = curatedMsg;
          if (isFoRedirect(curatedMsg)) {
            // Mapped project is a modpack (e.g. Fabulously Optimized) — the FO
            // tab is the curated choice for this instance, open it directly.
            curatedAvailable = false;
            mode = "fo";
            try {
              await loadFo(path);
              if (gen !== loadGen) return;
            } catch (foErr) {
              if (gen !== loadGen) return;
              mode = "custom";
              await loadCustom(path, false);
              if (gen !== loadGen) return;
              const foMsg = foErr instanceof Error ? foErr.message : String(foErr);
              warnings = [
                foMsg,
                "Opened Custom optimize instead — pick performance mods below.",
                ...warnings,
              ];
            }
          } else {
            // Mapped in JSON but unpublished / missing on Modrinth — keep Custom usable.
            curatedAvailable = false;
            mode = "custom";
            await loadCustom(path, false);
            if (gen !== loadGen) return;
            warnings = [
              curatedMsg,
              "Opened Custom optimize instead — pick performance mods below.",
              ...warnings,
            ];
          }
        }
      } else {
        await loadCustom(path, false);
        if (gen !== loadGen) return;
        if (!listedAvailable && curatedUnavailableMessage) {
          warnings = [curatedUnavailableMessage, ...warnings];
        }
      }
      error = null;
    } catch (e) {
      if (gen !== loadGen) return;
      mode = "custom";
      curatedAvailable = false;
      try {
        await loadCustom(path, false);
        if (gen !== loadGen) return;
        error = null;
        const msg = e instanceof Error ? e.message : String(e);
        if (msg && !warnings.includes(msg)) warnings = [...warnings, msg];
      } catch (e2) {
        if (gen !== loadGen) return;
        error = e2 instanceof Error ? e2.message : String(e2);
      }
    } finally {
      endLoad(gen);
    }
  }

  async function loadCurated(path: string) {
    const preview = await api.mods.previewCuratedOptimizePack(path);
    curatedMeta = {
      name: preview.pack.name,
      slug: preview.pack.slug,
      versionNumber: preview.pack.versionNumber,
      minecraftVersion: preview.minecraftVersion,
      loader: preview.loader,
    };
    projectLoader = preview.loader || projectLoader;
    projectMcVersion = preview.minecraftVersion || projectMcVersion;
    curatedMods = preview.mods.filter((m) => m.role !== "conflicts" && m.role !== "breaks_with");
    warnings = preview.warnings ?? [];
    configRows = (preview.configActions ?? []).map((a, i) =>
      actionToRow(a as Record<string, unknown>, i),
    );
    curatedAvailable = true;
    curatedUnavailableMessage = null;
  }

  async function loadFo(path: string) {
    const preview = await api.mods.previewFoOptimizePack(path);
    foMeta = {
      name: preview.pack.name,
      slug: preview.pack.slug,
      versionNumber: preview.pack.versionNumber,
      minecraftVersion: preview.minecraftVersion,
      loader: preview.loader,
      modCount: preview.pack.modCount,
    };
    foMods = (preview.mods ?? []).map((m) => ({
      ...m,
      selected: !m.alreadyInstalled,
    }));
    warnings = preview.warnings ?? [];
    configRows = (preview.configActions ?? []).map((a, i) =>
      actionToRow(a as Record<string, unknown>, i),
    );
    projectLoader = preview.loader || projectLoader;
    projectMcVersion = preview.minecraftVersion || projectMcVersion;
  }

  async function loadCustom(path: string, ai: boolean) {
    const plan = await api.mods.buildOptimizePlan(ai, path);
    // Do not overwrite curatedAvailable from plan.curatedAvailable — that only
    // means a JSON mapping exists, not that Modrinth preview succeeded.
    catalogSource = plan.catalogSource || "bundled";
    customMods = (plan.mods ?? []).map((m) => ({
      ...m,
      selected: !m.alreadyInstalled,
    }));
    warnings = plan.warnings ?? [];
    findings = plan.findings ?? [];
    const actions = (plan.plan?.actions as Record<string, unknown>[] | undefined) ?? [];
    configRows = actions.map((a, i) => actionToRow(a, i));
    const aiDiffs = (plan as { aiDiffs?: { path: string; ok: boolean; afterExcerpt?: string }[] })
      .aiDiffs;
    if (ai && Array.isArray(aiDiffs) && aiDiffs.length > 0) {
      const ok = aiDiffs.filter((d) => d.ok).length;
      warnings = [
        ...warnings,
        `AI preview: ${ok}/${aiDiffs.length} config patch(es) dry-run OK — review reasons for source: / cite: tags.`,
      ];
    }
    curatedMeta = {
      name: "Custom optimize",
      slug: "",
      minecraftVersion: plan.minecraftVersion,
      loader: plan.loader,
    };
    projectLoader = plan.loader || projectLoader;
    projectMcVersion = plan.minecraftVersion || projectMcVersion;
  }

  async function switchMode(next: Mode) {
    if (mode === next || loading || applying) return;
    mode = next;
    error = null;
    doneMessage = null;

    // Unavailable curated: stay on the tab and show an empty-state explanation.
    if (next === "curated" && !curatedAvailable) {
      return;
    }

    const gen = beginLoad();
    const path = get(projectPath);
    if (!path) {
      error = "No project open.";
      endLoad(gen);
      return;
    }
    try {
      if (next === "curated") {
        try {
          await loadCurated(path);
          if (gen !== loadGen) return;
          error = null;
        } catch (e) {
          if (gen !== loadGen) return;
          curatedAvailable = false;
          curatedUnavailableMessage = e instanceof Error ? e.message : String(e);
          if (isFoRedirect(curatedUnavailableMessage)) {
            mode = "fo";
            try {
              await loadFo(path);
              if (gen !== loadGen) return;
              error = null;
            } catch (foErr) {
              if (gen !== loadGen) return;
              mode = "custom";
              await loadCustom(path, useAiConfigs);
              if (gen !== loadGen) return;
              warnings = [
                foErr instanceof Error ? foErr.message : String(foErr),
                "Opened Custom optimize instead — pick performance mods below.",
                ...warnings,
              ];
              error = null;
            }
          } else {
            mode = "custom";
            await loadCustom(path, useAiConfigs);
            if (gen !== loadGen) return;
            warnings = [
              curatedUnavailableMessage,
              "Opened Custom optimize instead — pick performance mods below.",
              ...warnings,
            ];
            error = null;
          }
        }
      } else if (next === "fo") {
        await loadFo(path);
        if (gen !== loadGen) return;
        error = null;
      } else {
        await loadCustom(path, useAiConfigs);
        if (gen !== loadGen) return;
        error = null;
      }
    } catch (e) {
      if (gen !== loadGen) return;
      error = e instanceof Error ? e.message : String(e);
    } finally {
      endLoad(gen);
    }
  }

  async function refreshCustomWithAi() {
    const path = get(projectPath);
    if (!path || mode !== "custom") return;
    const gen = beginLoad();
    error = null;
    try {
      await loadCustom(path, useAiConfigs);
      if (gen !== loadGen) return;
    } catch (e) {
      if (gen !== loadGen) return;
      error = e instanceof Error ? e.message : String(e);
    } finally {
      endLoad(gen);
    }
  }

  function selectedConfigPlan(): Record<string, unknown> | null {
    const actions = configRows.filter((r) => r.selected).map((r) => r.raw);
    if (!actions.length) return null;
    return {
      schemaVersion: 1,
      humanExplanation:
        mode === "curated"
          ? "Optimize pack (curated) config templates"
          : mode === "fo"
            ? "Optimize pack (FO set) config templates"
            : "Optimize pack custom: safe client/performance config patches",
      confidence: 0.85,
      suspectedMods: [],
      needsUserReview: true,
      source: "optimize_pack",
      matchedCaseIds: [],
      actions,
    };
  }

  async function apply() {
    const path = get(projectPath);
    if (!path || applying || applyDisabled) return;
    applying = true;
    error = null;
    doneMessage = null;
    try {
      if (mode === "curated") {
        const configPlan = applyConfigs ? selectedConfigPlan() : null;
        const result = await api.mods.installCuratedOptimizePack(
          applyConfigs && !!configPlan,
          configPlan,
          path,
        );
        const install = result.install as { ok?: boolean; error?: string } | undefined;
        if (install && install.ok === false) {
          throw new Error(install.error || "Curated install failed");
        }
        doneMessage = "Curated optimize pack installed.";
        toasts.success("Optimize pack applied (curated)");
      } else {
        // FO and Custom share the selectable-offer shape; FO installs exact
        // pinned builds, Custom resolves latest compatible builds.
        const source = mode === "fo" ? foMods : customMods;
        const mods = source
          .filter((m) => m.selected && !m.alreadyInstalled)
          .map((m) => ({
            slug: m.slug,
            name: m.name,
            provider: m.provider,
            projectId: m.projectId,
            versionId: m.versionId ?? null,
            reason: m.reason,
            risk: m.risk,
            alreadyInstalled: m.alreadyInstalled,
          }));
        const configPlan = applyConfigs ? selectedConfigPlan() : null;
        const applyPlan =
          mode === "fo" ? api.mods.applyFoOptimizePlan : api.mods.applyOptimizeCustomPlan;
        const result = await applyPlan(
          mods,
          applyConfigs && !!configPlan,
          configPlan,
          path,
        );
        const errs = (result.errors as string[] | undefined) ?? [];
        if (errs.length) {
          error = errs.join("; ");
          toasts.error("Optimize pack finished with errors");
        } else {
          doneMessage = `Installed ${((result.installed as string[]) ?? []).length} item(s).`;
          toasts.success(mode === "fo" ? "Optimize pack applied (FO set)" : "Optimize pack applied (custom)");
        }
      }
      onApplied?.();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      toasts.error(error);
    } finally {
      applying = false;
    }
  }

  async function testLaunch() {
    const path = get(projectPath);
    if (!path || projectLaunching) return;
    await launchWithFeedback({ path, profile: "client" });
  }

  // One-shot open: always re-bootstrap when the dialog opens (avoid stale session).
  $effect(() => {
    if (!open) {
      sessionOpen = false;
      // Invalidate in-flight loads so finally cannot leave loading stuck.
      loadGen += 1;
      loading = false;
      return;
    }
    if (sessionOpen) return;
    sessionOpen = true;
    void bootstrap();
  });
</script>

{#if open}
  <div
    class="modal-backdrop"
    role="button"
    tabindex="-1"
    use:portal
    onclick={(e) => e.target === e.currentTarget && close()}
    onkeydown={() => {}}
  >
    <div
      class="modal optimize-dialog"
      role="dialog"
      aria-modal="true"
      use:trapFocus={{ onEscape: close }}
    >
      <div class="modal-header">
        <div>
          <h2><Zap size={18} /> Optimize pack</h2>
          <p>
            Add performance mods and safe config patches. Curated uses the author Modrinth pack;
            FO installs the exact Fabulously Optimized mod set (pinned builds);
            Custom resolves missing whitelist mods (Modrinth → CurseForge).
          </p>
        </div>
        <div class="header-actions">
          <button
            class="secondary presets-btn"
            type="button"
            onclick={() => { presetsOpen = true; }}
            title="Manage your own mod presets (Modrinth / CurseForge)"
          >
            <Package size={14} /> Presets
          </button>
          <button class="icon-btn" type="button" onclick={close} aria-label="Close"><X size={18} /></button>
        </div>
      </div>

      <div class="mode-tabs" role="tablist">
        <button
          type="button"
          class:active={mode === "curated"}
          role="tab"
          aria-selected={mode === "curated"}
          disabled={loading || applying}
          onclick={() => switchMode("curated")}
        >
          Curated pack
          {#if !curatedAvailable}<span class="hint">n/a</span>{/if}
        </button>
        <button
          type="button"
          class:active={mode === "fo"}
          role="tab"
          aria-selected={mode === "fo"}
          disabled={loading || applying}
          title="Fabulously Optimized — the exact tested mod set (pinned builds)"
          onclick={() => switchMode("fo")}
        >
          Fabulously Optimized
        </button>
        <button
          type="button"
          class:active={mode === "custom"}
          role="tab"
          aria-selected={mode === "custom"}
          disabled={loading || applying}
          onclick={() => switchMode("custom")}
        >
          Custom
        </button>
      </div>

      <div class="opt-body">
      {#if loading}
        <div class="opt-status"><Loader2 size={16} class="spin" /> Loading plan…</div>
      {:else if error}
        <div class="opt-error">{error}</div>
      {/if}

      {#if doneMessage}
        <div class="opt-done"><Check size={14} /> {doneMessage}</div>
      {/if}

      {#if !loading && (projectLoader || projectMcVersion || curatedMeta)}
        <p class="opt-meta">
          {contextLabel}
          {#if mode === "curated" && curatedAvailable && curatedMeta?.name}
            · {curatedMeta.name}
            {#if curatedMeta.versionNumber}
              <code>v{curatedMeta.versionNumber}</code>
            {/if}
          {:else if mode === "fo" && foMeta}
            · {foMeta.name}
            {#if foMeta.versionNumber}
              <code>v{foMeta.versionNumber}</code>
            {/if}
            · {foMeta.modCount} mods
          {:else if mode === "custom"}
            · Custom performance list
            {#if catalogSource === "supabase"}
              · FO catalog (DB)
            {/if}
          {/if}
        </p>
      {/if}

      {#if warnings.length && !loading}
        <ul class="opt-warnings">
          {#each warnings as w, i (`${i}-${w}`)}
            <li>{w}</li>
          {/each}
        </ul>
      {/if}

      {#if mode === "curated" && !loading}
        {#if !curatedAvailable}
          <div class="opt-empty-block">
            <p class="opt-empty">{curatedEmptyCopy()}</p>
            {#if isFoRedirect(curatedUnavailableMessage)}
              <button
                type="button"
                class="secondary opt-empty-cta"
                disabled={applying}
                onclick={() => switchMode("fo")}
              >
                Open FO set
              </button>
            {/if}
            <button
              type="button"
              class="secondary opt-empty-cta"
              disabled={applying}
              onclick={() => switchMode("custom")}
            >
              Open Custom optimize
            </button>
          </div>
        {:else}
          <div class="opt-section">
            <h3>
              Mods in pack
              <span class="count">{curatedInstallCount} to install · {curatedMods.length} total</span>
            </h3>
            {#if !curatedMods.length}
              <p class="opt-empty">Pack preview returned no installable mods.</p>
            {:else}
              <div class="opt-list">
                {#each curatedMods as m (m.slug + m.role)}
                  <div class="opt-row">
                    <div class="opt-meta-col">
                      <strong>{m.name || m.slug}</strong>
                      <code>{m.slug}</code>
                      <span class="role">{m.role}</span>
                    </div>
                    {#if m.alreadyInstalled}
                      <span class="pill ok">installed</span>
                    {:else}
                      <span class="pill add">will install</span>
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      {/if}

      {#if mode === "fo" && !loading}
        <div class="opt-section">
          <h3>
            Fabulously Optimized mod set
            {#if foMods.length}
              <span class="count">{selectedFoCount} selected · {foMods.length} total</span>
            {/if}
          </h3>
          {#if !foMods.length}
            <p class="opt-empty">
              FO set is empty for {contextLabel} — every entry is already installed or unresolvable.
            </p>
          {:else}
            <div class="opt-list">
              {#each foMods as m (m.provider + m.projectId + m.slug)}
                <label class="opt-row selectable">
                  <input
                    type="checkbox"
                    bind:checked={m.selected}
                    disabled={m.alreadyInstalled || applying}
                  />
                  <div class="opt-meta-col">
                    <strong>{m.name}</strong>
                    <code>{m.slug}</code>
                    {#if m.fileName}<span class="muted">{m.fileName}</span>{/if}
                  </div>
                  {#if m.alreadyInstalled}
                    <span class="pill ok">installed</span>
                  {:else}
                    <span class="pill">pinned</span>
                  {/if}
                </label>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      {#if mode === "custom" && !loading}
        <div class="opt-section">
          <h3>
            Missing performance mods
            {#if customMods.length}
              <span class="count">{selectedCustomCount} selected · {customMods.length} available</span>
            {/if}
          </h3>
          {#if !customMods.length}
            <p class="opt-empty">
              All whitelist performance mods are already present (or none resolve for {contextLabel}).
            </p>
          {:else}
            <div class="opt-list">
              {#each customMods as m (m.provider + m.projectId + m.slug)}
                <label class="opt-row selectable">
                  <input
                    type="checkbox"
                    bind:checked={m.selected}
                    disabled={m.alreadyInstalled || applying}
                  />
                  <div class="opt-meta-col">
                    <strong>{m.name}</strong>
                    <code>{m.slug}</code>
                    <span class="muted">{m.reason}</span>
                  </div>
                  <span class="pill">{m.provider}</span>
                </label>
              {/each}
            </div>
          {/if}
        </div>

        <label class="opt-check">
          <input
            type="checkbox"
            bind:checked={useAiConfigs}
            disabled={applying || loading}
            onchange={() => refreshCustomWithAi()}
          />
          Use AI for configs (Config Advisor + templates; review before apply)
        </label>
      {/if}

      {#if !loading && configRows.length && (mode === "custom" || mode === "fo" || curatedAvailable)}
        <div class="opt-section">
          <h3>
            Config patches
            <span class="count">{selectedConfigCount} selected · {configRows.length} total</span>
          </h3>
          <div class="opt-list">
            {#each configRows as row (row.key)}
              <label class="opt-row selectable">
                <input
                  type="checkbox"
                  bind:checked={row.selected}
                  disabled={applying || !applyConfigs}
                />
                <div class="opt-meta-col">
                  <strong>{row.path ?? row.op}</strong>
                  {#if row.reason}<span class="muted">{row.reason}</span>{/if}
                </div>
                <span class="risk">{row.risk}</span>
              </label>
            {/each}
          </div>
        </div>
      {/if}

      {#if mode === "custom" && findings.length && !loading}
        <details class="opt-findings">
          <summary>Performance audit ({findings.length})</summary>
          <ul>
            {#each findings as f, i (i)}
              <li>{String(f.message ?? f.code ?? "finding")}</li>
            {/each}
          </ul>
        </details>
      {/if}

      {#if !loading && (mode === "custom" || mode === "fo" || curatedAvailable)}
        <label class="opt-check">
          <input type="checkbox" bind:checked={applyConfigs} disabled={applying} />
          Apply safe config templates after installing mods
        </label>
      {/if}
      </div>

      <div class="opt-footer">
        <button class="ghost" type="button" onclick={close} disabled={applying}>Close</button>
        {#if doneMessage}
          <button class="secondary" type="button" onclick={testLaunch} disabled={applying || projectLaunching}>
            {projectLaunching ? "Launching…" : "Test launch"}
          </button>
        {/if}
        <button
          type="button"
          disabled={applyDisabled}
          onclick={apply}
        >
          {#if applying}
            <Loader2 size={14} class="spin" /> Applying…
          {:else}
            {applyLabel}
          {/if}
        </button>
      </div>
    </div>
  </div>
{/if}

<ModPresetsModal bind:open={presetsOpen} />

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 80;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(10px);
  }

  .optimize-dialog {
    width: min(640px, 100%);
    max-height: min(88vh, calc(100vh - 32px));
    display: flex;
    flex-direction: column;
    overflow: hidden;
    padding: 18px 20px 16px;
    border-radius: var(--border-radius-lg);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    box-shadow: 0 30px 100px rgba(0, 0, 0, 0.45);
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 16px;
    margin-bottom: 14px;
    flex-shrink: 0;
  }

  .modal-header h2 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 4px;
    font-size: 18px;
  }

  .modal-header p {
    margin: 0;
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.45;
  }

  .icon-btn {
    width: 32px;
    height: 32px;
    padding: 0;
    flex-shrink: 0;
    background: transparent;
    color: var(--text-muted);
  }

  .icon-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-secondary);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .presets-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    white-space: nowrap;
  }

  .opt-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    overscroll-behavior: contain;
    scrollbar-gutter: stable;
    padding-right: 2px;
  }

  .opt-footer {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--border-color);
  }
  .mode-tabs {
    display: flex;
    gap: 8px;
    margin: 0 0 12px;
    flex-shrink: 0;
  }
  .mode-tabs button {
    flex: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    cursor: pointer;
  }
  .mode-tabs button.active {
    color: var(--text-primary);
    border-color: var(--accent, #6ee7b7);
    background: color-mix(in srgb, var(--accent, #6ee7b7) 12%, var(--bg-tertiary));
  }
  .mode-tabs .hint {
    font-size: 12px;
    text-transform: uppercase;
    opacity: 0.7;
  }
  .opt-status,
  .opt-done,
  .opt-error {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 10px;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    font-size: 13px;
  }
  .opt-status {
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }
  .opt-done {
    background: rgba(34, 197, 94, 0.12);
    color: #86efac;
    border: 1px solid rgba(34, 197, 94, 0.28);
  }
  .opt-error {
    background: rgba(239, 68, 68, 0.12);
    color: #fca5a5;
    border: 1px solid rgba(239, 68, 68, 0.28);
  }
  .opt-meta {
    margin: 0 0 10px;
    font-size: 13px;
    color: var(--text-muted);
  }
  .opt-warnings {
    margin: 0 0 12px;
    padding: 8px 10px 8px 24px;
    border-radius: var(--border-radius-sm);
    background: rgba(245, 158, 11, 0.1);
    border: 1px solid rgba(245, 158, 11, 0.28);
    color: #fde68a;
    font-size: 12px;
  }
  .opt-section {
    margin-bottom: 14px;
  }
  .opt-section h3 {
    margin: 0 0 8px;
    font-size: 13px;
    font-weight: 600;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
  }
  .opt-section h3 .count {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-muted);
  }
  .opt-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 220px;
    overflow: auto;
  }
  .opt-row {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
  }
  .opt-row.selectable {
    cursor: pointer;
  }
  .opt-meta-col {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .opt-meta-col strong {
    font-size: 13px;
  }
  .opt-meta-col code,
  .opt-meta-col .muted,
  .opt-meta-col .role {
    font-size: 12px;
    color: var(--text-muted);
  }
  .pill {
    font-size: 12px;
    text-transform: uppercase;
    font-weight: 700;
    padding: 2px 6px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    color: var(--text-muted);
    white-space: nowrap;
  }
  .pill.ok {
    color: #86efac;
    border-color: rgba(34, 197, 94, 0.35);
  }
  .pill.add {
    color: #93c5fd;
    border-color: rgba(59, 130, 246, 0.35);
  }
  .risk {
    font-size: 12px;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .opt-check {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    margin: 0 0 10px;
  }
  .opt-empty {
    margin: 0;
    font-size: 13px;
    color: var(--text-muted);
    line-height: 1.45;
  }
  .opt-empty-block {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 10px;
    margin: 0 0 14px;
    padding: 12px;
    border-radius: var(--border-radius-md);
    border: 1px dashed var(--border-color);
    background: var(--bg-tertiary);
  }
  .opt-empty-cta {
    margin: 0;
  }
  .opt-findings {
    margin: 0 0 12px;
    font-size: 12px;
    color: var(--text-secondary);
  }
  .opt-findings ul {
    margin: 6px 0 0;
    padding-left: 18px;
  }
  :global(.spin) {
    animation: opt-spin 0.8s linear infinite;
  }
  @keyframes opt-spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
