#!/usr/bin/env node
/**
 * Muted-microtext guard: information text must not combine a SMALL font
 * (below 12px) with a LOW-CONTRAST color (muted/tertiary/neutral-gray).
 *
 * Complements the two existing guards:
 *  - check-font-sizes.mjs  — absolute 11px floor (size alone)
 *  - theme-contrast.test.ts — token-vs-surface contrast per theme (color alone)
 * This one catches the COMBINATION that reads as "tiny gray whisper text":
 * `class="text-[11px] text-[var(--text-muted)]"`, `.meta { font-size: 11px;
 * color: var(--text-muted) }`, etc. — usually the most important guidance
 * text in the app (status pills, "what to do next" hints).
 *
 * Rules:
 *  - size: Tailwind arbitrary `text-[<12px]` / `text-[11.x px]` or CSS
 *    `font-size: <12px` in <style> blocks and inline style attributes
 *  - color: text-muted / --text-muted / neutral-4xx-5xx grays / #94a3b8-ish
 *  - flags an element or CSS rule when BOTH appear
 *  - icons/aria-hidden elements and comments are ignored; an allowlist below
 *    covers intentional pixel-font decorations
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "src");

/** Intentional exceptions: [fileSuffix, reason]. Keep this SHORT. */
const ALLOWLIST = [
  // Pixel-font decorations (Press Start 2P) are decorative, never information.
  [/pixelato/i, "pixel-font decorative label"],
];

const SIZE_RE = /text-\[(1[01](?:\.\d+)?px)\]|text-\[(\d(?:\.\d+)?px)\]/g;
const CSS_SIZE_RE = /font-size:\s*(1[01](?:\.\d+)?|\d(?:\.\d+)?)px|\bfont:\s*(1[01](?:\.\d+)?|\d(?:\.\d+)?)px\b/g;
const COLOR_RE =
  /text-muted|text-\[var\(--text-muted\)\]|text-\[color:var\(--text-muted\)\]|color:\s*var\(--text-muted\)|text-neutral-[345]00|#94a3b8|#9ca3af|rgba?\(148,\s*163,\s*184/g;

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (/\.(svelte|ts)$/.test(name)) out.push(p);
  }
  return out;
}

function lineOf(text, index) {
  return text.slice(0, index).split("\n").length;
}

function scanFile(path) {
  const rel = path.slice(ROOT.length + 1);
  if (ALLOWLIST.some(([re]) => re.test(rel))) return [];
  const text = readFileSync(path, "utf8");
  const violations = [];

  // 1) Template/class attribute scan: find class="..." (or class:expression)
  // strings containing both a sub-12px size and a muted color.
  const classAttrRe = /class=(?:"[^"]*"|\{`[^`]*`\})/g;
  let m;
  while ((m = classAttrRe.exec(text))) {
    const attr = m[0];
    const sizes = [...attr.matchAll(SIZE_RE)].map((x) => x[1] ?? x[2]);
    const hasSmall = sizes.some((s) => parseFloat(s) < 12);
    if (hasSmall && COLOR_RE.test(attr)) {
      COLOR_RE.lastIndex = 0;
      violations.push({
        rel,
        line: lineOf(text, m.index),
        kind: "element",
        sample: attr.replace(/\s+/g, " ").slice(0, 110),
      });
    }
    COLOR_RE.lastIndex = 0;
  }

  // 2) <style> block scan: CSS rules combining small font-size + muted color.
  const styleRe = /<style[^>]*>([\s\S]*?)<\/style>/g;
  while ((m = styleRe.exec(text))) {
    const css = m[1];
    const ruleRe = /([^{}]+)\{([^{}]*)\}/g;
    let r;
    while ((r = ruleRe.exec(css))) {
      const body = r[2];
      const sizeMatch = [...body.matchAll(CSS_SIZE_RE)].map((x) => parseFloat(x[1] ?? x[2]));
      const hasSmall = sizeMatch.some((s) => s < 12);
      if (hasSmall && COLOR_RE.test(body)) {
        COLOR_RE.lastIndex = 0;
        violations.push({
          rel,
          line: lineOf(text, m.index + r.index),
          kind: "css-rule",
          sample: `${r[1].trim().replace(/\s+/g, " ").slice(0, 60)} { font-size: ${sizeMatch.join("/")}px; …muted… }`,
        });
      }
      COLOR_RE.lastIndex = 0;
    }
  }

  // 3) Inline style="...font-size:11px...; color: var(--text-muted)"
  const styleAttrRe = /style="[^"]*"/g;
  while ((m = styleAttrRe.exec(text))) {
    const attr = m[0];
    const sizes = [...attr.matchAll(CSS_SIZE_RE)].map((x) => parseFloat(x[1] ?? x[2]));
    if (sizes.some((s) => s < 12) && COLOR_RE.test(attr)) {
      COLOR_RE.lastIndex = 0;
      violations.push({
        rel,
        line: lineOf(text, m.index),
        kind: "inline-style",
        sample: attr.replace(/\s+/g, " ").slice(0, 110),
      });
    }
    COLOR_RE.lastIndex = 0;
  }
  return violations;
}

const files = walk(ROOT);
const all = files.flatMap(scanFile);
if (all.length > 0) {
  console.error(`check-muted-microtext: ✗ ${all.length} small muted-text combination(s) found:`);
  for (const v of all) {
    console.error(`  ${v.rel}:${v.line} [${v.kind}] ${v.sample}`);
  }
  process.exit(1);
}
console.log(`check-muted-microtext: OK — ${files.length} file(s) scanned, no sub-12px muted information text.`);
