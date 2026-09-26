//! Per-project launch history: archives crashed session logs and maintains
//! `.tuffbox/history/launches.jsonl` so Diagnose can analyze the log from the
//! session that actually crashed — even after `latest.log` is overwritten.

use crate::time_util;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

const DEFAULT_LIST_LIMIT: usize = 24;

/// Second-resolution IDs caused two fast launch attempts to overwrite the
/// same archived log directory. Keep the human-readable timestamp and add
/// milliseconds for a stable per-attempt identity.
fn launch_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("{}-{}", time_util::compact_now(), millis)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchHistoryEntry {
    pub id: String,
    pub started_at: String,
    pub ended_at: String,
    pub exit_code: Option<i32>,
    pub duration_secs: u64,
    pub fingerprint_key: Option<String>,
    pub crash_report_path: Option<String>,
    /// True when latest.log / debug.log were copied into the session folder.
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionArchiveMeta {
    pub id: String,
    pub started_at: String,
    pub exit_code: Option<i32>,
    pub duration_secs: u64,
    pub fingerprint_key: Option<String>,
    pub crash_report_path: Option<String>,
    pub latest_log_path: Option<String>,
    pub debug_log_path: Option<String>,
}

fn history_root(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join("history")
}

fn launches_jsonl(project_dir: &Path) -> PathBuf {
    history_root(project_dir).join("launches.jsonl")
}

fn session_dir(project_dir: &Path, session_id: &str) -> PathBuf {
    history_root(project_dir).join("launches").join(session_id)
}

fn rel_path(project_dir: &Path, abs: &Path) -> Option<String> {
    abs.strip_prefix(project_dir)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

/// Copy crashed-session logs into `.tuffbox/history/launches/<id>/` and append
/// a journal row. Returns the new session id.
pub fn archive_crashed_session(
    project_dir: &Path,
    exit_code: Option<i32>,
    duration_secs: u64,
    fingerprint_key: Option<String>,
    crash_report_path: Option<&Path>,
) -> Result<LaunchHistoryEntry, String> {
    let session_id = launch_id();
    let started_at = time_util::rfc3339_now();
    let dir = session_dir(project_dir, &session_id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let latest_src = project_dir.join("logs").join("latest.log");
    let debug_src = project_dir.join("logs").join("debug.log");

    let mut latest_log_path = None;
    let mut debug_log_path = None;

    if latest_src.is_file() {
        let dst = dir.join("latest.log");
        fs::copy(&latest_src, &dst).map_err(|e| e.to_string())?;
        latest_log_path = Some(format!("launches/{session_id}/latest.log"));
    }
    if debug_src.is_file() {
        let dst = dir.join("debug.log");
        fs::copy(&debug_src, &dst).map_err(|e| e.to_string())?;
        debug_log_path = Some(format!("launches/{session_id}/debug.log"));
    }

    let crash_report_rel = crash_report_path.and_then(|p| rel_path(project_dir, p));

    let meta = SessionArchiveMeta {
        id: session_id.clone(),
        started_at: started_at.clone(),
        exit_code,
        duration_secs,
        fingerprint_key: fingerprint_key.clone(),
        crash_report_path: crash_report_rel.clone(),
        latest_log_path: latest_log_path.clone(),
        debug_log_path: debug_log_path.clone(),
    };
    fs::write(
        dir.join("meta.json"),
        serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let entry = LaunchHistoryEntry {
        id: session_id,
        started_at,
        ended_at: time_util::rfc3339_now(),
        exit_code,
        duration_secs,
        fingerprint_key,
        crash_report_path: crash_report_rel,
        archived: latest_log_path.is_some() || debug_log_path.is_some(),
    };
    append_launch_entry(project_dir, &entry)?;
    Ok(entry)
}

/// Record a launch exit (success or crash) without archiving logs.
pub fn record_launch_exit(
    project_dir: &Path,
    exit_code: Option<i32>,
    duration_secs: u64,
    fingerprint_key: Option<String>,
) -> Result<(), String> {
    let entry = LaunchHistoryEntry {
        id: launch_id(),
        started_at: time_util::rfc3339_now(),
        ended_at: time_util::rfc3339_now(),
        exit_code,
        duration_secs,
        fingerprint_key,
        crash_report_path: None,
        archived: false,
    };
    append_launch_entry(project_dir, &entry)
}

fn append_launch_entry(project_dir: &Path, entry: &LaunchHistoryEntry) -> Result<(), String> {
    let path = launches_jsonl(project_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let line = serde_json::to_string(entry).map_err(|e| e.to_string())?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    writeln!(file, "{line}").map_err(|e| e.to_string())?;
    Ok(())
}

/// Newest-first launch journal (crashed + healthy exits).
pub fn list_launch_history(project_dir: &Path, limit: usize) -> Vec<LaunchHistoryEntry> {
    let path = launches_jsonl(project_dir);
    if !path.is_file() {
        return Vec::new();
    }
    let file = match fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = BufReader::new(file);
    let mut entries: Vec<LaunchHistoryEntry> = reader
        .lines()
        .filter_map(|l| l.ok())
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(&l).ok())
        .collect();
    entries.reverse();
    entries.truncate(limit.max(1).min(100));
    entries
}

pub fn list_launch_history_default(project_dir: &Path) -> Vec<LaunchHistoryEntry> {
    list_launch_history(project_dir, DEFAULT_LIST_LIMIT)
}

/// Path to archived latest.log for a session (falls back to debug.log).
pub fn archived_session_log_path(project_dir: &Path, session_id: &str) -> Option<PathBuf> {
    let dir = session_dir(project_dir, session_id);
    let latest = dir.join("latest.log");
    if latest.is_file() {
        return Some(latest);
    }
    let debug = dir.join("debug.log");
    if debug.is_file() {
        return Some(debug);
    }
    None
}

pub fn load_session_meta(project_dir: &Path, session_id: &str) -> Option<SessionArchiveMeta> {
    let meta_path = session_dir(project_dir, session_id).join("meta.json");
    let raw = fs::read_to_string(&meta_path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Sentinel prefix for selecting an archived session in Diagnose APIs.
pub const SESSION_REPORT_PREFIX: &str = "session/";

pub fn is_session_report_id(id: &str) -> bool {
    id.starts_with(SESSION_REPORT_PREFIX)
}

pub fn session_id_from_report_id(id: &str) -> Option<&str> {
    id.strip_prefix(SESSION_REPORT_PREFIX)
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn archive_and_list_roundtrip() {
        let tmp = std::env::temp_dir().join(format!(
            "tuffbox-launch-history-{}",
            time_util::compact_now()
        ));
        let logs = tmp.join("logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(
            logs.join("latest.log"),
            "---- Minecraft Crash Report ----\n",
        )
        .unwrap();

        let entry = archive_crashed_session(&tmp, Some(1), 42, Some("fp-abc".into()), None)
            .expect("archive");
        assert!(entry.archived);
        assert_eq!(entry.fingerprint_key.as_deref(), Some("fp-abc"));

        let listed = list_launch_history(&tmp, 5);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, entry.id);

        let log = archived_session_log_path(&tmp, &entry.id).expect("log path");
        assert!(log.is_file());

        let _ = fs::remove_dir_all(&tmp);
    }
}
