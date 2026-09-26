//! Shared helper functions used across multiple command modules.

use std::io::Write;
use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use std::sync::Mutex;
use tuffbox_core::{ProjectManifest, Snapshot, SnapshotStore};

use crate::pack_events;
use crate::types::LauncherDataState;

// ── Global locks ─────────────────────────────────────────────────

/// Serializes FTB Quests book / chat disk mutations across Tauri commands.
pub(crate) static QUEST_IO_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

// ── Path resolution ──────────────────────────────────────────────

pub(crate) fn find_manifest_in_project_dir(project_dir: &str) -> Result<PathBuf, String> {
    let dir = PathBuf::from(project_dir);
    let mut manifests = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.ends_with(".tuffbox.json"))
            .unwrap_or(false)
        {
            manifests.push(path);
        }
    }

    if manifests.is_empty() {
        return Err(format!(
            "project manifest not found in project directory: {}",
            dir.display()
        ));
    }

    if manifests.len() == 1 {
        return Ok(manifests.remove(0));
    }

    let state = load_launcher_data(&dir);
    if let Some(ref last_opened) = state.last_opened {
        let preferred = PathBuf::from(last_opened);
        if manifests.iter().any(|path| path == &preferred) {
            return Ok(preferred);
        }
    }

    let default = dir.join("project.tuffbox.json");
    if default.exists() {
        return Ok(default);
    }

    manifests.sort();
    Ok(manifests[0].clone())
}

/// Resolve a project directory or manifest path to the canonical `.tuffbox.json` file.
pub(crate) fn resolve_manifest_path(path: &str) -> Result<PathBuf, String> {
    let path_buf = PathBuf::from(path);

    if path_buf.is_dir() {
        return find_manifest_in_project_dir(path);
    }

    if path_buf.is_file() {
        return Ok(path_buf);
    }

    if let Some(parent) = path_buf.parent() {
        if parent.is_dir() {
            if let Ok(found) = find_manifest_in_project_dir(&parent.to_string_lossy()) {
                return Ok(found);
            }
        }
    }

    Err(format!(
        "project manifest not found: {}",
        path_buf.display()
    ))
}

pub(crate) fn manifest_parent(path: &str) -> Result<PathBuf, String> {
    let resolved = resolve_manifest_path(path)?;
    resolved
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "manifest has no parent directory".to_string())
}

// ── Snapshot helpers ─────────────────────────────────────────────

pub(crate) fn auto_snapshot(manifest_path: &Path, operation: &str) -> anyhow::Result<Snapshot> {
    auto_snapshot_detailed(manifest_path, operation, &[], &[])
}

pub(crate) fn auto_snapshot_with_changed_files(
    manifest_path: &Path,
    operation: &str,
    changed_files: &[PathBuf],
) -> anyhow::Result<Snapshot> {
    auto_snapshot_detailed(manifest_path, operation, changed_files, &[])
}

/// Safety snapshot before a mod install/remove/update. Does **not** write a History
/// journal entry — callers must call `finalize_mod_history` with human-readable lines
/// after the mutation (avoids logging raw Modrinth/CF ids like `aC3cM3Vq`).
pub(crate) fn auto_snapshot_before_mod_op(
    manifest_path: &Path,
    operation: &str,
) -> anyhow::Result<Snapshot> {
    auto_snapshot_detailed_ex(manifest_path, operation, &[], &[], false, Vec::new())
}

pub(crate) fn auto_snapshot_detailed(
    manifest_path: &Path,
    operation: &str,
    changed_files: &[PathBuf],
    actions_summary: &[String],
) -> anyhow::Result<Snapshot> {
    auto_snapshot_detailed_ex(manifest_path, operation, changed_files, actions_summary, true, Vec::new())
}

pub(crate) fn auto_snapshot_with_managed(
    manifest_path: &Path,
    operation: &str,
    changed_files: &[PathBuf],
    actions_summary: &[String],
    managed_files: Vec<PathBuf>,
) -> anyhow::Result<Snapshot> {
    auto_snapshot_detailed_ex(
        manifest_path,
        operation,
        changed_files,
        actions_summary,
        true,
        managed_files,
    )
}

