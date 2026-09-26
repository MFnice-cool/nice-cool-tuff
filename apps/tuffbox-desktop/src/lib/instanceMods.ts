/**
 * Pure helpers for the instance manager's Mods tab — filtering, sorting,
 * update merging and formatting. No Tauri/UI imports so the rules stay
 * unit-testable in the node vitest project.
 */

export interface ModRow {
  id: string;
  name: string;
  version: string;
  side?: string | null;
  source?: string | null;
  fileName?: string | null;
  contentType?: string | null;
  disabled?: boolean;
}

export interface ModUpdateInfo {
  modId: string;
  latestVersion: string;
  versionId?: string | null;
  fileName?: string | null;
  breakingLoader?: boolean;
  breakingMinecraft?: boolean;
  changelog?: string | null;
}

export type ModSortMode = "name" | "source" | "updates";

/** Human label for a manifest source kind ("modrinth" → "Modrinth"). */
export function sourceChip(source?: string | null): string {
  const s = (source ?? "").trim().toLowerCase();
  if (!s) return "Local";
  if (s === "modrinth") return "Modrinth";
  if (s === "curseforge") return "CurseForge";
  if (s === "local") return "Local";
  return s[0].toUpperCase() + s.slice(1);
}

/** Case-insensitive filter over display name, file name and version. */
export function matchesModFilter(
  mod: Pick<ModRow, "name" | "fileName" | "version">,
  query: string,
): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return (
    mod.name.toLowerCase().includes(q) ||
    (mod.fileName ?? "").toLowerCase().includes(q) ||
    mod.version.toLowerCase().includes(q)
  );
}

/**
 * Sorted copy. "updates" floats rows with an available update to the top;
 * everything else falls back to name order so the list never jumps randomly.
 */
export function sortMods<T extends ModRow & { hasUpdate?: boolean }>(
  rows: T[],
  mode: ModSortMode,
): T[] {
  const arr = [...rows];
  if (mode === "updates") {
    arr.sort(
      (a, b) => Number(!!b.hasUpdate) - Number(!!a.hasUpdate) || a.name.localeCompare(b.name),
    );
  } else if (mode === "source") {
    arr.sort(
      (a, b) => sourceChip(a.source).localeCompare(sourceChip(b.source)) || a.name.localeCompare(b.name),
    );
  } else {
    arr.sort((a, b) => a.name.localeCompare(b.name));
  }
  return arr;
}

/** modId → update info, normalized from the raw check_mod_updates payload. */
export function mergeUpdates(
  rows: Array<Record<string, unknown>>,
): Record<string, ModUpdateInfo> {
  const map: Record<string, ModUpdateInfo> = {};
  for (const raw of Array.isArray(rows) ? rows : []) {
    if (!raw || typeof raw !== "object") continue;
    const modId = typeof raw.modId === "string" ? raw.modId : "";
    if (!modId) continue;
    map[modId] = {
      modId,
      latestVersion: typeof raw.latestVersion === "string" ? raw.latestVersion : "",
      versionId: typeof raw.versionId === "string" ? raw.versionId : null,
      fileName: typeof raw.fileName === "string" ? raw.fileName : null,
      breakingLoader: raw.breakingLoader === true,
      breakingMinecraft: raw.breakingMinecraft === true,
      changelog: typeof raw.changelog === "string" ? raw.changelog : null,
    };
  }
  return map;
}

/** "→ 1.2.3 (breaking)" style badge text for an available update. */
export function updateLabel(update?: ModUpdateInfo | null): string {
  if (!update) return "";
  const breaking = update.breakingLoader || update.breakingMinecraft;
  return `→ ${update.latestVersion}${breaking ? " (breaking)" : ""}`;
}

/** Byte size for humans; tolerates null/undefined/negative input. */
export function formatBytes(bytes?: number | null): string {
  if (bytes == null || !Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let i = 0;
  while (value >= 1024 && i < units.length - 1) {
    value /= 1024;
    i++;
  }
  return `${value >= 100 ? value.toFixed(0) : value.toFixed(1)} ${units[i]}`;
}

/** Locale day stamp ("Mar 4, 2026") for worlds/screenshots; "never" when absent. */
export function formatDayStamp(ms?: number | null): string {
  if (ms == null || !Number.isFinite(ms) || ms <= 0) return "never";
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return "never";
  return d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

/** Locale timestamp for backup entries; "—" when missing/unparseable. */
export function formatStamp(iso?: string | null): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export interface VersionOption {
  id: string;
  label: string;
}

/**
 * Normalizes the loosely-typed get_mod_versions payload into select options
 * ("1.2.3 · beta · 2024-05-01"), marking the currently installed number.
 */
export function versionOptions(
  list: Record<string, unknown>[],
  currentVersion: string,
): VersionOption[] {
  const out: VersionOption[] = [];
  for (const v of Array.isArray(list) ? list : []) {
    if (!v || typeof v !== "object") continue;
    const id = String(v.id ?? v.version_id ?? "");
    if (!id) continue;
    const num = String(v.version_number ?? v.number ?? v.name ?? id);
    const type = typeof v.version_type === "string" ? v.version_type : "";
    const date =
      typeof v.date_published === "string" ? v.date_published.slice(0, 10) : "";
    const parts = [num, type, date].filter(Boolean).join(" · ");
    out.push({ id, label: num === currentVersion ? `${parts} (current)` : parts });
  }
  return out;
}
