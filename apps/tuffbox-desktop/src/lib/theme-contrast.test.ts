/**
 * Theme contrast regression test — WCAG 2.1 AA for every theme token pair.
 *
 * Why a test: "white text on a white background" is invisible to a human
 * skimming the launcher in one theme and impossible to check by hand across
 * 18 themes × dozens of token pairs. This test parses the *actual* CSS
 * (`styles.css` + `styles/themes.css`), resolves every token per theme
 * (hex / rgb(a) / var() / color-mix() / transparent + alpha compositing) and
 * fails with the exact pair, colors and measured ratio whenever a combination
 * drops below its WCAG threshold:
 *
 *   - body text tokens (primary/secondary/muted on every surface) — 4.5:1
 *   - --on-accent ink on accent fills (primary/hover/danger buttons) — 4.5:1
 *   - accents used as text/labels on surfaces — 3.0:1 (large/bold per WCAG)
 *   - account badge foregrounds on their composited badge backgrounds — 4.5:1
 *
 * Unresolvable pairs also fail loudly: a token that cannot be resolved is
 * exactly how invisible-text regressions slip through review.
 *
 * Thresholds follow WCAG 2.1 (https://www.w3.org/WAI/WCAG21/Understanding/contrast-minimum.html).
 */
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));
const SRC_DIR = join(HERE, "..");
const THEMES_TS = join(HERE, "themes.ts");

// ─── CSS parsing ─────────────────────────────────────────────────────────────

type CssBlock = { selector: string; body: string };

/** Split CSS into top-level `{ selector: body }` blocks (nested braces tracked). */
function parseBlocks(css: string): CssBlock[] {
  const blocks: CssBlock[] = [];
  let i = 0;
  while (i < css.length) {
    const open = css.indexOf("{", i);
    if (open === -1) break;
    // The "selector" is whatever sits between the previous block (or a
    // `;`-terminated at-rule like @import/@source) and this `{`. Preambles
    // must not glue themselves onto the first selector, or that block's
    // tokens (here: the whole :root) silently disappear from the audit.
    const head = css.slice(i, open);
    const lastSemi = head.lastIndexOf(";");
    const selector = (lastSemi === -1 ? head : head.slice(lastSemi + 1)).trim();
    let depth = 1;
    let j = open + 1;
    while (j < css.length && depth > 0) {
      if (css[j] === "{") depth++;
      else if (css[j] === "}") depth--;
      j++;
    }
    blocks.push({ selector, body: css.slice(open + 1, j - 1) });
    i = j;
  }
  return blocks;
}

/**
 * Theme ids a selector list addresses, or null when the selector is scoped
 * (`.home`, `button`, `:is(...) .qe` …) — scoped blocks are not theme roots.
 */
function themeIdsFromSelector(selector: string): string[] | null {
  const parts = selector.split(",").map((s) => s.trim()).filter(Boolean);
  if (!parts.length) return null;
  const ids: string[] = [];
  for (const part of parts) {
    if (part === ":root") {
      ids.push(":root");
      continue;
    }
    const m = /^\[data-theme="([^"]+)"\]$/.exec(part);
    if (!m) return null; // any non-theme compound → scoped rule
    ids.push(m[1]);
  }
  return ids;
}

function extractVars(body: string): Record<string, string> {
  const vars: Record<string, string> = {};
  const re = /(--[a-z0-9-]+)\s*:\s*([^;]+);/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(body))) vars[m[1]] = m[2].trim();
  return vars;
}

// ─── Color resolution ────────────────────────────────────────────────────────

type Rgb = { r: number; g: number; b: number; a: number };

function hexToRgb(hex: string): Rgb | null {
  let h = hex.replace("#", "");
  if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join("");
  if (h.length !== 6 && h.length !== 8) return null;
  const r = parseInt(h.slice(0, 2), 16);
  const g = parseInt(h.slice(2, 4), 16);
  const b = parseInt(h.slice(4, 6), 16);
  const a = h.length === 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1;
  if ([r, g, b, a].some((v) => Number.isNaN(v))) return null;
  return { r, g, b, a };
}

