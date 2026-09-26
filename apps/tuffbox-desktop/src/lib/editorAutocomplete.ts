/**
 * Language-aware autocomplete for the built-in config editor (Tune tab).
 *
 * The completion DECISIONS live in pure functions over `(doc, pos)` so they
 * are unit-testable without an editor instance; the CodeMirror adapters at
 * the bottom are thin wrappers. Sources:
 *
 * - JSON: keys seen elsewhere in the document (config files repeat keys
 *   across siblings/blocks), existing string values after `:`, and the
 *   `true` / `false` / `null` literals.
 * - TOML / properties / options.txt: keys from the document while typing a
 *   key, and the VALUES the same key has elsewhere in the file after `=`
 *   (config presets often repeat a key with different values).
 * - KubeJS scripts: recipe-event methods after `event.` and the common
 *   global event buses at the top level.
 */
import { autocompletion, snippet, type Completion, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";

export type CompletionKind = "json" | "flat" | "kubejs";

// ── JSON ────────────────────────────────────────────────────────────────

/** Every object key literal used in the document, deduplicated. */
export function collectJsonKeys(doc: string): string[] {
  const keys = new Set<string>();
  const re = /"((?:[^"\\]|\\.)+)"\s*:/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(doc))) keys.add(m[1]);
  return [...keys].sort();
}

/** Every string value used in the document, deduplicated. */
function collectJsonValues(doc: string): string[] {
  const vals = new Set<string>();
  const re = /:\s*"((?:[^"\\]|\\.)*)"/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(doc))) vals.add(m[1]);
  return [...vals].sort();
}

export interface PureCompletions {
  /** Offset the completion text starts at. */
  from: number;
  /** Plain labels (extra detail is added by the adapter). */
  labels: string[];
}

/** Quote parity of a single JSON line fragment (strings never span lines). */
function insideJsonString(lineBefore: string): boolean {
  let quotes = 0;
  for (let i = 0; i < lineBefore.length; i++) {
    if (lineBefore[i] === "\\") i++;
    else if (lineBefore[i] === '"') quotes++;
  }
  return quotes % 2 === 1;
}

export function jsonCompletionsAt(doc: string, pos: number): PureCompletions | null {
  const lineStart = doc.lastIndexOf("\n", pos - 1) + 1;
  const lineBefore = doc.slice(lineStart, pos);

  // Typing a bare value right after `:` → literal keywords.
  const afterColon = /:[ \t]*$/.test(lineBefore);
  if (!insideJsonString(lineBefore)) {
    if (afterColon) return { from: pos, labels: ["true", "false", "null"] };
    return null;
  }

  const quoteIdx = lineBefore.lastIndexOf('"');
  const stringFrom = lineStart + quoteIdx + 1;
  const beforeString = lineBefore.slice(0, quoteIdx);
  // Key side of the line = nothing before the string's quote except
  // whitespace, `{`, `,` or a comma-terminated previous token — i.e. the
  // line has not reached its `:` yet. Value strings can contain colons, but
  // those are INSIDE the quotes, not in beforeString.
  const isKeyPosition = !/:/.test(beforeString);

  if (isKeyPosition) {
    const keys = collectJsonKeys(doc);
    if (keys.length === 0) return null;
    return { from: stringFrom, labels: keys };
  }
  // Inside a value string → suggest values used elsewhere + booleans.
  const vals = [...collectJsonValues(doc), "true", "false"];
  if (vals.length === 0) return null;
  return { from: stringFrom, labels: vals };
}

// ── TOML / properties / options.txt ────────────────────────────────────

