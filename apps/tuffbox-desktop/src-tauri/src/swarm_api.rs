//! TuffBox desktop commands: pending plans, capsules, co-occurrence, share prompt,
//! and post-resolution distill (AI → Confirm → publish).

use crate::integrations;
use serde_json::json;
use std::path::{Path, PathBuf};
use tuffbox_core::action_plan::ActionPlan;
use tuffbox_core::crash_kb::{AuthorCaseInput, CrashCase};
use tuffbox_core::swarm::{
    clear_pending_action_plan, format_cooccurrence_for_prompt, load_pending_action_plan,
    mark_pack_observation, maybe_write_pending_from_score, merge_cooccurrence_pairs,
    normalize_mod_id_list, pack_mod_ids, plan_pack_observation,
    record_mod_set_cooccurrence, suggest_by_group_affinity, top_cooccurrence_groups,
    top_cooccurrence_pairs, write_pending_action_plan, ExperienceCapsule, ModPairStat,
    STRONG_MATCH_THRESHOLD,
};
use tuffbox_core::{ContentProvider, ProjectManifest, Snapshot, SnapshotMeta, SnapshotStore};
use tauri::Emitter;

fn resolve_manifest(path: &str) -> Result<PathBuf, String> {
    crate::resolve_manifest_path(path)
}

fn manifest_parent(path: &str) -> Result<PathBuf, String> {
    crate::manifest_parent(path)
}

fn load_project_manifest(path: &str) -> Result<(PathBuf, ProjectManifest), String> {
    let manifest_path = resolve_manifest(path)?;
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    Ok((manifest_path, manifest))
}

fn last_crash_fix_path(project_dir: &Path) -> PathBuf {
    project_dir
        .join(".tuffbox")
        .join("swarm")
        .join("last_crash_fix.json")
}

fn group_test_session_path(project_dir: &Path) -> PathBuf {
    project_dir
        .join(".tuffbox")
        .join("swarm")
        .join("mod_group_test.json")
}

