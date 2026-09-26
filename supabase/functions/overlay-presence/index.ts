// TuffBox overlay presence heartbeat: upsert my presence, return friends'
// live presence (stale > 2 min treated as offline and omitted).
// Auth: writeSecret against cosmetics_profiles (see overlay-friends).

import { createClient } from "https://esm.sh/@supabase/supabase-js@2.49.1";

const corsHeaders: Record<string, string> = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers":
    "authorization, x-client-info, apikey, content-type",
};

const STALE_MS = 2 * 60 * 1000;

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...corsHeaders, "Content-Type": "application/json" },
  });
}

function asString(v: unknown): string {
  return typeof v === "string" ? v : "";
}

async function sha256Hex(text: string): Promise<string> {
  const buf = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(text),
  );
  return Array.from(new Uint8Array(buf))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

type ServiceClient = ReturnType<typeof createClient>;

async function authenticate(
  sb: ServiceClient,
  playerKey: string,
  username: string,
  writeSecret: string,
): Promise<Response | null> {
  if (!playerKey || !username || !writeSecret || writeSecret.length < 16) {
    return jsonResponse(400, {
      error: "playerKey, username, writeSecret (>=16) required",
    });
  }
  const secretHash = await sha256Hex(writeSecret);
  const { data: existing } = await sb
    .from("cosmetics_profiles")
    .select("player_key,write_secret_hash")
    .eq("player_key", playerKey)
    .maybeSingle();

  if (existing) {
    if (existing.write_secret_hash !== secretHash) {
      return jsonResponse(403, { error: "writeSecret mismatch" });
    }
    return null;
  }

  const { error } = await sb.from("cosmetics_profiles").insert({
    player_key: playerKey,
    username,
    write_secret_hash: secretHash,
    share_public: false,
  });
  if (error && error.code !== "23505") {
    return jsonResponse(500, { error: error.message });
  }
  return null;
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
    return jsonResponse(400, { error: "invalid json" });
  }

  const playerKey = asString(body.playerKey).trim();
  const username = asString(body.username).trim();
  const writeSecret = asString(body.writeSecret).trim();
  const packName = asString(body.packName).trim().slice(0, 96);
  const server = asString(body.server).trim().slice(0, 128);
  const offline = body.offline === true;

  const sb = createClient(supabaseUrl, serviceKey);
  const authError = await authenticate(sb, playerKey, username, writeSecret);
  if (authError) return authError;

  if (offline) {
    await sb.from("player_presence").delete().eq("player_key", playerKey);
  } else {
    const { error } = await sb.from("player_presence").upsert({
      player_key: playerKey,
      username,
      pack_name: packName,
      server,
      updated_at: new Date().toISOString(),
    });
    if (error) return jsonResponse(500, { error: error.message });
  }

  // Friends' presence: accepted friendships → live presence rows.
  const { data: friendships } = await sb
    .from("player_friendships")
    .select("requester_key,addressee_key")
    .eq("status", "accepted")
    .or(`requester_key.eq.${playerKey},addressee_key.eq.${playerKey}`)
    .limit(200);

  const friendKeys = (friendships ?? []).map((f) =>
    f.requester_key === playerKey ? f.addressee_key : f.requester_key
  );

  let friends: unknown[] = [];
  if (friendKeys.length > 0) {
    const staleAfter = new Date(Date.now() - STALE_MS).toISOString();
    const { data: rows } = await sb
      .from("player_presence")
      .select("player_key,username,pack_name,server,updated_at")
      .in("player_key", friendKeys)
      .gt("updated_at", staleAfter)
      .limit(200);
    friends = (rows ?? []).map((r) => ({
      key: r.player_key,
      name: r.username,
      pack: r.pack_name,
      server: r.server,
      seenAt: r.updated_at,
    }));
  }

  return jsonResponse(200, { ok: true, friends });
});
