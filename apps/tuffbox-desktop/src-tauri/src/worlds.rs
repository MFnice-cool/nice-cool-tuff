//! World saves management: enriched world listing (level.dat metadata),
//! zip backup/restore/delete for single worlds and backup inventory
//! with per-world retention.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use base64::Engine;

use crate::helpers::manifest_parent;

/// How many zip backups are kept per world; older ones are pruned.
const MAX_BACKUPS_PER_WORLD: usize = 10;
/// Refuse to base64-encode world icons larger than this (icon.png is ~KB).
const MAX_ICON_BYTES: u64 = 4 * 1024 * 1024;

// ── Path helpers ─────────────────────────────────────────────────

fn saves_dir(project_dir: &Path) -> PathBuf {
    project_dir.join("saves")
}

fn backups_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join("world-backups")
}

fn validate_world_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.starts_with('.')
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
    {
        return Err("invalid world name".into());
    }
    Ok(())
}

fn validate_backup_file(name: &str) -> Result<(), String> {
    let ok = name.ends_with(".zip")
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !ok {
        return Err("invalid backup file".into());
    }
    Ok(())
}

fn format_size(size: u64) -> String {
    if size < 1_048_576 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else if size < 1_073_741_824 {
        format!("{:.1} MB", size as f64 / 1_048_576.0)
    } else {
        format!("{:.1} GB", size as f64 / 1_073_741_824.0)
    }
}

fn dir_size(d: &Path) -> u64 {
    let mut size = 0;
    fn walk(d: &Path, s: &mut u64) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, s);
            } else if let Ok(m) = p.metadata() {
                *s += m.len();
            }
        }
    }
    walk(d, &mut size);
    size
}

// ── Zip helpers ──────────────────────────────────────────────────

/// Zips `world_dir` (must live under saves/) into a new timestamped
/// archive inside `.tuffbox/world-backups/`. Returns the zip path.
fn zip_world_dir(project_dir: &Path, world_dir: &Path) -> Result<PathBuf, String> {
    let world_name = world_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| "world path has no name".to_string())?;
    let backup_dir = backups_dir(project_dir);
    std::fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;
    let zip_name = format!(
        "{}-{}.zip",
        world_name,
        tuffbox_core::time_util::compact_now()
    );
    let zip_path = backup_dir.join(&zip_name);
    let out = std::fs::File::create(&zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(out);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    fn add_world(
        zip: &mut zip::ZipWriter<std::fs::File>,
        opts: zip::write::SimpleFileOptions,
        base: &Path,
        dir: &Path,
    ) -> Result<(), String> {
        for e in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            let p = e.path();
            if p.is_dir() {
                add_world(zip, opts, base, &p)?;
            } else if p.is_file() {
                let rel = p
                    .strip_prefix(base)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                zip.start_file(rel, opts).map_err(|e| e.to_string())?;
                zip.write_all(&std::fs::read(&p).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    let parent = world_dir
        .parent()
        .ok_or_else(|| "world path has no parent directory".to_string())?;
    add_world(&mut zip, opts, parent, world_dir)?;
    zip.finish().map_err(|e| e.to_string())?;
    Ok(zip_path)
}

/// Keeps only the newest `MAX_BACKUPS_PER_WORLD` archives for a world.
fn prune_world_backups(project_dir: &Path, world_name: &str) {
    let prefix = format!("{}-", world_name);
    let dir = backups_dir(project_dir);
    let mut entries: Vec<(PathBuf, SystemTime)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| {
                    let n = n.to_string_lossy();
                    n.starts_with(&prefix) && n.ends_with(".zip")
                })
                .unwrap_or(false)
        })
        .map(|p| {
            let modified = p
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (p, modified)
        })
        .collect();
    entries.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
    for (path, _) in entries.into_iter().skip(MAX_BACKUPS_PER_WORLD) {
        let _ = std::fs::remove_file(path);
    }
}

/// Extracts the world folder name from a backup archive name produced by
/// [`zip_world_dir`]: `{world}-YYYYMMDDTHHMMSSZ.zip`.
fn parse_backup_world_name(file_name: &str) -> Option<String> {
    let stem = file_name.strip_suffix(".zip")?;
    let (world, ts) = stem.rsplit_once('-')?;
    let valid_ts = ts.len() == 16
        && ts.ends_with('Z')
        && ts.as_bytes()[8] == b'T'
        && ts[..15].chars().all(|c| c.is_ascii_digit() || c == 'T');
    if valid_ts && !world.is_empty() {
        Some(world.to_string())
    } else {
        None
    }
}

// ── Tauri commands ───────────────────────────────────────────────

