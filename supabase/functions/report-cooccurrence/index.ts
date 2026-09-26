// TuffSwarm: ingest mod co-occurrence sets for Create Mode AI.
// Client sends modIds (slugs/ids); server expands unordered pairs and bumps counts.
// Service role only inside this function — never ship it to clients.

import { createClient } from "https://esm.sh/@supabase/supabase-js@2.49.1";

const MAX_REPORTS_PER_HOUR = 30;
const MAX_MODS_PER_REPORT = 48;
const MAX_PAIRS_PER_REPORT = 800;
const MAX_ID_LEN = 64;

const corsHeaders: Record<string, string> = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers":
    "authorization, x-client-info, apikey, content-type",
};

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...corsHeaders, "Content-Type": "application/json" },
  });
}

function asString(v: unknown): string {
  return typeof v === "string" ? v : "";
}

function normalizeId(raw: string): string {
  return raw.trim().toLowerCase().slice(0, MAX_ID_LEN);
}

function expandPairs(
  modIds: string[],
  mcVersion: string,
  loader: string,
  source: string,
): Array<Record<string, string>> {
  const ids = [...new Set(modIds.map(normalizeId).filter(Boolean))].sort();
  const capped = ids.slice(0, MAX_MODS_PER_REPORT);
  const pairs: Array<Record<string, string>> = [];
  for (let i = 0; i < capped.length; i++) {
    for (let j = i + 1; j < capped.length; j++) {
      const a = capped[i];
      const b = capped[j];
      if (!a || !b || a === b) continue;
      const [mod_a, mod_b] = a < b ? [a, b] : [b, a];
      pairs.push({
        mod_a,
        mod_b,
        mc_version: mcVersion,
        loader,
        last_source: source,
      });
      if (pairs.length >= MAX_PAIRS_PER_REPORT) return pairs;
    }
  }
  return pairs;
}

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") {
    return new Response("ok", { headers: corsHeaders });
  }
  if (req.method !== "POST") {
    return jsonResponse(405, { error: "method not allowed" });
  }

  const supabaseUrl = Deno.env.get("SUPABASE_URL") ?? "";
  const serviceKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "";
  if (!supabaseUrl || !serviceKey) {
    return jsonResponse(500, { error: "server misconfigured" });
  }

  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return jsonResponse(400, { error: "invalid JSON" });
  }

  const mcVersion = asString(body.mcVersion ?? body.mc_version).slice(0, 32);
  const loader = asString(body.loader).trim().toLowerCase().slice(0, 32);
  const source = asString(body.source || "launcher").slice(0, 48) || "launcher";
  const clientKeyRaw = asString(body.clientKey ?? body.signerPublicKey).slice(0, 128);
  const clientKey = clientKeyRaw || "anon";

  const modIdsRaw = body.modIds ?? body.mod_ids;
  if (!Array.isArray(modIdsRaw) || modIdsRaw.length < 2) {
    return jsonResponse(400, { error: "modIds must be an array with at least 2 mods" });
  }
  const modIds = modIdsRaw
    .map((v) => (typeof v === "string" ? v : ""))
    .filter(Boolean);
  if (modIds.length < 2) {
    return jsonResponse(400, { error: "need at least 2 valid mod ids" });
  }

  const admin = createClient(supabaseUrl, serviceKey, {
    auth: { persistSession: false, autoRefreshToken: false },
  });

  const now = new Date();
  const { data: rateRow } = await admin
    .from("mod_cooccurrence_rate")
    .select("client_key, window_start, report_count")
    .eq("client_key", clientKey)
    .maybeSingle();

  let reportCount = 1;
  let windowStart = now.toISOString();
  if (rateRow?.window_start) {
    const start = new Date(rateRow.window_start);
    const ageMs = now.getTime() - start.getTime();
    if (ageMs < 60 * 60 * 1000) {
      reportCount = (rateRow.report_count ?? 0) + 1;
      windowStart = rateRow.window_start;
      if (reportCount > MAX_REPORTS_PER_HOUR) {
        return jsonResponse(429, { error: "rate limit: too many co-occurrence reports" });
      }
    }
  }
  await admin.from("mod_cooccurrence_rate").upsert({
    client_key: clientKey,
    window_start: windowStart,
    report_count: reportCount,
  });

  const pairs = expandPairs(modIds, mcVersion, loader, source);
  if (pairs.length === 0) {
    return jsonResponse(400, { error: "no valid pairs" });
  }

  const { data: bumped, error } = await admin.rpc("bump_mod_cooccurrence_pairs", {
    pairs,
  });
  if (error) {
    return jsonResponse(500, { error: error.message });
  }

  return jsonResponse(200, {
    ok: true,
    modCount: Math.min(
      new Set(modIds.map(normalizeId).filter(Boolean)).size,
      MAX_MODS_PER_REPORT,
    ),
    pairCount: bumped ?? pairs.length,
    mcVersion,
    loader,
  });
});