/** `key = value` / `key: value` pairs from a flat config document. */
export function collectFlatEntries(doc: string): Map<string, string[]> {
  const map = new Map<string, string[]>();
  const re = /^[ \t]*([^\s=#:]+)[ \t]*[:=][ \t]*(.+?)\s*$/gm;
  let m: RegExpExecArray | null;
  while ((m = re.exec(doc))) {
    const key = m[1];
    const value = m[2].trim().replace(/^["']|["']$/g, "");
    if (!value) continue; // half-typed `key = ` — not a preset
    const list = map.get(key) ?? [];
    if (!list.includes(value)) list.push(value);
    map.set(key, list);
  }
  return map;
}

export function flatCompletionsAt(doc: string, pos: number): PureCompletions | null {
  const lineStart = doc.lastIndexOf("\n", pos - 1) + 1;
  const lineBefore = doc.slice(lineStart, pos);

  // Comment lines never complete.
  if (/^[ \t]*[#!]/.test(lineBefore)) return null;

  const eq = lineBefore.search(/[=:]/);
  if (eq === -1) {
    // Still typing the key → keys from the document.
    const keys = [...collectFlatEntries(doc).keys()].sort();
    if (keys.length === 0) return null;
    const wordStart = lineBefore.length - (lineBefore.match(/[^\s]*$/)?.[0]?.length ?? 0);
    return { from: lineStart + wordStart, labels: keys };
  }

  // After `=` → values the SAME key has elsewhere (presets), + booleans.
  const key = lineBefore.slice(0, eq).trim();
  const values = collectFlatEntries(doc).get(key) ?? [];
  const afterEq = lineBefore.slice(eq + 1);
  const typed = afterEq.replace(/^[ \t"']*/, "");
  const valFrom = pos - typed.length;
  const labels = [...new Set([...values, "true", "false"])];
  return { from: valFrom, labels };
}

// ── KubeJS ─────────────────────────────────────────────────────────────

export interface ScriptCompletion {
  label: string;
  detail: string;
  /** Snippet applied on commit (CM `${}` placeholder syntax), if any. */
  apply?: string;
}

const RECIPE_EVENT_METHODS: ScriptCompletion[] = [
  { label: "remove", detail: "remove recipes by id/filter" },
  { label: "shaped", detail: "add a shaped (crafting table) recipe", apply: "shaped('${1:modid:result}', [${2:'AAA', 'ABA', 'AAA'}], { ${3:A: 'modid:ingredient'} })" },
  { label: "shapeless", detail: "add a shapeless recipe", apply: "shapeless('${1:modid:result}', [${2:modid:ingredient}])" },
  { label: "smelting", detail: "add a furnace recipe", apply: "smelting('${1:modid:input}', '${2:modid:result}')" },
  { label: "blasting", detail: "add a blast furnace recipe" },
  { label: "smoking", detail: "add a smoker recipe" },
  { label: "stonecutting", detail: "add a stonecutter recipe" },
  { label: "smithing", detail: "add a smithing table recipe" },
  { label: "replaceInput", detail: "swap an ingredient everywhere" },
  { label: "replaceOutput", detail: "swap a result everywhere" },
];

const KUBEJS_GLOBALS: ScriptCompletion[] = [
  { label: "ServerEvents", detail: "server event bus", apply: "ServerEvents.recipes(event => {\n\t$0\n})" },
  { label: "ItemEvents", detail: "item event bus", apply: "ItemEvents.modification(event => {\n\t$0\n})" },
  { label: "BlockEvents", detail: "block event bus" },
  { label: "EntityEvents", detail: "entity event bus" },
  { label: "PlayerEvents", detail: "player event bus" },
  { label: "WorldEvents", detail: "world event bus" },
  { label: "DataEvents", detail: "data pack event bus" },
  { label: "NetworkEvents", detail: "custom network channels" },
  { label: "JsonIO", detail: "read/write JSON files" },
  { label: "Text", detail: "build text components" },
  { label: "Ingredient", detail: "match item ingredients" },
  { label: "console", detail: "log to the KubeJS console" },
];

export function kubejsCompletionsAt(doc: string, pos: number): (PureCompletions & { extras?: ScriptCompletion[] }) | null {
  const before = doc.slice(Math.max(0, pos - 64), pos);
  const lineStart = doc.lastIndexOf("\n", pos - 1) + 1;
  const lineBefore = doc.slice(lineStart, pos);

  // `event.…` → recipe-event methods.
  if (/[.]$/.test(lineBefore) && /event\s*[.]$/.test(before)) {
    return {
      from: pos,
      labels: RECIPE_EVENT_METHODS.map((m) => m.label),
      extras: RECIPE_EVENT_METHODS,
    };
  }

  // Top-level statement start → global buses.
  if (/^[ \t]*$/u.test(lineBefore)) {
    return {
      from: pos,
      labels: KUBEJS_GLOBALS.map((g) => g.label),
      extras: KUBEJS_GLOBALS,
    };
  }
  return null;
}

// ── CodeMirror adapters ────────────────────────────────────────────────

function toOptions(labels: string[], type: string, extras?: Map<string, ScriptCompletion>): Completion[] {
  return labels.map((label) => {
    const extra = extras?.get(label);
    const option: Completion = { label, type };
    if (extra?.detail) option.detail = extra.detail;
    if (extra?.apply) option.apply = snippet(extra.apply);
    return option;
  });
}

function jsonCompletion(cx: CompletionContext): CompletionResult | null {
  const res = jsonCompletionsAt(cx.state.doc.toString(), cx.pos);
  if (!res || res.labels.length === 0) return null;
  const type = /^[a-z]+$/.test(res.labels[0] ?? "") ? "keyword" : "property";
  return { from: res.from, options: toOptions(res.labels, type) };
}

function flatCompletion(cx: CompletionContext): CompletionResult | null {
  const res = flatCompletionsAt(cx.state.doc.toString(), cx.pos);
  if (!res || res.labels.length === 0) return null;
  const looksLikeValue = res.labels.includes("true") && !/^[a-z-]+$/i.test(res.labels[0] ?? "");
  return { from: res.from, options: toOptions(res.labels, looksLikeValue ? "text" : "property") };
}

function kubejsCompletion(cx: CompletionContext): CompletionResult | null {
  const res = kubejsCompletionsAt(cx.state.doc.toString(), cx.pos);
  if (!res || res.labels.length === 0) return null;
  const extras = new Map((res.extras ?? []).map((e) => [e.label, e]));
  return { from: res.from, options: toOptions(res.labels, "keyword", extras) };
}

/** Autocomplete extension for a config editor language. */
export function completionsForExtension(kind: CompletionKind | null) {
  const source =
    kind === "json" ? jsonCompletion : kind === "flat" ? flatCompletion : kind === "kubejs" ? kubejsCompletion : null;
  if (!source) return [];
  return [autocompletion({ override: [source], activateOnTyping: true })];
}
