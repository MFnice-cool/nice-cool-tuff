//! TuffSwarm local helpers: ExperienceCapsule, pending ActionPlan file, mod co-occurrence.
//!
//! Network transport stays in `crash_remote` / desktop integrations.
//! Gate: `swarm.enabled` — Creation Mode and network Fix Mode.

use crate::action_plan::{
    parse_action_plan, validate_action_plan, ActionPlan, LauncherAction, ACTION_PLAN_SCHEMA_VERSION,
};
use crate::crash_kb::{public_case_for_export, CrashCase, CrashFingerprint};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const PENDING_PLAN_FILENAME: &str = "pending_action_plan.json";
pub const STRONG_MATCH_THRESHOLD: f64 = 0.75;
/// DHT / gossip record prefix for content-addressed capsules.
pub const CAPSULE_CONTENT_PREFIX: &str = "tuffswarm/cap/v1/";
/// Max ExperienceCapsule gossip payload (bytes) — soft spam limit.
pub const MAX_CAPSULE_GOSSIP_BYTES: usize = 64 * 1024;

/// Built-in TuffSwarm Supabase project (community inbox). Publishable/anon key is
/// public by design (RLS + Edge Function); never ship the service role.
pub const BUILTIN_SUPABASE_URL: &str = "https://vsoqnwknpueuubiovyjd.supabase.co";
/// Publishable key for PostgREST (`rest/v1/*`). Safe to embed in the client.
pub const BUILTIN_SUPABASE_ANON_KEY: &str = "sb_publishable_b0ICBMz_HvyRa8GioadWcg_Co5Vjljr";
/// Legacy JWT anon (`role=anon`) for Edge Functions gateway (`functions/v1/*`).
/// Prefer over publishable `sb_publishable_…` — gateway accepts JWT reliably.
pub const BUILTIN_SUPABASE_EDGE_ANON_KEY: &str =
    "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6InZzb3Fud2tucHVldXViaW92eWpkIiwicm9sZSI6ImFub24iLCJpYXQiOjE3ODQ4MTEwMDYsImV4cCI6MjEwMDM4NzAwNn0.E9L11ipWyNiSchUx6pxT3HOVxu_vHtYDUOnNTixqJaI";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwarmSettings {
    /// Master switch for network Fix Mode + Creation Mode.
    #[serde(default)]
    pub enabled: bool,
    /// First-run wizard completed.
    #[serde(default)]
    pub onboarding_done: bool,
    /// Prompt to share capsule after successful relaunch.
    #[serde(default = "default_true")]
    pub share_prompts_enabled: bool,
    /// Optional override for Supabase URL. Empty → use [`BUILTIN_SUPABASE_URL`].
    /// Anon key: keyring `swarm_supabase` or [`BUILTIN_SUPABASE_ANON_KEY`].
    #[serde(default)]
    pub supabase_url: String,
    /// Shared TuffSwarm hub base URL (`http://host:8787`) for capsule publish/lookup.
    /// Falls back to AI Crash KB endpoint when empty. Optional when Supabase is set.
    #[serde(default)]
    pub hub_url: String,
    /// Spawn/attach Phase C `tuffswarm-node` and prefer its control HTTP when healthy.
    #[serde(default)]
    pub p2p_enabled: bool,
    /// Local control URL of tuffswarm-node (default http://127.0.0.1:8790).
    #[serde(default = "default_p2p_control_url")]
    pub p2p_control_url: String,
    /// Optional peer multiaddr for `--bootstrap` when mDNS fails (`/ip4/…/tcp/…/p2p/…`).
    #[serde(default)]
    pub p2p_bootstrap: String,
    /// Opt-in: accept Circuit Relay v2 reservations (`--relay-server`; public IP / VPS).
    #[serde(default)]
    pub p2p_relay_server: bool,
    /// Opt-in: accept Fog diagnose jobs (local Ollama) for other players.
    #[serde(default)]
    pub volunteer_diagnose: bool,
    /// Opt-in: accept Creation Marketplace jobs (scaffold / GPU later) for peers.
    #[serde(default)]
    pub creation_worker: bool,
    /// Stub advertised VRAM (MB) for Creation routing (`--vram-mb`). Not measured.
    #[serde(default)]
    pub advertised_vram_mb: u32,
}

fn default_true() -> bool {
    true
}

fn default_p2p_control_url() -> String {
    "http://127.0.0.1:8790".into()
}

impl Default for SwarmSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            onboarding_done: false,
            share_prompts_enabled: true,
            supabase_url: String::new(),
            hub_url: String::new(),
            p2p_enabled: false,
            p2p_control_url: default_p2p_control_url(),
            p2p_bootstrap: String::new(),
            p2p_relay_server: false,
            volunteer_diagnose: false,
            creation_worker: false,
            advertised_vram_mb: 0,
        }
    }
}

impl SwarmSettings {
    /// Effective Supabase URL (settings override, else built-in community project).
    pub fn effective_supabase_url(&self) -> Option<String> {
        let override_url = self.supabase_url.trim().trim_end_matches('/');
        if !override_url.is_empty() {
            return Some(override_url.to_string());
        }
        let builtin = BUILTIN_SUPABASE_URL.trim().trim_end_matches('/');
        if builtin.is_empty() {
            None
        } else {
            Some(builtin.to_string())
        }
    }

