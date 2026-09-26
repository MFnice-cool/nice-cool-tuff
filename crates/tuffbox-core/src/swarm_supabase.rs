//! Supabase transport for ExperienceCapsule (Phase B+ preferred remote).
//!
//! Publish / vote / accept go through Edge Functions (`functions/v1/*`) and must use
//! the legacy JWT anon key (not publishable `sb_publishable_…`).
//! Lookup uses PostgREST SELECT with anon/publishable key (RLS read-only).

use crate::action_plan::LauncherAction;
use crate::crash_remote::{CrashLookupHit, CrashLookupResponse};
use crate::swarm::ExperienceCapsule;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const APP_USER_AGENT: &str = "TuffBox-IDE/0.1";

fn join_url(base: &str, path: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{base}/{path}")
}

fn supabase_headers(anon_key: &str) -> Result<reqwest::header::HeaderMap, String> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static(APP_USER_AGENT),
    );
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&format!("Bearer {anon_key}"))
            .map_err(|e| e.to_string())?,
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("apikey"),
        reqwest::header::HeaderValue::from_str(anon_key).map_err(|e| e.to_string())?,
    );
    Ok(headers)
}

/// POST `{supabaseUrl}/functions/v1/publish-capsule`
pub async fn publish_capsule_supabase(
    supabase_url: &str,
    anon_key: &str,
    capsule: &ExperienceCapsule,
) -> Result<Value, String> {
    let url = supabase_url.trim();
    let key = anon_key.trim();
    if url.is_empty() {
        return Err("Supabase URL is not configured".into());
    }
    if key.is_empty() {
        return Err("Supabase anon key is not configured".into());
    }
    if capsule.verify_signature()? != true {
        return Err("capsule must be Ed25519-signed before Supabase publish".into());
    }
    let public = capsule.to_public_json();
    let endpoint = join_url(url, "functions/v1/publish-capsule");
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(&endpoint)
        .headers(supabase_headers(key)?)
        .json(&public)
        .send()
        .await
        .map_err(|e| format!("supabase publish failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        let msg = body
            .get("error")
            .or_else(|| body.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("supabase publish {status}: {msg}"));
    }
    Ok(body)
}

/// GET experience_capsules filtered by fingerprint_key (exact) with optional loader/mc filters.
/// Prefers `saved` / high trust; `open` still returned but ranked low.
pub async fn lookup_capsules_supabase(
    supabase_url: &str,
    anon_key: &str,
    fingerprint_key: &str,
    loader: Option<&str>,
    mc_major: Option<&str>,
    limit: u32,
) -> Result<CrashLookupResponse, String> {
    let url = supabase_url.trim();
    let key = anon_key.trim();
    if url.is_empty() {
        return Err("Supabase URL is not configured".into());
    }
    if key.is_empty() {
        return Err("Supabase anon key is not configured".into());
    }
    let fp = fingerprint_key.trim();
    if fp.is_empty() {
        return Err("fingerprint key is empty".into());
    }
    let limit = limit.clamp(1, 25);

    // Prefer saved first, then trust/success. Rejected hidden by RLS.
    let mut query = format!(
        "experience_capsules?select=id,content_hash,fingerprint_key,solution,actions,success_score,success_count,confirm_count,reject_count,trust_score,status,payload&fingerprint_key=eq.{}&status=in.(saved,open)&order=status.asc,trust_score.desc,success_count.desc&limit={}",
        urlencoding_minimal(fp),
        limit
    );
    if let Some(l) = loader.map(str::trim).filter(|s| !s.is_empty()) {
        query.push_str(&format!("&loader=eq.{}", urlencoding_minimal(l)));
    }
    if let Some(m) = mc_major.map(str::trim).filter(|s| !s.is_empty()) {
        query.push_str(&format!("&mc_major=eq.{}", urlencoding_minimal(m)));
    }

    let endpoint = join_url(url, &format!("rest/v1/{query}"));
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(&endpoint)
        .headers(supabase_headers(key)?)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("supabase lookup failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!([]));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("supabase lookup {status}: {msg}"));
    }

    let rows = body.as_array().cloned().unwrap_or_default();
    let mut hits = Vec::new();
    for row in rows {
        if let Some(hit) = row_to_hit(&row) {
            hits.push(hit);
        }
    }
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok(CrashLookupResponse {
        kb_version: Some("supabase".into()),
        hits,
    })
}

/// POST `{supabaseUrl}/functions/v1/vote-capsule` — peer confirm/reject.
/// Requires a Supabase Auth user `access_token` (JWT). Device Ed25519 is not accepted.
/// `id_or_hash` may be full content_hash (64 hex) or capsule `id` / `cap-…`.
pub async fn vote_capsule_supabase(
    supabase_url: &str,
    anon_key: &str,
    id_or_hash: &str,
    vote: &str,
    access_token: &str,
) -> Result<Value, String> {
    let url = supabase_url.trim();
    let key = anon_key.trim();
    let token = access_token.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase is not configured".into());
    }
    if token.is_empty() {
        return Err("login required — register and sign in to vote".into());
    }
    let content_hash = resolve_content_hash(url, key, id_or_hash).await?;
    let endpoint = join_url(url, "functions/v1/vote-capsule");
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let payload = json!({
        "contentHash": content_hash,
        "vote": vote.trim().to_ascii_lowercase(),
    });
    let mut headers = supabase_headers(key)?;
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|e| e.to_string())?,
    );
    let response = client
        .post(&endpoint)
        .headers(headers)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("supabase vote failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        let msg = body
            .get("error")
            .or_else(|| body.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("supabase vote {status}: {msg}"));
    }
    Ok(body)
}

