import { api } from "../../lib/api";

/** Shared item-icon data-URL cache for quest UI. */
const cache: Record<string, string | null> = {};
/** In-flight loads — do not write null into cache until settled. */
const inflight = new Map<string, Promise<string | null>>();

export function normalizeItemId(id: string | null | undefined): string | null {
  if (!id) return null;
  const t = id.trim();
  if (!t || t.startsWith("#")) return null;
  return t;
}

export function glyphFromItemId(id: string | null | undefined, fallback = "?"): string {
  const n = normalizeItemId(id);
  if (!n) return (fallback[0] || "?").toUpperCase();
  const leaf = n.includes(":") ? n.split(":").pop()! : n;
  return (leaf[0] || fallback[0] || "?").toUpperCase();
}

/** `undefined` = not loaded yet; `null` = loaded, no texture. */
export function getCachedIcon(id: string | null | undefined): string | null | undefined {
  const n = normalizeItemId(id);
  if (!n) return null;
  if (Object.prototype.hasOwnProperty.call(cache, n)) return cache[n];
  return undefined;
}

export function isIconPending(id: string | null | undefined): boolean {
  const n = normalizeItemId(id);
  return !!n && inflight.has(n);
}

export async function preloadItemIcons(
  ids: string[],
  projectPath: string,
): Promise<Record<string, string | null>> {
  const unique = [
    ...new Set(ids.map(normalizeItemId).filter((x): x is string => !!x)),
  ];

  const toFetch: string[] = [];
  const waiters: Promise<unknown>[] = [];

  for (const id of unique) {
    if (Object.prototype.hasOwnProperty.call(cache, id)) continue;
    const pending = inflight.get(id);
    if (pending) {
      waiters.push(pending);
      continue;
    }
    toFetch.push(id);
  }

  if (toFetch.length > 0) {
    const batchPromise = (async () => {
      try {
        for (let i = 0; i < toFetch.length; i += 48) {
          const chunk = toFetch.slice(i, i + 48);
          const batch = await api.recipes.itemIconsBatch(chunk, projectPath);
          for (const id of chunk) {
            cache[id] = batch[id] ?? null;
            inflight.delete(id);
          }
        }
      } catch {
        for (const id of toFetch) {
          cache[id] = null;
          inflight.delete(id);
        }
      }
    })();

    for (const id of toFetch) {
      inflight.set(
        id,
        batchPromise.then(() => cache[id] ?? null),
      );
    }
    waiters.push(batchPromise);
  }

  if (waiters.length) await Promise.all(waiters);

  const out: Record<string, string | null> = {};
  for (const raw of ids) {
    const n = normalizeItemId(raw);
    if (n && cache[n] !== undefined) out[n] = cache[n];
  }
  return out;
}
