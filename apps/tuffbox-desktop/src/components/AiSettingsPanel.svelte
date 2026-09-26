<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import {
    AlertTriangle,
    CheckCircle2,
    Download,
    FileUp,
    FolderOpen,
    KeyRound,
    Pause,
    Play,
    Plug,
    RefreshCw,
    Search,
    Trash2,
  } from "@lucide/svelte";
  import {
    fitLabel,
    formatBytes,
    type AiProvider,
    type OllamaDetect,
    type OllamaModelInfo,
    type OllamaPullProgress,
    type OllamaStorage,
    type SuggestedModel,
  } from "../lib/api";
  import { toasts } from "../lib/toast";

  let {
    onsaved,
  }: {
    onsaved?: () => void;
  } = $props();

  type IntegrationStatus = {
    settings: {
      githubRepository: string;
      ai: {
        provider: string;
        endpoint: string;
        model: string;
        diagnoseMode?: string;
        crashKbEndpoint?: string;
        ollamaBinaryPath?: string;
        ollamaModelsPath?: string;
        speculativeDecoding?: boolean;
        draftModel?: string;
        tuneWebResearch?: boolean;
      };
      swarm?: Record<string, unknown>;
    };
    aiApiKeySet: boolean;
    crashKbTokenSet?: boolean;
  };

  type PresetId = "gemini" | "openai" | "openrouter" | "hermes" | "custom";
  type Surface = "local" | "cloud";

  const CLOUD_PRESETS: {
    id: PresetId;
    label: string;
    endpoint: string;
    model: string;
    needsKey: boolean;
  }[] = [
    {
      id: "gemini",
      label: "Gemini",
      endpoint: "https://generativelanguage.googleapis.com/v1beta/openai",
      model: "gemini-flash-latest",
      needsKey: true,
    },
    {
      id: "openai",
      label: "OpenAI",
      endpoint: "https://api.openai.com/v1",
      model: "gpt-4o-mini",
      needsKey: true,
    },
    {
      id: "openrouter",
      label: "OpenRouter",
      endpoint: "https://openrouter.ai/api/v1",
      model: "deepseek/deepseek-chat",
      needsKey: true,
    },
    {
      id: "hermes",
      label: "Local API",
      endpoint: "http://127.0.0.1:8000/v1",
      model: "hermes",
      needsKey: false,
    },
    {
      id: "custom",
      label: "Custom",
      endpoint: "",
      model: "",
      needsKey: false,
    },
  ];

  let surface = $state<Surface>("local");
  let preset = $state<PresetId>("openrouter");
  let provider = $state<AiProvider>("ollama");
  let endpoint = $state("http://127.0.0.1:11434");
  let model = $state("");
  let ollamaBinaryPath = $state("");
  let ollamaModelsPath = $state("");
  let diagnoseMode = $state<"server" | "local" | "kb_only">("server");
  let speculativeDecoding = $state(false);
  let tuneWebResearch = $state(true);
  let draftModel = $state("qwen2.5-coder:0.5b");
  let crashKbEndpoint = $state("");
  let crashKbTokenDraft = $state("");
  let crashKbTokenSet = $state(false);
  let apiKeyDraft = $state("");
  let apiKeySet = $state(false);
  let loading = $state(false);
  let saving = $state(false);
  let testing = $state(false);
  let detecting = $state(false);
  let scanningDisk = $state(false);
  let pulling = $state(false);
  let pullPaused = $state(false);
  let pausedModel = $state("");
  let importing = $state(false);
  let deleting = $state("");
  let advancedOpen = $state(false);
  let error = $state("");
  let message = $state("");
  let testResult = $state("");
  let models = $state.raw<OllamaModelInfo[]>([]);
  let suggestions = $state.raw<SuggestedModel[]>([]);
  let detect = $state.raw<OllamaDetect | null>(null);
  let storage = $state.raw<OllamaStorage | null>(null);
  let pullName = $state("qwen2.5:7b");
  let ggufName = $state("");
  let pullProgress = $state.raw<OllamaPullProgress | null>(null);
  let hostRamBytes = $state(0);

  let unlistenPull: UnlistenFn | null = null;
  let unlistenPullDone: UnlistenFn | null = null;

  const pullPct = $derived.by(() => {
    const p = pullProgress;
    if (!p || !p.total) return null;
    return Math.min(100, Math.round((p.completed / p.total) * 100));
  });

  const canResumePull = $derived(
    pullPaused && !!pausedModel && pullName.trim() === pausedModel,
  );

  const statusText = $derived.by(() => {
    if (surface === "cloud") {
      const label = CLOUD_PRESETS.find((p) => p.id === preset)?.label ?? "API";
      return `${label} · ${model || "—"} · ${apiKeySet ? "key set" : "no key"}`;
    }
    if (!detect) return "Detecting Ollama…";
    if (detect.running) return `Ollama running · ${models.length} model${models.length === 1 ? "" : "s"} · ${model || "—"}`;
    if (detect.installed) return "Ollama installed · API not responding";
    return "Ollama not found";
  });

  const statusKind = $derived.by(() => {
    if (surface === "cloud") return apiKeySet || endpoint.includes("127.0.0.1") ? "ok" : "warn";
    if (detect?.running) return "ok";
    if (detect?.installed) return "warn";
    return "bad";
  });

  onMount(() => {
    void load();
    void ensurePullListener().then(() => restorePullStatus());
  });

  onDestroy(() => {
    void unlistenPull?.();
    void unlistenPullDone?.();
  });

  async function restorePullStatus() {
    try {
      const snap = await invoke<{
        phase: string;
        model: string;
        completed: number;
        total: number;
        error?: string | null;
      }>("get_ollama_pull_status");
      if (!snap?.phase || snap.phase === "idle" || snap.phase === "succeeded") return;
      if (snap.model) {
        pullName = snap.model;
        if (snap.phase === "paused") pausedModel = snap.model;
      }
      if (snap.total > 0 || snap.completed > 0 || snap.phase === "running" || snap.phase === "paused") {
        pullProgress = {
          model: snap.model || pullName,
          status: snap.phase === "paused" ? "paused" : "downloading",
          completed: snap.completed || 0,
          total: snap.total || 0,
        };
      }
      pulling = snap.phase === "running";
      pullPaused = snap.phase === "paused";
      if (snap.phase === "running") {
        message = `Downloading ${snap.model} in background…`;
      } else if (snap.phase === "paused") {
        message = `Paused ${snap.model} — Resume continues from the same place`;
      } else if (snap.phase === "failed" && snap.error) {
        error = snap.error;
      }
    } catch {
      /* ignore */
    }
  }

  async function ensurePullListener() {
    if (!unlistenPull) {
      unlistenPull = await listen<OllamaPullProgress>("ollama-pull-progress", (ev) => {
        pullProgress = ev.payload;
        if (ev.payload.status === "paused") {
          pulling = false;
          pullPaused = true;
          pausedModel = ev.payload.model || pausedModel;
        } else if (ev.payload.model) {
          pulling = true;
          pullPaused = false;
        }
      });
    }
    if (!unlistenPullDone) {
      unlistenPullDone = await listen<{
        ok: boolean;
        paused?: boolean;
        model: string;
        error?: string;
        result?: {
          model: string;
          models?: OllamaModelInfo[];
          modelsPath?: string;
        };
        completed?: number;
        total?: number;
      }>("ollama-pull-finished", (ev) => {
        const p = ev.payload;
        if (p.paused) {
          pulling = false;
          pullPaused = true;
          pausedModel = p.model;
          if (pullProgress) {
            pullProgress = {
              ...pullProgress,
              status: "paused",
              completed: p.completed ?? pullProgress.completed,
              total: p.total ?? pullProgress.total,
            };
          }
          message = `Paused ${p.model} — Resume continues from the same place`;
          return;
        }
        pulling = false;
        if (p.ok && p.result) {
          pullPaused = false;
          pausedModel = "";
          pullProgress = null;
          model = p.result.model;
          pullName = p.result.model;
          if (p.result.models) models = p.result.models;
          message = p.result.modelsPath
            ? `Installed ${p.result.model} → ${p.result.modelsPath}`
            : `Installed ${p.result.model}`;
          void probeOllama();
          void refreshStorage();
          onsaved?.();
        } else {
          pullPaused = false;
          pausedModel = "";
          pullProgress = null;
          error = p.error || "Model download failed";
          message = "";
        }
      });
    }
  }

  async function load() {
    loading = true;
    error = "";
    message = "";
    testResult = "";
    try {
      const status = await invoke<IntegrationStatus>("get_integration_status");
      const ai = status.settings?.ai;
      provider = ai?.provider === "openai-compatible" ? "openai-compatible" : "ollama";
      endpoint =
        ai?.endpoint || (provider === "ollama" ? "http://127.0.0.1:11434" : "");
      model = ai?.model || "";
      ollamaBinaryPath = ai?.ollamaBinaryPath ?? "";
      ollamaModelsPath = ai?.ollamaModelsPath ?? "";
      const dm = ai?.diagnoseMode ?? "server";
      diagnoseMode = dm === "local" || dm === "kb_only" ? dm : "server";
      speculativeDecoding = !!ai?.speculativeDecoding;
      draftModel = ai?.draftModel?.trim() || "qwen2.5-coder:0.5b";
      tuneWebResearch = ai?.tuneWebResearch !== false;
      crashKbEndpoint = ai?.crashKbEndpoint ?? "";
      crashKbTokenSet = !!status.crashKbTokenSet;
      crashKbTokenDraft = "";
      apiKeySet = !!status.aiApiKeySet;
      apiKeyDraft = "";
      surface = provider === "ollama" ? "local" : "cloud";
      preset = detectCloudPreset(endpoint);
      if (!model.trim() && provider === "ollama") model = "qwen2.5:7b";
      if (provider === "ollama") {
        await probeOllama();
        await refreshStorage();
      } else {
        detect = null;
        models = [];
        storage = null;
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function detectCloudPreset(ep: string): PresetId {
    const e = ep.trim().replace(/\/$/, "");
    if (e.includes("generativelanguage.googleapis.com") || e.includes("/v1beta/openai")) return "gemini";
    if (e.includes("api.openai.com")) return "openai";
    if (e.includes("openrouter.ai")) return "openrouter";
    if (e.includes("127.0.0.1") || e.includes("localhost")) return "hermes";
    return e ? "custom" : "custom";
  }

  function setSurface(next: Surface) {
    surface = next;
    error = "";
    message = "";
    if (next === "local") {
      provider = "ollama";
      if (!endpoint.trim() || !endpoint.includes("11434")) {
        endpoint = "http://127.0.0.1:11434";
      }
      void probeOllama();
      void refreshStorage();
    } else {
      provider = "openai-compatible";
      detect = null;
      applyCloudPreset(preset);
    }
  }

  function applyCloudPreset(id: PresetId) {
    preset = id;
    provider = "openai-compatible";
    const p = CLOUD_PRESETS.find((x) => x.id === id);
    if (!p) return;
    if (p.endpoint) endpoint = p.endpoint;
    if (p.model) model = p.model;
  }

  async function probeOllama() {
    detecting = true;
    try {
      const result = await invoke<OllamaDetect>("detect_ollama", {
        endpoint: endpoint || null,
        binaryPath: ollamaBinaryPath || null,
      });
      detect = result;
      models = result.models ?? [];
      suggestions = result.suggestedModels ?? [];
      hostRamBytes = result.hostRamBytes ?? 0;
      if (result.binaryPath && !ollamaBinaryPath.trim()) {
        ollamaBinaryPath = result.binaryPath;
      }
      if (result.suggestedModels?.length && !pullName.trim()) {
        pullName = result.suggestedModels[0].name;
      }
      if (result.defaultModel && !model.trim()) {
        model = result.defaultModel;
        pullName = result.defaultModel;
      }
      if (!model.trim() && models.length > 0) {
        model = models[0].name;
      }
      if (result.installed && result.needsModel) {
        message = "Install a model below to get started.";
      }
    } catch (e) {
      detect = null;
      models = [];
      error = String(e);
    } finally {
      detecting = false;
    }
  }

  async function refreshStorage() {
    try {
      storage = await invoke<OllamaStorage>("get_ollama_storage", {
        modelsPath: ollamaModelsPath.trim() || null,
      });
      if (storage?.hostRamBytes) hostRamBytes = storage.hostRamBytes;
    } catch {
      storage = null;
    }
  }

  async function persistAiSettings(activeModel: string) {
    const status = await invoke<IntegrationStatus>("get_integration_status");
    await invoke("save_integration_settings", {
      settings: {
        githubRepository: status.settings?.githubRepository ?? "",
        swarm: status.settings?.swarm,
        ai: {
          provider,
          endpoint: endpoint.trim(),
          model: activeModel.trim() || "qwen2.5:7b",
          diagnoseMode,
          crashKbEndpoint: crashKbEndpoint.trim(),
          ollamaBinaryPath: ollamaBinaryPath.trim(),
          ollamaModelsPath: ollamaModelsPath.trim(),
          speculativeDecoding,
          draftModel: draftModel.trim() || "qwen2.5-coder:0.5b",
          tuneWebResearch,
        },
      },
    });
  }

  async function save() {
    saving = true;
    error = "";
    message = "";
    try {
      await persistAiSettings(model.trim());
      if (apiKeyDraft.trim()) {
        await invoke("set_integration_secret", { kind: "ai", value: apiKeyDraft.trim() });
        apiKeyDraft = "";
        apiKeySet = true;
      }
      if (crashKbTokenDraft.trim()) {
        await invoke("set_integration_secret", {
          kind: "crash_kb",
          value: crashKbTokenDraft.trim(),
        });
        crashKbTokenDraft = "";
        crashKbTokenSet = true;
      }
      message = "Saved.";
      await load();
      onsaved?.();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  async function test() {
    testing = true;
    error = "";
    testResult = "";
    try {
      await save();
      const result = await invoke<string>("test_integration", { provider: "ai" });
      testResult = result;
      message = result;
    } catch (e) {
      error = String(e);
    } finally {
      testing = false;
    }
  }

  async function clearKey() {
    try {
      await invoke("clear_integration_secret", { kind: "ai" });
      apiKeySet = false;
      message = "API key cleared.";
    } catch (e) {
      error = String(e);
    }
  }

  async function clearCrashKbToken() {
    try {
      await invoke("clear_integration_secret", { kind: "crash_kb" });
      crashKbTokenSet = false;
      message = "Crash KB token cleared.";
    } catch (e) {
      error = String(e);
    }
  }

  async function selectModel(name: string) {
    model = name;
    try {
      await persistAiSettings(name);
      message = `Active model: ${name}`;
      onsaved?.();
    } catch (e) {
      error = String(e);
    }
  }

  async function deleteModel(name: string) {
    if (!confirm(`Delete model “${name}”?`)) return;
    deleting = name;
    error = "";
    try {
      const result = await invoke<{
        ok: boolean;
        models: OllamaModelInfo[];
        activeModel?: string;
      }>("delete_ollama_model", {
        model: name,
        endpoint: endpoint || null,
        binaryPath: ollamaBinaryPath || null,
      });
      models = result.models ?? [];
      if (result.activeModel) model = result.activeModel;
      message = `Deleted ${name}`;
      await refreshStorage();
      onsaved?.();
    } catch (e) {
      error = String(e);
    } finally {
      deleting = "";
    }
  }

  async function pauseModelPull() {
    try {
      await invoke("pause_ollama_model_pull");
      message = "Pausing download…";
    } catch (e) {
      error = String(e);
    }
  }

  async function installModel(tag?: string) {
    const name = (tag || pullName || model).trim();
    if (!name) {
      error = "Enter a model tag (e.g. qwen2.5:7b).";
      return;
    }
    if (pulling) {
      error = "A model download is already running in the background.";
      return;
    }
    const resuming = pullPaused && name === pausedModel;
    pulling = true;
    pullPaused = false;
    if (!resuming) {
      pullProgress = null;
      pausedModel = "";
    }
    error = "";
    message = resuming
      ? `Resuming ${name} in background…`
      : `Downloading ${name} in background…`;
    try {
      await ensurePullListener();
      await persistAiSettings(name);
      const result = await invoke<{
        started?: boolean;
        ok: boolean;
        model: string;
        taskId?: string;
      }>("pull_ollama_model", {
        model: name,
        endpoint: endpoint || null,
        binaryPath: ollamaBinaryPath || null,
        modelsPath: ollamaModelsPath.trim() || null,
      });
      // Background job: keep `pulling` true until ollama-pull-finished.
      if (result.started) {
        toasts.info(
          resuming
            ? `Resumed ${result.model} in background`
            : `Downloading ${result.model} in background — keep using the launcher`,
          5000,
        );
        return;
      }
      // Legacy sync response (should not happen).
      pulling = false;
      model = result.model;
      pullName = result.model;
      message = `Installed ${result.model}`;
      await probeOllama();
      await refreshStorage();
      onsaved?.();
    } catch (e) {
      pulling = false;
      pullPaused = false;
      pausedModel = "";
      pullProgress = null;
      error = String(e);
      message = "";
    }
  }

  async function pickGgufAndImport() {
    try {
      const selected = await openDialog({
        multiple: false,
        directory: false,
        title: "Select local model file (.gguf)",
        filters: [
          { name: "GGUF model", extensions: ["gguf"] },
          { name: "All files", extensions: ["*"] },
        ],
      });
      if (typeof selected !== "string" || !selected) return;

      importing = true;
      error = "";
      const stem =
        ggufName.trim() ||
        selected
          .replace(/^.*[\\/]/, "")
          .replace(/\.gguf$/i, "")
          .toLowerCase()
          .replace(/[^a-z0-9._-]+/g, "-") ||
        "local-model";
      message = `Importing as “${stem}”…`;
      await persistAiSettings(stem);
      const result = await invoke<{ ok: boolean; model: string; models: OllamaModelInfo[] }>(
        "import_ollama_gguf",
        {
          filePath: selected,
          modelName: stem,
          binaryPath: ollamaBinaryPath || null,
        },
      );
      model = result.model;
      ggufName = result.model;
      models = result.models ?? [];
      message = `Imported ${result.model}`;
      await probeOllama();
      await refreshStorage();
      onsaved?.();
    } catch (e) {
      error = String(e);
      message = "";
    } finally {
      importing = false;
    }
  }

  async function scanDiskForOllama() {
    scanningDisk = true;
    error = "";
    message = "Scanning C:\\…";
    try {
      const scan = await invoke<{
        bestBinary?: string | null;
        bestModelsDir?: string | null;
        models?: string[];
        visited?: number;
        truncated?: boolean;
      }>("scan_ollama_disk", { root: "C:\\" });

      if (scan.bestBinary) ollamaBinaryPath = scan.bestBinary;
      if (scan.bestModelsDir) ollamaModelsPath = scan.bestModelsDir;
      const diskModels = scan.models ?? [];
      if (diskModels.length > 0 && (!model.trim() || !diskModels.includes(model))) {
        const prefer =
          diskModels.find((m) => /qwen2\.5:7b/i.test(m)) ||
          diskModels.find((m) => /:8b\b/i.test(m)) ||
          diskModels[0];
        model = prefer;
        pullName = prefer;
      }
      message =
        diskModels.length || scan.bestBinary
          ? `Found · ${diskModels.length} tag(s)${scan.truncated ? " (scan capped)" : ""}`
          : "Nothing found on C:\\";
      await persistAiSettings(model || pullName || "qwen2.5:7b");
      await probeOllama();
      await refreshStorage();
      onsaved?.();
    } catch (e) {
      error = String(e);
      message = "";
    } finally {
      scanningDisk = false;
    }
  }

  async function pickOllamaPath() {
    try {
      const selected = await openDialog({
        multiple: false,
        directory: false,
        title: "Select ollama executable",
        filters: [
          { name: "Ollama", extensions: ["exe"] },
          { name: "All files", extensions: ["*"] },
        ],
      });
      if (typeof selected === "string" && selected) {
        ollamaBinaryPath = selected;
        await probeOllama();
      }
    } catch (e) {
      error = String(e);
    }
  }

  async function pickOllamaFolder() {
    try {
      const selected = await openDialog({
        multiple: false,
        directory: true,
        title: "Select Ollama install folder",
      });
      if (typeof selected === "string" && selected) {
        ollamaBinaryPath = selected;
        await probeOllama();
      }
    } catch (e) {
      error = String(e);
    }
  }

  async function pickOllamaModelsFolder() {
    try {
      const selected = await openDialog({
        multiple: false,
        directory: true,
        title: "Select Ollama models folder",
      });
      if (typeof selected === "string" && selected) {
        ollamaModelsPath = selected;
        await persistAiSettings(model || pullName || "qwen2.5:7b");
        await refreshStorage();
        message = "Models folder updated.";
      }
    } catch (e) {
      error = String(e);
    }
  }

  function suggestionFit(s: SuggestedModel): string {
    if (!hostRamBytes) return "unknown";
    const need = s.minRamGb * 1024 ** 3;
    if (need <= hostRamBytes * 0.55) return "ok";
    if (need <= hostRamBytes * 0.85) return "tight";
    return "heavy";
  }
</script>

<div class="ai-panel">
  {#if loading}
    <p class="muted">Loading…</p>
  {:else}
    {#if error}<div class="notice error"><AlertTriangle size={14} /> {error}</div>{/if}
    {#if message}<div class="notice ok"><CheckCircle2 size={14} /> {message}</div>{/if}

    <div class="seg" role="tablist" aria-label="AI provider">
      <button type="button" class:on={surface === "local"} onclick={() => setSurface("local")}>Local</button>
      <button type="button" class:on={surface === "cloud"} onclick={() => setSurface("cloud")}>Cloud</button>
    </div>
    <p class="hint">
      {#if surface === "local"}
        Runs on your own PC with Ollama — private and works offline, but needs a model downloaded below.
      {:else}
        Uses an online AI service — nothing to download, but needs an API key.
      {/if}
    </p>

    <div class="status" class:ok={statusKind === "ok"} class:warn={statusKind === "warn"} class:bad={statusKind === "bad"}>
      <span class="status-text">{statusText}</span>
      <div class="status-actions">
        {#if surface === "local"}
          <button
            class="ghost mini"
            type="button"
            title="Refresh"
            onclick={() => { void probeOllama(); void refreshStorage(); }}
            disabled={detecting || scanningDisk}
          >
            <RefreshCw size={14} class={detecting ? "spin" : ""} />
          </button>
        {/if}
        <button
          class="ghost mini"
          type="button"
          onclick={test}
          disabled={testing || saving || pulling || importing}
        >
          <Plug size={14} />
          {testing ? "Testing…" : "Test"}
        </button>
      </div>
    </div>

    {#if surface === "local"}
      <section class="block">
        <div class="block-head">
          <strong>Models</strong>
          {#if hostRamBytes}
            <small>Host RAM {formatBytes(hostRamBytes)}</small>
          {/if}
        </div>

        {#if models.length === 0}
          <p class="muted tight">No models installed yet.</p>
        {:else}
          <div class="model-table" role="list">
            {#each models as m (m.name)}
              <div
                class="model-row"
                class:active={model === m.name}
                role="listitem"
              >
                <button type="button" class="model-main" onclick={() => selectModel(m.name)}>
                  <span class="name">{m.name}</span>
                  <span class="meta">
                    <span>{m.parameterSize || "—"}</span>
                    <span>{m.quantization || "—"}</span>
                    <span>{formatBytes(m.sizeBytes)}</span>
                    <span class="fit" data-fit={m.fit}>{fitLabel(m.fit)}</span>
                  </span>
                </button>
                <button
                  class="ghost mini danger"
                  type="button"
                  title="Delete"
                  disabled={!!deleting || pulling}
                  onclick={() => deleteModel(m.name)}
                >
                  <Trash2 size={14} />
                </button>
              </div>
            {/each}
          </div>
        {/if}

        <div class="install">
          <div class="field-row">
            <input
              bind:value={pullName}
              placeholder="qwen2.5:7b"
              autocomplete="off"
              disabled={pulling}
            />
            {#if pulling}
              <button type="button" class="ghost" onclick={pauseModelPull} title="Pause download">
                <Pause size={14} />
                {pullPct != null ? `Pause ${pullPct}%` : "Pause"}
              </button>
            {:else}
              <button
                type="button"
                onclick={() => installModel()}
                disabled={importing || !pullName.trim()}
                title={canResumePull ? "Resume download from the same place" : "Install model"}
              >
                {#if canResumePull}
                  <Play size={14} />
                  Resume
                {:else}
                  <Download size={14} />
                  Install
                {/if}
              </button>
            {/if}
            <button class="ghost" type="button" onclick={pickGgufAndImport} disabled={pulling || importing}>
              <FileUp size={14} />
              .gguf
            </button>
          </div>
          {#if (pulling || pullPaused) && pullProgress}
            <div class="progress" class:paused={pullPaused}>
              <div class="bar" style:width="{pullPct ?? 8}%"></div>
              <small>
                {pullPaused ? "paused" : pullProgress.status || "downloading"}
                · {formatBytes(pullProgress.completed)} / {formatBytes(pullProgress.total)}
                {#if pulling}
                  · runs in background
                {/if}
              </small>
            </div>
          {/if}

          {#if suggestions.length}
            <div class="suggestions">
              {#each suggestions as s (s.name)}
                {@const fit = suggestionFit(s)}
                <button
                  type="button"
                  class="sug"
                  class:on={pullName === s.name}
                  onclick={() => (pullName = s.name)}
                  disabled={pulling || importing}
                >
                  <span class="sug-name">{s.name}</span>
                  <span class="sug-meta">
                    ~{formatBytes(s.approxSizeBytes)} · ≥{s.minRamGb} GB RAM · ≥{s.minVramGb} GB VRAM
                    <span class="fit" data-fit={fit}>{fitLabel(fit)}</span>
                  </span>
                  <span class="sug-note">{s.note}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </section>

      <div class="storage">
        <span>Used {formatBytes(storage?.usedBytes)}</span>
        <span>Free {formatBytes(storage?.availableBytes)}</span>
        <span class="path" title={storage?.path || ollamaModelsPath || ""}>{storage?.path || ollamaModelsPath || "default models path"}</span>
      </div>
    {:else}
      <section class="block">
        <div class="presets">
          {#each CLOUD_PRESETS as p (p.id)}
            <button type="button" class="preset" class:on={preset === p.id} onclick={() => applyCloudPreset(p.id)}>
              {p.label}
            </button>
          {/each}
        </div>

        <label>
          Model
          <input bind:value={model} placeholder="deepseek/deepseek-chat" autocomplete="off" />
        </label>

        {#if preset === "custom" || preset === "hermes"}
          <label>
            Endpoint
            <input bind:value={endpoint} placeholder="https://api.example.com/v1" autocomplete="off" />
          </label>
        {/if}

        <label>
          <span class="lab"><KeyRound size={12} /> API key {apiKeySet ? "(saved)" : ""}</span>
          <input
            type="password"
            bind:value={apiKeyDraft}
            placeholder={apiKeySet ? "•••••••• (enter new to replace)" : "API key"}
            autocomplete="new-password"
          />
        </label>
        {#if apiKeySet}
          <button class="ghost mini" type="button" onclick={clearKey}>Clear key</button>
        {/if}
      </section>
    {/if}

    <details class="advanced" bind:open={advancedOpen}>
      <summary>Advanced</summary>
      <div class="adv-body">
        {#if surface === "local"}
          <label>
            Ollama binary
            <div class="field-row">
              <input bind:value={ollamaBinaryPath} placeholder="auto-detect" autocomplete="off" />
              <button class="ghost mini" type="button" onclick={pickOllamaPath}><FolderOpen size={14} /></button>
              <button class="ghost mini" type="button" onclick={pickOllamaFolder}>Folder</button>
            </div>
          </label>
          <label>
            Models folder
            <div class="field-row">
              <input bind:value={ollamaModelsPath} placeholder={detect?.modelsPath || "default"} autocomplete="off" />
              <button class="ghost mini" type="button" onclick={pickOllamaModelsFolder}><FolderOpen size={14} /></button>
              <button class="ghost mini" type="button" onclick={scanDiskForOllama} disabled={scanningDisk}>
                <Search size={14} class={scanningDisk ? "spin" : ""} />
                {scanningDisk ? "Scanning…" : "Scan C:"}
              </button>
            </div>
          </label>
          <label>
            Endpoint
            <input bind:value={endpoint} placeholder="http://127.0.0.1:11434" autocomplete="off" />
          </label>
        {:else if preset !== "custom" && preset !== "hermes"}
          <label>
            Endpoint
            <input bind:value={endpoint} autocomplete="off" />
          </label>
        {/if}

        <label>
          Crash explanations come from
          <select bind:value={diagnoseMode}>
            <option value="server">TuffBox's online knowledge base (recommended)</option>
            <option value="local">My local AI model</option>
            <option value="kb_only">Knowledge base only, no AI</option>
          </select>
          <small class="hint" style="margin-top: 2px;">
            When the game crashes, this decides who writes the explanation you see.
          </small>
        </label>
        <label class="check-row">
          <input type="checkbox" bind:checked={speculativeDecoding} />
          Speed up the local AI (draft model)
        </label>
        {#if speculativeDecoding}
          <label>
            Draft model
            <input
              bind:value={draftModel}
              placeholder="qwen2.5-coder:0.5b"
              autocomplete="off"
            />
          </label>
          <p class="hint">
            Lets a tiny helper model sketch the answer first, and your main model checks it —
            noticeably faster on big models. Download the draft model in Ollama before enabling.
          </p>
        {/if}
        <label class="check-row">
          <input type="checkbox" bind:checked={tuneWebResearch} />
          Let the Tune tab look up unknown settings online
        </label>
        <p class="hint">
          When the Tune AI meets a config option it doesn't know, it may check a short list of
          trusted sites (Modrinth, wikis, GitHub). Off = it only uses what's already on your PC.
        </p>
        <label>
          Crash KB URL
          <input bind:value={crashKbEndpoint} placeholder="https://kb.example.com" autocomplete="off" />
          <small class="hint" style="margin-top: 2px;">
            Only change this if you run your own crash knowledge base.
          </small>
        </label>
        <label>
          Crash KB token
          <div class="field-row">
            <input
              type="password"
              bind:value={crashKbTokenDraft}
              placeholder={crashKbTokenSet ? "•••••••• (set)" : "optional"}
              autocomplete="off"
            />
            <button
              class="ghost mini"
              type="button"
              disabled={!crashKbTokenSet}
              onclick={clearCrashKbToken}
            >
              Clear
            </button>
          </div>
        </label>
      </div>
    </details>

    <footer>
      {#if testResult}<small class="test-ok">{testResult}</small>{/if}
      <div class="spacer"></div>
      <button type="button" onclick={save} disabled={saving || scanningDisk || !endpoint.trim() || (surface === "cloud" && !model.trim())}>
        {saving ? "Saving…" : "Save"}
      </button>
    </footer>
  {/if}
</div>

<style>
  .ai-panel {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .muted { color: var(--text-muted); margin: 0; font-size: 13px; }
  .muted.tight { font-size: 12px; }
  .seg {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    padding: 3px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
  }
  .seg button {
    border: none;
    background: transparent;
    color: var(--text-secondary);
    padding: 8px 10px;
    font-size: 12px;
    font-weight: 600;
    border-radius: var(--border-radius-sm);
    cursor: pointer;
  }
  .seg button.on {
    background: var(--bg-elevated, var(--bg-secondary));
    color: var(--text-primary);
  }
  .status {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-primary);
    font-size: 12px;
    color: var(--text-secondary);
  }
  .status.ok { border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent); color: var(--accent-primary); }
  .status.warn { border-color: color-mix(in srgb, var(--accent-warning) 40%, transparent); color: var(--accent-warning); }
  .status.bad { border-color: color-mix(in srgb, var(--accent-danger) 35%, transparent); color: var(--accent-danger); }
  .status-text { flex: 1; min-width: 0; word-break: break-word; }
  .status-actions { display: flex; gap: 8px; flex-shrink: 0; }
  .block {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .block-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }
  .block-head strong { font-size: 13px; color: var(--text-primary); }
  .block-head small { font-size: 12px; color: var(--text-muted); }
  .model-table {
    display: flex;
    flex-direction: column;
    gap: 8px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    overflow: hidden;
  }
  .model-row {
    display: flex;
    align-items: stretch;
    gap: 8px;
    background: var(--bg-primary);
    border-bottom: 1px solid var(--border-color);
  }
  .model-row:last-child { border-bottom: none; }
  .model-row.active { background: rgba(251, 191, 36, 0.08); }
  .model-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    padding: 8px 10px;
    border: none;
    background: transparent;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }
  .model-main .name {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    word-break: break-all;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .fit {
    font-weight: 700;
    letter-spacing: 0.02em;
  }
  .fit[data-fit="ok"] { color: var(--accent-primary); }
  .fit[data-fit="tight"] { color: var(--accent-warning); }
  .fit[data-fit="heavy"] { color: var(--accent-danger); }
  .install { display: flex; flex-direction: column; gap: 8px; }
  .field-row { display: flex; gap: 8px; align-items: center; }
  .field-row input { flex: 1; min-width: 0; }
  .progress {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .progress .bar {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(145deg, #fbbf24, #d97706);
    transition: width 0.2s ease;
  }
  .progress.paused .bar {
    background: linear-gradient(145deg, #94a3b8, #64748b);
  }
  .progress small { font-size: 12px; color: var(--text-muted); }
  .suggestions {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .sug {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    padding: 8px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    cursor: pointer;
    text-align: left;
  }
  .sug.on {
    border-color: rgba(251, 191, 36, 0.5);
    background: rgba(251, 191, 36, 0.08);
  }
  .sug-name { font-size: 12px; font-weight: 700; color: var(--text-primary); }
  .sug-meta { font-size: 12px; color: var(--text-muted); display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .sug-note { font-size: 12px; color: var(--text-secondary); }
  .storage {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    font-size: 12px;
    color: var(--text-muted);
    padding: 6px 2px;
  }
  .storage .path {
    flex: 1 1 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono, ui-monospace, monospace);
  }
  .presets { display: flex; flex-wrap: wrap; gap: 8px; }
  .preset {
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    border-radius: 999px;
    padding: 5px 10px;
    font-size: 12px;
    cursor: pointer;
  }
  .preset.on {
    background: linear-gradient(145deg, #fbbf24, #d97706);
    color: #1a1200;
    border-color: transparent;
    font-weight: 700;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .lab { display: inline-flex; align-items: center; gap: 8px; }
  input, select {
    background: var(--bg-primary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
    padding: 8px 10px;
    color: var(--text-primary);
    font-size: 13px;
  }
  .advanced {
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    padding: 0;
    background: var(--bg-primary);
  }
  .advanced summary {
    cursor: pointer;
    padding: 8px 10px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    list-style: none;
  }
  .advanced summary::-webkit-details-marker { display: none; }
  .adv-body {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 0 10px 10px;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 2px;
    flex-wrap: wrap;
  }
  .spacer { flex: 1; }
  button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    border-radius: var(--border-radius-sm);
    padding: 8px 12px;
    font-size: 12px;
    cursor: pointer;
    border: 1px solid transparent;
    background: linear-gradient(145deg, #fbbf24, #d97706);
    color: #1a1200;
    font-weight: 700;
  }
  button:disabled { opacity: 0.55; cursor: not-allowed; }
  button.ghost {
    background: var(--bg-tertiary);
    border-color: var(--border-color);
    color: var(--text-secondary);
    font-weight: 500;
  }
  button.mini { padding: 6px 8px; }
  button.danger { color: var(--accent-danger); }
  .check-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
  }
  .check-row input { margin: 0; }
  .hint {
    margin: 0;
    color: var(--text-muted);
    font-size: 12px;
    line-height: 1.4;
    overflow-wrap: anywhere;
  }
  .notice {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    font-size: 12px;
    line-height: 1.35;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
  }
  .notice.error { background: color-mix(in srgb, var(--accent-danger) 12%, transparent); color: var(--accent-danger); }
  .notice.ok { background: color-mix(in srgb, var(--accent-primary) 12%, transparent); color: var(--accent-primary); }
  .test-ok { color: var(--accent-primary); font-size: 12px; }
  :global(.spin) { animation: spin 0.9s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