/// POST `{supabaseUrl}/functions/v1/accept-creation` — award worker Kudos after customer Accept.
/// `anon_key` must be the Edge JWT anon; `access_token` is the signed-in user session.
pub async fn accept_creation_supabase(
    supabase_url: &str,
    anon_key: &str,
    job_id: &str,
    worker_signer_public_key: &str,
    access_token: &str,
    amount: Option<u32>,
) -> Result<Value, String> {
    let url = supabase_url.trim();
    let key = anon_key.trim();
    let token = access_token.trim();
    let job = job_id.trim();
    let worker_pk = worker_signer_public_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase is not configured".into());
    }
    if token.is_empty() {
        return Err("login required — register and sign in to accept".into());
    }
    if job.is_empty() {
        return Err("jobId required".into());
    }
    if worker_pk.is_empty() {
        return Err("workerSignerPublicKey required".into());
    }
    let endpoint = join_url(url, "functions/v1/accept-creation");
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let mut payload = json!({
        "jobId": job,
        "workerSignerPublicKey": worker_pk,
    });
    if let Some(a) = amount {
        payload["amount"] = json!(a);
    }
    let mut headers = supabase_headers(key)?;
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|e| e.to_string())?,
    );
    let response = client
        .post(&endpoint)
        .headers(headers)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("supabase accept-creation failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        let msg = body
            .get("error")
            .or_else(|| body.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("supabase accept-creation {status}: {msg}"));
    }
    Ok(body)
}

