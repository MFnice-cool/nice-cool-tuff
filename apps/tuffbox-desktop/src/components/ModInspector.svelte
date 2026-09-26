<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { X, Loader2, ExternalLink } from "@lucide/svelte";
  import { fly } from "svelte/transition";
  import { quintOut } from "svelte/easing";

  export type InspectorMod = {
    id: string;
    name: string;
    version: string;
    source: string;
    projectId?: string | null;
    iconUrl?: string | null;
    contentType?: string | null;
    fileName?: string | null;
    side?: string | null;
    disabled?: boolean;
    /** Optional "why this is recommended" note from the mod picker / repair flow. */
    recommendedReason?: string | null;
  };

  type CatalogDetail = {
    id: string;
    slug?: string;
    name: string;
    description?: string;
    authors?: string[];
    author?: string | null;
    license?: string | null;
    issuesUrl?: string | null;
    sourceUrl?: string | null;
    iconUrl?: string | null;
    provider?: string;
  };

  let {
    mod,
    onclose,
    onopenlink,
    ontoggleDisabled,
    onopenversions,
  }: {
    mod: InspectorMod;
    onclose: () => void;
    onopenlink?: (url: string) => void;
    /** Toggle disable/enable (jar `.disabled` rename + manifest status). */
    ontoggleDisabled?: (mod: InspectorMod) => void;
    /** Open the version picker for this mod. */
    onopenversions?: (mod: InspectorMod) => void;
  } = $props();

  let detail = $state<CatalogDetail | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  const provider = $derived.by(() => {
    const s = (mod.source || "").toLowerCase();
    if (s === "curseforge" || s === "cf") return "curseforge" as const;
    if (s === "modrinth") return "modrinth" as const;
    return null;
  });

  const projectId = $derived((mod.projectId || (provider ? mod.id : "") || "").trim());

  const displayName = $derived(detail?.name?.trim() || mod.name);

  const sideLabel = $derived.by(() => {
    const s = (mod.side || "").toLowerCase();
    switch (s) {
      case "client": return "Client";
      case "server": return "Server";
      case "both": return "Both sides";
      case "optional": return "Optional";
      default: return s ? s : "Side unknown";
    }
  });

  const authors = $derived.by(() => {
    const fromDetail = (detail?.authors ?? []).map((a) => a.trim()).filter(Boolean);
    if (fromDetail.length) return fromDetail;
    const single = detail?.author?.trim();
    if (single) return [single];
    return [] as string[];
  });

  const description = $derived(
    (detail?.description ?? "").trim() || "No description available for this project.",
  );

  const license = $derived((detail?.license ?? "").trim() || null);

  const issuesUrl = $derived.by(() => {
    const raw = (detail?.issuesUrl || detail?.sourceUrl || "").trim();
    return raw || null;
  });

  const issuesLabel = $derived.by(() => {
    if (!issuesUrl) return null;
    try {
      const host = new URL(issuesUrl).hostname.replace(/^www\./, "");
      if (host.includes("github")) return "GitHub";
      if (host.includes("gitlab")) return "GitLab";
      if (host.includes("modrinth")) return "Modrinth";
      if (host.includes("curseforge")) return "CurseForge";
      return host;
    } catch {
      return "tracker";
    }
  });

  function prefersReducedMotion(): boolean {
    if (typeof document === "undefined") return true;
    if (document.documentElement.classList.contains("potato-pc")) return true;
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  function panelIntro(node: Element) {
    if (prefersReducedMotion()) return { duration: 0 };
    return fly(node, { x: 24, duration: 240, opacity: 0, easing: quintOut });
  }

  function localFallback(): CatalogDetail {
    return {
      id: mod.id,
      name: mod.name,
      description: mod.fileName
        ? `Local file: ${mod.fileName}`
        : "This item is not linked to Modrinth or CurseForge, so catalog details are unavailable.",
      authors: [],
      license: null,
      issuesUrl: null,
      iconUrl: mod.iconUrl ?? null,
    };
  }

  onMount(() => {
    const prov = provider;
    const id = projectId;
    if (!prov || !id) {
      detail = localFallback();
      loading = false;
      return;
    }

    let cancelled = false;
    void invoke<CatalogDetail>("get_catalog_project", {
      provider: prov,
      projectId: id,
    })
      .then((result) => {
        if (!cancelled) detail = result;
      })
      .catch((e) => {
        if (cancelled) return;
        error = String(e);
        detail = {
          id,
          name: mod.name,
          description: "",
          authors: [],
          license: null,
          issuesUrl: null,
        };
      })
      .finally(() => {
        if (!cancelled) loading = false;
      });

    return () => {
      cancelled = true;
    };
  });

  function openIssues(e: MouseEvent) {
    e.preventDefault();
    if (!issuesUrl) return;
    onopenlink?.(issuesUrl);
  }
</script>

<aside class="mod-inspector" aria-label="Mod details" in:panelIntro>
  <header class="inspector-header">
    <div class="inspector-heading">
      {#if mod.iconUrl || detail?.iconUrl}
        <img class="inspector-icon" src={detail?.iconUrl || mod.iconUrl || ""} alt="" />
      {/if}
      <div class="inspector-titles">
        <h2 class="inspector-name" title={displayName}>{displayName}</h2>
        {#if authors.length > 0}
          <p class="inspector-authors">by {authors.join(", ")}</p>
        {:else if loading}
          <p class="inspector-authors muted">Loading authors…</p>
        {/if}
      </div>
    </div>
    <button type="button" class="icon-btn" onclick={onclose} title="Close inspector" aria-label="Close inspector">
      <X size={18} />
    </button>
  </header>

  <div class="inspector-body">
    {#if loading}
      <div class="inspector-loading">
        <Loader2 size={18} class="spin" />
        <span>Loading details…</span>
      </div>
    {:else}
      {#if error}
        <p class="inspector-error">{error}</p>
      {/if}

      <section class="inspector-section">
        <p class="inspector-description">{description}</p>
      </section>

      <section class="inspector-section facts">
        {#if mod.side}
          <span class="fact-badge" title="Side">Side: {sideLabel}</span>
        {/if}
        {#if mod.contentType}
          <span class="fact-badge" title="Content type">{mod.contentType}</span>
        {/if}
        {#if mod.fileName}
          <span class="fact-badge mono" title="File name">{mod.fileName}</span>
        {/if}
      </section>

      {#if mod.recommendedReason}
        <section class="inspector-section recommended">
          <span class="recommended-label">Why recommended</span>
          <p>{mod.recommendedReason}</p>
        </section>
      {/if}

      {#if ontoggleDisabled || onopenversions}
        <section class="inspector-section actions">
          {#if ontoggleDisabled}
            <button
              type="button"
              class="inspector-action"
              class:disabled={mod.disabled}
              onclick={() => ontoggleDisabled?.(mod)}
            >
              {mod.disabled ? "Enable mod" : "Disable mod"}
            </button>
          {/if}
          {#if onopenversions}
            <button type="button" class="inspector-action" onclick={() => onopenversions?.(mod)}>
              Open versions
            </button>
          {/if}
        </section>
      {/if}

      {#if issuesUrl}
        <section class="inspector-section links">
          <span class="links-label">Report issues at:</span>
          <a
            class="issues-link"
            href={issuesUrl}
            target="_blank"
            rel="noopener noreferrer"
            onclick={openIssues}
          >
            {issuesLabel}
            <ExternalLink size={13} />
          </a>
        </section>
      {/if}
    {/if}
  </div>

  <footer class="inspector-footer">
    {#if license}
      <span class="license-badge" title="License">License: {license}</span>
    {:else if !loading}
      <span class="license-muted">License unknown</span>
    {/if}
    <span class="meta-muted">{mod.version}{#if mod.source} · {mod.source}{/if}</span>
  </footer>
</aside>

<style>
  .mod-inspector {
    display: flex;
    flex-direction: column;
    width: min(340px, 38vw);
    min-width: 260px;
    max-width: 380px;
    height: 100%;
    min-height: 0;
    background: var(--bg-elevated, var(--bg-tertiary));
    border-left: 1px solid color-mix(in srgb, var(--border-color) 70%, transparent);
    padding: 16px 16px 14px;
    gap: 14px;
    flex-shrink: 0;
  }

  .inspector-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
    flex-shrink: 0;
  }

  .inspector-heading {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    min-width: 0;
    flex: 1;
  }

  .inspector-icon {
    width: 44px;
    height: 44px;
    border-radius: var(--border-radius-sm);
    object-fit: cover;
    flex-shrink: 0;
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
  }

  .inspector-titles {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .inspector-name {
    margin: 0;
    font-size: 1.15rem;
    font-weight: 700;
    line-height: 1.25;
    color: var(--text-primary);
    overflow-wrap: anywhere;
  }

  .inspector-authors {
    margin: 0;
    font-size: 0.82rem;
    line-height: 1.35;
    color: var(--text-muted);
  }

  .inspector-authors.muted {
    font-style: italic;
  }

  .inspector-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding-right: 2px;
  }

  .inspector-loading {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-secondary);
    font-size: 0.85rem;
    padding: 8px 0;
  }

  .inspector-error {
    margin: 0;
    font-size: 0.78rem;
    color: var(--accent-danger, #f87171);
    line-height: 1.4;
  }

  .inspector-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .inspector-description {
    margin: 0;
    font-size: 0.9rem;
    line-height: 1.65;
    color: var(--text-secondary);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .inspector-section.facts {
    flex-direction: row;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .fact-badge {
    display: inline-flex;
    align-items: center;
    font-size: 0.72rem;
    font-weight: 700;
    letter-spacing: 0.02em;
    padding: 3px 8px;
    border-radius: 999px;
    color: var(--text-secondary);
    background: color-mix(in srgb, var(--bg-secondary) 80%, transparent);
    border: 1px solid color-mix(in srgb, var(--border-color) 80%, transparent);
  }
  .fact-badge.mono {
    font-family: var(--font-mono, ui-monospace, monospace);
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .inspector-section.actions {
    flex-direction: row;
    flex-wrap: wrap;
    gap: 8px;
  }

  .inspector-action {
    font-size: 0.82rem;
    font-weight: 600;
    padding: 6px 12px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    color: var(--text-primary);
    cursor: pointer;
  }
  .inspector-action:hover {
    border-color: var(--accent-primary);
  }
  .inspector-action.disabled {
    color: var(--accent-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 55%, transparent);
  }

  .inspector-section.recommended {
    padding: 10px 12px;
    border-radius: 10px;
    border: 1px solid color-mix(in srgb, var(--accent-secondary) 30%, transparent);
    background: color-mix(in srgb, var(--accent-secondary) 4%, transparent);
  }
  .recommended-label {
    font-size: 0.72rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--accent-secondary);
  }
  .recommended p {
    margin: 4px 0 0;
    font-size: 0.85rem;
    line-height: 1.5;
    color: var(--text-secondary);
  }

  .inspector-section.links {
    flex-direction: row;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 8px;
    padding-top: 4px;
  }

  .links-label {
    font-size: 0.82rem;
    color: var(--text-muted);
  }

  .issues-link {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--accent-primary);
    text-decoration: none;
  }

  .issues-link:hover {
    text-decoration: underline;
    color: var(--accent-hover, var(--accent-primary));
  }

  .inspector-footer {
    flex-shrink: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding-top: 10px;
    border-top: 1px solid color-mix(in srgb, var(--border-color) 55%, transparent);
  }

  .license-badge {
    display: inline-flex;
    align-items: center;
    font-size: 0.72rem;
    font-weight: 700;
    letter-spacing: 0.02em;
    padding: 3px 8px;
    border-radius: 999px;
    color: var(--text-secondary);
    background: color-mix(in srgb, var(--bg-secondary) 80%, transparent);
    border: 1px solid color-mix(in srgb, var(--border-color) 80%, transparent);
  }

  .license-muted,
  .meta-muted {
    font-size: 0.72rem;
    color: var(--text-muted);
  }

  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    flex-shrink: 0;
  }

  .icon-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  :global(.spin) {
    animation: inspector-spin 0.9s linear infinite;
  }

  @keyframes inspector-spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
