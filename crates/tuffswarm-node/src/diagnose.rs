//! Fog diagnose job types (L2) — shared by libp2p request-response and control HTTP.

use serde::{Deserialize, Serialize};
use tuffbox_core::action_plan::ActionPlan;
use tuffbox_core::crash_kb::CrashFingerprint;

pub const DIAGNOSE_PROTOCOL: &str = "/tuffswarm/diagnose/1.0.0";
pub const MAX_JOB_BYTES: usize = 64 * 1024;
#[allow(dead_code)] // reserved for response size guards
pub const MAX_RESULT_BYTES: usize = 256 * 1024;
pub const DEFAULT_DEADLINE_MS: u64 = 45_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnoseJob {
    #[serde(default = "schema_v1")]
    pub schema_version: u32,
    pub job_id: String,
    pub fingerprint: CrashFingerprint,
    #[serde(default)]
    pub excerpt: String,
    #[serde(default)]
    pub context: DiagnoseJobContext,
    #[serde(default = "default_deadline")]
    pub deadline_ms: u64,
}

fn schema_v1() -> u32 {
    1
}

fn default_deadline() -> u64 {
    DEFAULT_DEADLINE_MS
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnoseJobContext {
    #[serde(default)]
    pub suspected_mods: Vec<String>,
    #[serde(default)]
    pub mc_version: String,
    #[serde(default)]
    pub loader: String,
    #[serde(default)]
    pub fingerprint_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnoseResult {
    #[serde(default = "schema_v1")]
    pub schema_version: u32,
    pub job_id: String,
    #[serde(default)]
    pub worker_peer_id: Option<String>,
    pub ok: bool,
    #[serde(default)]
    pub plan: Option<ActionPlan>,
    #[serde(default)]
    pub error: Option<String>,
}

impl DiagnoseResult {
    pub fn err(job_id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            job_id: job_id.into(),
            worker_peer_id: None,
            ok: false,
            plan: None,
            error: Some(error.into()),
        }
    }

    #[allow(dead_code)]
    pub fn ok_plan(job_id: impl Into<String>, plan: ActionPlan, worker_peer_id: String) -> Self {
        Self {
            schema_version: 1,
            job_id: job_id.into(),
            worker_peer_id: Some(worker_peer_id),
            ok: true,
            plan: Some(plan),
            error: None,
        }
    }
}

impl DiagnoseJob {
    pub fn encoded_len(&self) -> Result<usize, String> {
        serde_json::to_vec(self)
            .map(|b| b.len())
            .map_err(|e| e.to_string())
    }

    pub fn validate_size(&self) -> Result<(), String> {
        let n = self.encoded_len()?;
        if n > MAX_JOB_BYTES {
            return Err(format!(
                "diagnose job exceeds max size ({n} > {MAX_JOB_BYTES} bytes)"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnose_job_serde_roundtrip() {
        let job = DiagnoseJob {
            schema_version: 1,
            job_id: "job-1".into(),
            fingerprint: CrashFingerprint {
                exception: "NullPointerException".into(),
                frames: vec!["a.b.c".into()],
                mod_file: None,
                mixin: None,
                mc_major: "1.20".into(),
                loader: "neoforge".into(),
                key: "npe|a.b.c||1.20|neoforge".into(),
                blame_mod_ids: vec![],
            },
            excerpt: "Caused by: java.lang.NullPointerException".into(),
            context: DiagnoseJobContext {
                suspected_mods: vec!["foo".into()],
                mc_version: "1.20.1".into(),
                loader: "neoforge".into(),
                fingerprint_key: "npe|a.b.c||1.20|neoforge".into(),
            },
            deadline_ms: 45_000,
        };
        let bytes = serde_json::to_vec(&job).unwrap();
        let back: DiagnoseJob = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back.job_id, "job-1");
        assert_eq!(back.fingerprint.key, job.fingerprint.key);
        assert!(back.validate_size().is_ok());
    }

    #[test]
    fn oversized_job_rejected() {
        let job = DiagnoseJob {
            schema_version: 1,
            job_id: "big".into(),
            fingerprint: CrashFingerprint {
                exception: "X".into(),
                frames: vec![],
                mod_file: None,
                mixin: None,
                mc_major: "1.20".into(),
                loader: "fabric".into(),
                key: "x".into(),
                blame_mod_ids: vec![],
            },
            excerpt: "x".repeat(MAX_JOB_BYTES),
            context: DiagnoseJobContext::default(),
            deadline_ms: 1_000,
        };
        assert!(job.validate_size().is_err());
    }
}
