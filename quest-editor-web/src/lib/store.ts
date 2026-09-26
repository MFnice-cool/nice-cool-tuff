/**
 * Quest editor types and in-memory store.
 * No backend — everything lives in the browser.
 */

// ─── Types (matching FTB Quests SNBT schema) ────────────────────

export interface QuestTask {
  id: string;
  type: string;
  title?: string | null;
  value?: unknown;
  properties?: Record<string, unknown>;
  /** False when title came only from lang overlay — omit on chapter SNBT export. */
  titleFromSnbt?: boolean;
}

export interface QuestReward {
  id: string;
  type: string;
  title?: string | null;
  properties?: Record<string, unknown>;
}

export interface QuestData {
  id: string;
  title: string;
  subtitle?: string | null;
  description?: string[];
  x: number;
  y: number;
  icon?: string | null;
  dependencies: string[];
  tasks: QuestTask[];
  rewards: QuestReward[];
  optional: boolean;
  shape?: string | null;
  size?: number;
  hideDependencyLines?: boolean | null;
  hideDependentLines?: boolean | null;
  canRepeat?: boolean | null;
  invisible?: boolean | null;
  disableToast?: boolean | null;
  minRequiredDependencies?: number | null;
  dependencyRequirement?: string | null;
  extras?: Record<string, unknown>;
  /** False when title came only from lang overlay — omit on chapter SNBT export. */
  titleFromSnbt?: boolean;
}

export interface QuestChapter {
  id: string;
  title: string;
  icon?: string | null;
  group?: string | null;
  orderIndex?: number | null;
  filename?: string | null;
  defaultQuestShape?: string | null;
  defaultHideDependencyLines?: boolean | null;
  quests: QuestData[];
  extras?: Record<string, unknown>;
  sourceFile?: string | null;
  /** False when title came only from lang overlay — omit on chapter SNBT export. */
  titleFromSnbt?: boolean;
}

export interface QuestChapterGroup {
  id: string;
  title: string;
  titleFromSnbt?: boolean;
}

export interface QuestRewardTable {
  id: string;
  filename?: string | null;
  orderIndex?: number | null;
  lootSize?: number | null;
  rewards: unknown[];
  extras?: Record<string, unknown>;
  sourceFile?: string | null;
}

export type LocaleValue = string | string[];
export type LocaleMap = Record<string, LocaleValue>;

export interface QuestBook {
  chapters: QuestChapter[];
  chapterGroups: QuestChapterGroup[];
  title?: string | null;
  subtitle?: string | null;
  bookSettings?: Record<string, unknown>;
  rewardTables?: QuestRewardTable[];
  /** locale code → translation keys (e.g. chapter.<id>.title) */
  locales?: Record<string, LocaleMap>;
  activeLocale?: string | null;
}

export interface QuestValidationIssue {
  questId: string;
  message: string;
}

export interface QuestBookLoadStats {
  chapterCount: number;
  fileCount: number;
  langKeyCount: number;
  rewardTableCount: number;
  groupCount: number;
  locale: string | null;
}

// ─── SNBT import/export helpers ─────────────────────────────────

import { parseSnbt, serializeSnbt, type SnbtValue } from "./snbt";

function gs(map: Record<string, SnbtValue>, key: string): string | undefined {
  const v = map[key];
  if (typeof v === "string") return v;
  if (v !== undefined && v !== null) return String(v);
  return undefined;
}

function iconIdFromMap(map: Record<string, SnbtValue>): string | undefined {
  const icon = map["icon"];
  if (typeof icon === "string") return icon;
  if (icon && typeof icon === "object" && !Array.isArray(icon)) {
    const id = (icon as Record<string, SnbtValue>)["id"];
    if (typeof id === "string") return id;
  }
  return undefined;
}

function parseDependencies(v: SnbtValue): string[] {
  if (Array.isArray(v)) return v.map((x) => String(x));
  if (typeof v === "string") return [v];
  return [];
}