    pub fn supabase_configured(&self) -> bool {
        self.effective_supabase_url().is_some() && !BUILTIN_SUPABASE_ANON_KEY.trim().is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperienceCapsule {
    pub schema_version: u32,
    pub id: String,
    pub fingerprint: CrashFingerprint,
    pub solution: String,
    #[serde(default)]
    pub actions: Vec<LauncherAction>,
    #[serde(default)]
    pub success_score: f64,
    #[serde(default)]
    pub success_count: u32,
    #[serde(default)]
    pub fail_count: u32,
    #[serde(default)]
    pub adapter_ref: Option<String>,
    #[serde(default)]
    pub kb_version: Option<String>,
    #[serde(default)]
    pub privacy: CapsulePrivacy,
    /// SHA-256 hex of canonical (fingerprint.key + solution + actions). Used for DHT dedupe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// libp2p / node peer id that signed this capsule (soft verify).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer_peer_id: Option<String>,
    /// Ed25519 public key (base64) used to verify `signature`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer_public_key: Option<String>,
    /// Ed25519 signature over `content_hash` utf-8 bytes (base64).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapsulePrivacy {
    pub raw_logs: bool,
    pub notes_included: bool,
}

impl Default for CapsulePrivacy {
    fn default() -> Self {
        Self {
            raw_logs: false,
            notes_included: false,
        }
    }
}

impl ExperienceCapsule {
    pub fn from_crash_case(case: &CrashCase) -> Self {
        let actions = if !case.launcher_actions.is_empty() {
            case.launcher_actions.clone()
        } else {
            case.actions
                .iter()
                .map(|a| LauncherAction {
                    op: match a.action_type.to_ascii_lowercase().as_str() {
                        "update" | "update_mod" => "update_mod".into(),
                        "remove" | "remove_mod" => "remove_mod".into(),
                        "install" | "install_mod" => "install_mod".into(),
                        "disable" | "disable_mod" => "disable_mod".into(),
                        "config_change" | "edit_config" => "edit_config".into(),
                        other => other.to_string(),
                    },
                    mod_id: a.mod_id.clone(),
                    provider: None,
                    project_id: None,
                    version: None,
                    path: None,
                    patch_type: None,
                    patch: None,
                    reason: Some(a.description.clone()),
                    risk: a.risk.clone(),
                })
                .collect()
        };
        let total = case.success_count.saturating_add(case.fail_count).max(1);
        let success_score = case.success_count as f64 / total as f64;
        let mut capsule = Self {
            schema_version: 1,
            id: case.id.clone(),
            fingerprint: case.fingerprint.clone(),
            solution: case.solution.clone(),
            actions,
            success_score,
            success_count: case.success_count,
            fail_count: case.fail_count,
            adapter_ref: None,
            kb_version: Some(crate::time_util::rfc3339_now()),
            privacy: CapsulePrivacy::default(),
            content_hash: None,
            signer_peer_id: None,
            signer_public_key: None,
            signature: None,
        };
        capsule.ensure_content_hash();
        capsule
    }

    /// Canonical bytes for content addressing (no volatile id/timestamps/signatures).
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let actions_json = serde_json::to_string(&self.actions).unwrap_or_else(|_| "[]".into());
        let mut out = Vec::new();
        out.extend_from_slice(self.fingerprint.key.as_bytes());
        out.push(b'\n');
        out.extend_from_slice(self.solution.as_bytes());
        out.push(b'\n');
        out.extend_from_slice(actions_json.as_bytes());
        out
    }

    pub fn compute_content_hash(&self) -> String {
        let digest = Sha256::digest(self.canonical_bytes());
        hex::encode(digest)
    }

    /// Fill `content_hash` and align `id` to `cap-{hash_prefix}` when empty or legacy.
    pub fn ensure_content_hash(&mut self) {
        let hash = self.compute_content_hash();
        self.content_hash = Some(hash.clone());
        if self.id.trim().is_empty()
            || self.id.starts_with("capsule-")
            || self.id.starts_with("remote-")
        {
            self.id = format!("cap-{}", &hash[..hash.len().min(32)]);
        }
    }

    pub fn dht_content_key(&self) -> String {
        let hash = self
            .content_hash
            .clone()
            .unwrap_or_else(|| self.compute_content_hash());
        format!("{CAPSULE_CONTENT_PREFIX}{hash}")
    }

    /// Sign content_hash with an Ed25519 signing key; stores base64 pubkey + signature.
    pub fn sign_ed25519(
        &mut self,
        signing_key: &SigningKey,
        signer_peer_id: impl Into<String>,
    ) -> Result<(), String> {
        self.ensure_content_hash();
        let hash = self
            .content_hash
            .as_ref()
            .ok_or_else(|| "content_hash missing".to_string())?;
        let sig = signing_key.sign(hash.as_bytes());
        self.signer_peer_id = Some(signer_peer_id.into());
        self.signer_public_key = Some(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            signing_key.verifying_key().as_bytes(),
        ));
        self.signature = Some(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            sig.to_bytes(),
        ));
        Ok(())
    }

    /// Verify soft signature. Returns Ok(true) if valid, Ok(false) if unsigned, Err if invalid.
    pub fn verify_signature(&self) -> Result<bool, String> {
        let (Some(pk_b64), Some(sig_b64), Some(hash)) = (
            self.signer_public_key.as_ref(),
            self.signature.as_ref(),
            self.content_hash.as_ref(),
        ) else {
            return Ok(false);
        };
        let expected = self.compute_content_hash();
        if hash != &expected {
            return Err("content_hash does not match canonical payload".into());
        }
        let pk_bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, pk_b64)
            .map_err(|e| format!("bad signerPublicKey: {e}"))?;
        let pk_arr: [u8; 32] = pk_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "signerPublicKey must be 32 bytes".to_string())?;
        let verifying =
            VerifyingKey::from_bytes(&pk_arr).map_err(|e| format!("invalid verifying key: {e}"))?;
        let sig_bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, sig_b64)
            .map_err(|e| format!("bad signature: {e}"))?;
        let sig_arr: [u8; 64] = sig_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "signature must be 64 bytes".to_string())?;
        let signature = Signature::from_bytes(&sig_arr);
        verifying
            .verify(hash.as_bytes(), &signature)
            .map_err(|e| format!("signature verify failed: {e}"))?;
        Ok(true)
    }

    /// P2P ingest policy: require a valid signature.
    pub fn accept_for_p2p_gossip(&self) -> Result<(), String> {
        match self.verify_signature()? {
            true => Ok(()),
            false => Err("unsigned capsule rejected by P2P policy".into()),
        }
    }

    pub fn generate_signing_key() -> SigningKey {
        let mut rng = rand::rngs::OsRng;
        SigningKey::generate(&mut rng)
    }

    pub fn to_action_plan(&self) -> ActionPlan {
        ActionPlan {
            schema_version: ACTION_PLAN_SCHEMA_VERSION,
            human_explanation: self.solution.clone(),
            confidence: self.success_score.clamp(0.0, 1.0),
            suspected_mods: Vec::new(),
            needs_user_review: true,
            source: Some("swarm".into()),
            matched_case_ids: vec![self.id.clone()],
            actions: self.actions.clone(),
            additional_context: None,
        }
    }

    pub fn to_public_json(&self) -> Value {
        let case = CrashCase {
            id: self.id.clone(),
            fingerprint: self.fingerprint.clone(),
            symptoms: Vec::new(),
            suspected_mods: Vec::new(),
            solution: self.solution.clone(),
            actions: Vec::new(),
            launcher_actions: self.actions.clone(),
            notes: None,
            source: "authored".into(),
            success_count: self.success_count,
            fail_count: self.fail_count,
        };
        let mut v = public_case_for_export(&case);
        if let Some(obj) = v.as_object_mut() {
            obj.insert("schemaVersion".into(), json_num(self.schema_version));
            obj.insert("successScore".into(), json_f64(self.success_score));
            // MUST: never advertise raw logs / notes in shared capsules.
            let privacy = CapsulePrivacy {
                raw_logs: false,
                notes_included: false,
            };
            obj.insert(
                "privacy".into(),
                serde_json::to_value(&privacy).unwrap_or(Value::Null),
            );
            if let Some(ref kv) = self.kb_version {
                obj.insert("kbVersion".into(), Value::String(kv.clone()));
            }
            if let Some(ref h) = self.content_hash {
                obj.insert("contentHash".into(), Value::String(h.clone()));
            }
            if let Some(ref p) = self.signer_peer_id {
                obj.insert("signerPeerId".into(), Value::String(p.clone()));
            }
            if let Some(ref k) = self.signer_public_key {
                obj.insert("signerPublicKey".into(), Value::String(k.clone()));
            }
            if let Some(ref s) = self.signature {
                obj.insert("signature".into(), Value::String(s.clone()));
            }
        }
        v
    }

    pub fn to_crash_case(&self) -> CrashCase {
        CrashCase {
            id: self.id.clone(),
            fingerprint: self.fingerprint.clone(),
            symptoms: Vec::new(),
            suspected_mods: Vec::new(),
            solution: self.solution.clone(),
            actions: Vec::new(),
            launcher_actions: self.actions.clone(),
            notes: None,
            source: "swarm".into(),
            success_count: self.success_count,
            fail_count: self.fail_count,
        }
    }

    /// Parse a capsule from public JSON (hub / export). Strips notes and raw logs.
    pub fn from_public_value(value: &Value) -> Result<Self, String> {
        // Accept either ExperienceCapsule shape or CrashCase-like export.
        if let Ok(mut capsule) = serde_json::from_value::<ExperienceCapsule>(value.clone()) {
            capsule.privacy = CapsulePrivacy::default();
            if capsule.solution.trim().is_empty() {
                return Err("capsule solution is empty".into());
            }
            if capsule.fingerprint.key.trim().is_empty() {
                return Err("capsule fingerprint.key is empty".into());
            }
            capsule.ensure_content_hash();
            return Ok(capsule);
        }
        let case: CrashCase = serde_json::from_value(value.clone())
            .map_err(|e| format!("invalid capsule payload: {e}"))?;
        if case.solution.trim().is_empty() {
            return Err("capsule solution is empty".into());
        }
        Ok(Self::from_crash_case(&case))
    }

    pub fn sanitized_for_network(&self) -> Self {
        let mut c = self.clone();
        c.privacy = CapsulePrivacy::default();
        c.ensure_content_hash();
        c
    }
}

