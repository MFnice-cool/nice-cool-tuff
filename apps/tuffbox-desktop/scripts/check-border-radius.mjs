#!/usr/bin/env node
/**
 * Guards against re-introducing hardcoded `border-radius` values that
 * duplicate an existing design token (see src/styles.css `--border-radius-*`).
 *
 * Context: a 2026-07 audit found ~140 spots across 36 components using
 * literal `8px`/`12px`/`16px`/`24px` instead of `var(--border-radius-sm|md|lg|xl)`.
 * Those don't respond to the rounded-corners toggle or to "sharp" themes
 * (win95, pixelato, aether, frost) that zero the radius tokens out, so
 * elements stayed visibly rounded where the theme says they shouldn't be.
 * This script keeps that class of regression from creeping back in.
 *
 * It intentionally does NOT flag every hardcoded border-radius (small
 * decorative accents like `3px` dots/swatches, or in-between values like
 * `10px`/`14px`/`20px`, are legitimate one-off sizing and not part of the
 * sm/md/lg/xl scale) — only exact token-value duplicates, which have no
 * excuse to not use the token.
 */
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const componentsRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "src",
  "components",
);

const TOKEN_BY_PX = { 8: "sm", 12: "md", 16: "lg", 24: "xl" };
const VIOLATION_RE = /border-radius:\s*(8|12|16|24)px\s*[;}]/g;

function walk(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full, out);
    else if (entry.name.endsWith(".svelte")) out.push(full);
  }
  return out;
}

let violations = 0;
for (const file of walk(componentsRoot)) {
  const src = readFileSync(file, "utf8");
  const lines = src.split("\n");
  lines.forEach((line, i) => {
    for (const m of line.matchAll(VIOLATION_RE)) {
      violations++;
      const token = TOKEN_BY_PX[m[1]];
      console.error(
        `${path.relative(process.cwd(), file)}:${i + 1}: hardcoded "border-radius: ${m[1]}px" — use var(--border-radius-${token}) instead`,
      );
    }
  });
}

if (violations > 0) {
  console.error(`\n${violations} hardcoded border-radius value(s) duplicate an existing design token. See comment at top of scripts/check-border-radius.mjs.`);
  process.exit(1);
} else {
  console.log("check-border-radius: OK — no hardcoded token-matching border-radius values found.");
}