/// Read public Kudos/RAC balance for a capsule author (`signer_public_key`).
pub async fn fetch_kudos_balance_supabase(
    supabase_url: &str,
    anon_key: &str,
    beneficiary_key: &str,
) -> Result<Value, String> {
    let url = supabase_url.trim();
    let key = anon_key.trim();
    let beneficiary = beneficiary_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase is not configured".into());
    }
    if beneficiary.is_empty() {
        return Err("beneficiary key empty".into());
    }
    let query = format!(
        "kudos_balances?select=beneficiary_key,total_kudos,rac,rac_updated_at,updated_at&beneficiary_key=eq.{}&limit=1",
        urlencoding_minimal(beneficiary)
    );
    let endpoint = join_url(url, &format!("rest/v1/{query}"));
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(&endpoint)
        .headers(supabase_headers(key)?)
        .send()
        .await
        .map_err(|e| format!("supabase kudos fetch failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!([]));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("supabase kudos {status}: {msg}"));
    }
    let row = body
        .as_array()
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "beneficiaryKey": beneficiary,
                "totalKudos": 0,
                "rac": 0,
            })
        });
    let total = row
        .get("total_kudos")
        .or_else(|| row.get("totalKudos"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let rac = row.get("rac").and_then(|v| v.as_f64()).unwrap_or(0.0);
    Ok(json!({
        "beneficiaryKey": beneficiary,
        "totalKudos": total,
        "rac": rac,
        "racUpdatedAt": row.get("rac_updated_at").or_else(|| row.get("racUpdatedAt")).cloned(),
        "updatedAt": row.get("updated_at").or_else(|| row.get("updatedAt")).cloned(),
    }))
}

fn looks_like_content_hash(s: &str) -> bool {
    let s = s.trim();
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

async fn resolve_content_hash(
    supabase_url: &str,
    anon_key: &str,
    id_or_hash: &str,
) -> Result<String, String> {
    let id = id_or_hash.trim();
    if id.is_empty() {
        return Err("empty capsule id".into());
    }
    if looks_like_content_hash(id) {
        return Ok(id.to_string());
    }
    let query = if id.starts_with("cap-") {
        format!(
            "experience_capsules?select=content_hash&id=eq.{}&limit=1",
            urlencoding_minimal(id)
        )
    } else {
        // Try id first; also allow content_hash prefix match via id field.
        format!(
            "experience_capsules?select=content_hash&or=(id.eq.{0},content_hash.eq.{0})&limit=1",
            urlencoding_minimal(id)
        )
    };
    let endpoint = join_url(supabase_url, &format!("rest/v1/{query}"));
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(&endpoint)
        .headers(supabase_headers(anon_key)?)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("resolve content_hash failed: {e}"))?;
    let body: Value = response.json().await.unwrap_or(json!([]));
    body.as_array()
        .and_then(|a| a.first())
        .and_then(|r| r.get("content_hash"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("capsule not found for id {id}"))
}

fn urlencoding_minimal(s: &str) -> String {
    // Encode reserved PostgREST / URL characters; leave safe unreserved as-is.
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn row_to_hit(row: &Value) -> Option<CrashLookupHit> {
    let id = row.get("id")?.as_str()?.to_string();
    let status = row.get("status").and_then(|v| v.as_str()).unwrap_or("open");
    if status == "rejected" || status == "quarantined" {
        return None;
    }
    let solution = row
        .get("solution")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if solution.trim().is_empty() {
        return None;
    }
    let fingerprint_key = row
        .get("fingerprint_key")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let trust = row
        .get("trust_score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let success_score = row
        .get("success_score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let success_count = row
        .get("success_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as f64;
    let confirm_count = row
        .get("confirm_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as f64;
    // Pending / unverified solutions stay low so wrong fixes don't dominate RAG.
    let status_boost = if status == "saved" || status == "active" {
        0.35
    } else {
        0.0
    };
    let score = (status_boost
        + trust * 0.4
        + success_score * 0.15
        + (confirm_count / (confirm_count + 3.0)) * 0.1)
        .clamp(0.0, 1.0);
    // Extra dampening when never peer-confirmed.
    let score = if success_count < 1.0 && status != "saved" && status != "active" {
        (score * 0.35).min(0.22)
    } else {
        score
    };

    let actions: Vec<LauncherAction> = row
        .get("actions")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let suspected_mods: Vec<String> = row
        .get("payload")
        .and_then(|p| p.get("fingerprint"))
        .and_then(|f| f.get("modFile"))
        .and_then(|v| v.as_str())
        .map(|s| vec![s.to_string()])
        .unwrap_or_default();

    // Prefer full content_hash in id slot for later voting when present.
    let vote_id = row
        .get("content_hash")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or(id);

    Some(CrashLookupHit {
        id: vote_id,
        score,
        solution,
        suspected_mods,
        actions,
        fingerprint_key,
    })
}

/// Community inbox row for Crash Votes UI (trust + fingerprint + fix plan).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityCapsuleCard {
    pub id: String,
    pub content_hash: String,
    pub fingerprint_key: String,
    pub exception: Option<String>,
    pub frames: Vec<String>,
    pub mod_file: Option<String>,
    pub mc_major: Option<String>,
    pub loader: Option<String>,
    pub solution: String,
    pub actions: Vec<LauncherAction>,
    pub involved_mods: Vec<String>,
    pub status: String,
    pub trust_score: f64,
    pub trust_percent: u32,
    pub confirm_count: u32,
    pub reject_count: u32,
    pub success_count: u32,
    pub fail_count: u32,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    /// Capsule author device key (Kudos beneficiary).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer_public_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_total_kudos: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_rac: Option<f64>,
}

/// List community capsules for the Crash Votes board (open + saved).
pub async fn list_community_capsules_supabase(
    supabase_url: &str,
    anon_key: &str,
    status_filter: Option<&str>,
    limit: u32,
) -> Result<Vec<CommunityCapsuleCard>, String> {
    let url = supabase_url.trim();
    let key = anon_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase is not configured".into());
    }
    let limit = limit.clamp(1, 100);
    let status_clause = match status_filter.map(str::trim).filter(|s| !s.is_empty()) {
        Some("open") | Some("pending") => "status=eq.open",
        Some("saved") | Some("active") => "status=eq.saved",
        Some("rejected") | Some("quarantined") => "status=eq.rejected",
        Some("all") | None => "status=in.(open,saved)",
        Some(other) => {
            return Err(format!("unknown status filter: {other}"));
        }
    };
    let query = format!(
        "experience_capsules?select=id,content_hash,fingerprint_key,solution,actions,success_score,success_count,fail_count,confirm_count,reject_count,trust_score,status,payload,mc_major,loader,signer_public_key,created_at,updated_at&{status_clause}&order=trust_score.desc,created_at.desc&limit={limit}"
    );
    let endpoint = join_url(url, &format!("rest/v1/{query}"));
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(45))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(&endpoint)
        .headers(supabase_headers(key)?)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("supabase list failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!([]));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("supabase list {status}: {msg}"));
    }
    let rows = body.as_array().cloned().unwrap_or_default();
    let mut cards: Vec<CommunityCapsuleCard> =
        rows.iter().filter_map(row_to_community_card).collect();
    enrich_community_cards_with_kudos(url, key, &mut cards).await;
    Ok(cards)
}

/// One PostgREST round-trip for unique author keys on the Crash Votes page.
async fn enrich_community_cards_with_kudos(
    supabase_url: &str,
    anon_key: &str,
    cards: &mut [CommunityCapsuleCard],
) {
    let mut keys: Vec<String> = cards
        .iter()
        .filter_map(|c| c.signer_public_key.clone())
        .filter(|k| !k.trim().is_empty())
        .collect();
    keys.sort();
    keys.dedup();
    if keys.is_empty() {
        return;
    }
    let Ok(map) = fetch_kudos_balances_batch_supabase(supabase_url, anon_key, &keys).await else {
        return;
    };
    for card in cards.iter_mut() {
        let Some(ref pk) = card.signer_public_key else {
            continue;
        };
        if let Some((total, rac)) = map.get(pk) {
            card.author_total_kudos = Some(*total);
            card.author_rac = Some(*rac);
        } else {
            card.author_total_kudos = Some(0.0);
            card.author_rac = Some(0.0);
        }
    }
}

/// Batch-read public Kudos/RAC for many beneficiary keys (one IN query).
pub async fn fetch_kudos_balances_batch_supabase(
    supabase_url: &str,
    anon_key: &str,
    beneficiary_keys: &[String],
) -> Result<std::collections::HashMap<String, (f64, f64)>, String> {
    let url = supabase_url.trim();
    let key = anon_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase is not configured".into());
    }
    let mut unique: Vec<String> = beneficiary_keys
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    unique.sort();
    unique.dedup();
    if unique.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    // PostgREST `in.(a,b)` — values may contain `/+` from base64; encode each.
    let in_list = unique
        .iter()
        .map(|k| format!("\"{}\"", k.replace('"', "")))
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "kudos_balances?select=beneficiary_key,total_kudos,rac&beneficiary_key=in.({})",
        in_list
    );
    let endpoint = join_url(url, &format!("rest/v1/{query}"));
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(&endpoint)
        .headers(supabase_headers(key)?)
        .send()
        .await
        .map_err(|e| format!("supabase kudos batch failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!([]));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("supabase kudos batch {status}: {msg}"));
    }
    let mut out = std::collections::HashMap::new();
    if let Some(rows) = body.as_array() {
        for row in rows {
            let Some(bk) = row
                .get("beneficiary_key")
                .or_else(|| row.get("beneficiaryKey"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
            else {
                continue;
            };
            let total = row
                .get("total_kudos")
                .or_else(|| row.get("totalKudos"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let rac = row.get("rac").and_then(|v| v.as_f64()).unwrap_or(0.0);
            out.insert(bk, (total, rac));
        }
    }
    Ok(out)
}

fn row_to_community_card(row: &Value) -> Option<CommunityCapsuleCard> {
    let content_hash = row
        .get("content_hash")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?
        .to_string();
    let id = row
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or(&content_hash)
        .to_string();
    let status = row
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("open")
        .to_string();
    if status == "rejected" || status == "quarantined" {
        return None;
    }
    let solution = row
        .get("solution")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if solution.trim().is_empty() {
        return None;
    }
    let fingerprint_key = row
        .get("fingerprint_key")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let payload = row.get("payload").cloned().unwrap_or(Value::Null);
    let fp = payload.get("fingerprint").cloned().unwrap_or(Value::Null);
    let exception = fp
        .get("exception")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            fingerprint_key
                .split('|')
                .next()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        });
    let frames: Vec<String> = fp
        .get("frames")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .take(12)
                .collect()
        })
        .unwrap_or_default();
    let mod_file = fp
        .get("modFile")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mc_major = row
        .get("mc_major")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            fp.get("mcMajor")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });
    let loader = row
        .get("loader")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            fp.get("loader")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });
    let actions: Vec<LauncherAction> = row
        .get("actions")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let mut involved: Vec<String> = Vec::new();
    if let Some(m) = &mod_file {
        if !m.is_empty() {
            involved.push(m.clone());
        }
    }
    for a in &actions {
        if let Some(m) = a.mod_id.as_ref().filter(|s| !s.is_empty()) {
            if !involved.iter().any(|x| x == m) {
                involved.push(m.clone());
            }
        }
    }
    let trust = row
        .get("trust_score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let confirm = row
        .get("confirm_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let reject = row
        .get("reject_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    Some(CommunityCapsuleCard {
        id,
        content_hash,
        fingerprint_key,
        exception,
        frames,
        mod_file,
        mc_major,
        loader,
        solution,
        actions,
        involved_mods: involved,
        status,
        trust_score: trust,
        trust_percent: (trust * 100.0).round() as u32,
        confirm_count: confirm,
        reject_count: reject,
        success_count: row
            .get("success_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        fail_count: row.get("fail_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        created_at: row
            .get("created_at")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        updated_at: row
            .get("updated_at")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        signer_public_key: row
            .get("signer_public_key")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        author_total_kudos: None,
        author_rac: None,
    })
}

/// POST `{supabaseUrl}/functions/v1/report-cooccurrence`
/// Uploads a mod set; server expands pairs and increments community counts.
pub async fn report_cooccurrence_supabase(
    supabase_url: &str,
    anon_key: &str,
    mod_ids: &[String],
    mc_version: &str,
    loader: &str,
    source: &str,
    client_key: Option<&str>,
) -> Result<Value, String> {
    let url = supabase_url.trim();
    let key = anon_key.trim();
    if url.is_empty() {
        return Err("Supabase URL is not configured".into());
    }
    if key.is_empty() {
        return Err("Supabase anon key is not configured".into());
    }
    let ids = crate::swarm::normalize_mod_id_list(mod_ids, 48);
    if ids.len() < 2 {
        return Err("need at least 2 mods to report co-occurrence".into());
    }
    let endpoint = join_url(url, "functions/v1/report-cooccurrence");
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let mut payload = json!({
        "modIds": ids,
        "mcVersion": mc_version,
        "loader": loader,
        "source": source,
    });
    if let Some(ck) = client_key.map(str::trim).filter(|s| !s.is_empty()) {
        payload["clientKey"] = json!(ck);
        payload["signerPublicKey"] = json!(ck);
    }
    let response = client
        .post(&endpoint)
        .headers(supabase_headers(key)?)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("supabase co-occurrence report failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        let msg = body
            .get("error")
            .or_else(|| body.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("supabase co-occurrence report {status}: {msg}"));
    }
    Ok(body)
}

/// GET top co-occurrence pairs from Supabase for Create Mode AI context.
pub async fn fetch_cooccurrence_supabase(
    supabase_url: &str,
    anon_key: &str,
    mc_version: &str,
    loader: &str,
    limit: u32,
) -> Result<Vec<crate::swarm::ModPairStat>, String> {
    let url = supabase_url.trim();
    let key = anon_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase is not configured".into());
    }
    let limit = limit.clamp(1, 100);
    let loader = loader.trim().to_ascii_lowercase();
    let mc = mc_version.trim();

    // Prefer exact mc+loader; fall back to loader-only if sparse.
    let mut pairs = fetch_cooccurrence_rows(url, key, Some(mc), Some(&loader), limit).await?;
    if pairs.len() < (limit as usize / 3).max(5) {
        let broader = fetch_cooccurrence_rows(url, key, None, Some(&loader), limit).await?;
        pairs = crate::swarm::merge_cooccurrence_pairs(&pairs, &broader, limit as usize);
    }
    if pairs.len() < (limit as usize / 4).max(3) {
        let global = fetch_cooccurrence_rows(url, key, None, None, limit).await?;
        pairs = crate::swarm::merge_cooccurrence_pairs(&pairs, &global, limit as usize);
    }
    Ok(pairs)
}

