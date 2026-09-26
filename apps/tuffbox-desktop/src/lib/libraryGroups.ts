/** Library instance groups (Prism-style), persisted in localStorage. */

export const DEFAULT_GROUP = "No group";

const GROUPS_KEY = "tuffbox.library.groups";
const COLLAPSED_KEY = "tuffbox.library.collapsedGroups";

export type GroupMap = Record<string, string>;

function readJson<T>(key: string, fallback: T): T {
  try {
    const raw = typeof window !== "undefined" ? localStorage.getItem(key) : null;
    if (!raw) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

function writeJson(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* quota / private mode */
  }
}

export function loadGroupMap(): GroupMap {
  const map = readJson<GroupMap>(GROUPS_KEY, {});
  return map && typeof map === "object" ? map : {};
}

export function saveGroupMap(map: GroupMap) {
  writeJson(GROUPS_KEY, map);
}

export function getGroup(map: GroupMap, path: string): string {
  const g = map[path]?.trim();
  return g || DEFAULT_GROUP;
}

export function setGroup(map: GroupMap, path: string, groupName: string): GroupMap {
  const name = groupName.trim() || DEFAULT_GROUP;
  const next = { ...map };
  if (name === DEFAULT_GROUP) delete next[path];
  else next[path] = name;
  saveGroupMap(next);
  return next;
}

export function loadCollapsedGroups(): Set<string> {
  const arr = readJson<string[]>(COLLAPSED_KEY, []);
  return new Set(Array.isArray(arr) ? arr : []);
}

export function saveCollapsedGroups(collapsed: Set<string>) {
  writeJson(COLLAPSED_KEY, [...collapsed]);
}

export function toggleCollapsed(collapsed: Set<string>, groupName: string): Set<string> {
  const next = new Set(collapsed);
  if (next.has(groupName)) next.delete(groupName);
  else next.add(groupName);
  saveCollapsedGroups(next);
  return next;
}

/** Unique group names from map + projects (always includes DEFAULT_GROUP). */
export function listGroupNames(map: GroupMap, paths: string[]): string[] {
  const names = new Set<string>([DEFAULT_GROUP]);
  for (const path of paths) names.add(getGroup(map, path));
  for (const g of Object.values(map)) {
    const t = g?.trim();
    if (t) names.add(t);
  }
  return [...names].sort((a, b) => {
    if (a === DEFAULT_GROUP) return -1;
    if (b === DEFAULT_GROUP) return 1;
    return a.localeCompare(b);
  });
}

/** Unique folder name derived from a base (Android-style drop onto another tile). */
export function suggestFolderName(map: GroupMap, baseName: string): string {
  const cleaned = (baseName.trim() || "Folder").slice(0, 48);
  const used = new Set(
    Object.values(map)
      .map((g) => g?.trim())
      .filter(Boolean),
  );
  if (!used.has(cleaned) && cleaned !== DEFAULT_GROUP) return cleaned;
  for (let i = 2; i < 100; i++) {
    const candidate = `${cleaned} ${i}`;
    if (!used.has(candidate)) return candidate;
  }
  return `${cleaned} ${Date.now()}`;
}

/**
 * Drop source instance onto target (Android home-screen folder UX):
 * - target already in a named group → move source into that group
 * - otherwise create a new folder named after the target and put both in it
 */
export function folderFromDrop(
  map: GroupMap,
  sourcePath: string,
  targetPath: string,
  targetDisplayName: string,
): { map: GroupMap; groupName: string; created: boolean } | null {
  if (!sourcePath || !targetPath || sourcePath === targetPath) return null;

  const targetGroup = getGroup(map, targetPath);
  if (targetGroup !== DEFAULT_GROUP) {
    const sourceGroup = getGroup(map, sourcePath);
    if (sourceGroup === targetGroup) return null;
    return {
      map: setGroup(map, sourcePath, targetGroup),
      groupName: targetGroup,
      created: false,
    };
  }

  const folderName = suggestFolderName(map, targetDisplayName);
  let next = setGroup(map, targetPath, folderName);
  next = setGroup(next, sourcePath, folderName);
  return { map: next, groupName: folderName, created: true };
}
