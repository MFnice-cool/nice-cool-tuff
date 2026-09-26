<script lang="ts">
  import HeadAvatar from "./HeadAvatar.svelte";
  import { ChevronLeft, ChevronRight } from "@lucide/svelte";
  import type { AccountEntry } from "../lib/store";

  /**
   * Xbox-style account picker: a horizontal rail of skin heads steered with
   * the mouse wheel, arrow keys, or clicks. Navigation only moves a LOCAL
   * selection highlight; the real account switch happens on explicit confirm
   * (Enter or clicking the selected head). Escape cancels back to the active
   * account. The signed-in head wears a theme-colored glow rising from the
   * nick up through the head; a pending pick glows softer. The selected head
   * stays centered; neighbors scale down and fade, rail edges dim via mask.
   */
  let {
    accounts = [],
    skinPaths = {},
    activeUuid = null,
    busy = false,
    onswitch = (_uuid: string) => {},
  }: {
    accounts?: AccountEntry[];
    skinPaths?: Record<string, string | null | undefined>;
    activeUuid?: string | null;
    busy?: boolean;
    onswitch?: (uuid: string) => void;
  } = $props();

  const HEAD_W = 64; // rendered head size (canvas) — CSS scale handles the rest
  const SLOT_GAP = 18; // px between slots — wide enough for the rail to breathe
  const SLOT_W = HEAD_W + SLOT_GAP; // one step of the rail
  const WHEEL_STEP = 22; // accumulated deltaY per selection step
  const WHEEL_COOLDOWN_MS = 90; // min gap between wheel-driven steps

  let root = $state<HTMLDivElement | undefined>();
  let viewport = $state<HTMLDivElement | undefined>();
  let viewportW = $state(288);
  let index = $state(
    (() => {
      const i = accounts.findIndex((a) => a.uuid === activeUuid);
      return i >= 0 ? i : 0;
    })(),
  );
  let wheelAcc = $state(0);
  let lastStepAt = 0;
  let wheelResetTimer: ReturnType<typeof setTimeout> | undefined;

  const count = $derived(accounts.length);
  const activeIndex = $derived(accounts.findIndex((a) => a.uuid === activeUuid));
  const selected = $derived(accounts[index]);
  // A pick awaiting confirmation: selected head differs from the signed-in one.
  const pending = $derived(count > 1 && !!selected && selected.uuid !== activeUuid);

  // Follow external account switches (AccountManager, login modal, boot);
  // doubles as cancel when the world changes behind a pending pick.
  $effect(() => {
    index = activeIndex >= 0 ? activeIndex : 0;
  });

  // Keep the rail centered inside the panel as its width changes.
  $effect(() => {
    if (!viewport) return;
    const ro = new ResizeObserver(() => {
      viewportW = viewport?.clientWidth ?? 288;
    });
    ro.observe(viewport);
    return () => ro.disconnect();
  });

  const trackOffset = $derived(viewportW / 2 - HEAD_W / 2 - index * SLOT_W);

  function visual(i: number) {
    const d = Math.abs(i - index);
    const scale = d === 0 ? 1 : d === 1 ? 0.72 : d === 2 ? 0.55 : 0.42;
    const opacity = d === 0 ? 1 : d === 1 ? 0.82 : 0.4;
    return { scale, opacity };
  }

  /** Move the LOCAL selection; never touches the backend. */
  function select(i: number) {
    const next = Math.min(count - 1, Math.max(0, i));
    if (next === index || busy) return;
    index = next;
    // Focus the group so Enter confirms the pick instead of leaking to
    // whatever the user had focused before they started scrolling.
    root?.focus({ preventScroll: true });
  }

  function move(dir: number) {
    if (count <= 1) return;
    select(index + dir);
  }

  /** Explicit confirmation: the only path that fires the real switch. */
  function confirmSelection() {
    if (busy || !selected || selected.uuid === activeUuid) return;
    onswitch(selected.uuid);
  }

  function cancelSelection() {
    index = activeIndex >= 0 ? activeIndex : 0;
    wheelAcc = 0;
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    if (count <= 1 || busy) return;
    wheelAcc += e.deltaY;
    const now = performance.now();
    let guard = 0;
    while (Math.abs(wheelAcc) >= WHEEL_STEP && guard++ < 8) {
      if (now - lastStepAt < WHEEL_COOLDOWN_MS) break;
      lastStepAt = now;
      const dir = wheelAcc > 0 ? 1 : -1;
      wheelAcc -= dir * WHEEL_STEP;
      move(dir);
    }
    clearTimeout(wheelResetTimer);
    wheelResetTimer = setTimeout(() => {
      wheelAcc = 0;
    }, 260);
  }

  function onKeydown(e: KeyboardEvent) {
    if (count <= 1) return;
    if (e.key === "ArrowLeft") {
      e.preventDefault();
      move(-1);
    } else if (e.key === "ArrowRight") {
      e.preventDefault();
      move(1);
    } else if (e.key === "Enter") {
      // preventDefault stops a focused slot button from also firing click.
      e.preventDefault();
      confirmSelection();
    } else if (e.key === "Escape") {
      e.preventDefault();
      cancelSelection();
    }
  }

  /** First click picks a neighbor; clicking the picked head again confirms. */
  function onSlotClick(i: number) {
    if (busy || count <= 1) return;
    const account = accounts[i];
    if (!account || account.uuid === activeUuid) return;
    if (i === index) confirmSelection();
    else select(i);
  }