fn json_num(n: u32) -> Value {
    Value::Number(n.into())
}
fn json_f64(n: f64) -> Value {
    serde_json::Number::from_f64(n)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

// ── Durable shared capsule library (cross-project / hub) ──────────

/// Append-only JSONL library of ExperienceCapsules (solutions only — no raw logs).
#[derive(Debug, Clone)]
pub struct CapsuleLibrary {
    path: PathBuf,
}

impl CapsuleLibrary {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn ensure_parent(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn load_all(&self) -> Vec<ExperienceCapsule> {
        let Ok(raw) = fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut seen = HashMap::<String, usize>::new();
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let Ok(capsule) = ExperienceCapsule::from_public_value(&value) else {
                continue;
            };
            if let Some(&idx) = seen.get(&capsule.id) {
                out[idx] = capsule;
            } else {
                seen.insert(capsule.id.clone(), out.len());
                out.push(capsule);
            }
        }
        out
    }

    /// Upsert by content_hash (preferred) or id. Accumulates success_count on re-publish.
    pub fn publish(&self, capsule: &ExperienceCapsule) -> Result<ExperienceCapsule, String> {
        let mut capsule = capsule.sanitized_for_network();
        capsule.ensure_content_hash();
        if capsule.solution.trim().is_empty() {
            return Err("refusing to publish empty solution".into());
        }
        if capsule.fingerprint.key.trim().is_empty() {
            return Err("refusing to publish capsule without fingerprint.key".into());
        }
        self.ensure_parent()?;
        let mut all = self.load_all();
        let hash = capsule.content_hash.clone();
        let match_idx = all.iter().position(|c| {
            if let (Some(a), Some(b)) = (&c.content_hash, &hash) {
                a == b
            } else {
                c.id == capsule.id
            }
        });
        if let Some(idx) = match_idx {
            let existing = &mut all[idx];
            existing.success_count = existing
                .success_count
                .saturating_add(capsule.success_count.max(1));
            existing.fail_count = existing.fail_count.saturating_add(capsule.fail_count);
            let total = existing
                .success_count
                .saturating_add(existing.fail_count)
                .max(1);
            existing.success_score = existing.success_count as f64 / total as f64;
            if !capsule.solution.is_empty() {
                existing.solution = capsule.solution.clone();
            }
            if !capsule.actions.is_empty() {
                existing.actions = capsule.actions.clone();
            }
            existing.fingerprint = capsule.fingerprint.clone();
            existing.privacy = CapsulePrivacy::default();
            existing.content_hash = capsule.content_hash.clone();
            // Prefer a verified signature when re-publishing.
            if capsule.signature.is_some() {
                existing.signer_peer_id = capsule.signer_peer_id.clone();
                existing.signer_public_key = capsule.signer_public_key.clone();
                existing.signature = capsule.signature.clone();
            }
            existing.ensure_content_hash();
            let out = existing.clone();
            self.rewrite_all(&all)?;
            return Ok(out);
        }
        all.push(capsule.clone());
        self.rewrite_all(&all)?;
        Ok(capsule)
    }

    fn rewrite_all(&self, capsules: &[ExperienceCapsule]) -> Result<(), String> {
        self.ensure_parent()?;
        let mut body = String::new();
        for c in capsules {
            let line = serde_json::to_string(&c.sanitized_for_network().to_public_json())
                .map_err(|e| e.to_string())?;
            body.push_str(&line);
            body.push('\n');
        }
        let tmp = self.path.with_extension("jsonl.tmp");
        fs::write(&tmp, body).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.path)
            .or_else(|_| {
                fs::remove_file(&self.path).ok();
                fs::rename(&tmp, &self.path)
            })
            .map_err(|e| e.to_string())
    }

    pub fn lookup(
        &self,
        fingerprint: &CrashFingerprint,
        haystack: &str,
        limit: usize,
    ) -> Vec<crate::crash_remote::CrashLookupHit> {
        let capsules = self.load_all();
        let cases: Vec<CrashCase> = capsules.iter().map(|c| c.to_crash_case()).collect();
        let mut hits: Vec<_> =
            crate::crash_kb::search_similar(&cases, fingerprint, haystack, limit * 2)
                .into_iter()
                .map(|h| {
                    let capsule = capsules.iter().find(|c| c.id == h.id);
                    let actions = capsule
                        .map(|c| c.actions.clone())
                        .or_else(|| {
                            cases
                                .iter()
                                .find(|c| c.id == h.id)
                                .map(|c| c.launcher_actions.clone())
                        })
                        .unwrap_or_default();
                    // Soft ranking: boost by success_count; unsigned hub legacy slightly demoted.
                    let success_boost = capsule
                        .map(|c| (c.success_count as f64).ln_1p() * 0.05)
                        .unwrap_or(0.0);
                    let signed_boost = capsule
                        .and_then(|c| c.verify_signature().ok())
                        .map(|ok| if ok { 0.05 } else { -0.02 })
                        .unwrap_or(-0.02);
                    crate::crash_remote::CrashLookupHit {
                        id: h.id,
                        score: (h.score + success_boost + signed_boost).clamp(0.0, 1.0),
                        solution: h.solution,
                        suspected_mods: h.suspected_mods,
                        actions,
                        fingerprint_key: h.fingerprint_key,
                    }
                })
                .collect();
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit);
        hits
    }

    pub fn diagnose_best(
        &self,
        fingerprint: &CrashFingerprint,
        haystack: &str,
    ) -> Option<ActionPlan> {
        let hit = self.lookup(fingerprint, haystack, 1).into_iter().next()?;
        Some(crate::action_plan::plan_from_launcher_actions(
            &hit.solution,
            &hit.suspected_mods,
            hit.actions,
            &hit.id,
            hit.score,
        ))
    }
}

