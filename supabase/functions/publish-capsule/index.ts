// TuffSwarm: ingest signed ExperienceCapsule with anti-spam / anti-wrong-fix gates.
// Trust: new capsules are open (success_count=0). Auth peers vote via vote-capsule → saved/rejected.
// Service role only inside this function — never ship it to clients.

import { createClient } from "https://esm.sh/@supabase/supabase-js@2.49.1";
import * as ed from "https://esm.sh/@noble/ed25519@2.1.0";
import { sha512 } from "https://esm.sh/@noble/hashes@1.4.0/sha512.js";

// @noble/ed25519@2.x is dependency-free by default; sync/async verify needs SHA-512 wired.
ed.etc.sha512Sync = (...m: Uint8Array[]) => sha512(ed.etc.concatBytes(...m));
ed.etc.sha512Async = (...m: Uint8Array[]) =>
  Promise.resolve(sha512(ed.etc.concatBytes(...m)));

const KNOWN_OPS = new Set([
  "install_mod",
  "remove_mod",
  "disable_mod",
  "update_mod",
  "change_mod_version",
  "reinstall_mod",
  "edit_config",
]);

const MAX_PUBLISH_PER_HOUR = 10;
const MAX_FP_PUBLISH_PER_DAY = 15;
const MAX_OPEN_PER_SIGNER_FP = 3;
const MIN_SOLUTION_LEN = 12;
const MAX_SOLUTION_LEN = 2000;
const MAX_ACTIONS = 8;
const MAX_PAYLOAD_BYTES = 48 * 1024;

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

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function decodeBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** Must match Rust ExperienceCapsule::canonical_bytes. */
function computeContentHash(
  fingerprintKey: string,
  solution: string,
  actions: unknown,
): Promise<string> {
  const actionsJson = JSON.stringify(actions ?? []);
  const canonical = `${fingerprintKey}\n${solution}\n${actionsJson}`;
  return crypto.subtle
    .digest("SHA-256", new TextEncoder().encode(canonical))
    .then((buf) => bytesToHex(new Uint8Array(buf)));
}

function validateActions(actions: unknown): string | null {
  if (!Array.isArray(actions)) return "actions must be an array";
  if (actions.length < 1) {
    return "at least one executable action is required (text-only solutions rejected)";
  }
  if (actions.length > MAX_ACTIONS) {
    return `too many actions (max ${MAX_ACTIONS})`;
  }
  for (const a of actions) {
    if (!a || typeof a !== "object") return "invalid action entry";
    const row = a as Record<string, unknown>;
    const op = asString(row.op);
    if (!KNOWN_OPS.has(op)) return `unknown action op: ${op || "(empty)"}`;
    // Require a target for mod ops so spam like disable_mod with empty modId dies.
    if (
      ["install_mod", "remove_mod", "disable_mod", "update_mod", "change_mod_version", "reinstall_mod"]
        .includes(op)
    ) {
      const modId = asString(row.modId).trim();
      if (!modId) return `action ${op} requires modId`;
    }
    if (op === "edit_config" && !asString(row.path).trim()) {
      return "edit_config requires path";
    }
  }
  return null;
}