pub fn load_group_test_session(
    project_dir: &Path,
) -> Option<tuffbox_core::mod_group_test::GroupTestSession> {
    let raw = std::fs::read_to_string(group_test_session_path(project_dir)).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn save_group_test_session(
    project_dir: &Path,
    session: &tuffbox_core::mod_group_test::GroupTestSession,
) -> Result<(), String> {
    let path = group_test_session_path(project_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(session).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

pub fn clear_group_test_session(project_dir: &Path) {
    let _ = std::fs::remove_file(group_test_session_path(project_dir));
}

fn decode_player_share(
    project_dir: &Path,
    fingerprint_key: &str,
    universe: &[String],
) -> tuffbox_core::mod_group_test::DecodedTrail {
    let events = crate::pack_events::events_between_crash_and_resolve(project_dir, fingerprint_key);
    let mut trail = crate::pack_events::pack_events_to_trail(project_dir, &events);
    if !trail
        .iter()
        .any(|e| matches!(e.kind, tuffbox_core::mod_group_test::TrailEventKind::Healthy))
    {
        trail.push(tuffbox_core::mod_group_test::TrailEvent {
            kind: tuffbox_core::mod_group_test::TrailEventKind::Healthy,
        });
    }
    let mut decoded = tuffbox_core::mod_group_test::decode_player_trail(universe, &trail);
    let extra: Vec<_> = events
        .iter()
        .filter_map(crate::pack_events::pack_event_to_launcher_action)
        .filter(|a| a.op != "disable_mod" && a.op != "enable_mod")
        .collect();
    decoded.extra_actions = extra;
    decoded
}

/// Soft-verify: healthy session must hold this long before success (seconds).
const SOFT_VERIFY_MIN_SECS: u64 = 180;

/// Honest default when the player (not built-in AI) changed the pack after a crash.
/// We only know the action trail crash → healthy launch — not which step fixed it.
pub const PLAYER_TRAIL_EXPLANATION: &str = "Actions recorded between crash and successful launch. Which change fixed the crash is unknown.";

fn is_known_cause_source(source: Option<&str>) -> bool {
    let Some(s) = source.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let s = s.to_ascii_lowercase();
    s == "ai"
        || s == "kb"
        || s == "hybrid"
        || s == "swarm"
        || s == "network"
        || s == "group_test"
        || s.starts_with("ai_")
        || s.starts_with("distill")
        || s.contains("action_plan")
        || s == "crash_assistant"
        || s.contains("pending_network")
}

fn is_player_trail_source(source: Option<&str>) -> bool {
    !is_known_cause_source(source)
}

fn launcher_action_dedupe_key(a: &tuffbox_core::action_plan::LauncherAction) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        a.op,
        a.mod_id.as_deref().unwrap_or(""),
        a.project_id.as_deref().unwrap_or(""),
        a.version.as_deref().unwrap_or(""),
        a.path.as_deref().unwrap_or("")
    )
}

fn merge_launcher_actions(
    existing: &[tuffbox_core::action_plan::LauncherAction],
    incoming: Vec<tuffbox_core::action_plan::LauncherAction>,
) -> Vec<tuffbox_core::action_plan::LauncherAction> {
    let mut out = existing.to_vec();
    let mut seen: std::collections::HashSet<String> = out
        .iter()
        .map(launcher_action_dedupe_key)
        .collect();
    for a in incoming {
        let key = launcher_action_dedupe_key(&a);
        if seen.insert(key) {
            out.push(a);
        }
    }
    out
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastCrashFixMarker {
    pub snapshot_id: String,
    pub fingerprint_key: String,
    pub plan_source: Option<String>,
    pub matched_case_ids: Vec<String>,
    pub human_explanation: String,
    pub actions: Vec<tuffbox_core::action_plan::LauncherAction>,
    pub created_at: String,
    /// Unix epoch seconds when the fix was applied — used to require a *newer*
    /// healthy `latest.log` before confirming resolution.
    #[serde(default)]
    pub created_at_unix: Option<u64>,
    #[serde(default)]
    pub shared: bool,
    /// Set once a successful launch / healthy diagnose confirms the fix worked.
    #[serde(default)]
    pub resolved: bool,
    #[serde(default)]
    pub resolved_at: Option<String>,
    /// Absolute path to the project manifest (for rollback / soft-verify events).
    #[serde(default)]
    pub manifest_path: Option<String>,
    /// When a post-fix healthy log was first observed (playtime gate).
    #[serde(default)]
    pub soft_verify_started_unix: Option<u64>,
    /// Explicit snapshot rollback after apply — counts as soft-verify reject.
    #[serde(default)]
    pub rolled_back: bool,
    /// Soft-verify ended in failure (crash) without rollback.
    #[serde(default)]
    pub soft_verify_failed: bool,
    /// Whether UI already received a soft-verify outcome event for this marker.
    #[serde(default)]
    pub vote_emitted: bool,
    /// History episode id linking crash_detected → fix → outcome.
    #[serde(default)]
    pub episode_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashFixBanner {
    pub snapshot_id: String,
    pub fingerprint_key: String,
    pub plan_source: Option<String>,
    pub human_explanation: String,
    pub matched_case_ids: Vec<String>,
    pub actions_summary: Vec<String>,
    pub created_at: String,
    pub resolved: bool,
    pub rolled_back: bool,
    pub soft_verify_started_unix: Option<u64>,
    pub min_playtime_secs: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashResolutionRecord {
    pub id: String,
    pub fingerprint_key: String,
    pub snapshot_id: String,
    pub plan_source: Option<String>,
    pub human_explanation: String,
    pub matched_case_ids: Vec<String>,
    pub actions_summary: Vec<String>,
    pub verified_by: String,
    pub created_at: String,
    pub resolved_at: String,
}

fn resolutions_path(project_dir: &Path) -> PathBuf {
    project_dir
        .join(".tuffbox")
        .join("history")
        .join("resolutions.jsonl")
}

pub fn list_crash_resolutions(project_dir: &Path) -> Result<Vec<CrashResolutionRecord>, String> {
    let path = resolutions_path(project_dir);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(rec) = serde_json::from_str::<CrashResolutionRecord>(line) {
            out.push(rec);
        }
    }
    out.reverse(); // newest first
    Ok(out)
}

fn append_crash_resolution(project_dir: &Path, rec: &CrashResolutionRecord) -> Result<(), String> {
    let path = resolutions_path(project_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let line = serde_json::to_string(rec).map_err(|e| e.to_string())?;
    writeln!(file, "{line}").map_err(|e| e.to_string())
}

fn marker_created_unix(marker: &LastCrashFixMarker) -> Option<u64> {
    marker
        .created_at_unix
        .or_else(|| tuffbox_core::time_util::parse_rfc3339_unix_secs(&marker.created_at))
}

fn file_mtime_secs(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Human-readable one-liner for History (how the user fixed the error).
pub fn format_launcher_action_summary(a: &tuffbox_core::action_plan::LauncherAction) -> String {
    let target = a
        .mod_id
        .as_deref()
        .or(a.project_id.as_deref())
        .or(a.path.as_deref())
        .filter(|s| !s.is_empty())
        .unwrap_or("-");
    match a.op.as_str() {
        "disable_mod" => format!("Disabled {target}"),
        "remove_mod" => format!("Removed {target}"),
        "update_mod" | "change_mod_version" => {
            if let Some(v) = a.version.as_deref().filter(|s| !s.is_empty()) {
                format!("Updated {target} → {v}")
            } else {
                format!("Updated {target}")
            }
        }
        "reinstall_mod" => format!("Reinstalled {target}"),
        "install_mod" => format!("Installed {target}"),
        "edit_config" => format!("Edited config {target}"),
        "raise_memory" => "Raised allocated memory".into(),
        "accept_eula" => "Accepted EULA".into(),
        "change_port" => "Changed server port".into(),
        "auto_java" => "Selected compatible Java".into(),
        "update_loader" => "Updated loader".into(),
        other => {
            if target == "-" {
                other.to_string()
            } else {
                format!("{other} {target}")
            }
        }
    }
}

fn pending_fix_marker_exists(project_dir: &Path) -> bool {
    let path = last_crash_fix_path(project_dir);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(marker) = serde_json::from_str::<LastCrashFixMarker>(&raw) else {
        return false;
    };
    !marker.resolved && !marker.rolled_back && !marker.soft_verify_failed
}

fn load_crash_fix_marker(project_dir: &Path) -> Result<Option<LastCrashFixMarker>, String> {
    let path = last_crash_fix_path(project_dir);
    if !path.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let marker: LastCrashFixMarker =
        serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok(Some(marker))
}

/// Public read of the last crash-fix marker (for snapshot detail synthesis).
pub fn peek_last_crash_fix_marker(
    project_dir: &Path,
) -> Result<Option<LastCrashFixMarker>, String> {
    load_crash_fix_marker(project_dir)
}

fn save_crash_fix_marker(project_dir: &Path, marker: &LastCrashFixMarker) -> Result<(), String> {
    let path = last_crash_fix_path(project_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(marker).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn running_playtime_for_manifest(manifest_path: &Path) -> Option<u64> {
    let id = manifest_path.to_string_lossy();
    let now = tuffbox_core::time_util::unix_now_secs();
    tuffbox_core::process::list_running()
        .into_iter()
        .filter(|p| p.id == id)
        .map(|p| now.saturating_sub(p.started_at))
        .max()
}

fn bypass_playtime_for(verified_by: &str) -> bool {
    verified_by == "diagnose_healthy"
}

fn emit_soft_verify_outcome(
    app: &tauri::AppHandle,
    manifest_path: &str,
    marker: &LastCrashFixMarker,
    outcome: &str,
    reason: &str,
) {
    let _ = app.emit(
        "tuffbox:soft-verify-outcome",
        json!({
            "path": manifest_path,
            "outcome": outcome,
            "reason": reason,
            "snapshotId": marker.snapshot_id,
            "fingerprintKey": marker.fingerprint_key,
            "matchedCaseIds": marker.matched_case_ids,
            "planSource": marker.plan_source,
            "humanExplanation": marker.human_explanation,
        }),
    );
}

/// When a crash fix was applied and the game later launches cleanly, record a
/// durable "resolved" history entry so the History tab shows the successful fix.
///
/// Soft-verify gates:
/// 1. `latest.log` newer than the fix marker and healthy (no fresh crash markers)
/// 2. Stable window ≥ [`SOFT_VERIFY_MIN_SECS`] (running playtime or elapsed since healthy)
/// 3. Marker not rolled back
pub fn maybe_confirm_crash_resolution(
    manifest_path: &Path,
    verified_by: &str,
) -> Result<Option<CrashResolutionRecord>, String> {
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest path has no parent".to_string())?;
    let mut marker = match load_crash_fix_marker(project_dir)? {
        Some(m) => m,
        None => return Ok(None),
    };
    if marker.resolved || marker.rolled_back || marker.soft_verify_failed {
        return Ok(None);
    }
    // Open crash episode with no recorded actions yet — wait for player/AI edits.
    if marker.actions.is_empty() {
        return Ok(None);
    }

    let latest = project_dir.join("logs").join("latest.log");
    if !latest.is_file() {
        return Ok(None);
    }
    let Some(marker_secs) = marker_created_unix(&marker) else {
        return Ok(None);
    };
    let Some(log_mtime) = file_mtime_secs(&latest) else {
        return Ok(None);
    };
    // Log must be from a session that started after the user applied the fix.
    if log_mtime <= marker_secs {
        return Ok(None);
    }

    let latest_log = tuffbox_core::process::read_log_tail(&latest, 900).unwrap_or_default();
    if !tuffbox_core::crash::log_indicates_healthy_session(&latest_log) {
        return Ok(None);
    }

    let now = tuffbox_core::time_util::unix_now_secs();
    let running_secs = running_playtime_for_manifest(manifest_path).unwrap_or(0);
    let bypass = bypass_playtime_for(verified_by);
    // Only advance soft-verify while the game process is alive.
    // Reset the clock on quit so a 30s session + idle wall-clock cannot confirm.
    if running_secs == 0 && !bypass {
        if marker.soft_verify_started_unix.is_some() {
            marker.soft_verify_started_unix = None;
            save_crash_fix_marker(project_dir, &marker)?;
        }
        return Ok(None);
    }
    if marker.soft_verify_started_unix.is_none() {
        // Backdate by known process uptime so the gate matches this session.
        marker.soft_verify_started_unix = Some(now.saturating_sub(running_secs));
        save_crash_fix_marker(project_dir, &marker)?;
    }
    let started = marker.soft_verify_started_unix.unwrap_or(now);
    let elapsed_this_session = now.saturating_sub(started);
    let playtime_ok =
        running_secs >= SOFT_VERIFY_MIN_SECS || elapsed_this_session >= SOFT_VERIFY_MIN_SECS;
    // Diagnose path may confirm without waiting full playtime (user explicitly
    // checked health); launch path always requires the soft-verify window.
    if !playtime_ok && !bypass {
        return Ok(None);
    }

    let now_rfc = tuffbox_core::time_util::rfc3339_now();
    // Group-test session wins when verify passed. Otherwise decode the trail
    // as group tests (do not keep every toggle).
    if let Some(session) = load_group_test_session(project_dir) {
        if session.verified {
            if let Some(plan) = session.share_plan() {
                marker.plan_source = Some("group_test".into());
                marker.human_explanation = plan.human_explanation.clone();
                marker.actions = plan.actions.clone();
                save_crash_fix_marker(project_dir, &marker)?;
            }
        }
    }
    if is_player_trail_source(marker.plan_source.as_deref()) {
        let universe = ProjectManifest::load_from_path(manifest_path)
            .map(|m| pack_mod_ids(&m))
            .unwrap_or_default();
        let decoded = decode_player_share(project_dir, &marker.fingerprint_key, &universe);
        let mut actions = decoded.disable_actions();
        actions.extend(decoded.extra_actions.clone());
        marker.actions = actions;
        marker.human_explanation = decoded.explanation.clone();
        marker.plan_source = Some("player_trail".into());
        save_crash_fix_marker(project_dir, &marker)?;
    }

    let mut actions_summary: Vec<String> = marker
        .actions
        .iter()
        .map(format_launcher_action_summary)
        .collect();
    if actions_summary.is_empty() && !marker.human_explanation.trim().is_empty() {
        actions_summary.push(marker.human_explanation.clone());
    }

    let rec = CrashResolutionRecord {
        id: format!(
            "resolved-{}",
            tuffbox_core::time_util::compact_now()
        ),
        fingerprint_key: marker.fingerprint_key.clone(),
        snapshot_id: marker.snapshot_id.clone(),
        plan_source: marker.plan_source.clone(),
        human_explanation: marker.human_explanation.clone(),
        matched_case_ids: marker.matched_case_ids.clone(),
        actions_summary,
        verified_by: verified_by.to_string(),
        created_at: marker.created_at.clone(),
        resolved_at: now_rfc.clone(),
    };
    append_crash_resolution(project_dir, &rec)?;

    // Snapshot so History / ChangeHistory surfaces a crash_resolved card.
    let lockfile_path = manifest_path.with_extension("lock.json");
    let lockfile_path = if lockfile_path.exists() {
        Some(lockfile_path)
    } else {
        None
    };
    let fp_prefix: String = rec.fingerprint_key.chars().take(24).collect();
    let how = if rec.actions_summary.is_empty() {
        rec.human_explanation.clone()
    } else {
        rec.actions_summary.join("; ")
    };
    let meta = SnapshotMeta {
        tags: vec!["crash_resolved".into(), "crash_fix".into()],
        crash_fingerprint_key: Some(rec.fingerprint_key.clone()),
        report_id: None,
        plan_source: rec.plan_source.clone().or_else(|| Some(verified_by.into())),
        matched_case_ids: rec.matched_case_ids.clone(),
        operation: "crash_resolved".into(),
        actions_summary: rec.actions_summary.clone(),
        actor: Some(
            crate::pack_events::actor_for_plan_source(rec.plan_source.as_deref()).to_string(),
        ),
        ..Default::default()
    };
    let store = SnapshotStore::new(project_dir);
    let resolved_snap = store.create_with_meta(
        format!("crash-resolved-{fp_prefix}"),
        format!(
            "Resolved crash ({verified_by}): {}",
            tuffbox_core::crash_kb::truncate_at_char_boundary(&how, 180)
        ),
        manifest_path,
        lockfile_path.as_ref(),
        &[] as &[std::path::PathBuf],
        meta,
    );
    let episode_id = marker
        .episode_id
        .clone()
        .unwrap_or_else(|| crate::pack_events::episode_id_for_fingerprint(&rec.fingerprint_key));
    if let Ok(ref snap) = resolved_snap {
        let _ = crate::pack_events::append_from_snapshot_with_episode(
            project_dir,
            "crash_resolved",
            &snap.id,
            &[] as &[std::path::PathBuf],
            &format!("Crash resolved ({verified_by})"),
            &rec.actions_summary,
            Some(&episode_id),
            Some(&rec.fingerprint_key),
            rec.plan_source.as_deref(),
        );
    }
    let _ = crate::pack_events::append_crash_outcome_event(
        project_dir,
        "crash_resolved",
        &episode_id,
        &rec.fingerprint_key,
        rec.plan_source.as_deref(),
        resolved_snap.as_ref().ok().map(|s| s.id.as_str()),
        &format!("Crash resolved ({verified_by}): {}", tuffbox_core::crash_kb::truncate_at_char_boundary(&how, 120)),
    );

    marker.resolved = true;
    marker.resolved_at = Some(now_rfc);
    marker.episode_id = Some(episode_id);
    save_crash_fix_marker(project_dir, &marker)?;

    Ok(Some(rec))
}

/// Mark a pending soft-verify as failed (crash / rollback) and emit outcome for UI votes.
pub fn fail_soft_verify(
    app: &tauri::AppHandle,
    manifest_path: &Path,
    reason: &str,
) -> Result<Option<LastCrashFixMarker>, String> {
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest path has no parent".to_string())?;
    let mut marker = match load_crash_fix_marker(project_dir)? {
        Some(m) => m,
        None => return Ok(None),
    };
    if marker.resolved || marker.rolled_back || marker.soft_verify_failed {
        return Ok(None);
    }
    let path_str = manifest_path.to_string_lossy().to_string();
    if reason == "rollback" {
        marker.rolled_back = true;
    } else {
        marker.soft_verify_failed = true;
    }
    let episode_id = marker
        .episode_id
        .clone()
        .unwrap_or_else(|| crate::pack_events::episode_id_for_fingerprint(&marker.fingerprint_key));
    marker.episode_id = Some(episode_id.clone());
    let outcome_op = if reason == "rollback" {
        "crash_fix_rollback"
    } else {
        "crash_fix_rejected"
    };
    let _ = crate::pack_events::append_crash_outcome_event(
        project_dir,
        outcome_op,
        &episode_id,
        &marker.fingerprint_key,
        marker.plan_source.as_deref(),
        Some(&marker.snapshot_id),
        &format!(
            "Crash fix {} ({})",
            if reason == "rollback" { "rolled back" } else { "rejected" },
            reason
        ),
    );
    if !marker.vote_emitted {
        marker.vote_emitted = true;
        save_crash_fix_marker(project_dir, &marker)?;
        emit_soft_verify_outcome(app, &path_str, &marker, "reject", reason);
    } else {
        save_crash_fix_marker(project_dir, &marker)?;
    }
    Ok(Some(marker))
}

fn emit_success_soft_verify(
    app: &tauri::AppHandle,
    manifest_path: &str,
    project_dir: &Path,
    rec: &CrashResolutionRecord,
) {
    let Ok(Some(mut marker)) = load_crash_fix_marker(project_dir) else {
        return;
    };
    if marker.vote_emitted {
        return;
    }
    marker.vote_emitted = true;
    let _ = save_crash_fix_marker(project_dir, &marker);
    emit_soft_verify_outcome(app, manifest_path, &marker, "confirm", &rec.verified_by);
}

/// Whether swarm share/distill UI should run after a verified resolution.
pub fn should_offer_distill() -> bool {
    let swarm = integrations::swarm_settings();
    swarm.enabled && swarm.share_prompts_enabled
}

/// Notify the UI to open the distill → Confirm dialog (no network write yet).
pub fn emit_distill_resolution(
    app: &tauri::AppHandle,
    manifest_path: &str,
    rec: &CrashResolutionRecord,
) {
    if !should_offer_distill() {
        return;
    }
    let _ = app.emit(
        "tuffbox:distill-resolution",
        json!({
            "path": manifest_path,
            "resolution": rec,
        }),
    );
}

/// Poll `latest.log` after launch until soft-verify succeeds
/// (playtime gate + healthy session) or the pending marker disappears / times out.
fn spawn_crash_resolution_watcher(app: tauri::AppHandle, manifest_path: PathBuf) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static ACTIVE: Mutex<Option<HashSet<String>>> = Mutex::new(None);

    let path_key = manifest_path.to_string_lossy().to_string();
    {
        let mut guard = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        let set = guard.get_or_insert_with(HashSet::new);
        if !set.insert(path_key.clone()) {
            // Already watching this project.
            return;
        }
    }

    std::thread::Builder::new()
        .name("tuffbox-crash-resolution".into())
        .spawn(move || {
            let clear_active = || {
                if let Ok(mut guard) = ACTIVE.lock() {
                    if let Some(set) = guard.as_mut() {
                        set.remove(&path_key);
                    }
                }
            };
            let Some(project_dir) = manifest_path.parent().map(|p| p.to_path_buf()) else {
                clear_active();
                return;
            };
            let path_str = manifest_path.to_string_lossy().to_string();
            // ~15 minutes: first boot + soft-verify playtime window.
            for _ in 0..180 {
                std::thread::sleep(std::time::Duration::from_secs(5));
                if !pending_fix_marker_exists(&project_dir) {
                    clear_active();
                    return;
                }
                // Mid-watch crash: only fail on explicit crash dump markers.
                // "Not healthy yet" (mid-boot / relaunch truncate) must NOT reject —
                // otherwise a second Play click after a good boot false-fails soft-verify.
                if let Ok(Some(marker)) = load_crash_fix_marker(&project_dir) {
                    if marker.soft_verify_started_unix.is_some() {
                        if let Some(marker_secs) = marker_created_unix(&marker) {
                            let latest = project_dir.join("logs").join("latest.log");
                            if let Some(log_mtime) = file_mtime_secs(&latest) {
                                if log_mtime > marker_secs {
                                    let latest_log =
                                        tuffbox_core::process::read_log_tail(&latest, 900)
                                            .unwrap_or_default();
                                    if tuffbox_core::crash::log_has_fresh_crash_markers(
                                        &latest_log,
                                    ) {
                                        let _ = fail_soft_verify(
                                            &app,
                                            &manifest_path,
                                            "post_fix_crash",
                                        );
                                        clear_active();
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
                match maybe_confirm_crash_resolution(&manifest_path, "successful_launch") {
                    Ok(Some(rec)) => {
                        emit_success_soft_verify(&app, &path_str, &project_dir, &rec);
                        emit_distill_resolution(&app, &path_str, &rec);
                        clear_active();
                        return;
                    }
                    Ok(None) | Err(_) => {}
                }
            }
            clear_active();
        })
        .ok();
}

#[tauri::command(rename_all = "camelCase")]
pub fn confirm_crash_resolution_after_launch(
    app: tauri::AppHandle,
    path: String,
) -> Result<Option<CrashResolutionRecord>, String> {
    let manifest_path = resolve_manifest(&path)?;
    let path_str = manifest_path.to_string_lossy().to_string();
    let project_dir = manifest_parent(&path_str)?;
    // Immediate attempt (game already healthy / relaunch into existing session).
    if let Some(rec) = maybe_confirm_crash_resolution(&manifest_path, "successful_launch")? {
        emit_success_soft_verify(&app, &path_str, &project_dir, &rec);
        emit_distill_resolution(&app, &path_str, &rec);
        return Ok(Some(rec));
    }
    // Otherwise watch until soft-verify playtime + healthy post-fix session.
    if pending_fix_marker_exists(&project_dir) {
        spawn_crash_resolution_watcher(app, manifest_path);
    }
    Ok(None)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn confirm_crash_resolution_from_diagnose(
    app: tauri::AppHandle,
    path: String,
) -> Result<Option<CrashResolutionRecord>, String> {
    // Blocking-pool conversion: sync commands run on the main thread; disk
    // I/O here stalled IPC delivery for the whole Diagnose flow.
    tokio::task::spawn_blocking(move || confirm_crash_resolution_from_diagnose_impl(app, path))
        .await
        .map_err(|e| e.to_string())?
}

fn confirm_crash_resolution_from_diagnose_impl(
    app: tauri::AppHandle,
    path: String,
) -> Result<Option<CrashResolutionRecord>, String> {
    let manifest_path = resolve_manifest(&path)?;
    let path_str = manifest_path.to_string_lossy().to_string();
    let project_dir = manifest_parent(&path_str)?;
    let rec = maybe_confirm_crash_resolution(&manifest_path, "diagnose_healthy")?;
    if let Some(ref r) = rec {
        emit_success_soft_verify(&app, &path_str, &project_dir, r);
        emit_distill_resolution(&app, &path_str, r);
    }
    Ok(rec)
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_crash_fix_banner(path: String) -> Result<Option<CrashFixBanner>, String> {
    let project_dir = manifest_parent(&path)?;
    let Some(marker) = load_crash_fix_marker(&project_dir)? else {
        return Ok(None);
    };
    if marker.resolved || marker.rolled_back || marker.soft_verify_failed || marker.shared {
        return Ok(None);
    }
    // Episode opened on crash with no edits yet — don't claim a fix was applied.
    if marker.actions.is_empty() {
        return Ok(None);
    }
    let actions_summary: Vec<String> = marker
        .actions
        .iter()
        .map(format_launcher_action_summary)
        .collect();
    Ok(Some(CrashFixBanner {
        snapshot_id: marker.snapshot_id,
        fingerprint_key: marker.fingerprint_key,
        plan_source: marker.plan_source,
        human_explanation: marker.human_explanation,
        matched_case_ids: marker.matched_case_ids,
        actions_summary,
        created_at: marker.created_at,
        resolved: marker.resolved,
        rolled_back: marker.rolled_back,
        soft_verify_started_unix: marker.soft_verify_started_unix,
        min_playtime_secs: SOFT_VERIFY_MIN_SECS,
    }))
}

#[tauri::command(rename_all = "camelCase")]
pub fn report_soft_verify_failure(
    app: tauri::AppHandle,
    path: String,
    reason: Option<String>,
) -> Result<Option<LastCrashFixMarker>, String> {
    let manifest_path = resolve_manifest(&path)?;
    let why = reason.unwrap_or_else(|| "launch_crash".into());
    fail_soft_verify(&app, &manifest_path, &why)
}

#[tauri::command(rename_all = "camelCase")]
pub fn rollback_last_crash_fix(
    app: tauri::AppHandle,
    path: String,
) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(&path)?;
    let Some(marker) = load_crash_fix_marker(&project_dir)? else {
        return Err("No pending crash-fix snapshot to roll back".into());
    };
    if marker.snapshot_id.trim().is_empty() {
        return Err("Crash-fix marker has no snapshot id".into());
    }
    let store = SnapshotStore::new(&project_dir);
    let snapshot = store
        .rollback(marker.snapshot_id.clone())
        .map_err(|e| e.to_string())?;
    let manifest_path = resolve_manifest(&path)?;
    let _ = fail_soft_verify(&app, &manifest_path, "rollback");
    Ok(json!({
        "ok": true,
        "snapshotId": snapshot.id,
        "name": snapshot.name,
    }))
}

/// If a snapshot rollback matches the pending crash-fix marker, emit reject.
pub fn note_snapshot_rollback(
    app: &tauri::AppHandle,
    project_dir: &Path,
    snapshot_id: &str,
) -> Result<(), String> {
    let Some(marker) = load_crash_fix_marker(project_dir)? else {
        return Ok(());
    };
    if marker.resolved || marker.rolled_back || marker.soft_verify_failed {
        return Ok(());
    }
    if marker.snapshot_id != snapshot_id {
        return Ok(());
    }
    let manifest_path = marker
        .manifest_path
        .as_ref()
        .map(PathBuf::from)
        .filter(|p| p.as_os_str().len() > 0)
        .unwrap_or_else(|| project_dir.join("__soft_verify__.json"));
    let _ = fail_soft_verify(app, &manifest_path, "rollback");
    Ok(())
}

fn resolve_fingerprint_key(manifest_path: &Path) -> String {
    let Some(project_dir) = manifest_path.parent() else {
        return "unknown".into();
    };
    let Ok(manifest) = ProjectManifest::load_from_path(manifest_path) else {
        return "unknown".into();
    };
    let loader = format!("{:?}", manifest.loader.kind).to_lowercase();
    let mut text = String::new();
    // Prefer the newest crash-report (what the user was fixing); fall back to latest.log.
    let reports_dir = project_dir.join("crash-reports");
    if let Ok(entries) = std::fs::read_dir(&reports_dir) {
        let mut newest: Option<(u64, PathBuf)> = None;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(mtime) = file_mtime_secs(&path) else {
                continue;
            };
            if newest.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
                newest = Some((mtime, path));
            }
        }
        if let Some((_, path)) = newest {
            text = std::fs::read_to_string(path).unwrap_or_default();
        }
    }
    if text.trim().is_empty() {
        text = std::fs::read_to_string(project_dir.join("logs").join("latest.log"))
            .unwrap_or_default();
    }
    let fp = tuffbox_core::crash_kb::fingerprint_from_text(
        &text,
        &manifest.minecraft.version,
        &loader,
    );
    if fp.key.trim().is_empty() {
        "unknown".into()
    } else {
        fp.key
    }
}

/// Record a pending crash-fix attempt from Diagnose one-click actions
/// (disable / raise memory / etc.) so a later healthy relaunch can confirm
/// *how* the user resolved the error.
pub fn record_user_fix_attempt(
    manifest_path: &Path,
    source: &str,
    human_explanation: &str,
    actions: Vec<tuffbox_core::action_plan::LauncherAction>,
    fingerprint_key: Option<&str>,
) -> Result<(), String> {
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest path has no parent".to_string())?;
    let fp = fingerprint_key
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| resolve_fingerprint_key(manifest_path));
    let episode_id = crate::pack_events::episode_id_for_fingerprint(&fp);
    let ai = is_known_cause_source(Some(source));

    if let Ok(Some(mut existing)) = load_crash_fix_marker(project_dir) {
        if !existing.resolved && !existing.rolled_back && !existing.soft_verify_failed {
            existing.actions = merge_launcher_actions(&existing.actions, actions);
            existing.fingerprint_key = fp.clone();
            existing.episode_id = Some(episode_id.clone());
            if existing.manifest_path.as_deref().is_none_or(|s| s.is_empty()) {
                existing.manifest_path = manifest_path.to_str().map(|s| s.to_string());
            }
            if ai {
                existing.plan_source = Some(source.to_string());
                if !human_explanation.trim().is_empty() {
                    existing.human_explanation = human_explanation.to_string();
                }
            } else {
                // Player / Diagnose UI path: keep trail honesty; append only.
                if is_player_trail_source(existing.plan_source.as_deref()) {
                    existing.plan_source = Some("player_trail".into());
                    existing.human_explanation = PLAYER_TRAIL_EXPLANATION.to_string();
                } else {
                    // AI plan was applied earlier, then more player edits — causality unknown.
                    existing.plan_source = Some("player_trail".into());
                    existing.human_explanation = PLAYER_TRAIL_EXPLANATION.to_string();
                }
            }
            // Do not bump created_at_unix — soft-verify needs log newer than episode start.
            save_crash_fix_marker(project_dir, &existing)?;
            return Ok(());
        }
    }

    let explanation = if ai {
        human_explanation.to_string()
    } else {
        PLAYER_TRAIL_EXPLANATION.to_string()
    };
    let plan_source = if ai {
        source.to_string()
    } else {
        "player_trail".into()
    };
    let plan = ActionPlan {
        schema_version: tuffbox_core::action_plan::ACTION_PLAN_SCHEMA_VERSION,
        human_explanation: explanation,
        confidence: 0.7,
        suspected_mods: Vec::new(),
        needs_user_review: false,
        source: Some(plan_source.clone()),
        matched_case_ids: Vec::new(),
        actions,
        additional_context: None,
    };
    let actor = crate::pack_events::actor_for_plan_source(Some(source)).to_string();
    let snapshot = Snapshot {
        id: format!("user-fix-{}", tuffbox_core::time_util::compact_now()),
        name: format!("crash-fix-{source}"),
        created_at: tuffbox_core::time_util::rfc3339_now(),
        reason: human_explanation.to_string(),
        manifest_path: manifest_path.to_path_buf(),
        lockfile_path: None,
        changed_files: Vec::new(),
        tags: vec!["crash_fix".into()],
        crash_fingerprint_key: Some(fp.clone()),
        report_id: None,
        plan_source: Some(plan_source),
        matched_case_ids: Vec::new(),
        operation: "crash_fix".into(),
        actions_summary: plan
            .actions
            .iter()
            .map(format_launcher_action_summary)
            .collect(),
        actor: Some(actor),
        managed_files: Vec::new(),
        // This marker snapshot's on-disk counterpart is stored separately via
        // SnapshotStore::create_with_meta; keep size 0 here (unknown/absent).
        size_bytes: 0,
    };
    write_last_crash_fix_marker(project_dir, &snapshot, &plan, &fp)?;
    if let Ok(Some(mut marker)) = load_crash_fix_marker(project_dir) {
        marker.episode_id = Some(episode_id);
        let _ = save_crash_fix_marker(project_dir, &marker);
    }
    Ok(())
}

/// Open a pending crash-fix episode when a crash is detected so later player
/// Content/UI edits can accumulate into the share trail (without Diagnose/AI).
pub fn ensure_open_crash_episode_marker(
    project_dir: &Path,
    fingerprint_key: &str,
    manifest_path: Option<&Path>,
) -> Result<(), String> {
    if fingerprint_key.trim().is_empty() {
        return Ok(());
    }
    if let Ok(Some(existing)) = load_crash_fix_marker(project_dir) {
        if !existing.resolved && !existing.rolled_back && !existing.soft_verify_failed {
            return Ok(());
        }
    }
    let now_unix = tuffbox_core::time_util::unix_now_secs();
    let marker = LastCrashFixMarker {
        snapshot_id: format!("crash-episode-{}", tuffbox_core::time_util::compact_now()),
        fingerprint_key: fingerprint_key.to_string(),
        plan_source: Some("player_trail".into()),
        matched_case_ids: Vec::new(),
        human_explanation: PLAYER_TRAIL_EXPLANATION.to_string(),
        actions: Vec::new(),
        created_at: tuffbox_core::time_util::rfc3339_now(),
        created_at_unix: Some(now_unix),
        shared: false,
        resolved: false,
        resolved_at: None,
        manifest_path: manifest_path.and_then(|p| p.to_str().map(|s| s.to_string())),
        soft_verify_started_unix: None,
        rolled_back: false,
        soft_verify_failed: false,
        vote_emitted: false,
        episode_id: Some(crate::pack_events::episode_id_for_fingerprint(fingerprint_key)),
    };
    save_crash_fix_marker(project_dir, &marker)
}

/// Append launcher-side mod ops (Content tab) into the open crash episode marker.
pub fn note_player_mod_actions_on_open_marker(
    manifest_path: &Path,
    operation: &str,
    mods: &[(String, Option<String>)],
    snapshot_id: Option<&str>,
) -> Result<(), String> {
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest path has no parent".to_string())?;
    let Ok(Some(mut marker)) = load_crash_fix_marker(project_dir) else {
        return Ok(());
    };
    if marker.resolved || marker.rolled_back || marker.soft_verify_failed {
        return Ok(());
    }
    if let Some(sid) = snapshot_id.filter(|s| !s.is_empty()) {
        if marker.snapshot_id.starts_with("crash-episode-") || marker.actions.is_empty() {
            marker.snapshot_id = sid.to_string();
            // Soft-verify should require a healthy log after this first edit.
            marker.created_at_unix = Some(tuffbox_core::time_util::unix_now_secs());
            marker.created_at = tuffbox_core::time_util::rfc3339_now();
            let _ = save_crash_fix_marker(project_dir, &marker);
        }
    }
    let op = match operation {
        "disable-mod" | "disable_mod" => "disable_mod",
        "enable-mod" | "enable_mod" => return Ok(()), // not in KNOWN_OPS
        "remove-mod" | "remove_mod" | "delete-mod" => "remove_mod",
        "update-mod" | "update_mod" => "update_mod",
        "add-mod" | "add-curseforge-mod" | "install_mod" => "install_mod",
        other if other.contains("disable") => "disable_mod",
        other if other.contains("remove") || other.contains("delete") => "remove_mod",
        other if other.contains("update") => "update_mod",
        other if other.contains("add") || other.contains("install") => "install_mod",
        _ => return Ok(()),
    };
    let actions: Vec<_> = mods
        .iter()
        .map(|(id, version)| tuffbox_core::action_plan::LauncherAction {
            op: op.into(),
            mod_id: Some(id.clone()),
            provider: None,
            project_id: None,
            version: version.clone(),
            path: None,
            patch_type: None,
            patch: None,
            reason: Some(format!("Player {operation}")),
            risk: "medium".into(),
        })
        .collect();
    if actions.is_empty() {
        return Ok(());
    }
    record_user_fix_attempt(
        manifest_path,
        "player_trail",
        PLAYER_TRAIL_EXPLANATION,
        actions,
        Some(marker.fingerprint_key.as_str()),
    )
}

pub fn write_last_crash_fix_marker(
    project_dir: &Path,
    snapshot: &Snapshot,
    plan: &ActionPlan,
    fingerprint_key: &str,
) -> Result<(), String> {
    let now_unix = tuffbox_core::time_util::unix_now_secs();
    let manifest_path = snapshot
        .manifest_path
        .to_str()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    // Prefer merging into an open episode so player edits after crash are kept.
    if let Ok(Some(mut existing)) = load_crash_fix_marker(project_dir) {
        if !existing.resolved && !existing.rolled_back && !existing.soft_verify_failed {
            let ai = is_known_cause_source(plan.source.as_deref());
            existing.actions = merge_launcher_actions(&existing.actions, plan.actions.clone());
            existing.fingerprint_key = fingerprint_key.to_string();
            existing.snapshot_id = snapshot.id.clone();
            existing.matched_case_ids = plan.matched_case_ids.clone();
            if existing.manifest_path.as_deref().is_none_or(|s| s.is_empty()) {
                existing.manifest_path = manifest_path;
            }
            if ai {
                existing.plan_source = plan.source.clone();
                existing.human_explanation = plan.human_explanation.clone();
            } else {
                existing.plan_source = Some("player_trail".into());
                existing.human_explanation = PLAYER_TRAIL_EXPLANATION.to_string();
            }
            existing.episode_id =
                Some(crate::pack_events::episode_id_for_fingerprint(fingerprint_key));
            return save_crash_fix_marker(project_dir, &existing);
        }
    }

    let marker = LastCrashFixMarker {
        snapshot_id: snapshot.id.clone(),
        fingerprint_key: fingerprint_key.to_string(),
        plan_source: plan.source.clone(),
        matched_case_ids: plan.matched_case_ids.clone(),
        human_explanation: plan.human_explanation.clone(),
        actions: plan.actions.clone(),
        created_at: tuffbox_core::time_util::rfc3339_now(),
        created_at_unix: Some(now_unix),
        shared: false,
        resolved: false,
        resolved_at: None,
        manifest_path,
        soft_verify_started_unix: None,
        rolled_back: false,
        soft_verify_failed: false,
        vote_emitted: false,
        episode_id: Some(crate::pack_events::episode_id_for_fingerprint(fingerprint_key)),
    };
    save_crash_fix_marker(project_dir, &marker)
}

pub fn auto_snapshot_crash_fix(
    manifest_path: &Path,
    plan: &ActionPlan,
    fingerprint_key: Option<&str>,
) -> Result<Snapshot, String> {
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest path has no parent".to_string())?;
    let lockfile_path = manifest_path.with_extension("lock.json");
    let lockfile_path = if lockfile_path.exists() {
        Some(lockfile_path)
    } else {
        None
    };
    let fp = fingerprint_key.unwrap_or("unknown");
    let fp_prefix: String = fp.chars().take(24).collect();
    let name = format!("auto-before-crash-fix-{fp_prefix}");
    let reason = format!(
        "Auto snapshot before crash fix ({})",
        plan.source.as_deref().unwrap_or("manual")
    );
    let actions_summary: Vec<String> = plan
        .actions
        .iter()
        .map(format_launcher_action_summary)
        .collect();
    let mut actions_summary = actions_summary;
    if actions_summary.is_empty() && !plan.human_explanation.trim().is_empty() {
        actions_summary.push(plan.human_explanation.clone());
    }
    let plan_source = plan.source.clone().or_else(|| Some("manual".into()));
    let actor = crate::pack_events::actor_for_plan_source(plan_source.as_deref()).to_string();
    let episode_id = crate::pack_events::episode_id_for_fingerprint(fp);
    let meta = SnapshotMeta {
        tags: vec!["crash_fix".into()],
        crash_fingerprint_key: fingerprint_key.map(|s| s.to_string()),
        report_id: None,
        plan_source: plan_source.clone(),
        matched_case_ids: plan.matched_case_ids.clone(),
        operation: "crash_fix".into(),
        actions_summary: actions_summary.clone(),
        actor: Some(actor),
        ..Default::default()
    };
    let store = SnapshotStore::new(project_dir);
    let snapshot = store
        .create_with_meta(
            &name,
            &reason,
            manifest_path,
            lockfile_path.as_ref(),
            &[] as &[std::path::PathBuf],
            meta,
        )
        .map_err(|e| e.to_string())?;
    write_snapshot_plan_json(&store, &snapshot.id, plan)?;
    let _ = crate::pack_events::append_from_snapshot_with_episode(
        project_dir,
        "crash_fix",
        &snapshot.id,
        &[] as &[std::path::PathBuf],
        &reason,
        &actions_summary,
        Some(&episode_id),
        Some(fp),
        plan_source.as_deref(),
    );
    let _ = write_last_crash_fix_marker(project_dir, &snapshot, plan, fp);
    if let Ok(Some(mut marker)) = load_crash_fix_marker(project_dir) {
        marker.episode_id = Some(episode_id);
        let _ = save_crash_fix_marker(project_dir, &marker);
    }
    Ok(snapshot)
}

/// Heuristic Crash Assistant apply path — still tags snapshot as crash_fix.
pub fn auto_snapshot_crash_fix_heuristic(
    manifest_path: &Path,
    fingerprint_key: Option<&str>,
    summary: &str,
    report_id: Option<&str>,
    actions: Vec<tuffbox_core::action_plan::LauncherAction>,
) -> Result<Snapshot, String> {
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest path has no parent".to_string())?;
    let lockfile_path = manifest_path.with_extension("lock.json");
    let lockfile_path = if lockfile_path.exists() {
        Some(lockfile_path)
    } else {
        None
    };
    let fp = fingerprint_key.unwrap_or("unknown");
    let fp_prefix: String = fp.chars().take(24).collect();
    let name = format!("auto-before-crash-fix-{fp_prefix}");
    let reason = format!("Auto snapshot before crash fix (heuristic): {summary}");
    let actions_summary: Vec<String> = actions
        .iter()
        .map(format_launcher_action_summary)
        .collect();
    let mut actions_summary = actions_summary;
    if actions_summary.is_empty() {
        actions_summary.push(summary.to_string());
    }
    let meta = SnapshotMeta {
        tags: vec!["crash_fix".into()],
        crash_fingerprint_key: fingerprint_key.map(|s| s.to_string()),
        report_id: report_id.map(|s| s.to_string()),
        plan_source: Some("heuristic".into()),
        matched_case_ids: Vec::new(),
        operation: "crash_fix".into(),
        actions_summary: actions_summary.clone(),
        actor: Some(
            crate::pack_events::actor_for_plan_source(Some("heuristic")).to_string(),
        ),
        ..Default::default()
    };
    let store = SnapshotStore::new(project_dir);
    let episode_id = crate::pack_events::episode_id_for_fingerprint(fp);
    let snapshot = store
        .create_with_meta(
            &name,
            &reason,
            manifest_path,
            lockfile_path.as_ref(),
            &[] as &[std::path::PathBuf],
            meta,
        )
        .map_err(|e| e.to_string())?;
    let plan = ActionPlan {
        schema_version: tuffbox_core::action_plan::ACTION_PLAN_SCHEMA_VERSION,
        human_explanation: summary.to_string(),
        confidence: 0.6,
        suspected_mods: Vec::new(),
        needs_user_review: true,
        source: Some("heuristic".into()),
        matched_case_ids: Vec::new(),
        actions,
        additional_context: None,
    };
    write_snapshot_plan_json(&store, &snapshot.id, &plan)?;
    let _ = crate::pack_events::append_from_snapshot_with_episode(
        project_dir,
        "crash_fix",
        &snapshot.id,
        &[] as &[std::path::PathBuf],
        &reason,
        &actions_summary,
        Some(&episode_id),
        Some(fp),
        Some("heuristic"),
    );
    let _ = write_last_crash_fix_marker(project_dir, &snapshot, &plan, fp);
    if let Ok(Some(mut marker)) = load_crash_fix_marker(project_dir) {
        marker.episode_id = Some(episode_id);
        marker.plan_source = Some("heuristic".into());
        let _ = save_crash_fix_marker(project_dir, &marker);
    }
    Ok(snapshot)
}

fn write_snapshot_plan_json(
    store: &SnapshotStore,
    snapshot_id: &str,
    plan: &ActionPlan,
) -> Result<(), String> {
    let path = store.snapshot_dir(snapshot_id).join("plan.json");
    let json = serde_json::to_vec_pretty(plan).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

pub fn load_snapshot_plan(
    project_dir: &Path,
    snapshot_id: &str,
) -> Option<ActionPlan> {
    let path = project_dir
        .join(".tuffbox")
        .join("snapshots")
        .join(snapshot_id)
        .join("plan.json");
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Map ChangePlan actions into LauncherAction rows for resolution history.
pub fn change_actions_to_launcher(
    actions: &[tuffbox_core::ChangeAction],
) -> Vec<tuffbox_core::action_plan::LauncherAction> {
    actions
        .iter()
        .map(|action| match action {
            tuffbox_core::ChangeAction::InstallMod {
                project_id,
                version,
            } => tuffbox_core::action_plan::LauncherAction {
                op: "install_mod".into(),
                // Prefer slug-like project_id as modId so grounding/validation agree.
                mod_id: Some(project_id.clone()),
                provider: Some("modrinth".into()),
                project_id: Some(project_id.clone()),
                version: version.clone(),
                path: None,
                patch_type: None,
                patch: None,
                reason: None,
                risk: "medium".into(),
            },
            tuffbox_core::ChangeAction::RemoveMod { node_id } => {
                tuffbox_core::action_plan::LauncherAction {
                    op: "remove_mod".into(),
                    mod_id: Some(node_id.0.trim_start_matches("mod:").to_string()),
                    provider: None,
                    project_id: None,
                    version: None,
                    path: None,
                    patch_type: None,
                    patch: None,
                    reason: None,
                    risk: "high".into(),
                }
            }
            tuffbox_core::ChangeAction::DisableMod { node_id } => {
                tuffbox_core::action_plan::LauncherAction {
                    op: "disable_mod".into(),
                    mod_id: Some(node_id.0.trim_start_matches("mod:").to_string()),
                    provider: None,
                    project_id: None,
                    version: None,
                    path: None,
                    patch_type: None,
                    patch: None,
                    reason: None,
                    risk: "medium".into(),
                }
            }
            tuffbox_core::ChangeAction::UpdateMod {
                node_id,
                target_version,
            } => tuffbox_core::action_plan::LauncherAction {
                op: "update_mod".into(),
                mod_id: Some(node_id.0.trim_start_matches("mod:").to_string()),
                provider: None,
                project_id: None,
                version: Some(target_version.clone()),
                path: None,
                patch_type: None,
                patch: None,
                reason: None,
                risk: "medium".into(),
            },
            tuffbox_core::ChangeAction::EditConfig { path, patch } => {
                tuffbox_core::action_plan::LauncherAction {
                    op: "edit_config".into(),
                    mod_id: None,
                    provider: None,
                    project_id: None,
                    version: None,
                    path: Some(path.clone()),
                    patch_type: None,
                    patch: Some(serde_json::Value::String(patch.clone())),
                    reason: None,
                    risk: "low".into(),
                }
            }
        })
        .collect()
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_pending_action_plan(path: String) -> Result<Option<ActionPlan>, String> {
    // Blocking-pool conversion: sync commands run on the main thread; disk
    // I/O here stalled IPC delivery for the whole Diagnose flow.
    tokio::task::spawn_blocking(move || get_pending_action_plan_impl(path))
        .await
        .map_err(|e| e.to_string())?
}

fn get_pending_action_plan_impl(path: String) -> Result<Option<ActionPlan>, String> {
    integrations::require_swarm_enabled()?;
    let project_dir = manifest_parent(&path)?;
    load_pending_action_plan(&project_dir)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn clear_pending_network_plan(path: String) -> Result<(), String> {
    // Blocking-pool conversion: sync commands run on the main thread; disk
    // I/O here stalled IPC delivery for the whole Diagnose flow.
    tokio::task::spawn_blocking(move || clear_pending_network_plan_impl(path))
        .await
        .map_err(|e| e.to_string())?
}

fn clear_pending_network_plan_impl(path: String) -> Result<(), String> {
    integrations::require_swarm_enabled()?;
    let project_dir = manifest_parent(&path)?;
    clear_pending_action_plan(&project_dir)
}

#[tauri::command(rename_all = "camelCase")]
pub fn write_pending_network_plan(path: String, plan: ActionPlan) -> Result<String, String> {
    integrations::require_swarm_enabled()?;
    let project_dir = manifest_parent(&path)?;
    let out = write_pending_action_plan(&project_dir, &plan)?;
    Ok(out.to_string_lossy().to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_community_crash_capsules(
    status: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<tuffbox_core::swarm_supabase::CommunityCapsuleCard>, String> {
    integrations::require_swarm_enabled()?;
    if !integrations::swarm_supabase_configured() {
        return Err("Community Supabase backend is not available".into());
    }
    let url = integrations::swarm_supabase_url().unwrap();
    let anon = integrations::swarm_supabase_anon_key().unwrap();
    tuffbox_core::swarm_supabase::list_community_capsules_supabase(
        &url,
        &anon,
        status.as_deref(),
        limit.unwrap_or(48),
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn vote_community_crash_capsule(
    content_hash: String,
    vote: String,
    access_token: String,
) -> Result<serde_json::Value, String> {
    integrations::require_swarm_enabled()?;
    if !integrations::swarm_supabase_configured() {
        return Err("Community Supabase backend is not available".into());
    }
    if access_token.trim().is_empty() {
        return Err("login required — register and sign in to vote".into());
    }
    let url = integrations::swarm_supabase_url().unwrap();
    let edge = integrations::swarm_supabase_edge_anon_key().unwrap();
    tuffbox_core::swarm_supabase::vote_capsule_supabase(
        &url,
        &edge,
        &content_hash,
        &vote,
        &access_token,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_local_kudos_balance() -> Result<serde_json::Value, String> {
    integrations::require_swarm_enabled()?;
    if !integrations::swarm_supabase_configured() {
        return Err("Community Supabase backend is not available".into());
    }
    let pk = tuffbox_core::swarm::device_signer_public_key_b64()?;
    let url = integrations::swarm_supabase_url().unwrap();
    let anon = integrations::swarm_supabase_anon_key().unwrap();
    tuffbox_core::swarm_supabase::fetch_kudos_balance_supabase(&url, &anon, &pk).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn accept_creation_result(
    job_id: String,
    worker_signer_public_key: String,
    access_token: String,
    amount: Option<u32>,
) -> Result<serde_json::Value, String> {
    integrations::require_swarm_enabled()?;
    if !integrations::swarm_supabase_configured() {
        return Err("Community Supabase backend is not available".into());
    }
    if access_token.trim().is_empty() {
        return Err("login required — register and sign in to accept".into());
    }
    if job_id.trim().is_empty() {
        return Err("jobId required".into());
    }
    if worker_signer_public_key.trim().is_empty() {
        return Err("workerSignerPublicKey required — worker did not report a device key".into());
    }
    let url = integrations::swarm_supabase_url().unwrap();
    let edge = integrations::swarm_supabase_edge_anon_key().unwrap();
    tuffbox_core::swarm_supabase::accept_creation_supabase(
        &url,
        &edge,
        &job_id,
        &worker_signer_public_key,
        &access_token,
        amount,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub fn propose_community_capsule_plan(
    path: String,
    content_hash: String,
    solution: String,
    actions: Vec<tuffbox_core::action_plan::LauncherAction>,
    matched_id: Option<String>,
    trust_meta: Option<String>,
) -> Result<String, String> {
    integrations::require_swarm_enabled()?;
    let project_dir = manifest_parent(&path)?;
    let trust_suffix = trust_meta
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!(" | {s}"))
        .unwrap_or_default();
    let mut plan = ActionPlan {
        schema_version: tuffbox_core::action_plan::ACTION_PLAN_SCHEMA_VERSION,
        human_explanation: solution,
        confidence: 0.45,
        suspected_mods: actions
            .iter()
            .filter_map(|a| a.mod_id.clone())
            .collect(),
        needs_user_review: true,
        source: Some("swarm".into()),
        matched_case_ids: vec![matched_id.unwrap_or(content_hash)],
        actions,
        additional_context: Some(format!(
            "Proposed from Crash Votes. Confirm in Diagnostics before apply.{trust_suffix}"
        )),
    };
    let validation = tuffbox_core::action_plan::validate_action_plan(&plan);
    if !validation.ok {
        return Err(format!(
            "invalid plan: {}",
            validation.errors.join("; ")
        ));
    }
    plan.needs_user_review = true;
    let out = write_pending_action_plan(&project_dir, &plan)?;
    Ok(out.to_string_lossy().to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_share_prompt_after_launch(path: String) -> Result<Option<LastCrashFixMarker>, String> {
    let swarm = integrations::swarm_settings();
    if !swarm.enabled || !swarm.share_prompts_enabled {
        return Ok(None);
    }
    let project_dir = manifest_parent(&path)?;
    let marker_path = last_crash_fix_path(&project_dir);
    if !marker_path.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&marker_path).map_err(|e| e.to_string())?;
    let marker: LastCrashFixMarker =
        serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if marker.shared {
        return Ok(None);
    }
    // Soft-verify gate: only after a verified healthy session (resolved).
    if !marker.resolved || marker.rolled_back || marker.soft_verify_failed {
        return Ok(None);
    }
    Ok(Some(marker))
}

#[tauri::command(rename_all = "camelCase")]
pub fn dismiss_share_prompt(path: String) -> Result<(), String> {
    let project_dir = manifest_parent(&path)?;
    let marker_path = last_crash_fix_path(&project_dir);
    if !marker_path.is_file() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&marker_path).map_err(|e| e.to_string())?;
    let mut marker: LastCrashFixMarker =
        serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    marker.shared = true;
    std::fs::write(
        marker_path,
        serde_json::to_vec_pretty(&marker).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn publish_experience_capsule(
    path: String,
    fingerprint_key: Option<String>,
    human_explanation: Option<String>,
    actions: Option<Vec<tuffbox_core::action_plan::LauncherAction>>,
) -> Result<serde_json::Value, String> {
    integrations::require_swarm_enabled()?;
    let project_dir = manifest_parent(&path)?;

    let marker_path = last_crash_fix_path(&project_dir);
    let marker: Option<LastCrashFixMarker> = std::fs::read_to_string(&marker_path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok());

    let fp_key = fingerprint_key
        .or_else(|| marker.as_ref().map(|m| m.fingerprint_key.clone()))
        .unwrap_or_else(|| "unknown".into());
    let user_supplied_explanation = human_explanation
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let user_supplied_actions = actions.is_some();
    let mut solution = human_explanation
        .or_else(|| marker.as_ref().map(|m| m.human_explanation.clone()))
        .unwrap_or_else(|| "Shared crash fix".into());
    let mut launcher_actions = actions.unwrap_or_else(|| {
        marker
            .as_ref()
            .map(|m| m.actions.clone())
            .unwrap_or_default()
    });

    let player_trail = marker
        .as_ref()
        .map(|m| is_player_trail_source(m.plan_source.as_deref()))
        .unwrap_or(true);
    if let Some(plan) = load_group_test_session(&project_dir).and_then(|s| s.share_plan()) {
        if !user_supplied_actions {
            launcher_actions = plan.actions.clone();
        }
        if !user_supplied_explanation {
            solution = plan.human_explanation.clone();
        }
    } else if player_trail && !user_supplied_actions {
        let universe = load_project_manifest(&path)
            .map(|(_, m)| pack_mod_ids(&m))
            .unwrap_or_default();
        let decoded = decode_player_share(
            &project_dir,
            marker
                .as_ref()
                .map(|m| m.fingerprint_key.as_str())
                .unwrap_or(fp_key.as_str()),
            &universe,
        );
        if !user_supplied_explanation {
            solution = decoded.explanation.clone();
        }
        let mut decoded_actions = decoded.disable_actions();
        decoded_actions.extend(decoded.extra_actions);
        launcher_actions = decoded_actions;
    }

    let (_manifest_path, manifest) = load_project_manifest(&path)?;
    let inventory_ids = pack_mod_ids(&manifest);
    let mut share_plan = ActionPlan {
        schema_version: tuffbox_core::action_plan::ACTION_PLAN_SCHEMA_VERSION,
        human_explanation: solution.clone(),
        confidence: 0.8,
        suspected_mods: Vec::new(),
        needs_user_review: true,
        source: Some("distill".into()),
        matched_case_ids: marker
            .as_ref()
            .map(|m| m.matched_case_ids.clone())
            .unwrap_or_default(),
        actions: launcher_actions,
        additional_context: None,
    };
    let grounded =
        tuffbox_core::action_plan::ground_action_plan(share_plan, &inventory_ids, &[]);
    share_plan = grounded.plan;
    let validation = tuffbox_core::action_plan::validate_action_plan_with_inventory(
        &share_plan,
        &inventory_ids,
        &[],
    );
    if !validation.ok {
        return Err(format!(
            "Cannot publish invalid plan: {}",
            validation.errors.join("; ")
        ));
    }
    launcher_actions = share_plan.actions.clone();
    solution = share_plan.human_explanation.clone();

    let loader = format!("{:?}", manifest.loader.kind).to_lowercase();
    let crash = std::fs::read_to_string(project_dir.join("logs").join("latest.log")).unwrap_or_default();
    // Prefer a real fingerprint from logs when available; never publish the log itself.
    let mut fingerprint = tuffbox_core::crash_kb::fingerprint_from_text(
        &crash,
        &manifest.minecraft.version,
        &loader,
    );
    if fingerprint.key.trim().is_empty() || fingerprint.key == "||||" {
        fingerprint = tuffbox_core::crash_kb::CrashFingerprint {
            exception: fp_key.clone(),
            frames: Vec::new(),
            mod_file: None,
            mixin: None,
            mc_major: fingerprint.mc_major,
            loader: loader.clone(),
            key: fp_key.clone(),
            blame_mod_ids: Vec::new(),
        };
    } else if !fp_key.is_empty() && fp_key != "unknown" {
        // Keep marker key if it was the one used for the fix.
        fingerprint.key = fp_key;
    }

    let case = CrashCase {
        id: format!("capsule-{}", tuffbox_core::time_util::compact_now()),
        fingerprint: fingerprint.clone(),
        symptoms: Vec::new(),
        suspected_mods: Vec::new(),
        solution: solution.clone(),
        actions: Vec::new(),
        launcher_actions: launcher_actions.clone(),
        notes: None,
        source: "authored".into(),
        success_count: 1,
        fail_count: 0,
    };
    let mut capsule = ExperienceCapsule::from_crash_case(&case).sanitized_for_network();
    // Soft-sign with persistent device key — required for Supabase Kudos beneficiary + Edge verify.
    let device_id = tuffbox_core::swarm::sign_capsule_with_device_key(&mut capsule)
        .map_err(|e| format!("Cannot publish unsigned capsule: {e}"))?;
    if capsule
        .signer_public_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        return Err("signer_public_key missing after device sign".into());
    }
    let public = capsule.to_public_json();

    // Project-local authored export (pack author).
    let _ = tuffbox_core::crash_kb::save_authored_case(
        &project_dir,
        AuthorCaseInput {
            id: Some(case.id.clone()),
            fingerprint: fingerprint.clone(),
            solution: solution.clone(),
            symptoms: Vec::new(),
            suspected_mods: Vec::new(),
            launcher_actions: launcher_actions.clone(),
            actions: Vec::new(),
            notes: None,
        },
    );

    // Machine-wide durable library — other projects on this PC keep the fix.
    let global = integrations::global_capsule_library();
    let stored_global = global.publish(&capsule)?;

    let mut published_remote = false;
    let mut remote_results = Vec::new();
    let mut remote_error: Option<String> = None;
    let mut supabase_ok = false;
    let mut p2p_gossip_ok: Option<bool> = None;
    let mut p2p_gossip_error: Option<String> = None;

    // Preferred transport: Supabase Edge Function (signed capsules only).
    if integrations::swarm_supabase_configured() {
        let url = integrations::swarm_supabase_url().unwrap();
        let edge = integrations::swarm_supabase_edge_anon_key().unwrap();
        match tuffbox_core::swarm_supabase::publish_capsule_supabase(&url, &edge, &capsule).await {
            Ok(body) => {
                published_remote = true;
                supabase_ok = true;
                remote_results.push(json!({
                    "transport": "supabase",
                    "ok": true,
                    "body": body,
                }));
            }
            Err(e) => {
                remote_results.push(json!({
                    "transport": "supabase",
                    "ok": false,
                    "error": e.clone(),
                }));
                remote_error = Some(e);
            }
        }
    }

    // Phase C / hub: prefer P2P control HTTP when healthy; hub remains bootstrap/fallback.
    // Publish to every available transport so hub stays seeded for non-P2P peers.
    let bases = crate::swarm_node::capsule_transport_bases().await;
    for base in &bases {
        let token = crate::swarm_node::auth_token_for_base(base);
        match tuffbox_core::crash_remote::publish_capsule_async(&base, token.as_deref(), &public)
            .await
        {
            Ok(body) => {
                let gossip_ok = body
                    .pointer("/gossip/ok")
                    .and_then(|v| v.as_bool());
                if let Some(ok) = gossip_ok {
                    p2p_gossip_ok = Some(ok);
                    if !ok {
                        p2p_gossip_error = body
                            .pointer("/gossip/error")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                    }
                }
                // HTTP stored on node/hub counts as remote even if gossip mesh failed.
                published_remote = true;
                remote_results.push(json!({
                    "base": base,
                    "ok": true,
                    "gossipOk": gossip_ok,
                    "body": body,
                }));
            }
            Err(e) => {
                remote_results.push(json!({ "base": base, "ok": false, "error": e.clone() }));
                if remote_error.is_none() {
                    remote_error = Some(e);
                }
            }
        }
    }
    if published_remote {
        remote_error = None;
    }

    let _ = dismiss_share_prompt(path);
    Ok(json!({
        "published": published_remote,
        "sharedLocal": true,
        "globalPath": global.path().to_string_lossy(),
        "capsuleId": stored_global.id,
        "fingerprintKey": stored_global.fingerprint.key,
        "deviceId": device_id,
        "signed": stored_global.signature.is_some(),
        "remote": remote_results,
        "error": remote_error,
        "supabaseOk": supabase_ok,
        "p2pGossipOk": p2p_gossip_ok,
        "p2pGossipError": p2p_gossip_error,
        "privacy": { "rawLogs": false, "notesIncluded": false },
        "capsule": public,
        "supabaseConfigured": integrations::swarm_supabase_configured(),
        "hubConfigured": integrations::swarm_network_base().is_some(),
        "p2pConfigured": integrations::swarm_settings().p2p_enabled,
        "transportBases": bases,
    }))
}

#[tauri::command(rename_all = "camelCase")]
pub fn record_project_cooccurrence(path: String) -> Result<(), String> {
    let project_dir = manifest_parent(&path)?;
    let (_manifest_path, manifest) = load_project_manifest(&path)?;
    let loader = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string();
    let ids = pack_mod_ids(&manifest);
    let plan = plan_pack_observation(
        &project_dir,
        &ids,
        &manifest.minecraft.version,
        &loader,
    );
    if plan.record_local {
        record_mod_set_cooccurrence(
            &project_dir,
            &plan.ids,
            &manifest.minecraft.version,
            &loader,
        )?;
        let _ = mark_pack_observation(&project_dir, &plan.fingerprint, false);
    }
    Ok(())
}

/// Local record + best-effort Supabase upload of mod co-occurrence.
/// Local write does not require swarm; network upload does.
/// Identical pack compositions are not re-counted (basket dedupe).
pub async fn record_and_upload_cooccurrence(
    path: &str,
    mod_ids: &[String],
    source: &str,
) -> Result<serde_json::Value, String> {
    record_and_upload_cooccurrence_opts(path, mod_ids, source, true).await
}

pub async fn record_and_upload_cooccurrence_opts(
    path: &str,
    mod_ids: &[String],
    source: &str,
    record_local: bool,
) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(path)?;
    let (_manifest_path, manifest) = load_project_manifest(path)?;
    let loader = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string();
    let mc = manifest.minecraft.version.clone();
    let ids = if mod_ids.is_empty() {
        pack_mod_ids(&manifest)
    } else {
        normalize_mod_id_list(mod_ids, 512)
    };

    let plan = plan_pack_observation(&project_dir, &ids, &mc, &loader);
    let do_local = record_local && plan.record_local;
    if do_local {
        let _ = record_mod_set_cooccurrence(&project_dir, &plan.ids, &mc, &loader);
    }

    let mut uploaded = false;
    let mut upload_error: Option<String> = None;
    let skipped = !plan.record_local && !plan.upload_network;
    // ponytail: co-occurrence is mod-id pairs only — upload when Supabase is configured,
    // without requiring the full TuffSwarm crash-network opt-in. Crash capsules still gate on swarm.enabled.
    if plan.upload_network && integrations::swarm_supabase_configured() {
        if plan.network_ids.len() < 2 {
            upload_error = Some("need at least 2 mods to report co-occurrence".into());
        } else if let (Some(url), Some(key)) = (
            integrations::swarm_supabase_url(),
            integrations::swarm_supabase_anon_key(),
        ) {
            let client_key = tuffbox_core::swarm::load_or_create_device_signing_key()
                .ok()
                .map(|(_, device_id)| device_id);
            match tuffbox_core::swarm_supabase::report_cooccurrence_supabase(
                &url,
                &key,
                &plan.network_ids,
                &mc,
                &loader,
                source,
                client_key.as_deref(),
            )
            .await
            {
                Ok(_) => {
                    uploaded = true;
                    let _ = mark_pack_observation(&project_dir, &plan.fingerprint, true);
                }
                Err(e) => {
                    upload_error = Some(e);
                    if do_local || plan.record_local {
                        let _ = mark_pack_observation(&project_dir, &plan.fingerprint, false);
                    }
                }
            }
        }
    } else if do_local {
        let _ = mark_pack_observation(&project_dir, &plan.fingerprint, false);
    } else if skipped {
        // identical pack already observed + uploaded
    }

    Ok(json!({
        "local": do_local,
        "uploaded": uploaded,
        "skippedDuplicate": skipped,
        "modCount": plan.ids.len(),
        "networkModCount": plan.network_ids.len(),
        "uploadError": upload_error,
        "mcVersion": mc,
        "loader": loader,
        "fingerprint": plan.fingerprint,
    }))
}

/// Fire-and-forget pack basket stats (install / export hooks).
pub fn spawn_pack_cooccurrence(path: String, source: &'static str) {
    tauri::async_runtime::spawn(async move {
        let _ = record_and_upload_cooccurrence(&path, &[], source).await;
    });
}

#[tauri::command(rename_all = "camelCase")]
pub async fn report_mod_cooccurrence(
    path: String,
    mod_ids: Option<Vec<String>>,
    source: Option<String>,
) -> Result<serde_json::Value, String> {
    let ids = mod_ids.unwrap_or_default();
    let source = source.unwrap_or_else(|| "manual".into());
    record_and_upload_cooccurrence(&path, &ids, &source).await
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_local_cooccurrence(path: String, limit: Option<u32>) -> Result<Vec<ModPairStat>, String> {
    let project_dir = manifest_parent(&path)?;
    Ok(top_cooccurrence_pairs(
        &project_dir,
        limit.unwrap_or(25) as usize,
    ))
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_creation_trends(
    path: String,
    limit: Option<u32>,
) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(&path)?;
    let limit = limit.unwrap_or(20) as usize;
    let local = top_cooccurrence_pairs(&project_dir, limit.max(40));

    let mut network_pairs: Vec<ModPairStat> = Vec::new();
    let mut network: Option<serde_json::Value> = None;
    let (_manifest_path, manifest) = load_project_manifest(&path)?;
    let loader = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string();
    let mc = manifest.minecraft.version.clone();

    // Prefer hub GET /v1/mods/cooccurrence (MPI-seeded, no user IP to MPI), then Supabase.
    if integrations::swarm_enabled() {
        if let Some(endpoint) = integrations::swarm_network_base() {
            let token = integrations::secret_optional("crash_kb");
            if let Ok(body) = tuffbox_core::crash_remote::fetch_cooccurrence_async(
                &endpoint,
                token.as_deref(),
                &mc,
                &loader,
                limit as u32,
            )
            .await
            {
                if let Some(arr) = body.get("pairs").and_then(|v| v.as_array()) {
                    network_pairs = arr
                        .iter()
                        .filter_map(|p| {
                            let a = p
                                .get("modA")
                                .or_else(|| p.get("mod_a"))
                                .and_then(|v| v.as_str())?;
                            let b = p
                                .get("modB")
                                .or_else(|| p.get("mod_b"))
                                .and_then(|v| v.as_str())?;
                            Some(ModPairStat {
                                mod_a: a.to_string(),
                                mod_b: b.to_string(),
                                count: p.get("count").and_then(|v| v.as_u64()).unwrap_or(1),
                            })
                        })
                        .collect();
                }
                if !network_pairs.is_empty() {
                    network = Some(body);
                }
            }
        }
        if network_pairs.is_empty() {
            if let (Some(url), Some(key)) = (
                integrations::swarm_supabase_url(),
                integrations::swarm_supabase_anon_key(),
            ) {
                if let Ok(pairs) = tuffbox_core::swarm_supabase::fetch_cooccurrence_supabase(
                    &url,
                    &key,
                    &mc,
                    &loader,
                    limit as u32,
                )
                .await
                {
                    network = Some(json!({ "pairs": pairs, "source": "supabase" }));
                    network_pairs = pairs;
                }
            }
        }
    }

    let merged = merge_cooccurrence_pairs(&local, &network_pairs, limit);
    let groups = top_cooccurrence_groups(&merged, limit.min(12));
    let prompt_hint = format_cooccurrence_for_prompt(&merged, limit.min(20));
    let installed: std::collections::HashSet<String> = pack_mod_ids(&manifest).into_iter().collect();
    let suggestions = suggest_by_group_affinity(&merged, &installed, limit.min(8));

    Ok(json!({
        "localPairs": local,
        "networkPairs": network_pairs,
        "mergedPairs": merged,
        "groups": groups,
        "suggestions": suggestions,
        "network": network,
        "promptHint": prompt_hint,
        "strongMatchThreshold": STRONG_MATCH_THRESHOLD,
        "supabaseConfigured": integrations::swarm_supabase_configured(),
    }))
}

/// Suggest Modrinth slugs that fit groups already present in the pack (≥2 co-occur edges).
#[tauri::command(rename_all = "camelCase")]
pub async fn suggest_mods_from_trends(
    path: String,
    limit: Option<u32>,
) -> Result<Vec<String>, String> {
    let limit = limit.unwrap_or(8) as usize;
    let trends = get_creation_trends(path, Some(limit as u32)).await?;
    if let Some(arr) = trends.get("suggestions").and_then(|v| v.as_array()) {
        return Ok(arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .take(limit)
            .collect());
    }
    Ok(Vec::new())
}

/// After installing `modId`, suggest popular co-occurring mods the user may also want.
/// Primary source: Supabase `partners_for_mod` (launcher + MPI graph). Local pairs soft-boost / fallback.
#[tauri::command(rename_all = "camelCase")]
pub async fn suggest_partners_for_mod(
    path: String,
    mod_id: String,
    limit: Option<u32>,
) -> Result<Vec<serde_json::Value>, String> {
    let limit = limit.unwrap_or(8).clamp(1, 20) as usize;
    let project_dir = manifest_parent(&path)?;
    let (_manifest_path, manifest) = load_project_manifest(&path)?;
    let loader = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string();
    let mc = manifest.minecraft.version.clone();
    let installed: std::collections::HashSet<String> = pack_mod_ids(&manifest).into_iter().collect();

    // Bridge CF numeric / project ids → Modrinth-style slug when the mod is already in the pack.
    let mod_id = {
        let raw = mod_id.trim().to_string();
        let bridged = manifest.mods.iter().find_map(|m| {
            let id_match = m.id.eq_ignore_ascii_case(&raw)
                || m.source
                    .project_id
                    .as_deref()
                    .is_some_and(|p| p.eq_ignore_ascii_case(&raw));
            if !id_match {
                return None;
            }
            if !m.id.is_empty() && !m.id.chars().all(|c| c.is_ascii_digit()) {
                return Some(m.id.clone());
            }
            m.source.project_id.clone().filter(|p| !p.chars().all(|c| c.is_ascii_digit()))
        });
        bridged.unwrap_or(raw)
    };
    let mod_id_lower = mod_id.to_ascii_lowercase();

    let mut primary = Vec::new();
    if let (Some(url), Some(key)) = (
        integrations::swarm_supabase_url(),
        integrations::swarm_supabase_anon_key(),
    ) {
        if let Ok(batch) = tuffbox_core::swarm_supabase::partners_for_mod_supabase(
            &url,
            &key,
            &mod_id,
            limit as u32,
            Some(&loader),
            Some(&mc),
        )
        .await
        {
            primary = batch;
        }
        // Soft fill/boost from separate Modpack Index graph (does not replace TuffSwarm).
        if let Ok(mpi) = tuffbox_core::swarm_supabase::partners_for_mod_mpi_supabase(
            &url,
            &key,
            &mod_id,
            limit as u32,
            Some(&loader),
            Some(&mc),
            None,
        )
        .await
        {
            if primary.is_empty() {
                primary = mpi;
            } else if primary.len() < 3 {
                primary = tuffbox_core::mod_suggest::merge_partner_stats(&[primary, mpi], limit * 2);
            } else {
                primary = tuffbox_core::mod_suggest::soft_boost_partners(&primary, &mpi, limit * 2);
            }
        }
    }

    let local_pairs = top_cooccurrence_pairs(&project_dir, 80);
    let local = tuffbox_core::mod_suggest::partners_from_pairs(&mod_id, &local_pairs, limit * 2);
    let partners = tuffbox_core::mod_suggest::soft_boost_partners(&primary, &local, limit * 2);
    let filtered: Vec<_> = partners
        .into_iter()
        .filter(|p| {
            let slug = p.partner.to_ascii_lowercase();
            !installed.contains(&slug)
                && !installed.contains(&p.partner)
                && slug != mod_id_lower
        })
        .take(limit * 2)
        .collect();

    // Compat filter + name/icon enrichment (Modrinth versions for this MC+loader).
    let enriched = tokio::task::spawn_blocking({
        let mc = mc.clone();
        let loader = loader.clone();
        move || {
            let provider = tuffbox_core::ModrinthProvider::new();
            let query = tuffbox_core::ProviderSearchQuery {
                minecraft_version: Some(mc),
                loader: Some(loader),
                ..Default::default()
            };
            let mut out = Vec::new();
            for p in filtered {
                let slug = p.partner.clone();
                let versions = provider.get_versions(&slug, &query).unwrap_or_default();
                if versions.is_empty() {
                    continue;
                }
                let (name, icon) = provider
                    .get_project(&slug)
                    .map(|proj| (proj.name, proj.icon_url))
                    .unwrap_or_else(|_| (slug.clone(), None));
                out.push(json!({
                    "slug": slug,
                    "count": p.pack_count,
                    "name": name,
                    "iconUrl": icon,
                    "compatibleVersion": versions.first().map(|v| v.version_number.clone()),
                }));
                if out.len() >= limit {
                    break;
                }
            }
            out
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(enriched)
}

fn collect_crash_fix_timeline(
    project_dir: &Path,
    fingerprint_key: &str,
) -> Vec<String> {
    let mut lines = Vec::new();
    for ev in crate::pack_events::events_between_crash_and_resolve(project_dir, fingerprint_key) {
        lines.push(format!(
            "[EVENT] {} · {} · {}",
            ev.ts,
            ev.op,
            tuffbox_core::crash_kb::truncate_at_char_boundary(&ev.summary, 200)
        ));
    }
    let store = SnapshotStore::new(project_dir);
    if let Ok(snaps) = store.list() {
        for snap in snaps.into_iter().filter(|s| {
            s.tags
                .iter()
                .any(|t| t == "crash_fix" || t == "crash_resolved")
                && s.crash_fingerprint_key
                    .as_deref()
                    .map(|k| k == fingerprint_key || fingerprint_key == "unknown")
                    .unwrap_or(true)
        }) {
            let tag = if snap.tags.iter().any(|t| t == "crash_resolved") {
                "RESOLVED"
            } else {
                "FIX"
            };
            lines.push(format!(
                "[{tag}] {} · {} · {}",
                snap.created_at,
                snap.plan_source.as_deref().unwrap_or("-"),
                tuffbox_core::crash_kb::truncate_at_char_boundary(&snap.reason, 200)
            ));
        }
    }
    // Newest-first from store.list — reverse for chronological prompt.
    // Keep journal events (already chronological) then append reversed snaps carefully:
    // Prefer journal-first chronological; snap lines may duplicate — truncate.
    lines.truncate(48);
    lines
}

fn fallback_plan_from_resolution(
    rec: &CrashResolutionRecord,
    marker: Option<&LastCrashFixMarker>,
) -> ActionPlan {
    let player = is_player_trail_source(
        marker
            .and_then(|m| m.plan_source.as_deref())
            .or(rec.plan_source.as_deref()),
    );
    let actions = marker
        .map(|m| m.actions.clone())
        .unwrap_or_default();
    let explanation = if player {
        PLAYER_TRAIL_EXPLANATION.to_string()
    } else {
        rec.human_explanation.clone()
    };
    ActionPlan {
        schema_version: tuffbox_core::action_plan::ACTION_PLAN_SCHEMA_VERSION,
        human_explanation: explanation,
        confidence: if player { 0.45 } else { 0.65 },
        suspected_mods: Vec::new(),
        needs_user_review: true,
        source: Some(if player {
            "player_trail".into()
        } else {
            "distill_fallback".into()
        }),
        matched_case_ids: rec.matched_case_ids.clone(),
        actions,
        additional_context: Some(if player {
            "Player-driven fix: decoded disable covering from the crash→launch trail; root cause unknown.".into()
        } else {
            format!(
                "Fallback plan from recorded resolution ({}); AI distill unavailable.",
                rec.verified_by
            )
        }),
    }
}

/// Distill a verified crash resolution into a minimal ActionPlan for network share.
/// Local only — MUST NOT publish. User Confirm in UI triggers publish_experience_capsule.
///
/// Player-driven episodes: decode the trail as group tests (healthy ⇒ enabled are
/// clean) and share the remaining covering — do not invent a single root cause.
/// Verified Diagnose group-test sessions win over decode. AI-authored applies
/// may still be compressed via LLM.
#[tauri::command(rename_all = "camelCase")]
pub async fn distill_resolved_crash_plan(
    path: String,
    resolution_id: Option<String>,
) -> Result<serde_json::Value, String> {
    integrations::require_swarm_enabled()?;
    let project_dir = manifest_parent(&path)?;
    let (_manifest_path, manifest) = load_project_manifest(&path)?;
    let loader = format!("{:?}", manifest.loader.kind).to_lowercase();

    let resolutions = list_crash_resolutions(&project_dir)?;
    let rec = if let Some(id) = resolution_id.as_deref() {
        resolutions
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| format!("resolution not found: {id}"))?
    } else {
        resolutions
            .into_iter()
            .next()
            .ok_or_else(|| "no crash resolutions recorded yet".to_string())?
    };

    let marker_path = last_crash_fix_path(&project_dir);
    let mut marker: Option<LastCrashFixMarker> = std::fs::read_to_string(&marker_path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok());

    let inventory_ids = pack_mod_ids(&manifest);

    if let Some(session) = load_group_test_session(&project_dir) {
        if let Some(plan) = session.share_plan() {
            if let Some(ref mut m) = marker {
                m.plan_source = Some("group_test".into());
                m.human_explanation = plan.human_explanation.clone();
                m.actions = plan.actions.clone();
            }
        }
    }

    let player_trail = is_player_trail_source(
        marker
            .as_ref()
            .and_then(|m| m.plan_source.as_deref())
            .or(rec.plan_source.as_deref()),
    );
    let group_test = marker
        .as_ref()
        .and_then(|m| m.plan_source.as_deref())
        .or(rec.plan_source.as_deref())
        == Some("group_test");

    if player_trail {
        if let Some(ref mut m) = marker {
            let decoded = decode_player_share(&project_dir, &rec.fingerprint_key, &inventory_ids);
            let mut actions = decoded.disable_actions();
            actions.extend(decoded.extra_actions.clone());
            m.actions = actions;
            m.human_explanation = decoded.explanation.clone();
        }
    }

    let mut timeline = collect_crash_fix_timeline(&project_dir, &rec.fingerprint_key);
    if let Some(ref m) = marker {
        for a in &m.actions {
            timeline.push(format!(
                "[MARKER] {}",
                format_launcher_action_summary(a)
            ));
        }
    }
    for s in &rec.actions_summary {
        if !timeline.iter().any(|l| l.contains(s)) {
            timeline.push(format!("[RESOLVED] {s}"));
        }
    }

    let crash_excerpt = {
        let reports = project_dir.join("crash-reports");
        let mut text = String::new();
        if let Ok(entries) = std::fs::read_dir(&reports) {
            let mut newest: Option<(u64, PathBuf)> = None;
            for entry in entries.flatten() {
                let p = entry.path();
                if !p.is_file() {
                    continue;
                }
                let Some(mtime) = file_mtime_secs(&p) else {
                    continue;
                };
                if newest.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
                    newest = Some((mtime, p));
                }
            }
            if let Some((_, p)) = newest {
                text = std::fs::read_to_string(p).unwrap_or_default();
            }
        }
        if text.trim().is_empty() {
            text = std::fs::read_to_string(project_dir.join("logs").join("latest.log"))
                .unwrap_or_default();
        }
        tuffbox_core::crash_kb::scrub_privacy_sensitive(&tuffbox_core::crash_kb::smart_excerpt(
            &text, 2500,
        ))
    };

    let (mut plan, distill_source) = if group_test {
        (
            fallback_plan_from_resolution(&rec, marker.as_ref()),
            "group_test",
        )
    } else if player_trail {
        (
            fallback_plan_from_resolution(&rec, marker.as_ref()),
            "player_trail",
        )
    } else {
        let distill_ctx = tuffbox_core::ai_explanation::DistillContext {
            fingerprint_key: rec.fingerprint_key.clone(),
            mc_version: manifest.minecraft.version.clone(),
            loader: loader.clone(),
            crash_excerpt,
            action_timeline: timeline,
            resolved_summary: rec.human_explanation.clone(),
            verified_by: rec.verified_by.clone(),
            final_actions_summary: rec.actions_summary.clone(),
        };
        let prompt = tuffbox_core::ai_explanation::build_distill_prompt(&distill_ctx);
        let settings = integrations::get_integration_status().settings;

        match integrations::call_ai_crash_explain(&settings.ai, &prompt).await {
            Ok(value) => {
                let raw = serde_json::to_string(&value).unwrap_or_default();
                match tuffbox_core::action_plan::parse_action_plan(&raw) {
                    Ok(mut plan) => {
                        plan.needs_user_review = true;
                        if plan.source.as_deref() != Some("distill") {
                            plan.source = Some("distill".into());
                        }
                        if plan.matched_case_ids.is_empty() {
                            plan.matched_case_ids = rec.matched_case_ids.clone();
                        }
                        (plan, "ai")
                    }
                    Err(_) => (
                        fallback_plan_from_resolution(&rec, marker.as_ref()),
                        "fallback_parse",
                    ),
                }
            }
            Err(_) => (
                fallback_plan_from_resolution(&rec, marker.as_ref()),
                "fallback_ai",
            ),
        }
    };

    let missing_ids: Vec<String> = Vec::new();
    if group_test {
        plan.source = Some("group_test".into());
        if let Some(ref m) = marker {
            plan.human_explanation = m.human_explanation.clone();
            plan.actions = m.actions.clone();
        }
    } else if player_trail {
        plan.source = Some("player_trail".into());
        if let Some(ref m) = marker {
            plan.human_explanation = m.human_explanation.clone();
            plan.actions = m.actions.clone();
        }
        plan.confidence = plan.confidence.min(0.55);
    } else if plan.source.as_deref().is_none_or(|s| !s.starts_with("distill")) {
        plan.source = Some("distill".into());
    }
    let grounded =
        tuffbox_core::action_plan::ground_action_plan(plan, &inventory_ids, &missing_ids);
    let plan = grounded.plan;
    let validation = tuffbox_core::action_plan::validate_action_plan_with_inventory(
        &plan,
        &inventory_ids,
        &missing_ids,
    );
    Ok(json!({
        "schemaVersion": plan.schema_version,
        "humanExplanation": plan.human_explanation,
        "confidence": plan.confidence,
        "suspectedMods": plan.suspected_mods,
        "needsUserReview": plan.needs_user_review,
        "source": plan.source,
        "matchedCaseIds": plan.matched_case_ids,
        "actions": plan.actions,
        "additionalContext": plan.additional_context,
        "validation": validation,
        "groundingNotes": grounded.notes,
        "distilledFrom": if group_test {
            "group_test"
        } else if player_trail {
            "player_trail"
        } else {
            "user_history"
        },
        "distillSource": distill_source,
        "resolutionId": rec.id,
        "fingerprintKey": rec.fingerprint_key,
        "verifiedBy": rec.verified_by,
        "playerTrail": player_trail,
        "beta": true,
    }))
}

/// After analyze: if swarm on and strong match, persist pending plan.
pub fn maybe_persist_pending_from_plan(
    project_dir: &Path,
    plan: &ActionPlan,
    network_used: bool,
) -> Option<PathBuf> {
    if !integrations::swarm_enabled() || !network_used {
        return None;
    }
    // No artificial score inflation: the old `.max(STRONG_MATCH_THRESHOLD)`
    // promoted ANY plan that merely echoed a matchedCaseId to "strong",
    // persisting weak matches as pending auto-fix plans. A pending plan is
    // written only when the plan's own confidence genuinely clears the
    // threshold.
    let score = plan.confidence;
    maybe_write_pending_from_score(project_dir, plan, score)
        .ok()
        .flatten()
}