/// Resolve HTTP hub/KB base: prefer swarm.hubUrl, else Crash KB endpoint.
/// Supabase is a separate transport (`swarm_supabase`) — not returned here.
pub fn resolve_swarm_network_base(hub_url: &str, crash_kb_endpoint: &str) -> Option<String> {
    let hub = hub_url.trim();
    if !hub.is_empty() {
        return Some(hub.to_string());
    }
    let kb = crash_kb_endpoint.trim();
    if !kb.is_empty() {
        return Some(kb.to_string());
    }
    None
}

/// Directory for machine-wide swarm state (capsules, device signing key).
pub fn global_swarm_dir() -> PathBuf {
    dirs_next_config().join("TuffBox").join("swarm")
}

fn dirs_next_config() -> PathBuf {
    // Prefer the same root desktop uses via `dirs` crate when available;
    // fall back to relative `.` so unit tests still work offline.
    std::env::var_os("TUFFBOX_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            #[cfg(windows)]
            {
                std::env::var_os("APPDATA").map(PathBuf::from)
            }
            #[cfg(not(windows))]
            {
                std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
            }
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

fn device_signing_key_path() -> PathBuf {
    global_swarm_dir().join("device_signing_key")
}

/// Load or create a persistent Ed25519 device key for soft-signing capsules.
/// Returns `(signing_key, device_id)` where device_id is `tb-<hex pubkey prefix>`.
pub fn load_or_create_device_signing_key() -> Result<(SigningKey, String), String> {
    let path = device_signing_key_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if path.is_file() {
        let raw = fs::read(&path).map_err(|e| e.to_string())?;
        if raw.len() == 32 {
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(&raw);
            let sk = SigningKey::from_bytes(&bytes);
            let device_id = device_id_from_verifying_key(&sk.verifying_key());
            return Ok((sk, device_id));
        }
    }
    let sk = ExperienceCapsule::generate_signing_key();
    fs::write(&path, sk.to_bytes()).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    let device_id = device_id_from_verifying_key(&sk.verifying_key());
    Ok((sk, device_id))
}

fn device_id_from_verifying_key(vk: &VerifyingKey) -> String {
    let hex = hex::encode(vk.as_bytes());
    format!("tb-{}", &hex[..hex.len().min(16)])
}

/// Sign a capsule with the persistent device key (required for Supabase publish).
pub fn sign_capsule_with_device_key(capsule: &mut ExperienceCapsule) -> Result<String, String> {
    let (sk, device_id) = load_or_create_device_signing_key()?;
    capsule.sign_ed25519(&sk, &device_id)?;
    Ok(device_id)
}

/// Base64 (standard) Ed25519 public key for this device — matches capsule `signerPublicKey`.
pub fn device_signer_public_key_b64() -> Result<String, String> {
    let (sk, _) = load_or_create_device_signing_key()?;
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        sk.verifying_key().as_bytes(),
    ))
}

