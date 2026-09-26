<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-shell";
  import {
    X,
    Play,
    SkipBack,
    SkipForward,
    Trash2,
    ArrowUp,
    ArrowDown,
    Plus,
    ExternalLink,
    Search,
    ListVideo,
    ListPlus,
    PictureInPicture2,
  } from "@lucide/svelte";
  import { supabase } from "../lib/supabaseAuth";
  import {
    youtubeQueueItems,
    youtubeQueueIndex,
    youtubeQueueAdd,
    youtubeQueueAddAndPlay,
    youtubeQueuePlayAt,
    youtubeQueueNext,
    youtubeQueuePrevious,
    youtubeQueueRemoveAt,
    youtubeQueueMoveUp,
    youtubeQueueMoveDown,
    youtubeQueueClear,
    openYoutubePlayer,
    closeYoutubeQueue,
    getYoutubeVideoProgress,
    setYoutubeVideoProgress,
    type YoutubeQueueItem,
  } from "../lib/store";
  import { trapFocus } from "../lib/focusTrap";

  type BrowseVideo = {
    video_id: string;
    title: string;
    thumbnail_url: string | null;
    channel_name: string | null;
    view_count?: number | null;
  };

  const COLS = "video_id,title,thumbnail_url,channel_name,view_count";

  let items = $state<YoutubeQueueItem[]>([]);
  let queueIndex = $state(0);
  const current = $derived(items[queueIndex] ?? null);

  let browseQuery = $state("");
  let browseResults = $state<BrowseVideo[]>([]);
  let browseLoading = $state(false);
  let browseError = $state("");
  let linkInput = $state("");
  let linkError = $state("");
  let linkBusy = $state(false);

  let queueScroller: HTMLElement | undefined = $state();
  let queueEls: (HTMLElement | null)[] = [];

  onMount(() => {
    const unsubItems = youtubeQueueItems.subscribe((v) => (items = v));
    const unsubIndex = youtubeQueueIndex.subscribe((v) => (queueIndex = v));
    window.addEventListener("message", onProgressMessage);
    void loadBrowse();
    return () => {
      window.removeEventListener("message", onProgressMessage);
      unsubItems();
      unsubIndex();
    };
  });

  $effect(() => {
    if (queueIndex < 0 || !queueScroller) return;
    const el = queueEls[queueIndex];
    if (el) {
      const r = el.getBoundingClientRect();
      const s = queueScroller.getBoundingClientRect();
      if (r.top < s.top || r.bottom > s.bottom) {
        el.scrollIntoView({ block: "nearest" });
      }
    }
  });

  function embedSrc(id: string) {
    const saved = getYoutubeVideoProgress(id);
    const resume = saved && saved > 1 ? `&start=${Math.floor(saved)}` : "";
    return `https://www.youtube-nocookie.com/embed/${id}?autoplay=1&rel=0&modestbranding=1&playsinline=1&iv_load_policy=3&enablejsapi=1${resume}`;
  }

  /** Learn the embed's playback position (YouTube posts infoDelivery postMessages when enablejsapi=1). */
  function onProgressMessage(ev: MessageEvent) {
    const d = ev.data as { event?: string; info?: { currentTime?: number } };
    if (!d || d.event !== "infoDelivery" || !d.info) return;
    const t = d.info.currentTime;
    const c = current;
    if (c && typeof t === "number" && Number.isFinite(t) && t > 1) {
      setYoutubeVideoProgress(c.videoId, Math.floor(t));
    }
  }

  function toMini() {
    const c = current;
    if (!c) return;
    openYoutubePlayer({
      videoId: c.videoId,
      title: c.title,
      originRect: null,
      startMini: true,
      start: getYoutubeVideoProgress(c.videoId),
    });
  }

  function thumbUrlOf(v: { thumbnail_url?: string | null; video_id: string }) {
    if (v.thumbnail_url) return v.thumbnail_url;
    return `https://i.ytimg.com/vi/${v.video_id}/hqdefault.jpg`;
  }

  function itemFromBrowse(v: BrowseVideo): YoutubeQueueItem {
    return { videoId: v.video_id, title: v.title, thumbnailUrl: thumbUrlOf(v) };
  }

  function queueContains(id: string) {
    return items.some((v) => v.videoId === id);
  }

  async function loadBrowse() {
    browseLoading = true;
    browseError = "";
    const q = browseQuery.trim();
    try {
      let query = supabase.from("youtube_feed").select(COLS);
      if (q) query = query.ilike("title", `%${q}%`);
      const { data, error } = await query
        .order("view_count", { ascending: false })
        .limit(60);
      if (error) throw new Error(error.message);
      browseResults = (data ?? []) as BrowseVideo[];
    } catch (e) {
      browseResults = [];
      browseError = String(e);
    } finally {
      browseLoading = false;
    }
  }

  function openYoutubeInBrowser() {
    const q = browseQuery.trim();
    void open(
      q
        ? `https://www.youtube.com/results?search_query=${encodeURIComponent(q)}`
        : "https://www.youtube.com",
    );
  }

  function parseYoutubeId(input: string): string | null {
    const raw = input.trim();
    if (!raw) return null;
    const patterns = [
      /(?:youtube\.com|youtube-nocookie\.com)\/embed\/([\w-]{11})/,
      /youtube\.com\/(?:shorts|live|v)\/([\w-]{11})/,
      /youtu\.be\/([\w-]{11})/,
      /youtube\.com\/watch\?(?:[^#\s]*?&)?v=([\w-]{11})/,
    ];
    for (const re of patterns) {
      const m = re.exec(raw);
      if (m?.[1]) return m[1];
    }
    if (/^[\w-]{11}$/.test(raw)) return raw;
    return null;
  }

  async function fetchTitle(id: string): Promise<string> {
    try {
      const res = await fetch(
        `https://www.youtube.com/oembed?format=json&url=${encodeURIComponent(
          `https://www.youtube.com/watch?v=${id}`,
        )}`,
      );
      if (!res.ok) return "";
      const j = (await res.json()) as { title?: unknown };
      return typeof j?.title === "string" ? j.title : "";
    } catch {
      return "";
    }
  }

  async function addByLink() {
    const id = parseYoutubeId(linkInput);
    if (!id) {
      linkError = "Not a YouTube link — paste a watch, shorts or youtu.be URL.";
      return;
    }
    linkBusy = true;
    linkError = "";
    const title = await fetchTitle(id);
    youtubeQueueAdd({
      videoId: id,
      title: title || "YouTube video",
      thumbnailUrl: `https://i.ytimg.com/vi/${id}/hqdefault.jpg`,
    });
    linkInput = "";
    linkBusy = false;
  }

  function closeQueue() {
    closeYoutubeQueue();
  }

  function onOverlayClick(e: MouseEvent) {
    if (e.target === e.currentTarget) closeQueue();
  }