</script>

<!-- Carousel container: focusable group (WCAG carousel pattern); the user scrolls it
     with the mouse wheel and steers it with arrow keys. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="account-carousel"
  bind:this={root}
  tabindex="0"
  role="group"
  aria-label="Minecraft accounts"
  onwheel={onWheel}
  onkeydown={onKeydown}
>
  {#if count > 1}
    <button
      type="button"
      class="carousel-arrow left"
      aria-label="Previous account"
      disabled={busy || index === 0}
      onclick={() => move(-1)}
    >
      <ChevronLeft size={16} />
    </button>
    <button
      type="button"
      class="carousel-arrow right"
      aria-label="Next account"
      disabled={busy || index === count - 1}
      onclick={() => move(1)}
    >
      <ChevronRight size={16} />
    </button>
  {/if}

  <div class="carousel-viewport" bind:this={viewport}>
    <div class="carousel-track" style="gap: {SLOT_GAP}px; transform: translateX({trackOffset}px);">
      {#each accounts as account, i (account.uuid)}
        {@const v = visual(i)}
        <button
          type="button"
          class="carousel-slot"
          class:active={account.uuid === activeUuid}
          class:selected={i === index}
          disabled={busy}
          title={account.name}
          onclick={() => onSlotClick(i)}
          style="--scale: {v.scale}; --opacity: {v.opacity};"
        >
          <span class="slot-head" aria-hidden="true">
            <HeadAvatar skinSrc={skinPaths[account.uuid] ?? null} size={HEAD_W} alt={account.name} />
          </span>
          <span class="slot-name">{account.name}</span>
        </button>
      {/each}
    </div>
  </div>
</div>

<style>
  .account-carousel {
    position: relative;
    padding: 8px 0 6px;
    outline: none;
  }

  .account-carousel:focus-visible .carousel-viewport {
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent-primary) 55%, transparent);
  }

  .carousel-viewport {
    overflow: hidden;
    /* Dim the rail edges so heads melt into the panel sides. */
    -webkit-mask-image: linear-gradient(
      90deg,
      transparent 0%,
      #000 15%,
      #000 85%,
      transparent 100%
    );
    mask-image: linear-gradient(
      90deg,
      transparent 0%,
      #000 15%,
      #000 85%,
      transparent 100%
    );
    border-radius: var(--border-radius-sm);
    transition: box-shadow var(--motion-fast) var(--ease-out);
  }

  .carousel-track {
    display: flex;
    align-items: flex-start;
    width: max-content;
    will-change: transform;
    transition: transform var(--motion-med) var(--ease-spring);
  }

  .carousel-slot {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    width: 64px;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    cursor: pointer;
    opacity: var(--opacity);
    transform: scale(var(--scale));
    transform-origin: center top;
    transition:
      transform var(--motion-med) var(--ease-spring),
      opacity var(--motion-med) var(--ease-out);
  }

  .carousel-slot:disabled {
    cursor: default;
  }

  .slot-head {
    position: relative;
    width: 64px;
    height: 64px;
    border-radius: 6px;
    overflow: visible;
  }

  /* Signed-in head: layered theme-colored aura — a bright inner bloom rising
     from the nick + a wide soft outer halo — plus a delicate gradient ring
     that traces the head silhouette. Gentle float on the whole slot. */
  .carousel-slot.active .slot-head::before {
    content: "";
    position: absolute;
    inset: -16px -22px -12px;
    border-radius: 50%;
    pointer-events: none;
    background:
      radial-gradient(
        ellipse 52% 62% at 50% 104%,
        color-mix(in srgb, var(--accent-primary) 62%, transparent) 0%,
        color-mix(in srgb, var(--accent-primary) 26%, transparent) 44%,
        transparent 74%
      ),
      radial-gradient(
        ellipse 80% 80% at 50% 55%,
        color-mix(in srgb, var(--accent-secondary) 14%, transparent) 0%,
        transparent 68%
      );
    z-index: -1;
    filter: blur(3px);
    animation: carousel-aura-breathe 3.4s ease-in-out infinite;
  }

  /* Delicate 1px gradient ring hugging the head silhouette (accent → violet). */
  .carousel-slot.active .slot-head::after {
    content: "";
    position: absolute;
    inset: -2px;
    border-radius: var(--border-radius-sm);
    pointer-events: none;
    padding: 1.5px;
    background: linear-gradient(
      135deg,
      color-mix(in srgb, var(--accent-primary) 85%, transparent),
      color-mix(in srgb, var(--accent-secondary) 55%, transparent) 55%,
      color-mix(in srgb, var(--accent-primary) 25%, transparent)
    );
    -webkit-mask:
      linear-gradient(#000 0 0) content-box,
      linear-gradient(#000 0 0);
    mask:
      linear-gradient(#000 0 0) content-box,
      linear-gradient(#000 0 0);
    -webkit-mask-composite: xor;
    mask-composite: exclude;
    opacity: 0.9;
  }

  /* Pending pick: same aura, softer, plus a slow ring "breathing" pulse that
     reads as "waiting for Enter / second click". */
  .carousel-slot.selected:not(.active) .slot-head::before {
    content: "";
    position: absolute;
    inset: -12px -18px -24px;
    border-radius: 50%;
    pointer-events: none;
    background:
      radial-gradient(
        ellipse 54% 64% at 50% 104%,
        color-mix(in srgb, var(--accent-primary) 34%, transparent) 0%,
        transparent 74%
      );
    z-index: -1;
    filter: blur(3px);
  }

  .carousel-slot.selected:not(.active) .slot-head::after {
    content: "";
    position: absolute;
    inset: -2px;
    border-radius: var(--border-radius-sm);
    pointer-events: none;
    padding: 1.5px;
    background: color-mix(in srgb, var(--accent-primary) 45%, transparent);
    -webkit-mask:
      linear-gradient(#000 0 0) content-box,
      linear-gradient(#000 0 0);
    mask:
      linear-gradient(#000 0 0) content-box,
      linear-gradient(#000 0 0);
    -webkit-mask-composite: xor;
    mask-composite: exclude;
    animation: carousel-ring-pulse 2s ease-in-out infinite;
  }

  /* Active slot lifts slightly — the aura follows, reads as "raised". */
  .carousel-slot.active {
    transform: scale(var(--scale)) translateY(-2px);
  }

  /* Neighbor heads: gentle "come here" lift + brighten on hover. */
  .carousel-slot:not(.active):not(.selected):hover:not(:disabled) .slot-head {
    filter: brightness(1.12);
  }
  .carousel-slot:not(.active):not(.selected):hover:not(:disabled) .slot-name {
    color: var(--text-primary);
  }

  @keyframes carousel-aura-breathe {
    0%,
    100% {
      opacity: 0.85;
      transform: scale(1);
    }
    50% {
      opacity: 1;
      transform: scale(1.04);
    }
  }

  @keyframes carousel-ring-pulse {
    0%,
    100% {
      opacity: 0.35;
    }
    50% {
      opacity: 0.8;
    }
  }

  .slot-name {
    font-family: var(--font-minecraft);
    font-size: 12px;
    letter-spacing: 0.3px;
    /* Fit the full nick: no clipping, overflow allowed to spill over the
       narrow 76px slot, centered so it stays balanced. */
    max-width: none;
    width: max-content;
    white-space: nowrap;
    text-align: center;
    line-height: 1.4;
    display: inline-block;
    overflow: visible;
    color: var(--text-secondary);
    transition: color var(--motion-fast) var(--ease-out);
  }

  .carousel-slot.active .slot-name {
    /* Solid accent with a faint glow — gradient text fought with the head
       aura and read poorly over it. */
    color: var(--accent-primary);
    text-shadow: 0 0 8px color-mix(in srgb, var(--accent-primary) 28%, transparent);
  }

  /* Selected-but-not-confirmed nick: solid accent, no gradient drama yet. */
  .carousel-slot.selected:not(.active) .slot-name {
    color: var(--accent-primary);
    text-shadow: 0 0 8px color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }

  /* Neutralize the global button hover/active chrome on carousel slots —
     the white "button" highlight on the nick was the global button:hover. */
  .carousel-slot:hover:not(:disabled),
  .carousel-slot:active:not(:disabled) {
    background: none;
    border-color: transparent;
    margin-top: 0;
  }
  .carousel-slot:focus-visible {
    outline: 2px solid color-mix(in srgb, var(--accent-primary) 60%, transparent);
    outline-offset: 4px;
    border-radius: var(--border-radius-sm);
  }

  .carousel-arrow {
    position: absolute;
    top: 26px; /* head-row center: pad-top 8px + head 64/2 − arrow 28/2 */
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border-radius: var(--border-radius-sm);
    background: var(--bg-elevated);
    border: 1px solid var(--border-color);
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.45);
    color: var(--text-secondary);
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--ease-out),
      border-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      transform var(--motion-fast) var(--ease-spring);
  }

  .carousel-arrow.left { left: 8px; }
  .carousel-arrow.right { right: 8px; }

  .carousel-arrow:hover:not(:disabled) {
    border-color: var(--accent-primary);
    color: var(--accent-primary);
    transform: scale(1.1);
  }

  .carousel-arrow:disabled {
    opacity: 0.4;
    cursor: default;
  }

  /* Weak GPUs / reduced motion: snap instead of sliding, no aura animations. */
  :global(html.potato-pc) .carousel-track,
  :global(html.potato-pc) .carousel-slot {
    transition: none;
  }
  :global(html.potato-pc) .slot-head::before,
  :global(html.potato-pc) .slot-head::after,
  :global(html.potato-pc) .carousel-slot.active .slot-name {
    animation: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .carousel-track,
    .carousel-slot,
    .carousel-arrow {
      transition: none;
    }
    .slot-head::before,
    .slot-head::after {
      animation: none;
    }
  }
</style>