/// Canonical vote message — must match Edge Function `vote-capsule`.
pub fn capsule_vote_message(vote: &str, content_hash: &str) -> String {
    format!("tuffswarm-vote:v1:{vote}:{content_hash}")
}

/// Sign a confirm/reject vote. Returns (signer_public_key_b64, signature_b64, device_id).
pub fn sign_capsule_vote(
    vote: &str,
    content_hash: &str,
) -> Result<(String, String, String), String> {
    let vote = vote.trim().to_ascii_lowercase();
    if vote != "confirm" && vote != "reject" {
        return Err("vote must be confirm or reject".into());
    }
    let hash = content_hash.trim();
    if hash.is_empty() {
        return Err("content_hash is empty".into());
    }
    let (sk, device_id) = load_or_create_device_signing_key()?;
    let msg = capsule_vote_message(&vote, hash);
    let sig = sk.sign(msg.as_bytes());
    let pk_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        sk.verifying_key().as_bytes(),
    );
    let sig_b64 =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, sig.to_bytes());
    Ok((pk_b64, sig_b64, device_id))
}

pub fn swarm_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join("swarm")
}

pub fn pending_plan_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join(PENDING_PLAN_FILENAME)
}

pub fn write_pending_action_plan(project_dir: &Path, plan: &ActionPlan) -> Result<PathBuf, String> {
    let validation = validate_action_plan(plan);
    if !validation.ok {
        return Err(format!(
            "cannot write pending plan: {}",
            validation.errors.join("; ")
        ));
    }
    let path = pending_plan_path(project_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        &path,
        serde_json::to_vec_pretty(plan).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn load_pending_action_plan(project_dir: &Path) -> Result<Option<ActionPlan>, String> {
    let path = pending_plan_path(project_dir);
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(Some(parse_action_plan(&raw)?))
}

pub fn clear_pending_action_plan(project_dir: &Path) -> Result<(), String> {
    let path = pending_plan_path(project_dir);
    if path.is_file() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn maybe_write_pending_from_score(
    project_dir: &Path,
    plan: &ActionPlan,
    score: f64,
) -> Result<Option<PathBuf>, String> {
    if score < STRONG_MATCH_THRESHOLD {
        return Ok(None);
    }
    Ok(Some(write_pending_action_plan(project_dir, plan)?))
}

// ── Co-occurrence ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CooccurrenceStore {
    #[serde(default)]
    pub pairs: HashMap<String, u64>,
    #[serde(default)]
    pub mc_version: String,
    #[serde(default)]
    pub loader: String,
}

/// Last pack-composition observation (basket) — avoids re-counting the same build.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PackObservationMarker {
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default)]
    pub network_uploaded: bool,
    #[serde(default)]
    pub at: String,
}

#[derive(Debug, Clone)]
pub struct PackObservationPlan {
    pub ids: Vec<String>,
    pub network_ids: Vec<String>,
    pub fingerprint: String,
    pub record_local: bool,
    pub upload_network: bool,
}

fn pair_key(a: &str, b: &str) -> String {
    let (x, y) = if a <= b { (a, b) } else { (b, a) };
    format!("{x}||{y}")
}

pub fn cooccurrence_path(project_dir: &Path) -> PathBuf {
    swarm_dir(project_dir).join("cooccurrence.json")
}

fn pack_observation_path(project_dir: &Path) -> PathBuf {
    swarm_dir(project_dir).join("cooccurrence_last_pack.json")
}

