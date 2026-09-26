#!/usr/bin/env node
/**
 * Glass-compositing guard: a "glass" surface (an element that paints with
 * `backdrop-filter`) must never get its OWN compositing state mutated by
 * other CSS — `opacity` below 1, `will-change`, `filter`, `mix-blend-mode`,
 * `mask` / `mask-image` / `clip-path`.
 *
 * Context: with "Glass transparency" on, every `.tb-card` (including the
 * Content tab's installed-mod rows) becomes a backdrop-filter surface via
 * `html[data-glass="on"] :where(.card, .panel, .tb-card, …)` in
 * src/styles/themes.css. Toggling a mod flips `.installed-card.disabled`,
 * which set `opacity: 0.72` on that same element. Changing opacity on a
 * backdrop-filter element forces WebView2 to re-group its backdrop layer and
 * re-sample the content behind it; the resample lands at stale coordinates
 * and a square patch of distorted launcher background stays painted over the
 * shell (reported 2026-09 on the Content tab, enable AND disable).
 *
 * Rule of thumb this guard enforces:
 *   - an element that carries backdrop-filter keeps `opacity: 1`, and has no
 *     own `will-change` / `filter` / blend / mask;
 *   - to dim or tint a glass surface, apply opacity to its CHILDREN (children
 *     paint inside the surface and never touch its backdrop sampling);
 *   - deliberately OUT of scope (no artifact reports, banning them would
 *     kill the app's motion design for no real failure mode): one-shot
 *     entrance animations (keyframe opacity/transform at mount) and plain
 *     `transform` (hover lift — the whole layer translates together with its
 *     backdrop).
 *
 * Documented exemptions (both verified against the artifact mechanism):
 *   1. TRANSIENT FEEDBACK ON SMALL CONTROLS — `:hover` / `:active` /
 *      `:disabled`-scoped rules on `button` / `input` / `select` /
 *      `textarea` elements (e.g. the global `button:active { filter:
 *      brightness(0.82) }` press feedback, `:disabled` dimming). Controls
 *      are tiny (no visible square patch), the states are momentary, and
 *      light themes blur `button.secondary` / inputs by design — exempting
 *      them keeps this guard focused on the surfaces where the artifact is
 *      actually visible (cards, panels, modals, rail, header). Layout
 *      surfaces with `:disabled`-style state rules (`.installed-card.disabled`)
 *      are still flagged — exactly the bug that motivated this guard.
 *   2. FULLY HIDDEN ELEMENTS — exact `opacity: 0` (visually-hidden native
 *      inputs, hidden graph handles): nothing is painted, and revealing is
 *      mount-equivalent, which falls under the exempted entrance-animation
 *      class above. Partial opacities (0 < x < 1) on non-controls are
 *      always flagged.
 *
 * Surfaces considered "glass":
 *   1. elements whose class list intersects the glass-contract classes
 *      (extracted live from the `html[data-glass="on"] :where(…)` rules in
 *      src/styles/themes.css, plus `.rail` / `.header` / `.modal` /
 *      `.glass-panel`; hardcoded fallback below), or the `dialog` tag;
 *   2. elements matched by any rule (global or component) that itself
 *      declares `backdrop-filter` — including the same rule carrying a
 *      banned property.
 *
 * Matching is a conservative static approximation: Svelte markup is parsed
 * into an element tree with parent chains (static `class="…"` plus
 * `class:foo` directives), selectors are matched by class-token subsets;
 * conditional classes count as present (worst case), pseudo-classes as
 * wildcards, `:not(.x)` as an exclusion.
 *
 * Suppression: `glass-compositing: ignore(<reason>)` in a CSS comment inside
 * the rule (or on the line directly above its selector). A reason is
 * mandatory; the ignore count is printed.
 */
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "src");

const FALLBACK_CONTRACT_CLASSES = [
  "card", "panel", "tb-card", "projects-section", // glass card set (themes.css)
  "rail", "header",                               // ambient chrome
  "modal", "modal-card",                          // dialogs
  "glass-panel",                                  // explicit utility (styles.css)
];

