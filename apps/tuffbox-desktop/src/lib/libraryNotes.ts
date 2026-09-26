/**
 * Per-instance notes (Prism-style), persisted in localStorage keyed by the
 * instance path — same conventions as libraryGroups.ts.
 */

const NOTES_KEY = "tuffbox.library.notes";

export type NotesMap = Record<string, string>;

function readRaw(): NotesMap {
  try {
    const raw = typeof localStorage === "undefined" ? null : localStorage.getItem(NOTES_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const out: NotesMap = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v === "string") out[k] = v;
    }
    return out;
  } catch {
    return {};
  }
}

function writeRaw(notes: NotesMap) {
  try {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(NOTES_KEY, JSON.stringify(notes));
    }
  } catch {
    /* quota / private mode */
  }
}

export function loadNotes(): NotesMap {
  return readRaw();
}

export function getNote(notes: NotesMap, path: string): string {
  return notes[path] ?? "";
}

/** Saves the note (trimmed, newlines normalized); empty text removes the entry. */
export function setNote(notes: NotesMap, path: string, text: string): NotesMap {
  const normalized = text.replace(/\r\n/g, "\n").trim();
  const next: NotesMap = { ...notes };
  if (normalized) next[path] = normalized;
  else delete next[path];
  writeRaw(next);
  return next;
}