pub fn load_cooccurrence(project_dir: &Path) -> CooccurrenceStore {
    let path = cooccurrence_path(project_dir);
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_cooccurrence(project_dir: &Path, store: &CooccurrenceStore) -> Result<(), String> {
    let path = cooccurrence_path(project_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        path,
        serde_json::to_vec_pretty(store).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Modrinth-style ids for pack baskets: jar mods only (skip resource/shader packs).
pub fn pack_mod_ids(manifest: &crate::manifest::ProjectManifest) -> Vec<String> {
    let ids: Vec<String> = manifest
        .mods
        .iter()
        .filter(|m| matches!(m.content_type, crate::manifest::ContentType::Mod))
        .map(|m| {
            // Prefer human slug (`id`); fall back to Modrinth project_id.
            if !m.id.trim().is_empty() {
                m.id.clone()
            } else {
                m.source.project_id.clone().unwrap_or_default()
            }
        })
        .collect();
    normalize_mod_id_list(&ids, 512)
}

/// Stable fingerprint of a full pack composition (all ids + mc + loader).
pub fn pack_set_fingerprint(ids: &[String], mc_version: &str, loader: &str) -> String {
    let mut h = Sha256::new();
    h.update(mc_version.trim().as_bytes());
    h.update(b"|");
    h.update(loader.trim().to_ascii_lowercase().as_bytes());
    h.update(b"|");
    for id in ids {
        h.update(id.as_bytes());
        h.update(b"\n");
    }
    format!("{:x}", h.finalize())
}

/// Cap network upload size with a stable subset (not alphabetical head-cut).
pub fn normalize_mod_id_list_for_network(mod_ids: &[String], max: usize) -> Vec<String> {
    let mut ids = normalize_mod_id_list(mod_ids, 10_000);
    let max = max.max(2);
    if ids.len() <= max {
        return ids;
    }
    ids.sort_by_cached_key(|id| {
        let mut h = Sha256::new();
        h.update(id.as_bytes());
        h.finalize().to_vec()
    });
    ids.truncate(max);
    ids.sort();
    ids.dedup();
    ids
}

fn load_pack_observation_marker(project_dir: &Path) -> PackObservationMarker {
    fs::read_to_string(pack_observation_path(project_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_pack_observation_marker(
    project_dir: &Path,
    marker: &PackObservationMarker,
) -> Result<(), String> {
    let path = pack_observation_path(project_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        path,
        serde_json::to_vec_pretty(marker).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Decide whether this pack composition is a new basket observation.
/// Same fingerprint → skip local re-count; still upload if prior network upload failed.
pub fn plan_pack_observation(
    project_dir: &Path,
    ids: &[String],
    mc_version: &str,
    loader: &str,
) -> PackObservationPlan {
    let ids = normalize_mod_id_list(ids, 512);
    let network_ids = normalize_mod_id_list_for_network(&ids, 48);
    let fingerprint = pack_set_fingerprint(&ids, mc_version, loader);
    let marker = load_pack_observation_marker(project_dir);
    let same = marker.fingerprint == fingerprint && !fingerprint.is_empty();
    PackObservationPlan {
        ids,
        network_ids,
        fingerprint,
        record_local: !same,
        upload_network: !same || !marker.network_uploaded,
    }
}

pub fn mark_pack_observation(
    project_dir: &Path,
    fingerprint: &str,
    network_uploaded: bool,
) -> Result<(), String> {
    let prev = load_pack_observation_marker(project_dir);
    let marker = PackObservationMarker {
        fingerprint: fingerprint.to_string(),
        network_uploaded: network_uploaded
            || (prev.fingerprint == fingerprint && prev.network_uploaded),
        at: crate::time_util::rfc3339_now(),
    };
    save_pack_observation_marker(project_dir, &marker)
}

/// Record all unordered pairs among installed mod ids.
pub fn record_mod_set_cooccurrence(
    project_dir: &Path,
    mod_ids: &[String],
    mc_version: &str,
    loader: &str,
) -> Result<(), String> {
    let mut store = load_cooccurrence(project_dir);
    store.mc_version = mc_version.to_string();
    store.loader = loader.to_string();
    let ids = normalize_mod_id_list(mod_ids, 256);
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            let key = pair_key(&ids[i], &ids[j]);
            *store.pairs.entry(key).or_insert(0) += 1;
        }
    }
    save_cooccurrence(project_dir, &store)
}

/// Cap + normalize mod ids for network upload (keeps stable order).
pub fn normalize_mod_id_list(mod_ids: &[String], max: usize) -> Vec<String> {
    let mut ids: Vec<String> = mod_ids
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty() && s.len() <= 64)
        .collect();
    ids.sort();
    ids.dedup();
    ids.truncate(max.max(2));
    ids
}

/// Merge local + network pair lists (sum counts), keep top `limit`.
pub fn merge_cooccurrence_pairs(
    local: &[ModPairStat],
    network: &[ModPairStat],
    limit: usize,
) -> Vec<ModPairStat> {
    use std::collections::HashMap;
    let mut map: HashMap<(String, String), u64> = HashMap::new();
    for p in local.iter().chain(network.iter()) {
        let a = p.mod_a.trim().to_ascii_lowercase();
        let b = p.mod_b.trim().to_ascii_lowercase();
        if a.is_empty() || b.is_empty() || a == b {
            continue;
        }
        let (x, y) = if a <= b { (a, b) } else { (b, a) };
        *map.entry((x, y)).or_insert(0) += p.count.max(1);
    }
    let mut pairs: Vec<ModPairStat> = map
        .into_iter()
        .map(|((mod_a, mod_b), count)| ModPairStat {
            mod_a,
            mod_b,
            count,
        })
        .collect();
    pairs.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.mod_a.cmp(&b.mod_a)));
    pairs.truncate(limit);
    pairs
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModPairStat {
    pub mod_a: String,
    pub mod_b: String,
    pub count: u64,
}

/// Frequent mod group (3+) derived from pair graph — for install suggestions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModGroupStat {
    pub mods: Vec<String>,
    /// Weakest edge in the clique (support floor).
    pub score: u64,
}

pub fn top_cooccurrence_pairs(project_dir: &Path, limit: usize) -> Vec<ModPairStat> {
    let store = load_cooccurrence(project_dir);
    let mut pairs: Vec<ModPairStat> = store
        .pairs
        .iter()
        .filter_map(|(k, &count)| {
            let mut parts = k.splitn(2, "||");
            let a = parts.next()?.to_string();
            let b = parts.next()?.to_string();
            Some(ModPairStat {
                mod_a: a,
                mod_b: b,
                count,
            })
        })
        .collect();
    pairs.sort_by(|a, b| b.count.cmp(&a.count));
    pairs.truncate(limit);
    pairs
}

fn pair_adjacency(pairs: &[ModPairStat]) -> HashMap<String, HashMap<String, u64>> {
    let mut adj: HashMap<String, HashMap<String, u64>> = HashMap::new();
    for p in pairs {
        let a = p.mod_a.trim().to_ascii_lowercase();
        let b = p.mod_b.trim().to_ascii_lowercase();
        if a.is_empty() || b.is_empty() || a == b {
            continue;
        }
        *adj.entry(a.clone())
            .or_default()
            .entry(b.clone())
            .or_insert(0) += p.count.max(1);
        *adj.entry(b).or_default().entry(a).or_insert(0) += p.count.max(1);
    }
    adj
}

/// Rank partners that frequently co-occur with `seed` (pairs + groups containing seed).
pub fn partners_for_mod(
    pairs: &[ModPairStat],
    seed: &str,
    exclude: &std::collections::HashSet<String>,
    limit: usize,
) -> Vec<(String, u64)> {
    let seed = seed.trim().to_ascii_lowercase();
    if seed.is_empty() || limit == 0 {
        return Vec::new();
    }
    let exclude: std::collections::HashSet<String> = exclude
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let mut scores: HashMap<String, u64> = HashMap::new();
    for p in pairs {
        let a = p.mod_a.trim().to_ascii_lowercase();
        let b = p.mod_b.trim().to_ascii_lowercase();
        let c = p.count.max(1);
        if a == seed && !exclude.contains(&b) && b != seed {
            *scores.entry(b).or_insert(0) += c;
        } else if b == seed && !exclude.contains(&a) && a != seed {
            *scores.entry(a).or_insert(0) += c;
        }
    }
    for g in top_cooccurrence_groups(pairs, 48) {
        if !g.mods.iter().any(|m| m == &seed) {
            continue;
        }
        for m in &g.mods {
            if m == &seed || exclude.contains(m) {
                continue;
            }
            *scores.entry(m.clone()).or_insert(0) += g.score.max(1);
        }
    }
    let mut ranked: Vec<(String, u64)> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(limit);
    ranked
}

/// Suggest mods that co-occur with **multiple** already-installed mods (group affinity).
/// Prefers candidates linked to ≥2 installed mods; falls back to ≥1 if needed.
pub fn suggest_by_group_affinity(
    pairs: &[ModPairStat],
    installed: &std::collections::HashSet<String>,
    limit: usize,
) -> Vec<String> {
    let installed: std::collections::HashSet<String> = installed
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if installed.is_empty() || limit == 0 {
        return Vec::new();
    }
    let adj = pair_adjacency(pairs);
    let mut scored: Vec<(String, u32, u64)> = Vec::new();
    for (cand, neighbors) in &adj {
        if installed.contains(cand) {
            continue;
        }
        let mut support = 0u32;
        let mut weight = 0u64;
        for id in &installed {
            if let Some(&w) = neighbors.get(id) {
                support += 1;
                weight += w;
            }
        }
        if support == 0 {
            continue;
        }
        scored.push((cand.clone(), support, weight));
    }
    scored.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.0.cmp(&b.0))
    });
    let multi: Vec<(String, u32, u64)> = scored
        .iter()
        .filter(|(_, support, _)| *support >= 2)
        .cloned()
        .collect();
    let mut out: Vec<String> = multi
        .iter()
        .map(|(id, _, _)| id.clone())
        .take(limit)
        .collect();
    if out.len() >= limit {
        return out;
    }
    for (id, support, _) in scored {
        if support >= 2 {
            continue;
        }
        if out.iter().any(|x| x == &id) {
            continue;
        }
        out.push(id);
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// Top triangles (groups of 3) from the pair graph — frequent “used together” clusters.
pub fn top_cooccurrence_groups(pairs: &[ModPairStat], limit: usize) -> Vec<ModGroupStat> {
    if limit == 0 {
        return Vec::new();
    }
    let mut ranked = pairs.to_vec();
    ranked.sort_by(|a, b| b.count.cmp(&a.count));
    ranked.truncate(200);
    let adj = pair_adjacency(&ranked);
    let mut nodes: Vec<String> = adj.keys().cloned().collect();
    nodes.sort();
    let mut groups: Vec<ModGroupStat> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for a in &nodes {
        let Some(na) = adj.get(a) else { continue };
        let mut partners: Vec<&String> = na.keys().collect();
        partners.sort();
        for (j, b) in partners.iter().enumerate() {
            if *b <= a {
                continue;
            }
            let ab = *na.get(*b).unwrap_or(&0);
            if ab == 0 {
                continue;
            }
            let Some(nb) = adj.get(*b) else { continue };
            for c in partners.iter().skip(j + 1) {
                if *c <= *b {
                    continue;
                }
                let ac = *na.get(*c).unwrap_or(&0);
                let bc = *nb.get(*c).unwrap_or(&0);
                if ac == 0 || bc == 0 {
                    continue;
                }
                let score = ab.min(ac).min(bc);
                let mut mods = vec![a.clone(), (*b).clone(), (*c).clone()];
                mods.sort();
                let key = mods.join("||");
                if !seen.insert(key) {
                    continue;
                }
                groups.push(ModGroupStat { mods, score });
            }
        }
    }
    groups.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.mods[0].cmp(&b.mods[0]))
    });
    groups.truncate(limit);
    groups
}

