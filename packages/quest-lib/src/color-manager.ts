/**
 * Color code search, aggregation, and recolor logic.
 * Ported from qbedit's Go implementation and improved:
 * - Tracks color state per character position
 * - Supports both & and § prefixes
 * - Bulk recolor and single-occurrence recolor
 * - Aggregates color counts across all quests
 */

import type { QuestData, QuestChapter } from "./types";

// ─── Types ──────────────────────────────────────────────────────

export interface ColorHit {
  questId: string;
  questTitle: string;
  chapterId: string;
  chapterTitle: string;
  field: "title" | "subtitle" | "description";
  /** Array index for description lines, -1 for title/subtitle */
  lineIndex: number;
  /** Position in the original string (after codes stripped) */
  position: number;
  /** The color code character active at this position */
  colorCode: string;
  /** The raw segment around this hit for display */
  segment: string;
  /** Character index in the raw string where the term match starts */
  rawStart: number;
}

export interface ColorAggregation {
  code: string;
  count: number;
  quests: Set<string>;
}

export interface ColorSearchOptions {
  caseSensitive: boolean;
  stripCodesBeforeMatch: boolean;
}

// ─── Color code parsing ─────────────────────────────────────────

/** Parse a string and return an array of {text, colorCode} segments. */
export function parseColorSegments(
  s: string
): { text: string; colorCode: string | null }[] {
  const segments: { text: string; colorCode: string | null }[] = [];
  let i = 0;
  let currentColor: string | null = null;
  let currentText = "";

  while (i < s.length) {
    const c = s[i];
    if ((c === "&" || c === "§") && i + 1 < s.length) {
      const code = s[i + 1]!.toLowerCase();
      if ((code >= "0" && code <= "9") || (code >= "a" && code <= "f") || "klmnor".includes(code)) {
        if (currentText) {
          segments.push({ text: currentText, colorCode: currentColor });
          currentText = "";
        }
        if (code === "r") {
          currentColor = null;
        } else if ((code >= "0" && code <= "9") || (code >= "a" && code <= "f")) {
          currentColor = code;
        }
        i += 2;
        continue;
      }
    }
    currentText += c;
    i++;
  }
  if (currentText) {
    segments.push({ text: currentText, colorCode: currentColor });
  }
  return segments;
}

/** Get the color code active at a specific position in the visible text. */
export function getColorAtPosition(s: string, pos: number): string | null {
  let visiblePos = 0;
  let currentColor: string | null = null;
  let i = 0;

  while (i < s.length && visiblePos <= pos) {
    const c = s[i];
    if ((c === "&" || c === "§") && i + 1 < s.length) {
      const code = s[i + 1]!.toLowerCase();
      if ((code >= "0" && code <= "9") || (code >= "a" && code <= "f")) {
        currentColor = code;
        i += 2;
        continue;
      }
      if (code === "r") {
        currentColor = null;
        i += 2;
        continue;
      }
      if ("klmno".includes(code)) {
        i += 2;
        continue;
      }
    }
    if (visiblePos === pos) return currentColor;
    visiblePos++;
    i++;
  }
  return currentColor;
}

// ─── Search ─────────────────────────────────────────────────────

/** Search for a term across all quests, returning color hits. */
export function searchColorHits(
  chapters: QuestChapter[],
  term: string,
  options: ColorSearchOptions = { caseSensitive: false, stripCodesBeforeMatch: true }
): ColorHit[] {
  if (!term.trim()) return [];

  const hits: ColorHit[] = [];
  const searchLower = options.caseSensitive ? term : term.toLowerCase();

  for (const ch of chapters) {
    for (const q of ch.quests) {
      // Search title
      const titleHits = searchField(q.title ?? "", "title", -1, q, ch, searchLower, options);
      hits.push(...titleHits);

      // Search subtitle
      const subtitleHits = searchField(q.subtitle ?? "", "subtitle", -1, q, ch, searchLower, options);
      hits.push(...subtitleHits);

      // Search description lines
      if (q.description) {
        for (let idx = 0; idx < q.description.length; idx++) {
          const line = q.description[idx]!;
          const lineHits = searchField(line, "description", idx, q, ch, searchLower, options);
          hits.push(...lineHits);
        }
      }
    }
  }

  return hits;
}

function searchField(
  rawText: string,
  field: "title" | "subtitle" | "description",
  lineIndex: number,
  quest: QuestData,
  chapter: QuestChapter,
  searchLower: string,
  options: ColorSearchOptions
): ColorHit[] {
  const hits: ColorHit[] = [];
  if (!rawText) return hits;

  const visibleText = options.stripCodesBeforeMatch
    ? rawText.replace(/[&§][0-9a-fk-or]/gi, "")
    : rawText;

  const textForSearch = options.caseSensitive ? visibleText : visibleText.toLowerCase();
  let searchStart = 0;

  while (searchStart < textForSearch.length) {
    const matchIdx = textForSearch.indexOf(searchLower, searchStart);
    if (matchIdx === -1) break;

    // Map back to raw text position
    let rawPos = 0;
    let visiblePos = 0;
    while (rawPos < rawText.length && visiblePos < matchIdx) {
      const c = rawText[rawPos];
      if ((c === "&" || c === "§") && rawPos + 1 < rawText.length) {
        const code = rawText[rawPos + 1]!.toLowerCase();
        if ("0123456789abcdefklmnor".includes(code)) {
          rawPos += 2;
          continue;
        }
      }
      visiblePos++;
      rawPos++;
    }

    const colorCode = getColorAtPosition(rawText, matchIdx) ?? "r";
    const segStart = Math.max(0, matchIdx - 5);
    const segEnd = Math.min(visibleText.length, matchIdx + searchLower.length + 10);
    const segment = visibleText.substring(segStart, segEnd);

    hits.push({
      questId: quest.id,
      questTitle: quest.title,
      chapterId: chapter.id,
      chapterTitle: chapter.title,
      field,
      lineIndex,
      position: matchIdx,
      colorCode,
      segment,
      rawStart: rawPos,
    });

    searchStart = matchIdx + searchLower.length;
  }

  return hits;
}

