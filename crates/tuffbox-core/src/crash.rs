use crate::{
    action_plan::is_invented_vanilla_resource_mod_id,
    change_plan::{ChangeAction, ChangeOption, ChangePlan, ChangeRisk},
    diagnostics::{Diagnostic, DiagnosticSeverity},
    graph::{DependencyGraph, NodeId},
    launch_history::{self, LaunchHistoryEntry},
    manifest::{ModSpec, ProjectManifest},
    mod_category,
    mod_conflict::{self, Conflict},
    resolve::{self, ResolveCtx},
    resolver::Resolver,
    snapshot::Snapshot,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use thiserror::Error;

const MAX_REPORT_BYTES: u64 = 4 * 1024 * 1024;
const LATEST_LOG_TAIL_LINES: usize = 900;
const DEBUG_LOG_TAIL_LINES: usize = 2400;
const MAX_EVIDENCE_PER_SUSPECT: usize = 8;
pub const LATEST_COMPATIBLE_VERSION: &str = "latest-compatible";

#[derive(Debug, Error)]
pub enum CrashError {
    #[error("failed to read crash reports directory {path}: {source}")]
    ReadCrashReportsDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read crash report {path}: {source}")]
    ReadCrashReport {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid crash report path: {0}")]
    InvalidReportPath(String),
    #[error("crash report is too large for inline analysis: {size} bytes")]
    ReportTooLarge { size: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashReportSummary {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CrashSignalKind {
    SuspectedMods,
    ModFile,
    CausedBy,
    Mixin,
    Exception,
    OpenGl,
    Performance,
    ResourceWarning,
    Entrypoint,
    LoaderMismatch,
    MissingDependency,
    ModVersionMismatch,
    MinecraftVersionMismatch,
    LoaderVersionMismatch,
    WrongLoader,
    OutOfMemory,
    Watchdog,
    PortConflict,
    EulaNotAccepted,
    CorruptJar,
    DuplicateMod,
    JavaVersion,
    TickingEntity,
    SideMismatch,
    ServerState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashSignal {
    pub source: String,
    pub line_number: usize,
    pub kind: CrashSignalKind,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuspectEvidence {
    pub source: String,
    pub line_number: usize,
    pub kind: CrashSignalKind,
    pub text: String,
    /// Positional / channel weight (0–100). Higher = stronger blame signal.
    #[serde(default = "default_evidence_weight")]
    pub weight: u8,
}

fn default_evidence_weight() -> u8 {
    50
}

/// How strongly a suspect is implicated in the crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum BlameRole {
    #[default]
    Related,
    Secondary,
    Primary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuspectedMod {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub file_name: Option<String>,
    pub known_in_manifest: bool,
    pub confidence: u8,
    pub evidence: Vec<SuspectEvidence>,
    /// Authors from manifest / jar metadata (may be empty).
    #[serde(default)]
    pub authors: Vec<String>,
    /// `primary` / `secondary` / `related` after multi-signal ranking.
    #[serde(default)]
    pub blame_role: BlameRole,
    /// Independent attribution channels that fired for this mod.
    #[serde(default)]
    pub match_sources: Vec<String>,
}

/// A plain-language explanation of a detected crash cause plus actionable
/// remediation steps the user can apply. Returned alongside suspects so the UI
/// can render a "Fix" panel without re-deriving meaning from raw signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisHint {
    pub id: String,
    pub title: String,
    pub severity: String,
    pub detail: String,
    pub steps: Vec<String>,
    /// Mod ids this hint is tied to (may be empty for system-level issues).
    pub related_mods: Vec<String>,
    /// Optional machine-actionable fix the UI can offer a button for.
    pub fix: Option<FixAction>,
    /// When several mods are implicated (e.g. multiple entrypoint/mixin
    /// suspects) this carries one fix action per known mod, so the UI can
    /// render a button for each. Falls back to `fix` when empty.
    pub fixes: Vec<FixAction>,
}

/// A fix the launcher can attempt automatically from the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixAction {
    pub kind: String,
    pub label: String,
    pub mod_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashReportSection {
    pub title: String,
    pub start_line: usize,
    pub end_line: usize,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashReportModEntry {
    pub id: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashReportAnalysis {
    pub summary: CrashReportSummary,
    pub content: String,
    pub sections: Vec<CrashReportSection>,
    pub mod_entries: Vec<CrashReportModEntry>,
    pub signals: Vec<CrashSignal>,
    pub suspected_mods: Vec<SuspectedMod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestLogAnalysis {
    pub path: PathBuf,
    pub exists: bool,
    pub tail: String,
    pub signals: Vec<CrashSignal>,
    pub suspected_mods: Vec<SuspectedMod>,
    pub hints: Vec<DiagnosisHint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldCrashCoords {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HsErrSummary {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<u64>,
    /// `oom` | `native` | `unknown`
    pub kind: String,
    pub problematic_frame: Option<String>,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportPackResult {
    pub path: PathBuf,
    pub file_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedSessionLog {
    pub session_id: String,
    pub started_at: String,
    pub exit_code: Option<i32>,
    pub analysis: LatestLogAnalysis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashDiagnosis {
    pub reports: Vec<CrashReportSummary>,
    pub selected_report: Option<CrashReportAnalysis>,
    pub latest_log: LatestLogAnalysis,
    pub launcher_log: LatestLogAnalysis,
    /// `logs/debug.log` when present (longer tail — early crash lines often live here).
    #[serde(default = "default_empty_log_analysis")]
    pub debug_log: LatestLogAnalysis,
    /// Archived logs from crashed launches (newest first).
    #[serde(default)]
    pub session_logs: Vec<ArchivedSessionLog>,
    /// Recent launch journal rows (newest first).
    #[serde(default)]
    pub launch_history: Vec<LaunchHistoryEntry>,
    pub suspected_mods: Vec<SuspectedMod>,
    pub hints: Vec<DiagnosisHint>,
    pub recent_snapshots: Vec<Snapshot>,
    pub graph_diagnostics: Vec<Diagnostic>,
    pub fix_plan: ChangePlan,
    /// Conflict pairs parsed from logs (victim/keeper + kind), the input to
    /// the policy-based resolver. Empty when the log names no explicit pair.
    #[serde(default)]
    pub conflicts: Vec<Conflict>,
    /// `latest_log` when a newer successful `logs/latest.log` supersedes crash-reports;
    /// otherwise `crash_report` when a report is selected/auto-picked.
    #[serde(default = "default_analysis_source")]
    pub analysis_source: String,
    /// True when a crash-report exists but was ignored because latest.log is newer.
    #[serde(default)]
    pub crash_report_stale: bool,
    /// True when `logs/latest.log` looks like a successful Minecraft session
    /// (no fresh crash markers). Diagnose should not push crash-log fix plans.
    #[serde(default)]
    pub session_healthy: bool,
    /// JVM fatal error logs (`hs_err_pid*.log`) in the instance root.
    #[serde(default)]
    pub hs_err_logs: Vec<HsErrSummary>,
    /// Entity/block coordinates parsed from the selected crash report (if any).
    #[serde(default)]
    pub world_coords: Option<WorldCrashCoords>,
    /// Allocated / max heap hint from System Details or hs_err (human-readable).
    #[serde(default)]
    pub memory_hint: Option<String>,
}

fn default_analysis_source() -> String {
    "crash_report".into()
}

fn default_empty_log_analysis() -> LatestLogAnalysis {
    LatestLogAnalysis {
        path: PathBuf::new(),
        exists: false,
        tail: String::new(),
        signals: Vec::new(),
        suspected_mods: Vec::new(),
        hints: Vec::new(),
    }
}

#[derive(Debug, Clone)]
struct ModCandidate<'a> {
    module: &'a ModSpec,
    tokens: Vec<String>,
    file_stem: Option<String>,
}

#[derive(Debug, Clone)]
struct SuspectAccumulator {
    id: String,
    name: String,
    version: Option<String>,
    file_name: Option<String>,
    known_in_manifest: bool,
    confidence: u8,
    evidence: Vec<SuspectEvidence>,
    authors: Vec<String>,
    match_sources: Vec<String>,
}

pub fn list_crash_reports(
    project_dir: impl AsRef<Path>,
) -> Result<Vec<CrashReportSummary>, CrashError> {
    let project_dir = project_dir.as_ref();
    let reports_dir = project_dir.join("crash-reports");
    if !reports_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut reports = Vec::new();
    for entry in fs::read_dir(&reports_dir).map_err(|source| CrashError::ReadCrashReportsDir {
        path: reports_dir.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| CrashError::ReadCrashReportsDir {
            path: reports_dir.clone(),
            source,
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.to_lowercase().ends_with(".txt") {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|source| CrashError::ReadCrashReport {
                path: path.clone(),
                source,
            })?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs());
        reports.push(CrashReportSummary {
            id: format!("crash-reports/{name}"),
            name,
            path,
            size: metadata.len(),
            modified,
        });
    }

    reports.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| b.name.cmp(&a.name))
    });
    Ok(reports)
}

pub fn analyze_crash_report(
    project_dir: impl AsRef<Path>,
    report_id: &str,
    manifest: &ProjectManifest,
) -> Result<CrashReportAnalysis, CrashError> {
    let project_dir = project_dir.as_ref();
    let relative = validate_report_id(report_id)?;
    let path = project_dir.join(&relative);
    let metadata = fs::metadata(&path).map_err(|source| CrashError::ReadCrashReport {
        path: path.clone(),
        source,
    })?;
    if metadata.len() > MAX_REPORT_BYTES {
        return Err(CrashError::ReportTooLarge {
            size: metadata.len(),
        });
    }
    let content = fs::read_to_string(&path).map_err(|source| CrashError::ReadCrashReport {
        path: path.clone(),
        source,
    })?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs());
    let summary = CrashReportSummary {
        id: report_id.to_string(),
        name: Path::new(report_id)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(report_id)
            .to_string(),
        path,
        size: metadata.len(),
        modified,
    };
    let sections = parse_crash_sections(&content);
    let mod_entries = parse_crash_mod_entries(&content, &sections);
    let (signals, suspected_mods) = analyze_text_for_suspects(&content, report_id, manifest);
    Ok(CrashReportAnalysis {
        summary,
        content,
        sections,
        mod_entries,
        signals,
        suspected_mods,
    })
}

pub fn analyze_latest_log(
    project_dir: impl AsRef<Path>,
    manifest: &ProjectManifest,
) -> LatestLogAnalysis {
    analyze_log_file(
        project_dir.as_ref().join("logs").join("latest.log"),
        "logs/latest.log",
        manifest,
        LATEST_LOG_TAIL_LINES,
    )
}

pub fn analyze_launcher_log(
    project_dir: impl AsRef<Path>,
    manifest: &ProjectManifest,
) -> LatestLogAnalysis {
    let project_dir = project_dir.as_ref();
    let candidates = [
        project_dir.join("launcher.log"),
        project_dir.join("launcher_log.txt"),
        project_dir.join("logs").join("launcher.log"),
        project_dir.join("logs").join("launcher_log.txt"),
    ];
    let log_path = candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .unwrap_or_else(|| project_dir.join("logs").join("launcher.log"));
    analyze_log_file(log_path, "launcher.log", manifest, LATEST_LOG_TAIL_LINES)
}

pub fn analyze_debug_log(
    project_dir: impl AsRef<Path>,
    manifest: &ProjectManifest,
) -> LatestLogAnalysis {
    analyze_log_file(
        project_dir.as_ref().join("logs").join("debug.log"),
        "logs/debug.log",
        manifest,
        DEBUG_LOG_TAIL_LINES,
    )
}

fn analyze_log_file(
    path: PathBuf,
    source: &str,
    manifest: &ProjectManifest,
    tail_lines: usize,
) -> LatestLogAnalysis {
    let exists = path.is_file();
    let tail = if exists {
        crate::process::read_log_tail(&path, tail_lines).unwrap_or_default()
    } else {
        String::new()
    };
    let (signals, suspected_mods) = analyze_text_for_suspects(&tail, source, manifest);
    let hints = build_hints(&signals, &suspected_mods);
    LatestLogAnalysis {
        path,
        exists,
        tail,
        signals,
        suspected_mods,
        hints,
    }
}

fn load_archived_session_logs(
    project_dir: &Path,
    manifest: &ProjectManifest,
) -> Vec<ArchivedSessionLog> {
    launch_history::list_launch_history_default(project_dir)
        .into_iter()
        .filter(|e| e.archived)
        .filter_map(|entry| {
            let log_path = launch_history::archived_session_log_path(project_dir, &entry.id)?;
            let source = format!("session/{}", entry.id);
            let analysis = analyze_log_file(log_path, &source, manifest, DEBUG_LOG_TAIL_LINES);
            Some(ArchivedSessionLog {
                session_id: entry.id,
                started_at: entry.started_at,
                exit_code: entry.exit_code,
                analysis,
            })
        })
        .collect()
}

pub fn build_crash_diagnosis(
    project_dir: impl AsRef<Path>,
    manifest: &ProjectManifest,
    selected_report_id: Option<&str>,
    recent_snapshots: Vec<Snapshot>,
) -> Result<CrashDiagnosis, CrashError> {
    let project_dir = project_dir.as_ref();
    let mut reports = list_crash_reports(project_dir)?;
    let hs_err_logs = list_hs_err_logs(project_dir)?;
    // Surface hs_err in the same picker as crash-reports (id prefix hs_err/).
    for hs in &hs_err_logs {
        reports.push(CrashReportSummary {
            id: hs.id.clone(),
            name: format!("[JVM] {}", hs.name),
            path: hs.path.clone(),
            size: hs.size,
            modified: hs.modified,
        });
    }
    reports.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| b.name.cmp(&a.name))
    });

    let latest_log = analyze_latest_log(project_dir, manifest);
    let launcher_log = analyze_launcher_log(project_dir, manifest);
    let debug_log = analyze_debug_log(project_dir, manifest);
    let session_logs = load_archived_session_logs(project_dir, manifest);
    let launch_history = launch_history::list_launch_history_default(project_dir);

    // Archived crashed sessions appear in the source picker alongside crash-reports.
    for archived in &session_logs {
        let label = format_session_picker_label(archived);
        reports.push(CrashReportSummary {
            id: format!(
                "{}{}",
                launch_history::SESSION_REPORT_PREFIX,
                archived.session_id
            ),
            name: label,
            path: archived.analysis.path.clone(),
            size: 0,
            modified: None,
        });
    }

    // Explicit user pick always wins. Special id `__latest_log__` forces live-log
    // analysis (AI Explain / Diagnose sidebar) and never auto-selects a crash file.
    // `__launcher_log__` forces launcher.log.
    // `session/<id>` selects an archived crashed-session log.
    let force_latest_log = selected_report_id == Some("__latest_log__");
    let force_launcher_log = selected_report_id == Some("__launcher_log__");
    let force_archived_session = selected_report_id
        .filter(|id| launch_history::is_session_report_id(id))
        .and_then(|id| launch_history::session_id_from_report_id(id).map(str::to_string));
    let explicit = selected_report_id
        .filter(|id| !id.is_empty() && *id != "__latest_log__" && *id != "__launcher_log__")
        .filter(|id| {
            launch_history::is_session_report_id(id)
                || reports.iter().any(|report| report.id == *id)
                || validate_report_id(id).is_ok()
        });
    let newest = reports.iter().find(|r| r.id.starts_with("crash-reports/"));
    let stale = newest
        .map(|r| latest_log_supersedes_crash(project_dir, Some(r.path.as_path()), &latest_log.tail))
        .unwrap_or(false);

    let archived_session_analysis = force_archived_session.as_ref().and_then(|sid| {
        session_logs
            .iter()
            .find(|s| s.session_id == *sid)
            .map(|s| s.analysis.clone())
    });

    let selected_id = if force_latest_log || force_launcher_log || force_archived_session.is_some()
    {
        None
    } else if let Some(id) = explicit {
        Some(id)
    } else if stale {
        None
    } else {
        newest.map(|report| report.id.as_str())
    };

    let selected_report = selected_id
        .filter(|id| !launch_history::is_session_report_id(id))
        .map(|id| analyze_crash_report(project_dir, id, manifest))
        .transpose()?;

    let analysis_source = if force_launcher_log {
        "launcher_log".to_string()
    } else if force_archived_session.is_some() {
        "archived_session".to_string()
    } else if selected_report
        .as_ref()
        .map(|r| r.summary.id.starts_with("hs_err/"))
        .unwrap_or(false)
    {
        "hs_err".to_string()
    } else if selected_report.is_some() {
        "crash_report".to_string()
    } else if stale && session_logs.first().is_some() {
        "archived_session".to_string()
    } else {
        "latest_log".to_string()
    };

    // Healthy live session (and user did not explicitly open an old crash):
    // suppress crash-log suspects / fix plans so Diagnose doesn't nag about
    // a crash that was already fixed and successfully relaunched.
    let session_healthy = explicit.is_none()
        && force_archived_session.is_none()
        && !force_launcher_log
        && log_indicates_healthy_session(&latest_log.tail);

    // When latest.log is a successful relaunch but we archived the crashed session,
    // use the archived log for suspect/hint analysis (stale crash-report alone is misleading).
    let archived_fallback = if session_healthy && stale {
        session_logs.first().map(|s| s.analysis.clone())
    } else {
        None
    };

    let has_archived_crash_context =
        archived_session_analysis.is_some() || archived_fallback.is_some();

    let mut suspect_sets = Vec::new();
    let mut combined_signals = Vec::new();
    let analyze_crash = !session_healthy || has_archived_crash_context;

    if analyze_crash {
        if let Some(report) = &selected_report {
            suspect_sets.push(report.suspected_mods.clone());
            combined_signals.extend(report.signals.clone());
        }
        if let Some(archived) = archived_session_analysis
            .clone()
            .or_else(|| archived_fallback.clone())
        {
            suspect_sets.push(archived.suspected_mods.clone());
            combined_signals.extend(archived.signals.clone());
        } else if force_launcher_log {
            suspect_sets.push(launcher_log.suspected_mods.clone());
            combined_signals.extend(launcher_log.signals.clone());
        } else if !session_healthy {
            suspect_sets.push(latest_log.suspected_mods.clone());
            combined_signals.extend(latest_log.signals.clone());
        }
        if debug_log.exists && !debug_log.tail.is_empty() {
            suspect_sets.push(debug_log.suspected_mods.clone());
            combined_signals.extend(debug_log.signals.clone());
        }
        suspect_sets.push(launcher_log.suspected_mods.clone());
        combined_signals.extend(launcher_log.signals.clone());
    }

    let suspected_mods = merge_suspected_mods(suspect_sets.into_iter().flatten());
    let log_for_enrich = archived_session_analysis
        .or_else(|| archived_fallback.clone())
        .unwrap_or_else(|| latest_log.clone());
    let suspected_mods = if session_healthy && !has_archived_crash_context {
        suspected_mods
    } else {
        enrich_diagnosis_suspects(
            project_dir,
            manifest,
            &selected_report,
            &log_for_enrich,
            suspected_mods,
        )
    };

    let graph = DependencyGraph::from_manifest(manifest);
    let graph_diagnostics = Resolver::analyze_project(manifest, &graph);
    let conflicts = build_conflicts_from_signals(&combined_signals);
    let fix_plan = if session_healthy && !has_archived_crash_context {
        ChangePlan {
            summary: "Minecraft launched successfully — no crash-log fixes needed. Remaining items below are dependency-graph checks only.".to_string(),
            risk: ChangeRisk::Low,
            actions: Vec::new(),
            requires_snapshot: false,
        options: Vec::new(),
        }
    } else {
        create_crash_fix_plan(
            &graph,
            &graph_diagnostics,
            &suspected_mods,
            &combined_signals,
        )
    };

    let mut hints = if session_healthy && !has_archived_crash_context {
        Vec::new()
    } else {
        build_hints(&combined_signals, &suspected_mods)
    };
    if session_healthy {
        hints.push(DiagnosisHint {
            id: "session-healthy".into(),
            title: "Build launched successfully".into(),
            severity: "info".into(),
            detail: if has_archived_crash_context {
                "latest.log shows a healthy session. Fix suggestions below come from the archived crashed launch — verify before applying.".into()
            } else {
                "latest.log shows a healthy Minecraft session with no fresh crash markers. \
                Historical crash-reports are kept for reference but are not used for fix suggestions."
                    .into()
            },
            steps: vec![
                "Play normally — no crash-log actions are required.".into(),
                "Open a crash-report or archived session below to revisit a past failure.".into(),
                "Use Live logs while the game is running to watch the current session.".into(),
            ],
            related_mods: Vec::new(),
            fix: None,
            fixes: Vec::new(),
        });
    }

    let world_coords = selected_report
        .as_ref()
        .and_then(|r| extract_world_coords(&r.content))
        .or_else(|| {
            archived_fallback
                .as_ref()
                .and_then(|a| extract_world_coords(&a.tail))
        });
    let memory_hint = selected_report
        .as_ref()
        .and_then(|r| extract_memory_hint(&r.content))
        .or_else(|| extract_memory_hint(&latest_log.tail))
        .or_else(|| {
            archived_fallback
                .as_ref()
                .and_then(|a| extract_memory_hint(&a.tail))
        });

    Ok(CrashDiagnosis {
        reports,
        selected_report,
        latest_log,
        launcher_log,
        debug_log,
        session_logs,
        launch_history,
        suspected_mods,
        hints,
        recent_snapshots,
        graph_diagnostics,
        fix_plan,
        conflicts,
        analysis_source,
        crash_report_stale: stale && explicit.is_none(),
        session_healthy,
        hs_err_logs,
        world_coords,
        memory_hint,
    })
}

fn format_session_picker_label(archived: &ArchivedSessionLog) -> String {
    let ts = archived.started_at.chars().take(16).collect::<String>();
    let code = archived
        .exit_code
        .map(|c| format!(" exit {c}"))
        .unwrap_or_default();
    format!("Crashed session {ts}{code}")
}

/// True when `logs/latest.log` is newer than `crash_report_path` and looks like a
/// post-crash successful (or at least non-crashed) session — so Diagnose should
/// not keep recommending fixes from the old crash-report.
pub fn latest_log_supersedes_crash(
    project_dir: &Path,
    crash_report_path: Option<&Path>,
    latest_log_tail: &str,
) -> bool {
    let latest_path = project_dir.join("logs").join("latest.log");
    let Some(latest_mtime) = file_mtime_secs(&latest_path) else {
        return false;
    };
    let Some(crash_path) = crash_report_path else {
        return false;
    };
    let Some(crash_mtime) = file_mtime_secs(crash_path) else {
        return false;
    };
    if latest_mtime <= crash_mtime {
        return false;
    }
    // Newer log without a fresh crash dump → treat crash-report as historical.
    !log_has_fresh_crash_markers(latest_log_tail)
        || log_indicates_successful_session(latest_log_tail)
}

fn file_mtime_secs(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

fn log_indicates_successful_session(log: &str) -> bool {
    let l = log.to_ascii_lowercase();
    // Vanilla / Fabric / Quilt / NeoForge “game reached playable state” markers.
    l.contains("sound engine started")
        || (l.contains("done (") && (l.contains("for help") || l.contains("!")))
        || l.contains("joining world")
        || l.contains("logged in with entity id")
        || l.contains("openal initialized")
        || l.contains("narrator library")
        || l.contains("completely loaded in")
        || l.contains("[chat]")
        || (l.contains("reloading resource manager")
            && (l.contains("sound engine") || l.contains("openal") || l.contains("done (")))
}

/// Session is healthy when the live log shows a successful boot and no fresh crash dump.
pub fn log_indicates_healthy_session(log: &str) -> bool {
    !log.trim().is_empty()
        && log_indicates_successful_session(log)
        && !log_has_fresh_crash_markers(log)
}

/// True when `latest.log` / crash text contains a fresh JVM/Minecraft crash dump.
pub fn log_has_fresh_crash_markers(log: &str) -> bool {
    let l = log.to_ascii_lowercase();
    l.contains("---- minecraft crash report ----")
        || l.contains("#@!@# game crashed")
        || l.contains("game crashed!")
        || l.contains("crash report saved to:")
}

pub fn parse_crash_sections(text: &str) -> Vec<CrashReportSection> {
    let lines = text.lines().collect::<Vec<_>>();
    let mut starts: Vec<(usize, String)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("-- ") && trimmed.ends_with(" --") {
            starts.push((index + 1, trimmed.trim_matches('-').trim().to_string()));
        } else if let Some(title) = forge_section_title(trimmed) {
            starts.push((index + 1, title.to_string()));
        }
    }
    starts.sort_by_key(|(line, _)| *line);
    starts.dedup_by_key(|(line, _)| *line);

    let mut sections = Vec::new();
    for (idx, (start_line, title)) in starts.iter().enumerate() {
        let end_line = starts
            .get(idx + 1)
            .map(|(next, _)| next.saturating_sub(1))
            .unwrap_or(lines.len());
        let preview = lines
            .iter()
            .skip(*start_line)
            .take(end_line.saturating_sub(*start_line).min(10))
            .map(|line| line.trim_end())
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(CrashReportSection {
            title: title.clone(),
            start_line: *start_line,
            end_line,
            preview,
        });
    }
    sections
}

fn forge_section_title(line: &str) -> Option<&'static str> {
    let heading = line.strip_suffix(':').unwrap_or(line).trim();
    if heading.eq_ignore_ascii_case("Forge Mod List") {
        Some("Forge Mod List")
    } else if heading.eq_ignore_ascii_case("FML Mod Loading") {
        Some("FML Mod Loading")
    } else if heading.eq_ignore_ascii_case("NeoForge Mod List") {
        Some("NeoForge Mod List")
    } else if heading.eq_ignore_ascii_case("Memory") {
        Some("Memory")
    } else if heading.eq_ignore_ascii_case("JVM Flags") {
        Some("JVM Flags")
    } else if heading.eq_ignore_ascii_case("CPU") {
        Some("CPU")
    } else if heading.eq_ignore_ascii_case("Processor") {
        Some("Processor")
    } else {
        None
    }
}

/// Parse Forge/NeoForge crash report mod table format.
/// Forge crash reports often have one of two table formats:
///   1. "| ID | Name | Version |" pipe-separated tables
///   2. "Mod List:" followed by indented name-version pairs
fn parse_forge_crash_mods(text: &str) -> Vec<CrashReportModEntry> {
    let mut entries = Vec::new();
    let lines: Vec<&str> = text.lines().collect();

    // Pattern 1: pipe table
    let mut in_pipe_table = false;
    for (_line_no, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('|') && trimmed.contains(" ID ") {
            in_pipe_table = true;
            continue;
        }
        if in_pipe_table {
            if !trimmed.starts_with('|') || trimmed.len() < 5 {
                if !entries.is_empty() {
                    break;
                }
                in_pipe_table = false;
                continue;
            }
            let cells: Vec<&str> = trimmed
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim())
                .collect();
            if cells.len() >= 2 && !cells[0].is_empty() && !cells[0].contains('-') {
                entries.push(CrashReportModEntry {
                    id: cells[0].to_string(),
                    name: cells
                        .get(1)
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string()),
                    version: cells
                        .get(2)
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string()),
                    raw: trimmed.to_string(),
                });
                if entries.len() >= 200 {
                    break;
                }
            }
        }
    }

    // Pattern 2: "Mod List:" followed by list
    if entries.is_empty() {
        let mut in_mod_list = false;
        for line in lines {
            let trimmed = line.trim();
            if trimmed.eq_ignore_ascii_case("Mod List:") || trimmed.starts_with("Mod List:") {
                in_mod_list = true;
                continue;
            }
            if in_mod_list {
                if trimmed.is_empty() || trimmed.starts_with("--") || trimmed.starts_with("[") {
                    if !entries.is_empty() {
                        break;
                    }
                    in_mod_list = false;
                    continue;
                }
                // Format: "- modid" or "modid (version)"
                let stripped = trimmed.trim_start_matches('-').trim();
                if let Some((name, ver)) = stripped.rsplit_once('(') {
                    let ver = ver.trim_end_matches(')').trim();
                    entries.push(CrashReportModEntry {
                        id: name.trim().to_string(),
                        name: None,
                        version: Some(ver.to_string()),
                        raw: trimmed.to_string(),
                    });
                } else {
                    entries.push(CrashReportModEntry {
                        id: stripped.to_string(),
                        name: None,
                        version: None,
                        raw: trimmed.to_string(),
                    });
                }
                if entries.len() >= 200 {
                    break;
                }
            }
        }
    }

    entries
}

pub fn parse_crash_mod_entries(
    text: &str,
    sections: &[CrashReportSection],
) -> Vec<CrashReportModEntry> {
    // Try Forge/NeoForge table format first
    let forge_entries = parse_forge_crash_mods(text);
    if !forge_entries.is_empty() {
        return forge_entries;
    }
    // Fallback: vanilla crash report -- Mods -- section
    let lines = text.lines().collect::<Vec<_>>();
    let Some(section) = sections
        .iter()
        .find(|section| section.title.eq_ignore_ascii_case("mods"))
    else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for line in lines
        .iter()
        .skip(section.start_line)
        .take(section.end_line.saturating_sub(section.start_line))
    {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("Mod") || trimmed.starts_with('-') {
            continue;
        }
        // Handle pipe-format lines inside the vanilla -- Mods -- section
        // (some hybrid reports mix formats)
        if trimmed.starts_with('|') {
            let cells: Vec<&str> = trimmed
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim())
                .collect();
            if cells.len() >= 2 && !cells[0].is_empty() && !cells[0].contains('-') {
                entries.push(CrashReportModEntry {
                    id: cells[0].to_string(),
                    name: cells
                        .get(1)
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string()),
                    version: cells
                        .get(2)
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string()),
                    raw: trimmed.to_string(),
                });
            }
            continue;
        }
        let normalized =
            trimmed.trim_matches(|c: char| c == '\t' || c == '|' || c == '[' || c == ']');
        let parts = normalized.split_whitespace().collect::<Vec<_>>();
        if parts.is_empty() {
            continue;
        }
        let id = parts[0]
            .trim_matches(|c: char| c == ':' || c == '|')
            .to_string();
        if id.len() < 2 || id.contains("----") {
            continue;
        }
        let version = parts
            .iter()
            .rev()
            .find(|part| part.chars().any(|c| c.is_ascii_digit()))
            .map(|part| part.trim_matches('|').to_string());
        let name = if parts.len() > 2 {
            Some(
                parts[1..parts.len().saturating_sub(1)]
                    .join(" ")
                    .trim()
                    .to_string(),
            )
            .filter(|s| !s.is_empty())
        } else {
            None
        };
        entries.push(CrashReportModEntry {
            id,
            name,
            version,
            raw: trimmed.to_string(),
        });
    }
    entries.truncate(300);
    entries
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogSection {
    General,
    ModList,
    Description,
    StackTrace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StackContext {
    Plain,
    DescriptionLine,
    PrimaryHead,
    PrimaryFrame,
    CausedByHead,
    CausedByFrame,
}

fn detect_log_section(line: &str, current: LogSection) -> LogSection {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    if trimmed.starts_with("-- ") && trimmed.ends_with(" --") {
        let title = trimmed.trim_matches('-').trim().to_ascii_lowercase();
        if title.contains("mod list")
            || title.contains("mods")
            || title.contains("fabric mods")
            || title.contains("quilt mods")
        {
            return LogSection::ModList;
        }
        if title.contains("head") || title.contains("stacktrace") || title.contains("stack trace") {
            return LogSection::StackTrace;
        }
        if title.contains("system details") {
            return LogSection::General;
        }
    }
    if lower.starts_with("mod list:")
        || lower.starts_with("fabric mods:")
        || lower.starts_with("quilt mods:")
        || lower.contains("| mod id |")
        || lower.contains("|    mod id    |")
    {
        return LogSection::ModList;
    }
    if lower.starts_with("description:") {
        return LogSection::Description;
    }
    if trimmed.starts_with('\t') || trimmed.starts_with("at ") || lower.contains("caused by:") {
        return LogSection::StackTrace;
    }
    current
}

fn is_mod_list_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|')
        && trimmed.contains('|')
        && !trimmed.to_ascii_lowercase().contains("mod id")
}

/// First mod id from a pipe-formatted `-- Mods --` table row, e.g. `| fabric-api | 0.92.0 |`.
fn extract_mod_list_id(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return None;
    }
    let cells: Vec<&str> = trimmed
        .trim_matches('|')
        .split('|')
        .map(|c| c.trim())
        .collect();
    let id = cells.first()?;
    if id.is_empty() || id.to_ascii_lowercase().contains("mod id") {
        return None;
    }
    Some(id.to_string())
}

