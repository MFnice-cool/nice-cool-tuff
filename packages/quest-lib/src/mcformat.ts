/**
 * Minecraft color code → HTML renderer.
 * Supports &-codes and §-codes (§ is treated as alias).
 * Improvements over qbedit: proper HTML escaping, configurable class prefix,
 * inline style fallback, strikethrough/underline/obfuscated support.
 */

const COLOR_MAP: Record<string, string> = {
  "0": "#000000", "1": "#0000AA", "2": "#00AA00", "3": "#00AAAA",
  "4": "#AA0000", "5": "#AA00AA", "6": "#FFAA00", "7": "#AAAAAA",
  "8": "#555555", "9": "#5555FF", "a": "#55FF55", "b": "#55FFFF",
  "c": "#FF5555", "d": "#FF55FF", "e": "#FFFF55", "f": "#FFFFFF",
};

const FORMATTING_MAP: Record<string, string> = {
  "k": "obfuscated", "l": "bold", "m": "strikethrough",
  "n": "underline", "o": "italic", "r": "reset",
};

/** Strip all & and § formatting codes from text. */
export function stripCodes(s: string): string {
  return s.replace(/[&§][0-9a-fk-or]/gi, "");
}

/** Convert Minecraft-formatted text to HTML. */
export function mcFormat(text: string): string {
  if (!text) return "";

  let out = "";
  let i = 0;
  let activeColor: string | null = null;
  let activeFormatting = new Set<string>();
  let spanOpen = false;

  function closeSpan() {
    if (spanOpen) { out += "</span>"; spanOpen = false; }
  }

  function openSpan() {
    const styles: string[] = [];
    if (activeColor) styles.push(`color:${activeColor}`);
    if (activeFormatting.has("bold")) styles.push("font-weight:bold");
    if (activeFormatting.has("italic")) styles.push("font-style:italic");
    if (activeFormatting.has("underline")) styles.push("text-decoration:underline");
    if (activeFormatting.has("strikethrough")) styles.push("text-decoration:line-through");
    if (activeFormatting.has("obfuscated")) styles.push("filter:blur(0.5px)");
    if (styles.length > 0) {
      out += `<span style="${styles.join(";")}">`;
      spanOpen = true;
    }
  }

  while (i < text.length) {
    const c = text[i];

    // Handle & and § color/formatting codes
    if ((c === "&" || c === "§") && i + 1 < text.length) {
      const code = text[i + 1]!.toLowerCase();
      if (code in COLOR_MAP) {
        closeSpan();
        activeColor = COLOR_MAP[code];
        activeFormatting.clear();
        i += 2;
        openSpan();
        continue;
      }
      if (code in FORMATTING_MAP) {
        if (code === "r") {
          activeColor = null;
          activeFormatting.clear();
          closeSpan();
        } else {
          activeFormatting.add(FORMATTING_MAP[code]!);
          closeSpan();
          openSpan();
        }
        i += 2;
        continue;
      }
    }

    // HTML-escape special characters
    if (c === "&") { out += "&amp;"; i++; continue; }
    if (c === "<") { out += "&lt;"; i++; continue; }
    if (c === ">") { out += "&gt;"; i++; continue; }
    if (c === '"') { out += "&quot;"; i++; continue; }

    out += c;
    i++;
  }

  closeSpan();
  return out;
}

/** Get just the color code character for a given display color (for color manager). */
export function getColorCode(char: string): string | null {
  const lower = char.toLowerCase();
  if (lower in COLOR_MAP) return lower;
  return null;
}

/** All 16 color codes with their display colors. */
export const MC_COLORS = Object.entries(COLOR_MAP).map(([code, color]) => ({ code, color }));

/** Formatting code names for display. */
export const FORMATTING_NAMES: Record<string, string> = {
  "0": "Black", "1": "Dark Blue", "2": "Dark Green", "3": "Dark Aqua",
  "4": "Dark Red", "5": "Dark Purple", "6": "Gold", "7": "Gray",
  "8": "Dark Gray", "9": "Blue", "a": "Green", "b": "Aqua",
  "c": "Red", "d": "Light Purple", "e": "Yellow", "f": "White",
  "k": "Obfuscated", "l": "Bold", "m": "Strikethrough",
  "n": "Underline", "o": "Italic", "r": "Reset",
};