const BANNED = [
  { props: ["opacity"], why: "opacity < 1", banned: (v) => Number.isFinite(parseFloat(v)) && parseFloat(v) < 1 },
  { props: ["will-change"], why: "will-change", banned: (v) => v.trim() !== "auto" },
  { props: ["filter"], why: "filter", banned: (v) => v.trim() !== "none" },
  { props: ["mix-blend-mode"], why: "mix-blend-mode", banned: (v) => v.trim() !== "normal" },
  { props: ["mask", "mask-image", "clip-path"], why: "mask/clip-path", banned: (v) => v.trim() !== "none" },
];

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

function walk(dir, exts, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full, exts, out);
    else if (exts.some((e) => entry.name.endsWith(e))) out.push(full);
  }
  return out;
}

const svelteFiles = walk(ROOT, [".svelte"]);
const globalCssFiles = [
  path.join(ROOT, "styles.css"),
  ...walk(path.join(ROOT, "styles"), [".css"]),
].filter((f) => {
  try { return readFileSync(f, "utf8").length >= 0; } catch { return false; }
});

// ---------------------------------------------------------------------------
// CSS parsing
// ---------------------------------------------------------------------------

/** Strip comments but keep suppression-marker lines intact. */
function stripComments(src) {
  return src
    .split("\n")
    .map((line) =>
      /glass-compositing:\s*ignore/i.test(line)
        ? line
        : line.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " ")),
    )
    .join("\n");
}

/** Parse flat + at-wrapped rules into { file, selector, line, endLine, decls } */
function parseRules(css, file) {
  const rules = [];
  const lineAt = (idx) => css.slice(0, idx).split("\n").length;
  let i = 0;

  function blockBounds(open) {
    let depth = 0;
    for (let k = open; k < css.length; k++) {
      if (css[k] === "{") depth++;
      else if (css[k] === "}") {
        depth--;
        if (depth === 0) return k;
      }
    }
    return css.length - 1;
  }

  function handle(prelude, preludeIdx, open) {
    const close = blockBounds(open);
    const body = css.slice(open + 1, close);
    if (body.includes("{")) {
      // at-rule (or garbage) with nested blocks — recurse one level
      let cursor = open + 1;
      while (cursor < close) {
        const nextOpen = css.indexOf("{", cursor);
        if (nextOpen === -1 || nextOpen >= close) break;
        const nestedPrelude = css.slice(cursor, nextOpen).trim();
        const nestedClose = blockBounds(nextOpen);
        if (nestedPrelude && !/^@(keyframes|font-face|page)/i.test(nestedPrelude)) {
          handle(nestedPrelude, lineAt(cursor), nextOpen);
        }
        cursor = nestedClose + 1;
      }
    } else {
      const decls = new Map();
      for (const decl of body.split(";")) {
        const idx = decl.indexOf(":");
        if (idx === -1) continue;
        const prop = decl.slice(0, idx).trim().toLowerCase();
        const value = decl.slice(idx + 1).trim();
        if (prop) decls.set(prop, value);
      }
      rules.push({
        file,
        selector: prelude.replace(/\s+/g, " ").trim(),
        line: lineAt(preludeIdx),
        endLine: lineAt(close),
        decls,
      });
    }
    return close + 1;
  }

  while (i < css.length) {
    while (i < css.length && /[\s;]/.test(css[i])) i++;
    if (i >= css.length) break;
    let j = i;
    while (j < css.length && css[j] !== "{" && css[j] !== ";") j++;
    if (j >= css.length) break;
    if (css[j] === ";") { i = j + 1; continue; }
    const prelude = css.slice(i, j).trim();
    if (!prelude) { i = j + 1; continue; }
    i = handle(prelude, i, j);
  }
  return rules;
}

// ---------------------------------------------------------------------------
// Selector parsing / matching
// ---------------------------------------------------------------------------

