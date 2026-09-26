#!/usr/bin/env node
/**
 * Perf-regression guard for the 2026-07 lazy-loading pass (Phase 1).
 *
 * Only the JS + CSS files that `dist/index.html` references directly
 * (<script src>, <link rel="stylesheet">) are guaranteed to load on every
 * startup, on every machine — everything else is behind App.svelte's
 * `VIEW_LOADERS` dynamic imports and only loads when a user opens that view.
 * This script sums the gzip size of exactly those startup-critical files
 * and fails the build if either budget is exceeded, so a future change that
 * accidentally re-adds a heavy static import (see check-lazy-views.mjs) or
 * pulls a large new dependency into the shared/startup chunk gets caught
 * here instead of shipping and re-slowing startup on weak laptops.
 *
 * Budgets have ~35-70% headroom over the current baseline (~163 KB JS /
 * ~17 KB CSS gzipped) for normal growth; run `npm run build` locally and
 * bump these deliberately (with a note why) if a real feature needs it.
 *
 * 2026-08 bump: CSS 30 -> 31 KB for the github-pack-transport line, which
 * legitimately grew the startup stylesheet with the steel-graphite dark theme
 * redesign, the light-theme Home (quartz backdrop + dark content islands), and
 * the quest-editor design-system aliases. Current startup CSS is ~30.0 KB
 * gzipped; 31 KB keeps ~1 KB headroom while still catching accidental bloat.
 *
 * 2026-08 second bump: CSS 31 -> 32 KB — the toggleable home quartz backdrop
 * (theme tokens in styles.css + per-theme overrides in themes.css, home-only
 * Settings toggle) added ~0.2 KB gzipped, landing at 31.2 KB. 32 KB restores
 * headroom while still catching accidental startup-CSS bloat.
 */
import { readFileSync, existsSync } from "node:fs";
import { gzipSync } from "node:zlib";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const distDir = path.join(root, "dist");
const indexHtmlPath = path.join(distDir, "index.html");

const BUDGETS_GZIP_BYTES = {
  ".js": 230 * 1024,
  // 2026-08 (layout-lib): Tailwind v4 theme vars + on-demand utilities for
  // @tuffbox/layout-lib add ~3.3 KB gz; baseline was already ~32.4 KB.
  // 2026-08 second bump (36 -> 40): layout-lib is now actually used by
  // Library (Grid/Stack) and more screens migrate onto it; keep headroom
  // so each migration doesn't require a gate edit.
  // 2026-08 third bump (40 -> 44): Snapshots/Quests/TestRuns Tailwind
  // migrations add per-screen utility sets.
  // 2026-09 bump (44 -> 46): BriefEditor/ProjectSettings/Mods UI polish pass
  // adds category chips, sync pill, rich MdToolbar, and panel hints.
  // 2026-09 second bump (46 -> 48): the typography pass (Inter Variable +
  // JetBrains Mono Variable @font-face in src/styles/fonts.css, latin +
  // cyrillic subsets only; woff2 files load on demand via unicode-range and
  // are not part of this budget) added @font-face declarations, offset by
  // replacing 52 hardcoded monospace stacks with the --font-mono token.
  // Current startup CSS is ~45.4 KB; 48 KB restores headroom while still
  // catching bloat.
  ".css": 48 * 1024,
};

if (!existsSync(indexHtmlPath)) {
  console.error(`check-bundle-budget: ${indexHtmlPath} not found — run "npm run build" first.`);
  process.exit(1);
}

const html = readFileSync(indexHtmlPath, "utf8");
const refs = [];
for (const m of html.matchAll(/(?:src|href)="(\/assets\/[^"]+\.(?:js|css))"/g)) {
  refs.push(m[1]);
}

if (refs.length === 0) {
  console.error("check-bundle-budget: found 0 asset references in dist/index.html — markup may have changed, please check scripts/check-bundle-budget.mjs");
  process.exit(1);
}

let failed = false;
for (const ref of refs) {
  const ext = path.extname(ref);
  const filePath = path.join(distDir, ref.replace(/^\//, ""));
  const raw = readFileSync(filePath);
  const gzip = gzipSync(raw).length;
  const budget = BUDGETS_GZIP_BYTES[ext];
  const kb = (n) => `${(n / 1024).toFixed(1)} KB`;
  if (budget && gzip > budget) {
    failed = true;
    console.error(`✗ ${ref}: ${kb(gzip)} gzipped exceeds ${kb(budget)} startup budget`);
  } else {
    console.log(`✓ ${ref}: ${kb(gzip)} gzipped (budget ${kb(budget ?? Infinity)})`);
  }
}

if (failed) {
  console.error("\nStartup bundle budget exceeded. See comment at top of scripts/check-bundle-budget.mjs.");
  process.exit(1);
}
