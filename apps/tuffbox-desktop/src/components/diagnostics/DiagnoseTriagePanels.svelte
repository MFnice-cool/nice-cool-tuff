<script lang="ts">
  import {
    AlertTriangle,
    ChevronDown,
    Database,
    Search,
    Layers,
    Ban,
    ArrowUpCircle,
  } from "@lucide/svelte";

  let {
    signalGroups = [],
    sections = [],
    suspected = [],
    recentSnapshots = [],
    mcreatorMods = [],
    classFinderResults = [],
    classQuery = $bindable(""),
    classBusy = false,
    classResults = [],
    dependentResults = [],
    toolsOpen = $bindable(false),
    disablingModId = null,
    bisectMods = [],
    worldCoords = null,
    memoryHint = null,
    cascadingBanner = null,
    sourceHint = "",
    onJumpLine,
    onDisableMod,
    onUpdateMod,
    onToggleBisect,
    onFindClass,
    onFindDependents,
    onOpenSnapshots,
  }: {
    signalGroups?: {
      title: string;
      hint: string;
      items: { source: string; lineNumber: number; kind: string; text: string }[];
    }[];
    sections?: { title: string; startLine: number; endLine: number; preview: string }[];
    suspected?: {
      id: string;
      name: string;
      confidence: number;
      knownInManifest: boolean;
      blameRole?: string;
      evidence?: { text: string; lineNumber: number }[];
    }[];
    recentSnapshots?: { id: string; name: string; createdAt: string; reason: string }[];
    mcreatorMods?: string[];
    classFinderResults?: { className: string; modId: string; modName: string }[];
    classQuery?: string;
    classBusy?: boolean;
    classResults?: { className: string; modId: string; modName: string }[];
    dependentResults?: { className: string; modId: string; modName: string }[];
    toolsOpen?: boolean;
    disablingModId?: string | null;
    bisectMods?: string[];
    worldCoords?: { x: number; y: number; z: number; label: string } | null;
    memoryHint?: string | null;
    cascadingBanner?: string | null;
    sourceHint?: string;
    onJumpLine?: (line: number) => void;
    onDisableMod?: (modId: string) => void;
    onUpdateMod?: (modId: string) => void;
    onToggleBisect?: (modId: string) => void;
    onFindClass?: (query: string) => void;
    onFindDependents?: (query: string) => void;
    onOpenSnapshots?: () => void;
  } = $props();
</script>