function splitTopLevel(str, sepChars) {
  const parts = [];
  let depth = 0;
  let cur = "";
  for (const ch of str) {
    if (ch === "(" || ch === "[") depth++;
    else if (ch === ")" || ch === "]") depth--;
    if (depth === 0 && sepChars.includes(ch)) { parts.push(cur); cur = ""; }
    else cur += ch;
  }
  parts.push(cur);
  return parts;
}

/** complex selector → [{ compound, comb }] with comb ∈ " " | ">" (before compound) */
function parseComplex(sel) {
  const tokens = [];
  let cur = "";
  let comb = " ";
  const flush = () => {
    if (cur.trim()) { tokens.push({ compound: cur.trim(), comb }); comb = " "; }
    cur = "";
  };
  for (let i = 0; i < sel.length; i++) {
    const ch = sel[i];
    if (ch === "(" || ch === "[") {
      const open = ch, close = ch === "(" ? ")" : "]";
      let depth = 0, j = i;
      for (; j < sel.length; j++) {
        if (sel[j] === open) depth++;
        else if (sel[j] === close) { depth--; if (depth === 0) break; }
      }
      cur += sel.slice(i, j + 1);
      i = j;
    } else if (ch === ">") { flush(); comb = ">"; }
    else if (/\s/.test(ch)) flush();
    else cur += ch;
  }
  flush();
  return tokens;
}

/** mini-compound inside :is/:where/:not — tag + classes (attrs/pseudos = wildcards) */
function parseMini(comp) {
  const classes = new Set();
  let tag = null;
  let i = 0;
  while (i < comp.length) {
    const ch = comp[i];
    if (ch === ".") {
      let j = i + 1;
      while (j < comp.length && /[\w-]/.test(comp[j])) j++;
      classes.add(comp.slice(i + 1, j));
      i = j;
    } else if (ch === ":" || ch === "[") {
      const j = ch === ":"
        ? i + 1
        : (() => { const k = comp.indexOf("]", i); return k === -1 ? comp.length : k + 1; })();
      // skip pseudo-class / attribute (pseudo-element marker handled at subject level)
      i = j;
    } else if (/[\w-]/.test(ch)) {
      let j = i;
      while (j < comp.length && /[\w-]/.test(comp[j])) j++;
      tag = comp.slice(i, j).toLowerCase();
      i = j;
    } else i++;
  }
  return { classes, tag };
}

function miniMatches(mini, el) {
  if (mini.tag && mini.tag !== "*" && mini.tag !== el.tag) return false;
  for (const c of mini.classes) if (!el.classes.has(c)) return false;
  return true;
}

function parseCompound(comp) {
  const classes = new Set();
  const anyOfGroups = []; // each: mini[] — at least one must match
  const notMinis = [];    // none may match
  let tag = null;
  let i = 0;
  while (i < comp.length) {
    const ch = comp[i];
    if (ch === ".") {
      let j = i + 1;
      while (j < comp.length && /[\w-]/.test(comp[j])) j++;
      classes.add(comp.slice(i + 1, j));
      i = j;
    } else if (ch === ":") {
      let j = i + 1;
      while (j < comp.length && /[\w-]/.test(comp[j])) j++;
      const name = comp.slice(i + 1, j).toLowerCase();
      let arg = null;
      if (comp[j] === "(") {
        let depth = 0, k = j;
        for (; k < comp.length; k++) {
          if (comp[k] === "(") depth++;
          else if (comp[k] === ")") { depth--; if (depth === 0) break; }
        }
        arg = comp.slice(j + 1, k);
        j = k + 1;
      }
      if ((name === "is" || name === "where") && arg) {
        const group = splitTopLevel(arg, ",")
          .map((p) => parseMini(p.trim()))
          .filter((mini) => mini.classes.size || mini.tag);
        if (group.length) anyOfGroups.push(group);
      } else if (name === "not" && arg) {
        for (const p of splitTopLevel(arg, ",")) {
          const mini = parseMini(p.trim());
          if (mini.classes.size || mini.tag) notMinis.push(mini);
        }
      }
      i = j; // every other pseudo-class is a wildcard for matching purposes
    } else if (ch === "[") {
      const j = comp.indexOf("]", i);
      i = j === -1 ? comp.length : j + 1;
    } else if (ch === "*") {
      tag = "*";
      i++;
    } else if (/[\w-]/.test(ch)) {
      let j = i;
      while (j < comp.length && /[\w-]/.test(comp[j])) j++;
      tag = comp.slice(i, j).toLowerCase();
      i = j;
    } else i++;
  }
  return { classes, anyOfGroups, notMinis, tag };
}

