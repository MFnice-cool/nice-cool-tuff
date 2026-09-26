/**
 * FTB Quests lang/*.snbt overlay for the desktop quest editor.
 * Mirrors quest-editor-web `applyLocaleOverlay`.
 */

export type LocaleValue = string | string[];
export type LocaleMap = Record<string, LocaleValue>;

export interface LocaleOverlayBook {
  title?: string | null;
  subtitle?: string | null;
  activeLocale?: string | null;
  locales?: Record<string, LocaleMap>;
  chapterGroups: {
    id: string;
    title: string;
    titleFromSnbt?: boolean;
  }[];
  chapters: {
    id: string;
    title: string;
    filename?: string | null;
    titleFromSnbt?: boolean;
    quests: {
      id: string;
      title: string;
      subtitle?: string | null;
      description?: string[];
      titleFromSnbt?: boolean;
      subtitleFromSnbt?: boolean;
      descriptionFromSnbt?: boolean;
      tasks: {
        id: string;
        title?: string | null;
        titleFromSnbt?: boolean;
      }[];
    }[];
  }[];
}

function localeString(loc: LocaleMap, key: string): string | undefined {
  const v = loc[key];
  if (typeof v === "string") return v;
  if (Array.isArray(v) && v.length > 0) return v.join("\n");
  return undefined;
}

function localeStringArray(loc: LocaleMap, key: string): string[] | undefined {
  const v = loc[key];
  if (Array.isArray(v)) return v;
  if (typeof v === "string") return [v];
  return undefined;
}

export function pickLocale(
  locales: Record<string, LocaleMap> | undefined,
  preferred?: string | null,
): { code: string; map: LocaleMap } | null {
  if (!locales) return null;
  if (preferred && locales[preferred]) return { code: preferred, map: locales[preferred]! };
  if (locales["en_us"]) return { code: "en_us", map: locales["en_us"]! };
  const first = Object.entries(locales)[0];
  return first ? { code: first[0], map: first[1] } : null;
}

/** Clear display text that came from a previous lang overlay (not from SNBT). */
export function clearLocaleTitles(book: LocaleOverlayBook): void {
  for (const ch of book.chapters) {
    if (!ch.titleFromSnbt) ch.title = "";
    for (const q of ch.quests) {
      if (!q.titleFromSnbt) q.title = "";
      if (q.subtitleFromSnbt === false) q.subtitle = null;
      if (q.descriptionFromSnbt === false) q.description = [];
      for (const t of q.tasks) {
        if (t.titleFromSnbt === false) t.title = null;
      }
    }
  }
  for (const g of book.chapterGroups) {
    if (!g.titleFromSnbt) g.title = "";
  }
}

/**
 * Apply FTB Quests lang/*.snbt strings onto the book for display.
 * Does not mark fields as inline SNBT (`*FromSnbt` stays false).
 */
