// Customer Accept of a CreationResult — awards Kudos to the worker device signer.
// Requires Supabase Auth JWT (verify_jwt=true). No award on worker reply / Fog.

import { createClient } from "https://esm.sh/@supabase/supabase-js@2.49.1";

const MAX_ACCEPTS_PER_HOUR = 40;

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

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") {
    return new Response("ok", { headers: corsHeaders });
  }
  if (req.method !== "POST") {
    return jsonResponse(405, { error: "method not allowed" });
  }

  const supabaseUrl = Deno.env.get("SUPABASE_URL") ?? "";
  const serviceKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "";
  const anonKey = Deno.env.get("SUPABASE_ANON_KEY") ?? "";
  if (!supabaseUrl || !serviceKey) {
    return jsonResponse(500, { error: "server misconfigured" });
  }

  const authHeader = req.headers.get("Authorization") ?? "";
  if (!authHeader.toLowerCase().startsWith("bearer ")) {
    return jsonResponse(401, {
      error: "login required — register and sign in to accept",
    });
  }

  const userClient = createClient(supabaseUrl, anonKey || serviceKey, {
    global: { headers: { Authorization: authHeader } },
    auth: { persistSession: false, autoRefreshToken: false },
  });
  const { data: userData, error: userErr } = await userClient.auth.getUser();
  if (userErr || !userData.user) {
    return jsonResponse(401, {
      error: "invalid or expired session — sign in again",
    });
  }
  const userId = userData.user.id;

  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return jsonResponse(400, { error: "invalid JSON body" });
  }

  const jobId = asString(body.jobId).trim();
  const workerSignerPublicKey = asString(body.workerSignerPublicKey).trim();
  const amountRaw = body.amount;
  const amount =
    typeof amountRaw === "number" && Number.isFinite(amountRaw)
      ? Math.max(1, Math.min(500, Math.floor(amountRaw)))
      : 50;

  if (!jobId || !workerSignerPublicKey) {
    return jsonResponse(400, {
      error: "jobId and workerSignerPublicKey required",
    });
  }
  if (jobId.length > 128 || workerSignerPublicKey.length > 256) {
    return jsonResponse(400, { error: "jobId or workerSignerPublicKey too long" });
  }

  const admin = createClient(supabaseUrl, serviceKey, {
    auth: { persistSession: false, autoRefreshToken: false },
  });

  // Rate limit by auth user (reuse capsule_publish_rate table).
  const rateKey = `accept-creation-user:${userId}`;
  const { data: rateRow } = await admin
    .from("capsule_publish_rate")
    .select("window_start, publish_count")
    .eq("signer_public_key", rateKey)
    .maybeSingle();
  const now = new Date();
  let acceptCount = 1;
  let windowStart = now.toISOString();
  if (rateRow?.window_start) {
    const start = new Date(rateRow.window_start as string);
    const elapsedMs = now.getTime() - start.getTime();
    if (elapsedMs < 60 * 60 * 1000) {
      acceptCount = (rateRow.publish_count as number) + 1;
      windowStart = rateRow.window_start as string;
      if (acceptCount > MAX_ACCEPTS_PER_HOUR) {
        return jsonResponse(429, { error: "too many accepts; try again later" });
      }
    }
  }
  await admin.from("capsule_publish_rate").upsert(
    {
      signer_public_key: rateKey,
      window_start: windowStart,
      publish_count: acceptCount,
    },
    { onConflict: "signer_public_key" },
  );

  const { data: award, error: awardErr } = await admin.rpc(
    "kudos_award_creation_accept",
    {
      p_beneficiary_key: workerSignerPublicKey,
      p_job_id: jobId,
      p_voter_user_id: userId,
      p_amount: amount,
    },
  );

  if (awardErr) {
    return jsonResponse(500, { error: awardErr.message });
  }

  return jsonResponse(200, {
    ok: true,
    jobId,
    kudos: award ?? null,
  });
});