async fn fetch_cooccurrence_rows(
    supabase_url: &str,
    anon_key: &str,
    mc_version: Option<&str>,
    loader: Option<&str>,
    limit: u32,
) -> Result<Vec<crate::swarm::ModPairStat>, String> {
    let mut filters = Vec::new();
    if let Some(mc) = mc_version.map(str::trim).filter(|s| !s.is_empty()) {
        filters.push(format!("mc_version=eq.{}", urlencoding_minimal(mc)));
    }
    if let Some(ld) = loader.map(str::trim).filter(|s| !s.is_empty()) {
        filters.push(format!("loader=eq.{}", urlencoding_minimal(ld)));
    }
    let filter_q = if filters.is_empty() {
        String::new()
    } else {
        format!("&{}", filters.join("&"))
    };
    let query = format!(
        "mod_cooccurrence_pairs?select=mod_a,mod_b,count&order=count.desc&limit={limit}{filter_q}"
    );
    let endpoint = join_url(supabase_url, &format!("rest/v1/{query}"));
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(&endpoint)
        .headers(supabase_headers(anon_key)?)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("supabase co-occurrence fetch failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!([]));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("supabase co-occurrence fetch {status}: {msg}"));
    }
    let rows = body.as_array().cloned().unwrap_or_default();
    Ok(rows
        .iter()
        .filter_map(|row| {
            let a = row.get("mod_a")?.as_str()?.trim();
            let b = row.get("mod_b")?.as_str()?.trim();
            if a.is_empty() || b.is_empty() {
                return None;
            }
            Some(crate::swarm::ModPairStat {
                mod_a: a.to_string(),
                mod_b: b.to_string(),
                count: row.get("count").and_then(|v| v.as_u64()).unwrap_or(1),
            })
        })
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartnerStat {
    pub partner: String,
    pub pack_count: u64,
}

/// RPC `partners_for_mod` — top companions for a seed mod.
pub async fn partners_for_mod_supabase(
    supabase_url: &str,
    anon_key: &str,
    mod_id: &str,
    limit: u32,
    loader: Option<&str>,
    mc_version: Option<&str>,
) -> Result<Vec<PartnerStat>, String> {
    let url = supabase_url.trim();
    let key = anon_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase is not configured".into());
    }
    let endpoint = join_url(url, "rest/v1/rpc/partners_for_mod");
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let payload = json!({
        "p_mod": mod_id.trim().to_ascii_lowercase(),
        "p_limit": limit.clamp(1, 50),
        "p_loader": loader.map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()),
        "p_mc_version": mc_version.map(str::trim).filter(|s| !s.is_empty()),
    });
    let response = client
        .post(&endpoint)
        .headers(supabase_headers(key)?)
        .header("Accept", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("partners_for_mod failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!([]));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("partners_for_mod {status}: {msg}"));
    }
    let rows = body.as_array().cloned().unwrap_or_default();
    Ok(rows
        .iter()
        .filter_map(|row| {
            let partner = row.get("partner")?.as_str()?.trim();
            if partner.is_empty() {
                return None;
            }
            Some(PartnerStat {
                partner: partner.to_string(),
                pack_count: row.get("pack_count").and_then(|v| v.as_u64()).unwrap_or(1),
            })
        })
        .collect())
}

/// Service-role: seed/bump co-occurrence pairs (hub MPI analytics).
pub async fn seed_cooccurrence_pairs_supabase(
    supabase_url: &str,
    service_role_key: &str,
    pairs: &[Value],
) -> Result<u64, String> {
    let url = supabase_url.trim();
    let key = service_role_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase service role is not configured".into());
    }
    if pairs.is_empty() {
        return Ok(0);
    }
    let endpoint = join_url(url, "rest/v1/rpc/seed_cooccurrence_pairs");
    let client = reqwest::Client::builder()
        .user_agent("TuffSwarm-Analytics/1.0")
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    // Chunk to keep payloads reasonable.
    let mut total = 0u64;
    for chunk in pairs.chunks(400) {
        let response = client
            .post(&endpoint)
            .headers(supabase_headers(key)?)
            .header("Accept", "application/json")
            .json(&json!({ "pairs": chunk }))
            .send()
            .await
            .map_err(|e| format!("seed_cooccurrence_pairs failed: {e}"))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(json!(0));
        if !status.is_success() {
            let msg = body
                .get("message")
                .or_else(|| body.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("request rejected");
            return Err(format!("seed_cooccurrence_pairs {status}: {msg}"));
        }
        total += body.as_u64().unwrap_or(chunk.len() as u64);
    }
    Ok(total)
}

/// Service-role: seed/bump Modpack Index co-occurrence pairs (separate table).
pub async fn seed_mpi_cooccurrence_pairs_supabase(
    supabase_url: &str,
    service_role_key: &str,
    pairs: &[Value],
) -> Result<u64, String> {
    let url = supabase_url.trim();
    let key = service_role_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase service role is not configured".into());
    }
    if pairs.is_empty() {
        return Ok(0);
    }
    let endpoint = join_url(url, "rest/v1/rpc/seed_mpi_cooccurrence_pairs");
    let client = reqwest::Client::builder()
        .user_agent("TuffSwarm-Analytics/1.0")
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let mut total = 0u64;
    for chunk in pairs.chunks(400) {
        let response = client
            .post(&endpoint)
            .headers(supabase_headers(key)?)
            .header("Accept", "application/json")
            .json(&json!({ "pairs": chunk }))
            .send()
            .await
            .map_err(|e| format!("seed_mpi_cooccurrence_pairs failed: {e}"))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(json!(0));
        if !status.is_success() {
            let msg = body
                .get("message")
                .or_else(|| body.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("request rejected");
            return Err(format!("seed_mpi_cooccurrence_pairs {status}: {msg}"));
        }
        total += body.as_u64().unwrap_or(chunk.len() as u64);
    }
    Ok(total)
}

/// Service-role: rebuild `mod_partner_tops` JSONB cache from pair tables.
pub async fn refresh_mod_partner_tops_supabase(
    supabase_url: &str,
    service_role_key: &str,
) -> Result<u64, String> {
    let url = supabase_url.trim();
    let key = service_role_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase service role is not configured".into());
    }
    let endpoint = join_url(url, "rest/v1/rpc/refresh_mod_partner_tops");
    let client = reqwest::Client::builder()
        .user_agent("TuffSwarm-Analytics/1.0")
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(&endpoint)
        .headers(supabase_headers(key)?)
        .header("Accept", "application/json")
        .json(&json!({}))
        .send()
        .await
        .map_err(|e| format!("refresh_mod_partner_tops failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!(0));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("refresh_mod_partner_tops {status}: {msg}"));
    }
    Ok(body.as_u64().unwrap_or(0))
}

/// RPC `partners_for_mod_mpi` — companions from Modpack Index graph only.
pub async fn partners_for_mod_mpi_supabase(
    supabase_url: &str,
    anon_key: &str,
    mod_id: &str,
    limit: u32,
    loader: Option<&str>,
    mc_version: Option<&str>,
    category_slug: Option<&str>,
) -> Result<Vec<PartnerStat>, String> {
    let url = supabase_url.trim();
    let key = anon_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase is not configured".into());
    }
    let endpoint = join_url(url, "rest/v1/rpc/partners_for_mod_mpi");
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let payload = json!({
        "p_mod": mod_id.trim().to_ascii_lowercase(),
        "p_limit": limit.clamp(1, 50),
        "p_loader": loader.map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()),
        "p_mc_version": mc_version.map(str::trim).filter(|s| !s.is_empty()),
        "p_category_slug": category_slug.map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()),
    });
    let response = client
        .post(&endpoint)
        .headers(supabase_headers(key)?)
        .header("Accept", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("partners_for_mod_mpi failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!([]));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("partners_for_mod_mpi {status}: {msg}"));
    }
    let rows = body.as_array().cloned().unwrap_or_default();
    Ok(rows
        .iter()
        .filter_map(|row| {
            let partner = row.get("partner")?.as_str()?.trim();
            if partner.is_empty() {
                return None;
            }
            Some(PartnerStat {
                partner: partner.to_string(),
                pack_count: row.get("pack_count").and_then(|v| v.as_u64()).unwrap_or(1),
            })
        })
        .collect())
}