fn auto_snapshot_detailed_ex(
    manifest_path: &Path,
    operation: &str,
    changed_files: &[PathBuf],
    actions_summary: &[String],
    journal: bool,
    managed_files: Vec<PathBuf>,
) -> anyhow::Result<Snapshot> {
    let project_dir = manifest_path.parent().ok_or_else(|| {
        anyhow::anyhow!("manifest path has no parent: {}", manifest_path.display())
    })?;
    let lockfile_path = manifest_path.with_extension("lock.json");
    let lockfile_path = if lockfile_path.exists() {
        Some(lockfile_path)
    } else {
        None
    };
    let store = SnapshotStore::new(project_dir);
    let name = format!("auto-before-{operation}");
    let reason = format!("Auto snapshot before {operation}");
    let summary: Vec<String> = if actions_summary.is_empty() {
        vec![format!("Safety point before {operation}")]
    } else {
        actions_summary.to_vec()
    };
    let actor = pack_events::actor_for_operation(operation).to_string();
    let meta = tuffbox_core::SnapshotMeta {
        operation: operation.to_string(),
        actions_summary: summary,
        actor: Some(actor),
        managed_files,
        ..Default::default()
    };
    let snapshot = store.create_with_meta(
        &name,
        &reason,
        manifest_path,
        lockfile_path.as_ref(),
        changed_files,
        meta,
    )?;
    if journal {
        let _ = pack_events::append_from_snapshot_with_summary(
            project_dir,
            operation,
            &snapshot.id,
            changed_files,
            &reason,
            &snapshot.actions_summary,
        );
    }
    Ok(snapshot)
}

pub(crate) fn save_manifest(path: &Path, manifest: &ProjectManifest) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(manifest)?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("manifest path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)?;
    let mut staged = tempfile::Builder::new()
        .prefix(".tuffbox-manifest-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    staged.write_all(json.as_bytes())?;
    staged.flush()?;
    staged.as_file().sync_all()?;
    staged
        .persist(path)
        .map_err(|error| anyhow::Error::new(error.error))?;
    persist_lockfile_for_manifest(path, manifest)?;
    Ok(())
}

pub(crate) fn lockfile_path_for_manifest(manifest_path: &Path) -> PathBuf {
    manifest_path.with_extension("lock.json")
}

pub(crate) fn save_lockfile(
    path: &Path,
    lockfile: &tuffbox_core::TuffboxLockfile,
) -> anyhow::Result<()> {
    lockfile.save_to_path(path)?;
    Ok(())
}

pub(crate) fn persist_lockfile_for_manifest(
    manifest_path: &Path,
    manifest: &ProjectManifest,
) -> anyhow::Result<tuffbox_core::TuffboxLockfile> {
    let graph = tuffbox_core::DependencyGraph::from_manifest(manifest);
    let lockfile = tuffbox_core::TuffboxLockfile::from_manifest_and_graph(manifest, &graph);
    save_lockfile(&lockfile_path_for_manifest(manifest_path), &lockfile)?;
    Ok(lockfile)
}

// ── File utilities ───────────────────────────────────────────────

pub(crate) fn safe_project_file(project_dir: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let relative = validate_relative_snapshot_path(relative_path)?;
    let target = project_dir.join(&relative);
    if !target.starts_with(project_dir) {
        return Err("path escapes project directory".to_string());
    }
    Ok(target)
}

pub(crate) fn is_editable_config_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase()
            .as_str(),
        "json"
            | "json5"
            | "toml"
            | "properties"
            | "cfg"
            | "conf"
            | "txt"
            | "js"
            | "zs"
            | "yaml"
            | "yml"
            | "md"
    )
}

pub(crate) fn validate_relative_snapshot_path(relative_path: &str) -> Result<PathBuf, String> {
    let relative = PathBuf::from(relative_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("invalid snapshot-relative path".to_string());
    }
    Ok(relative)
}

pub(crate) fn read_small_text_file(path: &Path) -> Result<String, String> {
    if !path.is_file() {
        return Ok(String::new());
    }
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.len() > 512 * 1024 {
        return Ok(format!(
            "# File is too large for inline diff: {} bytes\n",
            metadata.len()
        ));
    }
    std::fs::read_to_string(path)
        .map_err(|_| "# Binary or non-UTF8 file; inline diff unavailable.\n".to_string())
}

