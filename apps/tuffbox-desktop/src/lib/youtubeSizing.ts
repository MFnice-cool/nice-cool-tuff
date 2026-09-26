/**
 * YouTube modal sizing — the video counterpart of the UI auto-scale
 * (`suggestUiScalePercent` in store.ts): same philosophy — derive the size
 * from the actual viewport instead of a fixed value, so users on different
 * screen resolutions get a sensibly filled player.
 *
 * The old modal was hard-capped at 1400 CSS px. On a 2560×1440 screen that is
 * ~55% of the width (less on 4K), which users read as "the player is too
 * small". The suggestion below keeps 1400 as the floor (nobody's player
 * shrinks) and lets the 16:9 stage grow toward the viewport on larger screens:
 *
 *   fit  = largest 16:9 stage that still fits (frame chrome + shell padding
 *          reserved), bounded by both viewport axes
 *   fill = fraction of `fit` actually used; rises in the same screen bands
 *          the UI auto-scale uses, so 1366×768 laptops keep today's behavior,
 *          1080p keeps the familiar 1400px dialog, and 1440p/4K get a
 *          proportionally larger stage.
 *
 * All numbers are CSS px. The app-wide UI zoom (`zoom` on <html>, applied by
 * `applyUiScale`) changes the CSS-px viewport itself, so the suggestion
 * automatically cooperates with the Interface scale setting: e.g. a 1440p
 * window at 125% Windows scaling reports a ~2048×1152 CSS viewport and gets
 * the band that fits it.
 */

/** Previous hard cap — kept as the floor so no existing user's player shrinks. */
export const YOUTUBE_MODAL_MIN_WIDTH = 1400;

/** Vertical chrome around the 16:9 stage: header bar + shell paddings
 * (matches `.yp-frame-wrap { max-height: calc(100vh - 96px) }`). */
const CHROME_HEIGHT = 96;

/** Horizontal shell padding reserved around the dialog (matches
 * `calc(100vw - 32px)` in the dialog width clamp). */
const SHELL_PADDING = 32;

/**
 * Suggested modal width (CSS px) for the given CSS-px viewport.
 * Always ≤ the viewport width bound, so the dialog never overflows.
 *
 * Below the legacy cap the result is byte-for-byte the old behavior
 * (`min(1400px, 100vw - 32px)` — the frame-wrap max-height then letterboxes
 * the stage exactly as before). Growth only kicks in via `fit * fill` on
 * viewports large enough for the 16:9 stage to have headroom.
 */
export function suggestYouTubeModalWidth(viewportW: number, viewportH: number): number {
  const vw = Math.max(320, Math.round(Number(viewportW) || 0));
  const vh = Math.max(240, Math.round(Number(viewportH) || 0));

  // Largest 16:9 stage that fits the viewport with chrome reserved,
  // bounded by both axes.
  const byHeight = Math.round(((vh - CHROME_HEIGHT) * 16) / 9);
  const byWidth = vw - SHELL_PADDING;
  const fit = Math.max(320, Math.min(byHeight, byWidth));

  // Fill factor per viewport width band (CSS px) — mirrors the spirit of
  // suggestUiScalePercent's bands: nothing changes below 1080p-class widths,
  // larger screens convert more of the headroom into stage size.
  const fill = vw >= 2560 ? 0.72 : vw >= 1920 ? 0.75 : 1;

  // Outer bound is the viewport width (the legacy behavior's bound), the
  // inner floor is the legacy cap — so shrinking is impossible.
  return Math.max(320, Math.round(Math.min(byWidth, Math.max(YOUTUBE_MODAL_MIN_WIDTH, fit * fill))));
}
