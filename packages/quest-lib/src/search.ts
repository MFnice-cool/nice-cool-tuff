/**
 * Search functionality for quest editor.
 * Supports multi-term AND, fuzzy subsequence fallback, and /regex/ or re: patterns.
 */

import type { QuestData, QuestChapter } from "./types";

export interface SearchResult {
  quest: QuestData;
  chapterId: string;
  chapterTitle: string;
  matchField: "title" | "subtitle" | "id" | "description";
  matchText: string;
}

export interface SearchState {
  query: string;
  results: SearchResult[];
  selectedIndex: number;
  isOpen: boolean;
}

export function createSearchState(): SearchState {
  return {
    query: "",
    results: [],
    selectedIndex: 0,
    isOpen: false,
  };
}

/** True if `term` is a subsequence of `text` (order-preserving, gaps allowed). */
export function fuzzySubsequence(term: string, text: string): boolean {
  if (!term) return true;
  let ti = 0;
  for (let i = 0; i < text.length && ti < term.length; i++) {
    if (text[i] === term[ti]) ti += 1;
  }
  return ti === term.length;
}

/**
 * Parse `/pattern/flags` or `re:pattern` into a RegExp.
 * Empty flags on `/…/` default to `i`. Invalid patterns return null.
 */
export function parseSearchRegex(query: string): RegExp | null {
  const trimmed = query.trim();
  if (!trimmed) return null;

  if (trimmed.startsWith("re:") || trimmed.startsWith("RE:")) {
    const body = trimmed.slice(3);
    if (!body) return null;
    try {
      return new RegExp(body, "i");
    } catch {
      return null;
    }
  }

  if (trimmed.startsWith("/") && trimmed.length >= 3) {
    const last = trimmed.lastIndexOf("/");
    if (last <= 1) return null;
    const body = trimmed.slice(1, last);
    const flags = trimmed.slice(last + 1);
    if (!/^[gimsuy]*$/.test(flags)) return null;
    const effective = flags.length > 0 ? flags : "i";
    try {
      return new RegExp(body, effective);
    } catch {
      return null;
    }
  }

  return null;
}

function searchByRegex(
  re: RegExp,
  chapters: QuestChapter[],
): SearchResult[] {
  const results: SearchResult[] = [];
  const fields: SearchResult["matchField"][] = [
    "title",
    "subtitle",
    "id",
    "description",
  ];

  for (const ch of chapters) {
    for (const q of ch.quests) {
      const values: Record<SearchResult["matchField"], string> = {
        title: q.title,
        subtitle: q.subtitle ?? "",
        id: q.id,
        description: (q.description ?? []).join(" "),
      };
      let hit: SearchResult["matchField"] | null = null;
      for (const f of fields) {
        if (!values[f]) continue;
        re.lastIndex = 0;
        if (re.test(values[f])) {
          hit = f;
          break;
        }
      }
      if (!hit) continue;
      results.push({
        quest: q,
        chapterId: ch.id,
        chapterTitle: ch.title,
        matchField: hit,
        matchText:
          hit === "description"
            ? values.description.substring(0, 100)
            : values[hit],
      });
      if (results.length >= 200) return results;
    }
  }
  return results;
}

/** Lower score is better; null = no match. Exact substring beats fuzzy. */
function termScore(term: string, text: string): number | null {
  if (!term) return 0;
  if (text.includes(term)) return 0;
  if (term.length >= 2 && fuzzySubsequence(term, text)) {
    return 1 + Math.max(0, text.length - term.length);
  }
  return null;
}

function fieldScores(
  terms: string[],
  title: string,
  subtitle: string,
  id: string,
  desc: string,
): { field: SearchResult["matchField"]; score: number } | null {
  const candidates: Array<{ field: SearchResult["matchField"]; raw: string }> = [
    { field: "title", raw: title },
    { field: "subtitle", raw: subtitle },
    { field: "id", raw: id },
    { field: "description", raw: desc },
  ];

  let best: { field: SearchResult["matchField"]; score: number } | null = null;
  for (const c of candidates) {
    if (!c.raw && c.field !== "title" && c.field !== "id") continue;
    let total = 0;
    let ok = true;
    for (const t of terms) {
      const s = termScore(t, c.raw);
      if (s === null) {
        ok = false;
        break;
      }
      total += s;
    }
    if (!ok) continue;
    if (!best || total < best.score) {
      best = { field: c.field, score: total };
    }
  }
  return best;
}

export function searchQuests(
  query: string,
  chapters: QuestChapter[]
): SearchResult[] {
  if (!query.trim()) return [];

  const re = parseSearchRegex(query);
  if (re) return searchByRegex(re, chapters);

  const terms = query
    .toLowerCase()
    .split(/\s+/)
    .map((t) => t.trim())
    .filter(Boolean);
  if (terms.length === 0) return [];

  type Ranked = SearchResult & { score: number };
  const exact: Ranked[] = [];
  const fuzzy: Ranked[] = [];

  for (const ch of chapters) {
    for (const q of ch.quests) {
      const title = q.title.toLowerCase();
      const subtitle = (q.subtitle ?? "").toLowerCase();
      const id = q.id.toLowerCase();
      const descText = (q.description ?? []).join(" ").toLowerCase();
      const hay = `${title} ${subtitle} ${id} ${descText}`;

      if (terms.every((t) => hay.includes(t))) {
        let matchField: SearchResult["matchField"] = "description";
        let matchText = (q.description ?? []).join(" ").substring(0, 100);
        if (terms.every((t) => title.includes(t))) {
          matchField = "title";
          matchText = q.title;
        } else if (q.subtitle && terms.every((t) => subtitle.includes(t))) {
          matchField = "subtitle";
          matchText = q.subtitle;
        } else if (terms.every((t) => id.includes(t))) {
          matchField = "id";
          matchText = q.id;
        }
        exact.push({
          quest: q,
          chapterId: ch.id,
          chapterTitle: ch.title,
          matchField,
          matchText,
          score: 0,
        });
        continue;
      }

      const scored = fieldScores(terms, title, subtitle, id, descText);
      if (!scored) continue;
      const matchText =
        scored.field === "title"
          ? q.title
          : scored.field === "subtitle"
            ? (q.subtitle ?? "")
            : scored.field === "id"
              ? q.id
              : (q.description ?? []).join(" ").substring(0, 100);
      fuzzy.push({
        quest: q,
        chapterId: ch.id,
        chapterTitle: ch.title,
        matchField: scored.field,
        matchText,
        score: scored.score,
      });
    }
  }

  if (exact.length > 0) {
    return exact.slice(0, 200).map(({ score: _s, ...r }) => r);
  }

  fuzzy.sort((a, b) => a.score - b.score || a.matchText.localeCompare(b.matchText));
  return fuzzy.slice(0, 200).map(({ score: _s, ...r }) => r);
}

export function nextResult(state: SearchState): SearchState {
  if (state.results.length === 0) return state;
  return {
    ...state,
    selectedIndex: (state.selectedIndex + 1) % state.results.length,
  };
}

export function prevResult(state: SearchState): SearchState {
  if (state.results.length === 0) return state;
  return {
    ...state,
    selectedIndex: state.selectedIndex > 0
      ? state.selectedIndex - 1
      : state.results.length - 1,
  };
}