fn stack_context_for_line(
    line: &str,
    section: LogSection,
    in_caused_by: bool,
    primary_frames: usize,
) -> (StackContext, bool, usize) {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    if section == LogSection::Description || lower.starts_with("description:") {
        return (StackContext::DescriptionLine, in_caused_by, primary_frames);
    }
    if lower.contains("caused by:") {
        return (StackContext::CausedByHead, true, 0);
    }
    let is_frame = trimmed.starts_with("at ")
        || trimmed.starts_with("\tat ")
        || trimmed.contains("knot//")
        || (trimmed.starts_with('-') && trimmed.contains('('));
    if is_frame {
        if in_caused_by {
            return (StackContext::CausedByFrame, true, primary_frames);
        }
        if primary_frames == 0 {
            return (StackContext::PrimaryHead, false, 1);
        }
        return (
            StackContext::PrimaryFrame,
            false,
            primary_frames.saturating_add(1),
        );
    }
    if in_caused_by && !is_frame && !lower.contains("exception") {
        // Plain line after Caused by before frames — still part of caused-by block.
        return (StackContext::CausedByHead, true, primary_frames);
    }
    (
        StackContext::Plain,
        false,
        if is_frame { primary_frames } else { 0 },
    )
}

fn evidence_weight(ctx: StackContext, section: LogSection) -> u8 {
    match (ctx, section) {
        (StackContext::DescriptionLine, _) => 98,
        (StackContext::CausedByHead, _) => 92,
        (StackContext::CausedByFrame, _) => 88,
        (StackContext::PrimaryHead, _) => 85,
        (StackContext::PrimaryFrame, _) => 72,
        (_, LogSection::ModList) => 12,
        (StackContext::Plain, LogSection::StackTrace) => 40,
        _ => 50,
    }
}

fn scaled_confidence(base: u8, weight: u8) -> u8 {
    ((base as u16 * weight as u16) / 100).min(99) as u8
}

fn should_skip_token_match(section: LogSection, line: &str) -> bool {
    section == LogSection::ModList || is_mod_list_table_row(line)
}

pub fn analyze_text_for_suspects(
    text: &str,
    source: &str,
    manifest: &ProjectManifest,
) -> (Vec<CrashSignal>, Vec<SuspectedMod>) {
    let candidates = build_candidates(manifest);
    let mut signals = Vec::new();
    let mut suspects: BTreeMap<String, SuspectAccumulator> = BTreeMap::new();
    let mut section = LogSection::General;
    let mut in_caused_by = false;
    let mut primary_frames = 0usize;

    for (index, line) in text.lines().enumerate() {
        section = detect_log_section(line, section);
        if section == LogSection::ModList {
            if let Some(id) = extract_mod_list_id(line) {
                if let Some(candidate) = candidates
                    .iter()
                    .find(|c| c.module.id == id || c.tokens.iter().any(|t| t == &id))
                {
                    let line_number = index + 1;
                    add_manifest_suspect(
                        &mut suspects,
                        candidate.module,
                        evidence_weighted(
                            source,
                            line_number,
                            CrashSignalKind::SuspectedMods,
                            line,
                            12,
                        ),
                        scaled_confidence(confidence_for_kind(CrashSignalKind::SuspectedMods), 12),
                    );
                }
            }
            continue;
        }
        let (stack_ctx, next_caused, next_primary_frames) =
            stack_context_for_line(line, section, in_caused_by, primary_frames);
        in_caused_by = next_caused;
        primary_frames = next_primary_frames;

        let line_number = index + 1;
        let Some(kind) = classify_signal_line(line) else {
            continue;
        };

        let signal = CrashSignal {
            source: source.to_string(),
            line_number,
            kind,
            text: line.trim().to_string(),
        };
        signals.push(signal);

        let weight = evidence_weight(stack_ctx, section);
        let skip_tokens = should_skip_token_match(section, line);

        if matches!(
            kind,
            CrashSignalKind::Entrypoint
                | CrashSignalKind::LoaderMismatch
                | CrashSignalKind::CausedBy
        ) {
            for mod_id in extract_quoted_mod_ids(line) {
                if let Some(candidate) = candidates
                    .iter()
                    .find(|candidate| candidate.tokens.iter().any(|t| t == &mod_id))
                {
                    add_manifest_suspect(
                        &mut suspects,
                        candidate.module,
                        evidence_weighted(source, line_number, kind, line, weight),
                        scaled_confidence(96, weight),
                    );
                } else if !is_noise_token(&mod_id) {
                    add_inferred_suspect(
                        &mut suspects,
                        &mod_id,
                        None,
                        evidence_weighted(source, line_number, kind, line, weight),
                        scaled_confidence(82, weight),
                    );
                }
            }
        }

        if !skip_tokens
            && !matches!(
                kind,
                CrashSignalKind::Performance | CrashSignalKind::ResourceWarning
            )
        {
            for candidate in &candidates {
                if candidate_matches_line(candidate, line) {
                    add_manifest_suspect(
                        &mut suspects,
                        candidate.module,
                        evidence_weighted(source, line_number, kind, line, weight),
                        scaled_confidence(confidence_for_kind(kind), weight),
                    );
                }
            }

            if matches!(
                kind,
                CrashSignalKind::Exception
                    | CrashSignalKind::Mixin
                    | CrashSignalKind::Entrypoint
                    | CrashSignalKind::CausedBy
            ) {
                for pkg in extract_java_packages(line) {
                    for candidate in &candidates {
                        if candidate_matches_java_package(candidate, &pkg) {
                            let pkg_weight = weight.max(80);
                            add_manifest_suspect(
                                &mut suspects,
                                candidate.module,
                                evidence_weighted(source, line_number, kind, line, pkg_weight),
                                scaled_confidence(88, pkg_weight),
                            );
                            if let Some(entry) =
                                suspects.get_mut(&normalize_token(&candidate.module.id))
                            {
                                push_match_source(entry, "package");
                            }
                        }
                    }
                }
            }
        }

        if matches!(kind, CrashSignalKind::ModFile) {
            for jar_name in extract_jar_names(line) {
                if let Some(candidate) = candidates
                    .iter()
                    .find(|candidate| jar_matches_candidate(&jar_name, candidate))
                {
                    add_manifest_suspect(
                        &mut suspects,
                        candidate.module,
                        evidence_weighted(source, line_number, kind, line, weight.max(100)),
                        scaled_confidence(92, weight.max(100)),
                    );
                } else {
                    let inferred = infer_id_from_jar(&jar_name);
                    if !inferred.is_empty() && !is_noise_token(&inferred) {
                        add_inferred_suspect(
                            &mut suspects,
                            &inferred,
                            Some(jar_name),
                            evidence_weighted(source, line_number, kind, line, weight),
                            scaled_confidence(68, weight),
                        );
                    }
                }
            }
        }

        if matches!(kind, CrashSignalKind::Entrypoint) {
            for mod_id in extract_quoted_mod_ids(line) {
                if let Some(candidate) = candidates
                    .iter()
                    .find(|candidate| candidate.tokens.iter().any(|t| t == &mod_id))
                {
                    add_manifest_suspect(
                        &mut suspects,
                        candidate.module,
                        evidence_weighted(source, line_number, kind, line, weight.max(100)),
                        scaled_confidence(confidence_for_kind(kind), weight.max(100)),
                    );
                }
            }
        }

        if matches!(
            kind,
            CrashSignalKind::MissingDependency
                | CrashSignalKind::ModVersionMismatch
                | CrashSignalKind::MinecraftVersionMismatch
                | CrashSignalKind::LoaderVersionMismatch
                | CrashSignalKind::WrongLoader
        ) {
            for mod_id in extract_named_mods(line) {
                if let Some(candidate) = candidates
                    .iter()
                    .find(|candidate| candidate.tokens.iter().any(|t| t == &mod_id))
                {
                    add_manifest_suspect(
                        &mut suspects,
                        candidate.module,
                        evidence_weighted(source, line_number, kind, line, weight.max(100)),
                        scaled_confidence(confidence_for_kind(kind), weight.max(100)),
                    );
                } else if !is_noise_token(&mod_id) && !is_invented_vanilla_resource_mod_id(&mod_id)
                {
                    add_inferred_suspect(
                        &mut suspects,
                        &mod_id,
                        None,
                        evidence_weighted(source, line_number, kind, line, weight),
                        scaled_confidence(confidence_for_kind(kind).saturating_sub(8), weight),
                    );
                }
            }
        }

        if matches!(
            kind,
            CrashSignalKind::Mixin | CrashSignalKind::SuspectedMods | CrashSignalKind::CausedBy
        ) {
            for token in tokenize(line) {
                if token.len() < 3 || is_noise_token(&token) {
                    continue;
                }
                if let Some(candidate) = candidates
                    .iter()
                    .find(|candidate| candidate.tokens.iter().any(|t| t == &token))
                {
                    add_manifest_suspect(
                        &mut suspects,
                        candidate.module,
                        evidence_weighted(source, line_number, kind, line, weight.max(90)),
                        scaled_confidence(confidence_for_kind(kind), weight.max(90)),
                    );
                }
            }
        }
    }

    let suspected_mods = suspects
        .into_values()
        .map(accumulator_to_suspect)
        .collect::<Vec<_>>();

    (signals, merge_suspected_mods(suspected_mods))
}