/// GET top pairs from the MPI-only co-occurrence table.
pub async fn fetch_mpi_cooccurrence_supabase(
    supabase_url: &str,
    anon_key: &str,
    mc_version: &str,
    loader: &str,
    limit: u32,
) -> Result<Vec<crate::swarm::ModPairStat>, String> {
    let url = supabase_url.trim();
    let key = anon_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase is not configured".into());
    }
    let limit = limit.clamp(1, 100);
    let loader = loader.trim().to_ascii_lowercase();
    let mc = mc_version.trim();
    let mut pairs = fetch_mpi_cooccurrence_rows(url, key, Some(mc), Some(&loader), limit).await?;
    if pairs.len() < (limit as usize / 3).max(5) {
        let broader = fetch_mpi_cooccurrence_rows(url, key, None, Some(&loader), limit).await?;
        pairs = crate::swarm::merge_cooccurrence_pairs(&pairs, &broader, limit as usize);
    }
    if pairs.len() < (limit as usize / 4).max(3) {
        let global = fetch_mpi_cooccurrence_rows(url, key, None, None, limit).await?;
        pairs = crate::swarm::merge_cooccurrence_pairs(&pairs, &global, limit as usize);
    }
    Ok(pairs)
}

async fn fetch_mpi_cooccurrence_rows(
    supabase_url: &str,
    anon_key: &str,
    mc_version: Option<&str>,
    loader: Option<&str>,
    limit: u32,
) -> Result<Vec<crate::swarm::ModPairStat>, String> {
    let mut filters = Vec::new();
    if let Some(mc) = mc_version.map(str::trim).filter(|s| !s.is_empty()) {
        filters.push(format!("mc_version=eq.{}", urlencoding_minimal(mc)));
    }
    if let Some(ld) = loader.map(str::trim).filter(|s| !s.is_empty()) {
        filters.push(format!("loader=eq.{}", urlencoding_minimal(ld)));
    }
    let filter_q = if filters.is_empty() {
        String::new()
    } else {
        format!("&{}", filters.join("&"))
    };
    let query = format!(
        "mpi_mod_cooccurrence_pairs?select=mod_a,mod_b,count&order=count.desc&limit={limit}{filter_q}"
    );
    let endpoint = join_url(supabase_url, &format!("rest/v1/{query}"));
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(&endpoint)
        .headers(supabase_headers(anon_key)?)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("supabase MPI co-occurrence fetch failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!([]));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("supabase MPI co-occurrence fetch {status}: {msg}"));
    }
    let rows = body.as_array().cloned().unwrap_or_default();
    Ok(rows
        .iter()
        .filter_map(|row| {
            let a = row.get("mod_a")?.as_str()?.trim();
            let b = row.get("mod_b")?.as_str()?.trim();
            if a.is_empty() || b.is_empty() {
                return None;
            }
            Some(crate::swarm::ModPairStat {
                mod_a: a.to_string(),
                mod_b: b.to_string(),
                count: row.get("count").and_then(|v| v.as_u64()).unwrap_or(1),
            })
        })
        .collect())
}

