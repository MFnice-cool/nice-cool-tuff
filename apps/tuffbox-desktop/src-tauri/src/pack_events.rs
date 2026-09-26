//! Pack activity event log + baseline delta scanner for History / AI context.
//!
//! Storage:
//! - `.tuffbox/events.jsonl` — append-only activity journal
//! - `.tuffbox/history-baseline.json` — mtime/size map for external-edit detection

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_EVENTS: usize = 2000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackEvent {
    pub id: String,
    pub ts: String,
    pub actor: String,
    pub op: String,
    #[serde(default)]
    pub paths: Vec<String>,
    pub category: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

pub fn is_mod_toggle_event(ev: &PackEvent) -> bool {
    let op_l = ev.op.to_lowercase();
    if op_l.contains("crash") || op_l.contains("soft_verify") {
        return false;
    }
    let operation = ev
        .meta
        .as_ref()
        .and_then(|m| m.get("operation"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    operation.contains("disable")
        || operation.contains("enable")
        || operation.contains("group-test")
        || op_l.contains("disable")
        || op_l.contains("enable")
        || op_l.contains("group-test")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BaselineEntry {
    pub mtime: u64,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HistoryBaseline {
    #[serde(default)]
    pub files: HashMap<String, BaselineEntry>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProjectChangesResult {
    pub events: Vec<PackEvent>,
    pub baseline_updated: bool,
    pub added: usize,
    pub modified: usize,
    pub removed: usize,
    pub jar_drift: usize,
}

fn events_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join("events.jsonl")
}

fn baseline_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join("history-baseline.json")
}

fn now_rfc3339() -> String {
    tuffbox_core::time_util::rfc3339_now()
}

fn now_id_suffix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn category_for_path(path: &str) -> &'static str {
    let normalized = path.replace('\\', "/").to_lowercase();
    let root = normalized.split('/').next().unwrap_or("");
    if matches!(normalized.as_str(), "options.txt" | "servers.dat") {
        return "Configs";
    }
    match root {
        "mods" => "Mods",
        "config" | "defaultconfigs" | "kubejs" | "scripts" | "overrides" => "Configs",
        "shaderpacks" | "shaders" => "Shaders",
        "resourcepacks" | "texturepacks" => "Resource Packs",
        "datapacks" | "world" | "saves" => "World/Data",
        _ => "Other",
    }
}

pub fn actor_for_operation(operation: &str) -> &'static str {
    let op = operation.to_lowercase();
    if op.contains("crash_detected") {
        "launcher"
    } else if op.contains("crash_fix")
        || op.contains("crash-fix")
        || op.contains("action-plan")
        || op.contains("action_plan")
        || op.contains("swarm")
    {
        // Prefer plan_source via actor_for_plan_source when known; this is a
        // coarse fallback for snapshot names that still say "crash_fix".
        "launcher"
    } else if op.contains("track-history") || op.contains("scan") {
        "scan"
    } else if op.contains("add-mod")
        || op.contains("remove-mod")
        || op.contains("disable-mod")
        || op.contains("enable-mod")
        || op.contains("update-mod")
        || op.contains("edit-config")
        || op.contains("save-quest")
    {
        "user"
    } else {
        "launcher"
    }
}

/// Map ActionPlan / snapshot `planSource` to History fixMethod + actor.
pub fn normalize_fix_method(plan_source: Option<&str>) -> &'static str {
    let raw = plan_source.unwrap_or("").trim().to_ascii_lowercase();
    match raw.as_str() {
        "ai" | "ai_action_plan" | "llm" | "local" | "server" => "ai",
        "heuristic" | "crash_assistant" | "assistant" => "heuristic",
        "kb" | "kb_only" | "distill" => "kb",
        "swarm" => "swarm",
        "manual" | "user" => "manual",
        "" => "unknown",
        _ => "unknown",
    }
}

pub fn actor_for_plan_source(plan_source: Option<&str>) -> &'static str {
    match normalize_fix_method(plan_source) {
        "ai" | "kb" | "swarm" => "ai",
        "heuristic" => "launcher",
        "manual" => "user",
        _ => "launcher",
    }
}

pub fn episode_id_for_fingerprint(fingerprint_key: &str) -> String {
    let key = fingerprint_key.trim();
    if key.is_empty() || key == "unknown" {
        format!("ep-{}", now_id_suffix())
    } else {
        let compact: String = key
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(48)
            .collect();
        if compact.is_empty() {
            format!("ep-{}", now_id_suffix())
        } else {
            format!("ep-{compact}")
        }
    }
}

pub fn meta_str(meta: &Option<serde_json::Value>, key: &str) -> Option<String> {
    meta.as_ref()
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
}

pub fn op_for_operation(operation: &str, paths: &[String]) -> String {
    let op = operation.to_lowercase();
    if op.contains("crash_detected") || op == "crash-detected" {
        return "crash_detected".into();
    }
    if op.contains("crash_fix_rejected") || op.contains("crash-fix-rejected") {
        return "crash_fix_rejected".into();
    }
    if op.contains("crash_fix_rollback") || op.contains("crash-fix-rollback") {
        return "crash_fix_rollback".into();
    }
    if op.contains("crash_fix") || op.contains("crash-fix") {
        return "crash_fix".into();
    }
    if op.contains("crash_resolved") || op.contains("crash-resolved") {
        return "crash_resolved".into();
    }
    if op.contains("edit-config") || op.contains("save-quest") {
        return "file_edit".into();
    }
    if op.contains("track-history") {
        return "snapshot".into();
    }
    if op.contains("rollback") {
        return "rollback".into();
    }
    if op.contains("group-test")
        || op.contains("mod")
        || paths.iter().any(|p| p.starts_with("mods/"))
    {
        return "mod_change".into();
    }
    "snapshot".into()
}

