//! Desktop-side pack diff sources: manifests, snapshots, backup zips.
//!
//! Turns a tagged source payload into a normalized [`PackState`] from
//! `tuffbox_core::pack_diff`, then compares two states and attaches
//! unified text diffs for changed config files.

use std::io::Read;
use std::path::Path;

use serde::Deserialize;

use tuffbox_core::pack_diff::{pack_state_from_parts, PackState};

/// UI-facing source selector for `compare_pack_states`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSourcePayload {
    /// `manifest` | `snapshot` | `backup`
    #[serde(rename = "type")]
    pub kind: String,
    /// Manifest file path (kind = `manifest`).
    #[serde(default)]
    pub path: Option<String>,
    /// Project directory (kinds `snapshot` / `backup`).
    #[serde(default)]
    pub project_dir: Option<String>,
    /// Snapshot id (kind = `snapshot`).
    #[serde(default)]
    pub snapshot_id: Option<String>,
    /// Backup zip id (kind = `backup`).
    #[serde(default)]
    pub backup_id: Option<String>,
}

const MAX_CONFIG_BYTES: u64 = 512 * 1024;
/// Upper bound on inline config diffs returned per comparison.
const MAX_INLINE_CONFIG_DIFFS: usize = 24;

/// Walk a directory and collect editable config files, keyed relative to `base`.
fn collect_config_dir(base: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_config_dir(base, &p, out);
        } else if p.is_file() {
            let rel = p
                .strip_prefix(base)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            if !tuffbox_core::pack_diff::is_diffable_config(&rel) {
                continue;
            }
            if let Ok(content) = read_bounded_text(&p) {
                out.push((rel, content));
            }
        }
    }
}

/// Read a small UTF-8 text file; empty string for oversized/binary files
/// (matches the snapshot file-diff guard).
fn read_bounded_text(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_CONFIG_BYTES {
        return Ok(String::new());
    }
    std::fs::read_to_string(path)
        .map_err(|_| "# Binary or non-UTF8 file; inline diff unavailable.\n".to_string())
}

/// Extract editable config files from a zip archive in-memory (read-only).
/// (Kept inline in `load_state` — needs `&mut archive` with entry borrows.)

fn load_state_from_manifest_path(path: &Path) -> Result<PackState, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("read manifest: {e}"))?;
    pack_state_from_parts(&text, Vec::<(String, String)>::new())
}