/// Service-role: true if this MPI pack was already synced for scope.
pub async fn mpi_pack_already_synced(
    supabase_url: &str,
    service_role_key: &str,
    mpi_pack_id: u64,
    mc_version: &str,
    loader: &str,
) -> Result<bool, String> {
    mpi_pack_already_synced_scoped(
        supabase_url,
        service_role_key,
        mpi_pack_id,
        mc_version,
        loader,
        "",
    )
    .await
}

pub async fn mpi_pack_already_synced_scoped(
    supabase_url: &str,
    service_role_key: &str,
    mpi_pack_id: u64,
    mc_version: &str,
    loader: &str,
    category_slug: &str,
) -> Result<bool, String> {
    let url = supabase_url.trim();
    let key = service_role_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase service role is not configured".into());
    }
    let query = format!(
        "mpi_pack_sync?select=mpi_pack_id&mpi_pack_id=eq.{mpi_pack_id}&mc_version=eq.{}&loader=eq.{}&category_slug=eq.{}&limit=1",
        urlencoding_minimal(mc_version.trim()),
        urlencoding_minimal(&loader.trim().to_ascii_lowercase()),
        urlencoding_minimal(&category_slug.trim().to_ascii_lowercase())
    );
    let endpoint = join_url(url, &format!("rest/v1/{query}"));
    let client = reqwest::Client::builder()
        .user_agent("TuffSwarm-Analytics/1.0")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(&endpoint)
        .headers(supabase_headers(key)?)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("mpi_pack_sync lookup failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!([]));
    if !status.is_success() {
        return Err(format!("mpi_pack_sync lookup {status}"));
    }
    Ok(body.as_array().map(|a| !a.is_empty()).unwrap_or(false))
}

