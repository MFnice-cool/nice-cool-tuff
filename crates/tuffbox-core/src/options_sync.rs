//! Shared options.txt sync across projects, grouped by Minecraft version
//! (docs/17-dedup-and-options-sync.md, Part 2).
//!
//! Model:
//! - One shared template per MC version group lives in
//!   `<data_local>/TuffBox/options-profiles/<group-id>/options.txt`.
//! - A project opts in via a stamp file `.tuffbox-options-managed` in the
//!   project dir (game dir). No stamp ⇒ the project's options.txt is fully
//!   user-owned and NEVER touched by sync.
//! - A managed project pulls the shared template before launch (3-way merge
//!   so player edits always win) and pushes local edits back after the game
//!   exits (write-back), so the shared file "learns" from any managed project.
//!
//! Safety rules (hard requirements):
//! 1. Never overwrite an options.txt that has no stamp (could be the player's
//!    only copy of their settings).
//! 2. Any managed update first backs up the current local file
//!    (`options.txt.bak-<unix_ts>`), keeping the last 3 backups.
//! 3. All writes go through temp-file + rename (atomic on the same volume).

use crate::properties_parser::PropertiesFile;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Marker file in the game/project dir: presence = TuffBox manages this
/// project's options.txt (sync + write-back enabled).
pub const STAMP_FILE: &str = ".tuffbox-options-managed";

const PROFILES_DIR_NAME: &str = "options-profiles";
const LAST_SYNCED_FILE: &str = ".tuffbox-options-last-synced";
const MAX_BACKUPS: usize = 3;

/// Root of the shared options profiles:
/// `<local data>/TuffBox/options-profiles`. The `TUFFBOX_OPTIONS_PROFILES_ROOT`
/// env var overrides it (used by tests for isolation; ignored in production).
pub fn profiles_root() -> PathBuf {
    if let Ok(root) = std::env::var("TUFFBOX_OPTIONS_PROFILES_ROOT") {
        if !root.is_empty() {
            return PathBuf::from(root);
        }
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join(PROFILES_DIR_NAME)
}

/// Shared template path for a version group.
fn group_options_path(group_id: &str) -> PathBuf {
    profiles_root().join(group_id).join("options.txt")
}

/// Shared template path for a version group. Public for diagnostics/UI.
pub fn group_template_path(group_id: &str) -> PathBuf {
    group_options_path(group_id)
}

/// Last-synced snapshot for a project (3-way merge base).
fn last_synced_path(project_dir: &Path) -> PathBuf {
    project_dir.join(LAST_SYNCED_FILE)
}

pub fn stamp_path(project_dir: &Path) -> PathBuf {
    project_dir.join(STAMP_FILE)
}

/// True when this project is under options sync management.
pub fn is_managed(project_dir: &Path) -> bool {
    stamp_path(project_dir).is_file()
}

/// Enable management for a project. The current local options.txt (if any)
/// becomes the group's starting template when the group has none yet.
/// Returns Ok(true) when the local file was imported into the group.
pub fn enable_management(project_dir: &Path, mc_version: &str) -> std::io::Result<bool> {
    let group = version_group(mc_version);
    let shared_path = group_options_path(&group);
    let local = project_dir.join("options.txt");
    let mut imported = false;
    if !shared_path.is_file() && local.is_file() {
        write_shared(&group, &fs::read_to_string(&local).unwrap_or_default())?;
        imported = true;
    }
    // Writing the stamp is the last step: a crash before it leaves the
    // project untouched (fail-safe direction — sync stays off).
    let stamp = stamp_path(project_dir);
    if let Some(parent) = stamp.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&stamp, group.as_bytes())?;
    // Prime the merge base so the first sync doesn't treat every existing
    // local key as a player edit.
    if local.is_file() {
        let _ = fs::copy(&local, last_synced_path(project_dir));
    }
    Ok(imported)
}

/// Disable management (independent options). Local file is left as-is;
/// the merge-base snapshot is removed so a later re-enable starts fresh.
pub fn disable_management(project_dir: &Path) {
    let _ = fs::remove_file(stamp_path(project_dir));
    let _ = fs::remove_file(last_synced_path(project_dir));
}

// ---------------------------------------------------------------------------
// Version groups
// ---------------------------------------------------------------------------

