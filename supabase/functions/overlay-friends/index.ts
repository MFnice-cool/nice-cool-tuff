// TuffBox overlay friends: list / add (by username) / accept / remove.
// Auth: writeSecret — sha256 must match cosmetics_profiles.write_secret_hash
// (same ownership credential as cosmetics-upsert). First social write binds
// a stub profile when none exists yet.

import { createClient } from "https://esm.sh/@supabase/supabase-js@2.49.1";

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

/** Returns null on success; otherwise an error Response. */
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

  // First social write: bind identity (not publicly shared by default).
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
  const action = asString(body.action).trim().toLowerCase();

  const sb = createClient(supabaseUrl, serviceKey);
  const authError = await authenticate(sb, playerKey, username, writeSecret);
  if (authError) return authError;

  switch (action) {
    case "list": {
      const { data, error } = await sb
        .from("player_friendships")
        .select("id,requester_key,requester_name,addressee_key,addressee_name,status,created_at")
        .or(`requester_key.eq.${playerKey},addressee_key.eq.${playerKey}`)
        .order("created_at", { ascending: false })
        .limit(200);
      if (error) return jsonResponse(500, { error: error.message });

      const friends: unknown[] = [];
      const incoming: unknown[] = [];
      const outgoing: unknown[] = [];
      for (const row of data ?? []) {
        const mine = row.requester_key === playerKey;
        const peerKey = mine ? row.addressee_key : row.requester_key;
        const peerName = mine ? row.addressee_name : row.requester_name;
        const entry = { id: row.id, key: peerKey, name: peerName, since: row.created_at };
        if (row.status === "accepted") friends.push(entry);
        else if (mine) outgoing.push(entry);
        else incoming.push(entry);
      }
      return jsonResponse(200, { ok: true, friends, incoming, outgoing });
    }

    case "add": {
      const friendUsername = asString(body.friendUsername).trim();
      if (!friendUsername) return jsonResponse(400, { error: "friendUsername required" });

      const { data: target } = await sb
        .from("cosmetics_profiles")
        .select("player_key,username")
        .ilike("username", friendUsername)
        .limit(1)
        .maybeSingle();
      if (!target) return jsonResponse(404, { error: "player not found" });
      if (target.player_key === playerKey) {
        return jsonResponse(400, { error: "cannot add yourself" });
      }

      // Reverse pending (them → me) means mutual intent: accept it outright.
      const { data: reverse } = await sb
        .from("player_friendships")
        .select("id")
        .eq("requester_key", target.player_key)
        .eq("addressee_key", playerKey)
        .eq("status", "pending")
        .maybeSingle();
      if (reverse) {
        await sb
          .from("player_friendships")
          .update({ status: "accepted", updated_at: new Date().toISOString() })
          .eq("id", reverse.id);
        return jsonResponse(200, { ok: true, accepted: true, key: target.player_key, name: target.username });
      }

      const { data: existing } = await sb
        .from("player_friendships")
        .select("id,status")
        .eq("requester_key", playerKey)
        .eq("addressee_key", target.player_key)
        .maybeSingle();
      if (existing) {
        return jsonResponse(200, { ok: true, already: true, status: existing.status });
      }

      const { error } = await sb.from("player_friendships").insert({
        requester_key: playerKey,
        requester_name: username,
        addressee_key: target.player_key,
        addressee_name: target.username,
        status: "pending",
      });
      if (error) return jsonResponse(500, { error: error.message });
      return jsonResponse(200, { ok: true, sent: true, key: target.player_key, name: target.username });
    }

    case "accept": {
      const id = Number(body.friendshipId);
      if (!Number.isFinite(id)) return jsonResponse(400, { error: "friendshipId required" });
      const { error } = await sb
        .from("player_friendships")
        .update({ status: "accepted", updated_at: new Date().toISOString() })
        .eq("id", id)
        .eq("addressee_key", playerKey)
        .eq("status", "pending");
      if (error) return jsonResponse(500, { error: error.message });
      return jsonResponse(200, { ok: true });
    }

    case "remove": {
      const id = Number(body.friendshipId);
      if (!Number.isFinite(id)) return jsonResponse(400, { error: "friendshipId required" });
      const { error } = await sb
        .from("player_friendships")
        .delete()
        .eq("id", id)
        .or(`requester_key.eq.${playerKey},addressee_key.eq.${playerKey}`);
      if (error) return jsonResponse(500, { error: error.message });
      return jsonResponse(200, { ok: true });
    }

    default:
      return jsonResponse(400, { error: "unknown action" });
  }
});
