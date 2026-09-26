import { derived, writable } from "svelte/store";
import type { ChunkClipboard } from "./api";

export interface WorldMapClipboardState {
  clipboard: ChunkClipboard;
  sourceWorld: string;
  sourceDimension: string;
  copiedAt: number;
}

export const worldMapClipboard = writable<WorldMapClipboardState | null>(null);

export function setWorldMapClipboard(
  clipboard: ChunkClipboard,
  sourceWorld: string,
  sourceDimension: string,
): void {
  worldMapClipboard.set({
    clipboard,
    sourceWorld,
    sourceDimension,
    copiedAt: Date.now(),
  });
}

export function clearWorldMapClipboard(): void {
  worldMapClipboard.set(null);
}

export const hasWorldMapClipboard = derived(
  worldMapClipboard,
  ($c) => $c != null && ($c.clipboard.chunks?.length ?? 0) > 0,
);

export const worldMapClipboardCount = derived(
  worldMapClipboard,
  ($c) => $c?.clipboard.chunks?.length ?? 0,
);