/// Lists Minecraft worlds in the project's saves/ folder, enriched with
/// level.dat metadata (display name, game mode, difficulty, last played).
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn list_worlds(path: String) -> Result<Vec<serde_json::Value>, String> {
    let project_dir = manifest_parent(&path)?;
    let saves = saves_dir(&project_dir);
    if !saves.is_dir() {
        return Ok(vec![]);
    }
    let mut worlds = Vec::new();
    for entry in std::fs::read_dir(&saves)
        .into_iter()
        .flatten()
        .flatten()
    {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let has_level = p.join("level.dat").is_file();
        let has_icon = p.join("icon.png").is_file();
        // read_world_info walks the tree for size itself, so use it as the
        // single source when level.dat parses; fall back to a manual walk.
        let info = if has_level {
            tuffbox_core::level_dat::read_world_info(&p).ok()
        } else {
            None
        };
        let (size, size_formatted) = match &info {
            Some(i) => (i.size_bytes, i.size_formatted.clone()),
            None => {
                let s = dir_size(&p);
                (s, format_size(s))
            }
        };
        worlds.push(serde_json::json!({
            "name": name,
            "size": size,
            "sizeFormatted": size_formatted,
            "hasLevelDat": has_level,
            "hasIcon": has_icon,
            "displayName": info.as_ref().map(|i| i.name.clone()),
            "gameType": info.as_ref().map(|i| i.game_type.clone()),
            "difficulty": info.as_ref().map(|i| i.difficulty.clone()),
            "hardcore": info.as_ref().map(|i| i.hardcore),
            "cheatsEnabled": info.as_ref().map(|i| i.cheats_enabled),
            "lastPlayed": info.as_ref().map(|i| i.last_played),
        }));
    }
    // Most recently played first; worlds without level.dat (lastPlayed = 0)
    // sink to the bottom, ties broken by size.
    worlds.sort_by_key(|w| {
        let lp = w["lastPlayed"].as_u64().unwrap_or(0);
        let size = w["size"].as_u64().unwrap_or(0);
        (std::cmp::Reverse(lp), std::cmp::Reverse(size))
    });
    Ok(worlds)
}

/// Backs up a single world as a zip archive into `.tuffbox/world-backups/`,
/// then prunes old archives beyond the per-world retention limit.
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn backup_world(path: String, world_name: String) -> Result<String, String> {
    validate_world_name(&world_name)?;
    let project_dir = manifest_parent(&path)?;
    let world_dir = saves_dir(&project_dir).join(&world_name);
    if !world_dir.is_dir() {
        return Err("world not found".into());
    }
    let zip_path = zip_world_dir(&project_dir, &world_dir)?;
    prune_world_backups(&project_dir, &world_name);
    Ok(zip_path.to_string_lossy().to_string())
}

/// Restores a world from a zip backup. If the target world already exists,
/// `overwrite` must be true and the current world is safety-backed up first.
/// Returns the restored world's folder name.
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn restore_world_backup(
    path: String,
    backup_file: String,
    overwrite: Option<bool>,
) -> Result<String, String> {
    validate_backup_file(&backup_file)?;
    let project_dir = manifest_parent(&path)?;
    let zip_path = backups_dir(&project_dir).join(&backup_file);
    if !zip_path.is_file() {
        return Err("backup not found".into());
    }

    let file = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    // All entries must live under a single top-level world folder.
    let mut world_name: Option<String> = None;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().replace('\\', "/");
        let mut parts = name.split('/');
        let top = parts.next().unwrap_or_default();
        if top.is_empty() {
            continue;
        }
        if parts.any(|c| c == "..") {
            return Err(format!("zip entry escapes world directory: {name}"));
        }
        match &world_name {
            None => world_name = Some(top.to_string()),
            Some(w) if w != top => {
                return Err("backup contains multiple top-level folders".into());
            }
            _ => {}
        }
    }
    let world_name = world_name.ok_or_else(|| "backup archive is empty".to_string())?;
    validate_world_name(&world_name)?;

    let saves = saves_dir(&project_dir);
    let target = saves.join(&world_name);
    if target.exists() {
        if overwrite != Some(true) {
            return Err(format!("world '{world_name}' already exists"));
        }
        // Safety: back up the current world before replacing it.
        zip_world_dir(&project_dir, &target)?;
        prune_world_backups(&project_dir, &world_name);
        std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
    }

    let canonical_saves = std::fs::canonicalize(
        saves
            .is_dir()
            .then(|| saves.clone())
            .ok_or_else(|| "saves directory not found".to_string())?,
    )
    .map_err(|e| e.to_string())?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().replace('\\', "/");
        if name.ends_with('/') {
            continue;
        }
        let target_path = saves.join(&name);
        let canonical = std::fs::canonicalize(&target_path)
            .or_else(|_| {
                std::fs::canonicalize(target_path.parent().unwrap_or(&saves))
            })
            .map_err(|e| e.to_string())?;
        if !canonical.starts_with(&canonical_saves) {
            return Err(format!("zip entry escapes saves directory: {name}"));
        }
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut dest = std::fs::File::create(&target_path).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut dest).map_err(|e| e.to_string())?;
    }

    Ok(world_name)
}