function parseTasks(arr: SnbtValue[]): QuestTask[] {
  return arr
    .filter((x): x is Record<string, SnbtValue> => typeof x === "object" && x !== null && !Array.isArray(x))
    .map((m) => {
      const inlineTitle = gs(m, "title");
      return {
        id: gs(m, "id") ?? "",
        type: gs(m, "type") ?? "checkmark",
        title: inlineTitle ?? null,
        value: m["value"] ?? null,
        properties: extractExtras(m, ["id", "type", "title", "value"]),
        titleFromSnbt: inlineTitle != null,
      };
    });
}

function parseRewards(arr: SnbtValue[]): QuestReward[] {
  return arr
    .filter((x): x is Record<string, SnbtValue> => typeof x === "object" && x !== null && !Array.isArray(x))
    .map((m) => ({
      id: gs(m, "id") ?? "",
      type: gs(m, "type") ?? "command",
      title: gs(m, "title") ?? null,
      properties: extractExtras(m, ["id", "type", "title"]),
    }));
}

function extractExtras(
  map: Record<string, SnbtValue>,
  known: string[]
): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(map)) {
    if (!known.includes(k)) out[k] = v;
  }
  return out;
}

export function normalizeSnbtPath(path: string): string {
  return path.replace(/\\/g, "/");
}

function basename(path: string): string {
  const n = normalizeSnbtPath(path);
  const i = n.lastIndexOf("/");
  return i >= 0 ? n.slice(i + 1) : n;
}

function localeCodeFromLangPath(path: string): string | null {
  const m = normalizeSnbtPath(path).match(/(?:^|\/)lang\/([^/]+)\.snbt$/i);
  return m ? m[1]! : null;
}

export function isLangSnbtPath(path: string): boolean {
  return /(?:^|\/)lang\/[^/]+\.snbt$/i.test(normalizeSnbtPath(path));
}

export function isRewardTableSnbtPath(path: string): boolean {
  return /(?:^|\/)reward_tables\/[^/]+\.snbt$/i.test(normalizeSnbtPath(path));
}

export function isDataSnbtPath(path: string): boolean {
  return /(?:^|\/)data\.snbt$/i.test(normalizeSnbtPath(path));
}

export function isChapterGroupsSnbtPath(path: string): boolean {
  return /(?:^|\/)chapter_groups\.snbt$/i.test(normalizeSnbtPath(path));
}

export function isChapterSnbtPath(path: string): boolean {
  const p = normalizeSnbtPath(path);
  if (!p.endsWith(".snbt")) return false;
  if (isLangSnbtPath(p) || isRewardTableSnbtPath(p) || isDataSnbtPath(p) || isChapterGroupsSnbtPath(p)) {
    return false;
  }
  if (/(?:^|\/)chapters\/[^/]+\.snbt$/i.test(p)) return true;
  // Legacy flat drop: single filename or no known meta folder in path
  const parts = p.split("/").filter(Boolean);
  if (parts.length === 1) return true;
  const parent = parts[parts.length - 2]!;
  return parent !== "lang" && parent !== "reward_tables";
}

// ─── Parse SNBT chapter ─────────────────────────────────────────