export function applyLocaleOverlay<T extends LocaleOverlayBook>(
  book: T,
  locale?: string | null,
): T {
  const picked = pickLocale(book.locales, locale ?? book.activeLocale);
  if (!picked) {
    for (const ch of book.chapters) {
      if (!ch.title) ch.title = ch.filename ?? ch.id.slice(0, 8);
      for (const q of ch.quests) {
        if (!q.title) q.title = q.subtitle ?? q.id.slice(0, 8);
      }
    }
    for (const g of book.chapterGroups) {
      if (!g.title) g.title = g.id.slice(0, 8);
    }
    return book;
  }

  const loc = picked.map;
  book.activeLocale = picked.code;

  if (!book.title) {
    for (const [k, v] of Object.entries(loc)) {
      if (k.startsWith("file.") && k.endsWith(".title") && typeof v === "string") {
        book.title = v;
        break;
      }
    }
  }

  for (const g of book.chapterGroups) {
    if (g.titleFromSnbt) continue;
    const t = localeString(loc, `chapter_group.${g.id}.title`);
    if (t) g.title = t;
    else if (!g.title) g.title = g.id.slice(0, 8);
  }

  for (const ch of book.chapters) {
    if (!ch.titleFromSnbt) {
      const t = localeString(loc, `chapter.${ch.id}.title`);
      if (t) ch.title = t;
      else if (!ch.title) ch.title = ch.filename ?? ch.id.slice(0, 8);
    }

    for (const q of ch.quests) {
      const langTitle = localeString(loc, `quest.${q.id}.title`);
      const langSub = localeString(loc, `quest.${q.id}.quest_subtitle`);
      const langDesc = localeStringArray(loc, `quest.${q.id}.quest_desc`);

      if (!q.titleFromSnbt) {
        q.title = langTitle || q.title || langSub || q.subtitle || q.id.slice(0, 8);
      }
      if (q.subtitleFromSnbt !== true) {
        if (langSub && (q.subtitleFromSnbt === false || !q.subtitle)) {
          q.subtitle = langSub;
          q.subtitleFromSnbt = false;
        }
      }
      if (q.descriptionFromSnbt !== true) {
        if (
          langDesc &&
          (q.descriptionFromSnbt === false || !q.description?.length)
        ) {
          q.description = langDesc;
          q.descriptionFromSnbt = false;
        }
      }

      for (const t of q.tasks) {
        if (t.titleFromSnbt === false || t.title == null || t.title === "") {
          const tt = localeString(loc, `task.${t.id}.title`);
          if (tt) {
            t.title = tt;
            t.titleFromSnbt = false;
          }
        }
      }
    }
  }

  return book;
}

export function localeCodes(locales: Record<string, LocaleMap> | undefined): string[] {
  return Object.keys(locales ?? {}).sort((a, b) => a.localeCompare(b));
}

/**
 * Harvest display titles/subtitles/descriptions that are locale-sourced into a lang map.
 * Preserves unknown keys from `base`.
 */
export function harvestLocaleMap(
  book: {
    chapterGroups: { id: string; title: string; titleFromSnbt?: boolean }[];
    chapters: {
      id: string;
      title: string;
      titleFromSnbt?: boolean;
      quests: {
        id: string;
        title: string;
        subtitle?: string | null;
        description?: string[];
        titleFromSnbt?: boolean;
        subtitleFromSnbt?: boolean;
        descriptionFromSnbt?: boolean;
        tasks: { id: string; title?: string | null; titleFromSnbt?: boolean }[];
      }[];
    }[];
  },
  base: LocaleMap = {},
): LocaleMap {
  const map: LocaleMap = { ...base };

  for (const g of book.chapterGroups) {
    if (!g.titleFromSnbt && g.title.trim()) {
      map[`chapter_group.${g.id}.title`] = g.title;
    }
  }

  for (const ch of book.chapters) {
    if (!ch.titleFromSnbt && ch.title.trim()) {
      map[`chapter.${ch.id}.title`] = ch.title;
    }
    for (const q of ch.quests) {
      const titleKey = `quest.${q.id}.title`;
      const subKey = `quest.${q.id}.quest_subtitle`;
      const descKey = `quest.${q.id}.quest_desc`;

      if (!q.titleFromSnbt && q.title.trim()) {
        map[titleKey] = q.title;
      }
      if (q.subtitleFromSnbt === false && q.subtitle?.trim()) {
        map[subKey] = q.subtitle;
      }
      if (q.descriptionFromSnbt === false && q.description?.length) {
        map[descKey] = [...q.description];
      }
      for (const t of q.tasks) {
        if (t.titleFromSnbt === false && t.title?.trim()) {
          map[`task.${t.id}.title`] = t.title;
        }
      }
    }
  }

  return map;
}

