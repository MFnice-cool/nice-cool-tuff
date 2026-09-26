//! Creation Marketplace contracts (Mode 2) — shared by tuffswarm-node + desktop verifier.
//!
//! Transport reuses Fog-style libp2p request-response; this module is the job/result
//! schema and local hard-verify (syntax / path safety). No Fog diagnose types here.

use serde::{Deserialize, Serialize};

pub const CREATION_PROTOCOL: &str = "/tuffswarm/creation/1.0.0";
pub const MAX_CREATION_JOB_BYTES: usize = 48 * 1024;
pub const MAX_CREATION_RESULT_BYTES: usize = 256 * 1024;
pub const MAX_ARTIFACT_BYTES: usize = 96 * 1024;
pub const MAX_ARTIFACTS: usize = 12;
pub const DEFAULT_CREATION_DEADLINE_MS: u64 = 120_000;

pub const KNOWN_CREATION_KINDS: &[&str] = &[
    "kubejs_ore_gen",
    "quest_scripts",
    "recipe_balance",
    "mod_configs",
    "full_pack_scaffold",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreationConstraints {
    #[serde(default)]
    pub mc_version: String,
    #[serde(default)]
    pub loader: String,
    #[serde(default)]
    pub mod_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreationReward {
    #[serde(default = "default_reward_kind")]
    pub kind: String,
    #[serde(default = "default_reward_amount")]
    pub amount: u32,
}

fn default_reward_kind() -> String {
    "kudos".into()
}

fn default_reward_amount() -> u32 {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreationVerifySpec {
    #[serde(default = "default_true")]
    pub syntax: bool,
    #[serde(default)]
    pub test_launch: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreationJob {
    #[serde(default = "schema_v1")]
    pub schema_version: u32,
    pub job_id: String,
    pub kind: String,
    #[serde(default)]
    pub constraints: CreationConstraints,
    pub brief: String,
    #[serde(default)]
    pub reward: CreationReward,
    #[serde(default)]
    pub verify: CreationVerifySpec,
    #[serde(default = "default_deadline")]
    pub deadline_ms: u64,
}

fn schema_v1() -> u32 {
    1
}

fn default_deadline() -> u64 {
    DEFAULT_CREATION_DEADLINE_MS
}

impl Default for CreationConstraints {
    fn default() -> Self {
        Self {
            mc_version: String::new(),
            loader: String::new(),
            mod_ids: Vec::new(),
        }
    }
}

impl Default for CreationReward {
    fn default() -> Self {
        Self {
            kind: default_reward_kind(),
            amount: default_reward_amount(),
        }
    }
}

impl Default for CreationVerifySpec {
    fn default() -> Self {
        Self {
            syntax: true,
            test_launch: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreationArtifact {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreationResult {
    #[serde(default = "schema_v1")]
    pub schema_version: u32,
    pub job_id: String,
    #[serde(default)]
    pub worker_peer_id: Option<String>,
    /// Device Ed25519 public key (base64) of the worker — beneficiary for Accept→Kudos.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_signer_public_key: Option<String>,
    pub ok: bool,
    #[serde(default)]
    pub artifacts: Vec<CreationArtifact>,
    #[serde(default)]
    pub claimed_confidence: f64,
    #[serde(default)]
    pub error: Option<String>,
}

impl CreationResult {
    pub fn err(job_id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            job_id: job_id.into(),
            worker_peer_id: None,
            worker_signer_public_key: None,
            ok: false,
            artifacts: Vec::new(),
            claimed_confidence: 0.0,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationCheck {
    pub name: String,
    pub ok: bool,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReport {
    pub job_id: String,
    pub passed: bool,
    pub checks: Vec<VerificationCheck>,
    #[serde(default)]
    pub reward_granted: bool,
}

impl CreationJob {
    pub fn validate(&self) -> Result<(), String> {
        if self.job_id.trim().is_empty() {
            return Err("jobId required".into());
        }
        if !KNOWN_CREATION_KINDS.contains(&self.kind.as_str()) {
            return Err(format!(
                "unknown kind `{}` (expected one of {})",
                self.kind,
                KNOWN_CREATION_KINDS.join(", ")
            ));
        }
        let brief = self.brief.trim();
        if brief.len() < 8 {
            return Err("brief too short".into());
        }
        if brief.len() > 4_000 {
            return Err("brief too long".into());
        }
        let n = serde_json::to_vec(self)
            .map(|b| b.len())
            .map_err(|e| e.to_string())?;
        if n > MAX_CREATION_JOB_BYTES {
            return Err(format!(
                "creation job exceeds max size ({n} > {MAX_CREATION_JOB_BYTES} bytes)"
            ));
        }
        Ok(())
    }
}

/// Hard-verify worker artifacts on the customer launcher (syntax + path safety).
/// Does **not** grant Kudos — reward stays behind soft-verify / explicit accept later.
pub fn verify_creation_result(job: &CreationJob, result: &CreationResult) -> VerificationReport {
    let mut checks = Vec::new();
    if !result.ok {
        checks.push(VerificationCheck {
            name: "worker_ok".into(),
            ok: false,
            detail: result
                .error
                .clone()
                .unwrap_or_else(|| "worker returned ok:false".into()),
        });
        return VerificationReport {
            job_id: job.job_id.clone(),
            passed: false,
            checks,
            reward_granted: false,
        };
    }

    if result.job_id != job.job_id {
        checks.push(VerificationCheck {
            name: "job_id".into(),
            ok: false,
            detail: "result jobId mismatch".into(),
        });
    } else {
        checks.push(VerificationCheck {
            name: "job_id".into(),
            ok: true,
            detail: String::new(),
        });
    }

    if result.artifacts.is_empty() {
        checks.push(VerificationCheck {
            name: "artifacts".into(),
            ok: false,
            detail: "no artifacts".into(),
        });
    } else if result.artifacts.len() > MAX_ARTIFACTS {
        checks.push(VerificationCheck {
            name: "artifacts".into(),
            ok: false,
            detail: format!("too many artifacts ({})", result.artifacts.len()),
        });
    } else {
        checks.push(VerificationCheck {
            name: "artifacts".into(),
            ok: true,
            detail: format!("{} file(s)", result.artifacts.len()),
        });
    }

    if job.verify.syntax {
        for (i, art) in result.artifacts.iter().enumerate() {
            let name = format!("syntax[{i}]");
            match validate_artifact(art) {
                Ok(()) => checks.push(VerificationCheck {
                    name,
                    ok: true,
                    detail: art.path.clone(),
                }),
                Err(detail) => checks.push(VerificationCheck {
                    name,
                    ok: false,
                    detail,
                }),
            }
        }
    }

    let passed = checks.iter().all(|c| c.ok);
    VerificationReport {
        job_id: job.job_id.clone(),
        passed,
        checks,
        reward_granted: false,
    }
}

fn validate_artifact(art: &CreationArtifact) -> Result<(), String> {
    let path = normalize_rel_path(&art.path)?;
    if art.content.len() > MAX_ARTIFACT_BYTES {
        return Err(format!(
            "artifact exceeds max size ({} > {MAX_ARTIFACT_BYTES})",
            art.content.len()
        ));
    }
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".jar") {
        return Err("mod jars are not writable via Creation apply".into());
    }
    if lower.ends_with(".json") {
        serde_json::from_str::<serde_json::Value>(&art.content)
            .map_err(|e| format!("invalid JSON: {e}"))?;
    } else if lower.ends_with(".js") || lower.ends_with(".ts") {
        let open = art.content.matches('{').count();
        let close = art.content.matches('}').count();
        if open != close {
            return Err(format!("unbalanced braces ({{ {open} vs }} {close})"));
        }
        if art.content.trim().is_empty() {
            return Err("empty script".into());
        }
    } else if art.content.trim().is_empty() {
        return Err("empty content".into());
    }
    Ok(())
}

/// Normalize a relative artifact path; rejects absolute paths and `..`.
pub fn normalize_rel_path(path: &str) -> Result<String, String> {
    let path = path.trim().replace('\\', "/");
    if path.is_empty() {
        return Err("empty path".into());
    }
    if path.starts_with('/') || path.contains(':') {
        return Err("absolute paths not allowed".into());
    }
    if path.split('/').any(|p| p == ".." || p.is_empty()) {
        return Err("path traversal not allowed".into());
    }
    Ok(path)
}

/// Write verified Creation artifacts under `project_dir` (UTF-8, create parents).
/// Does not touch mod jars. Caller must confirm in UI first.
pub fn apply_creation_artifacts_to_dir(
    project_dir: &std::path::Path,
    artifacts: &[CreationArtifact],
) -> Result<Vec<String>, String> {
    if artifacts.is_empty() {
        return Err("no artifacts to apply".into());
    }
    if artifacts.len() > MAX_ARTIFACTS {
        return Err(format!("too many artifacts ({})", artifacts.len()));
    }
    let root = project_dir
        .canonicalize()
        .map_err(|e| format!("project dir: {e}"))?;
    let mut written = Vec::with_capacity(artifacts.len());
    for art in artifacts {
        validate_artifact(art)?;
        let rel = normalize_rel_path(&art.path)?;
        let dest = root.join(&rel);
        // Ensure dest stays under root even after join (Windows prefix quirks).
        let parent = dest
            .parent()
            .ok_or_else(|| "artifact has no parent".to_string())?;
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        let canon_parent = parent
            .canonicalize()
            .map_err(|e| format!("canonicalize {}: {e}", parent.display()))?;
        if !canon_parent.starts_with(&root) {
            return Err(format!("refusing write outside project: {rel}"));
        }
        std::fs::write(&dest, art.content.as_bytes()).map_err(|e| format!("write {rel}: {e}"))?;
        written.push(rel);
    }
    Ok(written)
}

/// Deterministic local scaffold used by Creation workers until richer generators land.
pub fn scaffold_creation_artifacts(job: &CreationJob) -> Vec<CreationArtifact> {
    let brief = job.brief.trim().replace('\n', " ");
    let mc = if job.constraints.mc_version.trim().is_empty() {
        "1.20.1"
    } else {
        job.constraints.mc_version.trim()
    };
    let loader = if job.constraints.loader.trim().is_empty() {
        "fabric"
    } else {
        job.constraints.loader.trim()
    };
    match job.kind.as_str() {
        "kubejs_ore_gen" => vec![CreationArtifact {
            path: "kubejs/server_scripts/tuffswarm_ores.js".into(),
            content: format!(
                "// TuffSwarm Creation scaffold — {brief}\n\
                 // mc={mc} loader={loader}\n\
                 ServerEvents.recipes(event => {{\n\
                   // TODO: replace with generated ore density recipes\n\
                 }})\n"
            ),
        }],
        "quest_scripts" => vec![CreationArtifact {
            path: "kubejs/server_scripts/tuffswarm_quests.js".into(),
            content: format!(
                "// TuffSwarm Creation scaffold — {brief}\n\
                 // mc={mc} loader={loader}\n\
                 // Quest chapter stubs — import into FTB Quests manually.\n"
            ),
        }],
        "recipe_balance" => vec![CreationArtifact {
            path: "kubejs/data/tuffswarm/recipe_balance.json".into(),
            content: serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "brief": brief,
                "mcVersion": mc,
                "loader": loader,
                "notes": ["Scaffold only — tune rates before shipping"],
                "multipliers": { "default": 1.0 }
            }))
            .unwrap_or_else(|_| "{}".into()),
        }],
        "mod_configs" => vec![CreationArtifact {
            path: "config/tuffswarm_creation.json".into(),
            content: serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "brief": brief,
                "mcVersion": mc,
                "loader": loader,
                "enabled": true
            }))
            .unwrap_or_else(|_| "{}".into()),
        }],
        _ => vec![CreationArtifact {
            path: "tuffswarm/creation_scaffold.md".into(),
            content: format!(
                "# Creation scaffold\n\nBrief: {brief}\n\nMC: {mc} / {loader}\n\n\
                 Replace this stub with pack files before publish.\n"
            ),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_job(kind: &str) -> CreationJob {
        CreationJob {
            schema_version: 1,
            job_id: "job-1".into(),
            kind: kind.into(),
            constraints: CreationConstraints {
                mc_version: "1.20.1".into(),
                loader: "fabric".into(),
                mod_ids: vec!["create".into()],
            },
            brief: "Generate overworld ore density for Create".into(),
            reward: CreationReward::default(),
            verify: CreationVerifySpec::default(),
            deadline_ms: DEFAULT_CREATION_DEADLINE_MS,
        }
    }

    #[test]
    fn scaffold_passes_verify() {
        let job = sample_job("kubejs_ore_gen");
        job.validate().unwrap();
        let arts = scaffold_creation_artifacts(&job);
        let result = CreationResult {
            schema_version: 1,
            job_id: job.job_id.clone(),
            worker_peer_id: Some("peer".into()),
            worker_signer_public_key: None,
            ok: true,
            artifacts: arts,
            claimed_confidence: 0.4,
            error: None,
        };
        let report = verify_creation_result(&job, &result);
        assert!(report.passed, "{report:?}");
        assert!(!report.reward_granted);
    }

    #[test]
    fn rejects_path_traversal() {
        let job = sample_job("mod_configs");
        let result = CreationResult {
            schema_version: 1,
            job_id: job.job_id.clone(),
            worker_peer_id: None,
            worker_signer_public_key: None,
            ok: true,
            artifacts: vec![CreationArtifact {
                path: "../evil.json".into(),
                content: "{}".into(),
            }],
            claimed_confidence: 0.5,
            error: None,
        };
        let report = verify_creation_result(&job, &result);
        assert!(!report.passed);
    }

    #[test]
    fn apply_writes_utf8_under_project() {
        let dir = tempfile::tempdir().expect("tempdir");
        let arts = vec![CreationArtifact {
            path: "kubejs/server_scripts/tuffbox_gen.js".into(),
            content: "ServerEvents.recipes(e => {})".into(),
        }];
        let written = apply_creation_artifacts_to_dir(dir.path(), &arts).expect("apply");
        assert_eq!(written, vec!["kubejs/server_scripts/tuffbox_gen.js"]);
        let body = std::fs::read_to_string(dir.path().join(&written[0])).unwrap();
        assert!(body.contains("ServerEvents"));
    }

    #[test]
    fn apply_rejects_jar() {
        let dir = tempfile::tempdir().expect("tempdir");
        let arts = vec![CreationArtifact {
            path: "mods/evil.jar".into(),
            content: "not-a-jar".into(),
        }];
        assert!(apply_creation_artifacts_to_dir(dir.path(), &arts).is_err());
    }
}
