//! Remote private crash knowledge-base client.
//!
//! The full KB corpus never ships in the launcher. Clients may only:
//! - `lookup` — top-N similar cases for the current fingerprint (local AI RAG)
//! - `diagnose` — server-side plan (KB ± LLM)
//!
//! Offline fallback uses the thin builtin seed in `crash_kb`.

use crate::action_plan::{parse_action_plan_value, ActionPlan, LauncherAction};
use crate::ai_explanation::AiAction;
use crate::crash_kb::{CrashFingerprint, SimilarCaseHit};
use crate::http::{http, http_async};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashLookupRequest {
    pub fingerprint: CrashFingerprint,
    #[serde(default)]
    pub excerpt: Option<String>,
    #[serde(default)]
    pub mc_version: Option<String>,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashLookupHit {
    pub id: String,
    pub score: f64,
    pub solution: String,
    #[serde(default)]
    pub suspected_mods: Vec<String>,
    #[serde(default)]
    pub actions: Vec<LauncherAction>,
    #[serde(default)]
    pub fingerprint_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashLookupResponse {
    #[serde(default)]
    pub kb_version: Option<String>,
    #[serde(default)]
    pub hits: Vec<CrashLookupHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashDiagnoseRequest {
    pub fingerprint: CrashFingerprint,
    #[serde(default)]
    pub context: Option<Value>,
    #[serde(default)]
    pub excerpt: Option<String>,
    /// When true, server may skip LLM if a strong KB match exists.
    #[serde(default)]
    pub prefer_kb_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashDiagnoseResponse {
    pub plan: ActionPlan,
    #[serde(default)]
    pub kb_version: Option<String>,
    #[serde(default)]
    pub used_llm: bool,
}

/// Convert remote lookup hits into the prompt/RAG SimilarCaseHit shape.
/// `anchored` marks hits that identify THE SAME crash (fingerprint-key
/// equality/soft-match or a blamed-mod overlap) — remote scores alone are
/// not anchors, they are additive over generic signals server-side too.
pub fn hits_to_similar_cases(
    hits: &[CrashLookupHit],
    fp: &crate::crash_kb::CrashFingerprint,
) -> Vec<SimilarCaseHit> {
    hits.iter()
        .map(|h| SimilarCaseHit {
            id: h.id.clone(),
            score: h.score,
            anchored: remote_hit_anchored(h, fp),
            solution: h.solution.clone(),
            suspected_mods: h.suspected_mods.clone(),
            actions: h
                .actions
                .iter()
                .map(|a| AiAction {
                    action_type: match a.op.as_str() {
                        "install_mod" => "install".into(),
                        "remove_mod" => "remove".into(),
                        "disable_mod" => "disable".into(),
                        "update_mod" | "change_mod_version" | "reinstall_mod" => "update".into(),
                        "edit_config" => "config_change".into(),
                        other => other.into(),
                    },
                    mod_id: a.mod_id.clone(),
                    description: a.reason.clone().unwrap_or_default(),
                    risk: a.risk.clone(),
                })
                .collect(),
            fingerprint_key: h.fingerprint_key.clone(),
            source: "remote".into(),
        })
        .collect()
}


/// A remote hit counts as anchored only when it identifies the same crash:
/// fingerprint-key equality (exact, soft with the trailing blame suffix
/// ignored, or prefix) — or a blamed-mod overlap with our fingerprint.
fn remote_hit_anchored(h: &CrashLookupHit, fp: &crate::crash_kb::CrashFingerprint) -> bool {
    let trunc = |k: &str| {
        k.rsplit_once('|')
            .map(|(head, _)| head.to_string())
            .unwrap_or_else(|| k.to_string())
    };
    if !h.fingerprint_key.is_empty() && !fp.key.is_empty() {
        if h.fingerprint_key == fp.key
            || trunc(&h.fingerprint_key) == trunc(&fp.key)
            || h.fingerprint_key.starts_with(&fp.key)
            || fp.key.starts_with(&h.fingerprint_key)
        {
            return true;
        }
    }
    if !fp.blame_mod_ids.is_empty() {
        let hit_mods: Vec<String> = h
            .suspected_mods
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        if fp
            .blame_mod_ids
            .iter()
            .any(|id| hit_mods.contains(&id.to_ascii_lowercase()))
        {
            return true;
        }
    }
    false
}

fn join_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{base}/{path}")
}

/// POST /v1/crash/lookup — returns top-N cases for local-AI RAG (no full corpus).
pub fn lookup_remote(
    base_url: &str,
    token: Option<&str>,
    request: &CrashLookupRequest,
) -> Result<CrashLookupResponse, String> {
    if base_url.trim().is_empty() {
        return Err("crash KB endpoint is not configured".into());
    }
    let url = join_url(base_url, "/v1/crash/lookup");
    let client = http();
    let mut req = client.post(&url).json(request);
    if let Some(token) = token.filter(|t| !t.trim().is_empty()) {
        req = req.bearer_auth(token);
    }
    let response = req
        .send()
        .map_err(|e| format!("crash KB lookup failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().map_err(|e| e.to_string())?;
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("crash KB lookup {status}: {msg}"));
    }
    serde_json::from_value(body).map_err(|e| format!("invalid lookup response: {e}"))
}

/// POST /v1/crash/diagnose — server returns a ready ActionPlan.
pub fn diagnose_remote(
    base_url: &str,
    token: Option<&str>,
    request: &CrashDiagnoseRequest,
) -> Result<CrashDiagnoseResponse, String> {
    if base_url.trim().is_empty() {
        return Err("crash KB endpoint is not configured".into());
    }
    let url = join_url(base_url, "/v1/crash/diagnose");
    let client = http();
    let mut req = client.post(&url).json(request);
    if let Some(token) = token.filter(|t| !t.trim().is_empty()) {
        req = req.bearer_auth(token);
    }
    let response = req
        .send()
        .map_err(|e| format!("crash KB diagnose failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().map_err(|e| e.to_string())?;
    let body = unwrap_n8n_diagnose_body(body);
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("crash KB diagnose {status}: {msg}"));
    }

    // Accept either { plan: {...} } or a bare ActionPlan object.
    parse_diagnose_response_body(body)
}

/// Async wrappers for Tauri (uses reqwest async).
pub async fn lookup_remote_async(
    base_url: &str,
    token: Option<&str>,
    request: &CrashLookupRequest,
) -> Result<CrashLookupResponse, String> {
    if base_url.trim().is_empty() {
        return Err("crash KB endpoint is not configured".into());
    }
    let url = join_url(base_url, "/v1/crash/lookup");
    let client = http_async();
    let mut req = client.post(&url).json(request);
    if let Some(token) = token.filter(|t| !t.trim().is_empty()) {
        req = req.bearer_auth(token);
    }
    let response = req
        .send()
        .await
        .map_err(|e| format!("crash KB lookup failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("crash KB lookup {status}: {msg}"));
    }
    serde_json::from_value(body).map_err(|e| format!("invalid lookup response: {e}"))
}

pub async fn diagnose_remote_async(
    base_url: &str,
    token: Option<&str>,
    request: &CrashDiagnoseRequest,
) -> Result<CrashDiagnoseResponse, String> {
    if base_url.trim().is_empty() {
        return Err("crash KB endpoint is not configured".into());
    }
    let url = join_url(base_url, "/v1/crash/diagnose");
    let client = http_async();
    let mut req = client.post(&url).json(request);
    if let Some(token) = token.filter(|t| !t.trim().is_empty()) {
        req = req.bearer_auth(token);
    }
    let response = req
        .send()
        .await
        .map_err(|e| format!("crash KB diagnose failed: {e}"))?;
    let status = response.status();
    let body: Value = response.json().await.map_err(|e| e.to_string())?;
    let body = unwrap_n8n_diagnose_body(body);
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("crash KB diagnose {status}: {msg}"));
    }
    parse_diagnose_response_body(body)
}

/// n8n Webhook Response Mode often wraps payloads as `[{ "json": { ... } }]`.
pub fn unwrap_n8n_diagnose_body(body: Value) -> Value {
    if let Some(arr) = body.as_array() {
        if let Some(first) = arr.first() {
            if let Some(inner) = first.get("json") {
                return inner.clone();
            }
            return first.clone();
        }
    }
    body
}

fn parse_diagnose_response_body(body: Value) -> Result<CrashDiagnoseResponse, String> {
    if body.get("plan").is_some() {
        let mut resp: CrashDiagnoseResponse =
            serde_json::from_value(body).map_err(|e| format!("invalid diagnose response: {e}"))?;
        if let Ok(normalized) =
            parse_action_plan_value(&serde_json::to_value(&resp.plan).unwrap_or(json!({})))
        {
            resp.plan = normalized;
        }
        Ok(resp)
    } else {
        let plan = parse_action_plan_value(&body)?;
        Ok(CrashDiagnoseResponse {
            plan,
            kb_version: body
                .get("kbVersion")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            used_llm: body
                .get("usedLlm")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
        })
    }
}

/// POST /v1/crash/capsules — publish ExperienceCapsule (Phase B HTTP).
pub async fn publish_capsule_async(
    base_url: &str,
    token: Option<&str>,
    capsule: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    if base_url.trim().is_empty() {
        return Err("crash KB endpoint is not configured".into());
    }
    let url = join_url(base_url, "/v1/crash/capsules");
    let client = http_async();
    let mut req = client.post(&url).json(capsule);
    if let Some(token) = token.filter(|t| !t.trim().is_empty()) {
        req = req.bearer_auth(token);
    }
    let response = req
        .send()
        .await
        .map_err(|e| format!("capsule publish failed: {e}"))?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("capsule publish {status}: {msg}"));
    }
    Ok(body)
}

