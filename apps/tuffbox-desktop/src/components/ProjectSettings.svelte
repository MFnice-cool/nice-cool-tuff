<script lang="ts">
  import {
    ArrowLeft,
    Save,
    Cpu,
    Container,
    Coffee,
    Terminal,
    Search,
    Database,
    RefreshCw,
    AlertTriangle,
    FileCog,
    User,
    Check,
    Copy,
    RotateCcw,
    Sliders,
    Sparkles,
    Shield,
    Share2,
  } from "@lucide/svelte";
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { projectInfo, projectPath, recentProjects } from "../lib/store";
  import EmptyState from "./EmptyState.svelte";
  import JavaPickerModal from "./JavaPickerModal.svelte";

  let {
    onBack = () => {},
    showBack = true,
    stayAfterSave = false,
  }: {
    onBack?: () => void;
    showBack?: boolean;
    stayAfterSave?: boolean;
  } = $props();

  const loaders = [
    { id: "vanilla", label: "Vanilla" },
    { id: "fabric", label: "Fabric" },
    { id: "forge", label: "Forge" },
    { id: "neoforge", label: "NeoForge" },
    { id: "quilt", label: "Quilt" },
  ];

  const MEMORY_PRESETS = [
    { label: "2 GB", mb: 2048 },
    { label: "4 GB", mb: 4096 },
    { label: "6 GB", mb: 6144 },
    { label: "8 GB", mb: 8192 },
    { label: "10 GB", mb: 10240 },
    { label: "12 GB", mb: 12288 },
    { label: "16 GB", mb: 16384 },
  ];

  const JVM_PRESETS = [
    {
      id: "default",
      label: "Default G1GC",
      desc: "Standard Java Garbage Collector",
      flags: "-XX:+UseG1GC",
    },
    {
      id: "optimized-g1",
      label: "Optimized G1GC",
      desc: "Balanced throughput & lower pause spikes",
      flags: "-XX:+UseG1GC -XX:+ParallelRefProcEnabled -XX:MaxGCPauseMillis=200 -XX:+UnlockExperimentalVMOptions -XX:+DisableExplicitGC -XX:+AlwaysPreTouch",
    },
    {
      id: "aikar",
      label: "Aikar's Optimized Flags",
      desc: "Community standard for minimal stutters",
      flags: "-XX:+UseG1GC -XX:+ParallelRefProcEnabled -XX:MaxGCPauseMillis=200 -XX:+UnlockExperimentalVMOptions -XX:+DisableExplicitGC -XX:+AlwaysPreTouch -XX:G1NewSizePercent=30 -XX:G1MaxNewSizePercent=40 -XX:G1ReservePercent=20 -XX:G1HeapWastePercent=5 -XX:G1MixedGCCountTarget=4 -XX:InitiatingHeapOccupancyPercent=15 -XX:G1MixedGCLiveThresholdPercent=90 -XX:G1RSetUpdatingPauseTimePercent=5 -XX:SurvivorRatio=32 -XX:+PerfDisableSharedMem -XX:MaxTenuringThreshold=1",
    },
    {
      id: "shenandoah",
      label: "Shenandoah GC",
      desc: "Ultra-low pause latency (Java 17+)",
      flags: "-XX:+UseShenandoahGC -XX:+UnlockExperimentalVMOptions -XX:ShenandoahGCHeuristics=adaptive",
    },
    {
      id: "zgc",
      label: "Generational ZGC",
      desc: "Sub-millisecond pauses (Java 21+)",
      flags: "-XX:+UseZGC -XX:+ZGenerational",
    },
  ];

  let memory = $state($projectInfo?.memoryMb ?? 4096);
  let memoryManualMb = $state($projectInfo?.memoryMb ?? 4096);
  let jvmArgs = $state(($projectInfo?.jvmArgs ?? []).join(" "));
  let javaPath = $state($projectInfo?.javaPath ?? "Auto-detect");
  let javaVersion = $state("");
  let playerName = $state($projectInfo?.playerName ?? "Player");

  let mcVersion = $state($projectInfo?.minecraftVersion ?? "");
  let loader = $state($projectInfo?.loaderKind ?? "vanilla");
  let loaderVersion = $state($projectInfo?.loaderVersion ?? "");

  let mcVersions = $state<{ id: string; popular: boolean }[]>([]);
  let loaderVersions = $state<{ id: string; stable: boolean }[]>([]);
  let showJavaPicker = $state(false);
  let saving = $state(false);
  let loading = $state(false);
  let error = $state("");
  let successMessage = $state("");
  let dirty = $state(false);

  // Per-pack auto-tune preview (heap estimate + JVM profile).
  let heapRec = $state<{
    memoryMb: number;
    baseMb: number;
    categoryMb: number;
    modCount: number;
  } | null>(null);
  let jvmRec = $state<{ profile: string; args: string[]; note: string } | null>(null);
  let tuneLoading = $state(false);
  let tuneLoadedFor = $state<string | null>(null);

  async function loadTuneRecs() {
    if (!$projectPath || tuneLoading) return;
    tuneLoading = true;
    try {
      const [heap, jvm] = await Promise.all([
        invoke("recommend_heap_cmd", { path: $projectPath }),
        invoke("recommend_jvm_cmd", { path: $projectPath }),
      ]);
      heapRec = heap as typeof heapRec;
      jvmRec = jvm as typeof jvmRec;
    } catch {
      heapRec = null;
      jvmRec = null;
    } finally {
      tuneLoading = false;
    }
  }

  // The view stays mounted across project switches — refetch per project.
  $effect(() => {
    const p = $projectPath;
    if (p && p !== tuneLoadedFor) {
      tuneLoadedFor = p;
      void loadTuneRecs();
    }
  });

  // Schema status
  let schemaVersion = $state("");
  let schemaNeedsMigration = $state(false);
  let schemaLoading = $state(false);

  async function loadSchemaStatus() {
    if (!$projectPath) return;
    schemaLoading = true;
    try {
      const status: any = await invoke("get_project_schema_status", { path: $projectPath });
      schemaVersion = status.detected ?? "?";
      schemaNeedsMigration = status.needsMigration ?? false;
    } catch {
      schemaVersion = "?";
    } finally {
      schemaLoading = false;
    }
  }

  // ── Shared options.txt sync (docs/17) ─────────────────────────────────
  let optionsManaged = $state(false);
  let optionsGroup = $state("");
  let optionsHasTemplate = $state(false);
  let optionsBusy = $state(false);

  async function loadOptionsStatus() {
    if (!$projectPath) return;
    try {
      const s: any = await invoke("options_sync_status", { path: $projectPath });
      optionsManaged = s.managed ?? false;
      optionsGroup = s.groupId ?? "";
      optionsHasTemplate = s.hasGroupTemplate ?? false;
    } catch {
      /* status stays at defaults */
    }
  }

  async function enableOptionsSync() {
    if (!$projectPath) return;
    optionsBusy = true;
    error = "";
    successMessage = "";
    try {
      const imported: boolean = await invoke("options_sync_enable", { path: $projectPath });
      if (!imported && !optionsHasTemplate) {
        successMessage = "Enabled shared options: current options.txt will be adopted on first launch.";
      } else {
        successMessage = "Shared options.txt enabled for this Minecraft version.";
      }
      await loadOptionsStatus();
    } catch (e) {
      error = `${e}`;
    } finally {
      optionsBusy = false;
    }
  }

  async function disableOptionsSync() {
    if (!$projectPath) return;
    optionsBusy = true;
    error = "";
    successMessage = "";
    try {
      await invoke("options_sync_disable", { path: $projectPath });
      successMessage = "Instance options.txt unlinked (now independent).";
      await loadOptionsStatus();
    } catch (e) {
      error = `${e}`;
    } finally {
      optionsBusy = false;
    }
  }

  async function pushOptionsToGroup() {
    if (!$projectPath) return;
    optionsBusy = true;
    error = "";
    successMessage = "";
    try {
      await invoke("options_sync_push", { path: $projectPath });
      successMessage = "Pushed local options.txt as template for all matching instances.";
      await loadOptionsStatus();
    } catch (e) {
      error = `${e}`;
    } finally {
      optionsBusy = false;
    }
  }

  // ── Per-pack dedup store opt-out (docs/17) ─────────────────────────────
  let dedupEnabled = $state(true);
  let dedupBusy = $state(false);

  async function loadDedupStatus() {
    if (!$projectPath) return;
    try {
      const s: any = await invoke("dedup_project_status", { path: $projectPath });
      dedupEnabled = s.enabled !== false;
    } catch {
      /* status stays at defaults */
    }
  }

  async function setDedupEnabled(next: boolean) {
    if (!$projectPath || dedupBusy) return;
    dedupBusy = true;
    error = "";
    successMessage = "";
    try {
      const r: any = await invoke("dedup_project_set_enabled", {
        path: $projectPath,
        enabled: next,
      });
      dedupEnabled = r.enabled !== false;
      if (next) {
        const linked = r.linked ?? 0;
        const recorded = r.recorded ?? 0;
        successMessage =
          `Deduplication on — ${linked} file(s) linked into the shared store` +
          (recorded ? `, ${recorded} new unique file(s) recorded` : "") +
          ".";
      } else {
        const made = r.materialized ?? 0;
        successMessage =
          made > 0
            ? `Deduplication off — ${made} file(s) are now independent copies owned by this pack.`
            : "Deduplication off — this pack already owns all of its files.";
      }
      await loadDedupStatus();
    } catch (e) {
      error = `${e}`;
    } finally {
      dedupBusy = false;
    }
  }

  async function migrateSchema() {
    if (!$projectPath) return;
    saving = true;
    error = "";
    successMessage = "";
    try {
      await invoke("migrate_project_schema", { path: $projectPath });
      successMessage = "Project schema migrated successfully.";
      await loadSchemaStatus();
    } catch (e) {
      error = `${e}`;
    } finally {
      saving = false;
    }
  }

  function markDirty() {
    dirty = true;
    successMessage = "";
  }

  function onMemorySliderChange(val: number) {
    memory = val;
    memoryManualMb = val;
    markDirty();
  }

  function onMemoryManualInput(val: number) {
    const clamped = Math.max(512, Math.min(65536, val || 1024));
    memoryManualMb = val;
    memory = clamped;
    markDirty();
  }

  function applyJvmPreset(flags: string) {
    jvmArgs = flags;
    markDirty();
    successMessage = "JVM preset applied.";
  }

  async function copyJvmArgs() {
    try {
      await navigator.clipboard.writeText(jvmArgs);
      successMessage = "JVM arguments copied to clipboard.";
    } catch {
      error = "Failed to copy JVM arguments.";
    }
  }

  function resetJvmArgs() {
    jvmArgs = "";
    markDirty();
    successMessage = "Reset to auto-tune (empty JVM args).";
  }

  onMount(() => {
    loading = true;
    error = "";
    void (async () => {
      try {
        const info = await invoke("validate_project", { path: $projectPath });
        applyProjectInfo(info as any);
      } catch {
        /* keep defaults */
      }
    })();
    void detectJavaPreview();
    void loadSchemaStatus();
    void loadOptionsStatus();
    void loadDedupStatus();
    void (async () => {
      try {
        const versions = (await invoke("get_minecraft_versions")) as {
          id: string;
          popular: boolean;
        }[];
        mcVersions = versions;
        await loadLoaderVersions();
      } catch (e) {
        error = `${e}`;
      } finally {
        loading = false;
      }
    })();

    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "s") {
        e.preventDefault();
        if ($projectPath && !saving) {
          void save();
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  let loadingLoader = $state(false);
  async function loadLoaderVersions() {
    if (loadingLoader) return;
    if (loader === "vanilla") {
      loaderVersions = [];
      loaderVersion = "";
      return;
    }
    loadingLoader = true;
    try {
      loaderVersions = await invoke("get_loader_versions", {
        loader,
        minecraftVersion: mcVersion,
      });
      if (!loaderVersions.some((v) => v.id === loaderVersion)) {
        loaderVersion = loaderVersions.find((v) => v.stable)?.id ?? loaderVersions[0]?.id ?? "";
      }
    } catch {
      loaderVersions = [];
      loaderVersion = "";
    } finally {
      loadingLoader = false;
    }
  }

  async function detectJavaPreview() {
    if (javaPath && javaPath !== "Auto-detect") {
      javaVersion = (await invoke("get_java_version", { path: javaPath }).catch(() => "")) as string;
    } else {
      javaVersion = (await invoke("get_default_java_version").catch(() => "")) as string;
    }
  }

  function applyProjectInfo(info: {
    minecraftVersion?: string;
    loaderKind?: string;
    loaderVersion?: string;
    memoryMb?: number;
    jvmArgs?: string[];
    javaPath?: string | null;
    playerName?: string | null;
  }) {
    if (info.minecraftVersion) mcVersion = info.minecraftVersion;
    if (info.loaderKind) loader = info.loaderKind;
    if (info.loaderVersion) loaderVersion = info.loaderVersion;
    if (info.memoryMb) {
      memory = info.memoryMb;
      memoryManualMb = info.memoryMb;
    }
    jvmArgs = (info.jvmArgs ?? []).join(" ");
    if (info.javaPath) javaPath = info.javaPath;
    if (info.playerName) playerName = info.playerName;
  }

  async function save() {
    if (!$projectPath) return;
    saving = true;
    error = "";
    successMessage = "";
    try {
      await invoke("update_project_settings", {
        path: $projectPath,
        minecraftVersion: mcVersion,
        loader,
        loaderVersion,
        javaPath: javaPath === "Auto-detect" ? null : javaPath,
        memoryMb: memory,
        jvmArgs: jvmArgs.split(/\s+/).filter(Boolean),
        playerName: playerName.trim() || null,
      });
      const info = await invoke("validate_project", { path: $projectPath });
      applyProjectInfo(info as any);
      projectInfo.set(info as any);
      recentProjects.updateInfo($projectPath, info as any);
      dirty = false;
      successMessage = "Settings saved successfully.";
      if (!stayAfterSave) onBack();
    } catch (e) {
      error = `${e}`;
    } finally {
      saving = false;
    }
  }

  async function onJavaSelected(path: string) {
    javaPath = path;
    markDirty();
    await detectJavaPreview();
  }

  function formatMemory(mb: number) {
    if (mb >= 1024) {
      const gb = mb / 1024;
      return Number.isInteger(gb) ? `${gb} GB` : `${gb.toFixed(1)} GB`;
    }
    return `${mb} MB`;
  }

  $effect(() => {
    if (mcVersions.length > 0 && loader !== "vanilla" && loaderVersions.length === 0) {
      loadLoaderVersions();
    }
  });

  function onLoaderChange() {
    loaderVersions = [];
    loaderVersion = "";
    markDirty();
    if (loader !== "vanilla") loadLoaderVersions();
  }

  function onMcVersionChange() {
    loaderVersions = [];
    loaderVersion = "";
    markDirty();
    if (loader !== "vanilla") loadLoaderVersions();
  }
</script>

<div class="settings-page">
  <!-- Sticky Header: pins title and save action at the top -->
  <header class="page-header sticky-header">
    <div class="header-inner">
      <div class="ph-text">
        <div class="ph-title-row">
          {#if showBack}
            <button type="button" class="sm-btn ghost" onclick={onBack}>
              <ArrowLeft size={16} /> Back
            </button>
          {/if}
          <h1 class="page-title">Setup & Runtime Settings</h1>
          <span
            class="sync-pill"
            class:unsaved={dirty}
            title={dirty ? "You have unsaved changes" : "All changes saved to project"}
          >
            <span class="sync-dot" class:on={!dirty}></span>
            {dirty ? "Unsaved changes" : "Saved"}
          </span>
        </div>
      </div>
      <div class="header-actions">
        {#if showBack}
          <button type="button" class="sm-btn" onclick={onBack}>Cancel</button>
        {/if}
        <button
          type="button"
          class="primary-btn"
          onclick={save}
          disabled={saving || !$projectPath}
          title="Save project settings (Ctrl+S)"
        >
          <Save size={15} />
          {saving ? "Saving…" : "Save changes"}
        </button>
      </div>
    </div>
  </header>

  {#if !$projectPath}
    <div class="empty-wrap">
      <EmptyState icon={Cpu} title="No project selected" description="Open an instance from the shelf to edit its runtime settings." />
    </div>
  {:else}
    <div class="main-scroll-body">
      <div class="setup-container">
        {#if loading}
          <div class="loading-bar">
            <RefreshCw size={15} class="spin" />
            <span>Fetching versions and instance metadata…</span>
          </div>
        {/if}

        {#if error}
          <div class="inline-error">{error}</div>
        {/if}
        {#if successMessage}
          <div class="inline-success">{successMessage}</div>
        {/if}

        <!-- ── Card 1: Game & Mod Loader ───────────────────────── -->
        <section class="panel glass-card">
          <div class="card-head-row">
            <div class="card-head-title">
              <Container size={18} />
              <h3>Game & Mod Loader</h3>
            </div>
            <span class="card-hint-tag">Core Engine</span>
          </div>
          <div class="form-grid-3">
            <div class="field">
              <label for="mc-version" class="fl-label">Minecraft version</label>
              <select id="mc-version" bind:value={mcVersion} onchange={onMcVersionChange}>
                {#each mcVersions as v}
                  <option value={v.id}>
                    {v.id}{#if v.popular} ★ popular{/if}
                  </option>
                {/each}
              </select>
            </div>

            <div class="field">
              <label for="loader-kind" class="fl-label">Mod Loader</label>
              <select id="loader-kind" bind:value={loader} onchange={onLoaderChange}>
                {#each loaders as l}
                  <option value={l.id}>{l.label}</option>
                {/each}
              </select>
            </div>

            <div class="field">
              <label for="loader-version" class="fl-label">Loader version</label>
              {#if loader === "vanilla"}
                <input id="loader-version" value="None (Vanilla)" disabled class="input-disabled" />
              {:else}
                <select id="loader-version" bind:value={loaderVersion} onchange={markDirty}>
                  {#each loaderVersions as v}
                    <option value={v.id}>{v.id}{#if v.stable} (stable){/if}</option>
                  {/each}
                </select>
              {/if}
            </div>
          </div>
        </section>

        <!-- ── Card 2: Java Virtual Machine ─────────────────────── -->
        <section class="panel glass-card">
          <div class="card-head-row">
            <div class="card-head-title">
              <Coffee size={18} />
              <h3>Java Virtual Machine</h3>
            </div>
            {#if javaVersion}
              <span class="java-badge">{javaVersion}</span>
            {/if}
          </div>
          <div class="field">
            <label for="java-path" class="fl-label">Java executable path</label>
            <div class="flex-input-row">
              <input
                id="java-path"
                bind:value={javaPath}
                readonly
                class="java-input"
                placeholder="Auto-detect (System default or managed JRE)"
              />
              <button
                type="button"
                class="sm-btn secondary-btn"
                onclick={() => (showJavaPicker = true)}
                title="Browse installed Java runtimes"
              >
                <Search size={14} /> Browse…
              </button>
              {#if javaPath !== "Auto-detect"}
                <button
                  type="button"
                  class="sm-btn ghost"
                  onclick={() => {
                    javaPath = "Auto-detect";
                    markDirty();
                    void detectJavaPreview();
                  }}
                  title="Reset to automatic detection"
                >
                  Auto-detect
                </button>
              {/if}
            </div>
          </div>
        </section>

        <!-- ── Card 3: Memory Allocation (RAM) ──────────────────── -->
        <section class="panel glass-card">
          <div class="card-head-row">
            <div class="card-head-title">
              <Cpu size={18} />
              <h3>Memory Allocation (RAM)</h3>
            </div>
            <div class="memory-live-badge">
              <span class="mem-highlight">{formatMemory(memory)}</span>
              <span class="mem-sub">({memory} MB)</span>
            </div>
          </div>
          <!-- Slider + Direct input side-by-side -->
          <div class="memory-control-row">
            <div class="slider-wrap">
              <input
                type="range"
                min={1024}
                max={16384}
                step={256}
                value={memory}
                oninput={(e) => onMemorySliderChange(Number((e.target as HTMLInputElement).value))}
                class="range-slider"
                aria-label="Memory slider"
              />
              <div class="slider-bounds">
                <span>1 GB</span>
                <span>8 GB</span>
                <span>16 GB</span>
              </div>
            </div>

            <div class="memory-manual-box">
              <label for="mem-manual" class="manual-label">Exact MB:</label>
              <div class="manual-input-wrap">
                <input
                  id="mem-manual"
                  type="number"
                  min={512}
                  max={65536}
                  step={128}
                  value={memoryManualMb}
                  oninput={(e) => onMemoryManualInput(Number((e.target as HTMLInputElement).value))}
                  class="number-input"
                />
                <span class="unit">MB</span>
              </div>
            </div>
          </div>

          <!-- Quick Presets -->
          <div class="memory-presets-row">
            <span class="presets-label">Quick presets:</span>
            <div class="preset-chips">
              {#each MEMORY_PRESETS as p}
                <button
                  type="button"
                  class="preset-chip"
                  class:active={memory === p.mb}
                  onclick={() => onMemorySliderChange(p.mb)}
                >
                  {p.label}
                </button>
              {/each}
            </div>
          </div>

          <div class="recommendation-box">
            <Sparkles size={14} class="rec-icon" />
            <span>
              <strong>Recommended:</strong> 4–8 GB for most packs.
            </span>
          </div>
          {#if heapRec}
            <div class="recommendation-box mt-2">
              <Sparkles size={14} class="rec-icon" />
              <span>
                <strong>Auto:</strong> {formatMemory(heapRec.memoryMb)} for {heapRec.modCount} mods.
              </span>
              <button
                type="button"
                class="preset-chip"
                onclick={() => onMemorySliderChange(heapRec!.memoryMb)}
              >
                Apply
              </button>
            </div>
          {/if}
        </section>

        <!-- ── Card 4: JVM Launch Arguments ─────────────────────── -->
        <section class="panel glass-card">
          <div class="card-head-row">
            <div class="card-head-title">
              <Terminal size={18} />
              <h3>JVM Launch Arguments</h3>
            </div>
            <div class="jvm-actions">
              <button type="button" class="sm-btn ghost" onclick={copyJvmArgs} title="Copy arguments">
                <Copy size={13} /> Copy
              </button>
              <button type="button" class="sm-btn ghost" onclick={resetJvmArgs} title="Reset to default">
                <RotateCcw size={13} /> Reset
              </button>
            </div>
          </div>
          <div class="field">
            <textarea
              bind:value={jvmArgs}
              oninput={markDirty}
              rows={4}
              wrap="off"
              placeholder="Empty = auto-tuned for this pack"
              spellcheck="false"
              class="jvm-textarea"
            ></textarea>
          </div>

          <!-- Optimization Presets -->
          <div class="jvm-presets-section">
            <span class="presets-label">Optimization Presets:</span>
            <div class="jvm-presets-grid">
              {#each JVM_PRESETS as preset}
                <button
                  type="button"
                  class="jvm-preset-card"
                  class:active={jvmArgs.trim() === preset.flags.trim()}
                  onclick={() => applyJvmPreset(preset.flags)}
                  title={preset.flags}
                >
                  <div class="preset-title-row">
                    <strong>{preset.label}</strong>
                    {#if jvmArgs.trim() === preset.flags.trim()}
                      <Check size={13} class="check-icon" />
                    {/if}
                  </div>
                </button>
              {/each}
            </div>
          </div>
          {#if jvmRec}
            <button
              type="button"
              class="jvm-preset-card"
              class:active={jvmArgs.trim() === jvmRec.args.join(" ").trim()}
              onclick={() => {
                jvmArgs = jvmRec!.args.join(" ");
                markDirty();
                successMessage = `Auto JVM profile applied (${jvmRec!.profile}).`;
              }}
              title={jvmRec.args.join(" ")}
            >
              <div class="preset-title-row">
                <strong>Auto: {jvmRec.profile}</strong>
                {#if jvmArgs.trim() === jvmRec.args.join(" ").trim()}
                  <Check size={13} class="check-icon" />
                {/if}
              </div>
            </button>
          {/if}
        </section>

        <!-- ── Card 5: Player & Shared Options ─────────────────── -->
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <!-- Player block -->
          <section class="panel glass-card">
            <div class="card-head-row">
              <div class="card-head-title">
                <User size={18} />
                <h3>Player Identity</h3>
              </div>
            </div>
            <div class="field">
              <label for="player-name" class="fl-label">Offline test player name</label>
              <input
                id="player-name"
                bind:value={playerName}
                oninput={markDirty}
                placeholder="Player"
                maxlength={16}
              />
            </div>
          </section>

          <!-- Shared options.txt block -->
          <section class="panel glass-card">
            <div class="card-head-row">
              <div class="card-head-title">
                <FileCog size={18} />
                <h3>Shared options.txt</h3>
              </div>
              <span class="options-badge" class:shared={optionsManaged}>
                {#if optionsManaged}
                  <Share2 size={12} /> Shared
                {:else}
                  <Shield size={12} /> Independent
                {/if}
              </span>
            </div>
            <div class="options-actions-block">
              <div class="options-status-text">
                {#if optionsManaged}
                  <span>Shared group: <code>{optionsGroup || mcVersion}</code></span>
                {:else}
                  <span>Independent <code>options.txt</code></span>
                {/if}
              </div>

              <div class="flex flex-wrap gap-2.5">
                {#if optionsManaged}
                  <button
                    type="button"
                    class="sm-btn secondary-btn"
                    onclick={disableOptionsSync}
                    disabled={optionsBusy}
                  >
                    {optionsBusy ? "Working…" : "Unlink (make independent)"}
                  </button>
                  <button
                    type="button"
                    class="sm-btn secondary-btn"
                    onclick={pushOptionsToGroup}
                    disabled={optionsBusy}
                    title="Overwrite group template with current instance settings"
                  >
                    Push current options to group
                  </button>
                {:else}
                  <button
                    type="button"
                    class="sm-btn primary-action-btn"
                    onclick={enableOptionsSync}
                    disabled={optionsBusy}
                  >
                    <Share2 size={13} />
                    {optionsBusy ? "Working…" : "Share options across instances"}
                  </button>
                {/if}
              </div>
            </div>
          </section>

          <!-- File deduplication block -->
          <section class="panel glass-card">
            <div class="card-head-row">
              <div class="card-head-title">
                <Database size={18} />
                <h3>File deduplication</h3>
              </div>
              <span class="options-badge" class:shared={dedupEnabled}>
                {#if dedupEnabled}
                  <Share2 size={12} /> Shared store
                {:else}
                  <Shield size={12} /> Independent files
                {/if}
              </span>
            </div>
            <div class="options-actions-block">
              <div class="options-status-text">
                {#if dedupEnabled}
                  <span>Shared file store enabled.</span>
                {:else}
                  <span>Independent file copies.</span>
                {/if}
              </div>

              <div class="flex flex-wrap gap-2.5">
                {#if dedupEnabled}
                  <button
                    type="button"
                    class="sm-btn secondary-btn"
                    onclick={() => setDedupEnabled(false)}
                    disabled={dedupBusy}
                    title="Replace shared files with this pack's own copies"
                  >
                    {dedupBusy ? "Working…" : "Turn off (make files independent)"}
                  </button>
                {:else}
                  <button
                    type="button"
                    class="sm-btn primary-action-btn"
                    onclick={() => setDedupEnabled(true)}
                    disabled={dedupBusy}
                    title="Link identical files into the shared store to save disk space"
                  >
                    <Share2 size={13} />
                    {dedupBusy ? "Working…" : "Turn on (share identical files)"}
                  </button>
                {/if}
              </div>
            </div>
          </section>
        </div>

        <!-- ── Card 6: Advanced & Project Schema (Collapsible) ─── -->
        <details class="panel glass-card advanced-panel">
          <summary class="advanced-summary">
            <div class="card-head-title">
              <Database size={16} />
              <span>Project schema & metadata</span>
            </div>
          </summary>

          <div class="advanced-body">
            <div class="schema-row">
              <span>Detected schema version:</span>
              <code>{schemaVersion || "0.1.0"}</code>
            </div>

            {#if schemaNeedsMigration}
              <div class="schema-warning">
                <AlertTriangle size={15} />
                <span>Schema migration available. This will normalize your project manifest to the current format.</span>
              </div>
              <button
                type="button"
                class="sm-btn secondary-btn self-start"
                onclick={migrateSchema}
                disabled={saving}
              >
                <RefreshCw size={14} />
                {saving ? "Migrating…" : "Migrate schema"}
              </button>
            {:else}
              <div class="schema-ok">
                <Check size={14} />
                <span>Project manifest schema is up to date.</span>
              </div>
            {/if}
          </div>
        </details>

        <!-- Bottom Action Bar -->
        <div class="bottom-actions-row">
          {#if showBack}
            <button type="button" class="sm-btn ghost" onclick={onBack}>Cancel</button>
          {/if}
          <button
            type="button"
            class="primary-btn"
            onclick={save}
            disabled={saving || !$projectPath}
          >
            <Save size={16} />
            {saving ? "Saving…" : "Save changes"}
          </button>
        </div>
      </div>
    </div>
  {/if}
</div>

{#if showJavaPicker}
  <JavaPickerModal
    current={javaPath === "Auto-detect" ? "" : javaPath}
    onclose={() => (showJavaPicker = false)}
    onselected={onJavaSelected}
  />
{/if}

<style>
  .settings-page {
    --setup-page-x: 20px;
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    padding: 0 var(--setup-page-x) 20px;
    box-sizing: border-box;
    width: 100%;
    background: rgba(0, 0, 0, 0.08);
  }

  .main-scroll-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-gutter: stable;
    padding-top: 10px;
    padding-right: 4px;
    padding-bottom: 28px;
    box-sizing: border-box;
  }

  /* Centered, balanced container: eliminates awkward bottom dead space */
  .setup-container {
    max-width: 1200px;
    margin: 0 auto;
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  /* Sticky top header — fixed: was width:calc(100%+2*pad) + negative margin
     causing 1px horizontal shift when vertical scrollbar appears (scrollbar-gutter
     on body is stable but header is outside scroll). Now width:auto with
     negative margin only, no calc, so no shift and no horizontal scroll. */
  .sticky-header {
    position: sticky;
    top: 0;
    z-index: 20;
    width: auto;
    box-sizing: border-box;
    flex-shrink: 0;
    padding: 14px var(--setup-page-x);
    margin: 0 calc(-1 * var(--setup-page-x)) 4px;
    background: color-mix(in srgb, var(--bg-primary, #ffffff) 88%, transparent);
    -webkit-backdrop-filter: blur(16px);
    backdrop-filter: blur(16px);
    border-bottom: 1px solid color-mix(in srgb, var(--border-color) 70%, transparent);
  }

  .header-inner {
    width: 100%;
    max-width: none;
    margin: 0;
    padding: 0 4px;
    box-sizing: border-box;
    display: flex;
    justify-content: space-between;
    gap: 20px;
    align-items: center;
    flex-wrap: wrap;
  }

  .ph-text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .ph-title-row {
    display: flex;
    align-items: center;
    gap: 14px;
    flex-wrap: wrap;
  }

  .page-title {
    font-size: 21px;
    font-weight: 800;
    color: var(--text-primary);
    line-height: 1.2;
    margin: 0;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-left: auto;
    flex-shrink: 0;
  }

  /* Glass card shell: high contrast in both dark & light themes */
  .glass-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
    box-shadow: var(--shadow-sm);
    padding: 22px 24px;
  }

  .card-head-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 16px;
    flex-wrap: wrap;
  }

  .card-head-title {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-primary);
  }

  .card-head-title h3 {
    margin: 0;
    font-size: 17px;
    font-weight: 700;
    color: var(--text-primary);
  }

  .card-head-title :global(svg) {
    color: var(--accent-primary);
  }

  .card-hint-tag {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-muted);
    padding: 3px 10px;
    border-radius: 999px;
    background: var(--bg-tertiary);
  }

  /* Form Grids — increased gaps for breathability, fixes “малые отступы” */
  .form-grid-3 {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 16px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .fl-label {
    font-size: 14px;
    font-weight: 700;
    color: var(--text-primary);
  }

  input,
  select,
  textarea {
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-elevated);
    color: var(--text-primary);
    padding: 11px 13px;
    font-family: inherit;
    font-size: 14.5px;
    transition: border-color var(--motion-fast) ease, box-shadow var(--motion-fast) ease;
  }

  input:focus,
  select:focus,
  textarea:focus {
    outline: none;
    border-color: var(--accent-primary);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent-primary) 15%, transparent);
  }

  .input-disabled {
    opacity: 0.65;
    cursor: not-allowed;
    background: var(--bg-tertiary);
  }

  .flex-input-row {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
  }

  .java-input {
    flex: 1;
    min-width: 0;
  }

  .java-badge {
    font-size: 14px;
    font-weight: 700;
    padding: 4px 10px;
    border-radius: var(--border-radius-sm);
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    color: var(--accent-primary);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 28%, transparent);
  }

  /* Memory allocation */
  .memory-live-badge {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .mem-highlight {
    font-size: 21px;
    font-weight: 800;
    color: var(--accent-primary);
    font-variant-numeric: tabular-nums;
  }
  .mem-sub {
    font-size: 14px;
    color: var(--text-secondary);
    font-weight: 600;
  }

  .memory-control-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 180px;
    gap: 24px;
    align-items: center;
    margin-bottom: 16px;
  }

  .slider-wrap {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .range-slider {
    width: 100%;
    height: 8px;
    border-radius: 4px;
    accent-color: var(--accent-primary);
    cursor: pointer;
    background: var(--bg-tertiary);
  }

  .slider-bounds {
    display: flex;
    justify-content: space-between;
    font-size: 14px;
    font-weight: 700;
    color: var(--text-secondary);
  }

  .memory-manual-box {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .manual-label {
    font-size: 14px;
    font-weight: 700;
    color: var(--text-primary);
  }
  .manual-input-wrap {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .number-input {
    width: 110px;
    text-align: right;
    font-weight: 700;
  }
  .unit {
    font-size: 14px;
    font-weight: 700;
    color: var(--text-secondary);
  }

  .memory-presets-row {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
    flex-wrap: wrap;
  }
  .presets-label {
    font-size: 14px;
    font-weight: 700;
    color: var(--text-primary);
  }
  .preset-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .preset-chip {
    padding: 8px 14px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
    color: var(--text-primary);
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: background var(--motion-fast) ease, border-color var(--motion-fast) ease;
  }
  .preset-chip:hover {
    background: var(--bg-hover);
    border-color: var(--accent-primary);
  }
  .preset-chip.active {
    background: var(--accent-primary);
    color: var(--on-accent, #fff);
    border-color: var(--accent-primary);
  }

  .recommendation-box {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 16px;
    border-radius: var(--border-radius-md);
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 22%, transparent);
    color: var(--text-primary);
    font-size: 14px;
    line-height: 1.5;
  }

  /* JVM Arguments */
  .jvm-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .jvm-textarea {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 14px;
    line-height: 1.6;
    white-space: pre;
    overflow-x: auto;
    min-height: 84px;
  }

  .jvm-presets-section {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 8px;
  }
  .jvm-presets-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 12px;
  }
  .jvm-preset-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    text-align: left;
    padding: 11px 14px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
    color: var(--text-primary);
    cursor: pointer;
    transition: border-color var(--motion-fast) ease, background var(--motion-fast) ease;
  }
  .jvm-preset-card:hover {
    border-color: var(--accent-primary);
    background: var(--bg-hover);
  }
  .jvm-preset-card.active {
    border-color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 10%, transparent);
  }
  .preset-title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 14px;
    font-weight: 700;
  }
  /* Shared options */
  .options-badge {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 700;
    padding: 4px 10px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }
  .options-badge.shared {
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    color: var(--accent-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 32%, transparent);
  }
  .options-actions-block {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .options-status-text {
    font-size: 14px;
    color: var(--text-secondary);
    line-height: 1.45;
  }
  .options-status-text code {
    font-weight: 700;
    color: var(--text-primary);
    background: var(--bg-elevated);
    padding: 2px 6px;
    border-radius: 4px;
  }

  /* Advanced / Project schema */
  .advanced-panel {
    cursor: default;
    user-select: none;
  }
  .advanced-summary {
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: space-between;
    list-style: none;
    font-size: 14.5px;
    font-weight: 700;
    color: var(--text-primary);
  }
  .advanced-summary::-webkit-details-marker {
    display: none;
  }  .advanced-body {
    margin-top: 14px;
    padding-top: 14px;
    border-top: 1px solid var(--border-color);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .schema-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 14px;
    color: var(--text-primary);
  }
  .schema-row code {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 14px;
    font-weight: 700;
    color: var(--accent-primary);
  }
  .schema-warning {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 14px;
    border-radius: var(--border-radius-md);
    background: color-mix(in srgb, var(--accent-warning) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent-warning) 30%, transparent);
    color: var(--accent-warning);
    font-size: 14px;
  }
  .schema-ok {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--accent-primary);
    font-size: 14px;
    font-weight: 600;
  }

  /* Buttons */
  .primary-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 40px;
    padding: 0 20px;
    border: none;
    border-radius: var(--border-radius-md);
    background: linear-gradient(180deg, #10b981, #059669);
    color: #fff;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    box-shadow: 0 0 14px rgba(16, 185, 129, 0.35), inset 0 1px 0 rgba(255, 255, 255, 0.22);
    transition: filter var(--motion-fast) ease, transform var(--motion-fast) ease;
  }
  .primary-btn:hover:not(:disabled) {
    filter: brightness(1.08);
    transform: translateY(-1px);
  }
  .primary-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .primary-action-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 38px;
    padding: 0 16px;
    border-radius: var(--border-radius-sm);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 40%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 15%, transparent);
    color: var(--accent-primary);
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: background var(--motion-fast) ease;
  }
  .primary-action-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-primary) 25%, transparent);
  }

  .secondary-btn {
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
    color: var(--text-primary);
  }
  .secondary-btn:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .sm-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 14px;
    border-radius: var(--border-radius-sm);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: background var(--motion-fast) ease, color var(--motion-fast) ease;
  }
  .sm-btn.ghost {
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-secondary);
  }
  .sm-btn.ghost:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .sync-pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 5px 12px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 600;
  }
  .sync-pill.unsaved {
    color: #f59e0b;
    border-color: rgba(245, 158, 11, 0.4);
    background: rgba(245, 158, 11, 0.1);
  }
  .sync-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #f59e0b;
  }
  .sync-dot.on {
    background: #10b981;
    box-shadow: 0 0 6px rgba(16, 185, 129, 0.6);
  }

  .bottom-actions-row {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 16px;
    padding: 16px 0 4px;
    margin-top: 8px;
    border-top: 1px solid color-mix(in srgb, var(--border-color) 60%, transparent);
    position: sticky;
    bottom: 0;
    background: color-mix(in srgb, var(--bg-secondary) 92%, transparent);
    backdrop-filter: blur(8px);
    z-index: 5;
  }

  .loading-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 16px;
    border-radius: var(--border-radius-md);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    font-size: 14px;
    margin-bottom: 14px;
  }

  .inline-error,
  .inline-success {
    padding: 12px 16px;
    border-radius: var(--border-radius-md);
    font-size: 14px;
    font-weight: 600;
    margin-bottom: 14px;
  }
  .inline-error {
    color: var(--accent-danger);
    background: color-mix(in srgb, var(--accent-danger) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent-danger) 30%, transparent);
  }
  .inline-success {
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent-primary) 30%, transparent);
  }

  .empty-wrap {
    width: 100%;
    max-width: 1200px;
    margin: 0 auto;
    box-sizing: border-box;
    padding: 40px 20px;
  }

  @media (max-width: 900px) {
    .form-grid-3 {
      grid-template-columns: 1fr;
    }
    .memory-control-row {
      grid-template-columns: 1fr;
    }
  }
</style>