/** Deep-copy a locale map (arrays for quest_desc). */
export function cloneLocaleMap(source: LocaleMap): LocaleMap {
  const out: LocaleMap = {};
  for (const [k, v] of Object.entries(source)) {
    out[k] = Array.isArray(v) ? [...v] : v;
  }
  return out;
}

/**
 * Keys implied by book structure (same surface as harvest, always emitted).
 * Includes subtitle/desc keys even when currently empty so gap reports cover them.
 */
export function expectedLocaleKeys(book: {
  chapterGroups: { id: string }[];
  chapters: {
    id: string;
    quests: {
      id: string;
      tasks: { id: string }[];
    }[];
  }[];
}): string[] {
  const keys: string[] = [];
  for (const g of book.chapterGroups) {
    keys.push(`chapter_group.${g.id}.title`);
  }
  for (const ch of book.chapters) {
    keys.push(`chapter.${ch.id}.title`);
    for (const q of ch.quests) {
      keys.push(`quest.${q.id}.title`);
      keys.push(`quest.${q.id}.quest_subtitle`);
      keys.push(`quest.${q.id}.quest_desc`);
      for (const t of q.tasks) {
        if (t.id) keys.push(`task.${t.id}.title`);
      }
    }
  }
  return keys;
}

export type LocaleGapKind = "missing" | "empty";

export interface LocaleGapEntry {
  key: string;
  kind: LocaleGapKind;
  /** Preview from base (stringified). */
  basePreview: string;
  questId?: string;
  chapterId?: string;
  groupId?: string;
}

function isEmptyLocaleValue(v: LocaleValue | undefined): boolean {
  if (v == null) return true;
  if (typeof v === "string") return !v.trim();
  return v.length === 0 || v.every((s) => !String(s).trim());
}

function previewLocaleValue(v: LocaleValue | undefined): string {
  if (v == null) return "";
  if (typeof v === "string") return v;
  return v.join("\n");
}

function parseLocaleKeyMeta(key: string): Pick<LocaleGapEntry, "questId" | "chapterId" | "groupId"> {
  const quest = /^quest\.([^.]+)\./.exec(key);
  if (quest) return { questId: quest[1] };
  const chapter = /^chapter\.([^.]+)\./.exec(key);
  if (chapter) return { chapterId: chapter[1] };
  const group = /^chapter_group\.([^.]+)\./.exec(key);
  if (group) return { groupId: group[1] };
  const task = /^task\.([^.]+)\./.exec(key);
  if (task) return { questId: undefined }; // resolved by caller if needed
  return {};
}

/**
 * Keys present (non-empty) in base but missing or empty in target.
 * When `keys` is provided, only those keys are considered (and base emptiness skips).
 */
export function localeGap(
  base: LocaleMap,
  target: LocaleMap,
  keys?: string[],
): LocaleGapEntry[] {
  const keyList =
    keys ??
    Array.from(
      new Set([...Object.keys(base), ...Object.keys(target)]),
    ).sort((a, b) => a.localeCompare(b));

  const out: LocaleGapEntry[] = [];
  for (const key of keyList) {
    const baseVal = base[key];
    if (isEmptyLocaleValue(baseVal)) continue;
    const targetVal = target[key];
    if (targetVal === undefined) {
      out.push({
        key,
        kind: "missing",
        basePreview: previewLocaleValue(baseVal),
        ...parseLocaleKeyMeta(key),
      });
    } else if (isEmptyLocaleValue(targetVal)) {
      out.push({
        key,
        kind: "empty",
        basePreview: previewLocaleValue(baseVal),
        ...parseLocaleKeyMeta(key),
      });
    }
  }
  return out;
}

/** Minecraft-style locale code: xx_yy (also allows longer region tags like en_us). */
export function isValidLocaleCode(code: string): boolean {
  return /^[a-z]{2,3}_[a-z0-9]{2,8}$/.test(code.trim());
}

export function localeValueAsString(map: LocaleMap, key: string): string {
  return previewLocaleValue(map[key]);
}
