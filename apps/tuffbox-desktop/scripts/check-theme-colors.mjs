#!/usr/bin/env node
/**
 * Raw-colour audit: finds hardcoded colours in component styles that bypass the
 * theme tokens. A literal here usually means the element keeps its dark-theme
 * (or light-theme) skin in every other theme — that is how Recipes / Quests /
 * History / Test ended up with unreadable pills and black slabs.
 *
 * Usage:
 *   node scripts/check-theme-colors.mjs            # report (always exit 0)
 *   node scripts/check-theme-colors.mjs --strict   # exit 1 when findings exist
 *   node scripts/check-theme-colors.mjs --all      # scan the whole src tree
 *   node scripts/check-theme-colors.mjs path…      # scan specific files/dirs
 *
 * Deliberately allowed:
 *   - pure neutrals used as shading masks: #000 / #fff / #000000 / #ffffff,
 *     rgba(0,0,0,…) and rgba(255,255,255,…) (they are theme-independent by
 *     construction — a darkener/lightener, not a skin colour);
 *   - colours inside color-mix() whose only literal operand is such a neutral
 *     (e.g. `color-mix(in srgb, var(--accent-primary) 60%, #000)`);
 *   - var() fallbacks are reported separately: they only apply when the token is
 *     missing, so they are a lower-severity smell than a bare literal.
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, extname } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname.replace(/\/$/, "");
const SRC = join(ROOT, "src");

const args = process.argv.slice(2);
const strict = args.includes("--strict");
const all = args.includes("--all");
const targets = args.filter((a) => !a.startsWith("--"));

/** Screens in scope by default: the IDE tabs this audit exists for. */
const DEFAULT_TARGETS = [
  "src/components/TestRuns.svelte",
  "src/components/test",
  "src/components/ChangeHistory.svelte",
  "src/components/RecipeBrowser.svelte",
  "src/components/QuestEditor.svelte",
  "src/components/quests",
  "src/components/ide",
].map((p) => join(ROOT, p));

const NEUTRAL = new Set(["#000", "#fff", "#000000", "#ffffff"]);
const EXT = new Set([".svelte", ".css"]);
const HEX = /#[0-9a-fA-F]{3,8}\b/g;
const RGB = /rgba?\(\s*[\d.]+/g;

function walk(path, out = []) {
  let st;
  try {
    st = statSync(path);
  } catch {
    return out;
  }
  if (st.isDirectory()) {
    if (/node_modules|dist|\.svelte-kit/.test(path)) return out;
    for (const entry of readdirSync(path)) walk(join(path, entry), out);
    return out;
  }
  if (EXT.has(extname(path))) out.push(path);
  return out;
}

/** True when the literal only darkens/lightens another colour inside color-mix(). */
function isMixNeutral(line, index, literal) {
  const before = line.slice(0, index);
  const open = before.lastIndexOf("color-mix(");
  if (open !== -1 && !before.slice(open).includes(")")) {
    return NEUTRAL.has(literal.toLowerCase());
  }
  return false;
}

const files = [];
for (const t of targets.length ? targets : all ? [SRC] : DEFAULT_TARGETS) {
  if (!t.startsWith("/")) walk(join(ROOT, t), files);
  else walk(t, files);
}

let findings = 0;
let fallbacks = 0;
const perFile = new Map();

for (const file of files) {
  const text = readFileSync(file, "utf8");
  const lines = text.split("\n");
  const hits = [];
  lines.forEach((line, i) => {
    for (const re of [HEX, RGB]) {
      re.lastIndex = 0;
      let m;
      while ((m = re.exec(line))) {
        const literal = m[0];
        if (re === HEX && NEUTRAL.has(literal.toLowerCase())) continue;
        if (re === RGB && /rgba?\(\s*(0|255)\s*,\s*(0|255)\s*,\s*(0|255)\s*[,)]/.test(literal)) {
          const parts = /rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)/.exec(m[0]);
          if (parts && (parts[1] === parts[2]) && (parts[2] === parts[3]) && (parts[1] === "0" || parts[1] === "255")) continue;
        }
        if (re === HEX && isMixNeutral(line, m.index, literal)) continue;
        if (re === RGB) {
          const after = line.slice(m.index, m.index + 40);
          const nums = after.match(/[\d.]+/g) ?? [];
          if (nums.length >= 3 && (nums[0] === nums[1]) && (nums[1] === nums[2]) && (nums[0] === "0" || nums[0] === "255")) continue;
          if (isMixNeutral(line, m.index, "#000")) continue;
        }
        const isFallback = /var\([^)]*$/.test(line.slice(0, m.index));
        hits.push({ line: i + 1, literal, isFallback, text: line.trim().slice(0, 110) });
      }
    }
  });
  if (hits.length) {
    perFile.set(file, hits);
    findings += hits.length;
    fallbacks += hits.filter((h) => h.isFallback).length;
  }
}

const rel = (p) => relative(ROOT, p);
if (findings === 0) {
  console.log(`check-theme-colors: OK — ${files.length} file(s) scanned, no raw colours outside the token set.`);
} else {
  console.log(`check-theme-colors: ${findings} raw colour literal(s) in ${perFile.size}/${files.length} file(s) — ${fallbacks} of them are var() fallbacks.`);
  for (const [file, hits] of perFile) {
    console.log(`\n  ${rel(file)}`);
    for (const h of hits.slice(0, 8)) console.log(`    ${h.line}: ${h.literal}${h.isFallback ? " (var fallback)" : ""}  ${h.text}`);
    if (hits.length > 8) console.log(`    … ${hits.length - 8} more`);
  }
  console.log("\nFix: use var(--…) theme tokens (bg/text/accent/on-accent). See docs/02-stack.md → theming.");
  if (!strict) console.log("(report only — pass --strict to fail on findings)");
}
process.exit(strict && findings > 0 ? 1 : 0);