function compoundMatches(parsed, el) {
  if (parsed.tag && parsed.tag !== "*" && parsed.tag !== el.tag) return false;
  for (const c of parsed.classes) if (!el.classes.has(c)) return false;
  for (const group of parsed.anyOfGroups) {
    if (!group.some((mini) => miniMatches(mini, el))) return false;
  }
  for (const mini of parsed.notMinis) {
    if (miniMatches(mini, el)) return false;
  }
  return true;
}

function selectorMatches(tokens, el) {
  const subjectParsed = parseCompound(tokens[tokens.length - 1].compound);
  if (!compoundMatches(subjectParsed, el)) return false;
  let node = el;
  for (let t = tokens.length - 2; t >= 0; t--) {
    const comb = tokens[t + 1].comb;
    const parsed = parseCompound(tokens[t].compound);
    if (comb === ">") {
      node = node.parent;
      if (!node || !compoundMatches(parsed, node)) return false;
    } else {
      let up = node.parent;
      while (up && !compoundMatches(parsed, up)) up = up.parent;
      if (!up) return false;
      node = up;
    }
  }
  return true;
}

function selectorMatchesAny(sel, el) {
  return splitTopLevel(sel, ",")
    .map((s) => s.trim().replace(/^:global\(([\s\S]*)\)$/, "$1"))
    .filter(Boolean)
    .some((s) => {
      // declarations on a pseudo-element (::before/::after/…) do not style the
      // element itself — never treat such rules as element matches
      if (/::[\w-]+(\(.*\))?\s*$/.test(s)) return false;
      const tokens = parseComplex(s);
      return tokens.length > 0 && selectorMatches(tokens, el);
    });
}

// ---------------------------------------------------------------------------
// Svelte markup → element trees (parent chains)
// ---------------------------------------------------------------------------

const VOID_TAGS = new Set([
  "area", "base", "br", "col", "embed", "hr", "img", "input", "link",
  "meta", "param", "source", "track", "wbr",
]);

function templateOf(src) {
  return src
    .replace(/<script[\s\S]*?<\/script>/g, (m) => m.replace(/[^\n]/g, " "))
    .replace(/<style[^>]*>[\s\S]*?<\/style>/g, (m) => m.replace(/[^\n]/g, " "));
}

function styleOf(src) {
  const m = src.match(/<style[^>]*>([\s\S]*?)<\/style>/);
  return m ? m[1] : "";
}

function parseElements(tpl, file) {
  const elements = [];
  const stack = [];
  const tagRe = /<(\/?)([A-Za-z][\w.-]*)((?:"[^"]*"|'[^']*'|[^"'>])*)(\/?)>/g;
  const lineAt = (idx) => tpl.slice(0, idx).split("\n").length;
  let m;
  while ((m = tagRe.exec(tpl)) !== null) {
    const [, closing, rawTag, attrs, selfClose] = m;
    const tag = rawTag.toLowerCase();
    const line = lineAt(m.index);
    if (closing) {
      for (let s = stack.length - 1; s >= 0; s--) {
        if (stack[s].rawTag === rawTag || stack[s].tag === tag) { stack.length = s; break; }
      }
      continue;
    }
    const classes = new Set();
    for (const cm of attrs.matchAll(/\bclass\s*=\s*(?:"([^"]*)"|'([^']*)')/g)) {
      const raw = cm[1] ?? cm[2] ?? "";
      // static tokens plus string literals inside svelte expressions
      for (const lit of raw.matchAll(/"([^"]*)"|'([^']*)'|([^\s"'{}]+)/g)) {
        const tok = lit[1] ?? lit[2] ?? lit[3] ?? "";
        for (const t of tok.split(/\s+/)) if (/^[\w-]+$/.test(t)) classes.add(t);
      }
    }
    for (const d of attrs.matchAll(/\bclass:([\w-]+)/g)) classes.add(d[1]);
    const el = { tag, rawTag, classes, parent: stack[stack.length - 1] ?? null, file, line };
    elements.push(el);
    stack.push(el);
    if (selfClose || VOID_TAGS.has(tag)) stack.pop();
  }
  return elements;
}