/// Deletes a world folder, optionally creating a zip backup first.
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn delete_world(
    path: String,
    world_name: String,
    backup_first: Option<bool>,
) -> Result<(), String> {
    validate_world_name(&world_name)?;
    let project_dir = manifest_parent(&path)?;
    let world_dir = saves_dir(&project_dir).join(&world_name);
    if !world_dir.is_dir() {
        return Err("world not found".into());
    }
    if backup_first == Some(true) {
        zip_world_dir(&project_dir, &world_dir)?;
        prune_world_backups(&project_dir, &world_name);
    }
    std::fs::remove_dir_all(&world_dir).map_err(|e| e.to_string())
}

/// Lists world backup archives under `.tuffbox/world-backups/`.
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn list_world_backups(path: String) -> Result<Vec<serde_json::Value>, String> {
    let project_dir = manifest_parent(&path)?;
    let dir = backups_dir(&project_dir);
    if !dir.is_dir() {
        return Ok(vec![]);
    }
    let mut backups = Vec::new();
    for entry in std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let file = entry.file_name().to_string_lossy().to_string();
        if !file.ends_with(".zip") {
            continue;
        }
        let meta = p.metadata().map_err(|e| e.to_string())?;
        let size = meta.len();
        let created_at = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        backups.push(serde_json::json!({
            "file": file,
            "worldName": parse_backup_world_name(&file),
            "size": size,
            "sizeFormatted": format_size(size),
            "createdAt": created_at,
        }));
    }
    backups.sort_by_key(|b| std::cmp::Reverse(b["createdAt"].as_u64().unwrap_or(0)));
    Ok(backups)
}

/// Deletes a single world backup archive.
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn delete_world_backup(path: String, backup_file: String) -> Result<(), String> {
    validate_backup_file(&backup_file)?;
    let project_dir = manifest_parent(&path)?;
    let zip_path = backups_dir(&project_dir).join(&backup_file);
    if !zip_path.is_file() {
        return Err("backup not found".into());
    }
    std::fs::remove_file(&zip_path).map_err(|e| e.to_string())
}

/// Reads a world's `icon.png` as a base64 data URL, or null if missing.
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn read_world_icon(
    path: String,
    world_name: String,
) -> Result<Option<String>, String> {
    validate_world_name(&world_name)?;
    let project_dir = manifest_parent(&path)?;
    let icon_path = saves_dir(&project_dir).join(&world_name).join("icon.png");
    if !icon_path.is_file() {
        return Ok(None);
    }
    let meta = icon_path.metadata().map_err(|e| e.to_string())?;
    if meta.len() > MAX_ICON_BYTES {
        return Ok(None);
    }
    let bytes = std::fs::read(&icon_path).map_err(|e| e.to_string())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(Some(format!("data:image/png;base64,{encoded}")))
}

// ── Screenshots ──────────────────────────────────────────────────

fn screenshots_dir(project_dir: &Path) -> PathBuf {
    project_dir.join("screenshots")
}

fn validate_screenshot_file(name: &str) -> Result<(), String> {
    let ok = (name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".jpeg"))
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ' | '(' | ')'));
    if !ok {
        return Err("invalid screenshot file name".into());
    }
    Ok(())
}

/// Lists the instance's screenshots (newest first) for the manager grid.
/// Names are returned with absolute paths so the frontend can render them
/// through the asset protocol (convertFileSrc) without re-reading bytes.
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn list_screenshots(path: String) -> Result<Vec<serde_json::Value>, String> {
    let project_dir = manifest_parent(&path)?;
    let dir = screenshots_dir(&project_dir);
    if !dir.is_dir() {
        return Ok(vec![]);
    }
    let mut shots = Vec::new();
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        let lower = file_name.to_lowercase();
        if !(lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg")) {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let size_bytes = meta.len();
        shots.push(serde_json::json!({
            "fileName": file_name,
            "path": p.to_string_lossy(),
            "sizeBytes": size_bytes,
            "sizeFormatted": format_size(size_bytes),
            "modifiedMs": modified_ms,
        }));
    }
    shots.sort_by_key(|s| std::cmp::Reverse(s["modifiedMs"].as_u64().unwrap_or(0)));
    Ok(shots)
}

/// Deletes one screenshot from the instance's screenshots folder.
/// The file name is validated (image extension, no separators) so the
/// join cannot escape the screenshots directory.
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn delete_screenshot(path: String, file_name: String) -> Result<(), String> {
    validate_screenshot_file(&file_name)?;
    let project_dir = manifest_parent(&path)?;
    let target = screenshots_dir(&project_dir).join(&file_name);
    if !target.is_file() {
        return Err("screenshot not found".into());
    }
    std::fs::remove_file(&target).map_err(|e| e.to_string())
}
