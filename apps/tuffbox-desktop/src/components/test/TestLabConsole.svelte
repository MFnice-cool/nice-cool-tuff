<script lang="ts">
  import {
    Terminal,
    Search,
    Copy,
    Eraser,
    Stethoscope,
  } from "@lucide/svelte";
  import { tick } from "svelte";

  type LiveDebugStats = {
    hostCpuPercent: number;
    hostMemoryUsedMb: number;
    hostMemoryTotalMb: number;
    instance: null | {
      pid: number;
      profile: string;
      startedAt: number;
      cpuPercent: number;
      memoryMb: number;
      virtualMemoryMb: number;
    };
  };

  const LOG_TAIL_LINES = 500;

  let {
    log = $bindable(),
    running = false,
    watching = false,
    live = $bindable<LiveDebugStats | null>(null),
    elapsed = 0,
    autoScroll = $bindable(true),
    activeLogRoot = $bindable<string | null>(null),
    projectPath = "",
    onclear,
    onopenserver,
    ondiagnose,
    onpause,
    onwatch,
  }: {
    log?: string;
    running?: boolean;
    watching?: boolean;
    live?: LiveDebugStats | null;
    elapsed?: number;
    autoScroll?: boolean;
    activeLogRoot?: string | null;
    projectPath?: string;
    onclear?: () => void;
    onopenserver?: () => void;
    ondiagnose?: () => void;
    onpause?: () => void;
    onwatch?: () => void;
  } = $props();

  /** Log level filter + free-text search over the raw log. */
  let logFilter = $state<"all" | "info" | "warn" | "error">("all");
  let logSearch = $state("");
  let logEl = $state<HTMLPreElement | null>(null);

  const logLevelTabs: Array<{ id: typeof logFilter; label: string }> = [
    { id: "all", label: "ALL" },
    { id: "info", label: "INFO" },
    { id: "warn", label: "WARN" },
    { id: "error", label: "ERROR" },
  ];

  const displayLog = $derived(tailLogLines(log ?? "", LOG_TAIL_LINES));
  const logLineCount = $derived(log ? log.split("\n").length : 0);
  const logTruncated = $derived(logLineCount > LOG_TAIL_LINES);

  const filteredDisplayLog = $derived.by(() => {
    const base = displayLog;
    if (!base) return "";
    let lines = base.split("\n");
    if (logFilter !== "all") {
      const kw =
        logFilter === "warn"
          ? /WARN|WARNING/i
          : logFilter === "error"
            ? /ERROR|FATAL|CRASH|Exception in/i
            : /INFO|\[|INFO\]/i;
      lines = lines.filter((l) => kw.test(l));
    }
    if (logSearch.trim()) {
      const q = logSearch.trim().toLowerCase();
      lines = lines.filter((l) => l.toLowerCase().includes(q));
    }
    if (lines.length === 0) return base ? "(no matching lines)" : "";
    return lines.join("\n");
  });

  function tailLogLines(text: string, maxLines: number): string {
    if (!text) return "";
    const lines = text.split("\n");
    if (lines.length <= maxLines) return text;
    const omitted = lines.length - maxLines;
    return `… (${omitted} earlier lines omitted)\n${lines.slice(-maxLines).join("\n")}`;
  }

  async function copyLog() {
    if (!log) return;
    try {
      await navigator.clipboard.writeText(log);
    } catch {
      /* ignore */
    }
  }

  async function scrollToEnd() {
    if (autoScroll && logEl) {
      await tick();
      logEl.scrollTop = logEl.scrollHeight;
    }
  }
  // Exposed to parent via a mount callback is not required — scroll handled by
  // the parent passing an updated log; Svelte re-renders and we re-scroll.
  $effect(() => {
    void scrollToEnd();
  });
</script>

