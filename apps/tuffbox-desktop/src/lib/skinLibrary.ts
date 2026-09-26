import type { CapeProvider } from "./store";

const STORAGE_KEY = "tuffbox.skinLibrary.v1";

export type SkinModelVariant = "classic" | "slim";

export interface SavedSkinCapeRef {
  provider: CapeProvider;
  id: string;
  label: string;
  url: string;
}

export interface SavedSkin {
  id: string;
  name: string;
  variant: SkinModelVariant;
  /** Absolute path to a PNG on disk (Tauri dialog). */
  filePath: string;
  cape: SavedSkinCapeRef | null;
  createdAt: number;
  updatedAt: number;
}

function canUseStorage(): boolean {
  return typeof localStorage !== "undefined";
}

function readAll(): SavedSkin[] {
  if (!canUseStorage()) return [];
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as SavedSkin[];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function writeAll(entries: SavedSkin[]) {
  if (!canUseStorage()) return;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(entries));
}

export function listSavedSkins(): SavedSkin[] {
  return readAll().sort((a, b) => b.updatedAt - a.updatedAt);
}

export function getSavedSkin(id: string): SavedSkin | null {
  return readAll().find((s) => s.id === id) ?? null;
}

export function upsertSavedSkin(
  input: Omit<SavedSkin, "id" | "createdAt" | "updatedAt"> & { id?: string },
): SavedSkin {
  const now = Date.now();
  const all = readAll();
  const existingIdx = input.id ? all.findIndex((s) => s.id === input.id) : -1;
  const entry: SavedSkin = {
    id: input.id ?? crypto.randomUUID(),
    name: input.name.trim() || "Untitled",
    variant: input.variant,
    filePath: input.filePath,
    cape: input.cape,
    createdAt: existingIdx >= 0 ? all[existingIdx].createdAt : now,
    updatedAt: now,
  };
  if (existingIdx >= 0) all[existingIdx] = entry;
  else all.unshift(entry);
  writeAll(all.slice(0, 64));
  return entry;
}

export function updateSavedSkinVariant(id: string, variant: SkinModelVariant): SavedSkin | null {
  const all = readAll();
  const idx = all.findIndex((s) => s.id === id);
  if (idx < 0) return null;
  all[idx] = { ...all[idx], variant, updatedAt: Date.now() };
  writeAll(all);
  return all[idx];
}

export function duplicateSavedSkin(id: string): SavedSkin | null {
  const src = getSavedSkin(id);
  if (!src) return null;
  return upsertSavedSkin({
    name: `${src.name} copy`,
    variant: src.variant,
    filePath: src.filePath,
    cape: src.cape,
  });
}

export function removeSavedSkin(id: string) {
  writeAll(readAll().filter((s) => s.id !== id));
}

/** True if this file path is already stored in the library. */
export function libraryHasFilePath(filePath: string): boolean {
  const norm = filePath.replace(/\\/g, "/").toLowerCase();
  return readAll().some((s) => s.filePath.replace(/\\/g, "/").toLowerCase() === norm);
}
