use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("snapshot directory not found: {0}")]
    NotFound(PathBuf),
    #[error("failed to read snapshot metadata: {0}")]
    ReadMetadata(#[source] std::io::Error),
    #[error("failed to parse snapshot metadata: {0}")]
    ParseMetadata(#[source] serde_json::Error),
    #[error("failed to copy file from {from} to {to}: {source}")]
    Copy {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("snapshot {id} not found")]
    SnapshotNotFound { id: String },
    #[error("failed to restore file {path}: {source}")]
    Restore {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotMeta {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub crash_fingerprint_key: Option<String>,
    #[serde(default)]
    pub report_id: Option<String>,
    /// `ai` | `kb` | `swarm` | `manual`
    #[serde(default)]
    pub plan_source: Option<String>,
    #[serde(default)]
    pub matched_case_ids: Vec<String>,
    /// Operation slug that this snapshot protects against (e.g. `add-mod`).
    #[serde(default)]
    pub operation: String,
    /// Human-readable one-liners describing planned/applied actions.
    #[serde(default)]
    pub actions_summary: Vec<String>,
    /// `launcher` | `ai` | `user` | `scan`
    #[serde(default)]
    pub actor: Option<String>,
    /// Relative managed files that existed at snapshot time. Rollback deletes
    /// extras under the same top-level folders (e.g. `mods/`).
    #[serde(default)]
    pub managed_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub reason: String,
    pub manifest_path: PathBuf,
    pub lockfile_path: Option<PathBuf>,
    pub changed_files: Vec<PathBuf>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub crash_fingerprint_key: Option<String>,
    #[serde(default)]
    pub report_id: Option<String>,
    #[serde(default)]
    pub plan_source: Option<String>,
    #[serde(default)]
    pub matched_case_ids: Vec<String>,
    #[serde(default)]
    pub operation: String,
    #[serde(default)]
    pub actions_summary: Vec<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub managed_files: Vec<PathBuf>,
    /// Total on-disk size (bytes) of the snapshot directory, including the
    /// copied manifest, lockfile and `changed_files`. 0 when unknown for
    /// snapshots written by older versions.
    #[serde(default)]
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDiff {
    pub added_files: Vec<PathBuf>,
    pub removed_files: Vec<PathBuf>,
    pub modified_files: Vec<PathBuf>,
}

pub struct SnapshotStore {
    project_dir: PathBuf,
    snapshots_dir: PathBuf,
}

impl SnapshotStore {
    pub fn new(project_dir: impl AsRef<Path>) -> Self {
        let project_dir = project_dir.as_ref().to_path_buf();
        let snapshots_dir = project_dir.join(".tuffbox").join("snapshots");
        Self {
            project_dir,
            snapshots_dir,
        }
    }

    pub fn ensure_snapshots_dir(&self) -> Result<(), SnapshotError> {
        fs::create_dir_all(&self.snapshots_dir).map_err(SnapshotError::ReadMetadata)
    }

    pub fn create(
        &self,
        name: impl Into<String>,
        reason: impl Into<String>,
        manifest_path: impl AsRef<Path>,
        lockfile_path: Option<impl AsRef<Path>>,
        changed_files: &[impl AsRef<Path>],
    ) -> Result<Snapshot, SnapshotError> {
        self.create_with_meta(
            name,
            reason,
            manifest_path,
            lockfile_path,
            changed_files,
            SnapshotMeta::default(),
        )
    }

    pub fn create_with_meta(
        &self,
        name: impl Into<String>,
        reason: impl Into<String>,
        manifest_path: impl AsRef<Path>,
        lockfile_path: Option<impl AsRef<Path>>,
        changed_files: &[impl AsRef<Path>],
        meta: SnapshotMeta,
    ) -> Result<Snapshot, SnapshotError> {
        self.ensure_snapshots_dir()?;

        let name: String = name.into();
        let reason: String = reason.into();
        let id = format!("{}-{}", slugify(&name), rfc3339_now_compact());
        let snapshot_dir = self.snapshots_dir.join(&id);
        fs::create_dir_all(&snapshot_dir).map_err(SnapshotError::ReadMetadata)?;

        let manifest_src = manifest_path.as_ref();
        let manifest_dst = snapshot_dir.join("manifest.json");
        copy_file(manifest_src, &manifest_dst)?;

        let lockfile_dst = if let Some(lockfile_path) = &lockfile_path {
            let src = lockfile_path.as_ref();
            let dst = snapshot_dir.join("lockfile.json");
            copy_file(src, &dst)?;
            Some(dst)
        } else {
            None
        };

        let mut files_to_copy: Vec<PathBuf> = changed_files
            .iter()
            .map(|p| p.as_ref().to_path_buf())
            .collect();
        for managed in &meta.managed_files {
            if !files_to_copy.iter().any(|p| p == managed) {
                files_to_copy.push(managed.clone());
            }
        }

        let mut copied_changed_files = Vec::new();
        let changed_files_dir = snapshot_dir.join("changed_files");
        for relative in &files_to_copy {
            let src = self.project_dir.join(relative);
            let dst = changed_files_dir.join(relative);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(SnapshotError::ReadMetadata)?;
            }
            copy_file(&src, &dst)?;
            copied_changed_files.push(relative.clone());
        }

        let snapshot = Snapshot {
            id,
            name,
            created_at: rfc3339_now(),
            reason,
            manifest_path: manifest_dst,
            lockfile_path: lockfile_dst,
            changed_files: copied_changed_files,
            tags: meta.tags,
            crash_fingerprint_key: meta.crash_fingerprint_key,
            report_id: meta.report_id,
            plan_source: meta.plan_source,
            matched_case_ids: meta.matched_case_ids,
            operation: meta.operation,
            actions_summary: meta.actions_summary,
            actor: meta.actor,
            managed_files: meta.managed_files,
            size_bytes: dir_size_bytes(&snapshot_dir),
        };

        let meta_path = snapshot_dir.join("snapshot.json");
        let meta_json =
            serde_json::to_string_pretty(&snapshot).expect("snapshot metadata should serialize");
        fs::write(&meta_path, meta_json).map_err(SnapshotError::ReadMetadata)?;

        Ok(snapshot)
    }

    /// Absolute path to a snapshot's on-disk directory.
    pub fn snapshot_dir(&self, id: impl AsRef<str>) -> PathBuf {
        self.snapshots_dir.join(id.as_ref())
    }

    /// Persist updated metadata fields back to `snapshot.json` (e.g. after
    /// learning the full action list for a bulk install).
    pub fn update_meta(&self, snapshot: &Snapshot) -> Result<(), SnapshotError> {
        let meta_path = self.snapshots_dir.join(&snapshot.id).join("snapshot.json");
        let meta_json =
            serde_json::to_string_pretty(snapshot).expect("snapshot metadata should serialize");
        fs::write(&meta_path, meta_json).map_err(SnapshotError::ReadMetadata)?;
        Ok(())
    }

    /// Delete a snapshot directory entirely.
    pub fn delete(&self, id: impl AsRef<str>) -> Result<(), SnapshotError> {
        let id = id.as_ref();
        let dir = self.snapshots_dir.join(id);
        if !dir.exists() {
            return Err(SnapshotError::SnapshotNotFound { id: id.to_string() });
        }
        fs::remove_dir_all(&dir).map_err(SnapshotError::ReadMetadata)?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<Snapshot>, SnapshotError> {
        if !self.snapshots_dir.exists() {
            return Ok(Vec::new());
        }

        let mut snapshots = Vec::new();
        for entry in fs::read_dir(&self.snapshots_dir).map_err(SnapshotError::ReadMetadata)? {
            let entry = entry.map_err(SnapshotError::ReadMetadata)?;
            let meta_path = entry.path().join("snapshot.json");
            if !meta_path.is_file() {
                continue;
            }
            let raw = fs::read_to_string(&meta_path).map_err(SnapshotError::ReadMetadata)?;
            let snapshot: Snapshot =
                serde_json::from_str(&raw).map_err(SnapshotError::ParseMetadata)?;
            snapshots.push(snapshot);
        }

        snapshots.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(snapshots)
    }

    pub fn get(&self, id: impl AsRef<str>) -> Result<Option<Snapshot>, SnapshotError> {
        let id = id.as_ref();
        let meta_path = self.snapshots_dir.join(id).join("snapshot.json");
        if !meta_path.is_file() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&meta_path).map_err(SnapshotError::ReadMetadata)?;
        let snapshot: Snapshot =
            serde_json::from_str(&raw).map_err(SnapshotError::ParseMetadata)?;
        Ok(Some(snapshot))
    }

    pub fn diff(
        &self,
        from_id: impl AsRef<str>,
        to_id: impl AsRef<str>,
    ) -> Result<SnapshotDiff, SnapshotError> {
        let from = self
            .get(&from_id)?
            .ok_or_else(|| SnapshotError::SnapshotNotFound {
                id: from_id.as_ref().to_string(),
            })?;
        let to = self
            .get(&to_id)?
            .ok_or_else(|| SnapshotError::SnapshotNotFound {
                id: to_id.as_ref().to_string(),
            })?;

        let from_files: std::collections::HashSet<_> = from.changed_files.iter().cloned().collect();
        let to_files: std::collections::HashSet<_> = to.changed_files.iter().cloned().collect();

        let from_changed_dir = self.snapshots_dir.join(&from.id).join("changed_files");
        let to_changed_dir = self.snapshots_dir.join(&to.id).join("changed_files");
        let mut modified_files = Vec::new();
        for relative in from_files.intersection(&to_files) {
            let from_path = from_changed_dir.join(relative);
            let to_path = to_changed_dir.join(relative);
            if files_differ(&from_path, &to_path).map_err(SnapshotError::ReadMetadata)? {
                modified_files.push(relative.clone());
            }
        }

        Ok(SnapshotDiff {
            added_files: to_files.difference(&from_files).cloned().collect(),
            removed_files: from_files.difference(&to_files).cloned().collect(),
            modified_files,
        })
    }

    /// Diff the stored contents of `id` against the current on-disk project
    /// state. Each of the snapshot's tracked relative files is compared
    /// against `project_dir/<relative>` by content; missing-on-disk -> added
    /// semantics is expressed through the calling layer, here the result
    /// reports whether the current file differs (or is now gone -> treated
    /// as a mismatch).
    pub fn diff_current(&self, id: impl AsRef<str>) -> Result<SnapshotDiff, SnapshotError> {
        let snapshot = self
            .get(&id)?
            .ok_or_else(|| SnapshotError::SnapshotNotFound {
                id: id.as_ref().to_string(),
            })?;

        let stored_dir = self.snapshots_dir.join(&snapshot.id).join("changed_files");
        let mut removed_files = Vec::new();
        let mut modified_files = Vec::new();

        // Files the snapshot stored but that now differ (or are missing) on disk.
        for relative in &snapshot.changed_files {
            let stored = stored_dir.join(relative);
            let current = self.project_dir.join(relative);
            if !current.is_file() {
                // Tracked in snapshot but gone from the live project -> the
                // "current" side is effectively removed.
                removed_files.push(relative.clone());
                continue;
            }
            if files_differ(&stored, &current).map_err(SnapshotError::ReadMetadata)? {
                modified_files.push(relative.clone());
            }
        }

        // Files that appeared since the checkpoint cannot be discovered
        // reliably without knowing the snapshot's tracked roots, so the
        // "added" set is intentionally empty; manifest-level additions are
        // reported separately through the manifest comparison.
        Ok(SnapshotDiff {
            added_files: Vec::new(),
            removed_files,
            modified_files,
        })
    }

    /// Backfill `size_bytes` for snapshots written before the field existed,
    /// persisting the updated metadata. Returns the number of snapshots whose
    /// metadata was rewritten.
    pub fn ensure_sizes(&self) -> Result<usize, SnapshotError> {
        let mut updated = 0;
        for mut snapshot in self.list()? {
            if snapshot.size_bytes == 0 {
                snapshot.size_bytes = dir_size_bytes(&self.snapshots_dir.join(&snapshot.id));
                self.update_meta(&snapshot)?;
                updated += 1;
            }
        }
        Ok(updated)
    }

    /// Delete `auto`-kind snapshots (created by the launcher / AI / scan) that
    /// are older than `older_than_days`, leaving user/`manual` and
    /// `crash_fix` snapshots untouched. Returns the ids that were removed and
    /// the total freed bytes.
    pub fn prune_auto_snapshots(
        &self,
        older_than_days: u64,
    ) -> Result<(Vec<String>, u64), SnapshotError> {
        use std::time::{Duration, SystemTime};
        let cutoff = SystemTime::now() - Duration::from_secs(older_than_days * 86_400);
        let mut removed = Vec::new();
        let mut freed = 0u64;
        for snapshot in self.list()? {
            let is_auto = snapshot.name.starts_with("auto-")
                || matches!(snapshot.actor.as_deref(), Some("launcher" | "ai" | "scan"));
            let is_crash = snapshot.tags.iter().any(|t| t == "crash_fix")
                || snapshot.name.contains("crash");
            let is_manual = snapshot.actor.as_deref() == Some("user") || snapshot.operation == "manual";
            if !is_auto || is_crash || is_manual {
                continue;
            }
            let created = crate::time_util::parse_rfc3339_unix_secs(&snapshot.created_at)
                .unwrap_or(u64::MAX);
            let created_time = SystemTime::UNIX_EPOCH + Duration::from_secs(created);
            if created_time < cutoff {
                freed += snapshot.size_bytes;
                self.delete(&snapshot.id)?;
                removed.push(snapshot.id.clone());
            }
        }
        Ok((removed, freed))
    }

    pub fn rollback(&self, id: impl AsRef<str>) -> Result<Snapshot, SnapshotError> {
        let snapshot = self
            .get(&id)?
            .ok_or_else(|| SnapshotError::SnapshotNotFound {
                id: id.as_ref().to_string(),
            })?;

        let manifest_dst = find_project_manifest(&self.project_dir)
            .unwrap_or_else(|| self.project_dir.join("project.tuffbox.json"));
        copy_file(&snapshot.manifest_path, &manifest_dst)?;

        if let Some(lockfile_path) = &snapshot.lockfile_path {
            let lockfile_dst = manifest_dst.with_extension("lock.json");
            copy_file(lockfile_path, &lockfile_dst)?;
        }

        let changed_files_dir = self.snapshots_dir.join(&snapshot.id).join("changed_files");
        for relative_path in &snapshot.changed_files {
            let src = changed_files_dir.join(relative_path);
            let dst = self.project_dir.join(relative_path);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|source| SnapshotError::Restore {
                    path: dst.clone(),
                    source,
                })?;
            }
            copy_file(&src, &dst)?;
        }

        if !snapshot.managed_files.is_empty() {
            remove_unmanaged_files(&self.project_dir, &snapshot.managed_files)?;
        }

        Ok(snapshot)
    }
}

fn find_project_manifest(project_dir: &Path) -> Option<PathBuf> {
    fs::read_dir(project_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.ends_with(".tuffbox.json"))
                .unwrap_or(false)
        })
}