// ---------------------------------------------------------------------------
// Load everything
// ---------------------------------------------------------------------------

const globalRules = [];
const globalSrc = new Map();
for (const file of globalCssFiles) {
  const src = readFileSync(file, "utf8");
  globalSrc.set(file, src);
  globalRules.push(...parseRules(stripComments(src), file));
}

const componentFiles = [];
const allElements = [];
for (const file of svelteFiles) {
  const src = readFileSync(file, "utf8");
  const tpl = templateOf(src);
  const elements = parseElements(tpl, file);
  allElements.push(...elements);
  const styleCss = stripComments(styleOf(src));
  const styleOffset = src.indexOf(styleOf(src));
  const baseLine = src.slice(0, styleOffset).split("\n").length;
  const rules = parseRules(styleCss, file).map((r) => ({ ...r, line: r.line + baseLine - 1, endLine: r.endLine + baseLine - 1 }));
  componentFiles.push({ file, src, rules });
}

// Glass-contract classes from themes.css `html[data-glass="on"] :where(…)`
const contractClasses = new Set(FALLBACK_CONTRACT_CLASSES);
const themesFile = globalCssFiles.find((f) => f.endsWith("themes.css"));
if (themesFile) {
  const found = new Set();
  for (const m of globalSrc.get(themesFile).matchAll(/html\[data-glass="on"\][^{}]*?:where\(([^)]*)\)/g)) {
    for (const sel of splitTopLevel(m[1], ",")) {
      for (const c of sel.matchAll(/\.([\w-]+)/g)) found.add(c[1]);
    }
  }
  if (found.size) {
    contractClasses.clear();
    for (const c of found) contractClasses.add(c);
  }
  contractClasses.add("glass-panel");
}

const allRules = [...globalRules, ...componentFiles.flatMap((f) => f.rules)];
const blurRules = allRules.filter((r) => {
  const bf = r.decls.get("backdrop-filter") ?? r.decls.get("-webkit-backdrop-filter");
  return bf && bf.trim() !== "none";
});

function isContractSurface(el) {
  if (el.tag === "dialog") return true;
  for (const c of el.classes) if (contractClasses.has(c)) return true;
  return false;
}

/** Element is a surface if contract-matched, or matched by any blur rule. */
function isSurface(el) {
  if (isContractSurface(el)) return true;
  for (const rule of blurRules) {
    const scoped = rule.file.endsWith(".svelte");
    if (scoped && rule.file !== el.file) continue;
    if (selectorMatchesAny(rule.selector, el)) return true;
  }
  return false;
}

// ---------------------------------------------------------------------------
// Violations
// ---------------------------------------------------------------------------

const suppressRe = /glass-compositing:\s*ignore\s*\(([^)]*)\)/i;

function suppressed(rule, src) {
  if (!src) return false;
  const lines = src.split("\n");
  for (let ln = Math.max(1, rule.line - 2); ln <= rule.endLine; ln++) {
    const m = (lines[ln - 1] ?? "").match(suppressRe);
    if (m) {
      if (!m[1]?.trim()) {
        console.error(`${path.relative(process.cwd(), rule.file)}:${ln}: glass-compositing: ignore() requires a reason`);
        process.exit(1);
      }
      return true;
    }
  }
  return false;
}

