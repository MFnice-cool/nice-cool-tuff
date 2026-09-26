#!/usr/bin/env node
/**
 * Seed public.optimize_mod_matrix from Fabulously Optimized .mrpack files.
 *
 * Usage:
 *   node scripts/seed-optimize-mods-from-fo.mjs              # print JSON summary
 *   node scripts/seed-optimize-mods-from-fo.mjs --sql > seed.sql
 *
 * Env (optional upsert to Supabase):
 *   SUPABASE_URL + SUPABASE_SERVICE_ROLE_KEY
 */
import { createWriteStream, mkdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pipeline } from "node:stream/promises";
import { execFileSync } from "node:child_process";

const FO_SLUG = "fabulously-optimized";
const FO_ID = "1KVo5zza";
const UA = "TuffBox-OptimizeSeed/1.0 (github.com/tuffbox)";
const MC_VERSIONS = [
  "1.16.5",
  "1.17.1",
  "1.18.2",
  "1.19.4",
  "1.20.1",
  "1.20.4",
  "1.21",
  "1.21.1",
  "1.21.4",
];

/** Project IDs / slugs that are loaders or language packs — not Optimize targets. */
const SKIP_IDS = new Set([
  "P7dR8mSH", // fabric-api
  "Ha28R6CL", // fabric-language-kotlin
  "qvIfYCYJ", // qsl
  "qvIfYCYJ".toLowerCase(),
]);
const SKIP_SLUGS = new Set([
  "fabric-api",
  "fabric-language-kotlin",
  "quilted-fabric-api",
  "qsl",
  "fabric-loader",
  "quilt-loader",
]);

const args = new Set(process.argv.slice(2));
const wantSql = args.has("--sql");
const wantUpsert = args.has("--upsert");

async function mrGet(path) {
  const res = await fetch(`https://api.modrinth.com/v2${path}`, {
    headers: { "User-Agent": UA, Accept: "application/json" },
  });
  if (!res.ok) throw new Error(`Modrinth ${path} → ${res.status}`);
  return res.json();
}

function projectIdFromCdn(url) {
  const m = String(url).match(/cdn\.modrinth\.com\/data\/([^/]+)\//i);
  return m?.[1] ?? null;
}

async function resolveSlug(projectId, cache) {
  if (!projectId) return null;
  if (cache.has(projectId)) return cache.get(projectId);
  try {
    const proj = await mrGet(`/project/${projectId}`);
    const slug = (proj.slug || "").toLowerCase();
    cache.set(projectId, slug);
    return slug;
  } catch {
    cache.set(projectId, null);
    return null;
  }
}

async function downloadTo(url, dest) {
  const res = await fetch(url, { headers: { "User-Agent": UA } });
  if (!res.ok) throw new Error(`download ${url} → ${res.status}`);
  await pipeline(res.body, createWriteStream(dest));
}

function extractIndex(mrpackPath, workDir) {
  // Prefer tar/PowerShell Expand-Archive isn't for zip of mrpack — use tar on Win10+
  try {
    execFileSync("tar", ["-xf", mrpackPath, "-C", workDir, "modrinth.index.json"], {
      stdio: "ignore",
    });
  } catch {
    // fallback: powershell Expand-Archive needs .zip extension
    const zipPath = join(workDir, "pack.zip");
    execFileSync("cmd", ["/c", `copy /y "${mrpackPath}" "${zipPath}"`], { stdio: "ignore" });
    execFileSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        `Expand-Archive -LiteralPath '${zipPath}' -DestinationPath '${workDir}' -Force`,
      ],
      { stdio: "ignore" },
    );
  }
  return JSON.parse(readFileSync(join(workDir, "modrinth.index.json"), "utf8"));
}

