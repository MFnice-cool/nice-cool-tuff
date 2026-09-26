<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open as openExternal } from "@tauri-apps/plugin-shell";
  import {
    ArrowLeft,
    Download,
    ExternalLink,
    Heart,
    Bookmark,
    Clock,
    Loader2,
    Package,
    Bug,
    Code2,
    BookOpen,
    MessageCircle,
    HandCoins,
    Users,
    Scale,
    Wrench,
    ImageOff,
  } from "@lucide/svelte";
  import { fly } from "svelte/transition";
  import { quintOut } from "svelte/easing";
  import { sanitizeHtml } from "../lib/sanitizeHtml";
  import CopyButton from "./CopyButton.svelte";

  let {
    result,
    minecraftVersion = null,
    loaderKind = null,
    installed = false,
    installing = false,
    onback,
    oninstall,
    onopenexternal,
  }: {
    result: {
      id: string;
      slug: string;
      name: string;
      description: string;
      projectType: string;
      iconUrl?: string | null;
      author?: string | null;
      downloads?: number | null;
      follows?: number | null;
      dateModified?: string | null;
      categories?: string[];
      provider?: string;
    };
    minecraftVersion?: string | null;
    loaderKind?: string | null;
    installed?: boolean;
    installing?: boolean;
    onback?: () => void;
    oninstall?: () => void;
    onopenexternal?: () => void;
  } = $props();

  type GalleryImage = { url: string; title?: string | null; description?: string | null };
  type Creator = { username: string; role?: string; avatarUrl?: string | null };

  type CatalogDetail = typeof result & {
    descriptionHtml?: string | null;
    authors?: string[];
    license?: string | null;
    clientSide?: string | null;
    serverSide?: string | null;
    issuesUrl?: string | null;
    sourceUrl?: string | null;
    wikiUrl?: string | null;
    discordUrl?: string | null;
    donateUrl?: string | null;
    loaders?: string[];
    gameVersions?: string[];
    dateCreated?: string | null;
    gallery?: GalleryImage[];
    creators?: Creator[];
  };

  type CatalogVersion = {
    id: string;
    versionNumber: string;
    name?: string | null;
    gameVersions: string[];
    loaders: string[];
    datePublished?: string | null;
    versionType?: string;
    changelog?: string | null;
    compatible?: boolean;
  };

  // result is a one-shot initial prop — snapshot once, then edit the copy.
  let detail: CatalogDetail = $state(initialDetail());

  function initialDetail(): CatalogDetail {
    return { ...result };
  }
  let loading = $state(true);
  let versionsLoading = $state(false);
  let versions = $state<CatalogVersion[]>([]);
  let tab: "overview" | "gallery" | "changelog" | "versions" = $state("overview");
  let showIncompatible = $state(false);
  let error: string | null = $state(null);
  let liked = $state(false);
  let saved = $state(false);

  const provider = $derived((result.provider ?? "modrinth").toLowerCase() === "curseforge" ? "curseforge" : "modrinth");
  const compatibleVersions = $derived(versions.filter((v) => v.compatible !== false));
  const shownVersions = $derived(showIncompatible ? versions : compatibleVersions);
  const gallery = $derived(detail.gallery ?? []);
  const creators = $derived(
    detail.creators?.length ? detail.creators : detail.author ? [{ username: detail.author, role: "Owner" }] : [],
  );
  // Major Minecraft lines for the Compatibility block (1.21.x, 1.20.x, …).
  const mcLines = $derived.by(() => {
    const set = new Set<string>();
    for (const v of detail.gameVersions ?? []) {
      const m = /^1\.\d+/.exec(v.trim());
      if (m) set.add(m[0] + ".x");
      else set.add(v.trim());
    }
    return [...set];
  });
  const loaders = $derived((detail.loaders ?? []).map((l) => l.charAt(0).toUpperCase() + l.slice(1)));
  // Changelog entries: versions that carry a non-empty changelog, newest first.
  const changelogEntries = $derived(
    versions.filter((v) => (v.changelog ?? "").trim().length > 0),
  );

  function formatCount(n: number | null | undefined): string {
    if (n == null) return "0";
    if (n >= 1_000_000) return (n / 1_000_000).toFixed(2) + "M";
    if (n >= 1_000) return (n / 1_000).toFixed(1) + "K";
    return String(n);
  }

  function formatRelative(iso: string | null | undefined): string {
    if (!iso) return "—";
    const then = new Date(iso).getTime();
    if (Number.isNaN(then)) return iso.slice(0, 10);
    const days = Math.floor((Date.now() - then) / 86_400_000);
    if (days < 1) return "today";
    if (days === 1) return "1 day ago";
    if (days < 30) return `${days} days ago`;
    const months = Math.floor(days / 30);
    if (months < 12) return `${months} mo ago`;
    return `${Math.floor(months / 12)}y ago`;
  }

  function formatDate(iso: string | null | undefined): string {
    if (!iso) return "—";
    try {
      return new Date(iso).toLocaleDateString(undefined, {
        year: "numeric",
        month: "short",
        day: "numeric",
      });
    } catch {
      return iso.slice(0, 10);
    }
  }

  function favoriteKey(): string {
    return `tuffbox:catalog:favorite:${provider}:${result.id}`;
  }

  function bookmarkKey(): string {
    return `tuffbox:catalog:bookmark:${provider}:${result.id}`;
  }

  function loadUserState() {
    try {
      liked = localStorage.getItem(favoriteKey()) === "1";
      saved = localStorage.getItem(bookmarkKey()) === "1";
    } catch {
      liked = false;
      saved = false;
    }
  }

  function toggleLiked() {
    liked = !liked;
    try {
      if (liked) localStorage.setItem(favoriteKey(), "1");
      else localStorage.removeItem(favoriteKey());
    } catch {
      /* storage unavailable — in-memory only */
    }
  }

  function toggleSaved() {
    saved = !saved;
    try {
      if (saved) localStorage.setItem(bookmarkKey(), "1");
      else localStorage.removeItem(bookmarkKey());
    } catch {
      /* storage unavailable — in-memory only */
    }
  }

  function external(url: string | null | undefined) {
    if (!url) return;
    if (onopenexternal && url === projectUrl()) {
      onopenexternal();
      return;
    }
    void openExternal(url);
  }

  function projectUrl(): string {
    const host = provider === "curseforge" ? "https://www.curseforge.com" : "https://modrinth.com";
    const seg = provider === "curseforge"
      ? { modpack: "modpacks", mod: "mc-mods", resourcepack: "texture-packs", shader: "shaders", datapack: "data-packs" }[detail.projectType] ?? "mc-mods"
      : { modpack: "modpack", mod: "mod", resourcepack: "resourcepack", shader: "shader", datapack: "datapack" }[detail.projectType] ?? "mod";
    return `${host}/${seg}/${detail.slug || detail.id}`;
  }

  const embedCode = $derived(
    provider === "modrinth"
      ? `<iframe src="https://modrinth.com/mod/${detail.slug}/embed" title="${detail.name}" style="width:640px;height:100px;border:0"></iframe>`
      : "",
  );
  const packwizCommand = $derived(
    provider === "modrinth" ? `packwiz modrinth install ${detail.slug}` : "",
  );

  async function loadDetail() {
    loading = true;
    error = null;
    try {
      detail = await invoke<CatalogDetail>("get_catalog_project", {
        provider,
        projectId: result.id,
      });
    } catch (e) {
      error = String(e);
      detail = { ...result };
    } finally {
      loading = false;
    }
  }

  async function loadVersions() {
    versionsLoading = true;
    try {
      versions = await invoke<CatalogVersion[]>("get_catalog_versions", {
        provider,
        projectId: result.id,
        minecraftVersion: minecraftVersion || null,
        loader: loaderKind || null,
      });
    } catch {
      versions = [];
    } finally {
      versionsLoading = false;
    }
  }

  onMount(() => {
    loadUserState();
    void loadDetail();
    void loadVersions();
  });
