import { describe, expect, it } from "vitest";
import {
  suggestYouTubeModalWidth,
  YOUTUBE_MODAL_MIN_WIDTH,
} from "./youtubeSizing";

/** Largest 16:9 stage that fits the viewport with chrome reserved. */
function fitWidth(vw: number, vh: number): number {
  return Math.min(Math.round(((vh - 96) * 16) / 9), vw - 32);
}

/** Old behavior: flat 1400px cap over the viewport width bound. */
function legacyWidth(vw: number): number {
  return Math.min(YOUTUBE_MODAL_MIN_WIDTH, vw - 32);
}

describe("suggestYouTubeModalWidth", () => {
  it("is identical to the legacy width when height-bound or well under the cap", () => {
    // Width-bound viewports and viewports whose 16:9 fit is under the legacy
    // cap get exactly the old width — the fix only ever grows the player.
    for (const [vw, vh] of [
      [800, 600],
      [1024, 640],
      [1280, 720],
      [1366, 768],
      [1518, 853], // 1366×768 at 90% interface zoom
    ] as const) {
      expect(suggestYouTubeModalWidth(vw, vh)).toBe(legacyWidth(vw));
    }
  });

  it("removes legacy pillarboxing without shrinking the video", () => {
    // 1600×900 / 2560×1440@150%: legacy gave 1400px with the frame clamped to
    // vh-96 (dead pillarbox bars); the video inside was already vh-bound. The
    // suggestion widens the dialog to exactly that stage — same video size,
    // no dead bars, dialog never below the legacy cap.
    expect(suggestYouTubeModalWidth(1600, 900)).toBe(1429);
    expect(suggestYouTubeModalWidth(1707, 960)).toBe(1536);
    expect(suggestYouTubeModalWidth(1707, 960)).toBeGreaterThanOrEqual(1400);
  });

  it("keeps 1080p at the familiar 1400px dialog", () => {
    expect(suggestYouTubeModalWidth(1920, 1080)).toBe(1400);
  });

  it("grows the stage on high-resolution screens", () => {
    // 2560×1440 (the reported case): well beyond the old 1400 cap.
    const wqhd = suggestYouTubeModalWidth(2560, 1440);
    expect(wqhd).toBe(1720);
    expect(wqhd).toBeGreaterThan(1400);
    // 4K grows further still.
    const uhd = suggestYouTubeModalWidth(3840, 2160);
    expect(uhd).toBe(2642);
    expect(uhd).toBeGreaterThan(wqhd);
    // Ultrawide stays height-bound, not width-bound.
    expect(suggestYouTubeModalWidth(3440, 1440)).toBe(1720);
  });

  it("grows only where the 16:9 stage has headroom", () => {
    // Beyond the legacy cap the stage (width * 9/16 + chrome) must fit the
    // viewport height — growth converts spare height, never overflows.
    for (const [vw, vh] of [
      [2560, 1440],
      [3440, 1440],
      [3840, 2160],
      [2200, 1200],
    ] as const) {
      const w = suggestYouTubeModalWidth(vw, vh);
      if (w > YOUTUBE_MODAL_MIN_WIDTH) {
        expect(w * (9 / 16)).toBeLessThanOrEqual(vh - 96 + 0.5);
      }
    }
  });

  it("always fits the viewport width", () => {
    const viewports: [number, number][] = [
      [640, 480],
      [500, 2000], // narrow window
      [800, 300], // short window
      [1366, 768],
      [1920, 1080],
      [2560, 1440],
      [3840, 2160],
    ];
    for (const [vw, vh] of viewports) {
      const w = suggestYouTubeModalWidth(vw, vh);
      expect(w).toBeLessThanOrEqual(vw - 32);
      expect(Number.isFinite(w)).toBe(true);
      expect(w).toBeGreaterThan(0);
    }
    // The degenerate 320px floor only guards absurd inputs; the CSS
    // `min(…, calc(100vw - 32px))` clamp remains the hard bound there.
    expect(suggestYouTubeModalWidth(320, 240)).toBeLessThanOrEqual(320);
  });

  it("adapts to the interface-scale zoom via the CSS-px viewport", () => {
    // Same physical 2560×1440 screen: at 150% scaling the CSS viewport is
    // ~1707×960 — the dialog widens only to the un-pillarboxed stage; at 125%
    // (~2048×1152) it takes the first growth band.
    expect(suggestYouTubeModalWidth(1707, 960)).toBe(1536);
    expect(suggestYouTubeModalWidth(2048, 1152)).toBe(1408);
  });

  it("degrades safely on degenerate input", () => {
    expect(suggestYouTubeModalWidth(0, 0)).toBeGreaterThan(0);
    expect(suggestYouTubeModalWidth(NaN, NaN)).toBeGreaterThan(0);
    expect(Number.isFinite(suggestYouTubeModalWidth(-100, -100))).toBe(true);
  });
});