fn load_state(source: &PackSourcePayload) -> Result<PackState, String> {
    match source.kind.as_str() {
        "manifest" => {
            let path = source.path.as_deref().filter(|s| !s.is_empty()).ok_or_else(|| "manifest source requires path".to_string())?;
            load_state_from_manifest_path(Path::new(path))
        }
        "snapshot" => {
            let project_dir = source
                .project_dir
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "snapshot source requires projectDir".to_string())?;
            let snapshot_id = source
                .snapshot_id
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "snapshot source requires snapshotId".to_string())?;
            if !snapshot_id
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            {
                return Err("invalid snapshot id".into());
            }
            // SnapshotStore::create stores the manifest at
            // <project>/.tuffbox/snapshots/<id>/manifest.json and changed
            // files under changed_files/<relative> (see snapshot.rs:156,179).
            let snap_dir = Path::new(project_dir)
                .join(".tuffbox")
                .join("snapshots")
                .join(snapshot_id);
            let text = std::fs::read_to_string(snap_dir.join("manifest.json"))
                .map_err(|e| format!("snapshot manifest: {e}"))?;
            // Snapshot stores only files that changed at capture time; merge
            // them with the CURRENT project configs so edits/removals since
            // the snapshot are visible in the diff.
            let mut config_files = Vec::new();
            collect_config_dir(&snap_dir.join("changed_files"), &snap_dir.join("changed_files"), &mut config_files);
            let dir = Path::new(project_dir);
            collect_config_dir(dir, &dir.join("config"), &mut config_files);
            pack_state_from_parts(&text, config_files)
        }
        "backup" => {
            let project_dir = source
                .project_dir
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "backup source requires projectDir".to_string())?;
            let backup_id = source
                .backup_id
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "backup source requires backupId".to_string())?;
            if !backup_id
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            {
                return Err("invalid backup id".into());
            }
            let zip_path =
                crate::helpers::backup_dir(Path::new(project_dir)).join(format!("{backup_id}.zip"));
            let file = std::fs::File::open(&zip_path).map_err(|e| format!("open backup: {e}"))?;
            let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
            let manifest_text = {
                let mut entry = archive
                    .by_name("project.tuffbox.json")
                    .map_err(|_| "backup contains no project.tuffbox.json".to_string())?;
                let mut s = String::new();
                entry
                    .read_to_string(&mut s)
                    .map_err(|e| format!("read backup manifest: {e}"))?;
                s
            };
            let mut config_files = Vec::new();
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
                if entry.is_dir() {
                    continue;
                }
                let name = entry.name().to_string();
                if name == "project.tuffbox.json"
                    || !tuffbox_core::pack_diff::is_diffable_config(&name)
                {
                    continue;
                }
                if entry.size() > MAX_CONFIG_BYTES {
                    continue;
                }
                let mut buf = String::new();
                if entry.read_to_string(&mut buf).is_ok() {
                    config_files.push((name, buf));
                }
            }
            pack_state_from_parts(&manifest_text, config_files)
        }
        _ => Err(format!("unknown pack source type: {}", source.kind)),
    }
}

/// Compare any two pack sources. Returns the diff report plus bounded
/// unified text diffs for changed config files.
#[tauri::command(rename_all = "camelCase")]
pub fn compare_pack_states(
    source_a: PackSourcePayload,
    source_b: PackSourcePayload,
) -> Result<serde_json::Value, String> {
    let sa = load_state(&source_a)?;
    let sb = load_state(&source_b)?;
    let report = tuffbox_core::pack_diff::diff_pack_states(&sa, &sb);

    let mut config_diffs = Vec::new();
    for path in report.changed_config_paths.iter().take(MAX_INLINE_CONFIG_DIFFS) {
        let text_a = sa
            .configs
            .get(path)
            .map(|f| f.content.clone())
            .unwrap_or_else(|| "(file absent)".into());
        let text_b = sb
            .configs
            .get(path)
            .map(|f| f.content.clone())
            .unwrap_or_else(|| "(file absent)".into());
        config_diffs.push(serde_json::json!({
            "path": path,
            "diffText": crate::helpers::unified_text_diff(&text_a, &text_b),
        }));
    }

    Ok(serde_json::json!({
        "report": serde_json::to_value(&report).map_err(|e| e.to_string())?,
        "configDiffs": config_diffs,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_payload_roundtrip() {
        let payload: PackSourcePayload =
            serde_json::from_str(r#"{"type":"manifest","path":"C:/x/p.tuffbox.json"}"#).unwrap();
        assert_eq!(payload.kind, "manifest");
        assert_eq!(payload.path.as_deref(), Some("C:/x/p.tuffbox.json"));
    }

    #[test]
    fn snapshot_payload_defaults() {
        let payload: PackSourcePayload =
            serde_json::from_str(r#"{"type":"snapshot","projectDir":"D:/proj","snapshotId":"snap-1"}"#)
                .unwrap();
        assert!(payload.path.is_none());
        assert_eq!(payload.snapshot_id.as_deref(), Some("snap-1"));
    }

    #[test]
    fn rejects_bad_backup_ids() {
        let payload = PackSourcePayload {
            kind: "backup".into(),
            path: None,
            project_dir: Some("D:/proj".into()),
            snapshot_id: None,
            backup_id: Some("../evil".into()),
        };
        assert!(load_state(&payload).is_err());
    }
}