</script>

<div class="catalog-page" transition:fly={{ x: 28, duration: 320, opacity: 0, easing: quintOut }}>
  <header class="page-head">
    <button type="button" class="back" onclick={() => onback?.()}>
      <ArrowLeft size={16} /> Back to search
    </button>
    <div class="head-actions">
      <button
        type="button"
        class="icon-action"
        class:active={liked}
        title={liked ? "Remove from favorites" : "Add to favorites"}
        onclick={toggleLiked}
      >
        <Heart size={16} />
      </button>
      <button
        type="button"
        class="icon-action"
        class:active={saved}
        title={saved ? "Remove bookmark" : "Bookmark"}
        onclick={toggleSaved}
      >
        <Bookmark size={16} />
      </button>
      <button type="button" class="ghost" onclick={() => external(projectUrl())}>
        <ExternalLink size={15} />
        Open on {provider === "curseforge" ? "CurseForge" : "Modrinth"}
      </button>
      <button
        type="button"
        class="primary"
        disabled={installing || installed}
        onclick={() => oninstall?.()}
      >
        <Download size={15} />
        {installed ? "Installed" : installing ? "Installing…" : "Install"}
      </button>
    </div>
  </header>

  <div class="hero">
    <div class="hero-icon">
      {#if detail.iconUrl}
        <img src={detail.iconUrl} alt="" />
      {:else}
        <span>{(detail.name?.[0] ?? "?").toUpperCase()}</span>
      {/if}
    </div>
    <div class="hero-body">
      <div class="eyebrow">
        <span class="provider" class:cf={provider === "curseforge"}>{provider === "curseforge" ? "CurseForge" : "Modrinth"}</span>
        <span class="type">{detail.projectType || "mod"}</span>
      </div>
      <h1>{detail.name}</h1>
      {#if detail.author || (detail.authors && detail.authors.length)}
        <p class="author">by {(detail.authors && detail.authors[0]) || detail.author}</p>
      {/if}
      <div class="stats">
        <span><Download size={13} /> {formatCount(detail.downloads)}</span>
        {#if provider !== "curseforge"}
          <span><Heart size={13} /> {formatCount(detail.follows)}</span>
        {/if}
        <span><Clock size={13} /> {formatRelative(detail.dateModified)}</span>
      </div>
    </div>
  </div>

  <div class="content-grid">
    <div class="main-col">
      <div class="tabs" role="tablist">
        <button type="button" class:active={tab === "overview"} onclick={() => (tab = "overview")}>Overview</button>
        <button type="button" class:active={tab === "gallery"} onclick={() => (tab = "gallery")} disabled={provider === "curseforge"}>
          Gallery{#if gallery.length} ({gallery.length}){/if}
        </button>
        <button type="button" class:active={tab === "changelog"} onclick={() => (tab = "changelog")}>
          Changelog{#if changelogEntries.length} ({changelogEntries.length}){/if}
        </button>
        <button type="button" class:active={tab === "versions"} onclick={() => (tab = "versions")}>
          Versions{#if versions.length}
            ({compatibleVersions.length}{#if compatibleVersions.length !== versions.length}/{versions.length}{/if})
          {/if}
        </button>
      </div>

      {#if loading}
        <div class="loading"><Loader2 size={18} class="spin" /> Loading project…</div>
      {:else if error}
        <div class="notice">{error}</div>
      {:else if tab === "overview"}
        <section class="overview">
          {#if detail.descriptionHtml}
            <div class="html-body">{@html sanitizeHtml(detail.descriptionHtml)}</div>
          {:else}
            <p class="plain">{detail.description || "No description."}</p>
          {/if}
        </section>
      {:else if tab === "gallery"}
        <section class="gallery">
          {#if gallery.length === 0}
            <div class="empty"><ImageOff size={18} /> No gallery images.</div>
          {:else}
            {#each gallery as img, i (img.url + i)}
              <figure>
                <a href={img.url} target="_blank" rel="noreferrer" onclick={(e) => { e.preventDefault(); external(img.url); }}>
                  <img src={img.url} alt={img.title ?? ""} loading="lazy" />
                </a>
                {#if img.title || img.description}
                  <figcaption>{img.title || img.description}</figcaption>
                {/if}
              </figure>
            {/each}
          {/if}
        </section>
      {:else if tab === "changelog"}
        <section class="changelog">
          {#if changelogEntries.length === 0}
            <div class="empty"><Clock size={18} /> No changelog entries published.</div>
          {:else}
            {#each changelogEntries.slice(0, 25) as v (v.id)}
              <article>
                <header>
                  <strong>{v.versionNumber || v.name || v.id}</strong>
                  <small>{formatDate(v.datePublished)}</small>
                </header>
                <div class="html-body">{@html sanitizeHtml(v.changelog ?? "")}</div>
              </article>
            {/each}
          {/if}
        </section>
      {:else}
        <section class="versions">
          <div class="versions-toolbar">
            <label>
              <input type="checkbox" bind:checked={showIncompatible} />
              Show incompatible
            </label>
            {#if versionsLoading}<span class="muted"><Loader2 size={13} class="spin" /> Loading…</span>{/if}
          </div>
          {#if shownVersions.length === 0}
            <div class="empty"><Package size={18} /> No versions matched this instance.</div>
          {:else}
            <ul>
              {#each shownVersions.slice(0, 40) as v (v.id)}
                <li class:incompat={v.compatible === false}>
                  <div>
                    <strong>{v.versionNumber || v.name || v.id}</strong>
                    <small>
                      {(v.versionType ?? "release")}
                      {#if v.loaders?.length} · {v.loaders.slice(0, 3).join(", ")}{/if}
                      {#if v.gameVersions?.length} · MC {v.gameVersions.slice(0, 4).join(", ")}{/if}
                      {#if v.datePublished} · {formatDate(v.datePublished)}{/if}
                    </small>
                  </div>
                  {#if v.compatible === false}<span class="badge">incompatible</span>{/if}
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      {/if}
    </div>

    <aside class="side-col">
      {#if mcLines.length || loaders.length || detail.clientSide}
        <div class="panel">
          <h3><Package size={14} /> Compatibility</h3>
          {#if mcLines.length}
            <div class="chip-row">
              {#each mcLines.slice(0, 10) as line (line)}
                <span class="chip">{line}</span>
              {/each}
            </div>
          {/if}
          {#if loaders.length}
            <div class="kv"><span>Loader</span><code>{loaders.join(", ")}</code></div>
          {/if}
          {#if detail.clientSide}
            <div class="kv"><span>Client-side</span><code>{detail.clientSide}</code></div>
          {/if}
          {#if detail.serverSide}
            <div class="kv"><span>Server-side</span><code>{detail.serverSide}</code></div>
          {/if}
        </div>
      {/if}

      {#if detail.issuesUrl || detail.sourceUrl || detail.wikiUrl || detail.discordUrl || detail.donateUrl}
        <div class="panel">
          <h3><ExternalLink size={14} /> Links</h3>
          {#if detail.issuesUrl}
            <button type="button" class="link-row" onclick={() => external(detail.issuesUrl)}>
              <Bug size={14} /> Report issues <ExternalLink size={11} />
            </button>
          {/if}
          {#if detail.sourceUrl}
            <button type="button" class="link-row" onclick={() => external(detail.sourceUrl)}>
              <Code2 size={14} /> View source <ExternalLink size={11} />
            </button>
          {/if}
          {#if detail.wikiUrl}
            <button type="button" class="link-row" onclick={() => external(detail.wikiUrl)}>
              <BookOpen size={14} /> Visit wiki <ExternalLink size={11} />
            </button>
          {/if}
          {#if detail.discordUrl}
            <button type="button" class="link-row" onclick={() => external(detail.discordUrl)}>
              <MessageCircle size={14} /> Discord <ExternalLink size={11} />
            </button>
          {/if}
          {#if detail.donateUrl}
            <button type="button" class="link-row donate" onclick={() => external(detail.donateUrl)}>
              <HandCoins size={14} /> Donate <ExternalLink size={11} />
            </button>
          {/if}
        </div>
      {/if}

      {#if detail.categories?.length}
        <div class="panel">
          <h3><Bookmark size={14} /> Tags</h3>
          <div class="chip-row">
            {#each detail.categories.slice(0, 12) as cat (cat)}
              <span class="chip">{cat}</span>
            {/each}
          </div>
        </div>
      {/if}

      {#if creators.length}
        <div class="panel">
          <h3><Users size={14} /> Creators</h3>
          <ul class="creators">
            {#each creators.slice(0, 10) as c, i (c.username + i)}
              <li>
                {#if c.avatarUrl}
                  <img src={c.avatarUrl} alt="" />
                {:else}
                  <span class="avatar-fallback">{c.username.charAt(0).toUpperCase()}</span>
                {/if}
                <span class="name">{c.username}</span>
                {#if c.role}<span class="role">{c.role}</span>{/if}
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      <div class="panel">
        <h3><Scale size={14} /> Details</h3>
        {#if detail.license}
          <div class="kv"><span>License</span><code>{detail.license}</code></div>
        {/if}
        <div class="kv"><span>Created</span><code>{formatDate(detail.dateCreated)}</code></div>
        <div class="kv"><span>Updated</span><code>{formatDate(detail.dateModified)}</code></div>
        <div class="kv"><span>Downloads</span><code>{formatCount(detail.downloads)}</code></div>
        {#if provider !== "curseforge"}
          <div class="kv"><span>Followers</span><code>{formatCount(detail.follows)}</code></div>
        {/if}
      </div>

      {#if provider === "modrinth"}
        <div class="panel">
          <h3><Wrench size={14} /> Tools</h3>
          <div class="kv"><span>Embed code</span></div>
          <div class="snippet">
            <code>{embedCode}</code>
            <CopyButton text={embedCode} label="Copy" />
          </div>
          <div class="kv"><span>Packwiz CLI</span></div>
          <div class="snippet">
            <code>{packwizCommand}</code>
            <CopyButton text={packwizCommand} label="Copy" />
          </div>
        </div>
      {/if}
    </aside>
  </div>
</div>

<style>
  .catalog-page {
    display: flex;
    flex-direction: column;
    gap: 14px;
    min-height: 100%;
  }
  .page-head {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
    align-items: center;
  }
  .back, .ghost, .primary {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 12px;
    border-radius: var(--border-radius-sm);
    font-size: 13px;
    font-weight: 650;
    cursor: pointer;
  }
  .back, .ghost {
    border: 1px solid var(--border-color);
    background: transparent;
    color: var(--text-secondary);
  }
  .primary {
    border: none;
    background: var(--accent-primary);
    color: var(--on-accent, #04140a);
  }
  .primary:disabled { opacity: 0.55; cursor: not-allowed; }
  .head-actions { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; }
  .icon-action {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }
  .icon-action:hover { color: var(--text-primary); }
  .icon-action.active { color: #f43f5e; border-color: rgba(244, 63, 94, 0.45); }
  .icon-action.active:has(+ *) { color: #f43f5e; }

  .hero {
    display: grid;
    grid-template-columns: 88px minmax(0, 1fr);
    gap: 16px;
    padding: 16px;
    border-radius: 14px;
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
  }
  .hero-icon {
    width: 88px;
    height: 88px;
    border-radius: 14px;
    overflow: hidden;
    background: var(--bg-tertiary);
    display: grid;
    place-items: center;
    font-size: 28px;
    font-weight: 900;
    color: var(--text-primary);
  }
  .hero-icon img { width: 100%; height: 100%; object-fit: cover; }
  .eyebrow { display: flex; gap: 8px; margin-bottom: 4px; }
  .provider, .type {
    font-size: 12px;
    font-weight: 800;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 2px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    color: var(--accent-primary);
  }
  .provider.cf { background: color-mix(in srgb, var(--accent-warning) 14%, transparent); color: var(--accent-warning); }
  h1 { margin: 0; font-size: 24px; color: var(--text-primary); }
  .author { margin: 4px 0 0; color: var(--text-muted); font-size: 13px; }
  .stats { display: flex; flex-wrap: wrap; gap: 12px; margin-top: 10px; color: var(--text-secondary); font-size: 12px; }
  .stats span { display: inline-flex; align-items: center; gap: 8px; }

  .content-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 300px;
    gap: 18px;
    align-items: start;
  }
  .main-col { min-width: 0; }
  .side-col {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-width: 0;
  }
  .panel {
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
    padding: 12px 14px;
  }
  .panel h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 10px;
    font-size: 12px;
    font-weight: 800;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-muted);
  }
  .chip-row { display: flex; flex-wrap: wrap; gap: 8px; }
  .chip {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    font-size: 12px;
  }
  .kv {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    padding: 4px 0;
    font-size: 12px;
  }
  .kv > span { color: var(--text-muted); }
  .kv code { color: var(--text-secondary); font-size: 12px; text-align: right; overflow-wrap: anywhere; }
  .link-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 4px;
    border: none;
    background: transparent;
    color: var(--text-secondary);
    font-size: 13px;
    cursor: pointer;
    text-align: left;
    border-radius: 6px;
  }
  .link-row:hover { background: var(--bg-tertiary); color: var(--text-primary); }
  .link-row.donate { color: #34d399; }
  .creators { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
  .creators li { display: flex; align-items: center; gap: 8px; font-size: 13px; }
  .creators img, .avatar-fallback {
    width: 24px;
    height: 24px;
    border-radius: 999px;
    object-fit: cover;
    background: var(--bg-tertiary);
  }
  .avatar-fallback {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    font-weight: 800;
    color: var(--text-muted);
  }
  .creators .name { color: var(--text-primary); font-weight: 650; }
  .creators .role { margin-left: auto; font-size: 12px; color: var(--text-muted); }
  .snippet {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 4px 0 10px;
    padding: 8px 10px;
    border-radius: var(--border-radius-sm);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
  }
  .snippet code { flex: 1; min-width: 0; font-size: 12px; color: var(--text-secondary); overflow-wrap: anywhere; }

  .tabs { display: flex; gap: 0; border-bottom: 1px solid var(--border-color); }
  .tabs button {
    padding: 10px 14px;
    border: none;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--text-muted);
    font-weight: 700;
    font-size: 13px;
    cursor: pointer;
  }
  .tabs button.active {
    color: var(--text-primary);
    border-bottom-color: var(--accent-primary);
  }
  .tabs button:disabled { opacity: 0.45; cursor: not-allowed; }

  .loading, .empty, .notice {
    padding: 20px;
    border-radius: var(--border-radius-md);
    border: 1px dashed var(--border-color);
    color: var(--text-muted);
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .notice { border-style: solid; color: var(--accent-danger); border-color: color-mix(in srgb, var(--accent-danger) 35%, transparent); }

  .plain { color: var(--text-secondary); line-height: 1.55; white-space: pre-wrap; }
  .html-body {
    color: var(--text-secondary);
    line-height: 1.55;
    overflow-wrap: anywhere;
  }
  .html-body :global(img) { max-width: 100%; height: auto; border-radius: var(--border-radius-sm); }
  .html-body :global(a) { color: var(--accent-primary); }
  .html-body :global(h1),
  .html-body :global(h2),
  .html-body :global(h3),
  .html-body :global(h4) {
    color: var(--text-primary);
    margin: 1.1em 0 0.45em;
    line-height: 1.25;
  }
  .html-body :global(h1) { font-size: 1.35rem; }
  .html-body :global(h2) { font-size: 1.2rem; }
  .html-body :global(h3) { font-size: 1.05rem; }
  .html-body :global(p),
  .html-body :global(ul),
  .html-body :global(ol) { margin: 0.55em 0; }
  .html-body :global(ul),
  .html-body :global(ol) { padding-left: 1.35em; }
  .html-body :global(code) {
    font-size: 0.9em;
    padding: 0.1em 0.35em;
    border-radius: 4px;
    background: var(--bg-tertiary);
  }
  .html-body :global(pre) {
    overflow: auto;
    padding: 12px;
    border-radius: 10px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
  }
  .html-body :global(pre code) { padding: 0; background: transparent; }
  .html-body :global(blockquote) {
    margin: 0.7em 0;
    padding-left: 12px;
    border-left: 3px solid var(--border-color);
    color: var(--text-muted);
  }
  .html-body :global(table) {
    width: 100%;
    border-collapse: collapse;
    margin: 0.8em 0;
    font-size: 13px;
  }
  .html-body :global(th),
  .html-body :global(td) {
    border: 1px solid var(--border-color);
    padding: 6px 8px;
    text-align: left;
  }

  .gallery {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 12px;
  }
  .gallery figure { margin: 0; border: 1px solid var(--border-color); border-radius: var(--border-radius-md); overflow: hidden; background: var(--bg-secondary); }
  .gallery img { display: block; width: 100%; aspect-ratio: 16 / 9; object-fit: cover; cursor: pointer; }
  .gallery figcaption { padding: 7px 10px; font-size: 12px; color: var(--text-muted); }

  .changelog { display: flex; flex-direction: column; gap: 12px; }
  .changelog article {
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
    padding: 12px 14px;
  }
  .changelog header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 10px;
    margin-bottom: 6px;
  }
  .changelog strong { color: var(--text-primary); font-size: 13px; }
  .changelog small { color: var(--text-muted); font-size: 12px; }

  .versions-toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 10px;
    color: var(--text-muted);
    font-size: 12px;
  }
  .versions ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
  .versions li {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    padding: 10px 12px;
    border-radius: 10px;
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
  }
  .versions li.incompat { opacity: 0.55; }
  .versions strong { display: block; color: var(--text-primary); font-size: 13px; }
  .versions small { color: var(--text-muted); font-size: 12px; }
  .badge {
    align-self: center;
    font-size: 12px;
    font-weight: 800;
    text-transform: uppercase;
    color: var(--accent-warning);
  }
  :global(.spin) { animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (max-width: 1000px) {
    .content-grid { grid-template-columns: 1fr; }
  }
  @media (max-width: 720px) {
    .hero { grid-template-columns: 1fr; }
  }
</style>
