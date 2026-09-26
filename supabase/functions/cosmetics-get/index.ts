// Public read for CustomSkinLoader + tuffbox-cosmetics mod.
// GET ?username=Steve  or  ?uuid=<player_key>
// Also serves CustomSkinAPI: GET /cosmetics-get/csl/<username>.json via path rewrite —
// query form: ?username=X&format=csl

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

function publicUrl(supabaseUrl: string, path: string | null): string | null {
  if (!path) return null;
  if (path.startsWith("http://") || path.startsWith("https://")) return path;
  return `${supabaseUrl}/storage/v1/object/public/cosmetics/${path}`;
}

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") {
    return new Response("ok", { headers: corsHeaders });
  }
  if (req.method !== "GET") {
    return jsonResponse(405, { error: "method not allowed" });
  }

  const supabaseUrl = Deno.env.get("SUPABASE_URL") ?? "";
  const serviceKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "";
  if (!supabaseUrl || !serviceKey) {
    return jsonResponse(500, { error: "server misconfigured" });
  }

  const url = new URL(req.url);
  let username = (url.searchParams.get("username") ?? "").trim();
  const uuid = (url.searchParams.get("uuid") ?? "").trim();
  let format = (url.searchParams.get("format") ?? "full").trim().toLowerCase();

  // CustomSkinAPI: GET .../cosmetics-get/{username}.json
  const pathMatch = url.pathname.match(/\/([^/]+)\.json$/i);
  if (pathMatch && !username) {
    username = decodeURIComponent(pathMatch[1]);
    format = "csl";
  }

  if (!username && !uuid) {
    return jsonResponse(400, { error: "username or uuid required" });
  }

  const sb = createClient(supabaseUrl, serviceKey);
  let q = sb
    .from("cosmetics_profiles")
    .select(
      "player_key,username,skin_path,cape_path,skin_model,cape_meta,cosmetics,share_public,updated_at",
    )
    .eq("share_public", true)
    .limit(1);

  if (uuid) {
    q = q.eq("player_key", uuid);
  } else {
    q = q.ilike("username", username);
  }

  const { data, error } = await q.maybeSingle();
  if (error) {
    return jsonResponse(500, { error: error.message });
  }
  if (!data) {
    return new Response(null, { status: 204, headers: corsHeaders });
  }

  const skinUrl = publicUrl(supabaseUrl, data.skin_path);
  const capeUrl = publicUrl(supabaseUrl, data.cape_path);

  if (format === "csl") {
    // CustomSkinAPI-compatible (BlessingSkin-style) for CustomSkinLoader loadlist root.
    const body: Record<string, unknown> = {
      username: data.username,
      skins: skinUrl ? { default: skinUrl } : {},
    };
    if (capeUrl) body.cape = capeUrl;
    if (data.skin_model === "slim" && skinUrl) {
      body.skins = { slim: skinUrl, default: skinUrl };
    }
    return jsonResponse(200, body);
  }

  return jsonResponse(200, {
    playerKey: data.player_key,
    username: data.username,
    skinUrl,
    capeUrl,
    skinModel: data.skin_model,
    capeMeta: data.cape_meta ?? {},
    cosmetics: data.cosmetics ?? {},
    updatedAt: data.updated_at,
  });
});