/// Service-role: mark MPI pack as synced.
pub async fn mark_mpi_pack_synced(
    supabase_url: &str,
    service_role_key: &str,
    mpi_pack_id: u64,
    mc_version: &str,
    loader: &str,
    mod_count: i32,
) -> Result<(), String> {
    mark_mpi_pack_synced_scoped(
        supabase_url,
        service_role_key,
        mpi_pack_id,
        mc_version,
        loader,
        "",
        mod_count,
    )
    .await
}

pub async fn mark_mpi_pack_synced_scoped(
    supabase_url: &str,
    service_role_key: &str,
    mpi_pack_id: u64,
    mc_version: &str,
    loader: &str,
    category_slug: &str,
    mod_count: i32,
) -> Result<(), String> {
    let url = supabase_url.trim();
    let key = service_role_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase service role is not configured".into());
    }
    let endpoint = join_url(url, "rest/v1/mpi_pack_sync");
    let client = reqwest::Client::builder()
        .user_agent("TuffSwarm-Analytics/1.0")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let payload = json!({
        "mpi_pack_id": mpi_pack_id,
        "mc_version": mc_version.trim(),
        "loader": loader.trim().to_ascii_lowercase(),
        "category_slug": category_slug.trim().to_ascii_lowercase(),
        "mod_count": mod_count,
        "synced_at": crate::time_util::rfc3339_now(),
    });
    let response = client
        .post(&endpoint)
        .headers(supabase_headers(key)?)
        .header("Prefer", "resolution=merge-duplicates")
        .header("Accept", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("mpi_pack_sync upsert failed: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("mpi_pack_sync upsert {status}: {body}"));
    }
    Ok(())
}

/// POST `/rest/v1/rpc/launcher_heartbeat` — mark this device online and open/extend a session.
pub async fn launcher_heartbeat_supabase(
    supabase_url: &str,
    anon_key: &str,
    device_id: &str,
    display_name: Option<&str>,
    app_version: Option<&str>,
) -> Result<Value, String> {
    rpc_launcher(
        supabase_url,
        anon_key,
        "launcher_heartbeat",
        json!({
            "p_device_id": device_id,
            "p_display_name": display_name,
            "p_app_version": app_version,
        }),
    )
    .await
}

/// POST `/rest/v1/rpc/launcher_goodbye` — end session and go offline.
pub async fn launcher_goodbye_supabase(
    supabase_url: &str,
    anon_key: &str,
    device_id: &str,
    reason: &str,
) -> Result<Value, String> {
    rpc_launcher(
        supabase_url,
        anon_key,
        "launcher_goodbye",
        json!({
            "p_device_id": device_id,
            "p_reason": reason,
        }),
    )
    .await
}

/// POST `/rest/v1/rpc/launcher_online_stats` — online count + list.
pub async fn launcher_online_stats_supabase(
    supabase_url: &str,
    anon_key: &str,
) -> Result<Value, String> {
    rpc_launcher(supabase_url, anon_key, "launcher_online_stats", json!({})).await
}

/// POST `/rest/v1/rpc/launcher_online_series` — minute buckets for charts.
pub async fn launcher_online_series_supabase(
    supabase_url: &str,
    anon_key: &str,
    hours: u32,
) -> Result<Value, String> {
    rpc_launcher(
        supabase_url,
        anon_key,
        "launcher_online_series",
        json!({ "p_hours": hours.max(1) }),
    )
    .await
}

/// POST `/rest/v1/rpc/launcher_game_session_start`.
pub async fn launcher_game_session_start_supabase(
    supabase_url: &str,
    anon_key: &str,
    device_id: &str,
    project_name: Option<&str>,
    app_version: Option<&str>,
) -> Result<Value, String> {
    rpc_launcher(
        supabase_url,
        anon_key,
        "launcher_game_session_start",
        json!({
            "p_device_id": device_id,
            "p_project_name": project_name,
            "p_app_version": app_version,
        }),
    )
    .await
}

/// POST `/rest/v1/rpc/launcher_game_session_end`.
pub async fn launcher_game_session_end_supabase(
    supabase_url: &str,
    anon_key: &str,
    device_id: &str,
    duration_seconds: Option<u64>,
    reason: &str,
) -> Result<Value, String> {
    rpc_launcher(
        supabase_url,
        anon_key,
        "launcher_game_session_end",
        json!({
            "p_device_id": device_id,
            "p_duration_seconds": duration_seconds,
            "p_reason": reason,
        }),
    )
    .await
}

