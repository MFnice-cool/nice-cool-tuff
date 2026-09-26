<!-- Litube-inspired lite player: large modal + draggable/resizable mini window. -->
<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { open } from "@tauri-apps/plugin-shell";
  import { X, ExternalLink, PictureInPicture2, Maximize2, GripVertical } from "@lucide/svelte";
  import { trapFocus } from "../lib/focusTrap";
  import { suggestYouTubeModalWidth, YOUTUBE_MODAL_MIN_WIDTH } from "../lib/youtubeSizing";

  let {
    videoId,
    title = "",
    originRect = null,
    startMini = false,
    start = 0,
    onclose,
  }: {
    videoId: string;
    title?: string;
    originRect?: DOMRect | null;
    startMini?: boolean;
    start?: number;
    onclose?: () => void;
  } = $props();
  const MINI_POS_KEY = "tuffbox-youtube-mini-pos";
  const MINI_SIZE_KEY = "tuffbox-youtube-mini-size";
  const MINI_W_DEFAULT = 440;
  const MINI_W_MIN = 280;
  const MINI_HEADER_H = 44;

  let embedAlive = $state(true);
  let shellEl: HTMLDivElement | null = $state(null);
  let dialogEl: HTMLDivElement | null = $state(null);
  let backdropIn = $state(false);
  let dialogIn = $state(false);
  let backdropOut = $state(false);
  let dialogOut = $state(false);
  let closing = $state(false);
  // startMini is a one-shot initial prop — snapshot at mount, not live.
  let mode: "modal" | "mini" = $state(initialMode());

  function initialMode(): "modal" | "mini" {
    return startMini ? "mini" : "modal";
  }

  let miniX = $state(24);
  let miniY = $state(24);
  let miniW = $state(MINI_W_DEFAULT);
  let dragging = $state(false);
  let resizing = $state(false);
  let dragDx = 0;
  let dragDy = 0;
  let resizeStartX = 0;
  let resizeStartW = MINI_W_DEFAULT;
  let resizeStartMiniX = 0;
  let resizeStartMiniY = 0;
  let resizeFromLeft = false;
  let resizeFromTop = false;

  let embedLoaded = $state(false);
  let embedError = $state(false);
  let showEmbedTip = $state(false);
  let embedTipDismissed = $state(false);
  let embedLoadTimer: ReturnType<typeof setTimeout> | undefined;

  // start is a one-shot initial prop — snapshot at mount, not live.
  const embedStart = Math.max(0, Math.floor(initialStart()));

  function initialStart(): number {
    return start || 0;
  }
  const embedSrc = $derived(
    `https://www.youtube-nocookie.com/embed/${videoId}?autoplay=1&rel=0&modestbranding=1&playsinline=1&iv_load_policy=3` +
      (embedStart > 0 ? `&start=${embedStart}` : ""),
  );
  const miniVideoH = $derived(Math.round((miniW * 9) / 16));

  function bodyPortal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }

  function clearEmbedLoadTimer() {
    if (embedLoadTimer !== undefined) {
      clearTimeout(embedLoadTimer);
      embedLoadTimer = undefined;
    }
  }

  function resetEmbedLoadState() {
    embedLoaded = false;
    embedError = false;
    showEmbedTip = false;
    embedTipDismissed = false;
  }

  function startEmbedLoadWatch() {
    clearEmbedLoadTimer();
    resetEmbedLoadState();
    if (!embedAlive) return;
    embedLoadTimer = setTimeout(() => {
      if (!embedLoaded && !embedTipDismissed && embedAlive) {
        showEmbedTip = true;
      }
    }, 5000);
  }

  function onEmbedLoad() {
    embedLoaded = true;
    showEmbedTip = false;
    clearEmbedLoadTimer();
  }

  function onEmbedError() {
    embedError = true;
    if (!embedTipDismissed && embedAlive) {
      showEmbedTip = true;
    }
    clearEmbedLoadTimer();
  }

  function dismissEmbedTip() {
    embedTipDismissed = true;
    showEmbedTip = false;
  }

  function destroyEmbed() {
    clearEmbedLoadTimer();
    embedAlive = false;
  }

  function prefersReducedMotion(): boolean {
    try {
      if (document.documentElement.classList.contains("potato-pc")) return true;
      return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    } catch {
      return false;
    }
  }

  function miniWMax(): number {
    const vw = document.documentElement?.clientWidth ?? window.innerWidth;
    return Math.max(MINI_W_MIN, Math.min(960, vw - 16));
  }

  function clampMiniW(w: number): number {
    return Math.min(miniWMax(), Math.max(MINI_W_MIN, Math.round(w)));
  }

  function loadMiniSize() {
    try {
      const raw = localStorage.getItem(MINI_SIZE_KEY);
      if (!raw) {
        miniW = MINI_W_DEFAULT;
        return;
      }
      const parsed = JSON.parse(raw) as { w?: number };
      if (typeof parsed.w === "number" && Number.isFinite(parsed.w)) {
        miniW = clampMiniW(parsed.w);
        return;
      }
    } catch {
      /* ignore */
    }
    miniW = MINI_W_DEFAULT;
  }

  function saveMiniSize() {
    try {
      localStorage.setItem(MINI_SIZE_KEY, JSON.stringify({ w: miniW }));
    } catch {
      /* ignore */
    }
  }

  function loadMiniGeometry() {
    loadMiniSize();
    try {
      const raw = localStorage.getItem(MINI_POS_KEY);
      if (!raw) {
        placeMiniDefault();
        return;
      }
      const parsed = JSON.parse(raw) as { x?: number; y?: number };
      if (typeof parsed.x === "number" && typeof parsed.y === "number") {
        miniX = parsed.x;
        miniY = parsed.y;
        clampMiniPos();
        return;
      }
    } catch {
      /* ignore */
    }
    placeMiniDefault();
  }

  function placeMiniDefault() {
    const pad = 20;
    const vw = document.documentElement?.clientWidth ?? window.innerWidth;
    const vh = document.documentElement?.clientHeight ?? window.innerHeight;
    const h = Math.round((miniW * 9) / 16) + MINI_HEADER_H;
    miniX = Math.max(pad, vw - miniW - pad);
    miniY = Math.max(pad, vh - h - pad);
  }

  function clampMiniPos() {
    const w = miniW;
    const h = Math.round((miniW * 9) / 16) + MINI_HEADER_H;
    // Use documentElement for reliable viewport size (matches actual visible area).
    const vw = document.documentElement?.clientWidth ?? window.innerWidth;
    const vh = document.documentElement?.clientHeight ?? window.innerHeight;
    const maxX = Math.max(8, vw - w - 8);
    const maxY = Math.max(8, vh - h - 8);
    miniX = Math.min(maxX, Math.max(8, miniX));
    miniY = Math.min(maxY, Math.max(8, miniY));
  }

  function saveMiniPos() {
    try {
      localStorage.setItem(MINI_POS_KEY, JSON.stringify({ x: miniX, y: miniY }));
    } catch {
      /* ignore */
    }
  }

  async function playOpenAnimation() {
    if (mode === "mini") {
      loadMiniGeometry();
      return;
    }
    await tick();
    if (!dialogEl) {
      backdropIn = true;
      dialogIn = true;
      return;
    }

    if (prefersReducedMotion()) {
      backdropIn = true;
      dialogIn = true;
      return;
    }

    backdropIn = true;

    if (originRect && originRect.width > 0 && originRect.height > 0) {
      const end = dialogEl.getBoundingClientRect();
      const dx =
        originRect.left + originRect.width / 2 - (end.left + end.width / 2);
      const dy =
        originRect.top + originRect.height / 2 - (end.top + end.height / 2);
      const sx = Math.max(0.08, originRect.width / end.width);
      const sy = Math.max(0.08, originRect.height / end.height);
      dialogEl.style.transform = `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})`;
      dialogEl.style.opacity = "0.35";
      void dialogEl.offsetWidth;
      dialogEl.style.transition =
        "transform 0.42s cubic-bezier(0.22, 1, 0.36, 1), opacity 0.32s ease";
      dialogEl.style.transform = "translate(0, 0) scale(1)";
      dialogEl.style.opacity = "1";
      dialogIn = true;
    } else {
      dialogIn = true;
    }
  }

  function toMini() {
    mode = "mini";
    backdropIn = false;
    dialogIn = true;
    loadMiniGeometry();
  }

  function toModal() {
    mode = "modal";
    refreshModalWidth();
    closing = false;
    backdropOut = false;
    dialogOut = false;
    void tick().then(() => {
      backdropIn = true;
      dialogIn = true;
    });
  }

  function close() {
    if (closing) return;
    closing = true;
    backdropIn = false;
    destroyEmbed();

    const finish = () => onclose?.();

    if (mode === "mini" || prefersReducedMotion() || !dialogEl) {
      finish();
      return;
    }

    backdropOut = true;
    dialogIn = false;
    dialogOut = true;

    if (originRect && originRect.width > 0) {
      const end = dialogEl.getBoundingClientRect();
      const dx =
        originRect.left + originRect.width / 2 - (end.left + end.width / 2);
      const dy =
        originRect.top + originRect.height / 2 - (end.top + end.height / 2);
      const sx = Math.max(0.08, originRect.width / end.width);
      const sy = Math.max(0.08, originRect.height / end.height);
      dialogEl.style.transition =
        "transform 0.28s cubic-bezier(0.4, 0, 1, 1), opacity 0.22s ease";
      dialogEl.style.transform = `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})`;
      dialogEl.style.opacity = "0";
    }

    window.setTimeout(finish, 280);
  }

  async function openInBrowser() {
    await open(`https://www.youtube.com/watch?v=${videoId}`);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }

  function onDragStart(e: PointerEvent) {
    if (e.button !== 0 || resizing) return;
    const target = e.target as HTMLElement | null;
    if (target?.closest("button") || target?.closest(".yp-resize")) return;
    dragging = true;
    dragDx = e.clientX - miniX;
    dragDy = e.clientY - miniY;
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
  }

  function onDragMove(e: PointerEvent) {
    if (!dragging) return;
    miniX = e.clientX - dragDx;
    miniY = e.clientY - dragDy;
    clampMiniPos();
  }

  function onDragEnd(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId);
    } catch {
      /* ignore */
    }
    saveMiniPos();
  }

  function onResizeStart(e: PointerEvent, fromLeft: boolean, fromTop: boolean) {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    dragging = false;
    resizing = true;
    resizeFromLeft = fromLeft;
    resizeFromTop = fromTop;
    resizeStartX = e.clientX;
    resizeStartW = miniW;
    resizeStartMiniX = miniX;
    resizeStartMiniY = miniY;
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
  }

  function onResizeMove(e: PointerEvent) {
    if (!resizing) return;
    const dx = e.clientX - resizeStartX;
    const nextW = clampMiniW(resizeFromLeft ? resizeStartW - dx : resizeStartW + dx);
    if (resizeFromLeft) {
      miniX = resizeStartMiniX + (resizeStartW - nextW);
    }
    if (resizeFromTop) {
      const prevH = Math.round((resizeStartW * 9) / 16) + MINI_HEADER_H;
      const nextH = Math.round((nextW * 9) / 16) + MINI_HEADER_H;
      miniY = resizeStartMiniY + (prevH - nextH);
    }
    miniW = nextW;
    clampMiniPos();
  }

  function onResizeEnd(e: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId);
    } catch {
      /* ignore */
    }
    saveMiniSize();
    saveMiniPos();
  }

  function onWinResize() {
    if (mode === "mini") {
      miniW = clampMiniW(miniW);
      clampMiniPos();
    } else {
      refreshModalWidth();
    }
  }

  // Screen-aware modal width (Settings "Interface scale" compatible: the
  // html zoom changes the CSS-px viewport, which this measures directly).
  let modalWidth = $state(YOUTUBE_MODAL_MIN_WIDTH);

  function refreshModalWidth() {
    const vw = document.documentElement?.clientWidth || window.innerWidth || 0;
    const vh = document.documentElement?.clientHeight || window.innerHeight || 0;
    modalWidth = suggestYouTubeModalWidth(vw, vh);
  }

  onMount(() => {
    refreshModalWidth();
    void playOpenAnimation();
  });

  $effect(() => {
    if (!embedAlive) {
      clearEmbedLoadTimer();
      return;
    }
    videoId;
    startEmbedLoadWatch();
    return () => clearEmbedLoadTimer();
  });

  onDestroy(destroyEmbed);
