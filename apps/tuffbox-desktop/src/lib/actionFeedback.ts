/**
 * Calm action feedback — soft ack after a click settles.
 * Hover delay lives in CSS; this only handles the post-press pulse.
 * No-ops under potato-pc / prefers-reduced-motion.
 */

const TARGET =
  'button, [role="button"], .chip, .rail-btn, .action-btn, .press-effect, .tb-press, .theme-swatch, .tb-card, .quiet-action, .primary-action';

const ACK_CLASS = "tb-acked";
const ACK_START_MS = 55;
const ACK_ANIM = "tb-action-ack";
const ACK_ANIM_RAIL = "tb-action-ack-rail";
const DRAG_PX = 8;

function motionBlocked(): boolean {
  if (typeof document === "undefined") return true;
  if (document.documentElement.classList.contains("potato-pc")) return true;
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function installActionFeedback(): () => void {
  let lastAt = 0;
  let downX = 0;
  let downY = 0;
  const timers = new WeakMap<Element, number>();

  const onPointerDown = (e: PointerEvent) => {
    if (e.button !== 0) return;
    downX = e.clientX;
    downY = e.clientY;
  };

  const onPointerUp = (e: PointerEvent) => {
    if (e.button !== 0 || motionBlocked()) return;
    const raw = e.target;
    if (!(raw instanceof Element)) return;
    const el = raw.closest(TARGET);
    if (!(el instanceof HTMLElement)) return;
    if (el.hasAttribute("disabled") || el.getAttribute("aria-disabled") === "true") return;
    if (Math.hypot(e.clientX - downX, e.clientY - downY) > DRAG_PX) return;

    const now = performance.now();
    if (now - lastAt < 90) return;
    lastAt = now;

    const prev = timers.get(el);
    if (prev != null) window.clearTimeout(prev);
    el.classList.remove(ACK_CLASS);

    const id = window.setTimeout(() => {
      timers.delete(el);
      if (!el.isConnected || motionBlocked()) return;
      el.classList.remove(ACK_CLASS);
      void el.offsetWidth;
      el.classList.add(ACK_CLASS);
    }, ACK_START_MS);
    timers.set(el, id);
  };

  const onAnimEnd = (e: AnimationEvent) => {
    if (!(e.target instanceof Element)) return;
    if (e.animationName !== ACK_ANIM && e.animationName !== ACK_ANIM_RAIL) return;
    e.target.classList.remove(ACK_CLASS);
  };

  document.addEventListener("pointerdown", onPointerDown, true);
  document.addEventListener("pointerup", onPointerUp, true);
  document.addEventListener("animationend", onAnimEnd, true);

  return () => {
    document.removeEventListener("pointerdown", onPointerDown, true);
    document.removeEventListener("pointerup", onPointerUp, true);
    document.removeEventListener("animationend", onAnimEnd, true);
  };
}