</script>

<div class="qw-overlay" role="presentation" onclick={onOverlayClick}>
  <div
    class="qw-modal"
    role="dialog"
    aria-modal="true"
    aria-label="YouTube player and queue"
    use:trapFocus={{ onEscape: closeQueue }}
  >
    <header class="qw-header">
      <div class="qw-brand">
        <ListVideo size={18} />
        <h2>YouTube</h2>
        <span class="qw-count">{items.length}</span>
      </div>
      <div class="qw-header-actions">
        {#if current}
          <button type="button" class="qw-ge" onclick={toMini} title="Watch current video in a floating mini window">
            <PictureInPicture2 size={16} />
            <span>Mini</span>
          </button>
        {/if}
        <button type="button" class="qw-icon" onclick={closeQueue} aria-label="Close">
          <X size={18} />
        </button>
      </div>
    </header>

    <div class="qw-body">
      <section class="qw-pane qw-pane-left">
        <div class="qw-player">
          {#if current}
            {#key current.videoId}
              <iframe
                class="qw-player-frame"
                src={embedSrc(current.videoId)}
                title={current.title || "YouTube video"}
                allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture"
                allowfullscreen
              ></iframe>
            {/key}
          {:else}
            <div class="qw-player-empty">
              <Play size={40} />
              <p>Queue is empty — find a video on the right or paste a link.</p>
            </div>
          {/if}
        </div>

        <div class="qw-player-bar">
          <button
            type="button"
            class="qw-ctl"
            disabled={!items.length}
            onclick={youtubeQueuePrevious}
            aria-label="Previous video"
            title="Previous"
          >
            <SkipBack size={16} />
          </button>
          <div class="qw-now">
            <strong class="qw-now-title">
              {current?.title || "Nothing playing"}
            </strong>
            {#if items.length}
              <span class="qw-now-pos">{queueIndex + 1} / {items.length}</span>
            {/if}
          </div>
          <button
            type="button"
            class="qw-ctl qw-ctl-play"
            disabled={!items.length}
            onclick={youtubeQueueNext}
            aria-label="Next video"
            title="Next"
          >
            <SkipForward size={16} />
          </button>
        </div>

        <div class="qw-queue-wrap">
          <div class="qw-section-head">
            <span>Queue</span>
            {#if items.length > 1}
              <button type="button" class="qw-ghost-btn" onclick={youtubeQueueClear}>
                Clear
              </button>
            {/if}
          </div>
          {#if items.length === 0}
            <p class="qw-empty">Empty — add videos from the browser or by link.</p>
          {:else}
            <div bind:this={queueScroller} class="qw-queue-list" role="list">
              {#each items as item, i (item.videoId)}
                <div
                  class="qw-qi"
                  class:active={i === queueIndex}
                  role="listitem"
                  bind:this={queueEls[i]}
                >
                  <img class="qw-qi-thumb" src={item.thumbnailUrl} alt="" loading="lazy" />
                  <div class="qw-qi-info">
                    <span class="qw-qi-title">{item.title}</span>
                    {#if i === queueIndex}
                      <span class="qw-now-badge">Playing</span>
                    {/if}
                  </div>
                  <div class="qw-qi-actions">
                    <button
                      type="button"
                      class="qw-mini-btn"
                      disabled={i === queueIndex}
                      onclick={() => youtubeQueuePlayAt(i)}
                      aria-label="Play now"
                      title="Play now"
                    >
                      <Play size={13} />
                    </button>
                    <button
                      type="button"
                      class="qw-mini-btn"
                      disabled={i === 0}
                      onclick={() => youtubeQueueMoveUp(i)}
                      aria-label="Move up"
                      title="Move up"
                    >
                      <ArrowUp size={13} />
                    </button>
                    <button
                      type="button"
                      class="qw-mini-btn"
                      disabled={i === items.length - 1}
                      onclick={() => youtubeQueueMoveDown(i)}
                      aria-label="Move down"
                      title="Move down"
                    >
                      <ArrowDown size={13} />
                    </button>
                    <button
                      type="button"
                      class="qw-mini-btn qw-mini-btn-danger"
                      onclick={() => youtubeQueueRemoveAt(i)}
                      aria-label="Remove from queue"
                      title="Remove from queue"
                    >
                      <Trash2 size={13} />
                    </button>
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      </section>

      <section class="qw-pane qw-pane-right">
        <div class="qw-tool">
          <label class="qw-field">
            <span class="qw-field-label">Add by link</span>
            <div class="qw-row-inputs">
              <input
                class="qw-input"
                type="text"
                placeholder="https://youtube.com/watch?v=…"
                bind:value={linkInput}
                onkeydown={(e) => {
                  if (e.key === "Enter") void addByLink();
                }}
              />
              <button type="button" class="qw-btn" disabled={linkBusy} onclick={() => void addByLink()}>
                <Plus size={14} />
                <span>Add</span>
              </button>
            </div>
            {#if linkError}
              <p class="qw-field-error">{linkError}</p>
            {/if}
          </label>
        </div>

        <div class="qw-section-head qw-browse-head">
          <span>Browse YouTube</span>
          <button type="button" class="qw-ghost-btn" onclick={openYoutubeInBrowser}>
            <ExternalLink size={13} />
            Open in browser
          </button>
        </div>

        <div class="qw-toolbar">
          <div class="qw-row-inputs">
            <input
              class="qw-input"
              type="text"
              placeholder="Search the catalog…"
              bind:value={browseQuery}
              onkeydown={(e) => {
                if (e.key === "Enter") void loadBrowse();
              }}
            />
            <button type="button" class="qw-btn" disabled={browseLoading} onclick={() => void loadBrowse()}>
              <Search size={14} />
            </button>
          </div>
        </div>

        <div class="qw-results" aria-busy={browseLoading}>
          {#if browseLoading}
            <p class="qw-status">Searching…</p>
          {:else if browseError}
            <p class="qw-status qw-status-error">Couldn’t load videos. {browseError}</p>
          {:else if browseResults.length === 0}
            <p class="qw-status">No videos found. Try another query or paste a link above.</p>
          {:else}
            {#each browseResults as v (v.video_id)}
              <div class="qw-res-item">
                <img class="qw-res-thumb" src={thumbUrlOf(v)} alt="" loading="lazy" />
                <div class="qw-res-info">
                  <span class="qw-res-title">{v.title}</span>
                  {#if v.channel_name}
                    <span class="qw-res-channel">{v.channel_name}</span>
                  {/if}
                </div>
                <div class="qw-res-actions">
                  <button
                    type="button"
                    class="qw-mini-btn"
                    disabled={queueContains(v.video_id)}
                    onclick={() => youtubeQueueAdd(itemFromBrowse(v))}
                    aria-label="Add to queue"
                    title={queueContains(v.video_id) ? "Already in queue" : "Add to queue"}
                  >
                    <ListPlus size={14} />
                  </button>
                  <button
                    type="button"
                    class="qw-mini-btn qw-mini-btn-accent"
                    onclick={() => youtubeQueueAddAndPlay(itemFromBrowse(v))}
                    aria-label="Play now"
                    title="Play now"
                  >
                    <Play size={14} />
                  </button>
                </div>
              </div>
            {/each}
          {/if}
        </div>

        <p class="qw-note">
          YouTube doesn’t allow full pages inside a window, so browse the catalog here,
          or open real YouTube in your browser and paste the link above.
        </p>
      </section>
    </div>
  </div>
</div>

<style>
  .qw-overlay {
    position: fixed;
    inset: 0;
    z-index: 9990;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: max(12px, 2vh) max(12px, 2vw);
    background: rgba(0, 0, 0, 0.62);
    backdrop-filter: blur(4px);
    animation: qw-fade-in 0.18s ease both;
  }

  .qw-modal {
    width: min(1180px, 100%);
    max-height: min(90vh, 900px);
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
    border: 1px solid color-mix(in srgb, var(--border-color) 72%, var(--accent-primary) 28%);
    border-radius: var(--border-radius-xl);
    box-shadow:
      0 24px 80px rgba(0, 0, 0, 0.5),
      var(--shadow-lg);
    overflow: hidden;
    animation: qw-pop 0.24s cubic-bezier(0.22, 1, 0.36, 1) both;
  }

  .qw-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border-color);
    background: color-mix(in srgb, var(--bg-elevated) 80%, var(--bg-secondary));
    flex-shrink: 0;
  }

  .qw-brand {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 0;
  }

  .qw-brand :global(svg) {
    color: var(--accent-primary);
    flex-shrink: 0;
  }

  .qw-brand h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 800;
  }

  .qw-count {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 1px 8px;
    border-radius: var(--border-radius-sm);
    background: color-mix(in srgb, var(--accent-primary) 16%, transparent);
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 700;
  }

  .qw-header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .qw-ge {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated);
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .qw-ge:hover {
    border-color: var(--accent-primary);
    color: var(--accent-primary);
  }

  .qw-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }

  .qw-icon:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .qw-body {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1.06fr) minmax(0, 0.94fr);
    gap: 18px;
    padding: 16px;
  }

  .qw-pane {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
  }

  .qw-pane-left {
    overflow: hidden;
  }

  .qw-pane-right {
    overflow: hidden;
  }

  .qw-player {
    position: relative;
    width: 100%;
    aspect-ratio: 16 / 9;
    flex-shrink: 0;
    border-radius: var(--border-radius-md);
    overflow: hidden;
    background: #000;
    border: 1px solid var(--border-color);
  }

  .qw-player-frame {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    border: 0;
  }

  .qw-player-empty {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    color: var(--text-muted);
    text-align: center;
    padding: 20px;
  }

  .qw-player-empty :global(svg) {
    opacity: 0.4;
  }

  .qw-player-empty p {
    margin: 0;
    font-size: 13px;
  }

  .qw-player-bar {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-shrink: 0;
    padding: 8px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: color-mix(in srgb, var(--bg-elevated) 55%, var(--bg-secondary));
  }

  .qw-ctl {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    padding: 0;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    flex-shrink: 0;
  }

  .qw-ctl:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .qw-ctl:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .qw-ctl-play {
    background: var(--accent-primary);
    color: var(--on-accent, #000);
  }

  .qw-ctl-play:hover:not(:disabled) {
    background: var(--accent-hover);
    color: var(--on-accent, #000);
  }

  .qw-now {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .qw-now-title {
    font-size: 13px;
    font-weight: 700;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .qw-now-pos {
    font-size: 12px;
    color: var(--text-muted);
  }

  .qw-queue-wrap {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .qw-section-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    font-size: 12px;
    font-weight: 800;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
  }

  .qw-ghost-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    text-transform: none;
    letter-spacing: 0;
    cursor: pointer;
  }

  .qw-ghost-btn:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .qw-empty {
    margin: 0;
    padding: 10px 12px;
    border-radius: var(--border-radius-sm);
    background: color-mix(in srgb, var(--bg-elevated) 55%, transparent);
    color: var(--text-muted);
    font-size: 12px;
  }

  .qw-queue-list {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-right: 4px;
    scrollbar-width: thin;
    scrollbar-color: var(--bg-elevated) transparent;
  }

  .qw-qi {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px;
    border: 1px solid transparent;
    border-radius: var(--border-radius-sm);
    background: color-mix(in srgb, var(--bg-elevated) 50%, transparent);
    transition: border-color var(--motion-fast, 160ms) ease;
  }

  .qw-qi.active {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, var(--border-color));
    background: color-mix(in srgb, var(--accent-primary) 9%, var(--bg-elevated));
  }

  .qw-qi-thumb {
    width: 64px;
    aspect-ratio: 16 / 9;
    object-fit: cover;
    flex-shrink: 0;
    border-radius: var(--border-radius-sm);
    background: var(--bg-tertiary);
  }

  .qw-qi-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .qw-qi-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .qw-now-badge {
    display: inline-block;
    line-height: 1.4;
    white-space: nowrap;
    vertical-align: baseline;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    align-self: flex-start;
    padding: 1px 6px;
    border-radius: var(--border-radius-sm);
    background: var(--accent-primary);
    color: var(--on-accent, #000);
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0.03em;
    text-transform: uppercase;
  }

  .qw-qi-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .qw-mini-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }

  .qw-mini-btn:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .qw-mini-btn:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .qw-mini-btn-accent:hover:not(:disabled) {
    color: var(--accent-primary);
  }

  .qw-mini-btn-danger:hover:not(:disabled) {
    color: var(--accent-danger, #ef4444);
    background: color-mix(in srgb, var(--accent-danger, #ef4444) 12%, transparent);
  }

  .qw-toolbar {
    flex-shrink: 0;
  }

  .qw-field {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .qw-field-label {
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
  }

  .qw-row-inputs {
    display: flex;
    gap: 8px;
  }

  .qw-input {
    flex: 1;
    min-width: 0;
  }

  .qw-field-error {
    margin: 0;
    font-size: 12px;
    color: var(--accent-danger, #ef4444);
  }

  .qw-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-radius: var(--border-radius-sm);
    border: none;
    background: var(--accent-primary);
    color: var(--on-accent, #000);
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    flex-shrink: 0;
  }

  .qw-btn:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .qw-btn:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .qw-browse-head {
    margin-top: 4px;
  }

  .qw-results {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-right: 4px;
    scrollbar-width: thin;
    scrollbar-color: var(--bg-elevated) transparent;
  }

  .qw-status {
    margin: 0;
    padding: 10px 12px;
    border-radius: var(--border-radius-sm);
    background: color-mix(in srgb, var(--bg-elevated) 55%, transparent);
    color: var(--text-muted);
    font-size: 12px;
  }

  .qw-status-error {
    color: var(--accent-danger, #ef4444);
  }

  .qw-res-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-sm);
    background: color-mix(in srgb, var(--bg-elevated) 50%, transparent);
  }

  .qw-res-thumb {
    width: 64px;
    aspect-ratio: 16 / 9;
    object-fit: cover;
    flex-shrink: 0;
    border-radius: var(--border-radius-sm);
    background: var(--bg-tertiary);
  }

  .qw-res-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .qw-res-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .qw-res-channel {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .qw-res-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .qw-note {
    margin: 0;
    flex-shrink: 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--text-muted);
    opacity: 0.85;
  }

  @keyframes qw-fade-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  @keyframes qw-pop {
    from {
      opacity: 0;
      transform: translateY(14px) scale(0.96);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }

  @media (max-width: 860px) {
    .qw-body {
      grid-template-columns: minmax(0, 1fr);
      overflow-y: auto;
    }

    .qw-pane {
      min-height: auto;
    }

    .qw-queue-list {
      max-height: 260px;
    }

    .qw-results {
      max-height: 380px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .qw-overlay,
    .qw-modal {
      animation: none;
    }
  }
</style>