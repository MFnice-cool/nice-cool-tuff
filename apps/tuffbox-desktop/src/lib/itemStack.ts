/**
 * FTB Quests item values: plain id string, stack compound, or itemfilters:*.
 * Helpers always merge into existing objects — never flatten filters to a string.
 */

export type ItemValue = string | Record<string, unknown>;

export type ItemEditMode = "simple" | "stack" | "filter_or" | "filter_and" | "filter_tag";

const FILTER_PREFIX = "itemfilters:";

export function isItemObject(v: unknown): v is Record<string, unknown> {
  return !!v && typeof v === "object" && !Array.isArray(v);
}

export function isFilterCompound(v: ItemValue | null | undefined): boolean {
  if (!isItemObject(v)) return false;
  const id = typeof v.id === "string" ? v.id : "";
  return id.startsWith(FILTER_PREFIX);
}

export function filterKind(
  v: ItemValue | null | undefined,
): "or" | "and" | "tag" | "other" | null {
  if (!isFilterCompound(v) || !isItemObject(v)) return null;
  const id = String(v.id);
  if (id === "itemfilters:or") return "or";
  if (id === "itemfilters:and") return "and";
  if (id === "itemfilters:tag") return "tag";
  return "other";
}

/** Display id for UI glyphs / preload (first concrete id when filter). */
export function stackDisplayId(v: ItemValue | null | undefined): string | null {
  if (v == null) return null;
  if (typeof v === "string") {
    const t = v.trim();
    return t || null;
  }
  if (!isItemObject(v)) return null;
  const id = v.id;
  if (typeof id === "string" && id.trim() && !id.startsWith(FILTER_PREFIX)) {
    return id.trim();
  }
  for (const child of listFilterChildren(v)) {
    const d = stackDisplayId(child);
    if (d && !d.startsWith("#")) return d;
  }
  if (typeof id === "string" && id.trim()) return id.trim();
  const item = v.item;
  if (typeof item === "string" && item.trim()) return item.trim();
  return null;
}

export function detectMode(v: ItemValue | null | undefined): ItemEditMode {
  if (v == null || v === "") return "simple";
  if (typeof v === "string") return "simple";
  const kind = filterKind(v);
  if (kind === "or") return "filter_or";
  if (kind === "and") return "filter_and";
  if (kind === "tag") return "filter_tag";
  if (kind === "other") return "stack";
  // Plain stack object { id, Count, tag, ... }
  return "stack";
}

export function readCount(v: ItemValue | null | undefined, fallback = 1): number {
  if (!isItemObject(v)) return fallback;
  const c = v.Count ?? v.count;
  if (typeof c === "number" && Number.isFinite(c)) return c;
  if (typeof c === "string" && c !== "" && !Number.isNaN(Number(c))) return Number(c);
  return fallback;
}

/** Ensure object form; strings become `{ id }`. Does not wrap filters. */
export function asStackObject(v: ItemValue | null | undefined): Record<string, unknown> {
  if (isItemObject(v)) return { ...v };
  if (typeof v === "string" && v.trim()) return { id: v.trim() };
  return { id: "minecraft:stone" };
}

export function setStackId(v: ItemValue | null | undefined, id: string): ItemValue {
  const trimmed = id.trim();
  if (typeof v === "string" || v == null) {
    return trimmed;
  }
  if (isFilterCompound(v)) {
    // Don't overwrite filter id with a picked item — callers should edit children.
    return v;
  }
  const next = { ...v };
  next.id = trimmed || "minecraft:stone";
  return next;
}

export function setStackCount(v: ItemValue | null | undefined, count: number): ItemValue {
  const n = Number.isFinite(count) && count > 0 ? count : 1;
  const obj = asStackObject(v);
  if (isFilterCompound(obj)) return obj;
  obj.Count = n;
  delete obj.count;
  return obj;
}

/** Read nested items from itemfilters:or / and. */
export function listFilterChildren(v: Record<string, unknown>): ItemValue[] {
  const tag = isItemObject(v.tag) ? v.tag : null;
  const raw = tag?.items ?? v.items;
  if (!Array.isArray(raw)) return [];
  return raw.filter(
    (x): x is ItemValue => typeof x === "string" || isItemObject(x),
  );
}

export function setFilterChildren(
  v: Record<string, unknown>,
  children: ItemValue[],
): Record<string, unknown> {
  const next = { ...v };
  const tag = isItemObject(next.tag) ? { ...next.tag } : {};
  tag.items = children;
  next.tag = tag;
  if ("items" in next && next.items !== tag.items) {
    // Prefer tag.items (FTB shape); drop top-level items if present.
    delete next.items;
  }
  return next;
}

export function readFilterTagValue(v: Record<string, unknown>): string {
  const tag = isItemObject(v.tag) ? v.tag : null;
  const val = tag?.value ?? v.value;
  return typeof val === "string" ? val : "";
}

export function setFilterTagValue(
  v: Record<string, unknown>,
  value: string,
): Record<string, unknown> {
  const next = { ...v };
  const tag = isItemObject(next.tag) ? { ...next.tag } : {};
  tag.value = value.trim() || "#minecraft:logs";
  next.tag = tag;
  next.id = "itemfilters:tag";
  return next;
}

export function convertToMode(
  v: ItemValue | null | undefined,
  mode: ItemEditMode,
): ItemValue {
  const display = stackDisplayId(v) ?? "minecraft:stone";
  switch (mode) {
    case "simple":
      return display;
    case "stack": {
      if (isItemObject(v) && !isFilterCompound(v)) {
        return { ...v, id: typeof v.id === "string" ? v.id : display };
      }
      return { id: display, Count: 1 };
    }
    case "filter_or": {
      if (isItemObject(v) && filterKind(v) === "or") return { ...v };
      const kids =
        isItemObject(v) && listFilterChildren(v).length
          ? listFilterChildren(v)
          : [display];
      return { id: "itemfilters:or", tag: { items: kids } };
    }
    case "filter_and": {
      if (isItemObject(v) && filterKind(v) === "and") return { ...v };
      const kids =
        isItemObject(v) && listFilterChildren(v).length
          ? listFilterChildren(v)
          : [display];
      return { id: "itemfilters:and", tag: { items: kids } };
    }
    case "filter_tag": {
      if (isItemObject(v) && filterKind(v) === "tag") return { ...v };
      const existing = isItemObject(v) ? readFilterTagValue(v) : "";
      return {
        id: "itemfilters:tag",
        tag: { value: existing || (display.startsWith("#") ? display : `#${display}`) },
      };
    }
    default:
      return display;
  }
}

export function readTagJson(v: ItemValue | null | undefined): string {
  if (!isItemObject(v)) return "";
  const payload = v.tag ?? v.components;
  if (payload == null) return "";
  try {
    return JSON.stringify(payload, null, 2);
  } catch {
    return "";
  }
}

/** Merge tag/components JSON into stack object; returns null if parse fails. */
export function applyTagJson(
  v: ItemValue | null | undefined,
  json: string,
  key: "tag" | "components" = "tag",
): ItemValue | null {
  const trimmed = json.trim();
  const obj = asStackObject(v);
  if (isFilterCompound(obj)) return obj;
  if (!trimmed) {
    const next = { ...obj };
    delete next[key];
    return next;
  }
  try {
    const parsed = JSON.parse(trimmed) as unknown;
    if (!isItemObject(parsed) && !Array.isArray(parsed)) return null;
    return { ...obj, [key]: parsed };
  } catch {
    return null;
  }
}