function parseSnbtChapter(content: string): QuestChapter | null {
  try {
    const j = parseSnbt(content);
    if (!j || typeof j !== "object" || Array.isArray(j)) return null;
    const m = j as Record<string, SnbtValue>;

    const questsArr = Array.isArray(m["quests"]) ? (m["quests"] as SnbtValue[]) : [];
    const quests: QuestData[] = questsArr
      .filter((q): q is Record<string, SnbtValue> => typeof q === "object" && q !== null && !Array.isArray(q))
      .map((qm) => {
        const q = qm as Record<string, SnbtValue>;
        const desc = q["description"];
        const description = Array.isArray(desc)
          ? desc.map((x) => String(x))
          : typeof desc === "string"
            ? [desc]
            : [];
        const inlineTitle = gs(q, "title");

        return {
          id: gs(q, "id") ?? "",
          title: inlineTitle ?? "",
          subtitle: gs(q, "subtitle") ?? null,
          description,
          x: typeof q["x"] === "number" ? q["x"] : parseFloat(String(q["x"] ?? 0)),
          y: typeof q["y"] === "number" ? q["y"] : parseFloat(String(q["y"] ?? 0)),
          icon: iconIdFromMap(q) ?? null,
          dependencies: parseDependencies(q["dependencies"] ?? []),
          tasks: parseTasks(Array.isArray(q["tasks"]) ? (q["tasks"] as SnbtValue[]) : []),
          rewards: parseRewards(Array.isArray(q["rewards"]) ? (q["rewards"] as SnbtValue[]) : []),
          optional: q["optional"] === true || q["optional"] === 1,
          shape: gs(q, "shape") ?? null,
          size: typeof q["size"] === "number" ? q["size"] : undefined,
          hideDependencyLines: q["hide_dependency_lines"] === true ? true : q["hide_dependency_lines"] === false ? false : null,
          hideDependentLines: q["hide_dependent_lines"] === true ? true : q["hide_dependent_lines"] === false ? false : null,
          canRepeat: q["can_repeat"] === true ? true : q["can_repeat"] === false ? false : null,
          invisible: q["invisible"] === true ? true : q["invisible"] === false ? false : null,
          disableToast: q["disable_toast"] === true ? true : q["disable_toast"] === false ? false : null,
          minRequiredDependencies: typeof q["min_required_dependencies"] === "number" ? (q["min_required_dependencies"] as number) : null,
          dependencyRequirement: gs(q, "dependency_requirement") ?? null,
          extras: extractExtras(q, [
            "id", "title", "subtitle", "description", "x", "y", "icon",
            "dependencies", "tasks", "rewards", "optional", "shape", "size",
            "hide_dependency_lines", "hide_dependent_lines", "can_repeat",
            "invisible", "disable_toast", "min_required_dependencies",
            "dependency_requirement",
          ]),
          titleFromSnbt: inlineTitle != null,
        };
      });

    const inlineTitle = gs(m, "title");
    return {
      id: gs(m, "id") ?? "untitled",
      title: inlineTitle ?? "",
      icon: iconIdFromMap(m) ?? null,
      group: gs(m, "group") ?? null,
      orderIndex: typeof m["order_index"] === "number" ? (m["order_index"] as number) : null,
      filename: gs(m, "filename") ?? null,
      defaultQuestShape: gs(m, "default_quest_shape") ?? null,
      defaultHideDependencyLines: m["default_hide_dependency_lines"] === true ? true : m["default_hide_dependency_lines"] === false ? false : null,
      quests,
      extras: extractExtras(m, [
        "id", "title", "icon", "group", "order_index", "quests", "filename",
        "default_quest_shape", "default_hide_dependency_lines",
      ]),
      sourceFile: null,
      titleFromSnbt: inlineTitle != null,
    };
  } catch {
    return null;
  }
}

function parseLocaleFile(content: string): LocaleMap | null {
  try {
    const j = parseSnbt(content);
    if (!j || typeof j !== "object" || Array.isArray(j)) return null;
    const out: LocaleMap = {};
    for (const [k, v] of Object.entries(j as Record<string, SnbtValue>)) {
      if (typeof v === "string") out[k] = v;
      else if (Array.isArray(v)) out[k] = v.map((x) => String(x));
      else if (v !== null && v !== undefined) out[k] = String(v);
    }
    return out;
  } catch {
    return null;
  }
}

function parseChapterGroups(content: string): QuestChapterGroup[] {
  try {
    const j = parseSnbt(content);
    if (!j || typeof j !== "object" || Array.isArray(j)) return [];
    const arr = (j as Record<string, SnbtValue>)["chapter_groups"];
    if (!Array.isArray(arr)) return [];
    return arr
      .filter((x): x is Record<string, SnbtValue> => typeof x === "object" && x !== null && !Array.isArray(x))
      .map((m) => {
        const inlineTitle = gs(m, "title");
        return {
          id: gs(m, "id") ?? "",
          title: inlineTitle ?? "",
          titleFromSnbt: inlineTitle != null,
        };
      })
      .filter((g) => g.id);
  } catch {
    return [];
  }
}

