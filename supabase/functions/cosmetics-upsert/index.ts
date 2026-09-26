// Upsert TuffBox cosmetics profile + optional PNG uploads (base64).
// Ownership: writeSecret (plaintext) hashed and stored; must match on update.

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

function decodeBase64Png(b64: string): Uint8Array | null {
  const clean = b64.replace(/^data:image\/png;base64,/, "").trim();
  if (!clean || clean.length > 12_000_000) return null;
  try {
    const bin = atob(clean);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    if (out.length < 100 || out.length > 8_388_608) return null;
    // PNG magic
    if (out[0] !== 0x89 || out[1] !== 0x50) return null;
    return out;
  } catch {
    return null;
  }
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
  if (!playerKey || !username || !writeSecret || writeSecret.length < 16) {
    return jsonResponse(400, {
      error: "playerKey, username, writeSecret (>=16) required",
    });
  }

  const skinModel =
    asString(body.skinModel).toLowerCase() === "slim" ? "slim" : "classic";
  const sharePublic = body.sharePublic !== false;
  const capeMeta =
    body.capeMeta && typeof body.capeMeta === "object"
      ? body.capeMeta
      : {};
  const cosmetics =
    body.cosmetics && typeof body.cosmetics === "object"
      ? body.cosmetics
      : {};

  const secretHash = await sha256Hex(writeSecret);
  const sb = createClient(supabaseUrl, serviceKey);

  const { data: existing } = await sb
    .from("cosmetics_profiles")
    .select("player_key,write_secret_hash,skin_path,cape_path")
    .eq("player_key", playerKey)
    .maybeSingle();

  if (existing && existing.write_secret_hash !== secretHash) {
    return jsonResponse(403, { error: "writeSecret mismatch" });
  }

  let skinPath: string | null = existing?.skin_path ?? null;
  let capePath: string | null = existing?.cape_path ?? null;

  const skinB64 = asString(body.skinPngBase64);
  if (skinB64) {
    const bytes = decodeBase64Png(skinB64);
    if (!bytes) return jsonResponse(400, { error: "invalid skin PNG" });
    skinPath = `${playerKey}/skin.png`;
    const { error: upErr } = await sb.storage
      .from("cosmetics")
      .upload(skinPath, bytes, {
        contentType: "image/png",
        upsert: true,
      });
    if (upErr) return jsonResponse(500, { error: upErr.message });
  }

  const capeB64 = asString(body.capePngBase64);
  if (capeB64) {
    const bytes = decodeBase64Png(capeB64);
    if (!bytes) return jsonResponse(400, { error: "invalid cape PNG" });
    capePath = `${playerKey}/cape.png`;
    const { error: upErr } = await sb.storage
      .from("cosmetics")
      .upload(capePath, bytes, {
        contentType: "image/png",
        upsert: true,
      });
    if (upErr) return jsonResponse(500, { error: upErr.message });
  }

  const row = {
    player_key: playerKey,
    username,
    skin_path: skinPath,
    cape_path: capePath,
    skin_model: skinModel,
    cape_meta: capeMeta,
    cosmetics,
    write_secret_hash: secretHash,
    share_public: sharePublic,
    updated_at: new Date().toISOString(),
  };

  const { error } = await sb.from("cosmetics_profiles").upsert(row, {
    onConflict: "player_key",
  });
  if (error) return jsonResponse(500, { error: error.message });

  return jsonResponse(200, {
    ok: true,
    playerKey,
    username,
    skinPath,
    capePath,
    sharePublic,
  });
});