</script>

<svelte:window onkeydown={onKeydown} onresize={onWinResize} />

<div
  bind:this={shellEl}
  class="yp-shell"
  class:is-mini={mode === "mini"}
  class:yp-in={backdropIn || mode === "mini"}
  class:yp-out={backdropOut}
  class:dragging
  class:resizing
  role={mode === "modal" ? "presentation" : "dialog"}
  aria-label={mode === "mini" ? title || "YouTube mini player" : undefined}
  style={mode === "mini" ? `left: ${miniX}px; top: ${miniY}px; width: ${miniW}px;` : undefined}
  use:bodyPortal
  onclick={(e) => mode === "modal" && e.target === e.currentTarget && close()}
  onkeydown={() => {}}
>
  <div
    bind:this={dialogEl}
    class="yp-dialog"
    class:yp-in={dialogIn || mode === "mini"}
    class:yp-out={dialogOut}
    class:mini={mode === "mini"}
    role="dialog"
    aria-modal={mode === "modal" ? "true" : "false"}
    aria-label={title || "YouTube player"}
    style={`--yp-modal-w: ${modalWidth}px;`}
    use:trapFocus={{ onEscape: close, enabled: mode === "modal" }}
  >
    <div
      class="yp-header"
      class:draggable={mode === "mini"}
      role={mode === "mini" ? "toolbar" : "banner"}
      onpointerdown={mode === "mini" ? onDragStart : undefined}
      onpointermove={mode === "mini" ? onDragMove : undefined}
      onpointerup={mode === "mini" ? onDragEnd : undefined}
      onpointercancel={mode === "mini" ? onDragEnd : undefined}
    >
      {#if mode === "mini"}
        <span class="yp-grip" aria-hidden="true"><GripVertical size={14} /></span>
      {/if}
      <h3 class="yp-title">{title || "YouTube"}</h3>
      <div class="yp-actions">
        {#if mode === "modal"}
          <button type="button" class="yp-btn" onclick={toMini} title="Watch in a floating mini window while you work">
            <PictureInPicture2 size={16} />
            <span>Mini player</span>
          </button>
          <button type="button" class="yp-btn" onclick={openInBrowser}>
            <ExternalLink size={16} />
            <span>Open in browser</span>
          </button>
        {:else}
          <button type="button" class="yp-icon-btn" onclick={toModal} aria-label="Expand" title="Expand">
            <Maximize2 size={16} />
          </button>
          <button type="button" class="yp-icon-btn" onclick={openInBrowser} aria-label="Open in browser" title="Open in browser">
            <ExternalLink size={16} />
          </button>
        {/if}
        <button type="button" class="yp-icon-btn" onclick={close} aria-label="Close">
          <X size={18} />
        </button>
      </div>
    </div>
    <div class="yp-frame-wrap" style={mode === "mini" ? `height: ${miniVideoH}px;` : undefined}>
      {#if embedAlive}
        <iframe
          src={embedSrc}
          title={title || "YouTube video"}
          allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture"
          allowfullscreen
          onload={onEmbedLoad}
          onerror={onEmbedError}
        ></iframe>
      {/if}
      {#if showEmbedTip && embedAlive}
        <div class="yp-embed-tip">
          <span class="yp-embed-tip-text">Video not playing?</span>
          <button type="button" class="yp-btn yp-embed-tip-btn" onclick={openInBrowser}>
            <ExternalLink size={14} />
            <span>Open in browser</span>
          </button>
          <button type="button" class="yp-icon-btn" onclick={dismissEmbedTip} aria-label="Dismiss tip">
            <X size={16} />
          </button>
        </div>
      {/if}
    </div>

    {#if mode === "mini"}
      <button
        type="button"
        class="yp-resize se"
        aria-label="Resize mini player"
        title="Drag to resize"
        onpointerdown={(e) => onResizeStart(e, false, false)}
        onpointermove={onResizeMove}
        onpointerup={onResizeEnd}
        onpointercancel={onResizeEnd}
      ></button>
      <button
        type="button"
        class="yp-resize sw"
        aria-label="Resize mini player from bottom-left"
        title="Drag to resize"
        onpointerdown={(e) => onResizeStart(e, true, false)}
        onpointermove={onResizeMove}
        onpointerup={onResizeEnd}
        onpointercancel={onResizeEnd}
      ></button>
      <button
        type="button"
        class="yp-resize ne"
        aria-label="Resize mini player from top-right"
        title="Drag to resize"
        onpointerdown={(e) => onResizeStart(e, false, true)}
        onpointermove={onResizeMove}
        onpointerup={onResizeEnd}
        onpointercancel={onResizeEnd}
      ></button>
      <button
        type="button"
        class="yp-resize nw"
        aria-label="Resize mini player from top-left"
        title="Drag to resize"
        onpointerdown={(e) => onResizeStart(e, true, true)}
        onpointermove={onResizeMove}
        onpointerup={onResizeEnd}
        onpointercancel={onResizeEnd}
      ></button>
    {/if}
  </div>
</div>

<style>
  .yp-shell {
    position: fixed;
    inset: 0;
    z-index: 10000;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: max(12px, 2vh) max(12px, 2vw);
    background:
      repeating-linear-gradient(
        0deg,
        transparent 0,
        transparent 3px,
        rgba(0, 0, 0, 0.12) 3px,
        rgba(0, 0, 0, 0.12) 4px
      ),
      radial-gradient(
        ellipse 70% 55% at 50% 42%,
        color-mix(in srgb, var(--accent-primary) 14%, transparent) 0%,
        transparent 62%
      ),
      radial-gradient(
        ellipse at center,
        color-mix(in srgb, var(--bg-primary) 35%, #000000) 0%,
        color-mix(in srgb, var(--bg-primary) 12%, #000000) 55%,
        rgba(0, 0, 0, 0.88) 100%
      );
    opacity: 0;
    transition: opacity 0.28s ease;
    /* Invisible until .yp-in — must not steal clicks from the launcher. */
    pointer-events: none;
  }

  .yp-shell.yp-in:not(.is-mini) {
    opacity: 1;
    pointer-events: auto;
  }

  .yp-shell.yp-out {
    opacity: 0;
    pointer-events: none;
  }

  /* Mini: no dimmed backdrop — developer keeps working underneath */
  .yp-shell.is-mini {
    inset: auto;
    padding: 0;
    background: none;
    opacity: 1;
    z-index: 10050;
    align-items: stretch;
    justify-content: stretch;
    pointer-events: none;
  }

  .yp-shell.is-mini .yp-dialog {
    pointer-events: auto;
    position: relative;
    width: 100%;
    max-height: none;
    margin: 0;
    opacity: 1;
    transform: none;
    transition: none;
  }

  .yp-dialog {
    /* Screen-aware width (see lib/youtubeSizing.ts): floor at the legacy
       1400px cap, grow toward the viewport on larger screens; the CSS clamps
       remain the hard bounds. */
    width: min(var(--yp-modal-w, 1400px), calc(100vw - 32px));
    max-height: calc(100vh - 24px);
    margin: auto;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
    border: 2px solid color-mix(in srgb, var(--border-color) 70%, var(--accent-primary) 30%);
    border-radius: var(--border-radius-xl, 12px);
    box-shadow:
      0 0 0 1px color-mix(in srgb, var(--accent-primary) 25%, transparent),
      0 24px 80px rgba(0, 0, 0, 0.55),
      var(--shadow-lg);
    overflow: hidden;
    transform-origin: center center;
    will-change: transform, opacity;
    opacity: 0;
    transform: translateY(18px) scale(0.92);
    transition:
      transform 0.42s cubic-bezier(0.22, 1, 0.36, 1),
      opacity 0.32s ease;
  }

  .yp-dialog.yp-in {
    opacity: 1;
    transform: translate(0, 0) scale(1);
  }

  .yp-dialog.yp-out {
    opacity: 0;
    transform: translateY(12px) scale(0.94);
  }

  .yp-dialog.mini {
    border-radius: var(--border-radius-lg, 14px);
    overflow: visible;
  }

  .yp-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border-color);
    background: color-mix(in srgb, var(--bg-elevated, var(--bg-tertiary)) 80%, var(--bg-secondary));
    flex-shrink: 0;
    border-radius: inherit;
    border-bottom-left-radius: 0;
    border-bottom-right-radius: 0;
  }

  .yp-header.draggable {
    cursor: grab;
    user-select: none;
    touch-action: none;
  }

  .yp-shell.dragging .yp-header.draggable {
    cursor: grabbing;
  }

  .yp-grip {
    display: inline-flex;
    color: var(--text-muted);
    flex-shrink: 0;
    opacity: 0.8;
  }

  .yp-title {
    margin: 0;
    flex: 1;
    min-width: 0;
    font-size: 14px;
    font-weight: 700;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .yp-dialog.mini .yp-title {
    font-size: 12px;
  }

  .yp-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .yp-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: var(--border-radius-md, 8px);
    border: 1px solid var(--border-color);
    background: var(--bg-elevated, var(--bg-primary));
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .yp-btn:hover {
    border-color: var(--accent-primary);
    color: var(--accent-primary);
  }

  .yp-btn :global(svg) {
    flex-shrink: 0;
  }

  .yp-icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: none;
    border-radius: var(--border-radius-md, 8px);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }

  .yp-icon-btn:hover {
    color: var(--text-primary);
    background: var(--bg-elevated, var(--bg-primary));
  }

  .yp-frame-wrap {
    position: relative;
    width: 100%;
    aspect-ratio: 16 / 9;
    max-height: calc(100vh - 96px);
    background: #000;
    flex: 1 1 auto;
    overflow: hidden;
    border-radius: inherit;
    border-top-left-radius: 0;
    border-top-right-radius: 0;
  }

  .yp-dialog.mini .yp-frame-wrap {
    max-height: none;
    flex: 0 0 auto;
    aspect-ratio: auto;
    border-radius: 0 0 var(--border-radius-lg, 14px) var(--border-radius-lg, 14px);
  }

  .yp-frame-wrap iframe {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    border: 0;
  }

  .yp-embed-tip {
    position: absolute;
    left: 50%;
    bottom: 12px;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-radius: var(--border-radius-md, 8px);
    border: 1px solid var(--border-color);
    background: color-mix(in srgb, var(--bg-secondary) 92%, #000);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.35);
    pointer-events: auto;
    z-index: 1;
    max-width: calc(100% - 24px);
  }

  .yp-embed-tip-text {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
  }

  .yp-embed-tip-btn {
    padding: 5px 8px;
    font-size: 12px;
  }

  .yp-resize {
    position: absolute;
    width: 14px;
    height: 14px;
    padding: 0;
    border: none;
    background: transparent;
    z-index: 2;
    touch-action: none;
  }

  .yp-resize.se {
    right: -4px;
    bottom: -4px;
    cursor: nwse-resize;
  }

  .yp-resize.sw {
    left: -4px;
    bottom: -4px;
    cursor: nesw-resize;
  }

  .yp-resize.ne {
    right: -4px;
    top: -4px;
    cursor: nesw-resize;
  }

  .yp-resize.nw {
    left: -4px;
    top: -4px;
    cursor: nwse-resize;
  }

  .yp-resize.se::after {
    content: "";
    position: absolute;
    right: 3px;
    bottom: 3px;
    width: 8px;
    height: 8px;
    border-right: 2px solid color-mix(in srgb, var(--accent-primary) 70%, var(--text-muted));
    border-bottom: 2px solid color-mix(in srgb, var(--accent-primary) 70%, var(--text-muted));
    border-radius: 0 0 2px 0;
    opacity: 0.85;
  }

  .yp-shell.resizing {
    user-select: none;
  }

  :global(.potato-pc) .yp-shell,
  :global(.potato-pc) .yp-dialog {
    transition: none !important;
  }

  @media (prefers-reduced-motion: reduce) {
    .yp-shell,
    .yp-dialog {
      transition: none !important;
    }
  }

  @media (max-width: 720px) {
    .yp-btn span {
      display: none;
    }
  }
</style>