pub fn merge_suspected_mods(mods: impl IntoIterator<Item = SuspectedMod>) -> Vec<SuspectedMod> {
    let mut by_id: BTreeMap<String, SuspectAccumulator> = BTreeMap::new();
    for module in mods {
        let key = compact_token(&normalize_token(&module.id));
        let entry = by_id.entry(key).or_insert_with(|| SuspectAccumulator {
            id: module.id.clone(),
            name: module.name.clone(),
            version: module.version.clone(),
            file_name: module.file_name.clone(),
            known_in_manifest: module.known_in_manifest,
            confidence: 0,
            evidence: Vec::new(),
            authors: module.authors.clone(),
            match_sources: module.match_sources.clone(),
        });
        entry.confidence = entry.confidence.max(module.confidence);
        if module.known_in_manifest && !entry.known_in_manifest {
            entry.id = module.id.clone();
            entry.name = module.name.clone();
            entry.version = module.version.clone();
            entry.file_name = module.file_name.clone();
            if !module.authors.is_empty() {
                entry.authors = module.authors.clone();
            }
        }
        entry.known_in_manifest |= module.known_in_manifest;
        if entry.version.is_none() {
            entry.version = module.version.clone();
        }
        if entry.file_name.is_none() {
            entry.file_name = module.file_name.clone();
        }
        if entry.authors.is_empty() && !module.authors.is_empty() {
            entry.authors = module.authors.clone();
        }
        for src in &module.match_sources {
            push_match_source(entry, src);
        }
        for evidence in module.evidence {
            if entry.evidence.len() >= MAX_EVIDENCE_PER_SUSPECT {
                break;
            }
            if !entry.evidence.iter().any(|item| {
                item.source == evidence.source && item.line_number == evidence.line_number
            }) {
                push_match_source(entry, &match_source_for_kind(evidence.kind));
                entry.evidence.push(evidence);
            }
        }
        let strong_count = entry
            .evidence
            .iter()
            .filter(|e| e.weight >= 70 || is_strong_match_source(&match_source_for_kind(e.kind)))
            .count();
        entry.confidence = entry
            .confidence
            .saturating_add((strong_count.saturating_sub(1) as u8).min(5));
        let distinct_sources: HashSet<String> =
            entry.evidence.iter().map(|e| e.source.clone()).collect();
        if distinct_sources.len() >= 2 {
            entry.confidence = entry.confidence.saturating_add(4).min(99);
        }
    }

    let mut out = by_id
        .into_values()
        .map(accumulator_to_suspect)
        .collect::<Vec<_>>();
    out.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| b.known_in_manifest.cmp(&a.known_in_manifest))
            .then_with(|| a.name.cmp(&b.name))
    });
    assign_blame_roles(&mut out);
    out
}

/// Build plain-language remediation hints from the detected signals and the
/// suspected mods they reference. Each hint carries actionable steps and an
/// optional machine-actionable `FixAction` the launcher UI can trigger.
pub fn build_hints(signals: &[CrashSignal], suspects: &[SuspectedMod]) -> Vec<DiagnosisHint> {
    let kinds: HashSet<CrashSignalKind> = signals.iter().map(|s| s.kind.clone()).collect();
    let mut hints: Vec<DiagnosisHint> = Vec::new();

    let mut push = |h: DiagnosisHint| {
        if !hints.iter().any(|existing| existing.id == h.id) {
            hints.push(h);
        }
    };

    // Top mod suspect, if any (highest confidence, known in manifest).
    let top = suspects
        .iter()
        .find(|s| s.known_in_manifest)
        .or_else(|| suspects.first());

    if kinds.contains(&CrashSignalKind::OutOfMemory) {
        push(DiagnosisHint {
            id: "out-of-memory".into(),
            title: "Not enough memory (OutOfMemoryError)".into(),
            severity: "critical".into(),
            detail: "The JVM ran out of heap memory. This is usually caused by too many \
                mods/entities/chunks for the allocated RAM, or a memory leak in a mod."
                .into(),
            steps: vec![
                "Increase the JVM heap: set memory_mb in project settings to at least 4–6 GB for heavily modded instances.".into(),
                "Lower view-distance / simulation-distance in the world or server settings.".into(),
                "Pre-generate chunks to reduce runtime world generation load.".into(),
                "If it recurs after raising RAM, a specific mod likely leaks memory — bisect mods.".into(),
            ],
            related_mods: top.map(|s| vec![s.id.clone()]).unwrap_or_default(),
            fix: Some(FixAction {
                kind: "raiseMemory".into(),
                label: "Raise allocated memory to 6 GB".into(),
                mod_id: None,
            }),
            fixes: vec![],
        });
    }

    if kinds.contains(&CrashSignalKind::Watchdog) {
        push(DiagnosisHint {
            id: "watchdog".into(),
            title: "Server watchdog timeout".into(),
            severity: "critical".into(),
            detail: "A single server tick took too long, so the watchdog force-stopped the server. \
                Usually a slow mod, overloaded world, or insufficient CPU/RAM."
                .into(),
            steps: vec![
                "Reduce view-distance / simulation-distance and entity counts.".into(),
                "Allocate more RAM and ensure the JVM has enough CPU headroom.".into(),
                "Remove or update the mod responsible for the slow tick (check ticking-entity/block-entity errors).".into(),
            ],
            related_mods: top.map(|s| vec![s.id.clone()]).unwrap_or_default(),
            fix: None,
            fixes: vec![],
        });
    }

    if kinds.contains(&CrashSignalKind::EulaNotAccepted) {
        push(DiagnosisHint {
            id: "eula".into(),
            title: "EULA not accepted".into(),
            severity: "critical".into(),
            detail: "The server refuses to start until you accept Mojang's EULA.".into(),
            steps: vec![
                "Open eula.txt in the instance folder and set eula=true.".into(),
                "Restart the server afterwards.".into(),
            ],
            related_mods: Vec::new(),
            fix: Some(FixAction {
                kind: "acceptEula".into(),
                label: "Accept EULA (set eula.txt eula=true)".into(),
                mod_id: None,
            }),
            fixes: vec![],
        });
    }

    if kinds.contains(&CrashSignalKind::PortConflict) {
        push(DiagnosisHint {
            id: "port-conflict".into(),
            title: "Port already in use".into(),
            severity: "critical".into(),
            detail: "Another process (often a previous server instance that did not shut down) \
                is already holding the Minecraft port (usually 25565)."
                .into(),
            steps: vec![
                "Stop the other server / Java process, or restart the machine.".into(),
                "Or change server-port in server.properties to a free port (e.g. 25566).".into(),
                "Ensure server-ip is empty unless you must bind to a specific address.".into(),
            ],
            related_mods: Vec::new(),
            fix: Some(FixAction {
                kind: "changePort".into(),
                label: "Use port 25566 instead".into(),
                mod_id: None,
            }),
            fixes: vec![],
        });
    }

    if kinds.contains(&CrashSignalKind::CorruptJar) {
        push(DiagnosisHint {
            id: "corrupt-jar".into(),
            title: "Corrupted mod jar".into(),
            severity: "critical".into(),
            detail: "A mod file is corrupt (zip END header / CEN header error) — usually from an \
                interrupted download. The failing jar name is printed in the error."
                .into(),
            steps: vec![
                "Re-download the named mod jar from its source (Modrinth/CurseForge) and replace the file.".into(),
                "Delete the corrupt file and let TuffBox re-fetch it if it is a managed mod.".into(),
                "If unsure which jar, re-download the most recently added mods first.".into(),
            ],
            related_mods: top.map(|s| vec![s.id.clone()]).unwrap_or_default(),
            fix: top
                .filter(|s| s.known_in_manifest)
                .map(|s| FixAction {
                    kind: "reinstallMod".into(),
                    label: format!("Re-download {}", s.name),
                    mod_id: Some(s.id.clone()),
                }),
                fixes: vec![],
        });
    }

    if kinds.contains(&CrashSignalKind::DuplicateMod) {
        push(DiagnosisHint {
            id: "duplicate-mod".into(),
            title: "Duplicate mod detected".into(),
            severity: "critical".into(),
            detail:
                "Two copies of the same mod are present (often an old jar left after updating). \
                The loader refuses to start."
                    .into(),
            steps: vec![
                "Open the mods folder and delete the older/duplicate jar of the named mod.".into(),
                "Keep only one version of each mod.".into(),
            ],
            related_mods: top.map(|s| vec![s.id.clone()]).unwrap_or_default(),
            fix: None,
            fixes: vec![],
        });
    }

    if kinds.contains(&CrashSignalKind::JavaVersion) {
        push(DiagnosisHint {
            id: "java-version".into(),
            title: "Wrong Java version".into(),
            severity: "critical".into(),
            detail: "UnsupportedClassVersionError means the mod/loader was built for a newer Java \
                than the one running. Modern Minecraft needs Java 17 (1.18+) or Java 21 (1.20.5+ / NeoForge)."
                .into(),
            steps: vec![
                "Install the Java version required by your Minecraft version and point the project at it.".into(),
                "1.17–1.20.4 → Java 17; 1.20.5+ and recent NeoForge → Java 21.".into(),
                "Update the loader if it also requires a newer Java.".into(),
            ],
            related_mods: Vec::new(),
            fix: Some(FixAction {
                kind: "autoJava".into(),
                label: "Auto-select a compatible Java runtime".into(),
                mod_id: None,
            }),
            fixes: vec![],
        });
    }

    if kinds.contains(&CrashSignalKind::TickingEntity) {
        push(DiagnosisHint {
            id: "ticking-entity".into(),
            title: "Crash while ticking an entity/block".into(),
            severity: "high".into(),
            detail: "A specific entity or block entity threw an exception during its tick — the \
                stack trace names the exact class, which identifies the culprit mod."
                .into(),
            steps: vec![
                "Identify the entity/block from the stack trace and remove or update that mod."
                    .into(),
                "If a chunk is corrupted, restore it from a backup or delete the region file."
                    .into(),
                "As a last resort, remove the most recently added mod and retest.".into(),
            ],
            related_mods: top.map(|s| vec![s.id.clone()]).unwrap_or_default(),
            fix: top.filter(|s| s.known_in_manifest).map(|s| FixAction {
                kind: "disableMod".into(),
                label: format!("Disable {}", s.name),
                mod_id: Some(s.id.clone()),
            }),
            fixes: vec![],
        });
    }

    if kinds.contains(&CrashSignalKind::SideMismatch) {
        push(DiagnosisHint {
            id: "side-mismatch".into(),
            title: "Mod loaded on the wrong side".into(),
            severity: "high".into(),
            detail: "A mod tried to load a client-only class on a server (or vice versa). This \
                happens with client-only mods installed on a dedicated server."
                .into(),
            steps: vec![
                "Remove the client-only mod from the server's mods folder.".into(),
                "Keep server-only mods out of the client instance.".into(),
            ],
            related_mods: top.map(|s| vec![s.id.clone()]).unwrap_or_default(),
            fix: top.filter(|s| s.known_in_manifest).map(|s| FixAction {
                kind: "disableMod".into(),
                label: format!("Disable {}", s.name),
                mod_id: Some(s.id.clone()),
            }),
            fixes: vec![],
        });
    }

    if kinds.contains(&CrashSignalKind::ServerState) {
        push(DiagnosisHint {
            id: "server-state".into(),
            title: "World/session lock after a crash".into(),
            severity: "high".into(),
            detail: "The server was killed mid-run (power loss / hard crash) and left a session \
                lock or inconsistent state file. Minecraft sometimes corrupts its own JSON on sudden shutdown."
                .into(),
            steps: vec![
                "Delete session.lock in the world folder if present.".into(),
                "Restore the world from the most recent backup.".into(),
                "Make sure the previous server process is fully stopped before restarting.".into(),
            ],
            related_mods: Vec::new(),
            fix: None,
            fixes: vec![],
        });
    }

    if kinds.contains(&CrashSignalKind::MissingDependency) {
        // Prefer ids explicitly marked missing in loader text ("… (sodium), which is missing").
        // Fall back to suspects that are not already in the pack manifest.
        let mut missing_ids: Vec<String> = Vec::new();
        let push_missing = |ids: &mut Vec<String>, id: String| {
            if is_invented_vanilla_resource_mod_id(&id) {
                return;
            }
            if !ids.iter().any(|x| x == &id) {
                ids.push(id);
            }
        };
        for signal in signals
            .iter()
            .filter(|s| s.kind == CrashSignalKind::MissingDependency)
        {
            for id in extract_missing_dependency_ids(&signal.text) {
                push_missing(&mut missing_ids, id);
            }
        }
        if missing_ids.is_empty() {
            for s in suspects.iter().filter(|s| !s.known_in_manifest) {
                push_missing(&mut missing_ids, s.id.clone());
            }
        }
        if missing_ids.is_empty() {
            for s in suspects {
                push_missing(&mut missing_ids, s.id.clone());
            }
        }

        // Only invent Install buttons for real mod ids. Vanilla resource paths
        // compacted into fake slugs must not surface as missing dependencies.
        if !missing_ids.is_empty() {
            let fixes: Vec<FixAction> = missing_ids
                .iter()
                .map(|id| FixAction {
                    kind: "installDependency".into(),
                    label: format!("Install {id}"),
                    mod_id: Some(id.clone()),
                })
                .collect();

            push(DiagnosisHint {
                id: "missing-dependency".into(),
                title: if missing_ids.len() > 1 {
                    format!("{} missing mod dependencies", missing_ids.len())
                } else {
                    "Missing mod dependency".into()
                },
                severity: "high".into(),
                detail: "One or more mods require another mod that is not installed (or could not be \
                    loaded). The loader reports it as a ModResolutionException / missing dependency."
                    .into(),
                steps: vec![
                    "Install each missing dependency for the same Minecraft + loader version.".into(),
                    "If the dependency is present, it may be the wrong version — update it.".into(),
                    "For JIJ (jar-in-jar) dependencies, update the parent mod.".into(),
                ],
                related_mods: missing_ids.clone(),
                fix: fixes.first().cloned(),
                fixes,
            });
        }
    }

    if kinds.contains(&CrashSignalKind::ModVersionMismatch) {
        let names: Vec<String> = suspects.iter().map(|s| s.id.clone()).collect();
        let mut mix_fixes = Vec::new();
        for s in suspects.iter().filter(|s| s.known_in_manifest).take(3) {
            let safe = mod_category::is_safe_to_disable(mod_category::classify(&s.id, &s.name));
            let (kind, verb) = if safe {
                ("disableMod", "Disable")
            } else {
                ("updateMod", "Update")
            };
            mix_fixes.push(FixAction {
                kind: kind.into(),
                label: format!("{verb} {}", s.name),
                mod_id: Some(s.id.clone()),
            });
        }
        push(DiagnosisHint {
            id: "version-mismatch".into(),
            title: "Mod / version conflict".into(),
            severity: "high".into(),
            detail: "Two mods conflict, or a mod is the wrong version for your setup. Common with \
                mixin conflicts or libraries at incompatible versions."
                .into(),
            steps: vec![
                "Choose which side to keep — the recommended option disables the replaceable (optimization / bridge / legacy) mod and keeps your content.".into(),
                "If two mods edit the same feature, keep only one or use a compatibility patch.".into(),
                "Check the mod's issue tracker for known incompatibilities.".into(),
            ],
            related_mods: names.clone(),
            fix: mix_fixes.first().cloned(),
            fixes: mix_fixes,
        });
    }

    if kinds.contains(&CrashSignalKind::MinecraftVersionMismatch) {
        push(DiagnosisHint {
            id: "minecraft-version".into(),
            title: "Wrong Minecraft version for mod".into(),
            severity: "high".into(),
            detail: "A mod requires a different Minecraft version than the one installed.".into(),
            steps: vec![
                "Either downgrade/upgrade Minecraft to the version the mod supports, or".into(),
                "Replace the mod with a build for your current Minecraft version.".into(),
            ],
            related_mods: top.map(|s| vec![s.id.clone()]).unwrap_or_default(),
            fix: top.filter(|s| s.known_in_manifest).map(|s| FixAction {
                kind: "updateMod".into(),
                label: format!("Update {}", s.name),
                mod_id: Some(s.id.clone()),
            }),
            fixes: vec![],
        });
    }

    if kinds.contains(&CrashSignalKind::WrongLoader) {
        push(DiagnosisHint {
            id: "wrong-loader".into(),
            title: "Wrong mod loader".into(),
            severity: "high".into(),
            detail:
                "A mod is built for a different loader (e.g. Forge mod on Fabric, or vice versa)."
                    .into(),
            steps: vec![
                "Install the correct loader (Fabric/Forge/NeoForge/Quilt) for the mod.".into(),
                "Or replace the mod with a port for your current loader.".into(),
            ],
            related_mods: top.map(|s| vec![s.id.clone()]).unwrap_or_default(),
            fix: None,
            fixes: vec![],
        });
    }

    if kinds.contains(&CrashSignalKind::LoaderVersionMismatch) {
        push(DiagnosisHint {
            id: "loader-version".into(),
            title: "Wrong loader version".into(),
            severity: "high".into(),
            detail:
                "A mod requires a newer (or older) version of the mod loader than is installed."
                    .into(),
            steps: vec![
                "Update the mod loader to the version the mod requires.".into(),
                "Fabric Loader, Forge, NeoForge and Quilt each have their own version line.".into(),
            ],
            related_mods: Vec::new(),
            fix: Some(FixAction {
                kind: "updateLoader".into(),
                label: "Update mod loader".into(),
                mod_id: None,
            }),
            fixes: vec![],
        });
    }

    if kinds.contains(&CrashSignalKind::Entrypoint) {
        // Prefer the mod named in "provided by '…'" over every weak suspect.
        let mut related = Vec::new();
        for signal in signals {
            if signal.kind != CrashSignalKind::Entrypoint {
                continue;
            }
            for id in extract_quoted_mod_ids(&signal.text) {
                if !related.contains(&id) {
                    related.push(id);
                }
            }
        }
        for s in suspects.iter().filter(|s| s.known_in_manifest) {
            if !related.contains(&s.id) {
                related.push(s.id.clone());
            }
            if related.len() >= 3 {
                break;
            }
        }
        related.truncate(3);
        push(DiagnosisHint {
            id: "entrypoint".into(),
            title: "Mod entrypoint failed".into(),
            severity: "high".into(),
            detail: "A mod's initialization code threw while the game was starting. Often a \
                version mismatch or a missing dependency for that specific mod."
                .into(),
            steps: vec![
                "Update or remove the mod named in the error.".into(),
                "Check for a missing dependency the mod requires.".into(),
            ],
            related_mods: related.clone(),
            fix: related.first().and_then(|id| {
                suspects
                    .iter()
                    .find(|s| &s.id == id && s.known_in_manifest)
                    .map(|s| FixAction {
                        kind: "disableMod".into(),
                        label: format!("Disable {}", s.name),
                        mod_id: Some(s.id.clone()),
                    })
            }),
            fixes: vec![],
        });
    }

    if kinds.contains(&CrashSignalKind::Mixin) {
        let mut related = Vec::new();
        for s in suspects.iter().filter(|s| s.known_in_manifest) {
            related.push(s.id.clone());
            if related.len() >= 3 {
                break;
            }
        }
        push(DiagnosisHint {
            id: "mixin".into(),
            title: "Mixin injection failure".into(),
            severity: "high".into(),
            detail: "A mod failed to apply its mixin transformers. Usually caused by a wrong \
                Minecraft/loader version, two mods editing the same code, or a library mismatch."
                .into(),
            steps: vec![
                "Update the mod whose mixin failed (named in the error / stack trace).".into(),
                "If two mods conflict on the same class, keep only one or add a compat patch."
                    .into(),
                "Verify the mod supports your exact Minecraft + loader version.".into(),
            ],
            related_mods: related,
            fix: top.filter(|s| s.known_in_manifest).map(|s| FixAction {
                kind: "updateMod".into(),
                label: format!("Update {}", s.name),
                mod_id: Some(s.id.clone()),
            }),
            fixes: vec![],
        });
    }

    // For hints that implicate several installed mods, offer a Fix button per
    // related mod (capped). Never expand to the entire mod list.
    let known_by_id: std::collections::HashMap<&str, &SuspectedMod> = suspects
        .iter()
        .filter(|s| s.known_in_manifest)
        .map(|s| (s.id.as_str(), s))
        .collect();
    for hint in hints.iter_mut() {
        if !hint.fixes.is_empty() {
            continue;
        }
        let Some(kind) = mod_fix_kind_for_hint(&hint.id) else {
            continue;
        };
        if hint.related_mods.is_empty() {
            continue;
        }
        let targets: Vec<&SuspectedMod> = hint
            .related_mods
            .iter()
            .filter_map(|id| known_by_id.get(id.as_str()).copied())
            .take(3)
            .collect();
        if targets.len() <= 1 {
            continue;
        }
        hint.fixes = targets
            .iter()
            .map(|s| FixAction {
                kind: kind.to_string(),
                label: format!("{} {}", fix_verb(kind), s.name),
                mod_id: Some(s.id.clone()),
            })
            .collect();
    }

    hints
}