{#if sourceHint}
  <p class="dx-source-hint muted-inline">{sourceHint}</p>
{/if}

{#if cascadingBanner}
  <div class="dx-banner warn">
    <AlertTriangle size={14} />
    <span>{cascadingBanner}</span>
    <button type="button" class="ghost mini" onclick={() => onJumpLine?.(0)}>Jump to early error</button>
  </div>
{/if}

{#if memoryHint}
  <div class="dx-banner info">
    <strong>Memory</strong>
    <span>{memoryHint}</span>
    <button type="button" class="ghost mini" onclick={() => onOpenSnapshots?.()}>Setup / JVM</button>
  </div>
{/if}

{#if worldCoords}
  <div class="dx-banner info">
    <strong>{worldCoords.label} coords</strong>
    <code>{worldCoords.x}, {worldCoords.y}, {worldCoords.z}</code>
    <span class="muted-inline">Hint: restore nearby chunk or teleport away if a ticking entity is stuck.</span>
  </div>
{/if}

{#if suspected.length > 0 || recentSnapshots.length > 0}
  <section class="panel dx-suspects">
    <div class="dx-suspects-grid">
      {#if suspected.length}
        <div>
          <h3><AlertTriangle size={14} /> Suspected mods</h3>
          <ul class="suspect-list">
            {#each suspected.slice(0, 8) as m (m.id)}
              <li>
                <div class="suspect-main">
                  <strong>{m.name}</strong>
                  <code>{m.id}</code>
                  <span class="pill">{m.confidence}%</span>
                  {#if m.blameRole}<span class="pill">{m.blameRole}</span>{/if}
                </div>
                <div class="suspect-acts">
                  {#if m.knownInManifest}
                    <button
                      type="button"
                      class="ghost mini"
                      disabled={disablingModId === m.id}
                      onclick={() => onDisableMod?.(m.id)}
                    >
                      <Ban size={12} /> Disable
                    </button>
                    <button type="button" class="ghost mini" onclick={() => onUpdateMod?.(m.id)}>
                      <ArrowUpCircle size={12} /> Update
                    </button>
                  {/if}
                  <label class="bisect-check" title="Include in group-test pool">
                    <input
                      type="checkbox"
                      checked={bisectMods.includes(m.id)}
                      onchange={() => onToggleBisect?.(m.id)}
                    />
                    Pool
                  </label>
                </div>
              </li>
            {/each}
          </ul>
        </div>
      {/if}
      {#if recentSnapshots.length}
        <div>
          <h3><Database size={14} /> Recent snapshots</h3>
          <ul class="snap-list">
            {#each recentSnapshots.slice(0, 5) as s (s.id)}
              <li>
                <strong>{s.name}</strong>
                <small>{s.reason} · {s.createdAt?.slice(0, 19)}</small>
              </li>
            {/each}
          </ul>
          <button type="button" class="ghost mini" onclick={() => onOpenSnapshots?.()}>Open Snapshots</button>
        </div>
      {/if}
    </div>
  </section>
{/if}

{#if signalGroups.length > 0 || sections.length > 0}
  <details class="panel collapsible-block">
    <summary>
      <span><Layers size={16} /> Evidence</span>
      <span class="tools-hint">
        {signalGroups.reduce((n, g) => n + g.items.length, 0)} signals
        {#if sections.length} · {sections.length} sections{/if}
        <ChevronDown size={14} />
      </span>
    </summary>
    <div class="dx-ev-body">
      {#if signalGroups.length}
        <div class="sig-groups">
          {#each signalGroups as g (g.title)}
            <div class="sig-group">
              <strong>{g.title}</strong>
              <small>{g.hint}</small>
              <ul>
                {#each g.items.slice(0, 6) as item, i (g.title + i + item.lineNumber)}
                  <li>
                    <button
                      type="button"
                      class="sig-line"
                      onclick={() => onJumpLine?.(item.lineNumber)}
                      title={item.text}
                    >
                      L{item.lineNumber}: {item.text.slice(0, 120)}
                    </button>
                  </li>
                {/each}
              </ul>
            </div>
          {/each}
        </div>
      {/if}
      {#if sections.length}
        <div class="sec-list">
          {#each sections as sec (sec.title + sec.startLine)}
            <button type="button" class="sec-card" onclick={() => onJumpLine?.(sec.startLine)}>
              <strong>{sec.title}</strong>
              <pre>{sec.preview.slice(0, 280)}</pre>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </details>
{/if}

<details class="panel collapsible-block" bind:open={toolsOpen}>
  <summary>
    <span><Search size={16} /> Analysis tools</span>
    <span class="tools-hint">
      Class finder · MCreator
      {#if mcreatorMods.length} · {mcreatorMods.length} MCreator{/if}
      <ChevronDown size={14} />
    </span>
  </summary>
  <div class="dx-tools-body">
    <div class="class-finder">
      <label>
        Class / package
        <input
          type="search"
          placeholder="e.g. me.jellysquid.mods.sodium"
          bind:value={classQuery}
          onkeydown={(e) => {
            if (e.key === "Enter") onFindClass?.(classQuery);
          }}
        />
      </label>
      <div class="actions">
        <button
          type="button"
          class="secondary small"
          disabled={classBusy || !classQuery.trim()}
          onclick={() => onFindClass?.(classQuery)}
        >
          Find in mods
        </button>
        <button
          type="button"
          class="ghost mini"
          disabled={classBusy || !classQuery.trim()}
          onclick={() => onFindDependents?.(classQuery)}
        >
          Who depends?
        </button>
      </div>
      {#if classResults.length}
        <ul class="class-hits">
          {#each classResults as r (r.modId + r.className)}
            <li><code>{r.modId}</code> — {r.modName} <small>{r.className}</small></li>
          {/each}
        </ul>
      {/if}
      {#if dependentResults.length}
        <ul class="class-hits">
          {#each dependentResults as r (r.modId + "d" + r.className)}
            <li>depends: <code>{r.modId}</code> — {r.modName}</li>
          {/each}
        </ul>
      {/if}
      {#if classFinderResults.length && !classResults.length}
        <p class="muted-inline">From crash analysis:</p>
        <ul class="class-hits">
          {#each classFinderResults.slice(0, 8) as r (r.modId + r.className)}
            <li><code>{r.modId}</code> — {r.className}</li>
          {/each}
        </ul>
      {/if}
    </div>
    {#if mcreatorMods.length}
      <div class="mcreator">
        <strong>MCreator mods</strong>
        <div class="crash-tags">
          {#each mcreatorMods as id (id)}
            <code>{id}</code>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</details>

<style>
  .dx-source-hint {
    margin: 0 0 8px;
    font-size: 12px;
  }
  .dx-banner {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    margin-bottom: 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    font-size: 12px;
    background: var(--bg-secondary);
  }
  .dx-banner.warn {
    border-color: rgba(251, 191, 36, 0.45);
    background: rgba(251, 191, 36, 0.08);
  }
  .dx-banner.info {
    border-color: rgba(96, 165, 250, 0.35);
    background: rgba(96, 165, 250, 0.06);
  }
  .dx-ev-body {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 8px 0 4px;
  }
  .sig-groups {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 10px;
  }
  .sig-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    border-radius: var(--border-radius-sm);
    background: var(--bg-primary);
    border: 1px solid var(--border-color);
  }
  .sig-group small {
    color: var(--text-muted);
    font-size: 12px;
  }
  .sig-group ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .sig-line {
    display: block;
    width: 100%;
    text-align: left;
    font-size: 12px;
    font-family: var(--font-mono, ui-monospace, monospace);
    padding: 4px 0;
    border: none;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
  }
  .sig-line:hover {
    color: var(--accent-primary, #0284c7);
  }
  .sec-list {
    display: grid;
    gap: 8px;
  }
  .sec-card {
    text-align: left;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-primary);
    color: inherit;
    cursor: pointer;
  }
  .sec-card pre {
    margin: 6px 0 0;
    font-size: 12px;
    white-space: pre-wrap;
    color: var(--text-muted);
    max-height: 72px;
    overflow: hidden;
  }
  .dx-suspects {
    margin-bottom: 12px;
    padding: 12px;
  }
  .dx-suspects-grid {
    display: grid;
    grid-template-columns: 1.4fr 1fr;
    gap: 16px;
  }
  .dx-suspects h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 8px;
    font-size: 13px;
  }
  .suspect-list,
  .snap-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .suspect-list li,
  .snap-list li {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    border-radius: var(--border-radius-sm);
    background: var(--bg-primary);
    border: 1px solid var(--border-color);
  }
  .suspect-main {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }
  .suspect-acts {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  .bisect-check {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
    cursor: pointer;
  }
  .pill {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--bg-elevated, rgba(255, 255, 255, 0.06));
    color: var(--text-muted);
  }
  .dx-tools-body {
    display: grid;
    gap: 12px;
    padding-top: 8px;
  }
  .class-finder label {
    display: grid;
    gap: 8px;
    font-size: 12px;
  }
  .class-finder input {
    padding: 6px 8px;
    border-radius: 6px;
    border: 1px solid var(--border-color);
    background: var(--bg-primary);
    color: inherit;
  }
  .class-hits {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
    font-size: 12px;
  }
  .class-hits li {
    padding: 4px 0;
    border-bottom: 1px solid var(--border-color);
  }
  .crash-tags {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 6px;
  }
  .muted-inline {
    color: var(--text-muted);
    font-size: 12px;
  }
  @media (max-width: 900px) {
    .dx-suspects-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