/// POST /v1/mods/cooccurrence — optional network stats for Creation trends.
/// Tries GET first (filtered hub/Supabase), then falls back to POST.
pub async fn fetch_cooccurrence_async(
    base_url: &str,
    token: Option<&str>,
    mc_version: &str,
    loader: &str,
    limit: u32,
) -> Result<serde_json::Value, String> {
    if base_url.trim().is_empty() {
        return Err("crash KB endpoint is not configured".into());
    }
    let client = http_async();

    let get_url = format!(
        "{}?version={}&loader={}&limit={}",
        join_url(base_url, "/v1/mods/cooccurrence"),
        urlencoding_simple(mc_version),
        urlencoding_simple(loader),
        limit
    );
    let mut get_req = client.get(&get_url);
    if let Some(token) = token.filter(|t| !t.trim().is_empty()) {
        get_req = get_req.bearer_auth(token);
    }
    if let Ok(response) = get_req.send().await {
        let status = response.status();
        if status.is_success() {
            let body: serde_json::Value = response.json().await.unwrap_or(json!({}));
            return Ok(body);
        }
    }

    let url = join_url(base_url, "/v1/mods/cooccurrence");
    let payload = json!({
        "mcVersion": mc_version,
        "loader": loader,
        "limit": limit,
    });
    let mut req = client.post(&url).json(&payload);
    if let Some(token) = token.filter(|t| !t.trim().is_empty()) {
        req = req.bearer_auth(token);
    }
    let response = req
        .send()
        .await
        .map_err(|e| format!("cooccurrence fetch failed: {e}"))?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("cooccurrence {status}: {msg}"));
    }
    Ok(body)
}