/// Maps a diagnosis-hint id to the fix action kind appropriate for a
/// per-mod button (disable / update / reinstall).
fn mod_fix_kind_for_hint(hint_id: &str) -> Option<&'static str> {
    match hint_id {
        "corrupt-jar" => Some("reinstallMod"),
        "ticking-entity" | "side-mismatch" | "entrypoint" => Some("disableMod"),
        "mixin" | "version-mismatch" | "minecraft-version" => Some("updateMod"),
        _ => None,
    }
}

/// Verb shown on the per-mod fix button label.
fn fix_verb(kind: &str) -> &'static str {
    match kind {
        "reinstallMod" => "Reinstall",
        "disableMod" => "Disable",
        "updateMod" => "Update",
        _ => "Fix",
    }
}

/// Parse conflict pairs from crash signals (any signal whose text looks like a
/// "mod A is incompatible / breaks / conflicts with mod B" loader line).
fn build_conflicts_from_signals(signals: &[CrashSignal]) -> Vec<Conflict> {
    mod_conflict::parse_conflicts_from_lines(
        &signals
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<&str>>(),
    )
}

/// Represent a ranked resolution as a selectable ChangeOption (one radio row in
/// the diagnostics "how to fix" panel).
fn ranked_fix_to_option(c: &resolve::RankedFix) -> ChangeOption {
    let base = match &c.action {
        ChangeAction::DisableMod { node_id } => format!("Disable {}", mod_node_label(node_id)),
        ChangeAction::UpdateMod { node_id, .. } => format!("Update {}", mod_node_label(node_id)),
        ChangeAction::InstallMod { project_id, .. } => format!("Install {project_id}"),
        ChangeAction::RemoveMod { node_id } => format!("Remove {}", mod_node_label(node_id)),
        ChangeAction::EditConfig { path, .. } => format!("Edit {path}"),
    };
    ChangeOption {
        label: if c.preferred {
            format!("{base} (recommended)")
        } else {
            base
        },
        keep_mod: (!c.keep_mod.is_empty()).then(|| c.keep_mod.clone()),
        reason: c.reason.clone(),
        preferred: c.preferred,
        actions: vec![c.action.clone()],
    }
}

fn build_conflict_summary(conflicts: &[Conflict], option_count: usize) -> String {
    let pairs: Vec<String> = conflicts
        .iter()
        .map(|c| format!("{} ↔ {}", c.a, c.b))
        .collect();
    format!(
        "Mod conflict detected: {}. {option_count} resolution choice(s) are offered — pick a side below. The recommended default disables the safest (most replaceable) side and keeps your content; it is reversible (jar → .disabled).",
        pairs.join(", ")
    )
}

fn mod_node_label(node_id: &NodeId) -> String {
    node_id
        .0
        .strip_prefix("mod:")
        .unwrap_or(&node_id.0)
        .to_string()
}

pub fn create_crash_fix_plan(
    graph: &DependencyGraph,
    diagnostics: &[Diagnostic],
    suspected_mods: &[SuspectedMod],
    signals: &[CrashSignal],
) -> ChangePlan {
    // Category-aware conflict resolution. Parse conflict pairs from the log,
    // rank *both* sides by replaceability, and offer every side as a selectable
    // option with a category-aware preferred default. Always reversible
    // (.disabled), never deletes content or libraries.
    let conflicts = build_conflicts_from_signals(signals);
    if !conflicts.is_empty() {
        let ctx = ResolveCtx {
            graph,
            manifest: None,
        };
        let ranked = resolve::ranked_candidates(&conflicts, suspected_mods, &ctx);
        if !ranked.is_empty() {
            let preferred: Vec<&resolve::RankedFix> =
                ranked.iter().filter(|c| c.preferred).collect();
            let chosen: Vec<&resolve::RankedFix> = if preferred.is_empty() {
                vec![&ranked[0]]
            } else {
                preferred
            };
            let actions: Vec<ChangeAction> = chosen.iter().map(|c| c.action.clone()).collect();
            let options: Vec<ChangeOption> = ranked.iter().map(ranked_fix_to_option).collect();
            return ChangePlan {
                summary: build_conflict_summary(&conflicts, options.len()),
                risk: ChangeRisk::Medium,
                actions,
                requires_snapshot: true,
                options,
            };
        }
    }

    if let Some(top) = suspected_mods.first() {
        let node_id = NodeId::module(&top.id);
        let mut actions = Vec::new();
        if top.known_in_manifest && graph.has_node(&node_id) {
            actions.push(ChangeAction::DisableMod {
                node_id: node_id.clone(),
            });
            actions.push(ChangeAction::UpdateMod {
                node_id: node_id.clone(),
                target_version: LATEST_COMPATIBLE_VERSION.to_string(),
            });
        }
        return ChangePlan {
            summary: if top.known_in_manifest {
                format!(
                    "Create a safety snapshot, then disable suspected mod {} (jar → .disabled) and rerun. If needed, update it to the latest compatible build afterward.",
                    top.name
                )
            } else {
                format!(
                    "Inspect inferred crash source `{}`. It is not mapped to a manifest mod yet, so verify local jars and latest.log before applying changes.",
                    top.name
                )
            },
            risk: ChangeRisk::Medium,
            actions,
            requires_snapshot: true,
            options: Vec::new(),
        };
    }

    if signals
        .iter()
        .any(|signal| signal.kind == CrashSignalKind::OpenGl)
    {
        return ChangePlan {
            summary: "Graphics / resource-pack shader failure detected (OpenGL errors, missing `minecraft:core/rendertype_*` shaders, or resource packs stripped on load). Disable resource packs and shader packs first, update GPU drivers, then test without render mods such as Sodium/Iris/Oculus/Voxy one group at a time. Do not install invented mods named after vanilla resource paths.".to_string(),
            risk: ChangeRisk::Medium,
            actions: Vec::new(),
            requires_snapshot: true,
        options: Vec::new(),
        };
    }

    if signals
        .iter()
        .any(|signal| signal.kind == CrashSignalKind::Performance)
    {
        return ChangePlan {
            summary: "Performance stall detected (`Can't keep up`). Reduce render/simulation load, lower view distance, review worldgen/entity-heavy mods and rerun the Test profile.".to_string(),
            risk: ChangeRisk::Low,
            actions: Vec::new(),
            requires_snapshot: false,
        options: Vec::new(),
        };
    }

    if let Some(plan) = Resolver::create_fix_plan(graph, diagnostics) {
        return plan;
    }

    let has_errors = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error);
    ChangePlan {
        summary: if has_errors {
            "Review graph diagnostics first, then rerun Test to produce a fresh latest.log."
                .to_string()
        } else {
            "No deterministic crash source found yet. Reproduce the crash, then analyze crash-reports/latest and logs/latest.log.".to_string()
        },
        risk: ChangeRisk::Low,
        actions: Vec::new(),
        requires_snapshot: false,
        options: Vec::new(),
    }
}

pub fn resolve_update_target_version(target_version: &str) -> Option<&str> {
    let target_version = target_version.trim();
    if target_version.is_empty() || target_version.eq_ignore_ascii_case(LATEST_COMPATIBLE_VERSION) {
        None
    } else {
        Some(target_version)
    }
}

fn build_candidates(manifest: &ProjectManifest) -> Vec<ModCandidate<'_>> {
    manifest
        .mods
        .iter()
        .map(|module| {
            let mut tokens = HashSet::new();
            insert_token_variants(&mut tokens, &module.id);
            insert_token_variants(&mut tokens, &module.name);
            if let Some(file_name) = &module.file_name {
                insert_token_variants(&mut tokens, file_name);
            }
            if let Some(project_id) = &module.source.project_id {
                insert_token_variants(&mut tokens, project_id);
            }
            let file_stem = module
                .file_name
                .as_deref()
                .map(|file| normalize_token(file.trim_end_matches(".jar")));
            ModCandidate {
                module,
                tokens: tokens
                    .into_iter()
                    .filter(|token| !is_noise_token(token))
                    .collect(),
                file_stem,
            }
        })
        .collect()
}

fn insert_token_variants(tokens: &mut HashSet<String>, value: &str) {
    let normalized = normalize_token(value.trim_end_matches(".jar"));
    if !normalized.is_empty() {
        tokens.insert(normalized.clone());
        let compact = compact_token(&normalized);
        if compact.len() >= 4 {
            tokens.insert(compact);
        }
    }
    for token in tokenize(value) {
        if token.len() >= 3 {
            let compact = compact_token(&token);
            tokens.insert(token);
            if compact.len() >= 4 {
                tokens.insert(compact);
            }
        }
    }
}

fn classify_signal_line(line: &str) -> Option<CrashSignalKind> {
    let lower = line.to_lowercase();

    // ---- System / environment crashes (highest priority) ----
    if lower.contains("java.lang.outofmemoryerror")
        || lower.contains("out of memory")
        || lower.contains("gc overhead limit exceeded")
    {
        return Some(CrashSignalKind::OutOfMemory);
    }
    if lower.contains("watchdog")
        || lower.contains("server watchdog")
        || lower.contains("the server has stopped responding")
        || lower.contains("a single server tick took")
    {
        return Some(CrashSignalKind::Watchdog);
    }
    if lower.contains("you need to agree to the eula")
        || lower.contains("eula.txt")
        || (lower.contains("eula") && lower.contains("not accepted"))
    {
        return Some(CrashSignalKind::EulaNotAccepted);
    }
    if lower.contains("failed to bind to port")
        || lower.contains("address already in use")
        || lower.contains("bind(..) failed")
    {
        return Some(CrashSignalKind::PortConflict);
    }
    if lower.contains("zip end header not found")
        || lower.contains("invalid cen header")
        || lower.contains("zipexception")
        || lower.contains("corrupt jar")
        || lower.contains("invalid or corrupt jarfile")
        || lower.contains("error analyzing")
    {
        return Some(CrashSignalKind::CorruptJar);
    }
    if lower.contains("duplicate mod")
        || lower.contains("duplicate mods")
        || (lower.contains("found duplicate") && lower.contains("mod"))
        || lower.contains("mod already loaded")
    {
        return Some(CrashSignalKind::DuplicateMod);
    }
    if lower.contains("unsupportedclassversionerror")
        || (lower.contains("unsupported") && lower.contains("class version"))
        || lower.contains("has been compiled by a more recent version of the java runtime")
    {
        return Some(CrashSignalKind::JavaVersion);
    }
    if lower.contains("ticking entity")
        || lower.contains("ticking block entity")
        || lower.contains("exception in server tick loop")
        || lower.contains("exception ticking")
    {
        return Some(CrashSignalKind::TickingEntity);
    }
    if lower.contains("attempted to load class") && lower.contains("invalid side")
        || lower.contains("for invalid side")
        || (lower.contains("client class") && lower.contains("server"))
    {
        return Some(CrashSignalKind::SideMismatch);
    }
    if lower.contains("state engine was in the incorrect state")
        || lower.contains("forced into state server_stopped")
        || lower.contains("failed to check session lock")
    {
        return Some(CrashSignalKind::ServerState);
    }

    // ---- Dependency / version / loader resolution errors ----
    // These are the most actionable crash causes, so they take priority.
    // Avoid bare `contains("mod")` / `contains("missing mod")` — the latter
    // matches "missing model" and invents Install minecraftbuiltinentity.
    let mentions_missing_mod = contains_missing_mod_phrase(&lower)
        || lower.contains("mod is missing")
        || lower.contains("mods are missing")
        || lower.contains("which is missing")
        || lower.contains("is missing!");
    let is_resolution_error = mentions_missing_mod
        || lower.contains("missing dependency")
        || lower.contains("could not be loaded")
        || lower.contains("dependency")
        || lower.contains("requires ")
        || lower.contains("incompatible mod set")
        || lower.contains("conflict")
        || lower.contains("incompatible")
        || lower.contains("modresolutionexception");
    if is_resolution_error {
        // Narrow down to a more specific kind when the text is explicit.
        let mentions_minecraft = lower.contains("minecraft");
        // A loader *name* (fabric/forge/...) in the text only counts when it is
        // clearly the subject, not e.g. the `net.fabricmc.loader` package in a
        // ModResolutionException stack line.
        let mentions_loader_kind = lower.contains("wrong loader")
            || lower.contains("not a fabric mod")
            || lower.contains("not a forge mod")
            || lower.contains("not a neoforge mod")
            || lower.contains("mod loader")
            || lower.contains("is in use")
            || lower.contains("requires the fabric")
            || lower.contains("requires the forge")
            || lower.contains("requires the neoforge")
            || lower.contains("requires the quilt");
        let mentions_loader_version = lower.contains("fabricloader")
            || lower.contains("fabric loader")
            || lower.contains("loader version")
            || lower.contains("loader 0.")
            || (lower.contains("loader") && (lower.contains("below") || lower.contains("above")));
        let mentions_version_mismatch = lower.contains("non-matching version")
            || lower.contains("wrong version")
            || lower.contains("but a ")
            || lower.contains("which is present")
            || lower.contains("incompatible")
            || lower.contains("conflict");

        // Explicit missing-dependency markers win over generic loader checks.
        if mentions_missing_mod
            || lower.contains("modresolutionexception")
            || lower.contains("missing dependency")
            || lower.contains("could not be loaded")
        {
            return Some(CrashSignalKind::MissingDependency);
        }
        if mentions_minecraft
            && (lower.contains("requires") || lower.contains("needs") || mentions_version_mismatch)
        {
            return Some(CrashSignalKind::MinecraftVersionMismatch);
        }
        // Wrong-loader (Forge mod on Fabric, etc.) takes priority over a plain
        // loader-version requirement.
        if mentions_loader_kind {
            return Some(CrashSignalKind::WrongLoader);
        }
        if mentions_loader_version {
            return Some(CrashSignalKind::LoaderVersionMismatch);
        }
        if mentions_version_mismatch {
            return Some(CrashSignalKind::ModVersionMismatch);
        }
        return Some(CrashSignalKind::MissingDependency);
    }
    if lower.contains("could not execute entrypoint")
        || (lower.contains("provided by '")
            && (lower.contains("due to errors")
                || lower.contains("could not execute")
                || lower.contains("exception")
                || lower.contains("failed to")
                || lower.contains("fatal")))
    {
        return Some(CrashSignalKind::Entrypoint);
    }
    if lower.contains("nosuchmethoderror")
        || lower.contains("nosuchfielderror")
        || lower.contains("net.neoforged.fml") && lower.contains("fabric")
    {
        return Some(CrashSignalKind::LoaderMismatch);
    }
    if lower.contains("opengl debug message")
        || lower.contains("gl_invalid_operation")
        || lower.contains("gl_invalid_")
        || lower.contains("blaze3d.opengl.gldebug")
        || lower.contains("failed to load required shader programs")
        || lower.contains("could not find shader:")
        || lower.contains("caught error loading resourcepacks")
        || (lower.contains("removing all selected resourcepacks")
            && (lower.contains("error") || lower.contains("failed")))
    {
        return Some(CrashSignalKind::OpenGl);
    }
    if lower.contains("can't keep up!") || lower.contains("is the server overloaded?") {
        return Some(CrashSignalKind::Performance);
    }
    if lower.contains("invalid mod icon")
        || lower.contains("broken icon")
        || lower.contains("lingering jcef helper")
    {
        return Some(CrashSignalKind::ResourceWarning);
    }
    if lower.contains("suspected mod") || lower.contains("suspected mods") {
        return Some(CrashSignalKind::SuspectedMods);
    }
    if lower.contains("mod file")
        || lower.contains("modfile")
        || lower.ends_with(".jar") && lower.contains("/mods/")
    {
        return Some(CrashSignalKind::ModFile);
    }
    if lower.contains("caused by:") {
        return Some(CrashSignalKind::CausedBy);
    }
    // Only treat as a mixin *failure* when the line clearly reports a broken
    // transformer — not every benign "mixin" mention in the loader startup
    // log (reference-map warnings, "Force-disabling mixin", the MIXIN
    // Subsystem banner). Those are normal and must not raise a false hint.
    if lower.contains("mixin")
        && (lower.contains("fail")
            || lower.contains("error")
            || lower.contains("exception")
            || lower.contains("could not")
            || lower.contains("couldn't")
            || lower.contains("conflict")
            || lower.contains("crash")
            || lower.contains("transform")
            || lower.contains("invalid"))
    {
        return Some(CrashSignalKind::Mixin);
    }
    if lower.contains("exception")
        || lower.contains("error:")
        || lower.contains("error ")
        || lower.contains("knot//")
        || lower.contains("net.fabricmc.loader")
        || lower.contains("java.base/")
        || (lower.starts_with("at ") && lower.contains(".java:"))
    {
        return Some(CrashSignalKind::Exception);
    }
    None
}