/// Minecraft version group: one shared options.txt per group. Options.txt
/// format differs across major versions (1.13 removed/notch-key changes,
/// 1.19.3+ chat settings, 1.20.5+), so the granularity is the minor version —
/// every 1.<minor>.x shares one template, matching `data_epoch` style rules.
pub fn version_group(mc_version: &str) -> String {
    // Parse leading "1.<minor>" (tolerates prefixes like "1.21.4-fabric",
    // snapshots "26w03a" fall through to the raw-id group).
    let parts: Vec<&str> = mc_version.split('.').collect();
    if parts.len() >= 2 && parts[0] == "1" {
        if let Ok(minor) = parts[1].parse::<u32>() {
            return format!("mc-1.{minor}");
        }
    }
    // Snapshot / exotic id: dedicated group so nothing mismatches.
    format!("mc-other-{}", sanitize(mc_version))
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// All groups that currently have a shared template.
pub fn list_groups() -> Vec<GroupInfo> {
    let mut groups = Vec::new();
    let Ok(entries) = fs::read_dir(profiles_root()) else {
        return groups;
    };
    for entry in entries.flatten() {
        let path = entry.path().join("options.txt");
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let modified = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        groups.push(GroupInfo {
            id: name,
            size_bytes: size,
            modified_unix: modified,
        });
    }
    groups.sort_by(|a, b| a.id.cmp(&b.id));
    groups
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInfo {
    pub id: String,
    pub size_bytes: u64,
    pub modified_unix: Option<u64>,
}

// ---------------------------------------------------------------------------
// Sync (before launch) and write-back (after exit)
// ---------------------------------------------------------------------------

/// Outcome of a pre-launch sync, for logging in the console.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SyncOutcome {
    /// No local file, group template copied into the project.
    SeededFromGroup,
    /// Local file already matches the merge result — nothing written.
    #[default]
    UpToDate,
    /// Merge produced changes; local file updated (backup created).
    Merged,
    /// Project is not managed — nothing was touched.
    SkippedUnmanaged,
    /// No local file and the group has no template yet — the game will
    /// create a fresh one; write-back will seed the group on first exit.
    NothingToSync,
    /// A filesystem error occurred; options are untouched (fail-safe).
    Error(String),
}

/// Pre-launch sync: bring the managed project's options.txt in line with the
/// group template via a 3-way merge (player edits always win). Never throws:
/// any error means "leave the file alone".
pub fn sync_before_launch(project_dir: &Path, mc_version: &str) -> SyncOutcome {
    if !is_managed(project_dir) {
        return SyncOutcome::SkippedUnmanaged;
    }
    let local = project_dir.join("options.txt");
    let group = version_group(mc_version);
    let shared_path = group_options_path(&group);

    let local_content = match fs::read_to_string(&local) {
        Ok(c) => Some(c),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return SyncOutcome::Error(e.to_string()),
    };

    let shared_content = match fs::read_to_string(&shared_path) {
        Ok(c) => Some(c),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return SyncOutcome::Error(e.to_string()),
    };

    match (local_content, shared_content) {
        (None, None) => SyncOutcome::NothingToSync,
        (None, Some(shared)) => {
            // Fresh project: copy the template atomically.
            match atomic_write(&local, &shared) {
                Ok(()) => {
                    let _ = fs::write(last_synced_path(project_dir), shared);
                    SyncOutcome::SeededFromGroup
                }
                Err(e) => SyncOutcome::Error(e.to_string()),
            }
        }
        (Some(local), None) => {
            // Group template gone (user deleted it): promote local to group.
            match write_shared(&group, &local) {
                Ok(()) => {
                    let _ = fs::write(last_synced_path(project_dir), &local);
                    SyncOutcome::UpToDate
                }
                Err(e) => SyncOutcome::Error(e.to_string()),
            }
        }
        (Some(local_txt), Some(shared_txt)) => {
            if sha1_hex(local_txt.as_bytes()) == sha1_hex(shared_txt.as_bytes()) {
                return SyncOutcome::UpToDate;
            }
            let base = fs::read_to_string(last_synced_path(project_dir)).unwrap_or_default();
            let merged = merge_three_way(&base, &shared_txt, &local_txt);
            if merged == local_txt {
                // Nothing new from the group; just refresh the merge base.
                let _ = fs::write(last_synced_path(project_dir), &shared_txt);
                return SyncOutcome::UpToDate;
            }
            // Back up the current local file before the managed update.
            if let Err(e) = backup_local(project_dir, &local) {
                return SyncOutcome::Error(e.to_string());
            }
            match atomic_write(&local, &merged) {
                Ok(()) => {
                    // Merge base becomes the group template: next time,
                    // "unchanged since sync" compares against what we pulled.
                    let _ = fs::write(last_synced_path(project_dir), &shared_txt);
                    SyncOutcome::Merged
                }
                Err(e) => SyncOutcome::Error(e.to_string()),
            }
        }
    }
}

/// Post-exit write-back: if the project is managed, push local changes (the
/// player's in-game settings) into the group template. Only keys the player
/// actually changed (vs the merge base) flow back — never a blind overwrite.
pub fn write_back_after_exit(project_dir: &Path, mc_version: &str) {
    if !is_managed(project_dir) {
        return;
    }
    let local = project_dir.join("options.txt");
    let Ok(local_content) = fs::read_to_string(&local) else {
        return; // game never created options.txt (e.g. crashed instantly)
    };
    let base = fs::read_to_string(last_synced_path(project_dir)).unwrap_or_default();
    let group = version_group(mc_version);
    let shared_path = group_options_path(&group);
    let shared_content = fs::read_to_string(&shared_path).unwrap_or_default();

    let merged_back = merge_three_way(&base, &local_content, &shared_content);
    if merged_back == shared_content {
        // No player-side edits; keep the template as-is.
        let _ = fs::write(last_synced_path(project_dir), &local_content);
        return;
    }
    if write_shared(&group, &merged_back).is_ok() {
        let _ = fs::write(last_synced_path(project_dir), &local_content);
    }
}

/// Manually push the project's current options.txt into its version group
/// (UI: "Push current options to group"). Blind by design — the user asked
/// for it — but still merged against the base so concurrent group edits from
/// other projects are preserved.
pub fn push_to_group(project_dir: &Path, mc_version: &str) -> Result<(), String> {
    if !is_managed(project_dir) {
        return Err("Project is not using shared options (independent mode)".into());
    }
    let local = project_dir.join("options.txt");
    let content = fs::read_to_string(&local).map_err(|e| e.to_string())?;
    let base = fs::read_to_string(last_synced_path(project_dir)).unwrap_or_default();
    let group = version_group(mc_version);
    let shared_content = fs::read_to_string(group_options_path(&group)).unwrap_or_default();
    let merged = merge_three_way(&base, &content, &shared_content);
    write_shared(&group, &merged).map_err(|e| e.to_string())?;
    let _ = fs::write(last_synced_path(project_dir), &content);
    Ok(())
}

/// Read the effective shared template for a version group, if any.
pub fn read_group_template(mc_version: &str) -> Option<String> {
    fs::read_to_string(group_options_path(&version_group(mc_version))).ok()
}

// ---------------------------------------------------------------------------
// 3-way merge
// ---------------------------------------------------------------------------

/// Three-way merge of .properties files.
/// - base: content at the last sync (or empty when unknown).
/// - theirs / ours: the two diverged variants.
/// Rules, applied per key:
/// - unchanged in `ours`, changed in `theirs` → take theirs;
/// - changed in `ours` (even if `theirs` also changed) → keep ours
///   (player edits win);
/// - only in `theirs` → add (new settings propagate through the group);
/// - only in `ours` → keep (player added a key).
///
/// Output preserves `ours` entry order first (player's file layout wins),
/// then any additions from `theirs` in their original order.
pub fn merge_three_way(base: &str, ours: &str, theirs: &str) -> String {
    let base_map: HashMap<String, String> = PropertiesFile::parse(base).to_map();
    let ours_file = PropertiesFile::parse(ours);
    let theirs_map: HashMap<String, String> = PropertiesFile::parse(theirs).to_map();

    let mut out = ours_file.clone();
    for entry in &mut out.entries {
        let base_val = base_map.get(&entry.key).map(|s| s.as_str());
        let theirs_val = theirs_map.get(&entry.key).map(|s| s.as_str());
        let ours_unchanged = base_val == Some(entry.value.as_str());
        let theirs_changed = theirs_val.is_some() && theirs_val != base_val;
        // Player edit always wins; only pull the group value when the
        // player hasn't touched this key since the last sync.
        if ours_unchanged && theirs_changed && theirs_val != Some(entry.value.as_str()) {
            if let Some(v) = theirs_val {
                entry.value = v.to_string();
            }
        }
    }
    // Add keys that exist only in `theirs` (new in the group since sync).
    let ours_keys: std::collections::HashSet<String> =
        out.entries.iter().map(|e| e.key.clone()).collect();
    for (key, val) in theirs_map.iter() {
        if !ours_keys.contains(key) {
            out.entries.push(crate::properties_parser::PropertyEntry {
                key: key.to_string(),
                value: val.to_string(),
                comment_before: None,
            });
        }
    }
    out.to_string()
}

// ---------------------------------------------------------------------------
// File helpers
// ---------------------------------------------------------------------------

fn sha1_hex(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Atomic write: temp file in the same dir + rename over the target.
fn atomic_write(target: &Path, content: &str) -> std::io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent dir"))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".tuffbox-options-tmp-{}", std::process::id()));
    fs::write(&tmp, content)?;
    match fs::rename(&tmp, target) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Shared template write: atomic + fsync-free (same process lifetime).
fn write_shared(group: &str, content: &str) -> std::io::Result<()> {
    atomic_write(&group_options_path(group), content)
}

/// Copy `local` to `options.txt.bak-<unix_ts>`, keep the newest
/// [`MAX_BACKUPS`] backups.
fn backup_local(project_dir: &Path, local: &Path) -> std::io::Result<()> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    fs::copy(local, project_dir.join(format!("options.txt.bak-{ts}")))?;
    prune_backups(project_dir);
    Ok(())
}

fn prune_backups(project_dir: &Path) {
    let mut backups: Vec<(u64, PathBuf)> = Vec::new();
    if let Ok(entries) = fs::read_dir(project_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(ts) = name
                .strip_prefix("options.txt.bak-")
                .and_then(|s| s.parse::<u64>().ok())
            {
                backups.push((ts, entry.path()));
            }
        }
    }
    backups.sort_by(|a, b| b.0.cmp(&a.0)); // newest first
    for (_, path) in backups.into_iter().skip(MAX_BACKUPS) {
        let _ = fs::remove_file(path);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct TempProject(tempfile::TempDir);

    impl TempProject {
        fn new(content: Option<&str>) -> Self {
            let dir = tempfile::tempdir().unwrap();
            if let Some(c) = content {
                fs::write(dir.path().join("options.txt"), c).unwrap();
            }
            Self(dir)
        }
        fn path(&self) -> &Path {
            self.0.path()
        }
    }

    /// Point the store at a temp root for the duration of a test.
    /// Uses an env override via a test-only indirection: `profiles_root`
    /// reads `dirs::data_local_dir`, so tests that need isolation write
    /// through the public API against the real root is NOT acceptable —
    /// instead we swap the root with a thread-local.
    // NOTE: real isolation below via `with_test_root`.
    fn with_test_root<F: FnOnce(&Path)>(f: F) {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Redirect the root through a process-global test path.
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("TUFFBOX_OPTIONS_PROFILES_ROOT", dir.path());
        f(dir.path());
        std::env::remove_var("TUFFBOX_OPTIONS_PROFILES_ROOT");
    }

    #[test]
    fn version_group_by_minor() {
        assert_eq!(version_group("1.21.4"), "mc-1.21");
        assert_eq!(version_group("1.21"), "mc-1.21");
        assert_eq!(version_group("1.20.1"), "mc-1.20");
        assert_eq!(version_group("1.12.2"), "mc-1.12");
        assert_eq!(version_group("26w03a").starts_with("mc-other-"), true);
    }

    #[test]
    fn merge_keeps_player_edits_pulls_group_changes() {
        let base = "gamma=1.0\nrenderDistance=12\n";
        // Player changed gamma; group changed renderDistance + added a key.
        let ours = "gamma=2.5\nrenderDistance=12\n";
        let theirs = "gamma=1.0\nrenderDistance=16\nmusicVolume=0.3\n";
        let merged = merge_three_way(base, ours, theirs);
        let m = PropertiesFile::parse(&merged);
        assert_eq!(m.get("gamma"), Some("2.5"), "player edit must win");
        assert_eq!(m.get("renderDistance"), Some("16"), "group edit propagates");
        assert_eq!(m.get("musicVolume"), Some("0.3"), "new group keys are added");
    }

    #[test]
    fn merge_player_edit_beats_conflicting_group_edit() {
        let base = "gamma=1.0\n";
        let ours = "gamma=2.0\n"; // player set 2.0
        let theirs = "gamma=3.0\n"; // group set 3.0
        let m = PropertiesFile::parse(&merge_three_way(base, ours, theirs));
        assert_eq!(m.get("gamma"), Some("2.0"));
    }

    #[test]
    fn merge_keeps_player_added_keys() {
        let base = "";
        let ours = "customKey=42\n";
        let theirs = "gamma=1.0\n";
        let m = PropertiesFile::parse(&merge_three_way(base, ours, theirs));
        assert_eq!(m.get("customKey"), Some("42"));
        assert_eq!(m.get("gamma"), Some("1.0"));
    }

    #[test]
    fn sync_seeds_fresh_project_from_group() {
        with_test_root(|root| {
            let proj = TempProject::new(None);
            let group_dir = root.join("mc-1.21");
            fs::create_dir_all(&group_dir).unwrap();
            fs::write(group_dir.join("options.txt"), "gamma=1.0\n").unwrap();
            enable_management(proj.path(), "1.21.4").unwrap();
            let outcome = sync_before_launch(proj.path(), "1.21.4");
            assert_eq!(outcome, SyncOutcome::SeededFromGroup);
            assert_eq!(
                fs::read_to_string(proj.path().join("options.txt")).unwrap(),
                "gamma=1.0\n"
            );
        });
    }

    #[test]
    fn sync_never_touches_unmanaged_project() {
        with_test_root(|root| {
            let proj = TempProject::new(Some("gamma=9.9\n"));
            let group_dir = root.join("mc-1.21");
            fs::create_dir_all(&group_dir).unwrap();
            fs::write(group_dir.join("options.txt"), "gamma=1.0\n").unwrap();
            let outcome = sync_before_launch(proj.path(), "1.21.4");
            assert_eq!(outcome, SyncOutcome::SkippedUnmanaged);
            assert_eq!(
                fs::read_to_string(proj.path().join("options.txt")).unwrap(),
                "gamma=9.9\n"
            );
        });
    }

    #[test]
    fn managed_update_creates_backup_and_merges() {
        with_test_root(|root| {
            let proj = TempProject::new(Some("gamma=1.0\nrenderDistance=12\n"));
            let group_dir = root.join("mc-1.21");
            fs::create_dir_all(&group_dir).unwrap();
            fs::write(group_dir.join("options.txt"), "gamma=1.0\nrenderDistance=16\n")
                .unwrap();
            enable_management(proj.path(), "1.21.4").unwrap();
            // Simulate the player having changed gamma after enable (base is
            // the enable-time snapshot).
            fs::write(proj.path().join("options.txt"), "gamma=2.5\nrenderDistance=12\n")
                .unwrap();
            let outcome = sync_before_launch(proj.path(), "1.21.4");
            assert_eq!(outcome, SyncOutcome::Merged);
            let m = PropertiesFile::parse(
                &fs::read_to_string(proj.path().join("options.txt")).unwrap(),
            );
            assert_eq!(m.get("gamma"), Some("2.5"));
            assert_eq!(m.get("renderDistance"), Some("16"));
            // Backup exists.
            let has_backup = fs::read_dir(proj.path())
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("options.txt.bak-"));
            assert!(has_backup);
        });
    }

    #[test]
    fn identical_files_are_up_to_date_no_rewrite() {
        with_test_root(|root| {
            let content = "gamma=1.0\n";
            let proj = TempProject::new(Some(content));
            let group_dir = root.join("mc-1.21");
            fs::create_dir_all(&group_dir).unwrap();
            fs::write(group_dir.join("options.txt"), content).unwrap();
            enable_management(proj.path(), "1.21.4").unwrap();
            assert_eq!(sync_before_launch(proj.path(), "1.21.4"), SyncOutcome::UpToDate);
        });
    }

    #[test]
    fn write_back_pushes_player_edits_into_group() {
        with_test_root(|root| {
            let content = "gamma=1.0\n";
            let proj = TempProject::new(Some(content));
            let group_dir = root.join("mc-1.21");
            fs::create_dir_all(&group_dir).unwrap();
            fs::write(group_dir.join("options.txt"), content).unwrap();
            enable_management(proj.path(), "1.21.4").unwrap();
            // Player changed settings in-game; the game rewrote options.txt.
            fs::write(proj.path().join("options.txt"), "gamma=2.5\n").unwrap();
            write_back_after_exit(proj.path(), "1.21.4");
            assert_eq!(
                fs::read_to_string(group_dir.join("options.txt")).unwrap(),
                "gamma=2.5\n"
            );
            // Write-back again with no further local edits → no change.
            write_back_after_exit(proj.path(), "1.21.4");
            assert_eq!(
                fs::read_to_string(group_dir.join("options.txt")).unwrap(),
                "gamma=2.5\n"
            );
        });
    }

    #[test]
    fn write_back_skips_unmanaged() {
        with_test_root(|root| {
            let proj = TempProject::new(Some("gamma=9.9\n"));
            let group_dir = root.join("mc-1.21");
            fs::create_dir_all(&group_dir).unwrap();
            fs::write(group_dir.join("options.txt"), "gamma=1.0\n").unwrap();
            write_back_after_exit(proj.path(), "1.21.4");
            assert_eq!(
                fs::read_to_string(group_dir.join("options.txt")).unwrap(),
                "gamma=1.0\n"
            );
        });
    }

    #[test]
    fn disable_makes_project_fully_independent() {
        with_test_root(|root| {
            let proj = TempProject::new(Some("gamma=1.0\n"));
            let group_dir = root.join("mc-1.21");
            fs::create_dir_all(&group_dir).unwrap();
            fs::write(group_dir.join("options.txt"), "gamma=1.0\n").unwrap();
            enable_management(proj.path(), "1.21.4").unwrap();
            disable_management(proj.path());
            assert!(!is_managed(proj.path()));
            fs::write(proj.path().join("options.txt"), "gamma=7.7\n").unwrap();
            assert_eq!(sync_before_launch(proj.path(), "1.21.4"), SyncOutcome::SkippedUnmanaged);
            write_back_after_exit(proj.path(), "1.21.4");
            assert_eq!(
                fs::read_to_string(group_dir.join("options.txt")).unwrap(),
                "gamma=1.0\n"
            );
        });
    }

    #[test]
    fn backups_pruned_to_max() {
        with_test_root(|root| {
            let proj = TempProject::new(Some("a=1\n"));
            let group_dir = root.join("mc-1.21");
            fs::create_dir_all(&group_dir).unwrap();
            fs::write(group_dir.join("options.txt"), "a=2\n").unwrap();
            enable_management(proj.path(), "1.21.4").unwrap();
            // Force several distinct merges to generate backups.
            for i in 0..5 {
                fs::write(group_dir.join("options.txt"), format!("a={}\n", i + 2)).unwrap();
                let _ = sync_before_launch(proj.path(), "1.21.4");
            }
            let count = fs::read_dir(proj.path())
                .unwrap()
                .flatten()
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("options.txt.bak-")
                })
                .count();
            assert!(count <= MAX_BACKUPS, "found {count} backups");
        });
    }

    #[test]
    fn push_to_group_requires_management() {
        let proj = TempProject::new(Some("a=1\n"));
        assert!(push_to_group(proj.path(), "1.21.4").is_err());
    }

    #[test]
    fn enable_imports_local_when_group_empty() {
        with_test_root(|root| {
            let proj = TempProject::new(Some("gamma=3.0\nfov=1.2\n"));
            let imported = enable_management(proj.path(), "1.20.1").unwrap();
            assert!(imported);
            let template =
                fs::read_to_string(root.join("mc-1.20").join("options.txt")).unwrap();
            assert_eq!(template, "gamma=3.0\nfov=1.2\n");
        });
    }
}