async function modsForMc(mc, slugCache) {
  const versions = await mrGet(
    `/project/${FO_SLUG}/version?game_versions=${encodeURIComponent(
      JSON.stringify([mc]),
    )}&loaders=${encodeURIComponent(JSON.stringify(["fabric"]))}`,
  );
  if (!Array.isArray(versions) || versions.length === 0) {
    console.error(`# no FO version for ${mc}`);
    return { mc, versionId: null, mods: [] };
  }
  // Prefer release channel
  const version =
    versions.find((v) => v.version_type === "release") ?? versions[0];
  const file =
    (version.files || []).find((f) => f.primary) ??
    (version.files || []).find((f) => String(f.filename || "").endsWith(".mrpack")) ??
    version.files?.[0];
  if (!file?.url) {
    console.error(`# no mrpack file for ${mc}`);
    return { mc, versionId: version.id, mods: [] };
  }

  const work = join(tmpdir(), `tuffbox-fo-${mc}-${Date.now()}`);
  mkdirSync(work, { recursive: true });
  const mrpack = join(work, "pack.mrpack");
  try {
    await downloadTo(file.url, mrpack);
    const index = extractIndex(mrpack, work);
    const mods = [];
    const seen = new Set();
    for (const entry of index.files || []) {
      const path = String(entry.path || "").replace(/\\/g, "/");
      if (!path.toLowerCase().startsWith("mods/")) continue;
      const url = entry.downloads?.[0] || "";
      const projectId = projectIdFromCdn(url);
      if (projectId && SKIP_IDS.has(projectId)) continue;
      const slug = await resolveSlug(projectId, slugCache);
      if (!slug || SKIP_SLUGS.has(slug) || seen.has(slug)) continue;
      seen.add(slug);
      mods.push({
        modrinth_slug: slug,
        sort_order: mods.length,
        name: slug,
        source: "fabulously-optimized",
        source_version_id: version.id,
      });
    }
    return { mc, versionId: version.id, mods };
  } finally {
    try {
      rmSync(work, { recursive: true, force: true });
    } catch {
      /* ignore */
    }
  }
}

async function upsert(loader, mc, rows) {
  const url = process.env.SUPABASE_URL?.replace(/\/$/, "");
  const key = process.env.SUPABASE_SERVICE_ROLE_KEY;
  if (!url || !key) throw new Error("SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY required for --upsert");
  const res = await fetch(`${url}/rest/v1/rpc/replace_optimize_mods_for`, {
    method: "POST",
    headers: {
      apikey: key,
      Authorization: `Bearer ${key}`,
      "Content-Type": "application/json",
      Prefer: "return=representation",
    },
    body: JSON.stringify({
      p_loader: loader,
      p_mc_version: mc,
      p_rows: rows,
    }),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`upsert ${mc}: ${res.status} ${text}`);
  return text;
}

async function main() {
  const slugCache = new Map([[FO_ID, FO_SLUG]]);
  const results = [];
  for (const mc of MC_VERSIONS) {
    process.stderr.write(`# FO ${mc}…\n`);
    const one = await modsForMc(mc, slugCache);
    results.push(one);
    process.stderr.write(`#   → ${one.mods.length} mods\n`);
  }

  // Also seed a "default" profile from the newest MC with mods.
  const newest = [...results].reverse().find((r) => r.mods.length > 0);
  if (newest) {
    results.push({
      mc: "default",
      versionId: newest.versionId,
      mods: newest.mods.map((m, i) => ({ ...m, sort_order: i })),
    });
  }

  if (wantSql) {
    for (const r of results) {
      if (!r.mods.length) continue;
      const json = JSON.stringify(r.mods).replace(/'/g, "''");
      console.log(
        `select public.replace_optimize_mods_for('fabric', '${r.mc}', '${json}'::jsonb);`,
      );
    }
    return;
  }

  if (wantUpsert) {
    for (const r of results) {
      if (!r.mods.length) continue;
      const n = await upsert("fabric", r.mc, r.mods);
      process.stderr.write(`# upserted fabric/${r.mc}: ${n}\n`);
    }
  }

  console.log(JSON.stringify(results, null, 2));
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
