#!/usr/bin/env node
/**
 * Bridge parity check (Reality Launcher-inspired).
 *
 * Verifies that:
 *  1. every command invoked from the TypeScript frontend (api.ts `cmd("x")`
 *     or direct `invoke("x")`) is registered in the Rust
 *     `generate_handler![...]` list — otherwise the UI gets "unknown command"
 *     at runtime;
 *  2. reports Rust-registered commands never called from TS as INFO (dead
 *     weight is legal: CLI/test/other hosts may use them), but an unexpected
 *     large drift (>25%) hints the extraction regexes broke.
 *
 * Usage: node scripts/check-bridge-parity.mjs
 * Exit code 1 on missing registration (hard error), 0 otherwise.
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const srcDir = join(root, "src");
const rustDir = join(root, "src-tauri", "src");

// ── Collect TS commands ────────────────────────────────────────────────
const tsFiles = [];
function walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) walk(p);
    else if (/\.(ts|svelte|js)$/.test(name) && !name.endsWith(".d.ts")) tsFiles.push(p);
  }
}
walk(srcDir);

const tsCommands = new Set();
const invokeRe = /(?:invoke|cmd)(?:<[^>]*>)?\(\s*["'`]([a-z_0-9]+)["'`]/g;
for (const file of tsFiles) {
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(invokeRe)) tsCommands.add(m[1]);
}

// ── Collect Rust registered commands ──────────────────────────────────
const rustFiles = [];
(function walkRust(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) walkRust(p);
    else if (name.endsWith(".rs")) rustFiles.push(p);
  }
})(rustDir);

const registered = new Set();
let handlerListFound = false;
for (const file of rustFiles) {
  const text = readFileSync(file, "utf8");
  const start = text.indexOf("generate_handler![");
  if (start === -1) continue;
  handlerListFound = true;
  const end = text.indexOf("])", start);
  const list = text.slice(start + "generate_handler!".length, end);
  for (const m of list.matchAll(/([A-Za-z_0-9]+(?:::[A-Za-z_0-9]+)*)/g)) {
    // `module::command` → command; bare `command` → command. Skip keywords.
    const path = m[1];
    if (/^(if|else|match|fn|pub|async)$/.test(path)) continue;
    registered.add(path.split("::").pop());
  }
}

if (!handlerListFound) {
  console.error("check-bridge-parity: generate_handler! not found in src-tauri — extraction broke?");
  process.exit(1);
}

// ── Compare ───────────────────────────────────────────────────────────
const missing = [...tsCommands].filter((c) => !registered.has(c)).sort();
const unused = [...registered].filter((c) => !tsCommands.has(c)).sort();

console.log(`check-bridge-parity: ${tsCommands.size} TS commands, ${registered.size} registered Rust commands`);

if (missing.length) {
  console.error(`\nFAIL: ${missing.length} command(s) invoked from TS but NOT registered in generate_handler!:`);
  for (const c of missing) console.error(`  - ${c}`);
  console.error("\nAdd them to the generate_handler! list in src-tauri/src/lib.rs.");
  process.exit(1);
}

// Info-only: Rust-side commands without TS callers (legal — tests/CLI).
if (unused.length) {
  console.log(`info: ${unused.length} registered commands have no TS caller (tests/CLI-only — fine)`);
}
if (unused.length > registered.size * 0.25) {
  console.warn(
    `warn: ${unused.length}/${registered.size} registered commands have no TS caller — ` +
      "if extraction regexes changed, re-check this script",
  );
}

console.log("check-bridge-parity: OK");