/** Resolve a CSS color value against a token map. Returns null when unsupported. */
function resolveColor(value: string | undefined, vars: Record<string, string>, depth = 0): Rgb | null {
  if (!value || depth > 16) return null;
  let v = value.trim();

  // var() — follow the reference (or its fallback).
  const varMatch = /^var\(\s*(--[a-z0-9-]+)\s*(?:,\s*(.+))?\)$/is.exec(v);
  if (varMatch) {
    const ref = vars[varMatch[1]];
    return resolveColor(ref ?? varMatch[2], vars, depth + 1);
  }

  if (v === "transparent") return { r: 0, g: 0, b: 0, a: 0 };
  if (v.startsWith("#")) return hexToRgb(v);

  const rgb =
    /^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*(?:,\s*([\d.]+)\s*)?\)$/.exec(v) ??
    /^rgba?\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*(?:\/\s*([\d.]+|%)\s*)?\)$/.exec(v);
  if (rgb) {
    let a = 1;
    if (rgb[4] !== undefined) a = rgb[4].endsWith("%") ? parseFloat(rgb[4]) / 100 : parseFloat(rgb[4]);
    return { r: parseFloat(rgb[1]), g: parseFloat(rgb[2]), b: parseFloat(rgb[3]), a };
  }

  const mix = /^color-mix\(\s*in\s+srgb\s*,\s*(.+?)\s*,\s*(.+)\)$/is.exec(v);
  if (mix) {
    let [, aRaw, bRaw] = mix;
    let pctA = 0.5;
    // Percentage may trail either operand ("color 40%") or lead it ("40% color").
    let m = /^(.*?)\s+([\d.]+)%$/s.exec(aRaw);
    if (m) {
      aRaw = m[1];
      pctA = parseFloat(m[2]) / 100;
    } else {
      m = /^([\d.]+)%\s+(.*)$/s.exec(aRaw);
      if (m) {
        pctA = parseFloat(m[1]) / 100;
        aRaw = m[2];
      }
    }
    let pctB = 1 - pctA;
    m = /^(.*?)\s+([\d.]+)%$/s.exec(bRaw);
    if (m) {
      bRaw = m[1];
      pctB = parseFloat(m[2]) / 100;
    }
    const ca = resolveColor(aRaw, vars, depth + 1);
    const cb = resolveColor(bRaw, vars, depth + 1);
    if (!ca || !cb) return null;
    return {
      r: ca.r * pctA + cb.r * pctB,
      g: ca.g * pctA + cb.g * pctB,
      b: ca.b * pctA + cb.b * pctB,
      a: ca.a * pctA + cb.a * pctB,
    };
  }
  return null;
}

/** Alpha-composite `fg` over opaque `bg`. */
function compositeOver(fg: Rgb, bg: Rgb): Rgb {
  const a = fg.a;
  return {
    r: fg.r * a + bg.r * (1 - a),
    g: fg.g * a + bg.g * (1 - a),
    b: fg.b * a + bg.b * (1 - a),
    a: 1,
  };
}

// ─── WCAG 2.1 contrast ───────────────────────────────────────────────────────

