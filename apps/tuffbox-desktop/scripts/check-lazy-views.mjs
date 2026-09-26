#!/usr/bin/env node
/**
 * Guards against silently un-doing the App.svelte lazy-loading work (2026-07
 * perf pass, Phase 1): every heavy view routed through `VIEW_LOADERS` /
 * `import(...)` must NOT also be statically imported at the top of the file.
 * A static import defeats the whole point — the component (and its full
 * dependency tree, e.g. ConfigEditor/IdeWorkspace/Mods/skinview3d) would be
 * bundled into the eager startup chunk again regardless of the dynamic
 * `import()` call still being present, silently bloating and slowing down
 * startup on every machine, weak ones especially.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const appSveltePath = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "src",
  "App.svelte",
);

const src = readFileSync(appSveltePath, "utf8");

const lazyPaths = new Set();
for (const m of src.matchAll(/=>\s*import\("(\.\/components\/[^"]+)"\)/g)) {
  lazyPaths.add(m[1]);
}

if (lazyPaths.size === 0) {
  console.error("check-lazy-views: found 0 entries in VIEW_LOADERS — regex may be stale, please check scripts/check-lazy-views.mjs against App.svelte");
  process.exit(1);
}

// Only look at the <script> section before the component markup, and only
// at plain `import X from "..."` (default-import) statements — that's the
// only form that would eagerly bundle a lazy view.
const scriptEnd = src.indexOf("</script>");
const scriptSrc = scriptEnd === -1 ? src : src.slice(0, scriptEnd);

let violations = 0;
for (const m of scriptSrc.matchAll(/^\s*import\s+\w+\s+from\s+"(\.\/components\/[^"]+)"/gm)) {
  if (lazyPaths.has(m[1])) {
    violations++;
    console.error(`App.svelte: "${m[1]}" is statically imported but is also in VIEW_LOADERS (dynamic import) — remove the static import, it defeats lazy-loading.`);
  }
}

if (violations > 0) {
  console.error(`\n${violations} lazy-loaded view(s) are also statically imported. See comment at top of scripts/check-lazy-views.mjs.`);
  process.exit(1);
} else {
  console.log(`check-lazy-views: OK — ${lazyPaths.size} lazy views are not statically imported.`);
}
