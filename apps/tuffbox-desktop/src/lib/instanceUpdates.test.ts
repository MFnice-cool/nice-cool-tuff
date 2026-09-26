import { afterEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import type { UpdateCountMap } from "./instanceUpdates";

// instanceUpdates pulls in api.ts -> store.ts, which touches localStorage at
// module scope — install the stub via vi.hoisted so it exists before those
// hoisted imports execute.
const storage = vi.hoisted(() => {
  class MemStorage {
    private m = new Map<string, string>();
    getItem(k: string) {
      return this.m.has(k) ? (this.m.get(k) as string) : null;
    }
    setItem(k: string, v: string) {
      this.m.set(k, String(v));
    }
    removeItem(k: string) {
      this.m.delete(k);
    }
  }
  const s = new MemStorage();
  (globalThis as { localStorage?: unknown }).localStorage = s;
  // store.ts applies the theme at module scope; give it a minimal document.
  (globalThis as { document?: unknown }).document = {
    documentElement: { setAttribute() {}, removeAttribute() {} },
  };
  return s;
});

const { clearUpdateCount, isStale, loadUpdateCounts, setUpdateCount, updateCounts } =
  await import("./instanceUpdates");

const KEY = "tuffbox.library.updateCounts";

afterEach(() => {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* ignore */
  }
  updateCounts.set({});
});

describe("isStale", () => {
  it("missing entries are stale", () => {
    expect(isStale(undefined)).toBe(true);
  });
  it("fresh entries within the window are not stale", () => {
    expect(isStale({ count: 2, checkedAt: Date.now() - 1000 })).toBe(false);
  });
  it("entries past the window are stale", () => {
    expect(isStale({ count: 2, checkedAt: Date.now() - 7 * 60 * 60 * 1000 })).toBe(true);
  });
});

describe("setUpdateCount / loadUpdateCounts round-trip", () => {
  it("publishes to the store and persists", () => {
    setUpdateCount("/packs/a", 3);
    expect(get(updateCounts)["/packs/a"].count).toBe(3);
    expect(loadUpdateCounts()["/packs/a"].count).toBe(3);
  });

  it("clamps invalid counts to 0", () => {
    setUpdateCount("/packs/b", -4);
    setUpdateCount("/packs/c", Number.NaN);
    expect(get(updateCounts)["/packs/b"].count).toBe(0);
    expect(get(updateCounts)["/packs/c"].count).toBe(0);
  });

  it("ignores empty paths", () => {
    setUpdateCount("", 5);
    expect(Object.keys(get(updateCounts))).toHaveLength(0);
  });

  it("keeps the checkedAt fresh on rewrite", () => {
    setUpdateCount("/packs/a", 1);
    const first = get(updateCounts)["/packs/a"].checkedAt ?? 0;
    setUpdateCount("/packs/a", 4);
    const second = get(updateCounts)["/packs/a"].checkedAt ?? 0;
    expect(second).toBeGreaterThanOrEqual(first);
    expect(get(updateCounts)["/packs/a"].count).toBe(4);
  });
});

describe("clearUpdateCount", () => {
  it("removes the entry from store and cache", () => {
    setUpdateCount("/packs/a", 2);
    clearUpdateCount("/packs/a");
    expect(get(updateCounts)["/packs/a"]).toBeUndefined();
    expect(loadUpdateCounts()["/packs/a"]).toBeUndefined();
  });

  it("no-ops for unknown paths", () => {
    const before: UpdateCountMap = { ...get(updateCounts) };
    clearUpdateCount("/nope");
    expect(get(updateCounts)).toEqual(before);
  });
});

describe("loadUpdateCounts robustness", () => {
  it("drops corrupt payloads and non-conforming entries", () => {
    try {
      localStorage.setItem(KEY, JSON.stringify({ a: { count: 1, checkedAt: 5 }, bad: "x", n: null }));
    } catch {
      /* ignore */
    }
    const loaded = loadUpdateCounts();
    expect(loaded.a).toEqual({ count: 1, checkedAt: 5 });
    expect(loaded.bad).toBeUndefined();
    expect(loaded.n).toBeUndefined();
  });

  it("returns an empty map for unparseable storage", () => {
    try {
      localStorage.setItem(KEY, "{oops");
    } catch {
      /* ignore */
    }
    expect(loadUpdateCounts()).toEqual({});
  });
});