pub fn append_pack_event(project_dir: &Path, mut event: PackEvent) -> Result<(), String> {
    let dir = project_dir.join(".tuffbox");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = events_path(project_dir);
    if event.id.is_empty() {
        event.id = format!("evt-{}", now_id_suffix());
    }
    if event.ts.is_empty() {
        event.ts = now_rfc3339();
    }
    let line = serde_json::to_string(&event).map_err(|e| e.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    writeln!(file, "{line}").map_err(|e| e.to_string())?;
    trim_events_file(&path)?;
    Ok(())
}

#[allow(dead_code)]
pub fn append_from_snapshot(
    project_dir: &Path,
    operation: &str,
    snapshot_id: &str,
    changed_files: &[PathBuf],
    reason: &str,
) -> Result<(), String> {
    append_from_snapshot_with_summary(project_dir, operation, snapshot_id, changed_files, reason, &[])
}

/// Like [`append_from_snapshot`], but prefers human `actions_summary` lines when
/// no changed files were copied into the snapshot (typical for add-mod).
pub fn append_from_snapshot_with_summary(
    project_dir: &Path,
    operation: &str,
    snapshot_id: &str,
    changed_files: &[PathBuf],
    reason: &str,
    actions_summary: &[String],
) -> Result<(), String> {
    append_from_snapshot_with_episode(
        project_dir,
        operation,
        snapshot_id,
        changed_files,
        reason,
        actions_summary,
        None,
        None,
        None,
    )
}

/// Snapshot journal write with optional crash-episode linkage.
pub fn append_from_snapshot_with_episode(
    project_dir: &Path,
    operation: &str,
    snapshot_id: &str,
    changed_files: &[PathBuf],
    reason: &str,
    actions_summary: &[String],
    episode_id: Option<&str>,
    fingerprint_key: Option<&str>,
    plan_source: Option<&str>,
) -> Result<(), String> {
    let paths: Vec<String> = changed_files
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let category = if paths.iter().any(|p| category_for_path(p) == "Mods")
        || operation.to_lowercase().contains("mod")
        || operation.to_lowercase().contains("crash")
    {
        if operation.to_lowercase().contains("crash") {
            "Resolutions".into()
        } else {
            "Mods".into()
        }
    } else {
        paths
            .first()
            .map(|p| category_for_path(p).to_string())
            .unwrap_or_else(|| "Other".into())
    };
    let summary = if !actions_summary.is_empty() {
        let head: Vec<&str> = actions_summary.iter().take(8).map(|s| s.as_str()).collect();
        let more = actions_summary.len().saturating_sub(head.len());
        if more > 0 {
            format!("{} (+{} more)", head.join("; "), more)
        } else {
            head.join("; ")
        }
    } else if paths.is_empty() {
        // Avoid "add-mod: Auto snapshot before add-mod" noise.
        if reason.starts_with("Auto snapshot before ") {
            format!("Safety point before {operation}")
        } else {
            format!("{operation}: {reason}")
        }
    } else if paths.len() == 1 {
        format!("{operation}: {}", paths[0])
    } else {
        format!("{operation}: {} files ({})", paths.len(), paths[0])
    };
    let mut tags = Vec::new();
    let op_l = operation.to_lowercase();
    if op_l.contains("crash_fix") && !op_l.contains("rejected") && !op_l.contains("rollback") {
        tags.push("crash_fix".into());
    }
    if op_l.contains("crash_resolved") {
        tags.push("crash_resolved".into());
    }
    if op_l.contains("crash_detected") {
        tags.push("crash".into());
    }
    let actor = if plan_source.is_some() {
        actor_for_plan_source(plan_source).to_string()
    } else {
        actor_for_operation(operation).to_string()
    };
    let ep = episode_id
        .map(|s| s.to_string())
        .or_else(|| fingerprint_key.map(episode_id_for_fingerprint));
    let mut meta = serde_json::json!({
        "reason": reason,
        "operation": operation,
        "actionsSummary": actions_summary,
    });
    if let Some(obj) = meta.as_object_mut() {
        if let Some(ep) = &ep {
            obj.insert("episodeId".into(), serde_json::json!(ep));
        }
        if let Some(fp) = fingerprint_key.filter(|s| !s.trim().is_empty()) {
            obj.insert("fingerprintKey".into(), serde_json::json!(fp));
        }
        if let Some(ps) = plan_source.filter(|s| !s.trim().is_empty()) {
            obj.insert("planSource".into(), serde_json::json!(ps));
            obj.insert(
                "fixMethod".into(),
                serde_json::json!(normalize_fix_method(Some(ps))),
            );
        }
    }
    append_pack_event(
        project_dir,
        PackEvent {
            id: format!("evt-{}-{}", snapshot_id, now_id_suffix() % 10_000),
            ts: now_rfc3339(),
            actor,
            op: op_for_operation(operation, &paths),
            paths,
            category,
            summary,
            snapshot_id: Some(snapshot_id.to_string()),
            tags,
            meta: Some(meta),
        },
    )
}

/// Record that a launch ended in a crash — starts / continues a History episode.
pub fn append_crash_detected(
    project_dir: &Path,
    fingerprint_key: &str,
    exit_code: Option<i32>,
    log_path: Option<&str>,
    message: &str,
) -> Result<String, String> {
    let episode_id = episode_id_for_fingerprint(fingerprint_key);
    let summary = if message.trim().is_empty() {
        format!("Launch crashed · fingerprint {}", &fingerprint_key.chars().take(24).collect::<String>())
    } else {
        let short: String = message.chars().take(160).collect();
        format!("Launch crashed: {short}")
    };
    let mut meta = serde_json::json!({
        "episodeId": episode_id,
        "fingerprintKey": fingerprint_key,
        "exitCode": exit_code,
    });
    if let Some(lp) = log_path.filter(|s| !s.is_empty()) {
        if let Some(obj) = meta.as_object_mut() {
            obj.insert("logPath".into(), serde_json::json!(lp));
        }
    }
    let id = format!("evt-crash-{}-{}", now_id_suffix() % 100_000, now_id_suffix() % 997);
    append_pack_event(
        project_dir,
        PackEvent {
            id,
            ts: now_rfc3339(),
            actor: "launcher".into(),
            op: "crash_detected".into(),
            paths: Vec::new(),
            category: "Resolutions".into(),
            summary,
            snapshot_id: None,
            tags: vec!["crash".into(), "crash_detected".into()],
            meta: Some(meta),
        },
    )?;
    Ok(episode_id)
}

/// Soft-verify / rollback outcomes as journal events (same episode).
pub fn append_crash_outcome_event(
    project_dir: &Path,
    op: &str,
    episode_id: &str,
    fingerprint_key: &str,
    plan_source: Option<&str>,
    snapshot_id: Option<&str>,
    summary: &str,
) -> Result<(), String> {
    let mut tags = vec!["crash".into()];
    let op_l = op.to_ascii_lowercase();
    if op_l.contains("resolved") {
        tags.push("crash_resolved".into());
        tags.push("crash_fix".into());
    } else if op_l.contains("reject") {
        tags.push("crash_fix".into());
        tags.push("crash_fix_rejected".into());
    } else if op_l.contains("rollback") {
        tags.push("crash_fix".into());
        tags.push("crash_fix_rollback".into());
    }
    let mut meta = serde_json::json!({
        "episodeId": episode_id,
        "fingerprintKey": fingerprint_key,
        "fixMethod": normalize_fix_method(plan_source),
    });
    if let Some(ps) = plan_source.filter(|s| !s.is_empty()) {
        if let Some(obj) = meta.as_object_mut() {
            obj.insert("planSource".into(), serde_json::json!(ps));
        }
    }
    append_pack_event(
        project_dir,
        PackEvent {
            id: format!("evt-{}-{}", op, now_id_suffix() % 100_000),
            ts: now_rfc3339(),
            actor: actor_for_plan_source(plan_source).into(),
            op: op_for_operation(op, &[]),
            paths: Vec::new(),
            category: "Resolutions".into(),
            summary: summary.to_string(),
            snapshot_id: snapshot_id.map(|s| s.to_string()),
            tags,
            meta: Some(meta),
        },
    )
}

/// Record a launcher mod operation with concrete mod names / jar paths.
pub fn record_mod_change_event(
    project_dir: &Path,
    operation: &str,
    snapshot_id: Option<&str>,
    summaries: &[String],
    paths: &[String],
    mod_ids: &[String],
) -> Result<(), String> {
    record_mod_change_event_meta(
        project_dir,
        operation,
        snapshot_id,
        summaries,
        paths,
        mod_ids,
        None,
    )
}

/// Like [`record_mod_change_event`], merging extra object keys into `meta`.
pub fn record_mod_change_event_meta(
    project_dir: &Path,
    operation: &str,
    snapshot_id: Option<&str>,
    summaries: &[String],
    paths: &[String],
    mod_ids: &[String],
    extra_meta: Option<serde_json::Value>,
) -> Result<(), String> {
    if summaries.is_empty() && paths.is_empty() && mod_ids.is_empty() {
        return Ok(());
    }
    let summary = if !summaries.is_empty() {
        let head: Vec<&str> = summaries.iter().take(12).map(|s| s.as_str()).collect();
        let more = summaries.len().saturating_sub(head.len());
        if more > 0 {
            format!("{} (+{} more)", head.join("; "), more)
        } else {
            head.join("; ")
        }
    } else if !mod_ids.is_empty() {
        format!("{operation}: {}", mod_ids.join(", "))
    } else {
        format!("{operation}: {} file(s)", paths.len())
    };
    let id = format!(
        "evt-mod-{}-{}",
        snapshot_id.unwrap_or("nosnap"),
        now_id_suffix() % 100_000
    );
    let mut meta = serde_json::json!({
        "operation": operation,
        "actionsSummary": summaries,
    });
    if let Some(obj) = meta.as_object_mut() {
        if !mod_ids.is_empty() {
            obj.insert("modIds".into(), serde_json::json!(mod_ids));
        }
        if mod_ids.len() == 1 {
            obj.insert("modId".into(), serde_json::json!(mod_ids[0]));
        }
        if let Some(extra) = extra_meta {
            if let Some(extra_obj) = extra.as_object() {
                for (k, v) in extra_obj {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }
    }
    let category = if operation.to_lowercase().contains("group-test")
        || operation.to_lowercase().contains("mod")
        || paths.iter().any(|p| category_for_path(p) == "Mods")
    {
        "Mods".into()
    } else {
        paths
            .first()
            .map(|p| category_for_path(p).to_string())
            .unwrap_or_else(|| "Mods".into())
    };
    append_pack_event(
        project_dir,
        PackEvent {
            id,
            ts: now_rfc3339(),
            actor: "launcher".into(),
            op: op_for_operation(operation, paths),
            paths: paths.to_vec(),
            category,
            summary,
            snapshot_id: snapshot_id.map(|s| s.to_string()),
            tags: vec!["launcher".into()],
            meta: Some(meta),
        },
    )?;
    sync_baseline_paths(project_dir, paths)?;
    Ok(())
}

/// One History card per group-test layout step (`meta.enabled` / `meta.disabled`).
pub fn record_group_test_layout_event(
    project_dir: &Path,
    snapshot_id: Option<&str>,
    enabled: &[String],
    disabled: &[String],
    paths: &[String],
) -> Result<(), String> {
    if enabled.is_empty() && disabled.is_empty() {
        return Ok(());
    }
    let mut parts = Vec::new();
    if !disabled.is_empty() {
        parts.push(format!("disable {}", disabled.join(", ")));
    }
    if !enabled.is_empty() {
        parts.push(format!("enable {}", enabled.join(", ")));
    }
    let summary = format!("Group test: {}", parts.join("; "));
    let mut mod_ids = Vec::new();
    for id in disabled.iter().chain(enabled.iter()) {
        if !mod_ids.iter().any(|x| x == id) {
            mod_ids.push(id.clone());
        }
    }
    record_mod_change_event_meta(
        project_dir,
        "group-test-layout",
        snapshot_id,
        &[summary],
        paths,
        &mod_ids,
        Some(serde_json::json!({
            "enabled": enabled,
            "disabled": disabled,
        })),
    )
}

/// Update history baseline entries for the given relative paths (so launcher
/// installs are not later reported as external_add).
pub fn sync_baseline_paths(project_dir: &Path, relative_paths: &[String]) -> Result<(), String> {
    if relative_paths.is_empty() {
        return Ok(());
    }
    let mut baseline = load_baseline(project_dir);
    for rel in relative_paths {
        let abs = project_dir.join(rel);
        if abs.is_file() {
            if let Ok(meta) = abs.metadata() {
                baseline.files.insert(
                    rel.replace('\\', "/"),
                    BaselineEntry {
                        mtime: file_mtime_secs(&abs),
                        size: meta.len(),
                    },
                );
            }
            cache_config_content(project_dir, rel);
        } else {
            baseline.files.remove(rel);
            remove_cached_config_content(project_dir, rel);
        }
    }
    baseline.updated_at = now_rfc3339();
    save_baseline(project_dir, &baseline)
}

fn content_cache_root(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join("history-content")
}

fn content_cache_path(project_dir: &Path, rel: &str) -> PathBuf {
    let safe = rel.replace('\\', "/").replace("..", "_");
    content_cache_root(project_dir).join(safe)
}

fn is_textish_config(rel: &str) -> bool {
    let path = Path::new(rel);
    crate::helpers::is_editable_config_path(path)
}

fn read_cached_config_content(project_dir: &Path, rel: &str) -> Option<String> {
    let path = content_cache_path(project_dir, rel);
    std::fs::read_to_string(path).ok()
}

fn cache_config_content(project_dir: &Path, rel: &str) {
    if !is_textish_config(rel) {
        return;
    }
    let abs = project_dir.join(rel);
    if !abs.is_file() {
        return;
    }
    let Ok(meta) = abs.metadata() else {
        return;
    };
    if meta.len() > 512 * 1024 {
        return;
    }
    let Ok(text) = std::fs::read_to_string(&abs) else {
        return;
    };
    let cache = content_cache_path(project_dir, rel);
    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(cache, text);
}

fn remove_cached_config_content(project_dir: &Path, rel: &str) {
    let _ = std::fs::remove_file(content_cache_path(project_dir, rel));
}

fn path_basename(rel: &str) -> &str {
    Path::new(rel)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(rel)
}

/// Short path for UI labels: basename, or mid-truncated relative path when deep.
fn short_path_label(rel: &str) -> String {
    let norm = rel.replace('\\', "/");
    let base = path_basename(&norm);
    if !norm.contains('/') || norm.len() <= 40 {
        return base.to_string();
    }
    // e.g. kubejs/…/tuffbox_ftb_quests.js
    let parts: Vec<&str> = norm.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 3 {
        format!("{}/…/{}", parts[0], base)
    } else {
        base.to_string()
    }
}

fn looks_like_human_operation(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    if t.contains("+ //") || t.contains("+ \t") || t.contains(": added (") || t.contains(": removed (")
    {
        return false;
    }
    if t.starts_with("Added on disk:")
        || t.starts_with("Removed from disk:")
        || t.starts_with("Changed on disk:")
        || t.starts_with("Pack change ·")
        || t.starts_with("Pack activity ·")
    {
        return false;
    }
    const PREFIXES: &[&str] = &[
        "Install ",
        "Remove ",
        "Update ",
        "Disable ",
        "Enable ",
        "Added ",
        "Edited ",
        "Removed ",
        "Fixed ",
        "Rolled back ",
    ];
    PREFIXES.iter().any(|p| t.starts_with(p))
}

fn truncate_label(s: &str, max_chars: usize) -> String {
    tuffbox_core::crash_kb::truncate_at_char_boundary(s, max_chars).to_string()
}

fn keys_hint_from_preview(place: &str) -> Option<String> {
    let place = place.trim();
    if place.is_empty() || place.starts_with("content updated") {
        return None;
    }
    let keys = place.strip_prefix("changed ").unwrap_or(place).trim();
    if keys.is_empty() {
        None
    } else {
        Some(keys.to_string())
    }
}

fn concise_file_op_summary(op: &str, rel: &str, place: &str) -> String {
    let base = short_path_label(rel);
    match op {
        "external_add" => format!("Added {base}"),
        "external_remove" => format!("Removed {base}"),
        _ => {
            if let Some(keys) = keys_hint_from_preview(place) {
                format!("Edited {base} · {keys}")
            } else {
                format!("Edited {base}")
            }
        }
    }
}

/// Rewrite legacy noisy History summaries (path: added (+ //…), Changed on disk: …)
/// into short human labels for display. Full diffs stay in meta.diff.
pub fn concise_event_summary(summary: &str, paths: &[String], op: &str) -> String {
    let s = summary.trim();
    let path = paths
        .iter()
        .map(|p| p.replace('\\', "/"))
        .find(|p| !p.is_empty() && p != op)
        .unwrap_or_default();
    let base = if path.is_empty() {
        String::new()
    } else {
        short_path_label(&path)
    };

    if s.is_empty() {
        return match op {
            "external_add" if !base.is_empty() => format!("Added {base}"),
            "external_remove" if !base.is_empty() => format!("Removed {base}"),
            "external_edit" | "file_edit" | "file_changed" if !base.is_empty() => {
                format!("Edited {base}")
            }
            _ if !base.is_empty() => format!("Changed {base}"),
            _ => op.replace('_', " "),
        };
    }

    if looks_like_human_operation(s) {
        return truncate_label(s, 80);
    }

    // "Added on disk: path" / "Removed from disk: path" / "Changed on disk: path"
    for (prefix, verb) in [
        ("Added on disk:", "Added"),
        ("Removed from disk:", "Removed"),
        ("Changed on disk:", "Edited"),
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let p = rest.trim();
            let label = if p.is_empty() {
                base.clone()
            } else {
                short_path_label(p)
            };
            if label.is_empty() {
                return verb.to_string();
            }
            return format!("{verb} {label}");
        }
    }

    // "rel/path.js: added (…)" / ": removed (…)" / ": changed …"
    if let Some((left, right)) = s.split_once(": ") {
        let left = left.trim();
        let right = right.trim();
        let label = if left.contains('/') || left.contains('\\') || left.contains('.') {
            short_path_label(left)
        } else if !base.is_empty() {
            base.clone()
        } else {
            left.to_string()
        };
        if right.starts_with("added") {
            return format!("Added {label}");
        }
        if right.starts_with("removed") {
            return format!("Removed {label}");
        }
        if let Some(keys) = keys_hint_from_preview(right) {
            return format!("Edited {label} · {keys}");
        }
        if right.starts_with("changed") || right.starts_with("content updated") {
            return format!("Edited {label}");
        }
    }

    // Raw diff dump leaked into summary/preview
    if s.contains("+ //")
        || s.lines().any(|l| l.starts_with("+ ") || l.starts_with("- "))
        || s.contains(" · +")
        || s.contains(" · -")
    {
        return match op {
            "external_add" if !base.is_empty() => format!("Added {base}"),
            "external_remove" if !base.is_empty() => format!("Removed {base}"),
            _ if !base.is_empty() => format!("Edited {base}"),
            _ => "Content updated".into(),
        };
    }

    if !base.is_empty() && (s.contains(&path) || s.len() > 80) {
        return match op {
            "external_add" => format!("Added {base}"),
            "external_remove" => format!("Removed {base}"),
            "external_edit" | "file_edit" | "file_changed" => format!("Edited {base}"),
            _ => truncate_label(s, 80),
        };
    }

    truncate_label(s, 80)
}

/// Pull changed keys / assignment lines from a unified diff for a short preview.
fn config_change_preview(diff: &str) -> String {
    let mut keys = Vec::new();
    let mut changed_lines = 0usize;
    let mut substantive_lines = 0usize;
    for line in diff.lines() {
        let trimmed = if let Some(rest) = line.strip_prefix("+ ").or_else(|| line.strip_prefix("- "))
        {
            rest.trim()
        } else {
            continue;
        };
        changed_lines += 1;
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        substantive_lines += 1;
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // TOML section
            let section = trimmed.trim_matches(|c| c == '[' || c == ']');
            if !section.is_empty() && !keys.iter().any(|k| k == section) {
                keys.push(section.to_string());
            }
            continue;
        }
        let key = trimmed
            .split_once('=')
            .or_else(|| trimmed.split_once(':'))
            .map(|(k, _)| k.trim().trim_matches('"').trim_matches('\''))
            .filter(|k| !k.is_empty() && k.len() < 80);
        if let Some(k) = key {
            if !keys.iter().any(|existing| existing == k) {
                keys.push(k.to_string());
            }
        }
        if keys.len() >= 6 {
            break;
        }
    }
    if !keys.is_empty() {
        return format!("changed {}", keys.join(", "));
    }
    let n = if substantive_lines > 0 {
        substantive_lines
    } else {
        changed_lines
    };
    if n == 0 {
        "content updated".into()
    } else {
        format!("content updated ({n} lines)")
    }
}

fn enrich_external_file_event(
    project_dir: &Path,
    rel: &str,
    op: &str,
) -> (String, Option<serde_json::Value>) {
    if !is_textish_config(rel) {
        let summary = concise_file_op_summary(op, rel, "");
        return (summary, None);
    }

    let abs = project_dir.join(rel);
    let after = if abs.is_file() {
        crate::helpers::read_small_text_file(&abs).unwrap_or_default()
    } else {
        String::new()
    };
    let before = read_cached_config_content(project_dir, rel).unwrap_or_default();

    let (summary, meta) = if op == "external_add" {
        let diff = crate::helpers::unified_text_diff("", &after);
        let place = if after.is_empty() {
            String::new()
        } else {
            config_change_preview(&diff)
        };
        let summary = concise_file_op_summary(op, rel, &place);
        (
            summary.clone(),
            Some(serde_json::json!({
                "diff": diff,
                "preview": summary,
            })),
        )
    } else if op == "external_remove" {
        let diff = crate::helpers::unified_text_diff(&before, "");
        let place = if before.is_empty() {
            String::new()
        } else {
            config_change_preview(&diff)
        };
        let summary = concise_file_op_summary(op, rel, &place);
        (
            summary.clone(),
            Some(serde_json::json!({
                "diff": diff,
                "preview": summary,
            })),
        )
    } else {
        let diff = crate::helpers::unified_text_diff(&before, &after);
        let place = config_change_preview(&diff);
        let summary = concise_file_op_summary(op, rel, &place);
        (
            summary.clone(),
            Some(serde_json::json!({
                "diff": diff,
                "preview": summary,
            })),
        )
    };

    if op == "external_remove" {
        remove_cached_config_content(project_dir, rel);
    } else {
        cache_config_content(project_dir, rel);
    }

    (summary, meta)
}

fn trim_events_file(path: &Path) -> Result<(), String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };
    let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() <= MAX_EVENTS {
        return Ok(());
    }
    let keep = &lines[lines.len() - MAX_EVENTS..];
    let mut out = String::new();
    for line in keep {
        out.push_str(line);
        out.push('\n');
    }
    std::fs::write(path, out).map_err(|e| e.to_string())
}