pub(crate) fn unified_text_diff(before: &str, after: &str) -> String {
    if before == after {
        return "No content changes.".to_string();
    }
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    let mut table = vec![vec![0usize; after_lines.len() + 1]; before_lines.len() + 1];
    for i in (0..before_lines.len()).rev() {
        for j in (0..after_lines.len()).rev() {
            table[i][j] = if before_lines[i] == after_lines[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }

    let mut out = String::new();
    let mut i = 0;
    let mut j = 0;
    while i < before_lines.len() && j < after_lines.len() {
        if before_lines[i] == after_lines[j] {
            out.push_str("  ");
            out.push_str(before_lines[i]);
            out.push('\n');
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            out.push_str("- ");
            out.push_str(before_lines[i]);
            out.push('\n');
            i += 1;
        } else {
            out.push_str("+ ");
            out.push_str(after_lines[j]);
            out.push('\n');
            j += 1;
        }
    }
    while i < before_lines.len() {
        out.push_str("- ");
        out.push_str(before_lines[i]);
        out.push('\n');
        i += 1;
    }
    while j < after_lines.len() {
        out.push_str("+ ");
        out.push_str(after_lines[j]);
        out.push('\n');
        j += 1;
    }
    out
}

pub(crate) fn copy_dir_recursive(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            // copy_replacing: an in-place fs::copy would mutate any
            // dedup-store hardlink already sitting at `target`.
            tuffbox_core::fs_util::copy_replacing(&entry.path(), &target)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ── Launcher data persistence ────────────────────────────────────

pub(crate) fn launcher_data_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join("launcher-data.json")
}

pub(crate) fn load_launcher_data(project_dir: &Path) -> LauncherDataState {
    let path = launcher_data_path(project_dir);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn save_launcher_data(
    project_dir: &Path,
    state: &LauncherDataState,
) -> Result<(), String> {
    let path = launcher_data_path(project_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

fn recent_projects_path() -> PathBuf {
    dirs::config_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("recent_projects.json")
}

pub(crate) fn load_recent_projects() -> Vec<crate::types::RecentProjectEntry> {
    let path = recent_projects_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn save_recent_projects(
    projects: &[crate::types::RecentProjectEntry],
) -> Result<(), String> {
    let path = recent_projects_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Preserve materialized home-cache fields when the frontend saves path+info only.
    let existing = load_recent_projects();
    let by_path: std::collections::HashMap<&str, &crate::types::RecentProjectEntry> = existing
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect();
    let merged: Vec<crate::types::RecentProjectEntry> = projects
        .iter()
        .map(|p| {
            let mut next = p.clone();
            if let Some(prev) = by_path.get(p.path.as_str()) {
                if next.icon_data_url.is_none() {
                    next.icon_data_url = prev.icon_data_url.clone();
                }
                if next.size_label.is_none() {
                    next.size_label = prev.size_label.clone();
                    next.size_bytes = prev.size_bytes;
                    next.size_fingerprint = prev.size_fingerprint.clone();
                }
                if next.stats_playtime_seconds.is_none() {
                    next.stats_playtime_seconds = prev.stats_playtime_seconds;
                    next.stats_last_launch = prev.stats_last_launch.clone();
                }
            }
            next
        })
        .collect();
    let json = serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

/// Clear size / icon / stats cache for one recent project (mods changed, launch, …).
pub(crate) fn invalidate_recent_home_cache(manifest_path: &str) {
    let mut projects = load_recent_projects();
    let mut changed = false;
    for entry in &mut projects {
        if entry.path == manifest_path
            || entry.path.replace('\\', "/") == manifest_path.replace('\\', "/")
        {
            entry.icon_data_url = None;
            entry.size_label = None;
            entry.size_bytes = None;
            entry.size_fingerprint = None;
            entry.stats_playtime_seconds = None;
            entry.stats_last_launch = None;
            changed = true;
        }
    }
    if changed {
        let _ = save_recent_projects_raw(&projects);
    }
}

/// Patch cache fields for a path without reordering the recent list.
pub(crate) fn patch_recent_home_cache(
    manifest_path: &str,
    patch: RecentHomeCachePatch,
) -> Result<(), String> {
    let mut projects = load_recent_projects();
    let mut found = false;
    for entry in &mut projects {
        if entry.path == manifest_path
            || entry.path.replace('\\', "/") == manifest_path.replace('\\', "/")
        {
            if let Some(v) = patch.icon_data_url {
                entry.icon_data_url = v;
            }
            if let Some(v) = patch.size_label {
                entry.size_label = v;
            }
            if let Some(v) = patch.size_bytes {
                entry.size_bytes = v;
            }
            if let Some(v) = patch.size_fingerprint {
                entry.size_fingerprint = v;
            }
            if let Some(v) = patch.stats_playtime_seconds {
                entry.stats_playtime_seconds = v;
            }
            if let Some(v) = patch.stats_last_launch {
                entry.stats_last_launch = v;
            }
            found = true;
            break;
        }
    }
    if found {
        save_recent_projects_raw(&projects)?;
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct RecentHomeCachePatch {
    pub icon_data_url: Option<Option<String>>,
    pub size_label: Option<Option<String>>,
    pub size_bytes: Option<Option<u64>>,
    pub size_fingerprint: Option<Option<String>>,
    pub stats_playtime_seconds: Option<Option<u64>>,
    pub stats_last_launch: Option<Option<String>>,
}

fn save_recent_projects_raw(
    projects: &[crate::types::RecentProjectEntry],
) -> Result<(), String> {
    let path = recent_projects_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(projects).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

/// Recursive byte size of common instance subfolders (mods, config, …).
pub(crate) fn compute_instance_size_bytes(project_dir: &Path) -> u64 {
    let mut total: u64 = 0;
    fn walk(dir: &Path, total: &mut u64) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, total);
            } else if let Ok(meta) = p.metadata() {
                *total += meta.len();
            }
        }
    }
    for sub in &[
        "mods",
        "config",
        "resourcepacks",
        "shaderpacks",
        "datapacks",
        "scripts",
        "logs",
    ] {
        walk(&project_dir.join(sub), &mut total);
    }
    total
}

pub(crate) fn format_byte_size(total: u64) -> String {
    if total < 1024 {
        format!("{} B", total)
    } else if total < 1024 * 1024 {
        format!("{:.1} KB", total as f64 / 1024.0)
    } else if total < 1024 * 1024 * 1024 {
        format!("{:.1} MB", total as f64 / 1024.0 / 1024.0)
    } else {
        format!("{:.1} GB", total as f64 / 1024.0 / 1024.0 / 1024.0)
    }
}

/// Cheap fingerprint from mods + config directory mtimes (invalidates size cache).
pub(crate) fn instance_size_fingerprint(project_dir: &Path) -> String {
    let mut parts = Vec::new();
    for sub in &["mods", "config"] {
        let p = project_dir.join(sub);
        let stamp = std::fs::metadata(&p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        parts.push(format!("{sub}:{stamp}"));
    }
    parts.join("|")
}

// ── Stats persistence ────────────────────────────────────────────

pub(crate) fn stats_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join("stats.json")
}

pub(crate) fn load_stats(project_dir: &Path) -> crate::types::ProjectStats {
    let p = stats_path(project_dir);
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn save_stats(
    project_dir: &Path,
    stats: &crate::types::ProjectStats,
) -> Result<(), String> {
    let p = stats_path(project_dir);
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &p,
        serde_json::to_string_pretty(stats).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

// ── Backup helpers ───────────────────────────────────────────────

pub(crate) fn backup_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join("backups")
}

pub(crate) fn load_backup_index(project_dir: &Path) -> crate::types::BackupIndex {
    let p = backup_dir(project_dir).join("index.json");
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(crate::types::BackupIndex {
            backups: vec![],
            max_count: 20,
        })
}

pub(crate) fn save_backup_index(
    project_dir: &Path,
    idx: &crate::types::BackupIndex,
) -> Result<(), String> {
    let d = backup_dir(project_dir);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    std::fs::write(
        d.join("index.json"),
        serde_json::to_string_pretty(idx).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

// ── Shell / string helpers ───────────────────────────────────────

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn shell_escape(s: &str) -> String {
    if s.chars().all(|c| c.is_alphanumeric() || "-_.:/".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

pub(crate) fn slugify_project_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else if c == ' ' || c == '_' || c == '-' {
                '-'
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