/// GET /v1/mods/modpacks — hub proxies Modpack Index (analytics UA).
pub async fn fetch_modpacks_async(
    base_url: &str,
    token: Option<&str>,
    query: Option<&str>,
    page: u32,
    limit: u32,
    category_id: Option<u32>,
    version: Option<&str>,
) -> Result<serde_json::Value, String> {
    if base_url.trim().is_empty() {
        return Err("hub endpoint is not configured".into());
    }
    let client = http_async();

    let mut url = format!(
        "{}?page={}&limit={}",
        join_url(base_url, "/v1/mods/modpacks"),
        page.max(1),
        limit.clamp(1, 40)
    );
    if let Some(q) = query.map(str::trim).filter(|s| !s.is_empty()) {
        url.push_str(&format!("&query={}", urlencoding_simple(q)));
    }
    if let Some(cid) = category_id {
        url.push_str(&format!("&categoryId={cid}"));
    }
    if let Some(v) = version.map(str::trim).filter(|s| !s.is_empty()) {
        url.push_str(&format!("&version={}", urlencoding_simple(v)));
    }

    let mut req = client.get(&url);
    if let Some(token) = token.filter(|t| !t.trim().is_empty()) {
        req = req.bearer_auth(token);
    }
    let response = req
        .send()
        .await
        .map_err(|e| format!("hub modpacks fetch failed: {e}"))?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("hub modpacks {status}: {msg}"));
    }
    Ok(body)
}

/// GET /v1/mods/modpack-categories — hub pack themes (+ MPI merge when available).
pub async fn fetch_modpack_categories_async(
    base_url: &str,
    token: Option<&str>,
) -> Result<serde_json::Value, String> {
    if base_url.trim().is_empty() {
        return Err("hub endpoint is not configured".into());
    }
    let client = http_async();
    let url = join_url(base_url, "/v1/mods/modpack-categories");
    let mut req = client.get(&url);
    if let Some(token) = token.filter(|t| !t.trim().is_empty()) {
        req = req.bearer_auth(token);
    }
    let response = req
        .send()
        .await
        .map_err(|e| format!("hub categories fetch failed: {e}"))?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("request rejected");
        return Err(format!("hub categories {status}: {msg}"));
    }
    Ok(body)
}

fn urlencoding_simple(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwraps_n8n_array_wrapper() {
        let wrapped = json!([{
            "json": {
                "schemaVersion": 1,
                "humanExplanation": "from n8n",
                "confidence": 0.9,
                "actions": [],
                "suspectedMods": [],
                "needsUserReview": true
            }
        }]);
        let body = unwrap_n8n_diagnose_body(wrapped);
        assert_eq!(body["humanExplanation"], "from n8n");
        let plan = parse_action_plan_value(&body).expect("parse plan");
        assert_eq!(plan.human_explanation, "from n8n");
    }

    #[test]
    fn unwrap_n8n_passthrough_object() {
        let body = json!({"plan": {"schemaVersion": 1, "humanExplanation": "direct", "confidence": 0.5, "actions": [], "suspectedMods": [], "needsUserReview": false}});
        let out = unwrap_n8n_diagnose_body(body.clone());
        assert_eq!(out, body);
    }
}