function parseBookData(content: string): {
  title: string | null;
  subtitle: string | null;
  bookSettings: Record<string, unknown>;
} {
  try {
    const j = parseSnbt(content);
    if (!j || typeof j !== "object" || Array.isArray(j)) {
      return { title: null, subtitle: null, bookSettings: {} };
    }
    const m = j as Record<string, SnbtValue>;
    const title = gs(m, "title") ?? null;
    const subtitle = gs(m, "subtitle") ?? null;
    const bookSettings = extractExtras(m, ["title", "subtitle"]);
    return { title, subtitle, bookSettings };
  } catch {
    return { title: null, subtitle: null, bookSettings: {} };
  }
}

function parseRewardTable(content: string, path: string): QuestRewardTable | null {
  try {
    const j = parseSnbt(content);
    if (!j || typeof j !== "object" || Array.isArray(j)) return null;
    const m = j as Record<string, SnbtValue>;
    const file = basename(path).replace(/\.snbt$/i, "");
    return {
      id: gs(m, "id") ?? file,
      filename: file,
      orderIndex: typeof m["order_index"] === "number" ? (m["order_index"] as number) : null,
      lootSize: typeof m["loot_size"] === "number" ? (m["loot_size"] as number) : null,
      rewards: Array.isArray(m["rewards"]) ? (m["rewards"] as unknown[]) : [],
      extras: extractExtras(m, ["id", "order_index", "loot_size", "rewards"]),
      sourceFile: path,
    };
  } catch {
    return null;
  }
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

function pickLocale(
  locales: Record<string, LocaleMap> | undefined,
  preferred?: string | null
): { code: string; map: LocaleMap } | null {
  if (!locales) return null;
  if (preferred && locales[preferred]) return { code: preferred, map: locales[preferred]! };
  if (locales["en_us"]) return { code: "en_us", map: locales["en_us"]! };
  const first = Object.entries(locales)[0];
  return first ? { code: first[0], map: first[1] } : null;
}

/**
 * Apply FTB Quests lang/*.snbt strings onto the book for display.
 * Does not mark fields as inline SNBT (titleFromSnbt stays false).
 */
export function applyLocaleOverlay(book: QuestBook, locale?: string | null): QuestBook {
  const picked = pickLocale(book.locales, locale ?? book.activeLocale);
  if (!picked) {
    // Fallbacks when no lang file
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
        // Prefer lang title; else existing inline/empty → subtitle → short id
        q.title = langTitle || q.title || langSub || q.subtitle || q.id.slice(0, 8);
      }
      if (!q.subtitle && langSub) q.subtitle = langSub;
      if ((!q.description || q.description.length === 0) && langDesc) {
        q.description = langDesc;
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

export function summarizeQuestBookLoad(book: QuestBook, fileCount: number): QuestBookLoadStats {
  const locale = book.activeLocale ?? (book.locales ? Object.keys(book.locales)[0] ?? null : null);
  const langKeyCount = locale && book.locales?.[locale]
    ? Object.keys(book.locales[locale]!).length
    : Object.values(book.locales ?? {}).reduce((n, m) => n + Object.keys(m).length, 0);
  return {
    chapterCount: book.chapters.length,
    fileCount,
    langKeyCount,
    rewardTableCount: book.rewardTables?.length ?? 0,
    groupCount: book.chapterGroups.length,
    locale,
  };
}

export function formatLoadMessage(stats: QuestBookLoadStats): string {
  const bits = [`${stats.chapterCount} chapter(s)`];
  if (stats.groupCount) bits.push(`${stats.groupCount} group(s)`);
  if (stats.rewardTableCount) bits.push(`${stats.rewardTableCount} reward table(s)`);
  if (stats.langKeyCount) {
    bits.push(`${stats.langKeyCount} lang key(s)${stats.locale ? ` [${stats.locale}]` : ""}`);
  }
  bits.push(`${stats.fileCount} file(s)`);
  return `Loaded ${bits.join(", ")}`;
}

// ─── Public API ─────────────────────────────────────────────────

export function loadQuestBookFromSnbt(files: Map<string, string>): QuestBook {
  const chapters: QuestChapter[] = [];
  const chapterGroups: QuestChapterGroup[] = [];
  const rewardTables: QuestRewardTable[] = [];
  const locales: Record<string, LocaleMap> = {};
  let title: string | null = null;
  let subtitle: string | null = null;
  let bookSettings: Record<string, unknown> | undefined;

  for (const [rawPath, content] of files) {
    if (!rawPath.endsWith(".snbt") && !normalizeSnbtPath(rawPath).endsWith(".snbt")) continue;
    const path = normalizeSnbtPath(rawPath);

    if (isLangSnbtPath(path)) {
      const code = localeCodeFromLangPath(path);
      const map = parseLocaleFile(content);
      if (code && map) locales[code] = map;
      continue;
    }

    if (isDataSnbtPath(path)) {
      const data = parseBookData(content);
      title = data.title;
      subtitle = data.subtitle;
      bookSettings = data.bookSettings;
      continue;
    }

    if (isChapterGroupsSnbtPath(path)) {
      chapterGroups.push(...parseChapterGroups(content));
      continue;
    }

    if (isRewardTableSnbtPath(path)) {
      const table = parseRewardTable(content, path);
      if (table) rewardTables.push(table);
      continue;
    }

    if (isChapterSnbtPath(path)) {
      const ch = parseSnbtChapter(content);
      if (ch) {
        ch.sourceFile = path;
        if (!ch.filename) {
          ch.filename = basename(path).replace(/\.snbt$/i, "");
        }
        chapters.push(ch);
      }
    }
  }

  chapters.sort((a, b) => (a.orderIndex ?? 0) - (b.orderIndex ?? 0));
  rewardTables.sort((a, b) => (a.orderIndex ?? 0) - (b.orderIndex ?? 0));

  const book: QuestBook = {
    chapters,
    chapterGroups,
    title,
    subtitle,
    bookSettings,
    rewardTables,
    locales: Object.keys(locales).length > 0 ? locales : undefined,
  };

  return applyLocaleOverlay(book);
}

// ─── Serialize back to SNBT ─────────────────────────────────────

function serializeQuestTask(t: QuestTask): Record<string, SnbtValue> {
  const obj: Record<string, SnbtValue> = { id: t.id, type: t.type };
  if (t.titleFromSnbt !== false && t.title) obj.title = t.title;
  if (t.value !== undefined) obj.value = t.value as SnbtValue;
  if (t.properties && Object.keys(t.properties).length > 0) {
    Object.assign(obj, t.properties);
  }
  return obj;
}

function serializeQuest(q: QuestData): Record<string, SnbtValue> {
  const obj: Record<string, SnbtValue> = {
    id: q.id,
    x: q.x,
    y: q.y,
  };
  if (q.titleFromSnbt !== false && q.title) obj.title = q.title;
  if (q.subtitle) obj.subtitle = q.subtitle;
  if (q.description && q.description.length > 0) obj.description = q.description;
  if (q.icon) obj.icon = q.icon;
  if (q.dependencies.length > 0) obj.dependencies = q.dependencies;
  if (q.tasks.length > 0) obj.tasks = q.tasks.map(serializeQuestTask);
  if (q.rewards.length > 0) obj.rewards = q.rewards.map(serializeQuestTask);
  if (q.optional) obj.optional = 1;
  if (q.shape) obj.shape = q.shape;
  if (q.size !== undefined && q.size !== 1) obj.size = q.size;
  if (q.hideDependencyLines !== null && q.hideDependencyLines !== undefined) {
    obj.hide_dependency_lines = q.hideDependencyLines ? 1 : 0;
  }
  if (q.hideDependentLines !== null && q.hideDependentLines !== undefined) {
    obj.hide_dependent_lines = q.hideDependentLines ? 1 : 0;
  }
  if (q.canRepeat !== null && q.canRepeat !== undefined) obj.can_repeat = q.canRepeat ? 1 : 0;
  if (q.invisible !== null && q.invisible !== undefined) obj.invisible = q.invisible ? 1 : 0;
  if (q.disableToast !== null && q.disableToast !== undefined) {
    obj.disable_toast = q.disableToast ? 1 : 0;
  }
  if (q.minRequiredDependencies !== null && q.minRequiredDependencies !== undefined) {
    obj.min_required_dependencies = q.minRequiredDependencies;
  }
  if (q.dependencyRequirement) obj.dependency_requirement = q.dependencyRequirement;
  if (q.extras) Object.assign(obj, q.extras);
  return obj;
}

function serializeChapter(ch: QuestChapter): string {
  const obj: Record<string, SnbtValue> = {
    id: ch.id,
  };
  if (ch.titleFromSnbt !== false && ch.title) obj.title = ch.title;
  if (ch.icon) obj.icon = ch.icon;
  if (ch.group) obj.group = ch.group;
  if (ch.orderIndex !== null && ch.orderIndex !== undefined) obj.order_index = ch.orderIndex;
  if (ch.filename) obj.filename = ch.filename;
  if (ch.defaultQuestShape) obj.default_quest_shape = ch.defaultQuestShape;
  if (ch.defaultHideDependencyLines !== null && ch.defaultHideDependencyLines !== undefined) {
    obj.default_hide_dependency_lines = ch.defaultHideDependencyLines ? 1 : 0;
  }
  if (ch.quests.length > 0) obj.quests = ch.quests.map(serializeQuest);
  if (ch.extras) Object.assign(obj, ch.extras);

  return serializeSnbt(obj);
}

export function exportChapterSnbt(ch: QuestChapter): string {
  return serializeChapter(ch);
}

// ─── Validation ─────────────────────────────────────────────────

export function validateQuestBook(book: QuestBook): QuestValidationIssue[] {
  const issues: QuestValidationIssue[] = [];
  const allIds = new Set<string>();

  for (const ch of book.chapters) {
    for (const q of ch.quests) {
      if (allIds.has(q.id)) {
        issues.push({ questId: q.id, message: `Duplicate quest id "${q.id}"` });
      }
      allIds.add(q.id);

      if (!q.title.trim()) {
        issues.push({ questId: q.id, message: "Empty title" });
      }
      if (q.tasks.length === 0) {
        issues.push({ questId: q.id, message: "No tasks defined" });
      }
    }

    // Check dependency references
    for (const q of ch.quests) {
      for (const dep of q.dependencies) {
        if (!allIds.has(dep)) {
          // Also check task ids
          const taskExists = ch.quests.some((oq) => oq.tasks.some((t) => t.id === dep));
          if (!taskExists) {
            issues.push({ questId: q.id, message: `Missing dependency "${dep}"` });
          }
        }
      }
    }

    // Cycle detection
    const visited = new Set<string>();
    const inStack = new Set<string>();
    function hasCycle(qid: string): boolean {
      if (inStack.has(qid)) return true;
      if (visited.has(qid)) return false;
      visited.add(qid);
      inStack.add(qid);
      const q = ch.quests.find((x) => x.id === qid);
      if (q) {
        for (const dep of q.dependencies) {
          if (hasCycle(dep)) return true;
        }
      }
      inStack.delete(qid);
      return false;
    }
    for (const q of ch.quests) {
      visited.clear();
      inStack.clear();
      if (hasCycle(q.id)) {
        issues.push({ questId: q.id, message: "Dependency cycle detected" });
        break; // one per chapter is enough
      }
    }
  }

  return issues;
}

// ─── LocalStorage persistence ───────────────────────────────────

const STORAGE_KEY = "tuffbox.quest_editor.book";

export function saveToStorage(book: QuestBook): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(book));
  } catch { /* ignore */ }
}

export function loadFromStorage(): QuestBook | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    return JSON.parse(raw) as QuestBook;
  } catch {
    return null;
  }
}

export function clearStorage(): void {
  localStorage.removeItem(STORAGE_KEY);
}