/// Match a Java package/FQN string (e.g. `net.earthcomputer.clientcommands`)
/// against a mod candidate. A candidate matches when its token equals the
/// package or is the final component of the package (`...clientcommands`).
fn candidate_matches_java_package(candidate: &ModCandidate<'_>, pkg: &str) -> bool {
    // Keep the package dots intact for suffix matching.
    let pkg = pkg.replace('\\', "/");
    candidate.tokens.iter().any(|token| {
        if token.len() < 4 || is_noise_token(token) {
            return false;
        }
        let compact = compact_token(token);
        compact == compact_token(&pkg)
            || pkg.ends_with(&format!("-{compact}"))
            || pkg.ends_with(&format!(".{compact}"))
    })
}

fn candidate_matches_line(candidate: &ModCandidate<'_>, line: &str) -> bool {
    let normalized_line = normalize_token(line);
    let compact_line = compact_token(&normalized_line);
    // Segment match avoids short name tokens like "critters" falsely hitting
    // another mod id such as "crittersandcompanions" via substring contains().
    let line_parts: HashSet<&str> = normalized_line
        .split(['-', '.'])
        .filter(|part| part.len() >= 3)
        .collect();

    candidate.tokens.iter().any(|token| {
        if token.len() < 4 || is_noise_token(token) {
            return false;
        }
        let compact = compact_token(token);
        if normalized_line == *token || compact_line == compact || compact_line == *token {
            return true;
        }
        if line_parts.contains(token.as_str()) || line_parts.contains(compact.as_str()) {
            return true;
        }
        // Long stems / modids (file names, mixin packages) may appear mid-line.
        compact.len() >= 10 && compact_line.contains(&compact)
    })
}

/// Extract Java fully-qualified names (and their package prefixes) from a
/// stack-trace line such as `at knot//net.earthcomputer.clientcommands...
/// .PlayerRandCracker.throwItem(PlayerRandCracker.java:412)`. Each dotted
/// segment becomes a candidate token so a mod whose id/name matches a package
/// component (e.g. `clientcommands`) is correctly attributed.
fn extract_java_packages(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in line.split(|c: char| {
        c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ':' | ';')
    }) {
        // Keep dots/packages intact; only switch Fabric's `knot//` separator to
        // a dot so `net/earthcomputer` style and `knot//net.x` both normalize.
        let unified = raw.replace("knot//", "").replace(['/', '\\'], ".");
        if unified.contains('.') && unified.chars().any(|c| c.is_ascii_alphabetic()) {
            let trimmed = unified.trim_end_matches(".class");
            // Emit every dotted prefix (full FQN down to the top-level package)
            // so `net.earthcomputer.clientcommands` also yields `clientcommands`.
            let mut acc = String::new();
            for seg in trimmed.split('.') {
                if seg.is_empty() {
                    continue;
                }
                acc = if acc.is_empty() {
                    seg.to_string()
                } else {
                    format!("{acc}.{seg}")
                };
                if acc.len() >= 3 {
                    out.push(acc.clone());
                }
            }
        }
    }
    out
}

fn jar_matches_candidate(jar_name: &str, candidate: &ModCandidate<'_>) -> bool {
    let normalized = normalize_token(jar_name.trim_end_matches(".jar"));
    let compact = compact_token(&normalized);
    if candidate.file_stem.as_deref() == Some(normalized.as_str()) {
        return true;
    }
    candidate.tokens.iter().any(|token| {
        if token.len() < 4 || is_noise_token(token) {
            return false;
        }
        let token_compact = compact_token(token);
        normalized == *token
            || compact == token_compact
            || (token_compact.len() >= 10
                && (compact.contains(&token_compact) || token_compact.contains(&compact)))
    })
}

fn add_manifest_suspect(
    suspects: &mut BTreeMap<String, SuspectAccumulator>,
    module: &ModSpec,
    evidence: SuspectEvidence,
    confidence: u8,
) {
    let key = normalize_token(&module.id);
    let src = match_source_for_kind(evidence.kind);
    let entry = suspects.entry(key).or_insert_with(|| SuspectAccumulator {
        id: module.id.clone(),
        name: module.name.clone(),
        version: Some(module.version.clone()),
        file_name: module.file_name.clone(),
        known_in_manifest: true,
        confidence: 0,
        evidence: Vec::new(),
        authors: module.authors.clone(),
        match_sources: Vec::new(),
    });
    if entry.authors.is_empty() && !module.authors.is_empty() {
        entry.authors = module.authors.clone();
    }
    entry.confidence = entry.confidence.max(confidence);
    push_match_source(entry, &src);
    push_evidence(entry, evidence);
}

fn add_inferred_suspect(
    suspects: &mut BTreeMap<String, SuspectAccumulator>,
    id: &str,
    file_name: Option<String>,
    evidence: SuspectEvidence,
    confidence: u8,
) {
    let key = normalize_token(id);
    let src = match_source_for_kind(evidence.kind);
    let entry = suspects.entry(key).or_insert_with(|| SuspectAccumulator {
        id: id.to_string(),
        name: id.to_string(),
        version: None,
        file_name,
        known_in_manifest: false,
        confidence: 0,
        evidence: Vec::new(),
        authors: Vec::new(),
        match_sources: Vec::new(),
    });
    entry.confidence = entry.confidence.max(confidence);
    push_match_source(entry, &src);
    push_evidence(entry, evidence);
}

fn accumulator_to_suspect(acc: SuspectAccumulator) -> SuspectedMod {
    SuspectedMod {
        id: acc.id,
        name: acc.name,
        version: acc.version,
        file_name: acc.file_name,
        known_in_manifest: acc.known_in_manifest,
        confidence: acc.confidence.min(99),
        evidence: acc.evidence,
        authors: acc.authors,
        blame_role: BlameRole::Related,
        match_sources: acc.match_sources,
    }
}

fn match_source_for_kind(kind: CrashSignalKind) -> String {
    match kind {
        CrashSignalKind::SuspectedMods => "suspected_mods_line".into(),
        CrashSignalKind::Entrypoint => "entrypoint".into(),
        CrashSignalKind::ModFile => "mod_file".into(),
        CrashSignalKind::Mixin => "mixin".into(),
        CrashSignalKind::Exception | CrashSignalKind::CausedBy => "exception".into(),
        CrashSignalKind::MissingDependency => "missing_dependency".into(),
        CrashSignalKind::ModVersionMismatch => "version_mismatch".into(),
        CrashSignalKind::LoaderMismatch
        | CrashSignalKind::WrongLoader
        | CrashSignalKind::LoaderVersionMismatch => "loader".into(),
        _ => "signal".into(),
    }
}

fn push_match_source(entry: &mut SuspectAccumulator, source: &str) {
    if source.is_empty() {
        return;
    }
    if !entry.match_sources.iter().any(|s| s == source) {
        entry.match_sources.push(source.to_string());
    }
}

/// Independent high-value channels used to promote primary blame.
fn is_strong_match_source(source: &str) -> bool {
    matches!(
        source,
        "suspected_mods_line"
            | "entrypoint"
            | "mod_file"
            | "class_in_jar"
            | "mixin"
            | "package"
            | "missing_dependency"
            | "caused_by"
    )
}

fn assign_blame_roles(suspects: &mut [SuspectedMod]) {
    for s in suspects.iter_mut() {
        let strong = s
            .match_sources
            .iter()
            .filter(|src| is_strong_match_source(src))
            .count();
        // Multi-signal agreement → primary; single strong → secondary; else related.
        let has_caused = s
            .evidence
            .iter()
            .any(|e| e.kind == CrashSignalKind::CausedBy);
        let has_entry = s.match_sources.iter().any(|src| src == "entrypoint");
        if has_caused || has_entry || (s.confidence >= 92 && strong >= 1) {
            s.blame_role = BlameRole::Primary;
            s.confidence = s.confidence.saturating_add(4).min(99);
        } else if strong == 1 || s.confidence >= 75 {
            s.blame_role = BlameRole::Secondary;
        } else {
            s.blame_role = BlameRole::Related;
        }
    }
    // Keep ranking: primary first, then confidence.
    suspects.sort_by(|a, b| {
        blame_rank(b.blame_role)
            .cmp(&blame_rank(a.blame_role))
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| a.name.cmp(&b.name))
    });
}

fn blame_rank(role: BlameRole) -> u8 {
    match role {
        BlameRole::Primary => 3,
        BlameRole::Secondary => 2,
        BlameRole::Related => 1,
    }
}

/// Post-merge enrichment: crash-report mod list, authors from jars, class→jar blame.
fn enrich_diagnosis_suspects(
    project_dir: &Path,
    manifest: &ProjectManifest,
    selected_report: &Option<CrashReportAnalysis>,
    latest_log: &LatestLogAnalysis,
    mut suspects: Vec<SuspectedMod>,
) -> Vec<SuspectedMod> {
    // 1) Force high confidence for Fabric "Suspected mods" / report mod entries.
    if let Some(report) = selected_report {
        for signal in report
            .signals
            .iter()
            .filter(|s| s.kind == CrashSignalKind::SuspectedMods)
        {
            for token in tokenize(&signal.text) {
                if token.len() < 2 || is_noise_token(&token) {
                    continue;
                }
                if let Some(module) = manifest.mods.iter().find(|m| {
                    normalize_token(&m.id) == token
                        || compact_token(&normalize_token(&m.id)) == compact_token(&token)
                        || normalize_token(&m.name) == token
                }) {
                    boost_or_insert_suspect(
                        &mut suspects,
                        module,
                        SuspectEvidence {
                            source: signal.source.clone(),
                            line_number: signal.line_number,
                            kind: CrashSignalKind::SuspectedMods,
                            text: signal.text.clone(),
                            weight: 97,
                        },
                        97,
                        "suspected_mods_line",
                    );
                } else if let Some(entry) = report.mod_entries.iter().find(|e| {
                    normalize_token(&e.id) == token
                        || e.name
                            .as_ref()
                            .map(|n| normalize_token(n) == token)
                            .unwrap_or(false)
                }) {
                    let mut inferred = SuspectedMod {
                        id: entry.id.clone(),
                        name: entry.name.clone().unwrap_or_else(|| entry.id.clone()),
                        version: entry.version.clone(),
                        file_name: None,
                        known_in_manifest: false,
                        confidence: 90,
                        evidence: vec![SuspectEvidence {
                            source: signal.source.clone(),
                            line_number: signal.line_number,
                            kind: CrashSignalKind::SuspectedMods,
                            text: signal.text.clone(),
                            weight: 90,
                        }],
                        authors: Vec::new(),
                        blame_role: BlameRole::Related,
                        match_sources: vec!["suspected_mods_line".into()],
                    };
                    if let Some(module) = manifest
                        .mods
                        .iter()
                        .find(|m| normalize_token(&m.id) == normalize_token(&entry.id))
                    {
                        inferred.id = module.id.clone();
                        inferred.name = module.name.clone();
                        inferred.version = Some(module.version.clone());
                        inferred.file_name = module.file_name.clone();
                        inferred.known_in_manifest = true;
                        inferred.authors = module.authors.clone();
                        inferred.confidence = 97;
                    }
                    suspects =
                        merge_suspected_mods(suspects.into_iter().chain(std::iter::once(inferred)));
                }
            }
        }
    }

    // 2) Fill authors from manifest / jar metadata.
    let mods_dir = project_dir.join("mods");
    for s in &mut suspects {
        if !s.authors.is_empty() {
            continue;
        }
        if let Some(module) = manifest.mods.iter().find(|m| {
            normalize_token(&m.id) == normalize_token(&s.id)
                || m.file_name
                    .as_ref()
                    .zip(s.file_name.as_ref())
                    .map(|(a, b)| a.eq_ignore_ascii_case(b))
                    .unwrap_or(false)
        }) {
            if !module.authors.is_empty() {
                s.authors = module.authors.clone();
            } else if let Some(file) = module.file_name.as_ref() {
                let jar = mods_dir.join(file);
                if let Ok(meta) = crate::mod_scan::scan_mod_jar(&jar) {
                    s.authors = meta.authors;
                    if s.file_name.is_none() {
                        s.file_name = Some(file.clone());
                    }
                }
            }
        } else if let Some(file) = s.file_name.as_ref() {
            let jar = mods_dir.join(file);
            if let Ok(meta) = crate::mod_scan::scan_mod_jar(&jar) {
                s.authors = meta.authors;
            }
        }
    }

    // 3) Class → jar → modid attribution.
    let mut haystack = String::new();
    if let Some(report) = selected_report {
        haystack.push_str(&report.content);
        haystack.push('\n');
    }
    haystack.push_str(&latest_log.tail);
    let class_names = crate::crash_assistant::extract_blame_class_names(&haystack, 8);
    if mods_dir.is_dir() && !class_names.is_empty() {
        for class_name in class_names {
            let matches = crate::crash_assistant::find_class_in_mods(&class_name, &mods_dir);
            for hit in matches {
                if hit.mod_id == "?" {
                    continue;
                }
                let evidence = SuspectEvidence {
                    source: "class-finder".into(),
                    line_number: 0,
                    kind: CrashSignalKind::Exception,
                    text: format!(
                        "{} provided by {}",
                        hit.class_name,
                        hit.file_name.as_deref().unwrap_or(&hit.mod_id)
                    ),
                    weight: 93,
                };
                if let Some(module) = manifest.mods.iter().find(|m| {
                    normalize_token(&m.id) == normalize_token(&hit.mod_id)
                        || m.file_name
                            .as_ref()
                            .zip(hit.file_name.as_ref())
                            .map(|(a, b)| a.eq_ignore_ascii_case(b))
                            .unwrap_or(false)
                }) {
                    boost_or_insert_suspect(&mut suspects, module, evidence, 93, "class_in_jar");
                } else {
                    let inferred = SuspectedMod {
                        id: hit.mod_id.clone(),
                        name: hit.mod_name.clone(),
                        version: None,
                        file_name: hit.file_name.clone(),
                        known_in_manifest: false,
                        confidence: 88,
                        evidence: vec![evidence],
                        authors: hit
                            .file_name
                            .as_ref()
                            .and_then(|f| crate::mod_scan::scan_mod_jar(&mods_dir.join(f)).ok())
                            .map(|m| m.authors)
                            .unwrap_or_default(),
                        blame_role: BlameRole::Related,
                        match_sources: vec!["class_in_jar".into()],
                    };
                    suspects =
                        merge_suspected_mods(suspects.into_iter().chain(std::iter::once(inferred)));
                }
            }
        }
    }

    assign_blame_roles(&mut suspects);
    suspects
}

fn boost_or_insert_suspect(
    suspects: &mut Vec<SuspectedMod>,
    module: &ModSpec,
    evidence: SuspectEvidence,
    confidence: u8,
    match_source: &str,
) {
    let key = compact_token(&normalize_token(&module.id));
    if let Some(existing) = suspects
        .iter_mut()
        .find(|s| compact_token(&normalize_token(&s.id)) == key)
    {
        existing.confidence = existing.confidence.max(confidence);
        existing.known_in_manifest = true;
        existing.id = module.id.clone();
        existing.name = module.name.clone();
        existing.version = Some(module.version.clone());
        if existing.file_name.is_none() {
            existing.file_name = module.file_name.clone();
        }
        if existing.authors.is_empty() {
            existing.authors = module.authors.clone();
        }
        if !existing.match_sources.iter().any(|s| s == match_source) {
            existing.match_sources.push(match_source.to_string());
        }
        if existing.evidence.len() < MAX_EVIDENCE_PER_SUSPECT
            && !existing
                .evidence
                .iter()
                .any(|e| e.source == evidence.source && e.line_number == evidence.line_number)
        {
            existing.evidence.push(evidence);
        }
    } else {
        suspects.push(SuspectedMod {
            id: module.id.clone(),
            name: module.name.clone(),
            version: Some(module.version.clone()),
            file_name: module.file_name.clone(),
            known_in_manifest: true,
            confidence,
            evidence: vec![evidence],
            authors: module.authors.clone(),
            blame_role: BlameRole::Related,
            match_sources: vec![match_source.to_string()],
        });
    }
}

fn push_evidence(entry: &mut SuspectAccumulator, evidence: SuspectEvidence) {
    if entry.evidence.len() >= MAX_EVIDENCE_PER_SUSPECT {
        return;
    }
    if !entry
        .evidence
        .iter()
        .any(|item| item.source == evidence.source && item.line_number == evidence.line_number)
    {
        entry.evidence.push(evidence);
    }
}

