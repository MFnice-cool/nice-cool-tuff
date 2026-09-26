<script lang="ts">
  import { GitMerge, ChevronDown, AlertTriangle } from "@lucide/svelte";

  type Diagnostic = {
    severity: string;
    code: string;
    message: string;
    relatedNodes: any[];
  };
  type DupJar = {
    fileName: string;
    modId: string;
    mtimeMs: number;
    size: number;
    inManifest: boolean;
  };
  type DupJarGroup = { modId: string; keepCandidate: string; jars: DupJar[] };
  type WrongLoaderJar = {
    fileName: string;
    detectedLoader: string;
    projectLoader: string;
    recommendation: string;
    reason: string;
  };

  let {
    graphDiagnostics = [],
    duplicateJarGroups = [],
    wrongLoaderJars = [],
    fixingIdx = null,
    duplicateJarFixing = null,
    wrongLoaderFixing = null,
    onFixMissingDependency,
    onFixDeduplicate,
    onKeepOneDuplicateJar,
    onDisableWrongJar,
    onRemoveWrongJar,
  }: {
    graphDiagnostics?: Diagnostic[];
    duplicateJarGroups?: DupJarGroup[];
    wrongLoaderJars?: WrongLoaderJar[];
    fixingIdx?: number | null;
    duplicateJarFixing?: string | null;
    wrongLoaderFixing?: string | null;
    onFixMissingDependency?: (payload: { modId: string; idx: number }) => void;
    onFixDeduplicate?: (idx: number) => void;
    onKeepOneDuplicateJar?: (payload: { modId: string; fileName: string }) => void;
    onDisableWrongJar?: (fileName: string) => void;
    onRemoveWrongJar?: (fileName: string) => void;
  } = $props();

  function missingModIdFromDiag(d: Diagnostic): string {
    const to = d.relatedNodes?.[1];
    if (typeof to === "string") return to.replace(/^mod:/i, "").trim();
    if (to && typeof to === "object" && to !== null && "0" in to) {
      return String((to as { 0: unknown })[0] ?? "").replace(/^mod:/i, "").trim();
    }
    return (d.message.match(/['"`]?([a-z0-9_-]{3,})['"`]?\s*$/i) || [])[1] || "";
  }
</script>

<!-- 4. Evidence (secondary) -->
<details class="panel collapsible-block dx-evidence-block" open={graphDiagnostics.length > 0 || wrongLoaderJars.length > 0 || duplicateJarGroups.length > 0}>
  <summary>
    <span><GitMerge size={16} /> Conflicts & jars</span>
    <span class="tools-hint">
      {graphDiagnostics.length} conflict{graphDiagnostics.length === 1 ? "" : "s"}
      {#if wrongLoaderJars.length} · {wrongLoaderJars.length} wrong jar{/if}
      {#if duplicateJarGroups.length} · {duplicateJarGroups.length} dup{/if}
      <ChevronDown size={14} />
    </span>
  </summary>
  <div class="dx-evidence-body">
    {#if graphDiagnostics.length === 0 && !wrongLoaderJars.length && !duplicateJarGroups.length}
      <div class="muted-box">No graph conflicts or jar issues.</div>
    {/if}
    {#if graphDiagnostics.length > 0}
      <div class="diag-list">
        {#each graphDiagnostics as d, idx (d.code + d.message + idx)}
          <div class="diag-row {String(d.severity).toLowerCase()}">
            <div>
              <strong>{d.code}</strong>
              <p>{d.message}</p>
            </div>
            <div class="diag-actions">
              {#if /MISSING|DEPEND/i.test(d.code + d.message)}
                {@const mid = missingModIdFromDiag(d)}
                {#if mid}
                  <button class="secondary small" onclick={() => onFixMissingDependency?.({ modId: mid, idx })} disabled={fixingIdx === idx}>
                    Install {mid}
                  </button>
                {/if}
              {/if}
              {#if /DUPLICATE/i.test(d.code)}
                <button class="secondary small" onclick={() => onFixDeduplicate?.(idx)} disabled={fixingIdx === idx || duplicateJarFixing !== null}>
                  Keep one jar
                </button>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
    {#if duplicateJarGroups.length > 0}
      <h3 class="dx-subhead"><AlertTriangle size={14} /> Duplicate mod jars</h3>
      <p class="tools-hint" style="margin: 0 0 8px">Same mod id in more than one jar — keep one, delete the rest.</p>
      {#each duplicateJarGroups as group (group.modId)}
        <div class="diag-row warning">
          <div>
            <strong>{group.modId}</strong>
            <p>{group.jars.length} jars · suggested keep: <code>{group.keepCandidate}</code></p>
            <ul class="dup-jar-list">
              {#each group.jars as jar (jar.fileName)}
                <li>
                  <code>{jar.fileName}</code>
                  {#if jar.inManifest}<span class="pill">manifest</span>{/if}
                  {#if jar.fileName === group.keepCandidate}<span class="pill">newest</span>{/if}
                  <button
                    class="ghost mini"
                    disabled={duplicateJarFixing !== null}
                    onclick={() => onKeepOneDuplicateJar?.({ modId: group.modId, fileName: jar.fileName })}
                    title="Keep this jar, delete the other copies"
                  >
                    {duplicateJarFixing === `${group.modId}::${jar.fileName}` ? "…" : "Keep this"}
                  </button>
                </li>
              {/each}
            </ul>
          </div>
          <div class="diag-actions">
            <button
              class="secondary small"
              disabled={duplicateJarFixing !== null}
              onclick={() => onKeepOneDuplicateJar?.({ modId: group.modId, fileName: group.keepCandidate })}
            >
              Keep newest
            </button>
          </div>
        </div>
      {/each}
    {/if}
    {#if wrongLoaderJars.length > 0}
      <h3 class="dx-subhead"><AlertTriangle size={14} /> Wrong-loader jars</h3>
      {#each wrongLoaderJars as jar (jar.fileName)}
        <div class="diag-row warning">
          <div>
            <strong>{jar.fileName}</strong>
            <p>{jar.reason ?? jar.detectedLoader ?? "Wrong loader"}</p>
          </div>
          <div class="diag-actions">
            <button class="ghost mini" onclick={() => onDisableWrongJar?.(jar.fileName)} disabled={wrongLoaderFixing === jar.fileName}>Disable</button>
            <button class="ghost mini danger" onclick={() => onRemoveWrongJar?.(jar.fileName)} disabled={wrongLoaderFixing === jar.fileName}>Remove</button>
          </div>
        </div>
      {/each}
    {/if}
  </div>
</details>

<style>
  .panel { padding: 16px; min-width: 0; background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: var(--border-radius-lg); }
  .muted-box { padding: 12px; border-radius: 10px; border: 1px dashed var(--border-color); color: var(--text-muted); font-size: 12px; }
  .ghost.danger { color: var(--accent-danger); }
  .ghost.danger:hover { color: var(--accent-danger); }
  .collapsible-block {
    margin-bottom: 12px;
    padding: 0;
  }
  .collapsible-block > summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    color: var(--text-secondary);
    cursor: pointer;
    list-style: none;
    font-size: 12px;
    font-weight: 700;
  }
  .collapsible-block > summary::-webkit-details-marker { display: none; }
  .collapsible-block > summary span { display: flex; align-items: center; gap: 8px; }
  .collapsible-block .tools-hint { color: var(--text-muted); font-weight: 500; }
  .collapsible-block[open] .tools-hint :global(svg),
  .collapsible-block[open] > summary :global(svg:last-child) { transform: rotate(180deg); }
  .dx-evidence-block .dx-evidence-body { padding: 0 12px 12px; }
  .dx-subhead {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 14px 0 8px;
    color: var(--text-secondary);
    font-size: 13px;
  }
  .diag-list { display: flex; flex-direction: column; gap: 8px; }
  .diag-row {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    border-radius: 10px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
  }
  .diag-row p { margin: 4px 0 0; color: var(--text-secondary); font-size: 12px; }
  .diag-actions { display: flex; flex-wrap: wrap; gap: 8px; align-items: flex-start; }
  .dup-jar-list {
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .dup-jar-list li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-secondary);
  }
  .dup-jar-list .pill {
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    padding: 2px 6px;
    border-radius: 999px;
    border: 1px solid var(--border-color);
    color: var(--text-muted);
  }
</style>