fn remove_unmanaged_files(
    project_dir: &Path,
    managed_files: &[PathBuf],
) -> Result<(), SnapshotError> {
    let allowed: HashSet<String> = managed_files
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let mut roots = HashSet::new();
    for path in managed_files {
        if let Some(root) = path.components().next() {
            roots.insert(root.as_os_str().to_os_string());
        }
    }
    for root in roots {
        let root_path = project_dir.join(&root);
        if root_path.is_dir() {
            delete_unmanaged_tree(&root_path, project_dir, &allowed)?;
        } else if root_path.is_file() {
            let rel = root.to_string_lossy().replace('\\', "/");
            if !allowed.contains(&rel) {
                fs::remove_file(&root_path).map_err(|source| SnapshotError::Restore {
                    path: root_path,
                    source,
                })?;
            }
        }
    }
    Ok(())
}

fn delete_unmanaged_tree(
    dir: &Path,
    project_dir: &Path,
    allowed: &HashSet<String>,
) -> Result<(), SnapshotError> {
    for entry in fs::read_dir(dir).map_err(SnapshotError::ReadMetadata)? {
        let entry = entry.map_err(SnapshotError::ReadMetadata)?;
        let path = entry.path();
        if path.is_dir() {
            delete_unmanaged_tree(&path, project_dir, allowed)?;
            if fs::read_dir(&path)
                .map(|mut it| it.next().is_none())
                .unwrap_or(false)
            {
                let _ = fs::remove_dir(&path);
            }
        } else if path.is_file() {
            let rel = path
                .strip_prefix(project_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if !allowed.contains(&rel) {
                fs::remove_file(&path).map_err(|source| SnapshotError::Restore {
                    path: path.clone(),
                    source,
                })?;
            }
        }
    }
    Ok(())
}

fn files_differ(left: &Path, right: &Path) -> std::io::Result<bool> {
    if !left.exists() || !right.exists() {
        return Ok(left.exists() != right.exists());
    }
    let left_meta = fs::metadata(left)?;
    let right_meta = fs::metadata(right)?;
    if left_meta.len() != right_meta.len() {
        return Ok(true);
    }
    Ok(fs::read(left)? != fs::read(right)?)
}

fn copy_file(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<(), SnapshotError> {
    let from = from.as_ref();
    let to = to.as_ref();
    // copy_replacing, never fs::copy: restoring over an existing file must
    // not write into its inode — jars/zips can be dedup-store hardlinks,
    // and an in-place write would corrupt every other pack sharing them.
    crate::fs_util::copy_replacing(from, to).map_err(|source| SnapshotError::Copy {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// Sum of the `len()` of every file under `dir`, recursing into
/// subdirectories. Missing/unreadable entries are skipped; the walker never
/// fails for a store-level helper.
fn dir_size_bytes(dir: &Path) -> u64 {
    fn walk(dir: &Path, acc: &mut u64) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, acc);
            } else if let Ok(meta) = entry.metadata() {
                *acc += meta.len();
            }
        }
    }
    let mut total = 0;
    walk(dir, &mut total);
    total
}

fn slugify(name: &str) -> String {
    name.to_lowercase()
        .replace(|c: char| !c.is_alphanumeric(), "-")
        .replace("--", "-")
        .trim_matches('-')
        .to_string()
}

/// Returns the current UTC time as an RFC 3339 timestamp, e.g.
/// `2026-06-29T12:34:56Z`.
///
/// Previously this returned a hardcoded fake date (`2026-06-29T00:00:00Z`)
/// for *every* snapshot, which broke history ordering (snapshots sorted by
/// `created_at` all compared equal) and, combined with
/// [`rfc3339_now_compact`] feeding the same frozen timestamp into snapshot
/// IDs, meant two snapshots created with the same name on the same "day"
/// silently collided and overwrote each other on disk.
fn rfc3339_now() -> String {
    crate::time_util::rfc3339_now()
}

/// Returns the current UTC time formatted for use inside a snapshot ID
/// (`YYYYMMDDTHHMMSSZ`), safe to use as a path segment (no colons).
fn rfc3339_now_compact() -> String {
    crate::time_util::compact_now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_project() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().to_path_buf();
        let manifest_path = project_dir.join("project.tuffbox.json");
        let mut file = fs::File::create(&manifest_path).unwrap();
        writeln!(file, "{{\"schemaVersion\":\"0.1.0\"}}").unwrap();
        (dir, project_dir)
    }

    #[test]
    fn restore_replaces_hardlinks_without_corrupting_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.jar");
        let b = dir.path().join("b.jar");
        std::fs::write(&a, b"original shared bytes").unwrap();
        std::fs::hard_link(&a, &b).unwrap(); // simulate a dedup-store link pair

        // "Restore" a.jar from a snapshot with different content.
        let incoming = dir.path().join("incoming.jar");
        std::fs::write(&incoming, b"restored bytes").unwrap();
        copy_file(&incoming, &a).unwrap();

        // a.jar got the new content; b.jar — the same inode the OLD a.jar
        // pointed at — must still hold the original bytes. An in-place
        // fs::copy would have mutated both.
        assert_eq!(std::fs::read(&a).unwrap(), b"restored bytes");
        assert_eq!(std::fs::read(&b).unwrap(), b"original shared bytes");
    }

    #[test]
    fn creates_and_lists_snapshot() {
        let (_dir, project_dir) = temp_project();
        let store = SnapshotStore::new(&project_dir);
        let manifest_path = project_dir.join("project.tuffbox.json");

        let snapshot = store
            .create(
                "before-update",
                "manual",
                &manifest_path,
                None::<&Path>,
                &[] as &[&Path],
            )
            .unwrap();

        assert_eq!(snapshot.name, "before-update");
        let snapshots = store.list().unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].id, snapshot.id);
    }

    #[test]
    fn rolls_back_manifest() {
        let (_dir, project_dir) = temp_project();
        let store = SnapshotStore::new(&project_dir);
        let manifest_path = project_dir.join("project.tuffbox.json");

        let snapshot = store
            .create(
                "baseline",
                "manual",
                &manifest_path,
                None::<&Path>,
                &[] as &[&Path],
            )
            .unwrap();

        fs::write(&manifest_path, "{\"modified\":true}").unwrap();
        store.rollback(&snapshot.id).unwrap();

        let restored = fs::read_to_string(&manifest_path).unwrap();
        assert!(restored.contains("schemaVersion"));
    }

    #[test]
    fn snapshot_timestamps_are_not_hardcoded() {
        // Regression test: `rfc3339_now`/`rfc3339_now_compact` used to
        // return a frozen fake date, so every snapshot had an identical
        // `created_at` and snapshots with the same name could collide by
        // ID. Verify the timestamp actually reflects wall-clock time
        // instead of a magic constant.
        let now = rfc3339_now();
        assert_ne!(
            now, "2026-06-29T00:00:00Z",
            "timestamp looks hardcoded: {now}"
        );
        assert!(now.ends_with('Z'));
    }

    #[test]
    fn stores_actions_summary_and_deletes() {
        let (_dir, project_dir) = temp_project();
        let store = SnapshotStore::new(&project_dir);
        let manifest_path = project_dir.join("project.tuffbox.json");

        let snapshot = store
            .create_with_meta(
                "before-bulk",
                "test",
                &manifest_path,
                None::<&Path>,
                &[] as &[&Path],
                SnapshotMeta {
                    operation: "bulk-add".into(),
                    actions_summary: vec!["Install sodium".into(), "Install lithium".into()],
                    actor: Some("launcher".into()),
                    ..Default::default()
                },
            )
            .unwrap();

        let loaded = store.get(&snapshot.id).unwrap().unwrap();
        assert_eq!(loaded.operation, "bulk-add");
        assert_eq!(loaded.actions_summary.len(), 2);
        assert_eq!(loaded.actor.as_deref(), Some("launcher"));

        store.delete(&snapshot.id).unwrap();
        assert!(store.get(&snapshot.id).unwrap().is_none());
    }

    #[test]
    fn rollback_restores_lockfile_beside_custom_manifest_name() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().to_path_buf();
        let manifest_path = project_dir.join("foo.tuffbox.json");
        let lockfile_path = project_dir.join("foo.tuffbox.lock.json");
        fs::write(&manifest_path, "{\"schemaVersion\":\"0.1.0\"}").unwrap();
        fs::write(&lockfile_path, "{\"schemaVersion\":\"0.1.0\",\"mods\":[]}").unwrap();

        let store = SnapshotStore::new(&project_dir);
        let snapshot = store
            .create(
                "before-update",
                "manual",
                &manifest_path,
                Some(&lockfile_path),
                &[] as &[&Path],
            )
            .unwrap();

        fs::write(
            &lockfile_path,
            "{\"schemaVersion\":\"0.1.0\",\"mods\":[\"new\"]}",
        )
        .unwrap();
        store.rollback(&snapshot.id).unwrap();

        let restored = fs::read_to_string(&lockfile_path).unwrap();
        assert!(
            restored.contains("\"mods\":[]"),
            "lockfile should restore beside foo.tuffbox.json, got: {restored}"
        );
        assert!(
            !project_dir.join("project.tuffbox.lock.json").is_file(),
            "rollback must not write the hardcoded project.tuffbox.lock.json path"
        );
    }

    #[test]
    fn rollback_removes_files_added_after_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().to_path_buf();
        let manifest_path = project_dir.join("foo.tuffbox.json");
        fs::write(&manifest_path, "{\"schemaVersion\":\"0.1.0\"}").unwrap();
        fs::create_dir_all(project_dir.join("mods")).unwrap();
        fs::write(project_dir.join("mods/kept.jar"), b"old").unwrap();

        let store = SnapshotStore::new(&project_dir);
        let snapshot = store
            .create_with_meta(
                "before-pull",
                "github_pack_update",
                &manifest_path,
                None::<&Path>,
                &["mods/kept.jar" as &str],
                SnapshotMeta {
                    operation: "github_pack_update".into(),
                    managed_files: vec![PathBuf::from("mods/kept.jar")],
                    ..Default::default()
                },
            )
            .unwrap();

        fs::write(project_dir.join("mods/kept.jar"), b"mutated").unwrap();
        fs::write(project_dir.join("mods/added.jar"), b"new").unwrap();
        store.rollback(&snapshot.id).unwrap();

        assert_eq!(fs::read(project_dir.join("mods/kept.jar")).unwrap(), b"old");
        assert!(
            !project_dir.join("mods/added.jar").is_file(),
            "files added after the snapshot must be removed on rollback"
        );
    }
}