fn evidence_weighted(
    source: &str,
    line_number: usize,
    kind: CrashSignalKind,
    line: &str,
    weight: u8,
) -> SuspectEvidence {
    SuspectEvidence {
        source: source.to_string(),
        line_number,
        kind,
        text: line.trim().to_string(),
        weight,
    }
}

fn confidence_for_kind(kind: CrashSignalKind) -> u8 {
    match kind {
        CrashSignalKind::SuspectedMods => 95,
        CrashSignalKind::ModFile => 88,
        CrashSignalKind::Entrypoint => 96,
        CrashSignalKind::MissingDependency => 92,
        CrashSignalKind::LoaderMismatch => 86,
        CrashSignalKind::ModVersionMismatch => 90,
        CrashSignalKind::MinecraftVersionMismatch => 90,
        CrashSignalKind::WrongLoader => 90,
        CrashSignalKind::LoaderVersionMismatch => 88,
        CrashSignalKind::OutOfMemory => 92,
        CrashSignalKind::Watchdog => 90,
        CrashSignalKind::PortConflict => 90,
        CrashSignalKind::EulaNotAccepted => 96,
        CrashSignalKind::CorruptJar => 94,
        CrashSignalKind::DuplicateMod => 92,
        CrashSignalKind::JavaVersion => 94,
        CrashSignalKind::TickingEntity => 84,
        CrashSignalKind::SideMismatch => 92,
        CrashSignalKind::ServerState => 80,
        CrashSignalKind::Mixin => 78,
        CrashSignalKind::CausedBy => 66,
        CrashSignalKind::OpenGl => 58,
        CrashSignalKind::Exception => 48,
        CrashSignalKind::ResourceWarning => 35,
        CrashSignalKind::Performance => 25,
    }
}

/// Extract mod ids that the loader says are missing, e.g.
/// `Mod 'Lithium' (lithium) requires ... of mod 'Sodium' (sodium), which is missing!`
fn extract_missing_dependency_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for line in text.lines() {
        let lower = line.to_lowercase();
        if let Some(idx) = lower.find("which is missing") {
            let before = &line[..idx];
            if let Some(open) = before.rfind('(') {
                if let Some(close) = before[open..].find(')') {
                    let id = normalize_token(&before[open + 1..open + close]);
                    if !id.is_empty()
                        && !is_noise_token(&id)
                        && !is_invented_vanilla_resource_mod_id(&id)
                        && id.len() >= 2
                    {
                        ids.push(id);
                    }
                }
            }
        }
        // `requires missing dependency mod:foo` / `missing dependency mod:bar`
        for marker in ["missing dependency mod:", "missing dependency mod "] {
            let mut search = lower.as_str();
            while let Some(pos) = search.find(marker) {
                let after = &search[pos + marker.len()..];
                let raw = after
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
                    .next()
                    .unwrap_or("");
                let id = normalize_token(raw);
                if !id.is_empty()
                    && !is_noise_token(&id)
                    && !is_invented_vanilla_resource_mod_id(&id)
                {
                    ids.push(id);
                }
                search = &after[raw.len().min(after.len())..];
            }
        }
    }
    ids.retain(|id| !is_noise_token(id) && !is_invented_vanilla_resource_mod_id(id));
    let mut uniq = Vec::new();
    for id in ids {
        if !uniq.iter().any(|x| x == &id) {
            uniq.push(id);
        }
    }
    uniq
}

fn extract_quoted_mod_ids(line: &str) -> Vec<String> {
    let lower = line.to_lowercase();
    let mut ids = Vec::new();
    if let Some(pos) = lower.find("provided by '") {
        let start = pos + "provided by '".len();
        if let Some(end) = line[start..].find('\'') {
            let token = normalize_token(&line[start..start + end]);
            if !token.is_empty() {
                ids.push(token);
            }
        }
    }
    ids
}

/// Extract mod identifiers named explicitly inside loader resolution errors,
/// e.g. `Mod 'Client Commands' (clientcommands) requires ...` or
/// `'fabricloader' (fabricloader) 0.x` or `mod fabric-api (fabric-api)`.
fn extract_named_mods(line: &str) -> Vec<String> {
    let mut ids = Vec::new();
    // Pattern: Mod 'Display Name' (modid)  /  'modid' (modid)
    let mut rest = line;
    while let Some(pos) = rest.find('\'') {
        let after_open = &rest[pos + 1..];
        if let Some(end) = after_open.find('\'') {
            let inner = &after_open[..end];
            // Look ahead for `(modid)` immediately after the closing quote.
            let tail = &after_open[end + 1..];
            if let Some(open) = tail.find('(') {
                if let Some(close) = tail[open..].find(')') {
                    let id = normalize_token(&tail[open + 1..open + close]);
                    if !id.is_empty() {
                        ids.push(id);
                    }
                }
            }
            ids.push(normalize_token(inner));
            rest = &after_open[end + 1..];
        } else {
            break;
        }
    }
    // Pattern: `mod <id> (` without quotes.
    for cap in line.match_indices("mod ") {
        let tail = &line[cap.0 + 4..];
        if let Some(open) = tail.find('(') {
            let id = normalize_token(&tail[..open]);
            if !id.is_empty() && id.len() >= 3 {
                ids.push(id);
            }
        }
    }
    // Pattern: `modid (incompatible)` / `modid (disabled)` in resource-pack or
    // mod lists (no quotes, no `mod ` prefix).
    for cap in line.match_indices('(') {
        let before = &line[..cap.0];
        let after = &line[cap.0 + 1..];
        let close = match after.find(')') {
            Some(c) => c,
            None => continue,
        };
        let reason = &after[..close];
        if reason == "incompatible" || reason == "disabled" || reason.contains("incompatible") {
            let id = normalize_token(before.trim_end().split(',').last().unwrap_or(before).trim());
            if !id.is_empty() && id.len() >= 3 {
                ids.push(id);
            }
        }
    }
    ids.retain(|id| {
        !is_noise_token(id) && id.len() >= 3 && !is_invented_vanilla_resource_mod_id(id)
    });
    ids
}

fn extract_jar_names(line: &str) -> Vec<String> {
    let mut jars = Vec::new();
    for raw in line.split(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';')
    }) {
        let trimmed = raw.trim_matches(|c: char| matches!(c, ':' | ',' | ';'));
        let lower = trimmed.to_lowercase();
        if let Some(idx) = lower.find(".jar") {
            let before = &trimmed[..idx + 4];
            let name = before
                .rsplit(|c| c == '/' || c == '\\')
                .next()
                .unwrap_or(before)
                .to_string();
            if !name.is_empty() && !jars.contains(&name) {
                jars.push(name);
            }
        }
    }
    jars
}

fn infer_id_from_jar(jar_name: &str) -> String {
    let stem = jar_name.trim_end_matches(".jar");
    let mut parts = Vec::new();
    for part in stem.split(|c| c == '-' || c == '_' || c == '+') {
        let part = part.trim();
        if part.is_empty() || looks_like_version_token(part) || part.starts_with("mc") {
            break;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        normalize_token(stem)
    } else {
        normalize_token(&parts.join("-"))
    }
}

fn looks_like_version_token(token: &str) -> bool {
    let lower = token.to_lowercase();
    lower
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
        || matches!(
            lower.as_str(),
            "fabric" | "forge" | "neoforge" | "quilt" | "common" | "client"
        )
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .map(normalize_token)
        .filter(|token| !token.is_empty())
        .collect()
}

fn normalize_token(value: &str) -> String {
    let mut out = String::new();
    let mut previous_dash = false;
    for ch in value.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            previous_dash = false;
        } else if ch == '_' || ch == '-' || ch == '.' || ch == ' ' {
            if !previous_dash && !out.is_empty() {
                out.push('-');
                previous_dash = true;
            }
        }
    }
    out.trim_matches('-').to_string()
}

fn compact_token(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

fn is_noise_token(token: &str) -> bool {
    matches!(
        token,
        "jar"
            | "mods"
            | "mod"
            | "file"
            | "minecraft"
            | "fabric"
            | "forge"
            | "neoforge"
            | "quilt"
            | "java"
            | "exception"
            | "error"
            | "mixin"
            | "mixins"
            | "caused"
            | "client"
            | "server"
            | "common"
            | "unknown"
            | "null"
    )
}

/// True for loader phrases like `missing mod` / `missing mods`, but not
/// `missing model` (resource/model errors that invent fake install targets).
fn contains_missing_mod_phrase(lower: &str) -> bool {
    let mut search = lower;
    while let Some(idx) = search.find("missing mod") {
        let after = &search[idx + "missing mod".len()..];
        let ok = match after.chars().next() {
            None => true,
            Some('s') => {
                // `missing mods` ok; `missing modsFoo` not a real phrase but
                // treat non-alnum after optional `s` as boundary.
                let rest = &after[1..];
                rest.is_empty()
                    || !rest
                        .chars()
                        .next()
                        .map(|c| c.is_ascii_alphanumeric())
                        .unwrap_or(false)
            }
            Some(c) if !c.is_ascii_alphanumeric() => true,
            Some(_) => false, // e.g. "missing model"
        };
        if ok {
            return true;
        }
        search = &search[idx + 1..];
    }
    false
}

fn validate_report_id(report_id: &str) -> Result<PathBuf, CrashError> {
    let relative = PathBuf::from(report_id);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(CrashError::InvalidReportPath(report_id.to_string()));
    }
    let normalized = report_id.replace('\\', "/");
    let ok = (normalized.starts_with("crash-reports/")
        && normalized.to_lowercase().ends_with(".txt"))
        || (normalized.starts_with("hs_err/")
            && normalized
                .rsplit('/')
                .next()
                .map(|n| {
                    let lower = n.to_lowercase();
                    lower.starts_with("hs_err_pid") && lower.ends_with(".log")
                })
                .unwrap_or(false))
        || (normalized.starts_with(".tuffbox/imported-crashes/")
            && (normalized.to_lowercase().ends_with(".txt")
                || normalized.to_lowercase().ends_with(".log")));
    if !ok {
        return Err(CrashError::InvalidReportPath(report_id.to_string()));
    }
    // hs_err ids are virtual (`hs_err/name`) — resolve to instance root file.
    if normalized.starts_with("hs_err/") {
        let name = normalized.trim_start_matches("hs_err/");
        return Ok(PathBuf::from(name));
    }
    Ok(relative)
}

/// List JVM fatal error logs in the instance root (`hs_err_pid*.log`).
pub fn list_hs_err_logs(project_dir: impl AsRef<Path>) -> Result<Vec<HsErrSummary>, CrashError> {
    let project_dir = project_dir.as_ref();
    let mut out = Vec::new();
    let entries = match fs::read_dir(project_dir) {
        Ok(e) => e,
        Err(_) => return Ok(out),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let lower = name.to_lowercase();
        if !(lower.starts_with("hs_err_pid") && lower.ends_with(".log")) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs());
        let content = fs::read_to_string(&path).unwrap_or_default();
        let (kind, frame, preview) = summarize_hs_err(&content);
        out.push(HsErrSummary {
            id: format!("hs_err/{name}"),
            name,
            path,
            size: metadata.len(),
            modified,
            kind,
            problematic_frame: frame,
            preview,
        });
    }
    out.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| b.name.cmp(&a.name))
    });
    Ok(out)
}

pub fn summarize_hs_err(content: &str) -> (String, Option<String>, String) {
    let lower = content.to_lowercase();
    let kind = if lower.contains("outofmemory")
        || lower.contains("java heap space")
        || lower.contains("native memory allocation")
        || lower.contains("# there is insufficient memory")
    {
        "oom".to_string()
    } else if lower.contains("problematic frame")
        || lower.contains("a fatal error has been detected")
    {
        "native".to_string()
    } else {
        "unknown".to_string()
    };
    let frame = content.lines().find_map(|line| {
        let t = line.trim();
        if t.to_lowercase().starts_with("# problematic frame:") {
            Some(t.trim_start_matches('#').trim().to_string())
        } else if t.starts_with("C  [") || t.starts_with("j  ") || t.starts_with("v  ~") {
            Some(t.to_string())
        } else {
            None
        }
    });
    let preview: String = content
        .lines()
        .take(12)
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(600)
        .collect();
    (kind, frame, preview)
}

pub fn extract_world_coords(text: &str) -> Option<WorldCrashCoords> {
    for line in text.lines() {
        let lower = line.to_lowercase();
        let label = if lower.contains("entity") && lower.contains("coordinate") {
            "Entity"
        } else if lower.contains("block location") {
            "Block"
        } else if lower.contains("location:") && lower.contains("world:") {
            "Location"
        } else {
            continue;
        };
        if let Some((x, y, z)) = parse_three_numbers(line) {
            return Some(WorldCrashCoords {
                x,
                y,
                z,
                label: label.into(),
            });
        }
    }
    None
}

fn parse_three_numbers(line: &str) -> Option<(f64, f64, f64)> {
    let mut nums = Vec::new();
    let mut cur = String::new();
    for ch in line.chars() {
        if ch.is_ascii_digit() || ch == '-' || ch == '.' {
            cur.push(ch);
        } else if !cur.is_empty() {
            if let Ok(v) = cur.parse::<f64>() {
                nums.push(v);
            }
            cur.clear();
            if nums.len() >= 3 {
                break;
            }
        }
    }
    if !cur.is_empty() {
        if let Ok(v) = cur.parse::<f64>() {
            nums.push(v);
        }
    }
    if nums.len() >= 3 {
        Some((nums[0], nums[1], nums[2]))
    } else {
        None
    }
}

pub fn extract_memory_hint(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.to_lowercase().starts_with("memory:") {
            let s = trimmed.trim_start_matches(|c: char| {
                c.eq_ignore_ascii_case(&'m')
                    || c.eq_ignore_ascii_case(&'e')
                    || c.eq_ignore_ascii_case(&'o')
                    || c.eq_ignore_ascii_case(&'r')
                    || c.eq_ignore_ascii_case(&'y')
                    || c == ':'
                    || c.is_whitespace()
            });
            // Simpler: split once on ':'
            if let Some((_, rest)) = trimmed.split_once(':') {
                let s = rest.trim();
                if !s.is_empty() {
                    return Some(s.chars().take(160).collect());
                }
            }
            let _ = s;
        }
    }
    let lower = text.to_lowercase();
    if lower.contains("outofmemory") || lower.contains("java heap space") {
        return Some("OutOfMemoryError detected — raise -Xmx or reduce loaded chunks/mods.".into());
    }
    None
}

/// Copy an external player crash into `.tuffbox/imported-crashes/` and return its report id.
pub fn import_external_crash(
    project_dir: impl AsRef<Path>,
    file_name: &str,
    content: &str,
) -> Result<String, CrashError> {
    let project_dir = project_dir.as_ref();
    let safe = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let safe = if safe.to_lowercase().ends_with(".txt") || safe.to_lowercase().ends_with(".log") {
        safe
    } else {
        format!("{safe}.txt")
    };
    let dir = project_dir.join(".tuffbox").join("imported-crashes");
    fs::create_dir_all(&dir).map_err(|source| CrashError::ReadCrashReport {
        path: dir.clone(),
        source,
    })?;
    let path = dir.join(&safe);
    if content.len() as u64 > MAX_REPORT_BYTES {
        return Err(CrashError::ReportTooLarge {
            size: content.len() as u64,
        });
    }
    fs::write(&path, content).map_err(|source| CrashError::ReadCrashReport {
        path: path.clone(),
        source,
    })?;
    Ok(format!(".tuffbox/imported-crashes/{safe}"))
}

/// Export a support pack zip for Discord/GitHub (no secrets beyond log tails).
pub fn export_diagnose_support_pack(
    project_dir: impl AsRef<Path>,
    report_id: Option<&str>,
    findings_summary: &str,
    recent_events_summary: &str,
    action_plan_json: Option<&str>,
    mod_ids: &[String],
) -> Result<SupportPackResult, CrashError> {
    use std::io::Write;
    use zip::{write::SimpleFileOptions, ZipWriter};

    let project_dir = project_dir.as_ref();
    let stamp = chrono_like_stamp();
    let out_dir = project_dir.join(".tuffbox").join("support");
    fs::create_dir_all(&out_dir).map_err(|source| CrashError::ReadCrashReport {
        path: out_dir.clone(),
        source,
    })?;
    let out_path = out_dir.join(format!("diagnose-{stamp}.zip"));
    let file = fs::File::create(&out_path).map_err(|source| CrashError::ReadCrashReport {
        path: out_path.clone(),
        source,
    })?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut file_count = 0usize;

    let meta = format!(
        "{{\n  \"generatedAt\": \"{stamp}\",\n  \"reportId\": {report},\n  \"findings\": {findings},\n  \"recentEvents\": {events}\n}}\n",
        stamp = stamp,
        report = serde_json::to_string(&report_id).unwrap_or_else(|_| "null".into()),
        findings = serde_json::to_string(findings_summary).unwrap_or_else(|_| "\"\"".into()),
        events = serde_json::to_string(recent_events_summary).unwrap_or_else(|_| "\"\"".into()),
    );
    zip.start_file("meta.json", options)
        .map_err(|e| CrashError::InvalidReportPath(e.to_string()))?;
    zip.write_all(meta.as_bytes())
        .map_err(|source| CrashError::ReadCrashReport {
            path: out_path.clone(),
            source,
        })?;
    file_count += 1;

    if let Some(plan) = action_plan_json {
        zip.start_file("action-plan.json", options)
            .map_err(|e| CrashError::InvalidReportPath(e.to_string()))?;
        zip.write_all(plan.as_bytes())
            .map_err(|source| CrashError::ReadCrashReport {
                path: out_path.clone(),
                source,
            })?;
        file_count += 1;
    }

    // Attach selected crash / hs_err / imported file.
    if let Some(id) = report_id {
        if id != "__latest_log__" && id != "__launcher_log__" {
            if let Ok(rel) = validate_report_id(id) {
                let path = project_dir.join(rel);
                if path.is_file() {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("crash.txt");
                    if let Ok(bytes) = fs::read(&path) {
                        zip.start_file(format!("logs/{name}"), options)
                            .map_err(|e| CrashError::InvalidReportPath(e.to_string()))?;
                        zip.write_all(&bytes)
                            .map_err(|source| CrashError::ReadCrashReport {
                                path: path.clone(),
                                source,
                            })?;
                        file_count += 1;
                    }
                }
            }
        }
    }

    // Always include latest.log tail if present.
    let latest = project_dir.join("logs").join("latest.log");
    if latest.is_file() {
        let tail = read_tail_bytes(&latest, 256 * 1024).unwrap_or_default();
        zip.start_file("logs/latest.log.tail.txt", options)
            .map_err(|e| CrashError::InvalidReportPath(e.to_string()))?;
        zip.write_all(&tail)
            .map_err(|source| CrashError::ReadCrashReport {
                path: latest.clone(),
                source,
            })?;
        file_count += 1;
    }

    if !mod_ids.is_empty() {
        let body = format!("mod_count={}\n{}\n", mod_ids.len(), mod_ids.join("\n"));
        zip.start_file("modlist-ids.txt", options)
            .map_err(|e| CrashError::InvalidReportPath(e.to_string()))?;
        zip.write_all(body.as_bytes())
            .map_err(|source| CrashError::ReadCrashReport {
                path: out_path.clone(),
                source,
            })?;
        file_count += 1;
    }

    zip.finish()
        .map_err(|e| CrashError::InvalidReportPath(e.to_string()))?;
    Ok(SupportPackResult {
        path: out_path,
        file_count,
    })
}

