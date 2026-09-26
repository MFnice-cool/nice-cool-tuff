/**
 * Per-instance mod-update counters for the library ("update center").
 *
 * The backend `check_mod_updates` call hits the Modrinth API, so counts are
 * checked lazily — when an instance is selected — and cached in localStorage
 * with a staleness window. Tiles/rows/side panel read the store; the instance
 * manager refreshes it after every check/update so the badge never lies.
 */

import { writable, get } from "svelte/store";
import { api } from "./api";

export interface UpdateCountEntry {
  count: number;
  /** Unix epoch ms of the last successful check. */
  checkedAt: number;
}

const KEY = "tuffbox.library.updateCounts";
/** Re-check when the cached entry is older than this. */
export const STALE_MS = 6 * 60 * 60 * 1000;

export type UpdateCountMap = Record<string, UpdateCountEntry>;

export function loadUpdateCounts(): UpdateCountMap {
  try {
    const raw = typeof localStorage === "undefined" ? null : localStorage.getItem(KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const out: UpdateCountMap = {};
    for (const [path, value] of Object.entries(parsed as Record<string, unknown>)) {
      if (
        value &&
        typeof value === "object" &&
        typeof (value as UpdateCountEntry).count === "number" &&
        Number.isFinite((value as UpdateCountEntry).count) &&
        typeof (value as UpdateCountEntry).checkedAt === "number"
      ) {
        out[path] = { count: (value as UpdateCountEntry).count, checkedAt: (value as UpdateCountEntry).checkedAt };
      }
    }
    return out;
  } catch {
    return {};
  }
}

function saveCache(map: UpdateCountMap) {
  try {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(KEY, JSON.stringify(map));
    }
  } catch {
    /* quota / private mode */
  }
}

export const updateCounts = writable<UpdateCountMap>(loadUpdateCounts());

/** True when the entry is missing or older than the staleness window. */
export function isStale(entry: UpdateCountEntry | undefined, now = Date.now()): boolean {
  if (!entry) return true;
  return now - entry.checkedAt > STALE_MS;
}

/** Publishes a fresh count (store + cache). Negative/invalid input clamps to 0. */
export function setUpdateCount(path: string, count: number): void {
  if (!path) return;
  const safe = Number.isFinite(count) && count > 0 ? Math.floor(count) : 0;
  updateCounts.update((map) => {
    const prev = map[path];
    if (prev && prev.count === safe && !isStale(prev)) return map;
    const next: UpdateCountMap = { ...map, [path]: { count: safe, checkedAt: Date.now() } };
    saveCache(next);
    return next;
  });
}

/** In-flight checks per path — dedupes selection flicker / double effects. */
const inflight = new Set<string>();

/**
 * Background check for one instance. Returns the fresh count, the cached
 * count when a check is already running, or null on failure (cache untouched).
 * No-op when the cached entry is still fresh unless `force`.
 */
export async function refreshUpdateCount(path: string, force = false): Promise<number | null> {
  if (!path) return null;
  const cached = get(updateCounts)[path];
  if (!force && !isStale(cached)) return cached?.count ?? 0;
  if (inflight.has(path)) return cached?.count ?? null;
  inflight.add(path);
  try {
    const list = await api.mods.checkUpdates(path);
    const count = Array.isArray(list) ? list.length : 0;
    setUpdateCount(path, count);
    return count;
  } catch {
    return null;
  } finally {
    inflight.delete(path);
  }
}

/** Drops one instance's counter (e.g. instance removed from the library). */
export function clearUpdateCount(path: string): void {
  updateCounts.update((map) => {
    if (!(path in map)) return map;
    const next = { ...map };
    delete next[path];
    saveCache(next);
    return next;
  });
}