function ruleSrc(rule) {
  return rule.file.endsWith(".svelte")
    ? componentFiles.find((f) => f.file === rule.file)?.src
    : globalSrc.get(rule.file) ?? null;
}

function surfaceSource(el) {
  if (el.tag === "dialog") return "tag dialog";
  for (const c of el.classes) if (contractClasses.has(c)) return `contract class .${c}`;
  for (const rule of blurRules) {
    const scoped = rule.file.endsWith(".svelte");
    if (scoped && rule.file !== el.file) continue;
    if (selectorMatchesAny(rule.selector, el)) {
      return `backdrop-filter rule ${path.relative(process.cwd(), rule.file)}:${rule.line}`;
    }
  }
  return null;
}

const CONTROL_TAGS = new Set(["button", "input", "select", "textarea"]);
/** Exemption 1: transient feedback scoped to hover/active/disabled states. */
const CONTROL_STATE_RE = /(:hover|:active|:disabled|\[disabled)|(\.disabled\b)/i;
/** Exemption 2: exact `opacity: 0` — fully hidden, nothing painted. */
function isHiddenOpacity(value) {
  return /^\s*0\s*(!important)?\s*$/.test(value);
}

const violations = [];
let ignored = 0;

for (const rule of allRules) {
  const bannedHere = [];
  for (const check of BANNED) {
    for (const prop of check.props) {
      const value = rule.decls.get(prop);
      if (value && check.banned(value)) bannedHere.push({ check, prop, value });
    }
  }
  if (!bannedHere.length) continue;
  const declaresBlur =
    (rule.decls.get("backdrop-filter") ?? rule.decls.get("-webkit-backdrop-filter") ?? "none").trim() !== "none";
  const scoped = rule.file.endsWith(".svelte");

  for (const el of allElements) {
    if (scoped && el.file !== rule.file) continue;
    if (!selectorMatchesAny(rule.selector, el)) continue;
    const source = declaresBlur ? "same rule declares backdrop-filter" : surfaceSource(el);
    if (!source) continue;
    // Documented exemptions — see header
    if (CONTROL_TAGS.has(el.tag) && CONTROL_STATE_RE.test(rule.selector)) break; // transient control feedback
    if (bannedHere.length === 1 && bannedHere[0].prop === "opacity" && isHiddenOpacity(bannedHere[0].value)) break; // fully hidden
    if (suppressed(rule, ruleSrc(rule))) { ignored += bannedHere.length; break; }
    const rel = path.relative(process.cwd(), rule.file);
    for (const { check, prop, value } of bannedHere) {
      violations.push(
        `${rel}:${rule.line}: \`${rule.selector}\` sets ${check.why} (\`${prop}: ${value}\`) on a backdrop-filter glass surface\n` +
        `    (<${el.tag} class="${[...el.classes].join(" ")}" at ${path.relative(process.cwd(), el.file)}:${el.line}>; surface via ${source}).\n` +
        `    Mutating a glass surface's own compositing state makes WebView2 re-sample its backdrop → square artifact over the launcher background.\n` +
        `    Fix: apply ${check.why} to CHILD elements instead (see header of scripts/check-glass-compositing.mjs), or suppress with a reason.`,
      );
    }
    break; // one report per rule is enough
  }
}

if (violations.length) {
  console.error("check-glass-compositing: ✗ compositing mutation(s) on glass (backdrop-filter) surfaces:\n");
  for (const v of violations) console.error(`  • ${v}\n`);
  console.error(`${violations.length} violation(s), ${ignored} documented ignore(s).`);
  process.exit(1);
} else {
  console.log(
    `check-glass-compositing: OK — ${allElements.length} element(s) × ${allRules.length} rule(s) scanned across ${svelteFiles.length} component(s) + ${globalCssFiles.length} stylesheet(s), ${ignored} documented ignore(s).`,
  );
}