// ─── Aggregation ─────────────────────────────────────────────────

/** Aggregate color codes from all hits. */
export function aggregateColors(hits: ColorHit[]): ColorAggregation[] {
  const map = new Map<string, ColorAggregation>();

  for (const hit of hits) {
    const code = hit.colorCode;
    let agg = map.get(code);
    if (!agg) {
      agg = { code, count: 0, quests: new Set() };
      map.set(code, agg);
    }
    agg.count++;
    agg.quests.add(hit.questId);
  }

  return Array.from(map.values()).sort((a, b) => b.count - a.count);
}

// ─── Recolor ─────────────────────────────────────────────────────

/** Replace the color code at a specific visible position in a raw string. */
export function recolorAtPosition(
  rawStr: string,
  newPos: string,
  targetPos: number
): string {
  let visiblePos = 0;
  let i = 0;

  while (i < rawStr.length) {
    const c = rawStr[i];
    if ((c === "&" || c === "§") && i + 1 < rawStr.length) {
      const code = rawStr[i + 1]!.toLowerCase();
      if ("0123456789abcdef".includes(code) || code === "r" || "klmno".includes(code)) {
        i += 2;
        continue;
      }
    }
    if (visiblePos === targetPos) {
      // Find the last color code before this position
      let lastColorPos = -1;
      let scan = 0;
      while (scan < i) {
        const sc = rawStr[scan];
        if ((sc === "&" || sc === "§") && scan + 1 < rawStr.length) {
          const scode = rawStr[scan + 1]!.toLowerCase();
          if ("0123456789abcdef".includes(scode)) {
            lastColorPos = scan;
          }
        }
        scan++;
      }

      if (lastColorPos >= 0) {
        // Replace existing color code
        const prefix = rawStr[lastColorPos] === "§" ? "§" : "&";
        return rawStr.substring(0, lastColorPos + 1) + newPos + rawStr.substring(lastColorPos + 2);
      } else {
        // No color code before this position, insert one
        const prefix = "&";
        return rawStr.substring(0, i) + prefix + newPos + rawStr.substring(i);
      }
    }
    visiblePos++;
    i++;
  }
  return rawStr;
}

/** Bulk recolor: replace all occurrences of a term's color code. */
export function recolorAllOccurrences(
  rawStr: string,
  term: string,
  newColor: string,
  caseSensitive: boolean = false
): string {
  if (!term) return rawStr;

  // Build the visible text and a mapping from visible index → raw index
  const visibleChars: string[] = [];
  const visToRaw: number[] = [];
  let i = 0;
  while (i < rawStr.length) {
    const c = rawStr[i];
    if ((c === "&" || c === "§") && i + 1 < rawStr.length) {
      const code = rawStr[i + 1]!.toLowerCase();
      if ("0123456789abcdefklmnor".includes(code)) {
        i += 2;
        continue;
      }
    }
    visToRaw.push(i);
    visibleChars.push(c);
    i++;
  }

  const visibleText = visibleChars.join("");
  const searchLower = caseSensitive ? term : term.toLowerCase();
  const textForSearch = caseSensitive ? visibleText : visibleText.toLowerCase();

  // Collect all match positions first (in visible coordinates)
  const matchPositions: number[] = [];
  let searchStart = 0;
  while (searchStart < textForSearch.length) {
    const idx = textForSearch.indexOf(searchLower, searchStart);
    if (idx === -1) break;
    matchPositions.push(idx);
    searchStart = idx + term.length;
  }

  // Process in reverse order to avoid position shifts
  let result = rawStr;
  for (let j = matchPositions.length - 1; j >= 0; j--) {
    const visIdx = matchPositions[j]!;
    // Find the color code active at this visible position
    let currentColor: string | null = null;
    let scanRaw = 0;
    while (scanRaw < result.length) {
      const sc = result[scanRaw];
      if ((sc === "&" || sc === "§") && scanRaw + 1 < result.length) {
        const scode = result[scanRaw + 1]!.toLowerCase();
        if ("0123456789abcdef".includes(scode)) {
          currentColor = scode;
          scanRaw += 2;
          continue;
        }
        if (scode === "r") {
          currentColor = null;
          scanRaw += 2;
          continue;
        }
        if ("klmno".includes(scode)) {
          scanRaw += 2;
          continue;
        }
      }
      // We need to count visible chars up to visIdx
      break;
    }

    // Map visible index back to raw position in current string
    let rawPos = 0;
    let visCount = 0;
    while (rawPos < result.length && visCount < visIdx) {
      const c = result[rawPos];
      if ((c === "&" || c === "§") && rawPos + 1 < result.length) {
        const code = result[rawPos + 1]!.toLowerCase();
        if ("0123456789abcdefklmnor".includes(code)) {
          rawPos += 2;
          continue;
        }
      }
      visCount++;
      rawPos++;
    }

    result = recolorAtPosition(result, newColor, visIdx);
  }

  return result;
}
