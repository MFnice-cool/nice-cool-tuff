#!/usr/bin/env node
/**
 * Readability floor: no `font-size` below 11px anywhere in the UI.
 *
 * A 2026-09 UI audit counted ~150 declarations at 8–10.5px across 59 files —
 * uppercase micro-badges, "9px" severity tags, 8px quest-node labels. Below
 * ~11px, text is unreadable on 100%-scaled laptop screens and unusable for
 * Cyrillic glyphs (which run smaller than Latin at the same px size). The
 * interface scale setting (`--ui-scale` zoom) multiplies whatever is there,
 * so tiny bases stay tiny until the user zooms 120%+.
 *
 * Floor: 11px for regular UI text. 10px is allowed ONLY inside rules that set
 * the pixel font (Press Start 2P via --font-minecraft) — its glyphs are block
 * caps roughly 1.4× the x-height of a normal sans at the same size, so 10px
 * pixel font reads like ~13px UI text. The allowlist below pins the exact
 * spots; anything new needs an entry here with a reason, or ≥ 11px.
 *
 * Usage:
 *   node scripts/check-font-sizes.mjs        # scan src/, exit 1 on findings
 */
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const appRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const srcRoot = path.join(appRoot, "src");

const FLOOR_PX = 11;
const PIXEL_FONT_FLOOR_PX = 10;
const SIZE_RE = /font-size:\s*([\d.]+)(px|rem|em)/g;
/** CSS `font:` shorthand — `font: 10.5px ui-monospace, monospace;` used to
 * slip past this check because only the longhand was scanned. */
const FONT_SHORTHAND_RE = /\bfont:\s*([\d.]+)(px|rem)\b/g;
/** Tailwind arbitrary size utilities in markup (text-[10px], text-[0.65rem]). */
const TW_SIZE_RE = /text-\[([\d.]+)(px|rem)\]/g;
const REM_PX = 16;

/** Pixel-font rules allowed down to 10px: `file → selector` + a reason. */
const PIXEL_FONT_ALLOWLIST = [
  // Nick label in Press Start 2P next to the skin preview — 11px of the pixel
  // font overflows the narrow account card and wraps under the avatar.
  "src/components/Me.svelte → .account-text .name",
];

function walk(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "node_modules" || entry.name === "dist") continue;
      walk(full, out);
    } else if (/\.(svelte|css|ts)$/.test(entry.name)) {
      out.push(full);
    }
  }
  return out;
}

/** The declaration block a character index belongs to (selector + body). */
function blockAt(src, index) {
  const bodyEnd = src.indexOf("}", index);
  let depth = 0;
  let start = index;
  while (start > 0) {
    const ch = src[start];
    if (ch === "}") depth++;
    if (ch === "{") {
      if (depth === 0) break;
      depth--;
    }
    start--;
  }
  const headStart = Math.max(src.lastIndexOf(";", start), src.lastIndexOf("}", start));
  return {
    selector: src.slice(headStart + 1, start).trim(),
    body: src.slice(start + 1, bodyEnd === -1 ? src.length : bodyEnd),
  };
}

const isPixelFontBlock = (body) => /var\(--font-minecraft\)|"Press Start 2P"/.test(body);

let violations = 0;
const files = walk(srcRoot);
for (const file of files) {
  const src = readFileSync(file, "utf8");
  const rel = path.relative(appRoot, file).replaceAll("\\", "/");
  const lines = src.split("\n");
  let offset = 0;
  for (let lineNo = 0; lineNo < lines.length; lineNo++) {
    const line = lines[lineNo];
    for (const [re, kind] of [
      [SIZE_RE, "font-size"],
      [FONT_SHORTHAND_RE, "font shorthand"],
      [TW_SIZE_RE, "text-[…] utility"],
    ]) {
      re.lastIndex = 0;
      for (const m of line.matchAll(re)) {
        const value = parseFloat(m[1]) * (m[2] === "px" ? 1 : m[2] === "rem" ? REM_PX : NaN);
        if (Number.isNaN(value)) continue; // em — relative to parent, skip
        // Tailwind utilities are one-off inline sizing: no pixel-font context.
        const isPixelFont = kind === "font-size" && isPixelFontBlock(blockAt(src, offset + m.index).body);
        const floor = isPixelFont ? PIXEL_FONT_FLOOR_PX : FLOOR_PX;
        if (value >= floor) continue;
        if (isPixelFont && PIXEL_FONT_ALLOWLIST.includes(`${rel} → ${blockAt(src, offset + m.index).selector}`)) continue;
        violations++;
        console.error(`${rel}:${lineNo + 1}: ${kind} ${m[1]}${m[2]} < ${floor}px floor${isPixelFont ? " (pixel font)" : ""}`);
      }
    }
    offset += line.length + 1;
  }
}

if (violations === 0) {
  console.log(`check-font-sizes: OK — ${files.length} file(s) scanned, nothing below the ${FLOOR_PX}px floor.`);
  process.exit(0);
}
console.error(`\ncheck-font-sizes: ${violations} declaration(s) below the ${FLOOR_PX}px readability floor.`);
console.error("Fix: bump to 11px (12px+ for body copy), or use an existing size token. Pixel-font rules may go down to 10px via the allowlist with a reason.");
process.exit(1);