/// Build a short prompt hint for Creation mode from local (and optional network) pairs.
pub fn format_cooccurrence_for_prompt(pairs: &[ModPairStat], limit: usize) -> String {
    let mut out = String::from("## Mod co-occurrence trends (most frequent pairs)\n");
    if pairs.is_empty() {
        out.push_str("- (no stats yet — install confirmed packs to grow this signal)\n");
        return out;
    }
    for (i, p) in pairs.iter().take(limit).enumerate() {
        out.push_str(&format!(
            "{}. `{}` + `{}` (count {})\n",
            i + 1,
            p.mod_a,
            p.mod_b,
            p.count
        ));
    }
    let groups = top_cooccurrence_groups(pairs, limit.min(8));
    if !groups.is_empty() {
        out.push_str("\n## Frequent mod groups (3+)\n");
        for (i, g) in groups.iter().enumerate() {
            out.push_str(&format!(
                "{}. `{}` (support {})\n",
                i + 1,
                g.mods.join("` + `"),
                g.score
            ));
        }
    }
    out.push_str(
        "\nWhen planning PackBrief mustHave/categories, prefer mods and groups that fit these trends for the target MC/loader. Do not invent Modrinth IDs — use search queries.\n",
    );
    out
}

#[cfg(test)]
mod cooccurrence_tests {
    use super::*;