/// POST `/rest/v1/rpc/launcher_recent_sessions`.
pub async fn launcher_recent_sessions_supabase(
    supabase_url: &str,
    anon_key: &str,
    limit: u32,
) -> Result<Value, String> {
    rpc_launcher(
        supabase_url,
        anon_key,
        "launcher_recent_sessions",
        json!({ "p_limit": limit }),
    )
    .await
}

async fn rpc_launcher(
    supabase_url: &str,
    anon_key: &str,
    fn_name: &str,
    body: Value,
) -> Result<Value, String> {
    let url = supabase_url.trim();
    let key = anon_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase is not configured".into());
    }
    let endpoint = join_url(url, &format!("rest/v1/rpc/{fn_name}"));
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(&endpoint)
        .headers(supabase_headers(key)?)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("supabase {fn_name} failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .or_else(|| body.get("hint"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("supabase {fn_name} {status}: {msg}"));
    }
    Ok(body)
}

/// RPC `optimize_mods_for` — custom Optimize list (FO for Fabric; other loaders later).
#[derive(Debug, Clone)]
pub struct OptimizeModRow {
    pub modrinth_slug: String,
    pub sort_order: i32,
    pub name: Option<String>,
    pub source: Option<String>,
}

pub async fn optimize_mods_for_supabase(
    supabase_url: &str,
    anon_key: &str,
    loader: &str,
    mc_version: &str,
) -> Result<Vec<OptimizeModRow>, String> {
    let url = supabase_url.trim();
    let key = anon_key.trim();
    if url.is_empty() || key.is_empty() {
        return Err("Supabase is not configured".into());
    }
    let endpoint = join_url(url, "rest/v1/rpc/optimize_mods_for");
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let payload = json!({
        "p_loader": loader.trim().to_ascii_lowercase(),
        "p_mc_version": mc_version.trim(),
    });
    let response = client
        .post(&endpoint)
        .headers(supabase_headers(key)?)
        .header("Accept", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("optimize_mods_for failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!([]));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("optimize_mods_for {status}: {msg}"));
    }
    let rows = body.as_array().cloned().unwrap_or_default();
    Ok(rows
        .iter()
        .filter_map(|row| {
            let slug = row.get("modrinth_slug")?.as_str()?.trim();
            if slug.is_empty() {
                return None;
            }
            Some(OptimizeModRow {
                modrinth_slug: slug.to_ascii_lowercase(),
                sort_order: row.get("sort_order").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                name: row
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
                source: row
                    .get("source")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            })
        })
        .collect())
}

/// Convert DB rows into local OptimizeModCandidate shape (reason/risk defaults).
pub fn optimize_rows_to_candidates(
    rows: &[OptimizeModRow],
) -> Vec<crate::optimize_pack::OptimizeModCandidate> {
    let mut out = Vec::with_capacity(rows.len());
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        let slug = row.modrinth_slug.trim().to_ascii_lowercase();
        if slug.is_empty() || !seen.insert(slug.clone()) {
            continue;
        }
        let name = row.name.clone().unwrap_or_else(|| slug.clone());
        let source = row.source.as_deref().unwrap_or("fabulously-optimized");
        out.push(crate::optimize_pack::OptimizeModCandidate {
            slug: slug.clone(),
            name,
            reason: format!("From {source} catalog for this Minecraft version"),
            risk: "low".into(),
            category: "performance".into(),
            modrinth_slug: Some(slug),
            curseforge_slug: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencoding_encodes_pipe() {
        assert_eq!(urlencoding_minimal("a|b"), "a%7Cb");
    }

    #[test]
    fn optimize_rows_to_candidates_dedupes() {
        let rows = vec![
            OptimizeModRow {
                modrinth_slug: "sodium".into(),
                sort_order: 0,
                name: Some("Sodium".into()),
                source: Some("fabulously-optimized".into()),
            },
            OptimizeModRow {
                modrinth_slug: "sodium".into(),
                sort_order: 1,
                name: None,
                source: None,
            },
        ];
        let c = optimize_rows_to_candidates(&rows);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].slug, "sodium");
        assert_eq!(c[0].modrinth_slug.as_deref(), Some("sodium"));
    }

    /// Smoke: `mod_partner_tops.partners` JSONB matches PartnerStat / RPC columns.
    #[test]
    fn partner_tops_jsonb_shape_parses() {
        let cached = json!([
            {"partner": "jei", "pack_count": 42},
            {"partner": "sodium", "pack_count": 10},
            {"partner": "", "pack_count": 99},
            {"pack_count": 1}
        ]);
        let stats: Vec<PartnerStat> = cached
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|row| {
                let partner = row.get("partner")?.as_str()?.trim();
                if partner.is_empty() {
                    return None;
                }
                Some(PartnerStat {
                    partner: partner.to_string(),
                    pack_count: row.get("pack_count").and_then(|v| v.as_u64()).unwrap_or(1),
                })
            })
            .collect();
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].partner, "jei");
        assert_eq!(stats[0].pack_count, 42);
        assert_eq!(stats[1].partner, "sodium");
    }

    #[test]
    fn refresh_mod_partner_tops_rpc_path() {
        let endpoint = join_url(
            "https://example.supabase.co",
            "rest/v1/rpc/refresh_mod_partner_tops",
        );
        assert_eq!(
            endpoint,
            "https://example.supabase.co/rest/v1/rpc/refresh_mod_partner_tops"
        );
    }
}