fn chrono_like_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn read_tail_bytes(path: &Path, max: usize) -> Result<Vec<u8>, std::io::Error> {
    let data = fs::read(path)?;
    if data.len() <= max {
        Ok(data)
    } else {
        Ok(data[data.len() - max..].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{
        LoaderKind, LoaderSpec, MinecraftSpec, ModSource, ProjectManifest, ProjectMetadata, Side,
        SourceKind,
    };

    fn manifest() -> ProjectManifest {
        ProjectManifest {
            schema_version: "0.1.0".to_string(),
            project: ProjectMetadata {
                id: "test".to_string(),
                name: "Test".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: Vec::new(),
            },
            minecraft: MinecraftSpec {
                version: "1.20.1".to_string(),
            },
            loader: LoaderSpec {
                kind: LoaderKind::Fabric,
                version: "0.15.0".to_string(),
            },
            brief: None,
            listing: None,
            java: None,
            profiles: Vec::new(),
            mods: vec![ModSpec {
                id: "sodium".to_string(),
                name: "Sodium".to_string(),
                source: ModSource {
                    kind: SourceKind::Modrinth,
                    project_id: Some("AANobbMI".to_string()),
                    file_id: None,
                    url: None,
                    path: None,
                    icon_url: None,
                    categories: Vec::new(),
                },
                version: "0.5.8".to_string(),
                file_name: Some("sodium-fabric-mc1.20.1-0.5.8.jar".to_string()),
                hashes: None,
                side: Side::Client,
                dependencies: Vec::new(),
                status: Vec::new(),
                content_type: crate::manifest::ContentType::Mod,
                authors: Vec::new(),
                option: None,
            }],
            overrides: None,
        }
    }

    #[test]
    fn detects_mod_file_suspect_from_crash_report() {
        let text = "Mod File: /instance/mods/sodium-fabric-mc1.20.1-0.5.8.jar\nCaused by: java.lang.IllegalStateException";
        let (_signals, suspects) =
            analyze_text_for_suspects(text, "crash-reports/latest.txt", &manifest());
        assert_eq!(suspects[0].id, "sodium");
        assert!(suspects[0].confidence >= 88);
    }

    #[test]
    fn prefers_disable_optimization_mods_on_version_conflict() {
        let mut m = manifest();
        let base = m.mods[0].clone();
        let mut mk = |id: &str, name: &str| {
            let mut s = base.clone();
            s.id = id.to_string();
            s.name = name.to_string();
            s
        };
        m.mods.push(mk("indium", "Indium"));
        m.mods.push(mk("spb-revamped", "SP-Backrooms Revamped"));
        let graph = crate::graph::DependencyGraph::from_manifest(&m);
        let suspects = vec![
            SuspectedMod {
                id: "spb-revamped".to_string(),
                name: "SP-Backrooms Revamped".to_string(),
                version: Some("1.2.0".to_string()),
                file_name: None,
                known_in_manifest: true,
                confidence: 95,
                evidence: Vec::new(),
                authors: Vec::new(),
                blame_role: BlameRole::Primary,
                match_sources: vec!["log_line".into()],
            },
            SuspectedMod {
                id: "sodium".to_string(),
                name: "Sodium".to_string(),
                version: Some("0.5.13".to_string()),
                file_name: None,
                known_in_manifest: true,
                confidence: 90,
                evidence: Vec::new(),
                authors: Vec::new(),
                blame_role: BlameRole::Secondary,
                match_sources: vec!["log_line".into()],
            },
            SuspectedMod {
                id: "indium".to_string(),
                name: "Indium".to_string(),
                version: Some("1.0.36".to_string()),
                file_name: None,
                known_in_manifest: true,
                confidence: 90,
                evidence: Vec::new(),
                authors: Vec::new(),
                blame_role: BlameRole::Secondary,
                match_sources: vec!["log_line".into()],
            },
        ];
        let signals = vec![
            CrashSignal {
                source: "logs/latest.log".into(),
                line_number: 3,
                kind: CrashSignalKind::ModVersionMismatch,
                text: "Mod 'SP-Backrooms Revamped' (spb-revamped) 1.2.0 is incompatible with any version of mod 'Sodium'".into(),
            },
            CrashSignal {
                source: "logs/latest.log".into(),
                line_number: 4,
                kind: CrashSignalKind::ModVersionMismatch,
                text: "Mod 'SP-Backrooms Revamped' (spb-revamped) 1.2.0 is incompatible with any version of mod 'Indium'".into(),
            },
        ];
        let plan = create_crash_fix_plan(&graph, &[], &suspects, &signals);
        assert!(
            plan.options.len() >= 3,
            "expected ~3 options, got {}",
            plan.options.len()
        );
        let preferred: Vec<&ChangeOption> = plan.options.iter().filter(|o| o.preferred).collect();
        // Preferred first moves = the optimisation / bridge sides, not the content.
        assert!(preferred
            .iter()
            .any(|o| o.label.to_lowercase().contains("sodium")));
        assert!(preferred
            .iter()
            .any(|o| o.label.to_lowercase().contains("indium")));
        assert!(!preferred
            .iter()
            .any(|o| o.label.to_lowercase().contains("backrooms")));
        // The content side must be present as a (non-preferred) alternative.
        assert!(plan
            .options
            .iter()
            .any(|o| { o.label.to_lowercase().contains("spb-revamped") && !o.preferred }));
        // Default (applied) actions disable the replaceable sides, never content.
        assert!(plan.actions.iter().any(
            |a| matches!(a, ChangeAction::DisableMod { node_id } if node_id.0 == "mod:sodium")
        ));
        assert!(plan.actions.iter().any(
            |a| matches!(a, ChangeAction::DisableMod { node_id } if node_id.0 == "mod:indium")
        ));
        assert!(!plan.actions.iter().any(
            |a| matches!(a, ChangeAction::DisableMod { node_id } if node_id.0 == "mod:spb-revamped")
        ));
    }

    #[test]
    fn detects_mixin_suspect_by_mod_id() {
        let text = "Mixin apply failed sodium.mixins.json:features.render.MixinWorldRenderer -> net.minecraft.WorldRenderer";
        let (_signals, suspects) = analyze_text_for_suspects(text, "logs/latest.log", &manifest());
        assert_eq!(suspects[0].id, "sodium");
    }

    #[test]
    fn resolves_compact_provided_by_id_to_installed_mod() {
        let mut manifest = manifest();
        manifest.mods.push(ModSpec {
            id: "critters-and-companions".to_string(),
            name: "Critters and Companions".to_string(),
            source: ModSource {
                kind: SourceKind::Modrinth,
                project_id: Some("critters-and-companions".to_string()),
                file_id: None,
                url: None,
                path: None,
                icon_url: None,
                categories: Vec::new(),
            },
            version: "2.1.0".to_string(),
            file_name: Some("crittersandcompanions-fabric-2.1.0.jar".to_string()),
            hashes: None,
            side: Side::Both,
            dependencies: Vec::new(),
            status: Vec::new(),
            content_type: crate::manifest::ContentType::Mod,
            authors: Vec::new(),
            option: None,
        });
        // A different "critters*" mod must not steal the provided-by match via
        // the shared short name token "critters".
        manifest.mods.push(ModSpec {
            id: "cosy-critters".to_string(),
            name: "Cosy Critters & Creepy Crawlies".to_string(),
            source: ModSource {
                kind: SourceKind::Modrinth,
                project_id: Some("cosy-critters".to_string()),
                file_id: None,
                url: None,
                path: None,
                icon_url: None,
                categories: Vec::new(),
            },
            version: "0.1.2".to_string(),
            file_name: Some("cosycritters-0.1.2+1.21.1-fabric.jar".to_string()),
            hashes: None,
            side: Side::Client,
            dependencies: Vec::new(),
            status: Vec::new(),
            content_type: crate::manifest::ContentType::Mod,
            authors: Vec::new(),
            option: None,
        });
        let text = "Could not execute entrypoint stage 'main' due to errors, provided by 'crittersandcompanions'!";

        let (signals, suspects) =
            analyze_text_for_suspects(text, "crash-reports/latest.txt", &manifest);

        assert_eq!(signals[0].kind, CrashSignalKind::Entrypoint);
        assert_eq!(suspects[0].id, "critters-and-companions");
        assert_eq!(suspects[0].name, "Critters and Companions");
        assert!(suspects[0].known_in_manifest);
        assert!(suspects[0].confidence >= 96);
        assert!(
            !suspects.iter().any(|s| s.id == "cosy-critters"),
            "Cosy Critters should not match crittersandcompanions via substring 'critters'"
        );
    }

    #[test]
    fn ignores_benign_provided_by_without_failure() {
        let mut manifest = manifest();
        manifest.mods.push(ModSpec {
            id: "critters-and-companions".to_string(),
            name: "Critters and Companions".to_string(),
            source: ModSource {
                kind: SourceKind::Modrinth,
                project_id: Some("critters-and-companions".to_string()),
                file_id: None,
                url: None,
                path: None,
                icon_url: None,
                categories: Vec::new(),
            },
            version: "2.1.0".to_string(),
            file_name: Some("crittersandcompanions-fabric-2.1.0.jar".to_string()),
            hashes: None,
            side: Side::Both,
            dependencies: Vec::new(),
            status: Vec::new(),
            content_type: crate::manifest::ContentType::Mod,
            authors: Vec::new(),
            option: None,
        });
        let text = "[Fabric] Loading 120 mods:\n\t- crittersandcompanions 2.1.0 provided by 'crittersandcompanions'\nDone.";
        let (signals, suspects) = analyze_text_for_suspects(text, "logs/latest.log", &manifest);
        assert!(
            !signals
                .iter()
                .any(|s| s.kind == CrashSignalKind::Entrypoint),
            "benign 'provided by' must not be Entrypoint"
        );
        assert!(
            suspects.is_empty(),
            "healthy log must not suspect Critters: {:?}",
            suspects.iter().map(|s| &s.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn merge_upgrades_inferred_provider_to_manifest_mod() {
        let manifest = manifest();
        let inferred = SuspectedMod {
            id: "s-o-d-i-u-m".to_string(),
            name: "s-o-d-i-u-m".to_string(),
            version: None,
            file_name: None,
            known_in_manifest: false,
            confidence: 70,
            evidence: Vec::new(),
            authors: Vec::new(),
            blame_role: BlameRole::Related,
            match_sources: Vec::new(),
        };
        let resolved = SuspectedMod {
            id: "sodium".to_string(),
            name: manifest.mods[0].name.clone(),
            version: Some(manifest.mods[0].version.clone()),
            file_name: manifest.mods[0].file_name.clone(),
            known_in_manifest: true,
            confidence: 96,
            evidence: Vec::new(),
            authors: Vec::new(),
            blame_role: BlameRole::Related,
            match_sources: Vec::new(),
        };

        let suspects = merge_suspected_mods([inferred, resolved]);

        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].id, "sodium");
        assert_eq!(suspects[0].name, "Sodium");
        assert!(suspects[0].known_in_manifest);
    }

    #[test]
    fn detects_opengl_debug_as_render_signal() {
        let text = "OpenGL debug message: id=1282, source=API, type=ERROR, severity=HIGH, message='GL_INVALID_OPERATION error generated. No active program.'";
        let (signals, suspects) = analyze_text_for_suspects(text, "logs/latest.log", &manifest());
        assert_eq!(signals[0].kind, CrashSignalKind::OpenGl);
        assert!(suspects.is_empty());
    }

    #[test]
    fn detects_shader_resourcepack_failure_as_render_signal() {
        let text = "Caught error loading resourcepacks, removing all selected resourcepacks\n\
Failed to load required shader programs:\n\
 - minecraft:core/rendertype_entity_translucent: Could not find shader: minecraft:rendertype_entity_translucent (VERTEX)";
        let (signals, _) = analyze_text_for_suspects(text, "logs/latest.log", &manifest());
        assert!(
            signals.iter().any(|s| s.kind == CrashSignalKind::OpenGl),
            "expected OpenGl signal, got {:?}",
            signals.iter().map(|s| &s.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn detects_missing_dependency_with_named_mod() {
        let mut manifest = manifest();
        manifest.mods.push(ModSpec {
            id: "lithium".to_string(),
            name: "Lithium".to_string(),
            source: ModSource {
                kind: SourceKind::Modrinth,
                project_id: Some("lithium".to_string()),
                file_id: None,
                url: None,
                path: None,
                icon_url: None,
                categories: Vec::new(),
            },
            version: "0.11.0".to_string(),
            file_name: Some("lithium-fabric-0.11.0.jar".to_string()),
            hashes: None,
            side: Side::Client,
            dependencies: Vec::new(),
            status: Vec::new(),
            content_type: crate::manifest::ContentType::Mod,
            authors: Vec::new(),
            option: None,
        });
        // Real Fabric loader resolution error format.
        let text = "net.fabricmc.loader.impl.discovery.ModResolutionException: Mod 'Lithium' (lithium) requires version 1.0.0 or later of mod 'jellysquid3's sodium' (sodium), which is missing!";
        let (signals, suspects) =
            analyze_text_for_suspects(text, "crash-reports/latest.txt", &manifest);
        assert_eq!(signals[0].kind, CrashSignalKind::MissingDependency);
        assert!(suspects.iter().any(|s| s.id == "lithium"));
        assert!(suspects.iter().any(|s| s.id == "sodium"));

        let missing = extract_missing_dependency_ids(text);
        assert_eq!(missing, vec!["sodium".to_string()]);

        let hints = build_hints(&signals, &suspects);
        let hint = hints
            .iter()
            .find(|h| h.id == "missing-dependency")
            .expect("missing-dependency hint");
        assert!(
            hint.fixes
                .iter()
                .any(|f| f.mod_id.as_deref() == Some("sodium")),
            "expected per-mod Install for sodium, got {:?}",
            hint.fixes
        );
        assert!(
            !hint
                .fixes
                .iter()
                .any(|f| f.mod_id.as_deref() == Some("lithium")),
            "requester lithium must not get an Install button"
        );
    }

    #[test]
    fn missing_model_builtin_entity_does_not_suggest_fake_install() {
        // `missing` + substring `mod` inside `model` used to classify this as
        // MissingDependency and invent Install minecraftbuiltinentity.
        let text = "Missing model 'minecraft:builtin/entity' referenced from: item/foo";
        let (signals, suspects) = analyze_text_for_suspects(text, "logs/latest.log", &manifest());
        assert!(
            !signals
                .iter()
                .any(|s| s.kind == CrashSignalKind::MissingDependency),
            "resource model lines must not be MissingDependency, got {:?}",
            signals.iter().map(|s| &s.kind).collect::<Vec<_>>()
        );
        assert!(
            !suspects.iter().any(|s| s.id == "minecraftbuiltinentity"
                || crate::action_plan::is_invented_vanilla_resource_mod_id(&s.id)),
            "must not invent suspects from vanilla resource paths, got {:?}",
            suspects.iter().map(|s| &s.id).collect::<Vec<_>>()
        );

        let hints = build_hints(&signals, &suspects);
        assert!(
            !hints.iter().any(|h| h.id == "missing-dependency"),
            "must not emit missing-dependency Install hint, got {:?}",
            hints
        );
        assert!(
            !hints.iter().any(|h| {
                h.fixes.iter().any(|f| {
                    f.mod_id
                        .as_deref()
                        .is_some_and(crate::action_plan::is_invented_vanilla_resource_mod_id)
                }) || h
                    .related_mods
                    .iter()
                    .any(|id| crate::action_plan::is_invented_vanilla_resource_mod_id(id))
            }),
            "no Install for invented vanilla resource ids"
        );
    }

    #[test]
    fn invented_vanilla_resource_id_filtered_from_missing_dep_extract() {
        let text = "Mod 'Foo' (foo) requires version 1.0.0 or later of mod 'minecraft:builtin/entity' (minecraft:builtin/entity), which is missing!";
        let missing = extract_missing_dependency_ids(text);
        assert!(
            !missing
                .iter()
                .any(|id| crate::action_plan::is_invented_vanilla_resource_mod_id(id)),
            "extract must drop compacted vanilla paths, got {missing:?}"
        );
        assert!(
            !extract_named_mods(text)
                .iter()
                .any(|id| crate::action_plan::is_invented_vanilla_resource_mod_id(id)),
            "named-mod extract must drop compacted vanilla paths"
        );
    }

    #[test]
    fn detects_wrong_minecraft_version_for_mod() {
        let mut manifest = manifest();
        manifest.mods.push(ModSpec {
            id: "iris".to_string(),
            name: "Iris".to_string(),
            source: ModSource {
                kind: SourceKind::Modrinth,
                project_id: Some("iris".to_string()),
                file_id: None,
                url: None,
                path: None,
                icon_url: None,
                categories: Vec::new(),
            },
            version: "1.7.0".to_string(),
            file_name: Some("iris-1.7.0.jar".to_string()),
            hashes: None,
            side: Side::Client,
            dependencies: Vec::new(),
            status: Vec::new(),
            content_type: crate::manifest::ContentType::Mod,
            authors: Vec::new(),
            option: None,
        });
        let text = "Incompatible mod set!\nMod 'Iris' (iris) requires version 1.21.4 or later of 'Minecraft' (minecraft), but a non-matching version 1.20.1 is present!";
        let (signals, suspects) =
            analyze_text_for_suspects(text, "crash-reports/latest.txt", &manifest);
        assert!(
            signals
                .iter()
                .any(|s| s.kind == CrashSignalKind::MinecraftVersionMismatch),
            "expected a MinecraftVersionMismatch signal, got {:?}",
            signals
        );
        assert!(suspects.iter().any(|s| s.id == "iris"));
        assert_eq!(suspects[0].confidence, 90);
    }

    #[test]
    fn detects_wrong_loader_for_mod() {
        let mut manifest = manifest();
        manifest.mods.push(ModSpec {
            id: "create".to_string(),
            name: "Create".to_string(),
            source: ModSource {
                kind: SourceKind::Curseforge,
                project_id: Some("create".to_string()),
                file_id: None,
                url: None,
                path: None,
                icon_url: None,
                categories: Vec::new(),
            },
            version: "0.5.1".to_string(),
            file_name: Some("create-1.20.1-0.5.1.jar".to_string()),
            hashes: None,
            side: Side::Both,
            dependencies: Vec::new(),
            status: Vec::new(),
            content_type: crate::manifest::ContentType::Mod,
            authors: Vec::new(),
            option: None,
        });
        let text = "Mod 'Create' (create) requires the Forge mod loader, but Fabric Loader 0.15.0 is in use!";
        let (signals, suspects) =
            analyze_text_for_suspects(text, "crash-reports/latest.txt", &manifest);
        assert_eq!(signals[0].kind, CrashSignalKind::WrongLoader);
        assert!(suspects.iter().any(|s| s.id == "create"));
    }

    #[test]
    fn detects_wrong_mod_version_conflict() {
        let mut manifest = manifest();
        manifest.mods.push(ModSpec {
            id: "sodium".to_string(),
            name: "Sodium".to_string(),
            source: ModSource {
                kind: SourceKind::Modrinth,
                project_id: Some("AANobbMI".to_string()),
                file_id: None,
                url: None,
                path: None,
                icon_url: None,
                categories: Vec::new(),
            },
            version: "0.6.0".to_string(),
            file_name: Some("sodium-fabric-0.6.0.jar".to_string()),
            hashes: None,
            side: Side::Client,
            dependencies: Vec::new(),
            status: Vec::new(),
            content_type: crate::manifest::ContentType::Mod,
            authors: Vec::new(),
            option: None,
        });
        let text = "Mod 'Reese's Sodium Options' (reeses-sodium-options) 1.8.0 conflicts with 'Sodium' (sodium) 0.6.0 (incompatible).";
        let (signals, suspects) =
            analyze_text_for_suspects(text, "crash-reports/latest.txt", &manifest);
        assert_eq!(signals[0].kind, CrashSignalKind::ModVersionMismatch);
        assert!(suspects.iter().any(|s| s.id == "sodium"));
    }

    #[test]
    fn detects_loader_version_mismatch() {
        let mut manifest = manifest();
        manifest.mods.push(ModSpec {
            id: "fabric-api".to_string(),
            name: "Fabric API".to_string(),
            source: ModSource {
                kind: SourceKind::Modrinth,
                project_id: Some("fabric-api".to_string()),
                file_id: None,
                url: None,
                path: None,
                icon_url: None,
                categories: Vec::new(),
            },
            version: "0.92.0".to_string(),
            file_name: Some("fabric-api-0.92.0.jar".to_string()),
            hashes: None,
            side: Side::Both,
            dependencies: Vec::new(),
            status: Vec::new(),
            content_type: crate::manifest::ContentType::Mod,
            authors: Vec::new(),
            option: None,
        });
        let text = "Mod 'Fabric API' (fabric-api) requires Fabric Loader 0.16.0 or later, but 0.15.0 is present!";
        let (signals, suspects) =
            analyze_text_for_suspects(text, "crash-reports/latest.txt", &manifest);
        assert_eq!(signals[0].kind, CrashSignalKind::LoaderVersionMismatch);
        assert!(suspects.iter().any(|s| s.id == "fabric-api"));
    }

    #[test]
    fn detects_stacktrace_mod_by_java_package() {
        // Real crash from Fabulously Optimized: clientcommands NPE in tick task.
        let mut manifest = manifest();
        manifest.mods.push(ModSpec {
            id: "clientcommands".to_string(),
            name: "Client Commands".to_string(),
            source: ModSource {
                kind: SourceKind::Modrinth,
                project_id: Some("clientcommands".to_string()),
                file_id: None,
                url: None,
                path: None,
                icon_url: None,
                categories: Vec::new(),
            },
            version: "2.9.11".to_string(),
            file_name: Some("clientcommands-2.9.11.jar".to_string()),
            hashes: None,
            side: Side::Client,
            dependencies: Vec::new(),
            status: Vec::new(),
            content_type: crate::manifest::ContentType::Mod,
            authors: Vec::new(),
            option: None,
        });
        let text = "java.lang.NullPointerException: Cannot read field \"field_7512\" because \"player\" is null\n\tat knot//net.earthcomputer.clientcommands.features.PlayerRandCracker.throwItem(PlayerRandCracker.java:412)";
        let (_signals, suspects) =
            analyze_text_for_suspects(text, "crash-reports/latest.txt", &manifest);
        assert!(suspects.iter().any(|s| s.id == "clientcommands"));
    }

    #[test]
    fn detects_clientcommands_from_real_fo_crash() {
        // Real fragment from Fabulously Optimized 8.1.0 crash-2025-09-20:
        // the stack trace names `net.earthcomputer.clientcommands`, and the
        // resource-pack list flags `clientcommands (incompatible)`.
        let mut manifest = manifest();
        manifest.mods.push(ModSpec {
            id: "clientcommands".to_string(),
            name: "Client Commands".to_string(),
            source: ModSource {
                kind: SourceKind::Modrinth,
                project_id: Some("clientcommands".to_string()),
                file_id: None,
                url: None,
                path: None,
                icon_url: None,
                categories: Vec::new(),
            },
            version: "2.9.11".to_string(),
            file_name: Some("clientcommands-2.9.11.jar".to_string()),
            hashes: None,
            side: Side::Client,
            dependencies: Vec::new(),
            status: Vec::new(),
            content_type: crate::manifest::ContentType::Mod,
            authors: Vec::new(),
            option: None,
        });
        let text = "\
java.lang.NullPointerException: Cannot read field \"field_7512\" because \"player\" is null
	at knot//net.earthcomputer.clientcommands.features.PlayerRandCracker.throwItem(PlayerRandCracker.java:412)
	at knot//net.earthcomputer.clientcommands.task.ItemThrowTask.onTick(ItemThrowTask.java:60)
Mixins in Stacktrace:
	net.minecraft.class_310:
		net.earthcomputer.clientcommands.mixin.events.MinecraftMixin (mixins.clientcommands.json)
Resource Packs: vanilla, fabric, animatica, antip2w, betterconfig, clientcommands (incompatible), cloth-config";
        let (signals, suspects) =
            analyze_text_for_suspects(text, "crash-reports/latest.txt", &manifest);
        assert!(
            suspects.iter().any(|s| s.id == "clientcommands"),
            "clientcommands should be attributed via Java package / mixin / incompatible marker"
        );
        // The stack-trace lines must carry a high-confidence signal.
        assert!(signals
            .iter()
            .any(|s| s.kind == CrashSignalKind::Exception || s.kind == CrashSignalKind::Mixin));
    }

    #[test]
    fn detects_incompatible_marker_in_resource_pack_list() {
        let mut manifest = manifest();
        manifest.mods.push(ModSpec {
            id: "clientcommands".to_string(),
            name: "Client Commands".to_string(),
            source: ModSource {
                kind: SourceKind::Modrinth,
                project_id: Some("clientcommands".to_string()),
                file_id: None,
                url: None,
                path: None,
                icon_url: None,
                categories: Vec::new(),
            },
            version: "2.9.11".to_string(),
            file_name: Some("clientcommands-2.9.11.jar".to_string()),
            hashes: None,
            side: Side::Client,
            dependencies: Vec::new(),
            status: Vec::new(),
            content_type: crate::manifest::ContentType::Mod,
            authors: Vec::new(),
            option: None,
        });
        let text = "Resource Packs: vanilla, fabric, animatica, antip2w, betterconfig, clientcommands (incompatible), cloth-config";
        let (signals, suspects) =
            analyze_text_for_suspects(text, "crash-reports/latest.txt", &manifest);
        assert!(
            suspects.iter().any(|s| s.id == "clientcommands"),
            "clientcommands should be attributed via the (incompatible) marker"
        );
        assert!(signals
            .iter()
            .any(|s| s.kind == CrashSignalKind::ModVersionMismatch));
    }

    #[test]
    fn validates_report_path() {
        assert!(validate_report_id("crash-reports/crash.txt").is_ok());
        assert!(validate_report_id("../crash.txt").is_err());
        assert!(validate_report_id("logs/latest.log").is_err());
    }

    #[test]
    fn parses_forge_sections_without_vanilla_sections_or_duplicates() {
        let text = "\
Forge Mod List:
| ID | Name | Version |
| sodium | Sodium | 0.5.8 |
-- System Details --
Memory: 2048 MB / 4096 MB
CPU: 8x Example CPU
JVM Flags:
-Xmx4G";

        let sections = parse_crash_sections(text);
        let titles = sections
            .iter()
            .map(|section| section.title.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            titles,
            vec!["Forge Mod List", "System Details", "JVM Flags"]
        );
        assert_eq!(
            titles
                .iter()
                .filter(|title| **title == "Forge Mod List")
                .count(),
            1
        );
    }

    #[test]
    fn parses_standalone_forge_heading_without_vanilla_start() {
        let sections = parse_crash_sections("preamble\nNeoForge Mod List:\nexamplemod (1.0)");

        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].title, "NeoForge Mod List");
        assert_eq!(sections[0].start_line, 2);
        assert!(sections[0].preview.contains("examplemod"));
    }

    #[test]
    fn diagnosis_does_not_create_missing_log_files() {
        let dir = tempfile::tempdir().unwrap();

        let diagnosis = build_crash_diagnosis(dir.path(), &manifest(), None, Vec::new()).unwrap();

        assert!(!diagnosis.latest_log.exists);
        assert!(!diagnosis.launcher_log.exists);
        assert!(!dir.path().join("logs").exists());
        assert!(!diagnosis.latest_log.path.exists());
        assert!(!diagnosis.launcher_log.path.exists());
    }

    #[test]
    fn newer_healthy_latest_log_skips_stale_crash_report() {
        let dir = tempfile::tempdir().unwrap();
        let crash_dir = dir.path().join("crash-reports");
        let logs_dir = dir.path().join("logs");
        fs::create_dir_all(&crash_dir).unwrap();
        fs::create_dir_all(&logs_dir).unwrap();

        let crash_path = crash_dir.join("crash-2020-01-01_00.00.00-client.txt");
        fs::write(
            &crash_path,
            "---- Minecraft Crash Report ----\njava.lang.NullPointerException: old crash\n\tat bad.Mod.init(Mod.java:1)\n",
        )
        .unwrap();
        // Ensure distinct mtimes on filesystems with coarse resolution.
        std::thread::sleep(std::time::Duration::from_millis(1100));

        let latest = logs_dir.join("latest.log");
        fs::write(
            &latest,
            "[Render thread/INFO]: Sound engine started\n[Render thread/INFO]: Created: 1024x512x4 minecraft:textures/atlas/blocks.png-atlas\n[Render thread/INFO]: Reloading ResourceManager: Default\n",
        )
        .unwrap();

        let diagnosis = build_crash_diagnosis(dir.path(), &manifest(), None, Vec::new()).unwrap();
        assert!(diagnosis.crash_report_stale, "expected stale flag");
        assert!(
            diagnosis.selected_report.is_none(),
            "should not auto-select old crash"
        );
        assert_eq!(diagnosis.analysis_source, "latest_log");
        assert!(
            diagnosis.session_healthy,
            "healthy live log must set session_healthy"
        );
        assert!(
            diagnosis.fix_plan.actions.is_empty(),
            "healthy session must not propose crash-log fixes"
        );
        assert!(
            diagnosis.suspected_mods.is_empty(),
            "healthy session must not keep crash suspects"
        );
        assert!(latest_log_supersedes_crash(
            dir.path(),
            Some(crash_path.as_path()),
            &diagnosis.latest_log.tail
        ));
    }

    #[test]
    fn explicit_report_id_still_loads_stale_crash() {
        let dir = tempfile::tempdir().unwrap();
        let crash_dir = dir.path().join("crash-reports");
        let logs_dir = dir.path().join("logs");
        fs::create_dir_all(&crash_dir).unwrap();
        fs::create_dir_all(&logs_dir).unwrap();

        let name = "crash-2020-01-01_00.00.00-client.txt";
        let crash_path = crash_dir.join(name);
        fs::write(
            &crash_path,
            "---- Minecraft Crash Report ----\njava.lang.NullPointerException: old crash\n",
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(
            logs_dir.join("latest.log"),
            "[Render thread/INFO]: Sound engine started\n",
        )
        .unwrap();

        let id = format!("crash-reports/{name}");
        let diagnosis =
            build_crash_diagnosis(dir.path(), &manifest(), Some(&id), Vec::new()).unwrap();
        assert!(diagnosis.selected_report.is_some());
        assert_eq!(diagnosis.analysis_source, "crash_report");
        assert!(!diagnosis.crash_report_stale);
    }

    #[test]
    fn latest_compatible_fix_target_resolves_to_automatic_selection() {
        assert_eq!(
            resolve_update_target_version(LATEST_COMPATIBLE_VERSION),
            None
        );
        assert_eq!(resolve_update_target_version("  "), None);
        assert_eq!(
            resolve_update_target_version("version-id"),
            Some("version-id")
        );

        let manifest = manifest();
        let graph = DependencyGraph::from_manifest(&manifest);
        let suspect = SuspectedMod {
            id: "sodium".to_string(),
            name: "Sodium".to_string(),
            version: Some("0.5.8".to_string()),
            file_name: manifest.mods[0].file_name.clone(),
            known_in_manifest: true,
            confidence: 96,
            evidence: Vec::new(),
            authors: Vec::new(),
            blame_role: BlameRole::Related,
            match_sources: Vec::new(),
        };
        let plan = create_crash_fix_plan(&graph, &[], &[suspect], &[]);

        let update = plan
            .actions
            .iter()
            .find_map(|action| match action {
                ChangeAction::UpdateMod { target_version, .. } => Some(target_version),
                _ => None,
            })
            .expect("expected update action");
        assert_eq!(update, LATEST_COMPATIBLE_VERSION);
        assert_eq!(resolve_update_target_version(update), None);
    }

    #[test]
    fn suspected_mods_line_and_mod_file_become_primary() {
        let text = "\
---- Minecraft Crash Report ----
Suspected Mods: sodium
Mod File: /instance/mods/sodium-fabric-mc1.20.1-0.5.8.jar
Caused by: java.lang.IllegalStateException: boom
";
        let (_signals, suspects) =
            analyze_text_for_suspects(text, "crash-reports/latest.txt", &manifest());
        let sodium = suspects.iter().find(|s| s.id == "sodium").expect("sodium");
        assert!(sodium.confidence >= 90);
        assert!(
            sodium
                .match_sources
                .iter()
                .any(|s| s == "suspected_mods_line")
                || sodium.match_sources.iter().any(|s| s == "mod_file")
        );
        // Multi-signal → primary after merge/assign.
        assert_eq!(sodium.blame_role, BlameRole::Primary);
    }

    #[test]
    fn mod_spec_authors_serde_default() {
        let json = r#"{
            "id":"x","name":"X","source":{"type":"modrinth"},
            "version":"1","side":"both"
        }"#;
        let m: ModSpec = serde_json::from_str(json).unwrap();
        assert!(m.authors.is_empty());
    }

    #[test]
    fn summarizes_hs_err_oom() {
        let text = "# A fatal error has been detected by the Java Runtime Environment:\n# OutOfMemoryError\n# Problematic frame:\n# C  [jvm.dll+0x123]\n";
        let (kind, frame, _) = summarize_hs_err(text);
        assert_eq!(kind, "oom");
        assert!(frame.is_some());
    }

    #[test]
    fn extracts_world_coords_and_memory() {
        let text = "Entity Coordinates: 12.5, 64.0, -8.25\nMemory: 2048MB / 4096MB up to 8192MB\n";
        let c = extract_world_coords(text).expect("coords");
        assert!((c.x - 12.5).abs() < 0.01);
        assert_eq!(c.label, "Entity");
        let mem = extract_memory_hint(text).expect("mem");
        assert!(mem.contains("2048"));
    }

    fn multi_mod_manifest() -> ProjectManifest {
        let mut m = manifest();
        for (id, name, jar) in [
            ("fabric-api", "Fabric API", "fabric-api-0.92.0.jar"),
            ("create", "Create", "create-0.5.1.jar"),
            ("indium", "Indium", "indium-1.0.27.jar"),
            ("sodium", "Sodium", "sodium-0.5.8.jar"),
        ] {
            m.mods.push(ModSpec {
                id: id.to_string(),
                name: name.to_string(),
                source: ModSource {
                    kind: SourceKind::Modrinth,
                    project_id: Some(id.to_string()),
                    file_id: None,
                    url: None,
                    path: None,
                    icon_url: None,
                    categories: Vec::new(),
                },
                version: "1.0.0".to_string(),
                file_name: Some(jar.to_string()),
                hashes: None,
                side: Side::Both,
                dependencies: Vec::new(),
                status: Vec::new(),
                content_type: crate::manifest::ContentType::Mod,
                authors: Vec::new(),
                option: None,
            });
        }
        m
    }

    #[test]
    fn mod_list_dump_does_not_promote_every_mod_to_primary() {
        let manifest = multi_mod_manifest();
        let text = "\
---- Minecraft Crash Report ----
Description: create failed during tick

-- Mods --
| Mod id       | Version |
| fabric-api   | 0.92.0  |
| create       | 0.5.1   |
| indium       | 1.0.27  |
| sodium       | 0.5.8   |

java.lang.RuntimeException: Tick failed
Caused by: java.lang.IllegalStateException: create broke
\tat knot//com.simibubi.create.content.kinetics.base.KineticBlockEntity.tick(KineticBlockEntity.java:88)
";
        let (_signals, suspects) =
            analyze_text_for_suspects(text, "crash-reports/latest.txt", &manifest);
        let create = suspects.iter().find(|s| s.id == "create").expect("create");
        assert_eq!(create.blame_role, BlameRole::Primary);
        for id in ["fabric-api", "indium", "sodium"] {
            let s = suspects.iter().find(|x| x.id == id).expect(id);
            assert_ne!(
                s.blame_role,
                BlameRole::Primary,
                "{id} must not be primary from mod-list dump alone"
            );
        }
    }

    #[test]
    fn mixin_conflict_ranks_both_mods_secondary_not_random_primary() {
        let manifest = multi_mod_manifest();
        let text = "\
Mixin apply failed indium.mixins.json:MixinItemRenderer failed
Mixin apply failed sodium.mixins.json:MixinWorldRenderer failed
Caused by: org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError
";
        let (_signals, suspects) =
            analyze_text_for_suspects(text, "crash-reports/latest.txt", &manifest);
        let primaries: Vec<_> = suspects
            .iter()
            .filter(|s| s.blame_role == BlameRole::Primary)
            .collect();
        assert!(
            primaries.len() <= 1,
            "mixin conflict should not invent multiple primaries: {:?}",
            primaries.iter().map(|s| &s.id).collect::<Vec<_>>()
        );
        assert!(suspects
            .iter()
            .any(|s| s.id == "indium" || s.id == "sodium"));
    }

    #[test]
    fn description_beats_library_mod_in_stack() {
        let manifest = multi_mod_manifest();
        let text = "\
Description: create tick failure near contraption

java.lang.RuntimeException: wrapper
\tat knot//net.fabricmc.fabric.api.event.lifecycle.v1.ServerTickEvents.lambda$static$0(ServerTickEvents.java:12)
Caused by: java.lang.IllegalStateException: create entity stuck
\tat knot//com.simibubi.create.content.contraptions.Contraption.tick(Contraption.java:44)
";
        let (_signals, suspects) =
            analyze_text_for_suspects(text, "crash-reports/latest.txt", &manifest);
        let create = suspects.iter().find(|s| s.id == "create").expect("create");
        assert!(
            create.confidence >= 80,
            "create should rank above fabric-api library frame"
        );
        if let Some(fapi) = suspects.iter().find(|s| s.id == "fabric-api") {
            assert!(
                create.confidence > fapi.confidence,
                "content mod in Caused by should beat library API frame"
            );
        }
    }
}
