// TuffBox overlay chat: send a direct message to an accepted friend.
// Auth: writeSecret against cosmetics_profiles (see overlay-friends).

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

const MAX_CHAT_CHARS = 500;
const MAX_CHAT_BYTES = 2000;

/** NFC + strip bidi/controls/tags + length caps. Keeps emoji (incl. ZWJ/VS16). */
function sanitizeChatBody(input: string): string {
  // NFC so visually-identical strings compare equal and can't smuggle via NFD.
  let s = input.normalize("NFC");
  let out = "";
  let bytes = 0;
  let chars = 0;
  for (const ch of s) {
    if (chars >= MAX_CHAT_CHARS || bytes >= MAX_CHAT_BYTES) break;
    const cp = ch.codePointAt(0)!;
    // Flatten newlines/tabs.
    const c = cp === 0x0a || cp === 0x0d || cp === 0x09 ? " " : ch;
    const u = c.codePointAt(0)!;
    // C0 / DEL / C1
    if (u < 0x20 || (u >= 0x7f && u <= 0x9f)) continue;
    // Bidi / isolate / marks that reverse UI (Trojan Source).
    if (
      (u >= 0x202a && u <= 0x202e) ||
      (u >= 0x2066 && u <= 0x2069) ||
      u === 0x200e ||
      u === 0x200f ||
      u === 0x061c
    ) {
      continue;
    }
    // Zero-width (keep ZWJ U+200D and VS16 U+FE0F for emoji sequences).
    if (
      u === 0x200b ||
      u === 0x200c ||
      u === 0x2060 ||
      u === 0xfeff ||
      u === 0x00ad ||
      u === 0x180e ||
      (u >= 0x206a && u <= 0x206f)
    ) {
      continue;
    }
    // Tags block + private use.
    if (
      (u >= 0xe0000 && u <= 0xe007f) ||
      (u >= 0xe000 && u <= 0xf8ff) ||
      (u >= 0xf0000 && u <= 0xffffd) ||
      (u >= 0x100000 && u <= 0x10fffd)
    ) {
      continue;
    }
    const enc = new TextEncoder().encode(c);
    if (bytes + enc.length > MAX_CHAT_BYTES) break;
    out += c;
    bytes += enc.length;
    chars += 1;
  }
  // Collapse runs of spaces.
  return out.replace(/ {2,}/g, " ").trim();
}

function isSafeKey(s: string): boolean {
  return s.length >= 8 && s.length <= 64 && /^[0-9a-fA-F-]+$/.test(s);
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

function conversationId(a: string, b: string): string {
  return a < b ? `${a}:${b}` : `${b}:${a}`;
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
  const toKey = asString(body.toKey).trim();
  const rawBody = asString(body.body);

  if (!isSafeKey(toKey)) {
    return jsonResponse(400, { error: "toKey invalid" });
  }
  if (!isSafeKey(playerKey)) {
    return jsonResponse(400, { error: "playerKey invalid" });
  }
  if (toKey === playerKey) {
    return jsonResponse(400, { error: "cannot message yourself" });
  }

  const text = sanitizeChatBody(rawBody);
  if (!text) {
    return jsonResponse(400, { error: "body empty after sanitise" });
  }
  // JS string length ≈ UTF-16 code units; also enforce scalar-ish bound.
  if ([...text].length > MAX_CHAT_CHARS || new TextEncoder().encode(text).length > MAX_CHAT_BYTES) {
    return jsonResponse(400, { error: "body too long" });
  }

  // Rate limit: max 20 messages / 60s from this player (cheap anti-spam).
  const sb = createClient(supabaseUrl, serviceKey);
  const authError = await authenticate(sb, playerKey, username, writeSecret);
  if (authError) return authError;

  const since = new Date(Date.now() - 60_000).toISOString();
  const { count: recentCount } = await sb
    .from("chat_messages")
    .select("id", { count: "exact", head: true })
    .eq("from_key", playerKey)
    .gte("created_at", since);
  if ((recentCount ?? 0) >= 20) {
    return jsonResponse(429, { error: "rate limited — slow down" });
  }

  // Anti-spam: only accepted friends can be messaged.
  // Keys are hex/uuid-safe so embedding in the filter is OK after isSafeKey.
  const { data: friendship } = await sb
    .from("player_friendships")
    .select("id")
    .eq("status", "accepted")
    .or(
      `and(requester_key.eq.${playerKey},addressee_key.eq.${toKey}),` +
        `and(requester_key.eq.${toKey},addressee_key.eq.${playerKey})`,
    )
    .maybeSingle();
  if (!friendship) {
    return jsonResponse(403, { error: "not friends" });
  }

  // Store NFC-sanitised UTF-8 body (emoji preserved).
  const safeName = sanitizeChatBody(username).slice(0, 32) || "player";
  const { data, error } = await sb
    .from("chat_messages")
    .insert({
      conversation_id: conversationId(playerKey, toKey),
      from_key: playerKey,
      from_name: safeName,
      to_key: toKey,
      body: text,
    })
    .select("id,created_at")
    .single();
  if (error) return jsonResponse(500, { error: error.message });

  return jsonResponse(200, {
    ok: true,
    id: data.id,
    createdAt: data.created_at,
    body: text, // echo sanitised form so clients stay in sync
  });
});