function qualityGate(solution: string, fingerprintKey: string): string | null {
  const s = solution.trim();
  if (s.length < MIN_SOLUTION_LEN) {
    return `solution too short (min ${MIN_SOLUTION_LEN} chars)`;
  }
  if (s.length > MAX_SOLUTION_LEN) {
    return `solution too long (max ${MAX_SOLUTION_LEN} chars)`;
  }
  if (fingerprintKey.trim().length < 3) {
    return "fingerprint.key too short";
  }
  // Obvious spam / placeholder
  const low = s.toLowerCase();
  const banned = ["asdf", "test test", "lorem ipsum", "xxx", "qwerty"];
  if (banned.some((b) => low === b || low.startsWith(b + " "))) {
    return "solution looks like placeholder spam";
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
    return jsonResponse(400, { error: "invalid JSON body" });
  }

  const rawSize = JSON.stringify(body).length;
  if (rawSize > MAX_PAYLOAD_BYTES) {
    return jsonResponse(413, { error: "payload too large" });
  }

  const fingerprint =
    (body.fingerprint as Record<string, unknown> | undefined) ?? {};
  const fingerprintKey =
    asString(fingerprint.key) || asString(body.fingerprintKey);
  const solution = asString(body.solution) ||
    asString(body.humanExplanation);
  const actions = body.actions ?? body.launcherActions ?? [];
  const contentHash = asString(body.contentHash);
  const signature = asString(body.signature);
  const signerPublicKey = asString(body.signerPublicKey);
  const signerPeerId = asString(body.signerPeerId) || null;
  const id = asString(body.id) || `cap-${contentHash.slice(0, 32)}`;
  const mcMajor = asString(fingerprint.mcMajor) || null;
  const loader = asString(fingerprint.loader) || null;

  if (!fingerprintKey.trim()) {
    return jsonResponse(400, { error: "fingerprint.key is empty" });
  }
  if (!contentHash || !signature || !signerPublicKey) {
    return jsonResponse(400, {
      error: "contentHash, signature, and signerPublicKey are required",
    });
  }

  const qErr = qualityGate(solution, fingerprintKey);
  if (qErr) return jsonResponse(400, { error: qErr });

  const privacy = body.privacy as Record<string, unknown> | undefined;
  if (privacy?.rawLogs === true || privacy?.notesIncluded === true) {
    return jsonResponse(400, { error: "raw logs / notes are not allowed" });
  }
  if (body.notes != null && asString(body.notes).trim() !== "") {
    return jsonResponse(400, { error: "notes must not be included" });
  }
  const rawBlob = JSON.stringify(body).toLowerCase();
  if (
    rawBlob.includes("---- minecraft crash report ----") ||
    rawBlob.includes("latest.log")
  ) {
    return jsonResponse(400, { error: "payload looks like raw crash logs" });
  }

  const actionsErr = validateActions(actions);
  if (actionsErr) return jsonResponse(400, { error: actionsErr });

  const expectedHash = await computeContentHash(
    fingerprintKey,
    solution,
    actions,
  );
  if (expectedHash !== contentHash) {
    return jsonResponse(400, {
      error: "contentHash does not match canonical payload",
    });
  }

  let pk: Uint8Array;
  let sig: Uint8Array;
  try {
    pk = decodeBase64(signerPublicKey);
    sig = decodeBase64(signature);
  } catch {
    return jsonResponse(400, { error: "invalid base64 for key/signature" });
  }
  if (pk.length !== 32 || sig.length !== 64) {
    return jsonResponse(400, {
      error: "signerPublicKey must be 32 bytes and signature 64 bytes",
    });
  }

  const msg = new TextEncoder().encode(contentHash);
  let ok = false;
  try {
    ok = await ed.verify(sig, msg, pk);
  } catch (e) {
    const detail = e instanceof Error ? e.message : String(e);
    return jsonResponse(400, {
      error: "signature verify error",
      detail,
    });
  }
  if (!ok) {
    return jsonResponse(400, { error: "signature verify failed" });
  }

  const admin = createClient(supabaseUrl, serviceKey, {
    auth: { persistSession: false, autoRefreshToken: false },
  });

  // Signer hourly rate limit.
  const { data: rateRow } = await admin
    .from("capsule_publish_rate")
    .select("window_start, publish_count")
    .eq("signer_public_key", signerPublicKey)
    .maybeSingle();

  const now = new Date();
  let publishCount = 1;
  let windowStart = now.toISOString();
  if (rateRow) {
    const start = new Date(rateRow.window_start);
    const ageMs = now.getTime() - start.getTime();
    if (ageMs < 60 * 60 * 1000) {
      if ((rateRow.publish_count as number) >= MAX_PUBLISH_PER_HOUR) {
        return jsonResponse(429, {
          error: "rate limit exceeded for this signer",
        });
      }
      publishCount = (rateRow.publish_count as number) + 1;
      windowStart = rateRow.window_start;
    }
  }
  await admin.from("capsule_publish_rate").upsert({
    signer_public_key: signerPublicKey,
    window_start: windowStart,
    publish_count: publishCount,
  });

  // Per-fingerprint daily cap (limits flooding wrong fixes for one crash class).
  const { data: fpRate } = await admin
    .from("capsule_fp_publish_rate")
    .select("window_start, publish_count")
    .eq("fingerprint_key", fingerprintKey)
    .maybeSingle();
  let fpCount = 1;
  let fpWindow = now.toISOString();
  if (fpRate) {
    const start = new Date(fpRate.window_start);
    if (now.getTime() - start.getTime() < 24 * 60 * 60 * 1000) {
      if ((fpRate.publish_count as number) >= MAX_FP_PUBLISH_PER_DAY) {
        return jsonResponse(429, {
          error: "rate limit exceeded for this fingerprint",
        });
      }
      fpCount = (fpRate.publish_count as number) + 1;
      fpWindow = fpRate.window_start;
    }
  }
  await admin.from("capsule_fp_publish_rate").upsert({
    fingerprint_key: fingerprintKey,
    window_start: fpWindow,
    publish_count: fpCount,
  });

  // Cap pending capsules per signer+fingerprint (anti multi-wrong spam).
  const { count: pendingCount } = await admin
    .from("experience_capsules")
    .select("content_hash", { count: "exact", head: true })
    .eq("signer_public_key", signerPublicKey)
    .eq("fingerprint_key", fingerprintKey)
    .eq("status", "open");

  const { data: existing } = await admin
    .from("experience_capsules")
    .select(
      "success_count, fail_count, confirm_count, reject_count, status, trust_score, signer_public_key",
    )
    .eq("content_hash", contentHash)
    .maybeSingle();

  if (
    !existing &&
    (pendingCount ?? 0) >= MAX_OPEN_PER_SIGNER_FP
  ) {
    return jsonResponse(429, {
      error:
        "too many open unverified capsules for this crash from this device",
    });
  }

  // NEVER trust client success_count / successScore. New = pending @ 0.
  // Idempotent re-publish keeps existing trust counters.
  const successCount = existing ? (existing.success_count as number) : 0;
  const failCount = existing ? (existing.fail_count as number) : 0;
  const confirmCount = existing ? (existing.confirm_count as number) : 0;
  const rejectCount = existing ? (existing.reject_count as number) : 0;
  const status = existing ? (existing.status as string) : "open";
  const trustScore = existing
    ? (existing.trust_score as number)
    : 0;
  // Soft display score for pending (lookup ranks low until peer confirms).
  const successScore = status === "saved"
    ? Math.min(1, trustScore || 0.5)
    : Math.min(0.2, trustScore);

  if (existing && existing.status === "rejected") {
    return jsonResponse(403, {
      error: "capsule is rejected; republish rejected",
    });
  }

  const sanitizedPayload = {
    schemaVersion: body.schemaVersion ?? 1,
    id,
    fingerprint: {
      exception: fingerprint.exception ?? null,
      frames: fingerprint.frames ?? [],
      modFile: fingerprint.modFile ?? null,
      mixin: fingerprint.mixin ?? null,
      mcMajor,
      loader,
      key: fingerprintKey,
    },
    solution,
    actions,
    successScore,
    successCount,
    failCount,
    confirmCount,
    rejectCount,
    status,
    trustScore,
    contentHash,
    signerPublicKey,
    signature,
    signerPeerId,
    privacy: { rawLogs: false, notesIncluded: false },
  };

  const { error: upsertErr } = await admin.from("experience_capsules").upsert({
    content_hash: contentHash,
    id,
    fingerprint_key: fingerprintKey,
    mc_major: mcMajor,
    loader,
    solution,
    actions,
    success_score: successScore,
    success_count: successCount,
    fail_count: failCount,
    confirm_count: confirmCount,
    reject_count: rejectCount,
    trust_score: trustScore,
    status,
    signer_public_key: signerPublicKey,
    signature,
    signer_peer_id: signerPeerId,
    payload: sanitizedPayload,
    updated_at: now.toISOString(),
  }, { onConflict: "content_hash" });

  if (upsertErr) {
    return jsonResponse(500, { error: upsertErr.message });
  }

  return jsonResponse(200, {
    ok: true,
    contentHash,
    id,
    status,
    successCount,
    trustScore,
    deduped: !!existing,
    note:
      status === "open"
        ? "Capsule stored as open — peer Keep/Discard votes accumulate; admin moderation sets saved/rejected"
        : undefined,
  });
});

