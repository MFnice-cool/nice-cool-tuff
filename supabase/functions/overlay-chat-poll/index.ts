// TuffBox overlay chat: incremental poll of my direct messages.
// Auth: writeSecret against cosmetics_profiles (see overlay-friends).
// Also prunes >30d messages with low probability (cheap retention).

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
  const sinceId = Number(body.sinceId) || 0;

  const sb = createClient(supabaseUrl, serviceKey);
  const authError = await authenticate(sb, playerKey, username, writeSecret);
  if (authError) return authError;

  const { data, error } = await sb
    .from("chat_messages")
    .select("id,conversation_id,from_key,from_name,to_key,body,created_at")
    .gt("id", sinceId)
    .or(`from_key.eq.${playerKey},to_key.eq.${playerKey}`)
    .order("id", { ascending: true })
    .limit(200);
  if (error) return jsonResponse(500, { error: error.message });

  // Cheap retention: prune ~1% of polls.
  if (Math.random() < 0.01) {
    sb.rpc("chat_messages_prune").then(() => {}).catch(() => {});
  }

  const messages = (data ?? []).map((m) => ({
    id: m.id,
    conversation: m.conversation_id,
    fromKey: m.from_key,
    fromName: m.from_name,
    toKey: m.to_key,
    body: m.body,
    at: m.created_at,
  }));

  const maxId = messages.length > 0 ? messages[messages.length - 1].id : sinceId;
  return jsonResponse(200, { ok: true, messages, cursor: maxId });
});
