//! Filesystem content packs (resourcepacks / shaderpacks).
//!
//! Lists zip files and folders in an instance content directory and
//! supports Prism-style enable/disable via renaming to `*.disabled`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentPackEntry {
    pub name: String,
    pub file_name: String,
    pub enabled: bool,
    pub kind: String,
    pub size: u64,
    pub size_formatted: String,
}

fn format_size(size: u64) -> String {
    if size < 1024 {
        format!("{size} B")
    } else if size < 1048576 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else if size < 1073741824 {
        format!("{:.1} MB", size as f64 / 1048576.0)
    } else {
        format!("{:.1} GB", size as f64 / 1073741824.0)
    }
}

fn entry_size(path: &Path) -> u64 {
    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(rd) = fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Ok(m) = p.metadata() {
                    total = total.saturating_add(m.len());
                }
            }
        }
    }
    total
}

/// Lists packs in `project_dir/{folder}` (`resourcepacks` or `shaderpacks`).
pub fn list_content_packs(
    project_dir: &Path,
    folder: &str,
) -> Result<Vec<ContentPackEntry>, String> {
    let dir = project_dir.join(folder);
    if !dir.is_dir() {
        return Ok(vec![]);
    }

    let mut out = Vec::new();
    for e in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let path = e.path();
        let file_name = e.file_name().to_string_lossy().to_string();
        if file_name.starts_with('.') {
            continue;
        }

        let lower = file_name.to_lowercase();
        let enabled = !lower.ends_with(".disabled");
        let base_name = if enabled {
            file_name.clone()
        } else {
            file_name
                .trim_end_matches(".disabled")
                .trim_end_matches(".DISABLED")
                .to_string()
        };

        let kind = if path.is_dir() {
            "folder".to_string()
        } else if base_name.to_lowercase().ends_with(".zip") {
            "zip".to_string()
        } else {
            // Skip loose non-zip files (e.g. readmes).
            continue;
        };

        let display = base_name
            .trim_end_matches(".zip")
            .trim_end_matches(".ZIP")
            .to_string();
        let size = entry_size(&path);
        out.push(ContentPackEntry {
            name: display,
            file_name,
            enabled,
            kind,
            size,
            size_formatted: format_size(size),
        });
    }

    out.sort_by(|a, b| {
        b.enabled
            .cmp(&a.enabled)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

/// Enable or disable a pack by renaming `file.zip` ↔ `file.zip.disabled`.
pub fn set_content_pack_enabled(
    project_dir: &Path,
    folder: &str,
    file_name: &str,
    enabled: bool,
) -> Result<ContentPackEntry, String> {
    if file_name.contains("..") || file_name.contains('/') || file_name.contains('\\') {
        return Err("invalid file name".into());
    }
    let dir = project_dir.join(folder);
    let src = dir.join(file_name);
    if !src.exists() {
        return Err(format!("pack not found: {file_name}"));
    }

    let lower = file_name.to_lowercase();
    let currently_enabled = !lower.ends_with(".disabled");
    if currently_enabled == enabled {
        // Already in desired state — return current entry.
        let packs = list_content_packs(project_dir, folder)?;
        return packs
            .into_iter()
            .find(|p| p.file_name == file_name)
            .ok_or_else(|| "pack not found after check".to_string());
    }

    let dest_name = if enabled {
        file_name
            .strip_suffix(".disabled")
            .or_else(|| file_name.strip_suffix(".DISABLED"))
            .unwrap_or(file_name)
            .to_string()
    } else {
        format!("{file_name}.disabled")
    };
    let dest = dir.join(&dest_name);
    if dest.exists() {
        return Err(format!("target already exists: {dest_name}"));
    }
    fs::rename(&src, &dest).map_err(|e| e.to_string())?;

    let packs = list_content_packs(project_dir, folder)?;
    packs
        .into_iter()
        .find(|p| p.file_name == dest_name)
        .ok_or_else(|| "pack missing after rename".to_string())
}

pub fn open_content_pack_folder(project_dir: &Path, folder: &str) -> PathBuf {
    project_dir.join(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn lists_and_toggles_zip_pack() {
        let dir = std::env::temp_dir().join(format!("tuffbox_packs_{}", std::process::id()));
        let rp = dir.join("resourcepacks");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&rp).unwrap();
        let zip = rp.join("CoolPack.zip");
        fs::File::create(&zip).unwrap().write_all(b"PK").unwrap();

        let listed = list_content_packs(&dir, "resourcepacks").unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].enabled);
        assert_eq!(listed[0].name, "CoolPack");

        let disabled =
            set_content_pack_enabled(&dir, "resourcepacks", "CoolPack.zip", false).unwrap();
        assert!(!disabled.enabled);
        assert!(disabled.file_name.ends_with(".disabled"));

        let enabled =
            set_content_pack_enabled(&dir, "resourcepacks", &disabled.file_name, true).unwrap();
        assert!(enabled.enabled);

        let _ = fs::remove_dir_all(&dir);
    }
}
