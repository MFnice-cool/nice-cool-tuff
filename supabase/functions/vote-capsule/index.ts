// Peer confirm/reject — requires Supabase Auth JWT (verify_jwt=true).
// Votes only accumulate Keep/Discard counts + trust.
// Final status (saved/rejected) is set by admin moderation (admin panel), not auto-threshold.

import { createClient } from "https://esm.sh/@supabase/supabase-js@2.49.1";

const MAX_VOTES_PER_HOUR = 40;

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

function trustFromCounts(confirm: number, reject: number): number {
  const c = Math.max(0, confirm);
  const r = Math.max(0, reject);
  // Soft-verify: rejects/rollbacks weigh 2× (NotebookLM ranking).
  return c / (c + 2 * r + 1);
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
      error: "login required — register and sign in to vote",
    });
  }

  // Validate user JWT (anon client + user token).
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

  const contentHash = asString(body.contentHash);
  const capsuleId = asString(body.capsuleId) || asString(body.id);
  const vote = asString(body.vote).toLowerCase();

  if ((!contentHash && !capsuleId) || !vote) {
    return jsonResponse(400, {
      error: "contentHash (or capsuleId) and vote required",
    });
  }
  if (vote !== "confirm" && vote !== "reject") {
    return jsonResponse(400, { error: "vote must be confirm or reject" });
  }

  const admin = createClient(supabaseUrl, serviceKey, {
    auth: { persistSession: false, autoRefreshToken: false },
  });

  let lookup = admin
    .from("experience_capsules")
    .select(
      "content_hash, signer_public_key, confirm_count, reject_count, success_count, fail_count, status",
    );
  lookup = contentHash
    ? lookup.eq("content_hash", contentHash)
    : lookup.eq("id", capsuleId);
  const { data: capsule, error: capErr } = await lookup.maybeSingle();

  if (capErr || !capsule) {
    return jsonResponse(404, { error: "capsule not found" });
  }

  const resolvedHash = capsule.content_hash as string;
  if (capsule.status === "rejected") {
    return jsonResponse(403, { error: "capsule already rejected" });
  }

  // Rate limit by auth user
  const rateKey = `vote-user:${userId}`;
  const { data: rateRow } = await admin
    .from("capsule_publish_rate")
    .select("window_start, publish_count")
    .eq("signer_public_key", rateKey)
    .maybeSingle();
  const now = new Date();
  let voteCount = 1;
  let windowStart = now.toISOString();
  if (rateRow) {
    const start = new Date(rateRow.window_start);
    if (now.getTime() - start.getTime() < 60 * 60 * 1000) {
      if ((rateRow.publish_count as number) >= MAX_VOTES_PER_HOUR) {
        return jsonResponse(429, { error: "vote rate limit exceeded" });
      }
      voteCount = (rateRow.publish_count as number) + 1;
      windowStart = rateRow.window_start;
    }
  }
  await admin.from("capsule_publish_rate").upsert({
    signer_public_key: rateKey,
    window_start: windowStart,
    publish_count: voteCount,
  });

  const { error: voteInsertErr } = await admin.from("capsule_votes").insert({
    content_hash: resolvedHash,
    voter_user_id: userId,
    vote,
    voter_public_key: null,
    signature: null,
  });
  if (voteInsertErr) {
    if (voteInsertErr.code === "23505") {
      return jsonResponse(409, { error: "already voted on this capsule" });
    }
    return jsonResponse(500, { error: voteInsertErr.message });
  }

  let confirm = capsule.confirm_count as number;
  let reject = capsule.reject_count as number;
  let success = capsule.success_count as number;
  let fail = capsule.fail_count as number;
  if (vote === "confirm") {
    confirm += 1;
    success += 1;
  } else {
    reject += 1;
    fail += 1;
  }

  const trust = trustFromCounts(confirm, reject);
  // Keep admin decisions sticky; community votes never flip saved/rejected.
  let status = capsule.status as string;
  if (status !== "saved" && status !== "rejected") {
    status = "open";
  }

  const successScore = status === "rejected"
    ? 0
    : status === "saved"
    ? Math.min(1, Math.max(0.35, trust))
    : Math.min(0.2, trust);

  const { error: updErr } = await admin.from("experience_capsules").update({
    confirm_count: confirm,
    reject_count: reject,
    success_count: success,
    fail_count: fail,
    trust_score: trust,
    success_score: successScore,
    status,
    updated_at: now.toISOString(),
  }).eq("content_hash", resolvedHash);

  if (updErr) {
    return jsonResponse(500, { error: updErr.message });
  }

  // Kudos/RAC: reward capsule author on soft-verify confirm only (not Fog, not reject).
  let kudos: Record<string, unknown> | null = null;
  if (vote === "confirm") {
    const authorKey = asString(capsule.signer_public_key).trim();
    if (authorKey) {
      const { data: award, error: awardErr } = await admin.rpc(
        "kudos_award_soft_verify",
        {
          p_beneficiary_key: authorKey,
          p_content_hash: resolvedHash,
          p_voter_user_id: userId,
          p_amount: 10,
        },
      );
      if (awardErr) {
        console.warn("kudos_award_soft_verify:", awardErr.message);
      } else if (award && typeof award === "object") {
        kudos = award as Record<string, unknown>;
      }
    }
  }

  return jsonResponse(200, {
    ok: true,
    contentHash: resolvedHash,
    vote,
    status,
    confirmCount: confirm,
    rejectCount: reject,
    trustScore: trust,
    userId,
    kudos,
  });
});
