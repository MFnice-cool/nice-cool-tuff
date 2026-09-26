<script lang="ts">
  import { X, Package, Loader2, Check, Plus, Trash2, Download, Search } from "@lucide/svelte";
  import { api } from "../lib/api";
  import { projectPath } from "../lib/store";
  import { toasts } from "../lib/toast";
  import { trapFocus } from "../lib/focusTrap";
  import { portal } from "../lib/portal";
  import { get } from "svelte/store";

  let {
    open = $bindable(false),
    onInstalled,
  }: {
    open?: boolean;
    onInstalled?: () => void;
  } = $props();

  type PresetMod = {
    provider: string;
    projectId: string;
    slug: string;
    name: string;
  };

  type Preset = {
    id: string;
    name: string;
    createdAt: string;
    mods: PresetMod[];
  };

  type ResolvedOffer = {
    slug: string;
    name: string;
    provider: string;
    projectId: string;
    versionId?: string | null;
    reason: string;
    risk: string;
    alreadyInstalled: boolean;
    /** Set when resolution failed for the current MC/loader. */
    resolveError?: string;
  };

  type SearchResultRow = {
    id: string;
    slug: string;
    name: string;
    provider: string;
  };

  let presets = $state<Preset[]>([]);
  let activePresetId = $state<string | null>(null);
  let loading = $state(false);
  let saving = $state(false);
  let resolving = $state(false);
  let installing = $state(false);
  let error = $state<string | null>(null);
  let doneMessage = $state<string | null>(null);
  let newPresetName = $state("");
  let renameDraft = $state("");
  let resolvedOffers = $state<ResolvedOffer[]>([]);

  // Search state (Modrinth / CurseForge)
  let searchQuery = $state("");
  let searchProvider = $state<"modrinth" | "curseforge">("modrinth");
  let searching = $state(false);
  let searchError = $state<string | null>(null);
  let searchResults = $state<SearchResultRow[]>([]);
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  const activePreset = $derived(presets.find((p) => p.id === activePresetId) ?? null);
  const resolvedForActive = $derived.by(() => {
    if (!activePreset) return new Map<string, ResolvedOffer>();
    const map = new Map<string, ResolvedOffer>();
    for (const offer of resolvedOffers) {
      map.set(`${offer.provider}:${offer.projectId}`, offer);
    }
    return map;
  });
  const resolvedStats = $derived.by(() => {
    if (!activePreset) return { total: 0, ok: 0, failed: 0, installed: 0 };
    let ok = 0;
    let failed = 0;
    let installed = 0;
    for (const m of activePreset.mods) {
      const offer = resolvedForActive.get(`${m.provider}:${m.projectId}`);
      if (!offer) continue;
      if (offer.resolveError) failed += 1;
      else {
        ok += 1;
        if (offer.alreadyInstalled) installed += 1;
      }
    }
    return { total: activePreset.mods.length, ok, failed, installed };
  });
  const installDisabled = $derived(
    installing || resolving || !activePreset || activePreset.mods.length === 0,
  );
  const installLabel = $derived.by(() => {
    if (!activePreset) return "Install preset";
    const toInstall = resolvedStats.ok - resolvedStats.installed;
    if (resolvedStats.failed > 0) {
      return `Install (${toInstall} of ${activePreset.mods.length})`;
    }
    if (toInstall > 0) return `Install (${toInstall} mod${toInstall === 1 ? "" : "s"})`;
    return "Install (already installed)";
  });

  function close() {
    open = false;
  }

  function uid(): string {
    return `preset-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
  }

  async function refresh() {
    loading = true;
    error = null;
    try {
      const store = await api.mods.getPresets();
      presets = store.presets ?? [];
      if (activePresetId && !presets.some((p) => p.id === activePresetId)) {
        activePresetId = presets[0]?.id ?? null;
      }
      if (!activePresetId) activePresetId = presets[0]?.id ?? null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function persist(next: Preset[]) {
    saving = true;
    try {
      await api.mods.savePresets({ presets: next });
      presets = next;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      toasts.error(error);
    } finally {
      saving = false;
    }
  }

  function selectPreset(id: string) {
    activePresetId = id;
    error = null;
    doneMessage = null;
    resolvedOffers = [];
  }

  async function createPreset() {
    const name = newPresetName.trim();
    if (!name) return;
    const preset: Preset = { id: uid(), name, createdAt: String(Date.now()), mods: [] };
    newPresetName = "";
    await persist([...presets, preset]);
    activePresetId = preset.id;
  }

  async function deletePreset(id: string) {
    const next = presets.filter((p) => p.id !== id);
    await persist(next);
    if (activePresetId === id) activePresetId = next[0]?.id ?? null;
    resolvedOffers = [];
  }

  async function renamePreset() {
    if (!activePreset) return;
    const name = renameDraft.trim();
    renameDraft = "";
    if (!name || name === activePreset.name) return;
    await persist(
      presets.map((p) => (p.id === activePreset.id ? { ...p, name } : p)),
    );
  }

  async function removeMod(entry: PresetMod) {
    if (!activePreset) return;
    await persist(
      presets.map((p) =>
        p.id === activePreset.id
          ? {
              ...p,
              mods: p.mods.filter(
                (m) => !(m.provider === entry.provider && m.projectId === entry.projectId),
              ),
            }
          : p,
      ),
    );
    resolvedOffers = resolvedOffers.filter(
      (o) => !(o.provider === entry.provider && o.projectId === entry.projectId),
    );
  }

  let searchSeq = 0;

  function queueSearch() {
    if (searchTimer) clearTimeout(searchTimer);
    const q = searchQuery.trim();
    if (!q) {
      searchResults = [];
      searchError = null;
      return;
    }
    searchTimer = setTimeout(() => void runSearch(q), 350);
  }

  async function runSearch(q: string) {
    const seq = ++searchSeq;
    searching = true;
    searchError = null;
    const path = get(projectPath);
    try {
      if (searchProvider === "modrinth") {
        const res = await api.mods.search(q, { contentType: "mod", p: path ?? "" });
        if (seq !== searchSeq) return;
        searchResults = (res.results ?? []).map((r) => ({
          id: r.id,
          slug: r.slug,
          name: r.name || r.slug,
          provider: "modrinth",
        }));
      } else {
        const res = await api.mods.searchCurseforge(q, { contentType: "mod", p: path ?? "" });
        if (seq !== searchSeq) return;
        searchResults = (res.results ?? []).map((r) => ({
          id: r.id,
          slug: r.slug,
          name: r.name || r.slug,
          provider: "curseforge",
        }));
      }
    } catch (e) {
      if (seq !== searchSeq) return;
      searchResults = [];
      searchError = e instanceof Error ? e.message : String(e);
    } finally {
      if (seq === searchSeq) searching = false;
    }
  }

  function alreadyInPreset(row: SearchResultRow): boolean {
    return (
      activePreset?.mods.some(
        (m) => m.provider === row.provider && m.projectId === row.id,
      ) ?? false
    );
  }

  async function addToPreset(row: SearchResultRow) {
    if (!activePreset) return;
    await persist(
      presets.map((p) =>
        p.id === activePreset.id
          ? {
              ...p,
              mods: [
                ...p.mods,
                {
                  provider: row.provider,
                  projectId: row.id,
                  slug: row.slug,
                  name: row.name,
                },
              ],
            }
          : p,
      ),
    );
    searchResults = searchResults.filter(
      (r) => !(r.provider === row.provider && r.id === row.id),
    );
  }

  async function resolveActive() {
    const path = get(projectPath);
    const preset = activePreset;
    if (!path || !preset || resolving) return;
    resolving = true;
    error = null;
    try {
      const offers: ResolvedOffer[] = [];
      // Resolve sequentially to be gentle on Modrinth / CurseForge rate limits.
      for (const m of preset.mods) {
        try {
          const offer = await api.mods.resolvePresetMod(m, path);
          offers.push(offer);
        } catch (e) {
          offers.push({
            slug: m.slug,
            name: m.name || m.slug,
            provider: m.provider,
            projectId: m.projectId,
            reason: "",
            risk: "unknown",
            alreadyInstalled: false,
            resolveError: e instanceof Error ? e.message : String(e),
          });
        }
      }
      resolvedOffers = offers;
    } finally {
      resolving = false;
    }
  }

  async function installPreset() {
    const path = get(projectPath);
    const preset = activePreset;
    if (!path || !preset || installing) return;
    installing = true;
    error = null;
    doneMessage = null;
    try {
      // Resolve against the current instance (works for any MC version / loader).
      const offers = [...resolvedOffers];
      if (offers.length !== preset.mods.length) {
        await resolveActive();
        offers.splice(0, offers.length, ...resolvedOffers);
      }
      const installable = offers.filter((o) => !o.resolveError && !o.alreadyInstalled);
      const errors: string[] = [];
      for (const offer of installable) {
        try {
          if (offer.provider === "curseforge") {
            await api.mods.addCurseforge(offer.projectId, "both", path);
          } else {
            await api.mods.addWithDeps(offer.projectId, "both", path);
          }
        } catch (e) {
          errors.push(`${offer.name}: ${e instanceof Error ? e.message : String(e)}`);
        }
      }
      const failedResolution = offers.filter((o) => o.resolveError);
      for (const f of failedResolution) {
        errors.push(`${f.name}: ${f.resolveError}`);
      }
      if (errors.length) {
        error = errors.join("; ");
        toasts.error("Preset install finished with errors");
      } else {
        doneMessage = `Installed ${installable.length} mod${installable.length === 1 ? "" : "s"} from "${preset.name}".`;
        toasts.success("Preset installed");
      }
      onInstalled?.();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      toasts.error(error);
    } finally {
      installing = false;
    }
  }

  // One-shot open: refresh presets each time the dialog opens.
  let sessionOpen = $state(false);
  $effect(() => {
    if (!open) {
      sessionOpen = false;
      return;
    }
    if (sessionOpen) return;
    sessionOpen = true;
    void refresh();
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
      class="modal presets-dialog"
      role="dialog"
      aria-modal="true"
      use:trapFocus={{ onEscape: close }}
    >
      <div class="modal-header">
        <div>
          <h2><Package size={18} /> Mod presets</h2>
          <p>
            Build your own mod set from Modrinth and CurseForge. Install it into any
            instance with one click — versions resolve automatically for its Minecraft version.
          </p>
        </div>
        <button class="icon-btn" type="button" onclick={close} aria-label="Close"><X size={18} /></button>
      </div>

      <div class="presets-layout">
        <aside class="presets-sidebar">
          <form
            class="preset-create"
            onsubmit={(e) => { e.preventDefault(); void createPreset(); }}
          >
            <input
              type="text"
              placeholder="New preset name"
              bind:value={newPresetName}
              maxlength={60}
            />
            <button type="submit" disabled={!newPresetName.trim() || saving} aria-label="Create preset">
              <Plus size={14} />
            </button>
          </form>

          {#if loading}
            <div class="side-status"><Loader2 size={14} class="spin" /> Loading…</div>
          {:else if !presets.length}
            <p class="side-empty">No presets yet — create one above.</p>
          {:else}
            <div class="preset-list">
              {#each presets as p (p.id)}
                <button
                  type="button"
                  class="preset-item"
                  class:active={p.id === activePresetId}
                  onclick={() => selectPreset(p.id)}
                >
                  <span class="preset-name">{p.name}</span>
                  <span class="preset-count">{p.mods.length}</span>
                </button>
              {/each}
            </div>
          {/if}
        </aside>

        <section class="presets-main">
          {#if doneMessage}
            <div class="opt-done"><Check size={14} /> {doneMessage}</div>
          {/if}
          {#if error}
            <div class="opt-error">{error}</div>
          {/if}

          {#if !activePreset}
            <div class="main-empty">
              <p>Select or create a preset to manage its mods.</p>
            </div>
          {:else}
            <div class="preset-head">
              <input
                class="rename-input"
                type="text"
                value={activePreset.name}
                onchange={(e) => { renameDraft = (e.currentTarget as HTMLInputElement).value; void renamePreset(); }}
                maxlength={60}
                aria-label="Preset name"
              />
              <button
                class="ghost danger"
                type="button"
                onclick={() => void deletePreset(activePreset.id)}
                disabled={saving || installing}
                aria-label="Delete preset"
              >
                <Trash2 size={14} />
              </button>
            </div>

            <div class="search-box">
              <div class="search-tabs" role="tablist">
                <button
                  type="button"
                  class:active={searchProvider === "modrinth"}
                  onclick={() => { searchProvider = "modrinth"; searchResults = []; queueSearch(); }}
                >Modrinth</button>
                <button
                  type="button"
                  class:active={searchProvider === "curseforge"}
                  onclick={() => { searchProvider = "curseforge"; searchResults = []; queueSearch(); }}
                >CurseForge</button>
              </div>
              <div class="search-input-wrap">
                <Search size={14} />
                <input
                  type="text"
                  placeholder={`Search ${searchProvider === "modrinth" ? "Modrinth" : "CurseForge"} mods…`}
                  bind:value={searchQuery}
                  oninput={() => queueSearch()}
                  disabled={searching}
                />
                {#if searching}<Loader2 size={14} class="spin" />{/if}
              </div>
              {#if searchError}
                <p class="search-error">{searchError}</p>
              {/if}
              {#if searchResults.length}
                <div class="search-results">
                  {#each searchResults as row (`${row.provider}:${row.id}`)}
                    <div class="search-row">
                      <div class="row-meta">
                        <strong>{row.name}</strong>
                        <code>{row.slug}</code>
                      </div>
                      <span class="pill">{row.provider}</span>
                      <button
                        type="button"
                        class="secondary sm"
                        disabled={saving || alreadyInPreset(row)}
                        onclick={() => void addToPreset(row)}
                      >
                        {alreadyInPreset(row) ? "Added" : "Add"}
                      </button>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>

            <div class="mods-head">
              <h3>
                Mods in preset
                {#if activePreset.mods.length}
                  <span class="count">{activePreset.mods.length} total</span>
                {/if}
              </h3>
              <button
                type="button"
                class="ghost sm"
                disabled={resolving || installing || !activePreset.mods.length}
                onclick={() => void resolveActive()}
              >
                {#if resolving}<Loader2 size={12} class="spin" />{/if}
                Check versions
              </button>
            </div>

            {#if !activePreset.mods.length}
              <p class="mods-empty">
                No mods yet — search above and add projects. Install works on any Minecraft
                version: TuffBox picks the matching release for the target instance.
              </p>
            {:else}
              <div class="mod-list">
                {#each activePreset.mods as m (`${m.provider}:${m.projectId}`)}
                  {@const offer = resolvedForActive.get(`${m.provider}:${m.projectId}`)}
                  <div class="mod-row">
                    <div class="row-meta">
                      <strong>{m.name || m.slug || m.projectId}</strong>
                      <code>{m.slug}</code>
                      {#if offer?.resolveError}
                        <span class="row-fail">{offer.resolveError}</span>
                      {:else if offer?.alreadyInstalled}
                        <span class="row-ok">already installed</span>
                      {:else if offer}
                        <span class="row-ok">{offer.name} — will install</span>
                      {/if}
                    </div>
                    <span class="pill">{m.provider}</span>
                    <button
                      type="button"
                      class="icon-btn"
                      onclick={() => void removeMod(m)}
                      disabled={saving || installing}
                      aria-label={`Remove ${m.name}`}
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
          {/if}
        </section>
      </div>

      <div class="opt-footer">
        <button class="ghost" type="button" onclick={close} disabled={installing}>Close</button>
        <button
          type="button"
          disabled={installDisabled}
          onclick={() => void installPreset()}
        >
          {#if installing}
            <Loader2 size={14} class="spin" /> Installing…
          {:else}
            <Download size={14} /> {installLabel}
          {/if}
        </button>
      </div>
    </div>
  </div>
{/if}

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

  .presets-dialog {
    width: min(860px, 100%);
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
    width: 30px;
    height: 30px;
    padding: 0;
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    color: var(--text-muted);
  }

  .icon-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-secondary);
  }

  .presets-layout {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: 14px;
  }

  .presets-sidebar {
    width: 200px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
  }

  .preset-create {
    display: flex;
    gap: 8px;
  }

  .preset-create input {
    flex: 1;
    min-width: 0;
  }

  .preset-list {
    flex: 1;
    min-height: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .preset-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    cursor: pointer;
    text-align: left;
  }

  .preset-item.active {
    color: var(--text-primary);
    border-color: var(--accent, #6ee7b7);
    background: color-mix(in srgb, var(--accent, #6ee7b7) 12%, var(--bg-tertiary));
  }

  .preset-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
  }

  .preset-count {
    font-size: 12px;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .side-status,
  .side-empty {
    font-size: 12px;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .presets-main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    overflow: auto;
    overscroll-behavior: contain;
    scrollbar-gutter: stable;
    padding-right: 2px;
  }

  .main-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    font-size: 13px;
  }

  .preset-head {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .rename-input {
    flex: 1;
    min-width: 0;
    font-size: 14px;
    font-weight: 600;
  }

  .ghost.danger {
    color: #fca5a5;
  }

  .search-box {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
  }

  .search-tabs {
    display: flex;
    gap: 8px;
  }

  .search-tabs button {
    padding: 5px 12px;
    font-size: 12px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }

  .search-tabs button.active {
    color: var(--text-primary);
    border-color: var(--accent, #6ee7b7);
    background: color-mix(in srgb, var(--accent, #6ee7b7) 12%, transparent);
  }

  .search-input-wrap {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-muted);
  }

  .search-input-wrap input {
    flex: 1;
    min-width: 0;
  }

  .search-error {
    margin: 0;
    font-size: 12px;
    color: #fca5a5;
  }

  .search-results {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 160px;
    overflow: auto;
  }

  .search-row,
  .mod-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
  }

  .row-meta {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .row-meta strong {
    font-size: 13px;
  }

  .row-meta code {
    font-size: 12px;
    color: var(--text-muted);
  }

  .row-ok {
    font-size: 12px;
    color: #86efac;
  }

  .row-fail {
    font-size: 12px;
    color: #fca5a5;
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

  .mods-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }

  .mods-head h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .mods-head .count {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-muted);
  }

  .mods-empty {
    margin: 0;
    font-size: 13px;
    color: var(--text-muted);
    line-height: 1.45;
  }

  .mod-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  button.sm {
    padding: 5px 10px;
    font-size: 12px;
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

  .opt-done,
  .opt-error {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    font-size: 13px;
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
    word-break: break-word;
  }

  :global(.spin) {
    animation: presets-spin 0.8s linear infinite;
  }

  @keyframes presets-spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