function relativeLuminance({ r, g, b }: Rgb): number {
  const channel = (c: number) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

function contrastRatio(fg: Rgb, bg: Rgb): number {
  const l1 = relativeLuminance(fg);
  const l2 = relativeLuminance(bg);
  const [hi, lo] = l1 > l2 ? [l1, l2] : [l2, l1];
  return (hi + 0.05) / (lo + 0.05);
}

// ─── Theme token maps ────────────────────────────────────────────────────────

const css = [
  readFileSync(join(SRC_DIR, "styles.css"), "utf8"),
  readFileSync(join(SRC_DIR, "styles", "themes.css"), "utf8"),
]
  .join("\n")
  // Comments must not leak into selectors (a leading /* … */ block otherwise
  // glues itself onto the first selector and hides that theme's tokens).
  .replace(/\/\*[\s\S]*?\*\//g, "");

const blocksByTheme = new Map<string, Record<string, string>>();
for (const block of parseBlocks(css)) {
  const ids = themeIdsFromSelector(block.selector);
  if (!ids) continue;
  const vars = extractVars(block.body);
  for (const id of ids) {
    const merged = { ...(blocksByTheme.get(id) ?? {}), ...vars };
    blocksByTheme.set(id, merged);
  }
}

/** Canonical theme catalog, kept in sync with src/lib/themes.ts automatically. */
const THEME_IDS = [...readFileSync(THEMES_TS, "utf8").matchAll(/\bid:\s*"([a-z0-9-]+)"/g)].map(
  (m) => m[1],
);

/** Per-theme tokens: :root defaults + the theme's own overrides. */
const ROOT_VARS = blocksByTheme.get(":root") ?? {};
const themeTokens = new Map<string, Record<string, string>>(
  THEME_IDS.map((id) => [id, { ...ROOT_VARS, ...(blocksByTheme.get(id) ?? {}) }]),
);

// ─── The audit ───────────────────────────────────────────────────────────────

/** [fgToken, bgToken, minimum ratio] — tokens without the leading `--`. */
type Pair = [fg: string, bg: string, min: number];

const SURFACES = ["bg-primary", "bg-secondary", "bg-tertiary", "bg-elevated", "bg-hover", "bg-active"] as const;

const BODY_PAIRS: Pair[] = [
  ...SURFACES.map((s) => ["text-primary", s, 4.5] as Pair),
  ...["bg-primary", "bg-secondary", "bg-tertiary", "bg-elevated"].map(
    (s) => ["text-secondary", s, 4.5] as Pair,
  ),
  ...["bg-primary", "bg-secondary", "bg-tertiary", "bg-elevated"].map(
    (s) => ["text-muted", s, 4.5] as Pair,
  ),
  ["on-accent", "accent-primary", 4.5],
  ["on-accent", "accent-hover", 4.5],
  ["on-accent", "accent-danger", 4.5],
  // Accents double as label/icon text on surfaces — WCAG large/bold tier.
  ...["bg-primary", "bg-secondary", "bg-tertiary"].map((s) => ["accent-primary", s, 3.0] as Pair),
  ["accent-secondary", "bg-secondary", 3.0],
  ["accent-danger", "bg-secondary", 3.0],
  ["accent-danger", "bg-tertiary", 3.0],
  ["accent-warning", "bg-secondary", 3.0],
];

const BADGES = ["ms", "offline", "ygg"] as const;

interface Violation {
  theme: string;
  pair: string;
  fg: string;
  bg: string;
  ratio: number;
  min: number;
}

const violations: Violation[] = [];

function record(theme: string, pair: string, fgRaw: string, bgRaw: string, ratio: number, min: number) {
  if (ratio < min) violations.push({ theme, pair, fg: fgRaw, bg: bgRaw, ratio, min });
}

for (const [theme, vars] of themeTokens) {
  for (const [fgToken, bgToken, min] of BODY_PAIRS) {
    const fgRaw = vars[`--${fgToken}`];
    let fg = resolveColor(fgRaw, vars);
    let bg = resolveColor(vars[`--${bgToken}`], vars);
    if (!fg || !bg) {
      violations.push({
        theme,
        pair: `${fgToken} on ${bgToken}`,
        fg: fgRaw ?? "<missing>",
        bg: vars[`--${bgToken}`] ?? "<missing>",
        ratio: -1,
        min,
      });
      continue;
    }
    if (bg.a < 1) {
      // Translucent surfaces sit on the app shell.
      const shell = resolveColor(vars["--bg-primary"], vars) ?? { r: 0, g: 0, b: 0, a: 1 };
      bg = compositeOver(bg, shell);
    }
    if (fg.a < 1) {
      const shell = resolveColor(vars["--bg-primary"], vars) ?? { r: 0, g: 0, b: 0, a: 1 };
      fg = compositeOver(fg, shell);
    }
    record(theme, `${fgToken} on ${bgToken}`, fgRaw!, vars[`--${bgToken}`]!, contrastRatio(fg, bg), min);
  }

  for (const badge of BADGES) {
    const fg = resolveColor(vars[`--badge-${badge}-fg`], vars);
    const badgeBg = resolveColor(vars[`--badge-${badge}-bg`], vars);
    const panel = resolveColor(vars["--bg-secondary"], vars);
    if (!fg || !badgeBg || !panel) {
      violations.push({
        theme,
        pair: `badge-${badge}`,
        fg: vars[`--badge-${badge}-fg`] ?? "<missing>",
        bg: vars[`--badge-${badge}-bg`] ?? "<missing>",
        ratio: -1,
        min: 4.5,
      });
      continue;
    }
    record(
      theme,
      `badge-${badge} (composited on bg-secondary)`,
      vars[`--badge-${badge}-fg`]!,
      vars[`--badge-${badge}-bg`]!,
      contrastRatio(fg, compositeOver(badgeBg, panel)),
      4.5,
    );
  }
}

describe("theme contrast (WCAG 2.1)", () => {
  it("covers every theme in the catalog", () => {
    expect(THEME_IDS.length).toBeGreaterThanOrEqual(18);
    for (const id of THEME_IDS) {
      expect(blocksByTheme.has(id), `theme "${id}" has no [data-theme] block in themes.css`).toBe(true);
    }
  });

  it("keeps every token pair readable in every theme", () => {
    const report = violations
      .map(
        (v) =>
          `  ${v.theme}: ${v.pair} — ${v.fg} on ${v.bg} = ${v.ratio === -1 ? "unresolvable" : `${v.ratio.toFixed(2)}:1`} (need ${v.min}:1)`,
      )
      .join("\n");
    expect(
      violations,
      `${violations.length} unreadable combination(s) across ${THEME_IDS.length} themes:\n${report}`,
    ).toEqual([]);
  });
});