pub fn list_pack_events(project_dir: &Path, limit: Option<usize>) -> Vec<PackEvent> {
    let path = events_path(project_dir);
    let Ok(file) = std::fs::File::open(&path) else {
        return Vec::new();
    };
    let reader = std::io::BufReader::new(file);
    let mut events = Vec::new();
    for line in reader.lines().flatten() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Ok(ev) = serde_json::from_str::<PackEvent>(t) {
            events.push(ev);
        }
    }
    events.reverse(); // newest first
    if let Some(n) = limit {
        events.truncate(n);
    }
    events
}

pub fn event_by_id(project_dir: &Path, id: &str) -> Option<PackEvent> {
    list_pack_events(project_dir, None)
        .into_iter()
        .find(|ev| ev.id == id)
}

pub fn event_diff_text(ev: &PackEvent) -> String {
    ev.meta
        .as_ref()
        .and_then(|m| m.get("diff"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| ev.summary.clone())
}

/// Chronological pack events from the latest `crash_detected` for this fingerprint
/// until `crash_resolved` / soft-verify outcome (or end of journal).
/// Used to document the player action trail when causality is unknown.
pub fn events_between_crash_and_resolve(
    project_dir: &Path,
    fingerprint_key: &str,
) -> Vec<PackEvent> {
    let episode = episode_id_for_fingerprint(fingerprint_key);
    let mut chronological: Vec<PackEvent> = list_pack_events(project_dir, Some(800));
    chronological.reverse(); // oldest → newest
    let mut start = None;
    for (i, ev) in chronological.iter().enumerate() {
        let is_detect = ev.op == "crash_detected" || ev.tags.iter().any(|t| t == "crash_detected");
        if !is_detect {
            continue;
        }
        let fp_match = ev
            .meta
            .as_ref()
            .and_then(|m| m.get("fingerprintKey"))
            .and_then(|v| v.as_str())
            .map(|k| k == fingerprint_key)
            .unwrap_or(false);
        let ep_match = ev
            .meta
            .as_ref()
            .and_then(|m| m.get("episodeId"))
            .and_then(|v| v.as_str())
            == Some(episode.as_str());
        if fp_match || ep_match {
            start = Some(i);
        }
    }
    let Some(start_idx) = start else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ev in chronological.into_iter().skip(start_idx) {
        let is_outcome = ev.op == "crash_resolved"
            || ev.op == "soft_verify_confirm"
            || ev.op == "soft_verify_reject"
            || ev.tags.iter().any(|t| t == "crash_resolved");
        if is_outcome && !out.is_empty() {
            break;
        }
        out.push(ev);
    }
    out
}

/// Best-effort map of journal ops → ActionPlan launcher ops for share/distill.
pub fn pack_event_to_launcher_action(
    ev: &PackEvent,
) -> Option<tuffbox_core::action_plan::LauncherAction> {
    let op_l = ev.op.to_lowercase();
    if op_l.contains("crash_detected")
        || op_l.contains("crash_resolved")
        || op_l.contains("soft_verify")
        || op_l == "snapshot"
        || op_l == "rollback"
    {
        return None;
    }
    let operation = ev
        .meta
        .as_ref()
        .and_then(|m| m.get("operation"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    if operation.contains("group-test") || op_l.contains("group-test") {
        return None;
    }
    let path = ev.paths.first().cloned();
    let looks_config = path
        .as_deref()
        .map(|p| {
            let p = p.replace('\\', "/").to_lowercase();
            p.contains("/config/")
                || p.ends_with(".toml")
                || p.ends_with(".json")
                || p.ends_with(".properties")
        })
        .unwrap_or(false);
    let mod_guess = event_mod_ids(ev, None)
        .into_iter()
        .next()
        .or_else(|| path.as_ref().and_then(|p| jar_file_stem(p)));
    let (op, mod_id, path_out) = if operation.contains("disable") || op_l.contains("disable") {
        ("disable_mod", mod_guess, path)
    } else if operation.contains("enable") || op_l.contains("enable") {
        // enable_mod is not in KNOWN_OPS — skip for capsule publish validation.
        return None;
    } else if operation.contains("remove")
        || op_l.contains("remove")
        || op_l == "external_remove"
        || op_l == "mod_removed"
    {
        ("remove_mod", mod_guess, path)
    } else if operation.contains("update") || op_l.contains("update") {
        ("update_mod", mod_guess, path)
    } else if looks_config
        || op_l.contains("edit")
        || op_l == "external_edit"
        || op_l == "file_edit"
    {
        ("edit_config", None, path)
    } else if operation.contains("add")
        || operation.contains("install")
        || op_l.contains("install")
        || op_l == "external_add"
        || op_l == "mod_added"
        || op_l == "mod_change"
    {
        ("install_mod", mod_guess, path)
    } else {
        return None;
    };
    Some(tuffbox_core::action_plan::LauncherAction {
        op: op.into(),
        mod_id,
        provider: None,
        project_id: None,
        version: None,
        path: path_out,
        patch_type: None,
        patch: None,
        reason: Some(ev.summary.clone()),
        risk: "medium".into(),
    })
}

/// Like [`pack_event_to_launcher_action`] but keeps `enable_mod` for trail replay
/// (enable is not a publishable KNOWN_OP).
pub fn pack_event_to_replay_action(
    ev: &PackEvent,
) -> Option<tuffbox_core::action_plan::LauncherAction> {
    let op_l = ev.op.to_lowercase();
    let operation = ev
        .meta
        .as_ref()
        .and_then(|m| m.get("operation"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    if operation.contains("group-test") || op_l.contains("group-test") {
        return None;
    }
    if operation.contains("enable") || op_l.contains("enable") {
        let path = ev.paths.first().cloned();
        let mod_id = event_mod_ids(ev, None)
            .into_iter()
            .next()
            .or_else(|| path.as_ref().and_then(|p| jar_file_stem(p)));
        return Some(tuffbox_core::action_plan::LauncherAction {
            op: "enable_mod".into(),
            mod_id,
            provider: None,
            project_id: None,
            version: None,
            path,
            patch_type: None,
            patch: None,
            reason: Some(ev.summary.clone()),
            risk: "medium".into(),
        });
    }
    pack_event_to_launcher_action(ev)
}

fn meta_string_array(meta: &Option<serde_json::Value>, key: &str) -> Vec<String> {
    meta.as_ref()
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn jar_file_stem(rel: &str) -> Option<String> {
    Path::new(rel).file_name().map(|n| {
        let n = n.to_string_lossy();
        n.strip_suffix(".disabled")
            .unwrap_or(&n)
            .strip_suffix(".jar")
            .unwrap_or(&n)
            .to_string()
    })
}

/// Canonical mod ids on a journal event: `meta.modIds`, else manifest `fileName`, else fallback.
fn event_mod_ids(ev: &PackEvent, project_dir: Option<&Path>) -> Vec<String> {
    let mut ids = meta_string_array(&ev.meta, "modIds");
    if ids.is_empty() {
        if let Some(id) = meta_str(&ev.meta, "modId") {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        if let Some(dir) = project_dir {
            for path in &ev.paths {
                if let Some(id) = mod_id_for_rel_path(dir, path) {
                    if !ids.iter().any(|x| x == &id) {
                        ids.push(id);
                    }
                }
            }
        }
    }
    if ids.is_empty() {
        if let Some(id) = fallback_mod_id_from_event(ev) {
            ids.push(id);
        }
    }
    ids
}

fn fallback_mod_id_from_event(ev: &PackEvent) -> Option<String> {
    if let Some(path) = ev.paths.first() {
        if let Some(stem) = jar_file_stem(path) {
            if !stem.is_empty() {
                return Some(stem);
            }
        }
    }
    let summary = ev.summary.trim();
    for prefix in ["Disable ", "Enable ", "Install ", "Remove ", "Update "] {
        if let Some(rest) = summary.strip_prefix(prefix) {
            let id = rest.split_whitespace().next().unwrap_or("").trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    None
}

fn mod_id_from_event(ev: &PackEvent, project_dir: Option<&Path>) -> Option<String> {
    event_mod_ids(ev, project_dir).into_iter().next()
}

/// Journal → group-test trail events (enable/disable + crash/healthy launches).
pub fn pack_events_to_trail(
    project_dir: &Path,
    events: &[PackEvent],
) -> Vec<tuffbox_core::mod_group_test::TrailEvent> {
    use tuffbox_core::mod_group_test::{TrailEvent, TrailEventKind};
    let mut out = Vec::new();
    for ev in events {
        let op_l = ev.op.to_lowercase();
        let operation = ev
            .meta
            .as_ref()
            .and_then(|m| m.get("operation"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        if op_l.contains("crash_detected") {
            out.push(TrailEvent {
                kind: TrailEventKind::Crash,
            });
            continue;
        }
        if op_l.contains("crash_resolved")
            || op_l == "soft_verify_confirm"
            || ev.tags.iter().any(|t| t == "crash_resolved")
        {
            out.push(TrailEvent {
                kind: TrailEventKind::Healthy,
            });
            continue;
        }
        if operation.contains("group-test") || op_l.contains("group-test") {
            for id in meta_string_array(&ev.meta, "disabled") {
                out.push(TrailEvent {
                    kind: TrailEventKind::Disable(id),
                });
            }
            for id in meta_string_array(&ev.meta, "enabled") {
                out.push(TrailEvent {
                    kind: TrailEventKind::Enable(id),
                });
            }
            continue;
        }
        if operation.contains("disable") || op_l.contains("disable") {
            for id in event_mod_ids(ev, Some(project_dir)) {
                out.push(TrailEvent {
                    kind: TrailEventKind::Disable(id),
                });
            }
            continue;
        }
        if operation.contains("enable") || op_l.contains("enable") {
            for id in event_mod_ids(ev, Some(project_dir)) {
                out.push(TrailEvent {
                    kind: TrailEventKind::Enable(id),
                });
            }
            continue;
        }
        out.push(TrailEvent {
            kind: TrailEventKind::Other,
        });
    }
    out
}

/// Mod ids touched by add/update/remove since the latest crash_detected (newest-first list).
pub fn recently_changed_mod_ids(project_dir: &Path, fingerprint_key: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for ev in events_between_crash_and_resolve(project_dir, fingerprint_key) {
        let op_l = ev.op.to_lowercase();
        if op_l.contains("crash") {
            continue;
        }
        for id in event_mod_ids(&ev, Some(project_dir)) {
            if !ids.iter().any(|x| x == &id) {
                ids.push(id);
            }
        }
    }
    ids
}

pub fn recent_pack_change_lines(project_dir: &Path, limit: usize) -> Vec<String> {
    recent_pack_change_lines_filtered(project_dir, limit, false)
}

/// Journal lines for Crash Planner: skip enable/disable (those become trail covering).
pub fn recent_non_toggle_pack_change_lines(project_dir: &Path, limit: usize) -> Vec<String> {
    recent_pack_change_lines_filtered(project_dir, limit, true)
}

fn recent_pack_change_lines_filtered(
    project_dir: &Path,
    limit: usize,
    skip_toggles: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    let scan = if skip_toggles {
        limit.saturating_mul(4)
    } else {
        limit.saturating_mul(2)
    };
    for ev in list_pack_events(project_dir, Some(scan)) {
        if skip_toggles && is_mod_toggle_event(&ev) {
            continue;
        }
        let paths = if ev.paths.is_empty() {
            String::new()
        } else {
            format!(" ({})", ev.paths.iter().take(3).cloned().collect::<Vec<_>>().join(", "))
        };
        lines.push(format!(
            "[{}] [{}/{}] {}{}",
            truncate_ts(&ev.ts),
            ev.actor,
            ev.op,
            ev.summary,
            paths
        ));
        if lines.len() >= limit {
            break;
        }
    }
    lines
}

fn truncate_ts(ts: &str) -> &str {
    // Prefer date+time without fractional seconds for prompts.
    if ts.len() >= 19 {
        &ts[..19]
    } else {
        ts
    }
}

pub fn load_baseline(project_dir: &Path) -> HistoryBaseline {
    let path = baseline_path(project_dir);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_baseline(project_dir: &Path, baseline: &HistoryBaseline) -> Result<(), String> {
    let dir = project_dir.join(".tuffbox");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(baseline).map_err(|e| e.to_string())?;
    std::fs::write(baseline_path(project_dir), json).map_err(|e| e.to_string())
}

/// Roots enabled by History settings category map.
pub fn roots_from_tracked(tracked: &HashMap<String, bool>) -> Vec<&'static str> {
    let mut roots = Vec::new();
    let on = |key: &str| tracked.get(key).copied().unwrap_or(false);
    if on("Mods") {
        roots.push("mods");
    }
    if on("Configs") {
        roots.extend([
            "config",
            "defaultconfigs",
            "kubejs",
            "scripts",
            "overrides",
            "options.txt",
            "servers.dat",
        ]);
    }
    if on("Shaders") {
        roots.extend(["shaderpacks", "shaders"]);
    }
    if on("Resource Packs") {
        roots.extend(["resourcepacks", "texturepacks"]);
    }
    if on("World/Data") {
        roots.extend(["datapacks", "saves"]);
    }
    roots
}

fn file_mtime_secs(path: &Path) -> u64 {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn collect_relative_files(project_dir: &Path, roots: &[&str]) -> HashMap<String, BaselineEntry> {
    let mut out = HashMap::new();
    for root in roots {
        if *root == "options.txt" || *root == "servers.dat" {
            let p = project_dir.join(root);
            if p.is_file() {
                if let Ok(meta) = p.metadata() {
                    out.insert(
                        (*root).to_string(),
                        BaselineEntry {
                            mtime: file_mtime_secs(&p),
                            size: meta.len(),
                        },
                    );
                }
            }
            continue;
        }
        let dir = project_dir.join(root);
        if !dir.is_dir() {
            continue;
        }
        walk_files(project_dir, &dir, &mut out);
    }
    out
}

fn walk_files(project_dir: &Path, dir: &Path, out: &mut HashMap<String, BaselineEntry>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            walk_files(project_dir, &path, out);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        // Skip huge binaries in baseline (still track jars in mods/).
        let Ok(meta) = path.metadata() else {
            continue;
        };
        let rel = path
            .strip_prefix(project_dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let is_jar = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("jar"))
            .unwrap_or(false);
        if meta.len() > 64 * 1024 * 1024 && !is_jar {
            continue;
        }
        out.insert(
            rel,
            BaselineEntry {
                mtime: file_mtime_secs(&path),
                size: meta.len(),
            },
        );
    }
}

fn load_first_manifest(project_dir: &Path) -> Option<tuffbox_core::ProjectManifest> {
    let rd = std::fs::read_dir(project_dir).ok()?;
    for entry in rd.flatten() {
        let p = entry.path();
        let name = p.file_name()?.to_str()?;
        if name.ends_with(".tuffbox.json") {
            return tuffbox_core::ProjectManifest::load_from_path(&p).ok();
        }
    }
    None
}

fn manifest_mod_file_names(project_dir: &Path) -> HashSet<String> {
    let mut names = HashSet::new();
    if let Some(manifest) = load_first_manifest(project_dir) {
        for m in manifest.mods {
            if let Some(fnm) = m.file_name {
                names.insert(fnm);
            }
        }
    }
    names
}

/// `mods/X.jar` and `mods/X.jar.disabled` → (`mods/X.jar`, is_disabled_path).
fn jar_toggle_pair(rel: &str) -> Option<(String, bool)> {
    let n = rel.replace('\\', "/");
    let lower = n.to_ascii_lowercase();
    if !lower.starts_with("mods/") {
        return None;
    }
    if lower.ends_with(".jar.disabled") {
        let active_len = n.len().saturating_sub(".disabled".len());
        Some((n[..active_len].to_string(), true))
    } else if lower.ends_with(".jar") {
        Some((n, false))
    } else {
        None
    }
}

fn mod_id_for_rel_path(project_dir: &Path, rel: &str) -> Option<String> {
    let (active, _) = jar_toggle_pair(rel)?;
    let want = Path::new(&active)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())?;
    let manifest = load_first_manifest(project_dir)?;
    manifest.mods.into_iter().find_map(|m| {
        let fnm = m.file_name?;
        if fnm.eq_ignore_ascii_case(&want) {
            Some(m.id)
        } else {
            None
        }
    })
}

fn scan_jar_toggle_event(project_dir: &Path, active_rel: &str, enabling: bool) -> Option<PackEvent> {
    let ids: Vec<String> = mod_id_for_rel_path(project_dir, active_rel)
        .into_iter()
        .collect();
    let label = ids.first().cloned().unwrap_or_else(|| {
        jar_file_stem(active_rel).unwrap_or_else(|| active_rel.to_string())
    });
    let operation = if enabling { "enable-mod" } else { "disable-mod" };
    let summary = if enabling {
        format!("Enable {label}")
    } else {
        format!("Disable {label}")
    };
    let path_now = if enabling {
        active_rel.to_string()
    } else {
        format!("{active_rel}.disabled")
    };
    let mut meta = serde_json::json!({ "operation": operation });
    if let Some(obj) = meta.as_object_mut() {
        if !ids.is_empty() {
            obj.insert("modIds".into(), serde_json::json!(ids.clone()));
        }
        if ids.len() == 1 {
            obj.insert("modId".into(), serde_json::json!(ids[0]));
        }
    }
    Some(PackEvent {
        id: format!("evt-toggle-{}", now_id_suffix()),
        ts: now_rfc3339(),
        actor: "scan".into(),
        op: "mod_change".into(),
        paths: vec![path_now],
        category: "Mods".into(),
        summary,
        snapshot_id: None,
        tags: vec!["external".into(), "toggle".into()],
        meta: Some(meta),
    })
}

/// Compare disk to baseline; emit events; update baseline.
pub fn scan_project_changes(
    project_dir: &Path,
    tracked: &HashMap<String, bool>,
) -> Result<ScanProjectChangesResult, String> {
    let roots = roots_from_tracked(tracked);
    let current = collect_relative_files(project_dir, &roots);
    let mut baseline = load_baseline(project_dir);
    let had_baseline = !baseline.files.is_empty();

    let mut added = 0usize;
    let mut modified = 0usize;
    let mut removed = 0usize;
    let mut jar_drift = 0usize;
    let mut new_events = Vec::new();

    if had_baseline {
        let mut added_rels: Vec<String> = Vec::new();
        let mut removed_rels: Vec<String> = Vec::new();
        let mut modified_rels: Vec<String> = Vec::new();
        for (rel, cur) in &current {
            match baseline.files.get(rel) {
                None => added_rels.push(rel.clone()),
                Some(prev) if prev.mtime != cur.mtime || prev.size != cur.size => {
                    modified_rels.push(rel.clone());
                }
                _ => {}
            }
        }
        for rel in baseline.files.keys() {
            if !current.contains_key(rel) {
                removed_rels.push(rel.clone());
            }
        }

        let mut consumed: HashSet<String> = HashSet::new();
        for rem in &removed_rels {
            let Some((active, rem_is_disabled)) = jar_toggle_pair(rem) else {
                continue;
            };
            let Some(add_rel) = added_rels.iter().find(|a| {
                jar_toggle_pair(a).is_some_and(|(act, dis)| {
                    act.eq_ignore_ascii_case(&active) && dis != rem_is_disabled
                })
            }) else {
                continue;
            };
            let add_rel = add_rel.clone();
            consumed.insert(rem.clone());
            consumed.insert(add_rel);
            let enabling = rem_is_disabled;
            if let Some(ev) = scan_jar_toggle_event(project_dir, &active, enabling) {
                let _ = append_pack_event(project_dir, ev.clone());
                new_events.push(ev);
            }
        }

        for rel in &added_rels {
            if consumed.contains(rel) {
                continue;
            }
            added += 1;
            let (summary, meta) = enrich_external_file_event(project_dir, rel, "external_add");
            let ev = PackEvent {
                id: format!("evt-add-{}", now_id_suffix()),
                ts: now_rfc3339(),
                actor: "scan".into(),
                op: "external_add".into(),
                paths: vec![rel.clone()],
                category: category_for_path(rel).into(),
                summary,
                snapshot_id: None,
                tags: vec!["external".into()],
                meta,
            };
            let _ = append_pack_event(project_dir, ev.clone());
            new_events.push(ev);
        }
        for rel in &modified_rels {
            modified += 1;
            let (summary, meta) = enrich_external_file_event(project_dir, rel, "external_edit");
            let ev = PackEvent {
                id: format!("evt-edit-{}", now_id_suffix()),
                ts: now_rfc3339(),
                actor: "scan".into(),
                op: "external_edit".into(),
                paths: vec![rel.clone()],
                category: category_for_path(rel).into(),
                summary,
                snapshot_id: None,
                tags: vec!["external".into()],
                meta,
            };
            let _ = append_pack_event(project_dir, ev.clone());
            new_events.push(ev);
        }
        for rel in &removed_rels {
            if consumed.contains(rel) {
                continue;
            }
            removed += 1;
            let (summary, meta) =
                enrich_external_file_event(project_dir, rel, "external_remove");
            let ev = PackEvent {
                id: format!("evt-rm-{}", now_id_suffix()),
                ts: now_rfc3339(),
                actor: "scan".into(),
                op: "external_remove".into(),
                paths: vec![rel.clone()],
                category: category_for_path(rel).into(),
                summary,
                snapshot_id: None,
                tags: vec!["external".into()],
                meta,
            };
            let _ = append_pack_event(project_dir, ev.clone());
            new_events.push(ev);
        }
    } else {
        // Seed content cache for editable configs so the next edit yields a real diff.
        for rel in current.keys() {
            cache_config_content(project_dir, rel);
        }
    }

    // jar_drift: mods/*.jar not listed in manifest (only after baseline exists —
    // never spam every orphan jar on first seed scan).
    let manifest_jars = manifest_mod_file_names(project_dir);
    if had_baseline {
        for rel in current.keys() {
            if !rel.starts_with("mods/") {
                continue;
            }
            let name = rel.rsplit('/').next().unwrap_or(rel);
            if !name.to_lowercase().ends_with(".jar") {
                continue;
            }
            if name.to_lowercase().ends_with(".disabled") {
                continue;
            }
            if !manifest_jars.contains(name) {
                jar_drift += 1;
                let is_new = new_events.iter().any(|e| e.paths.iter().any(|p| p == rel));
                if is_new {
                    let ev = PackEvent {
                        id: format!("evt-drift-{}", now_id_suffix()),
                        ts: now_rfc3339(),
                        actor: "scan".into(),
                        op: "jar_drift".into(),
                        paths: vec![rel.clone()],
                        category: "Mods".into(),
                        summary: format!("Jar on disk not in manifest: {name}"),
                        snapshot_id: None,
                        tags: vec!["jar_drift".into(), "external".into()],
                        meta: Some(
                            serde_json::json!({ "hint": "Import to manifest or remove orphan jar" }),
                        ),
                    };
                    let _ = append_pack_event(project_dir, ev.clone());
                    new_events.push(ev);
                }
            }
        }
    } else {
        for rel in current.keys() {
            if !rel.starts_with("mods/") {
                continue;
            }
            let name = rel.rsplit('/').next().unwrap_or(rel);
            if name.to_lowercase().ends_with(".jar")
                && !name.to_lowercase().ends_with(".disabled")
                && !manifest_jars.contains(name)
            {
                jar_drift += 1;
            }
        }
    }

    baseline.files = current;
    baseline.updated_at = now_rfc3339();
    save_baseline(project_dir, &baseline)?;

    Ok(ScanProjectChangesResult {
        events: new_events,
        baseline_updated: true,
        added,
        modified,
        removed,
        jar_drift,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_project() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tuffbox-hist-{nanos}"));
        fs::create_dir_all(dir.join("config")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        dir
    }

    fn write_mini_manifest(dir: &Path, mods: &[(&str, &str)]) {
        let mods_json: Vec<String> = mods
            .iter()
            .map(|(id, file)| {
                format!(
                    r#"{{"id":"{id}","name":"{id}","version":"1","fileName":"{file}","side":"client","source":{{"type":"local"}}}}"#
                )
            })
            .collect();
        let json = format!(
            r#"{{
  "schemaVersion": "0.1.0",
  "project": {{"id":"t","name":"t","version":"1"}},
  "minecraft": {{"version":"1.21.1"}},
  "loader": {{"type":"fabric","version":"0.16.0"}},
  "profiles": [{{"id":"client","name":"Client","side":"client"}}],
  "mods": [{}]
}}"#,
            mods_json.join(",")
        );
        fs::write(dir.join("pack.tuffbox.json"), json).unwrap();
    }

    #[test]
    fn snapshot_summary_prefers_actions_over_auto_reason() {
        let dir = temp_project();
        append_from_snapshot_with_summary(
            &dir,
            "add-mod-with-dependencies",
            "snap-1",
            &[],
            "Auto snapshot before add-mod-with-dependencies",
            &[
                "Install Cloth Config API 15.0.140".into(),
                "Install Mod Menu 11.0.3".into(),
            ],
        )
        .unwrap();
        let events = list_pack_events(&dir, Some(10));
        assert_eq!(events.len(), 1);
        assert!(events[0].summary.contains("Cloth Config"));
        assert!(!events[0].summary.contains("Auto snapshot"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn external_config_edit_includes_diff_and_keys() {
        let dir = temp_project();
        let rel = "config/demo.toml";
        fs::write(dir.join(rel), "enabled = false\nmax = 1\n").unwrap();
        cache_config_content(&dir, rel);

        fs::write(dir.join(rel), "enabled = true\nmax = 1\n").unwrap();
        let (summary, meta) = enrich_external_file_event(&dir, rel, "external_edit");
        assert!(summary.contains("enabled"), "{summary}");
        assert!(summary.starts_with("Edited "), "{summary}");
        assert!(!summary.contains("+ "), "{summary}");
        let meta = meta.expect("meta");
        let preview = meta.get("preview").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(preview, summary);
        let diff = meta.get("diff").and_then(|v| v.as_str()).unwrap_or("");
        assert!(diff.contains("- enabled = false") || diff.contains("- enabled = false\n"));
        assert!(diff.contains("+ enabled = true") || diff.contains("+ enabled = true\n"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn event_diff_text_prefers_meta_diff() {
        let ev = PackEvent {
            id: "evt-1".into(),
            ts: "2026-01-01T00:00:00Z".into(),
            actor: "scan".into(),
            op: "external_edit".into(),
            paths: vec!["config/demo.toml".into()],
            category: "Configs".into(),
            summary: "Edited demo.toml".into(),
            snapshot_id: None,
            tags: vec![],
            meta: Some(serde_json::json!({ "diff": "+ a\n- b" })),
        };
        assert_eq!(event_diff_text(&ev), "+ a\n- b");
        let mut stored = ev.clone();
        stored.meta = None;
        assert_eq!(event_diff_text(&stored), "Edited demo.toml");
    }

    #[test]
    fn external_add_comment_only_summary_is_concise() {
        let dir = temp_project();
        let rel = "kubejs/server_scripts/tuffbox_ftb_quests.js";
        fs::create_dir_all(dir.join("kubejs/server_scripts")).unwrap();
        fs::write(
            dir.join(rel),
            "// Generated / managed by TuffBox Quests\nlet x = 1;\n",
        )
        .unwrap();
        let (summary, meta) = enrich_external_file_event(&dir, rel, "external_add");
        assert!(summary.starts_with("Added "), "{summary}");
        assert!(summary.contains("tuffbox_ftb_quests.js"), "{summary}");
        assert!(!summary.contains("+ //"), "{summary}");
        assert!(!summary.contains(": added"), "{summary}");
        let meta = meta.expect("meta");
        let preview = meta.get("preview").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(preview, summary);
        let diff = meta.get("diff").and_then(|v| v.as_str()).unwrap_or("");
        assert!(diff.contains("+ //") || diff.contains("Generated"), "{diff}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn concise_event_summary_rewrites_legacy_noise() {
        let paths = vec!["kubejs/server_scripts/tuffbox_ftb_quests.js".into()];
        let legacy = "kubejs/server_scripts/tuffbox_ftb_quests.js: added (+ // Generated / managed by TuffBox Quests · Ku";
        let clean = concise_event_summary(legacy, &paths, "external_add");
        assert_eq!(clean, "Added kubejs/…/tuffbox_ftb_quests.js");
        assert!(!clean.contains("+ //"));

        let edited = concise_event_summary(
            "config/demo.toml: changed enabled",
            &["config/demo.toml".into()],
            "external_edit",
        );
        assert_eq!(edited, "Edited demo.toml · enabled");

        let install = concise_event_summary(
            "Install Mouse Tweaks 2.26",
            &["mods/mousetweaks.jar".into()],
            "mod_change",
        );
        assert_eq!(install, "Install Mouse Tweaks 2.26");
    }

    #[test]
    fn config_preview_skips_comment_dump() {
        let preview = config_change_preview(
            "--- a\n+++ b\n+ // Generated / managed by TuffBox\n+ let x = 1;\n",
        );
        assert!(!preview.contains("+ //"), "{preview}");
        assert!(
            preview.starts_with("content updated") || preview.contains("changed"),
            "{preview}"
        );
    }

    #[test]
    fn sync_baseline_avoids_false_external_add() {
        let dir = temp_project();
        let mut tracked = HashMap::new();
        tracked.insert("Mods".into(), true);
        tracked.insert("Configs".into(), true);

        // Seed baseline empty → first scan should not emit adds.
        let seed = scan_project_changes(&dir, &tracked).unwrap();
        assert_eq!(seed.added, 0);
        assert!(seed.events.is_empty());

        let jar_rel = "mods/cloth-config.jar".to_string();
        fs::write(dir.join(&jar_rel), b"fake-jar").unwrap();
        // Launcher syncs baseline after install.
        sync_baseline_paths(&dir, &[jar_rel.clone()]).unwrap();

        let after = scan_project_changes(&dir, &tracked).unwrap();
        assert_eq!(after.added, 0, "launcher jar must not appear as external_add");
        assert!(
            after
                .events
                .iter()
                .all(|e| e.op != "external_add" || !e.paths.contains(&jar_rel))
        );

        // Truly external add still detected.
        let orphan = "mods/orphan.jar".to_string();
        fs::write(dir.join(&orphan), b"orphan").unwrap();
        let external = scan_project_changes(&dir, &tracked).unwrap();
        assert_eq!(external.added, 1);
        assert!(external.events.iter().any(|e| e.op == "external_add"));

        // External remove.
        fs::remove_file(dir.join(&orphan)).unwrap();
        let removed = scan_project_changes(&dir, &tracked).unwrap();
        assert_eq!(removed.removed, 1);
        assert!(removed.events.iter().any(|e| e.op == "external_remove"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_preview_extracts_assignment_keys() {
        let preview = config_change_preview(
            "  keep = 1\n- enabled = false\n+ enabled = true\n- [section]\n+ [section]\n",
        );
        assert!(preview.contains("enabled"), "{preview}");
    }

    #[test]
    fn normalize_fix_method_mapping() {
        assert_eq!(normalize_fix_method(Some("ai_action_plan")), "ai");
        assert_eq!(normalize_fix_method(Some("heuristic")), "heuristic");
        assert_eq!(normalize_fix_method(Some("kb_only")), "kb");
        assert_eq!(normalize_fix_method(Some("swarm")), "swarm");
        assert_eq!(normalize_fix_method(Some("manual")), "manual");
        assert_eq!(normalize_fix_method(None), "unknown");
        assert_eq!(actor_for_plan_source(Some("heuristic")), "launcher");
        assert_eq!(actor_for_plan_source(Some("ai")), "ai");
        assert_eq!(actor_for_plan_source(Some("manual")), "user");
    }

    #[test]
    fn crash_detected_writes_episode_meta() {
        let dir = temp_project();
        let ep = append_crash_detected(&dir, "fp-test-key", Some(1), Some("logs/latest.log"), "boom")
            .unwrap();
        assert!(ep.starts_with("ep-"));
        let events = list_pack_events(&dir, Some(10));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].op, "crash_detected");
        assert_eq!(meta_str(&events[0].meta, "fingerprintKey").as_deref(), Some("fp-test-key"));
        assert_eq!(meta_str(&events[0].meta, "episodeId").as_deref(), Some(ep.as_str()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn events_between_crash_and_resolve_keeps_player_trail() {
        let dir = temp_project();
        let _ = append_crash_detected(&dir, "fp-trail", Some(1), None, "boom").unwrap();
        record_mod_change_event(
            &dir,
            "disable-mod",
            None,
            &["Disable sodium".into()],
            &["mods/sodium.jar.disabled".into()],
            &[],
        )
        .unwrap();
        let trail = events_between_crash_and_resolve(&dir, "fp-trail");
        assert!(trail.len() >= 2, "{trail:?}");
        assert_eq!(trail[0].op, "crash_detected");
        assert!(trail.iter().any(|e| e.op == "mod_change"));
        let actions: Vec<_> = trail
            .iter()
            .filter_map(pack_event_to_launcher_action)
            .collect();
        assert!(
            actions.iter().any(|a| a.op == "disable_mod"),
            "{actions:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pack_events_to_trail_includes_enable_and_healthy() {
        let dir = temp_project();
        let _ = append_crash_detected(&dir, "fp-gt", Some(1), None, "boom").unwrap();
        record_mod_change_event(
            &dir,
            "disable-mod",
            None,
            &["Disable foo".into()],
            &["mods/foo.jar.disabled".into()],
            &[],
        )
        .unwrap();
        record_mod_change_event(
            &dir,
            "enable-mod",
            None,
            &["Enable foo".into()],
            &["mods/foo.jar".into()],
            &[],
        )
        .unwrap();
        let trail = events_between_crash_and_resolve(&dir, "fp-gt");
        let kinds = pack_events_to_trail(&dir, &trail);
        assert!(kinds.iter().any(|e| matches!(e.kind, tuffbox_core::mod_group_test::TrailEventKind::Crash)));
        assert!(kinds.iter().any(|e| matches!(e.kind, tuffbox_core::mod_group_test::TrailEventKind::Disable(_))));
        assert!(kinds.iter().any(|e| matches!(e.kind, tuffbox_core::mod_group_test::TrailEventKind::Enable(_))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_non_toggle_lines_drop_enable_disable() {
        let dir = temp_project();
        record_mod_change_event(
            &dir,
            "disable-mod",
            None,
            &["Disable foo".into()],
            &["mods/foo.jar.disabled".into()],
            &[],
        )
        .unwrap();
        record_mod_change_event(
            &dir,
            "enable-mod",
            None,
            &["Enable foo".into()],
            &["mods/foo.jar".into()],
            &[],
        )
        .unwrap();
        record_mod_change_event(
            &dir,
            "update-mod",
            None,
            &["Update sodium".into()],
            &["mods/sodium.jar".into()],
            &[],
        )
        .unwrap();
        let all = recent_pack_change_lines(&dir, 12);
        assert!(all.iter().any(|l| l.contains("Disable foo")), "{all:?}");
        let filtered = recent_non_toggle_pack_change_lines(&dir, 12);
        assert!(
            filtered.iter().any(|l| l.contains("Update sodium")),
            "{filtered:?}"
        );
        assert!(
            !filtered.iter().any(|l| l.contains("Enable foo") || l.contains("Disable foo")),
            "{filtered:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_stores_manifest_mod_id_not_jar_stem() {
        let dir = temp_project();
        write_mini_manifest(&dir, &[("sodium", "sodium-0.5.8.jar")]);
        record_mod_change_event(
            &dir,
            "disable-mod",
            None,
            &["Disable Sodium 0.5.8".into()],
            &["mods/sodium-0.5.8.jar.disabled".into()],
            &["sodium".into()],
        )
        .unwrap();
        let events = list_pack_events(&dir, Some(10));
        assert_eq!(events.len(), 1);
        assert_eq!(
            mod_id_from_event(&events[0], Some(&dir)).as_deref(),
            Some("sodium")
        );
        assert_eq!(event_mod_ids(&events[0], Some(&dir)), vec!["sodium"]);
        assert_ne!(
            mod_id_from_event(&events[0], Some(&dir)).as_deref(),
            Some("sodium-0.5.8")
        );

        let mut legacy = events[0].clone();
        legacy.meta = Some(serde_json::json!({ "operation": "disable-mod" }));
        assert_eq!(
            mod_id_from_event(&legacy, Some(&dir)).as_deref(),
            Some("sodium"),
            "manifest lookup by fileName should beat jar stem"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn trail_expands_mod_ids_and_group_test_arrays() {
        let dir = temp_project();
        record_mod_change_event(
            &dir,
            "disable-mod",
            None,
            &["Disable a and b".into()],
            &[],
            &["a".into(), "b".into()],
        )
        .unwrap();
        record_group_test_layout_event(
            &dir,
            None,
            &["c".into()],
            &["a".into(), "b".into()],
            &[],
        )
        .unwrap();
        let events = list_pack_events(&dir, Some(10));
        let kinds = pack_events_to_trail(&dir, &events);
        let disables: Vec<_> = kinds
            .iter()
            .filter_map(|e| match &e.kind {
                tuffbox_core::mod_group_test::TrailEventKind::Disable(id) => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let enables: Vec<_> = kinds
            .iter()
            .filter_map(|e| match &e.kind {
                tuffbox_core::mod_group_test::TrailEventKind::Enable(id) => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert!(disables.contains(&"a") && disables.contains(&"b"), "{disables:?}");
        assert!(enables.contains(&"c"), "{enables:?}");
        assert!(
            events.iter().any(|e| {
                meta_str(&e.meta, "operation").as_deref() == Some("group-test-layout")
                    && is_mod_toggle_event(e)
            }),
            "group-test-layout must count as a toggle"
        );
        assert!(
            pack_event_to_launcher_action(
                events
                    .iter()
                    .find(|e| meta_str(&e.meta, "operation").as_deref() == Some("group-test-layout"))
                    .unwrap()
            )
            .is_none()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_pairs_jar_disabled_into_toggle() {
        let dir = temp_project();
        write_mini_manifest(&dir, &[("cloth-config", "cloth-config.jar")]);
        let mut tracked = HashMap::new();
        tracked.insert("Mods".into(), true);
        tracked.insert("Configs".into(), true);

        let jar_rel = "mods/cloth-config.jar";
        fs::write(dir.join(jar_rel), b"fake-jar").unwrap();
        let seed = scan_project_changes(&dir, &tracked).unwrap();
        assert!(seed.events.is_empty());

        fs::rename(dir.join(jar_rel), dir.join("mods/cloth-config.jar.disabled")).unwrap();
        let toggled = scan_project_changes(&dir, &tracked).unwrap();
        assert_eq!(toggled.added, 0, "{:?}", toggled.events);
        assert_eq!(toggled.removed, 0, "{:?}", toggled.events);
        assert_eq!(toggled.events.len(), 1, "{:?}", toggled.events);
        assert_eq!(toggled.events[0].op, "mod_change");
        assert_eq!(
            meta_str(&toggled.events[0].meta, "operation").as_deref(),
            Some("disable-mod")
        );
        assert_eq!(
            event_mod_ids(&toggled.events[0], Some(&dir)),
            vec!["cloth-config"]
        );
        assert!(!toggled.events.iter().any(|e| e.op == "external_add" || e.op == "external_remove"));

        fs::rename(
            dir.join("mods/cloth-config.jar.disabled"),
            dir.join(jar_rel),
        )
        .unwrap();
        let enabled = scan_project_changes(&dir, &tracked).unwrap();
        assert_eq!(enabled.events.len(), 1, "{:?}", enabled.events);
        assert_eq!(
            meta_str(&enabled.events[0].meta, "operation").as_deref(),
            Some("enable-mod")
        );
        assert_eq!(
            event_mod_ids(&enabled.events[0], Some(&dir)),
            vec!["cloth-config"]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn edit_config_journals_file_edit() {
        let dir = temp_project();
        record_mod_change_event(
            &dir,
            "edit-config",
            None,
            &["edited config config/demo.toml".into()],
            &["config/demo.toml".into()],
            &[],
        )
        .unwrap();
        let events = list_pack_events(&dir, Some(10));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].op, "file_edit");
        assert_eq!(
            meta_str(&events[0].meta, "operation").as_deref(),
            Some("edit-config")
        );
        assert_eq!(events[0].category, "Configs");
        let _ = fs::remove_dir_all(&dir);
    }
}