<div class="flex-1 min-h-0 flex flex-col overflow-hidden glass-card">
  <!-- Console toolbar -->
  <div class="flex items-center justify-between gap-2.5 px-4 py-2.5 border-b border-[color:var(--border-color)] shrink-0 flex-wrap">
    <div class="flex items-center gap-2.5 min-w-0">
      <span class="flex items-center gap-2 text-[12px] font-semibold text-[color:var(--text-primary)]">
        <Terminal size={13} />
        <span class="font-mono">latest.log</span>
      </span>
      {#if live?.instance}
        <span class="console-status">
          <span class="status-dot live"></span>
          PID {live.instance.pid} · {elapsed}s
        </span>
      {:else if running}
        <span class="console-status">
          <span class="status-dot busy"></span> launching…
        </span>
      {:else}
        <span class="console-status"><span class="status-dot off"></span> idle</span>
      {/if}
    </div>
    <div class="flex items-center gap-2 flex-wrap">
      <label class="flex items-center gap-2 text-[color:var(--text-muted)] text-[12px] cursor-pointer" title="Toggle auto-scroll">
        <input type="checkbox" class="w-auto accent-[var(--accent-primary)]" bind:checked={autoScroll} /> Auto-scroll
      </label>
      {#if log}
        <button class="icon-btn" onclick={copyLog} title="Copy log to clipboard" aria-label="Copy log">
          <Copy size={13} />
        </button>
        <button class="icon-btn" onclick={onclear} title="Clear log" aria-label="Clear log">
          <Eraser size={13} />
        </button>
      {/if}
      {#if activeLogRoot && activeLogRoot !== projectPath}
        <button class="icon-btn" onclick={onopenserver} title="Open server console" aria-label="Open server console">
          <Terminal size={13} />
        </button>
      {/if}
      <button class="icon-btn" onclick={ondiagnose} title="Open in Diagnose" aria-label="Open in Diagnose">
        <Stethoscope size={13} />
      </button>
      {#if watching}
        <button class="ghost mini" onclick={onpause}>Pause</button>
      {:else if running || live?.instance}
        <button class="ghost mini" onclick={onwatch}>Watch</button>
      {/if}
    </div>
  </div>

  <!-- Log filter bar -->
  <div class="flex items-center gap-2 px-4 py-2 border-b border-[color:var(--border-color)] shrink-0 flex-wrap">
    <div class="seg-control" role="group" aria-label="Log level filter">
      {#each logLevelTabs as t (t.id)}
        <button
          type="button"
          class:active={logFilter === t.id}
          onclick={() => (logFilter = t.id)}
        >
          {t.label}
        </button>
      {/each}
    </div>
    <div class="relative flex-1 min-w-[140px]">
      <Search size={12} class="search-ico" />
      <input
        type="text"
        class="search-input"
        placeholder="Filter log text…"
        bind:value={logSearch}
      />
    </div>
    {#if logTruncated}
      <span class="text-[12px] text-[color:var(--text-muted)] whitespace-nowrap">
        tail {LOG_TAIL_LINES}/{logLineCount}
      </span>
    {/if}
  </div>

  <!-- Log body -->
  {#if displayLog}
    <pre class="flex-1 min-h-0 overflow-auto m-0 px-4 py-3.5 console-log" bind:this={logEl}>{filteredDisplayLog}</pre>
  {:else}
    <div class="flex-1 min-h-0 grid place-items-center px-4">
      <div class="console-empty text-center">
        <Terminal size={34} class="mx-auto opacity-60" />
        <p class="mt-3 text-[13px] font-semibold text-[color:var(--text-secondary)]">Waiting for output</p>
        <p class="mt-1 text-[12px] text-[color:var(--text-muted)] max-w-[280px] mx-auto leading-relaxed">
          latest.log appears here once a profile runs. Start one from the bar above, or press{" "}
          <kbd class="kbd">Ctrl</kbd> + <kbd class="kbd">Shift</kbd> + <kbd class="kbd">P</kbd> to
          launch from anywhere in the IDE.
        </p>
      </div>
    </div>
  {/if}
</div>

<style>
  .console-log {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12.5px;
    line-height: 1.6;
    color: var(--text-secondary);
    white-space: pre-wrap;
    background: var(--bg-elevated);
    border-top: 1px solid var(--border-color);
  }
  .console-status {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
    font-family: var(--font-mono, ui-monospace, monospace);
  }
  .status-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    display: inline-block;
  }
  .status-dot.live {
    background: var(--accent-primary);
    box-shadow: 0 0 6px color-mix(in srgb, var(--accent-primary) 60%, transparent);
  }
  .status-dot.busy {
    background: var(--accent-warning);
    box-shadow: 0 0 6px color-mix(in srgb, var(--accent-warning) 60%, transparent);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .status-dot.off {
    background: var(--text-muted);
  }
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }
  .seg-control {
    display: inline-flex;
    gap: 8px;
    padding: 2px;
    border-radius: var(--border-radius-sm);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
  }
  .seg-control button {
    border: none;
    background: transparent;
    color: var(--text-muted);
    padding: 5px 11px;
    border-radius: var(--border-radius-sm);
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.03em;
    cursor: pointer;
  }
  .seg-control button.active {
    background: color-mix(in srgb, var(--accent-primary) 16%, transparent);
    /* Accent-on-accent-tint measured 2.62:1 in the light themes; the mix with
       the primary text colour keeps the hue and restores contrast in every
       theme (darker on light panels, lighter on dark ones). */
    color: color-mix(in srgb, var(--accent-primary) 62%, var(--text-primary));
  }
  .icon-btn {
    width: 30px;
    height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    cursor: pointer;
    transition: background var(--motion-fast, 160ms) ease, color var(--motion-fast, 160ms) ease;
  }
  .icon-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .mini { padding: 5px 9px; font-size: 12px; justify-self: start; }
  :global(.search-ico) {
    position: absolute;
    left: 9px;
    top: 50%;
    transform: translateY(-50%);
    color: var(--text-muted);
    pointer-events: none;
  }
  .search-input {
    width: 100%;
    height: 30px;
    padding: 5px 10px 5px 28px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
    color: var(--text-primary);
    font-size: 12px;
  }
  .search-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }
  .console-empty kbd {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
    padding: 2px 6px;
    border-radius: 6px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }
  .kbd {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
    padding: 2px 6px;
    border-radius: 6px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }
</style>