    #[test]
    fn merge_sums_counts() {
        let local = vec![ModPairStat {
            mod_a: "sodium".into(),
            mod_b: "iris".into(),
            count: 2,
        }];
        let network = vec![
            ModPairStat {
                mod_a: "iris".into(),
                mod_b: "sodium".into(),
                count: 5,
            },
            ModPairStat {
                mod_a: "create".into(),
                mod_b: "jei".into(),
                count: 9,
            },
        ];
        let merged = merge_cooccurrence_pairs(&local, &network, 10);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].mod_a, "create");
        assert_eq!(merged[0].count, 9);
        assert_eq!(merged[1].count, 7);
    }

    #[test]
    fn pack_observation_dedupes_same_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let ids = vec!["jei".into(), "sodium".into(), "iris".into()];
        let p1 = plan_pack_observation(dir.path(), &ids, "1.20.1", "fabric");
        assert!(p1.record_local);
        assert!(p1.upload_network);
        // Local recorded, network failed → retry upload only
        mark_pack_observation(dir.path(), &p1.fingerprint, false).unwrap();
        let p2 = plan_pack_observation(dir.path(), &ids, "1.20.1", "fabric");
        assert!(!p2.record_local);
        assert!(p2.upload_network);
        mark_pack_observation(dir.path(), &p1.fingerprint, true).unwrap();
        let p3 = plan_pack_observation(dir.path(), &ids, "1.20.1", "fabric");
        assert!(!p3.record_local);
        assert!(!p3.upload_network);
    }

    #[test]
    fn network_sample_is_stable_subset() {
        let ids: Vec<String> = (0..80).map(|i| format!("mod-{i:02}")).collect();
        let a = normalize_mod_id_list_for_network(&ids, 48);
        let b = normalize_mod_id_list_for_network(&ids, 48);
        assert_eq!(a.len(), 48);
        assert_eq!(a, b);
    }

    #[test]
    fn group_affinity_prefers_multi_support() {
        let pairs = vec![
            ModPairStat {
                mod_a: "create".into(),
                mod_b: "jei".into(),
                count: 10,
            },
            ModPairStat {
                mod_a: "create".into(),
                mod_b: "sodium".into(),
                count: 8,
            },
            ModPairStat {
                mod_a: "jei".into(),
                mod_b: "sodium".into(),
                count: 7,
            },
            ModPairStat {
                mod_a: "create".into(),
                mod_b: "lonely".into(),
                count: 100,
            },
        ];
        let installed: std::collections::HashSet<String> =
            ["create".into(), "jei".into()].into_iter().collect();
        let suggestions = suggest_by_group_affinity(&pairs, &installed, 5);
        assert_eq!(suggestions.first().map(String::as_str), Some("sodium"));
        let groups = top_cooccurrence_groups(&pairs, 5);
        assert!(groups.iter().any(|g| g.mods == ["create", "jei", "sodium"]));
        let exclude: std::collections::HashSet<String> = ["create".into()].into_iter().collect();
        let partners = partners_for_mod(&pairs, "create", &exclude, 5);
        assert!(partners.iter().any(|(id, _)| id == "jei" || id == "sodium"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn pending_plan_roundtrip() {
        let dir = tempdir().unwrap();
        let plan = ActionPlan {
            schema_version: 1,
            human_explanation: "Install indium".into(),
            confidence: 0.9,
            suspected_mods: vec!["sodium".into()],
            needs_user_review: true,
            source: Some("swarm".into()),
            matched_case_ids: vec!["c1".into()],
            actions: vec![LauncherAction {
                op: "install_mod".into(),
                mod_id: Some("indium".into()),
                provider: None,
                project_id: None,
                version: None,
                path: None,
                patch_type: None,
                patch: None,
                reason: Some("missing dep".into()),
                risk: "low".into(),
            }],
            additional_context: None,
        };
        write_pending_action_plan(dir.path(), &plan).unwrap();
        let loaded = load_pending_action_plan(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.actions[0].mod_id.as_deref(), Some("indium"));
        clear_pending_action_plan(dir.path()).unwrap();
        assert!(load_pending_action_plan(dir.path()).unwrap().is_none());
    }

    #[test]
    fn capsule_library_persists_and_looks_up() {
        let dir = tempdir().unwrap();
        let lib = CapsuleLibrary::open(dir.path().join("capsules.jsonl"));
        let case = CrashCase {
            id: "c-share".into(),
            fingerprint: CrashFingerprint {
                exception: "MixinTransformerError".into(),
                frames: vec!["create.flywheel".into()],
                mod_file: None,
                mixin: Some("create".into()),
                mc_major: "1.20".into(),
                loader: "fabric".into(),
                key: "mixin|create|fabric".into(),
                blame_mod_ids: vec!["create".into()],
            },
            symptoms: vec![],
            suspected_mods: vec!["create".into()],
            solution: "Align Create + Flywheel".into(),
            actions: vec![],
            launcher_actions: vec![LauncherAction {
                op: "update_mod".into(),
                mod_id: Some("create".into()),
                provider: None,
                project_id: None,
                version: None,
                path: None,
                patch_type: None,
                patch: None,
                reason: Some("version mismatch".into()),
                risk: "medium".into(),
            }],
            notes: Some("secret notes must not leak".into()),
            source: "authored".into(),
            success_count: 1,
            fail_count: 0,
        };
        let published = lib
            .publish(&ExperienceCapsule::from_crash_case(&case))
            .unwrap();
        assert!(!published.privacy.raw_logs);
        let hits = lib.lookup(&case.fingerprint, "mixin create flywheel", 5);
        assert!(!hits.is_empty());
        assert!(hits[0].solution.contains("Align Create"));
        let public = published.to_public_json();
        assert!(public.get("notes").is_none());
    }

    #[test]
    fn capsule_content_hash_and_signature_roundtrip() {
        let case = CrashCase {
            id: "c-sign".into(),
            fingerprint: CrashFingerprint {
                exception: "MixinTransformerError".into(),
                frames: vec!["create.flywheel".into()],
                mod_file: None,
                mixin: None,
                mc_major: "1.20".into(),
                loader: "fabric".into(),
                key: "mixin|create|fabric".into(),
                blame_mod_ids: Vec::new(),
            },
            symptoms: vec![],
            suspected_mods: vec![],
            solution: "Align Create".into(),
            actions: vec![],
            launcher_actions: vec![],
            notes: None,
            source: "authored".into(),
            success_count: 1,
            fail_count: 0,
        };
        let mut capsule = ExperienceCapsule::from_crash_case(&case);
        assert!(capsule.content_hash.is_some());
        let sk = ExperienceCapsule::generate_signing_key();
        capsule.sign_ed25519(&sk, "12D3KooWtest").unwrap();
        assert_eq!(capsule.verify_signature().unwrap(), true);
        capsule.accept_for_p2p_gossip().unwrap();
    }

    #[test]
    fn device_signing_key_persists() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("TUFFBOX_CONFIG_DIR", dir.path());
        let (sk1, id1) = load_or_create_device_signing_key().unwrap();
        let (sk2, id2) = load_or_create_device_signing_key().unwrap();
        assert_eq!(id1, id2);
        assert_eq!(sk1.to_bytes(), sk2.to_bytes());
        assert!(id1.starts_with("tb-"));
        std::env::remove_var("TUFFBOX_CONFIG_DIR");
    }
}
