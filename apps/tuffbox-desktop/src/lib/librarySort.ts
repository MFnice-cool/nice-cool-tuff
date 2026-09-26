/** Pure filter + sort logic for the Library instance grid (unit-tested). */
import type { RecentProject } from "./store";
import type { HomeStatsBrief } from "./homeBootstrap";

export type SortMode = "recent" | "name" | "playtime";

/** Live instance filter: matches name, Minecraft version or loader. */
export function matchesInstanceFilter(
  p: RecentProject,
  rawQuery: string,
  /** When set, the group name is searchable too ("server" finds its group). */
  groupName?: string | null,
): boolean {
  const q = rawQuery.trim().toLowerCase();
  if (!q) return true;
  return (
    p.info.name.toLowerCase().includes(q) ||
    p.info.minecraftVersion.toLowerCase().includes(q) ||
    p.info.loaderKind.toLowerCase().includes(q) ||
    (!!groupName && groupName.toLowerCase().includes(q))
  );
}

function lastLaunchMs(stats: Record<string, HomeStatsBrief> | undefined, path: string): number {
  const t = stats?.[path]?.lastLaunch;
  if (!t) return 0;
  const ms = Date.parse(t);
  return Number.isNaN(ms) ? 0 : ms;
}

/**
 * Sort projects inside a group.
 * - "recent": last played first (store order as tiebreaker via stable sort)
 * - "name": alphabetical
 * - "playtime": most played first
 */
export function sortInstances(
  list: RecentProject[],
  mode: SortMode,
  stats?: Record<string, HomeStatsBrief>,
): RecentProject[] {
  const arr = [...list];
  if (mode === "name") {
    arr.sort((a, b) => a.info.name.localeCompare(b.info.name));
  } else if (mode === "playtime") {
    arr.sort((a, b) => (stats?.[b.path]?.playtime ?? 0) - (stats?.[a.path]?.playtime ?? 0));
  } else {
    arr.sort((a, b) => lastLaunchMs(stats, b.path) - lastLaunchMs(stats, a.path));
  }
  return arr;
}

export function isValidSortMode(v: unknown): v is SortMode {
  return v === "recent" || v === "name" || v === "playtime";
}
