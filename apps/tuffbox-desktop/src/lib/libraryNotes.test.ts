import { afterEach, describe, expect, it } from "vitest";
import { getNote, loadNotes, setNote, type NotesMap } from "./libraryNotes";

// Minimal localStorage for the node vitest project (libraryNotes reads it
// lazily, so installing the stub at module scope is enough).
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
const storage = new MemStorage();
(globalThis as { localStorage?: unknown }).localStorage = storage;

const KEY = "tuffbox.library.notes";

afterEach(() => {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* ignore */
  }
});

describe("libraryNotes", () => {
  it("starts empty and round-trips through localStorage", () => {
    expect(loadNotes()).toEqual({});
    let notes: NotesMap = {};
    notes = setNote(notes, "/packs/a", "  Run on Java 21\r\nbackup weekly  ");
    expect(getNote(notes, "/packs/a")).toBe("Run on Java 21\nbackup weekly");
    expect(loadNotes()).toEqual({ "/packs/a": "Run on Java 21\nbackup weekly" });
  });

  it("removes the entry when the text becomes empty", () => {
    let notes: NotesMap = {};
    notes = setNote(notes, "/packs/a", "hello");
    notes = setNote(notes, "/packs/a", "   ");
    expect(notes).toEqual({});
    expect(loadNotes()).toEqual({});
  });

  it("does not mutate the previous map object", () => {
    const before: NotesMap = { "/packs/b": "keep" };
    const next = setNote(before, "/packs/c", "new");
    expect(before).toEqual({ "/packs/b": "keep" });
    expect(next["/packs/c"]).toBe("new");
  });

  it("ignores corrupt storage payloads", () => {
    try {
      localStorage.setItem(KEY, "{oops");
    } catch {
      /* ignore */
    }
    expect(loadNotes()).toEqual({});
  });

  it("drops non-string values from storage", () => {
    try {
      localStorage.setItem(KEY, JSON.stringify({ a: "ok", b: 42, c: null }));
    } catch {
      /* ignore */
    }
    expect(loadNotes()).toEqual({ a: "ok" });
  });
});
